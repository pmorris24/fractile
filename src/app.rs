use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, Sender, TryRecvError, channel},
    },
    thread,
    time::{Duration, Instant},
};

use crate::{
    AppInstanceId, BindingState, Decision, Direction, DisplaySelector, DisplayTarget,
    NavigationTile, ResizeDirection, Settings, WorkspaceId, Workspaces,
    config::Config,
    control::{self, Command as ControlCommand, Request as ControlRequest},
    deep_link::OpenTarget,
    hotkeys::{DeliveryWatch, HotkeyPlan, Hotkeys},
    live, login_item, macos, permission_guide, status_item,
    swipe::{Fingers, Swipe},
    termination, url_handler,
};

/// Timestamped diagnostic line on stderr (redirected to `agent.log` in bundle
/// launches). Never include window titles or other user content.
///
/// Unlike `eprintln!`, a failed write (such as a full disk) is ignored rather
/// than panicking the calling thread, and each line is written in one call.
macro_rules! log_event {
    ($($arg:tt)*) => {{
        let now = $crate::config::now();
        let line = format!(
            "[{}.{:03}] {}\n",
            now.as_secs(),
            now.subsec_millis(),
            format_args!($($arg)*)
        );
        // Writing to stderr itself has no fallback sink when the pipe or disk is full.
        drop(std::io::Write::write_all(&mut std::io::stderr().lock(), line.as_bytes()));
    }};
}

mod app_loop;
mod arrange;
mod backend;
mod bridge;
mod commands;
mod cycle;
mod detach_tab;
mod displays;
mod focus;
mod hud_event;
mod input;
mod journal;
mod memory;
mod minimums;
mod move_workspace;
mod parking;
mod pins;
mod runtime;
mod scratchpad;
mod space_transition;
mod stale_parked;
mod state_writer;
mod switching;
mod worker;

pub use app_loop::{MainThreadAgent, TickOutcome};
pub use bridge::{
    HUD_MAX_DISPLAYS, HUD_MAX_FLOATING, HUD_MAX_TILES, HUD_PORTRAIT_PREVIEW_RECT, HUD_PREVIEW_RECT,
    UiAction, UiAgentSnapshot, UiBridge, UiHudDisplay, UiHudEvent, UiHudKind, UiHudLayout,
    UiHudTile, UiHudToast, UiHudWorkspace, UiLaunchAtLogin, UiNews, UiUpdateStatus,
    UiWorkspaceWindows, hud_preview_rect,
};

pub struct HostCallbacks {
    pub open_settings: Box<dyn FnMut() -> Result<(), String>>,
    pub open_target: Box<dyn FnMut(OpenTarget) -> Result<(), String>>,
    pub open_setup: Box<dyn FnMut() -> Result<(), String>>,
    pub save_hud: Box<dyn FnMut(bool) -> Result<(), String>>,
}

use app_loop::AppLoopInit;
use backend::*;
use input::{GestureTracker, InputQueue, QueuedInput};
use journal::JournalState;
use memory::MemoryState;
use parking::{ParkRecord, ParkTable};
use runtime::*;
use worker::spawn_worker;

pub const TICK_INTERVAL: Duration = Duration::from_millis(25);
/// Prefix on `UiAgentSnapshot::last_error` when installing a staged update fails.
pub const UPDATE_FAILED_PREFIX: &str = "Update failed: ";
/// Retile without an event this often while the user is active...
const FALLBACK_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
/// ...and this often once nothing has happened for `RECENT_ACTIVITY`. Each
/// retile sends Accessibility queries to every app, so idle ones stay rare.
const IDLE_FALLBACK_REFRESH_INTERVAL: Duration = Duration::from_secs(5);
const RECENT_ACTIVITY: Duration = Duration::from_secs(5);
/// `NSWorkspace` reports activations as they happen; this re-check only
/// covers a missed notification (lock screen, notification centre hiccups).
const FRONTMOST_FALLBACK_INTERVAL: Duration = Duration::from_secs(2);
const APPLICATION_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const ACCESSIBILITY_POLL_INTERVAL: Duration = Duration::from_millis(120);
const RESTORE_RETRY_INTERVAL: Duration = Duration::from_secs(1);
const APPLICATION_ACTIVATION_SETTLE: Duration = Duration::from_millis(50);
/// How long after macOS changes the active Space background cycles hold
/// layout: the desktop and fullscreen slides finish well within it.
const SPACE_TRANSITION_SETTLE: Duration = Duration::from_millis(700);

