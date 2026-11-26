//! Pure state machine and owner-only persistence for Spaceman's four-step setup tour.

use std::{fs, path::Path};

use spaceman::state_file::write_atomic_owner_only;

use crate::controls::PermissionControl;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Step {
    Welcome,
    /// Accessibility, and nothing else.
    Setup,
    /// The suggested arrangement of the open windows; its primary button starts tiling.
    Arrange,
    /// A few real shortcuts on the real workspaces.
    TryIt,
}

impl Step {
    pub const TOUR: [Self; 4] = [Self::Welcome, Self::Setup, Self::Arrange, Self::TryIt];

    pub fn position(self) -> usize {
        Self::TOUR
            .iter()
            .position(|step| *step == self)
            .unwrap_or(0)
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::Welcome => "welcome",
            Self::Setup => "setup",
            Self::Arrange => "arrange",
            Self::TryIt => "try",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SetupState {
    pub accessibility: bool,
    pub launch_at_login: bool,
    /// Automatic tiling is on.
    pub tiling: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Input {
    Continue,
    Back,
    GrantAccessibility,
    ToggleLaunchAtLogin,
    /// Arrange's Skip: leave the windows where they are.
    Skip,
    /// Try it's Undo & restore: pause, and put every window back.
    Undo,
    Status(SetupState),
    Dismiss,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Effect {
    None,
    OpenAccessibility,
    ToggleLaunchAtLogin,
    /// Accessibility just turned on: hold for a beat so the checkmark settles, then advance.
    AdvanceSoon,
    /// Apply the plan and start tiling.
    Arrange,
    /// Pause tiling and restore every window.
    Undo,
    Finish,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct View {
    pub step: Step,
    /// `1` when moving forward, `-1` when going back, `0` on initial paint.
    pub direction: i8,
    /// Position among the dotted steps.
    pub index: usize,
    pub count: usize,
    pub accessibility: PermissionControl,
    pub launch_at_login: bool,
    pub ready: bool,
    pub tiling: bool,
    pub can_go_back: bool,
    pub manual: bool,
}

#[derive(Clone, Debug)]
pub struct Flow {
    step: Step,
    direction: i8,
    status: SetupState,
    asked_accessibility: bool,
    advance_due: bool,
    manual: bool,
}

impl Flow {
    pub fn new(start: Step, status: SetupState, manual: bool) -> Self {
        let step = if !manual && start == Step::Setup && status.accessibility {
            // Resuming after macOS Quit & Reopen once Accessibility is granted lands on the
            // arrangement, which is what starts tiling.
            Step::Arrange
        } else if start.position() > Step::Setup.position() && !status.accessibility {
            Step::Setup
        } else {
            start
        };
        Self {
            step,
            direction: 0,
            status,
            asked_accessibility: false,
            advance_due: false,
            manual,
        }
    }

    pub fn step(&self) -> Step {
        self.step
    }

    pub fn manual(&self) -> bool {
        self.manual
    }

    pub fn view(&self) -> View {
        let accessibility = if self.status.accessibility {
            PermissionControl::Allowed {
                animated: self.asked_accessibility,
            }
        } else if self.asked_accessibility {
            PermissionControl::Waiting
        } else {
            PermissionControl::Allow
        };
        View {
            step: self.step,
            direction: self.direction,
            index: self.step.position(),
            count: Step::TOUR.len(),
            accessibility,
            launch_at_login: self.status.launch_at_login,
            ready: self.status.accessibility,
            tiling: self.status.tiling,
            // Try it goes back with Undo & restore, which also puts the windows back.
            can_go_back: matches!(self.step, Step::Setup | Step::Arrange),
            manual: self.manual,
        }
    }

    fn go(&mut self, step: Step) {
        self.direction = if step.position() > self.step.position() {
            1
        } else {
            -1
        };
        self.advance_due = false;
        self.step = step;
    }

    pub fn on(&mut self, input: Input) -> Effect {
        match input {
            Input::Continue => match self.step {
                Step::Welcome => {
                    self.go(Step::Setup);
                    Effect::None
                }
                Step::Setup if !self.status.accessibility => Effect::None,
                Step::Setup => {
                    self.go(Step::Arrange);
                    Effect::None
                }
                Step::Arrange => {
                    self.go(Step::TryIt);
                    Effect::Arrange
                }
                Step::TryIt => Effect::Finish,
            },
            Input::Back => {
                if self.view().can_go_back {
                    self.go(Step::TOUR[self.step.position() - 1]);
                }
                Effect::None
            }
            Input::GrantAccessibility => {
                if self.step == Step::Setup && !self.status.accessibility {
                    self.asked_accessibility = true;
                    Effect::OpenAccessibility
                } else {
                    Effect::None
                }
            }
            // Launch at login is Try it's: the permission step asks for one thing.
            Input::ToggleLaunchAtLogin if self.step == Step::TryIt => Effect::ToggleLaunchAtLogin,
            Input::ToggleLaunchAtLogin => Effect::None,
            Input::Skip if self.step != Step::Arrange => Effect::None,
            // With tiling already on there is still something to try; otherwise the windows
            // stay where they are and the tour is over.
            Input::Skip if self.status.tiling => {
                self.go(Step::TryIt);
                Effect::None
            }
            Input::Skip => Effect::Finish,
            Input::Undo if self.step == Step::TryIt => {
                self.go(Step::Arrange);
                Effect::Undo
            }
            Input::Undo => Effect::None,
            Input::Status(next) => {
                let was_ready = self.status.accessibility;
                self.status = next;
                if self.step == Step::Setup && !was_ready && next.accessibility {
                    self.advance_due = true;
                }
                // Only the permission step moves on by itself. Try it never does.
                if std::mem::take(&mut self.advance_due) && self.step == Step::Setup {
                    Effect::AdvanceSoon
                } else {
                    Effect::None
                }
            }
            Input::Dismiss => Effect::Finish,
        }
    }
}

pub fn launch_step(accessibility_trusted: bool) -> Option<Step> {
    let support = spaceman::config::application_support_path("onboarding-complete")?;
    let directory = support.parent()?;
    launch_step_at(directory, accessibility_trusted)
}

pub fn record_step(step: Step) {
    if let Some(path) = spaceman::config::application_support_path("onboarding-step")
        && let Some(directory) = path.parent()
    {
        drop(record_step_at(directory, step));
    }
}

pub fn record_completion() {
    if let Some(path) = spaceman::config::application_support_path("onboarding-complete")
        && let Some(directory) = path.parent()
    {
        drop(record_completion_at(directory));
    }
}

/// Decides whether a launch opens into the setup tour, and at which step.
/// Existing trusted installs with no in-progress tour record completion silently
/// so unattended restarts stay windowless.
pub(crate) fn launch_step_at(directory: &Path, accessibility_trusted: bool) -> Option<Step> {
    if completion_recorded_at(directory) {
        return None;
    }
    let saved = saved_step_at(directory);
    if !accessibility_trusted || saved.is_some() {
        return Some(saved.unwrap_or(Step::Welcome));
    }
    drop(record_completion_at(directory));
    None
}

pub(crate) fn completion_recorded_at(directory: &Path) -> bool {
    directory.join("onboarding-complete").is_file()
}

pub(crate) fn saved_step_at(directory: &Path) -> Option<Step> {
    match fs::read_to_string(directory.join("onboarding-step"))
        .ok()?
        .trim()
    {
        "setup" => Some(Step::Setup),
        // `rules` (removed in 0.3.2) and `done` (0.3.4's last step) were both past the
        // Accessibility grant with tiling still held: they resume at the arrangement.
        "rules" | "done" | "arrange" | "try" => Some(Step::Arrange),
        _ => None,
    }
}

pub(crate) fn record_step_at(directory: &Path, step: Step) -> Result<(), String> {
    write_atomic_owner_only(&directory.join("onboarding-step"), step.key().as_bytes())
        .map_err(|error| error.to_string())
}

pub(crate) fn record_completion_at(directory: &Path) -> Result<(), String> {
    write_atomic_owner_only(&directory.join("onboarding-complete"), b"1\n")
        .map_err(|error| error.to_string())?;
    drop(fs::remove_file(directory.join("onboarding-step")));
    Ok(())
}