// AX creation and app activation can both precede the first on-screen frame.
const WINDOW_OPENING_SETTLE: Duration = Duration::from_millis(500);
const WINDOW_OPENING_RECHECK: Duration = Duration::from_millis(40);
/// A cancelled cycle may have been a focus follow (Cmd+Tab, Dock, a
/// notification) interrupted by the activation it answers. Accessibility can
/// still report the old focus on the cycle right after, and no further event
/// arrives once it catches up, so retile at the opening cadence for this long.
const CANCELLED_CYCLE_RETRY: Duration = Duration::from_millis(500);

fn opening_recheck(
    now: Instant,
    until: Option<Instant>,
    frontmost: Option<&AppInstanceId>,
    missing: Option<&AppInstanceId>,
) -> Option<Instant> {
    let deadline = until?;
    let next = now + WINDOW_OPENING_RECHECK;
    (frontmost? == missing? && next <= deadline).then_some(next)
}

/// The next retile while a cancelled cycle's retry window is open.
fn cancelled_cycle_recheck(now: Instant, until: Option<Instant>) -> Option<Instant> {
    let next = now + WINDOW_OPENING_RECHECK;
    (next <= until?).then_some(next)
}

/// How long since the last refresh a retile without an event waits.
fn fallback_refresh_interval(since_activity: Duration) -> Duration {
    if since_activity < RECENT_ACTIVITY {
        FALLBACK_REFRESH_INTERVAL
    } else {
        IDLE_FALLBACK_REFRESH_INTERVAL
    }
}

const fn refresh_due(
    window_created: bool,
    event_dirty: bool,
    fallback_due: bool,
    activation_ready: bool,
) -> bool {
    window_created || (activation_ready && (event_dirty || fallback_due))
}

#[derive(Clone, Copy)]
enum ManagementScope {
    Both,
    Main,
    Secondary,
    Tertiary,
}

impl ManagementScope {
    fn initial_workspaces(self) -> &'static [WorkspaceId] {
        match self {
            Self::Both => &[WorkspaceId::ONE, WorkspaceId::FIVE, WorkspaceId::EIGHT],
            Self::Main => &[WorkspaceId::ONE],
            Self::Secondary => &[WorkspaceId::FIVE],
            Self::Tertiary => &[WorkspaceId::EIGHT],
        }
    }

    fn automatic_status(self) -> &'static str {
        match self {
            Self::Both => "Automatic tiling enabled · Workspaces 1 / 5 / 8",
            Self::Main => "Automatic tiling enabled · Workspace 1",
            Self::Secondary => "Automatic tiling enabled · Workspace 5",
            Self::Tertiary => "Automatic tiling enabled · Workspace 8",
        }
    }

    fn from_environment() -> Result<Self, String> {
        match crate::config::managed_display_env()? {
            Some(value) => Self::parse(&value),
            None => Ok(Self::Both),
        }
    }

    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "main" => Ok(Self::Main),
            "secondary" => Ok(Self::Secondary),
            "tertiary" => Ok(Self::Tertiary),
            "both" => Ok(Self::Both),
            _ => Err(format!(
                "SPACEMAN_MANAGED_DISPLAY must be main, secondary, tertiary, or both; got {value:?}"
            )),
        }
    }

    fn manages_selector(self, display: &DisplaySelector) -> bool {
        matches!(
            (self, display),
            (Self::Both, _)
                | (Self::Main, DisplaySelector::Main)
                | (Self::Secondary, DisplaySelector::Secondary)
                | (Self::Tertiary, DisplaySelector::Tertiary)
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Action {
    Worker(WorkerAction),
    /// Carbon delivered the hotkey with this ID.
    Hotkey(u32),
    /// The keyboard tap saw an Option key press (virtual key, Carbon modifiers).
    OptionKeyDown(u32, u32),
    ToggleAutomatic,
    /// The first-run setup tour finished, so its hold on tiling ends.
    FinishSetup,
    /// The first-run setup tour ended without starting tiling: its hold ends, paused.
    SkipSetup,
    /// The setup tour wants the open windows, title-free, to suggest an arrangement.
    PlanSetup,
    /// Start tiling with the open windows arranged as the setup tour's plan says.
    Arrange(crate::onboarding_plan::Arrangement),
    /// Pause tiling and put every window back where it was before the arrangement.
    UndoArrangement,
    OpenSettings,
    OpenTarget(OpenTarget),
    OpenSetup,
    OpenAccessibility,
    ToggleHud,
    SetHud(bool),
    SetPauseOnBuiltinDisplay(bool),
    CheckForUpdates,
    InstallUpdate,
    ConfirmUpdateSwitch,
    DownloadUpdate,
    Terminate,
    ToggleLaunchAtLogin,
    Restart,
    Quit,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum WorkerAction {
    Retile,
    SwitchWorkspace(WorkspaceId),
    /// Pointer-initiated workspace selection from the status menu or Workspace HUD:
    /// switches and focuses without triggering mouse-follows-focus warping.
    SelectWorkspace(WorkspaceId),
    WorkspaceBackAndForth,
    MoveWorkspaceToDisplay(DisplayTarget),
    /// A trackpad swipe and its finger count, stepping through the occupied
    /// workspaces of the display under the pointer.
    Swipe(Swipe, Fingers, (i32, i32)),
    /// The Workspace Steps Lab's `switch next|prev`: the swipe's step, on the focused display.
    StepWorkspace(crate::WorkspaceStep),
    MoveFocused(WorkspaceId),
    MoveFocusedFollow(WorkspaceId),
    /// The Workspace Steps Lab's `move next|prev [--follow]`: the focused window one step
    /// along every enabled workspace of its display.
    MoveFocusedStep {
        step: crate::WorkspaceStep,
        follow: bool,
    },
    MoveApp(String, WorkspaceId),
    /// Send the focused window's active tab to a workspace through the app's
    /// own "Move Tab to New Window"; a single-tab window moves whole.
    MoveTab {
        target: WorkspaceId,
        follow: bool,
    },
    CycleFocus,
    /// Option-Shift-backtick: the same ring as `CycleFocus`, walked backward.
    CycleFocusPrevious,
    Focus(Direction),
    Reorder(Direction),
    ToggleLayout,
    Resize(ResizeDirection),
    /// Resize the focused window to this width (`Horizontal`) or height (`Vertical`) in points.
    ResizeTo(crate::Axis, i32),
    /// Equalize the focused tile's immediate split.
    Balance,
    ToggleFullscreenLayout,
    /// Float the focused window above the workspace's tiles, or tile it again.
    ToggleFloat,
    /// Pin the focused window (floating it first) on every workspace, or unpin it.
    TogglePin,
    /// Kit noticed something wrong: mark the journal and snapshot it.
    MarkWrong,
    /// Move the open windows onto the workspaces the setup tour's plan gives them.
    Arrange(crate::onboarding_plan::Arrangement),
    /// The Scratchpad Lab: summon or hide, or send a window to or from it.
    Scratchpad(crate::scratchpad::ScratchpadCommand),
    /// The Focus follows mouse Lab: the pointer rested at `point` on the window-server
    /// window `window_id`; a visible tile there takes focus. Never moves the pointer.
    FocusHovered {
        window_id: u32,
        point: (i32, i32),
    },
}

impl WorkerAction {
    fn queued_acknowledgement(&self) -> String {
        match self {
            Self::Retile => "retile queued".into(),
            Self::SwitchWorkspace(workspace) | Self::SelectWorkspace(workspace) => {
                format!("switch to workspace {workspace} queued")
            }
            Self::WorkspaceBackAndForth => "workspace back-and-forth queued".into(),
            Self::MoveWorkspaceToDisplay(_) => "move workspace to display queued".into(),
            Self::Swipe(..) => "swipe queued".into(),
            Self::StepWorkspace(step) => format!("switch {} queued", step.as_str()),
            Self::MoveFocused(workspace) => format!("move to workspace {workspace} queued"),
            Self::MoveFocusedFollow(workspace) => {
                format!("move and switch to workspace {workspace} queued")
            }
            Self::MoveFocusedStep { step, follow } => {
                let follow = if *follow { " and follow" } else { "" };
                format!("move {}{follow} queued", step.as_str())
            }
            Self::MoveApp(_, workspace) => format!("move app to workspace {workspace} queued"),
            Self::MoveTab {
                target,
                follow: false,
            } => format!("move tab to workspace {target} queued"),
            Self::MoveTab {
                target,
                follow: true,
            } => format!("move tab and switch to workspace {target} queued"),
            Self::CycleFocus => "focus cycle queued".into(),
            Self::CycleFocusPrevious => "reverse focus cycle queued".into(),
            Self::Focus(_) => "focus queued".into(),
            Self::Reorder(_) => "directional move queued".into(),
            Self::ToggleLayout => "layout toggle queued".into(),
            Self::Resize(_) => "resize queued".into(),
            Self::ResizeTo(..) => "resize queued".into(),
            Self::Balance => "balance queued".into(),
            Self::ToggleFullscreenLayout => "fullscreen layout toggle queued".into(),
            Self::ToggleFloat => "float toggle queued".into(),
            Self::TogglePin => "pin toggle queued".into(),
            Self::MarkWrong => "marking the journal".into(),
            Self::Arrange(_) => "arrangement queued".into(),
            Self::Scratchpad(command) => format!("scratchpad {} queued", command.as_str()),
            Self::FocusHovered { .. } => "hover focus queued".into(),
        }
    }
}

enum WorkerCommand {
    Action(WorkerAction, macos::DisplayOutputs),
    Gesture(FinalGesture, bool, macos::DisplayOutputs),
    /// Restore every parked window, onto these displays when they are known.
    RestoreAll(Option<macos::DisplayOutputs>),
    /// Restore every parked window, then put back the frames the last arrangement changed.
    UndoArrangement(Option<macos::DisplayOutputs>),
    /// Read the open windows, title-free, for the setup tour's plan.
    Inventory(macos::DisplayOutputs),
    Stop,
}

/// Worker commands slower than this are logged; long Accessibility stalls
/// otherwise look like Spaceman silently doing nothing.
const SLOW_WORKER_COMMAND: Duration = Duration::from_secs(2);

impl WorkerCommand {
    const fn label(&self) -> &'static str {
        match self {
            Self::Action(WorkerAction::Retile, _) => "retile",
            Self::Action(..) => "action",
            Self::Gesture(..) => "gesture",
            Self::RestoreAll(_) => "restore all",
            Self::UndoArrangement(_) => "undo arrangement",
            Self::Inventory(_) => "inventory",
            Self::Stop => "stop",
        }
    }

    fn interruptible(&self) -> bool {
        matches!(
            self,
            Self::Action(WorkerAction::Retile, _) | Self::Gesture(_, true, _)
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum FinalGesture {
    Drop {
        window: crate::PhysicalWindowId,
        point: (i32, i32),
        bounds: Option<input::PointerBounds>,
    },
    Resize(crate::PhysicalWindowId, crate::Rect),
}

impl FinalGesture {
    #[cfg(test)]
    const fn drop(window: crate::PhysicalWindowId, point: (i32, i32)) -> Self {
        Self::Drop {
            window,
            point,
            bounds: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ExitIntent {
    Quit,
    Restart,
    Update,
    Terminate,
}

impl ExitIntent {
    fn mode_label(self) -> &'static str {
        match self {
            Self::Quit => "quitting",
            Self::Restart => "restarting",
            Self::Update => "updating",
            Self::Terminate => "terminating",
        }
    }
}

enum WorkerResult {
    Status {
        result: Result<CycleStatus, String>,
        debug: Box<RuntimeDebug>,
    },
    Restored {
        failed: usize,
    },
    Inventory(Box<crate::onboarding_plan::Inventory>),
}

/// Startup state initialized before the Rune event loop starts: single-agent
/// lock, control socket, signal handler, and the window-management worker.
pub struct PreparedAgent {
    actions: Receiver<Action>,
    actions_sender: Sender<Action>,
    commands: Sender<WorkerCommand>,
    results: Receiver<WorkerResult>,
    cancelled: Arc<AtomicBool>,
    scope: ManagementScope,
    hotkey_workspaces: Vec<WorkspaceId>,
    shortcuts: crate::shortcuts::Shortcuts,
    hud: bool,
    swipe_fingers: Fingers,
    menu_bar_icon: crate::config::MenuBarIcon,
    focus_follows_mouse: bool,
    pause_on_builtin_display: bool,
    controls: Receiver<ControlRequest>,
    control_server: control::Server,
    bridge: UiBridge,
}

pub fn prepare() -> Result<(PreparedAgent, UiBridge), String> {
    macos::redirect_stderr_to_log();
    log_event!(
        "agent started (pid {}, version {})",
        std::process::id(),
        crate::updater::build_version()
    );
    let scope = ManagementScope::from_environment()?;
    let parked = load_parked()?;
    let (config, workspace_state) =
        recover_startup_metadata(Config::load(), load_workspace_state());
    let hud = config.hud();
    let swipe_fingers = config.swipe_fingers();
    let menu_bar_icon = config.menu_bar_icon();
    let focus_follows_mouse = config.labs().focus_follows_mouse;
    let pause_on_builtin_display = config.pause_on_builtin_display();
    let shortcuts = config.shortcuts().clone();
    let mut configured_workspaces = Workspaces::default();
    config.configure_workspaces(&mut configured_workspaces);
    let mut hotkey_workspaces = WorkspaceId::numbered()
        .chain(config.named_workspaces())
        .chain(
            workspace_state
                .as_ref()
                .into_iter()
                .flat_map(crate::workspace::WorkspaceState::named_workspaces_in_use),
        )
        .filter(|workspace| scope.manages_selector(configured_workspaces.assignment(*workspace)))
        .collect::<HashSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    hotkey_workspaces.sort();
    let (commands, worker_commands) = channel();
    let (worker_results, results) = channel();
    let (actions_sender, actions) = channel();
    let (control_requests, controls) = channel();
    let control_server = control::listen(control_requests)?;
    let signal_actions = actions_sender.clone();
    ctrlc::set_handler(move || {
        // Receiver disconnection means the main loop has already exited.
        drop(signal_actions.send(Action::Quit));
    })
    .map_err(|error| format!("failed to install app signal handler: {error}"))?;
    let cancelled = Arc::new(AtomicBool::new(false));
    spawn_worker(
        worker_commands,
        worker_results,
        Arc::clone(&cancelled),
        scope,
        parked,
        config,
        workspace_state,
    );
    let bridge = UiBridge::new(actions_sender.clone());
    Ok((
        PreparedAgent {
            actions,
            actions_sender,
            commands,
            results,
            cancelled,
            scope,
            hotkey_workspaces,
            shortcuts,
            hud,
            swipe_fingers,
            menu_bar_icon,
            focus_follows_mouse,
            pause_on_builtin_display,
            controls,
            control_server,
            bridge: bridge.clone(),
        },
        bridge,
    ))
}

impl PreparedAgent {
    /// Installs main-thread AppKit hooks (termination gate, status item, updater,
    /// AX event monitor) once winit's `NSApplication` delegate is active.
    /// `setup_tour_open` holds automatic tiling until the first-run setup tour
    /// finishes, so a new install is never retiled behind the setup window.
    pub fn start_main_thread(
        self,
        mut callbacks: HostCallbacks,
        setup_tour_open: bool,
    ) -> Result<MainThreadAgent, String> {
        if let Err(error) = termination::install(self.actions_sender.clone()) {
            log_event!("failed to install application termination gate: {error}");
            return Err(format!(
                "failed to install application termination gate: {error}"
            ));
        }
        if let Err(error) = url_handler::install(self.actions_sender.clone()) {
            log_event!("could not install application:openURLs: handler: {error}");
        }
        if let Err(error) = status_item::install(self.actions_sender.clone()) {
            log_event!("failed to install Spaceman status item: {error}");
            termination::allow_next();
            return Err(format!("failed to install Spaceman status item: {error}"));
        }
        status_item::set_hud(self.hud);
        status_item::set_visible(self.menu_bar_icon != crate::config::MenuBarIcon::Off);
        let launch_status = login_item::status();
        match &launch_status {
            Ok(status) => status_item::set_launch_at_login(*status),
            Err(error) => {
                status_item::set_status(&format!("Launch at Login unavailable: {error}"));
            }
        }
        let workspace_numbers = self
            .scope
            .initial_workspaces()
            .iter()
            .filter_map(|workspace| workspace.number())
            .collect::<Vec<_>>();
        status_item::set_workspace_numbers(&workspace_numbers);
        let trusted = macos::accessibility_trusted();
        status_item::set_accessibility(trusted);
        let events = macos::WindowEventMonitor::new();
        let mut agent = MainThreadAgent::new(
            AppLoopInit {
                actions: self.actions,
                actions_sender: self.actions_sender,
                commands: self.commands,
                results: self.results,
                cancelled: self.cancelled,
                scope: self.scope,
                events,
                hotkey_workspaces: self.hotkey_workspaces,
                shortcuts: self.shortcuts,
                hud: self.hud,
                swipe_fingers: self.swipe_fingers,
                focus_follows_mouse: self.focus_follows_mouse,
                pause_on_builtin_display: self.pause_on_builtin_display,
                trusted,
                setup_tour_open,
                controls: self.controls,
                control_server: self.control_server,
                bridge: self.bridge,
                callbacks: HostCallbacks {
                    open_settings: Box::new(|| Ok(())),
                    open_target: Box::new(|_| Ok(())),
                    open_setup: Box::new(|| Ok(())),
                    save_hud: Box::new(|_| Ok(())),
                },
            },
            launch_status,
        );
        if !trusted {
            status_item::set_status("Paused · Accessibility required");
            if let Err(error) = (callbacks.open_settings)() {
                status_item::set_status(&format!("Could not open Settings: {error}"));
            }
        }
        agent.set_callbacks(callbacks);
        Ok(agent)
    }
}

pub fn allow_immediate_termination() {
    termination::allow_next();
}

pub fn finish_appkit_termination() {
    termination::reply(true);
}

#[cfg(test)]
mod tests;
