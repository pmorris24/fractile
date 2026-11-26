mod arrange;
mod benchmarks;
mod cross_display;
mod dispatch;
mod display_disconnect;
mod drag_path;
mod drag_rows;
mod dual_selector_move;
mod dwindle;
mod empty_workspace_choice;
mod empty_workspace_move;
mod float;
mod float_regressions;
mod focus;
mod focus_follows_mouse;
mod fullscreen;
mod gestures;
mod journal;
mod layout;
mod memory;
mod minimums;
mod move_handoff;
mod move_tab;
mod move_workspace;
mod ordered_out;
mod parking;
mod physical_displays;
mod recent_focus;
mod reconciliation;
mod reconnect_unsettled;
mod reliability;
mod reported;
mod residency;
mod restart_recall;
mod rules;
mod scratchpad;
mod space_transitions;
mod spacing;
mod stacked_resize;
mod startup;
mod state_machine;
mod switch_repeat;
mod switching;
mod tear_regressions;
mod three_display;
mod unplugged_desktops;
mod utility_floats;
mod workspace_order;
mod workspace_steps;
use crate::{BindingState, Slot, SlotId};

use super::*;
use super::{
    app_loop::{
        AfterCommand, RestoreKind, TilingStart, TilingStartup, WorkerDispatch, WorkerLifecycle,
    },
    cycle::OMISSION_SETTLE,
    input::{is_window_drag, is_window_resize},
    parking::REPARK_ATTEMPT_LIMIT,
};

#[derive(Clone, Debug, Eq, PartialEq)]
enum BackendCall {
    Persist,
    SetPosition(crate::PhysicalWindowId, crate::Rect),
    Apply(crate::PhysicalWindowId, crate::Rect),
    Raise(crate::PhysicalWindowId),
    Focus(crate::PhysicalWindowId),
    WarpPointer((i32, i32)),
    DetachTab(crate::PhysicalWindowId),
}

/// What pressing "Move Tab to New Window" does in the fake app.
#[derive(Clone, Debug)]
enum FakeTabDetach {
    /// Chromium: the tab opens in a new window at this frame, which takes focus.
    NewWindow(crate::PhysicalWindowId, crate::Rect),
    /// AppKit tab group: the tab's own window moves to `moved_to` and keeps
    /// focus; the group's next tab, `next`, shows in its old place.
    TabGroup {
        next: crate::PhysicalWindowId,
        moved_to: crate::Rect,
    },
    /// Pressed, but no window has appeared yet.
    Silent,
    Failed,
}

#[derive(Default)]
struct FakeBackend {
    apps_confirming_no_windows: HashSet<AppInstanceId>,
    close_probes: Vec<AppInstanceId>,
    /// Windows whose app is hidden or which are minimized.
    set_aside: HashSet<crate::PhysicalWindowId>,
    frames: HashMap<crate::PhysicalWindowId, crate::Rect>,
    server_frames: HashMap<crate::PhysicalWindowId, crate::Rect>,
    /// Windows the window server still lists but has ordered out (closed
    /// windows an app keeps alive, minimized windows, other Spaces).
    ordered_out: HashSet<u32>,
    /// Simulates apps whose real window movement lags position writes.
    frozen_server_frames: bool,
    focused: Option<crate::PhysicalWindowId>,
    frontmost: Option<AppInstanceId>,
    observation: Option<crate::NativeObservation>,
    persisted: HashMap<crate::PhysicalWindowId, crate::Rect>,
    calls: Vec<BackendCall>,
    /// Windows whose position writes went through the parking path.
    parked_writes: Vec<crate::PhysicalWindowId>,
    fail_persist: bool,
    frame_outcome: Option<macos::WindowAccessError>,
    managed_outcome: Option<macos::ApplyOutcome>,
    /// Fails position writes with this outcome (an app briefly inaccessible).
    position_outcome: Option<macos::ApplyOutcome>,
    /// Position writes macOS clamps back, leaving the window where it was.
    refuses_position: Option<fn(crate::Rect) -> bool>,
    /// Widths windows refuse to shrink below, clamped synchronously like AppKit.
    minimum_widths: HashMap<crate::PhysicalWindowId, i32>,
    /// Display frames. A write that moves a window from one of them to
    /// another has its height fit to the bottom of the display it left, as
    /// macOS does before the app sees its new screen (#428).
    departed_display_clamp: Vec<crate::Rect>,
    focus_outcome: Option<bool>,
    /// Raised by the next position write: an activation cancelling the worker
    /// in the middle of a switch.
    cancel_on_position: Option<Arc<AtomicBool>>,
    server_snapshot_unavailable: bool,
    running_apps: Option<HashSet<AppInstanceId>>,
    snapshot_count: usize,
    observed_apply_count: usize,
    workspace_state: Option<crate::workspace::WorkspaceState>,
    window_memory: Option<crate::window_memory::WindowMemory>,
    /// Window-server stacking, front-most first.
    stacking: Option<Vec<u32>>,
    journal: Vec<crate::journal::JournalEvent>,
    config_path: Option<PathBuf>,
    fail_persist_config: bool,
    persisted_monitors: Vec<(WorkspaceId, DisplaySelector)>,
    pointer: Option<(i32, i32)>,
    mouse_down: bool,
    /// The right or middle mouse button is held.
    other_button_down: bool,
    /// When macOS last changed the active Space.
    space_changed_at: Option<Instant>,
    /// The next position write starts a Space slide (Cmd+Tab out of a
    /// native fullscreen app while a focus follow runs).
    space_change_on_position: bool,
    /// "Move Tab to New Window"; `None` when the window has one tab.
    tab_detach: Option<FakeTabDetach>,
    /// Virtual time: it moves only when a test advances it.
    clock: FakeClock,
}

struct FakeClock(Instant);

impl Default for FakeClock {
    fn default() -> Self {
        Self(Instant::now())
    }
}

impl FakeBackend {
    /// Let `by` pass on the worker's clock.
    fn advance(&mut self, by: Duration) {
        self.clock.0 += by;
    }

    /// The frame macOS leaves a window written to `target` on another display:
    /// its height stops at the bottom of the display it left (#428).
    fn fit_to_departed_display(
        &self,
        window: &crate::PhysicalWindowId,
        target: crate::Rect,
    ) -> Option<crate::Rect> {
        let before = *self.frames.get(window)?;
        let from = self
            .departed_display_clamp
            .iter()
            .find(|display| display.contains_center(before))?;
        let height = from.y + from.height - target.y;
        (!from.contains_center(target) && height > 0 && height < target.height)
            .then_some(crate::Rect { height, ..target })
    }

    /// A window the app opens: on screen at `frame` and in its next snapshot.
    fn open_window(&mut self, window: &crate::PhysicalWindowId, frame: crate::Rect) {
        self.frames.insert(window.clone(), frame);
        let Some(observation) = self.observation.as_mut() else {
            return;
        };
        for app in &mut observation.applications {
            if let crate::AppObservation::Complete { app, windows, .. } = app
                && app.id == window.app
            {
                windows.push(crate::ObservedWindow {
                    id: window.clone(),
                    frame,
                    resizable: true,
                    title: None,
                    document: None,
                });
            }
        }
    }
}

impl WorkspaceBackend for FakeBackend {
    fn now(&self) -> Instant {
        self.clock.0
    }

    fn pause(&mut self, duration: Duration) {
        self.advance(duration);
    }

    fn confirms_no_windows(&mut self, app: &AppInstanceId) -> bool {
        self.close_probes.push(app.clone());
        self.apps_confirming_no_windows.contains(app)
    }

    fn confirms_set_aside(&mut self, window: &crate::PhysicalWindowId) -> bool {
        self.set_aside.contains(window)
    }

    fn snapshot(
        &mut self,
        cycle: u64,
        outputs: &macos::DisplayOutputs,
    ) -> Result<crate::NativeObservation, String> {
        self.snapshot_count += 1;
        let mut observation = self.observation.clone().ok_or("missing fake observation")?;
        observation.cycle = cycle;
        for app in &mut observation.applications {
            if let crate::AppObservation::Complete {
                focused, windows, ..
            } = app
            {
                windows.retain_mut(|window| {
                    let Some(frame) = self.frames.get(&window.id).copied() else {
                        return false;
                    };
                    window.frame = frame;
                    outputs.union().contains_center(frame)
                });
                if focused
                    .as_ref()
                    .is_some_and(|focused| !windows.iter().any(|window| &window.id == focused))
                {
                    *focused = None;
                }
            }
        }
        Ok(observation)
    }

    fn is_process_running(&mut self, app: &AppInstanceId) -> bool {
        self.running_apps
            .as_ref()
            .is_none_or(|running| running.contains(app))
    }

    fn apply_observed(
        &mut self,
        placements: &[crate::Placement],
        settings: &Settings,
        cancelled: &AtomicBool,
    ) -> Vec<macos::ApplyResult> {
        self.observed_apply_count += 1;
        self.apply_managed(placements, settings, cancelled)
    }

    fn frame(
        &mut self,
        window: &crate::PhysicalWindowId,
    ) -> Result<crate::Rect, macos::WindowAccessError> {
        if let Some(outcome) = self.frame_outcome {
            return Err(outcome);
        }
        self.frames
            .get(window)
            .copied()
            .ok_or(macos::WindowAccessError::Gone)
    }

    fn window_server_frame(&mut self, window: &crate::PhysicalWindowId) -> Option<crate::Rect> {
        self.server_frames
            .get(window)
            .or_else(|| self.frames.get(window))
            .copied()
    }

    fn window_server_frames(&mut self) -> Option<HashMap<u32, crate::Rect>> {
        if self.server_snapshot_unavailable {
            return None;
        }
        let mut frames: HashMap<u32, crate::Rect> = self
            .frames
            .iter()
            .map(|(window, frame)| (window.window_id, *frame))
            .collect();
        for (window, frame) in &self.server_frames {
            frames.insert(window.window_id, *frame);
        }
        Some(frames)
    }

    fn onscreen_window_frames(&mut self) -> Option<HashMap<u32, crate::Rect>> {
        let mut frames = self.window_server_frames()?;
        frames.retain(|window_id, _| !self.ordered_out.contains(window_id));
        Some(frames)
    }

    fn onscreen_window_order(&mut self) -> Option<Vec<u32>> {
        self.stacking.clone()
    }

    fn window_owners(&mut self, window_ids: &HashSet<u32>) -> HashMap<u32, AppInstanceId> {
        self.frames
            .keys()
            .filter(|window| window_ids.contains(&window.window_id))
            .map(|window| (window.window_id, window.app.clone()))
            .collect()
    }

    fn set_parked_position(
        &mut self,
        window: &crate::PhysicalWindowId,
        target: crate::Rect,
    ) -> macos::ApplyOutcome {
        self.parked_writes.push(window.clone());
        self.set_position(window, target)
    }

    fn set_position(
        &mut self,
        window: &crate::PhysicalWindowId,
        target: crate::Rect,
    ) -> macos::ApplyOutcome {
        self.calls
            .push(BackendCall::SetPosition(window.clone(), target));
        if std::mem::take(&mut self.space_change_on_position) {
            self.space_changed_at = Some(self.clock.0);
        }
        if let Some(cancelled) = self.cancel_on_position.take() {
            cancelled.store(true, Ordering::Release);
        }
        if let Some(outcome) = self.position_outcome {
            return outcome;
        }
        if self.refuses_position.is_some_and(|refuses| refuses(target)) {
            return macos::ApplyOutcome::Unavailable;
        }
        let Some(frame) = self.frames.get_mut(window) else {
            return macos::ApplyOutcome::Gone;
        };
        frame.x = target.x;
        frame.y = target.y;
        if !self.frozen_server_frames
            && let Some(server) = self.server_frames.get_mut(window)
        {
            server.x = target.x;
            server.y = target.y;
        }
        macos::ApplyOutcome::Applied
    }

    fn focused_window(&mut self) -> Option<crate::PhysicalWindowId> {
        self.focused.clone()
    }

    fn frontmost_application(&mut self) -> Option<AppInstanceId> {
        self.frontmost.clone()
    }

    fn active_space_changed_at(&mut self) -> Option<Instant> {
        self.space_changed_at
    }

    fn raise_window(&mut self, window: &crate::PhysicalWindowId) -> bool {
        self.calls.push(BackendCall::Raise(window.clone()));
        true
    }

    fn focus_window(&mut self, window: &crate::PhysicalWindowId) -> bool {
        self.calls.push(BackendCall::Focus(window.clone()));
        if self.focus_outcome == Some(false) {
            return false;
        }
        self.focused = Some(window.clone());
        self.frontmost = Some(window.app.clone());
        true
    }

    fn detach_tab(&mut self, window: &crate::PhysicalWindowId) -> macos::TabDetachResult {
        self.calls.push(BackendCall::DetachTab(window.clone()));
        match self.tab_detach.take() {
            None => return macos::TabDetachResult::SingleTabOrUnsupported,
            Some(FakeTabDetach::NewWindow(new, frame)) => {
                self.open_window(&new, frame);
                self.focused = Some(new);
            }
            Some(FakeTabDetach::TabGroup { next, moved_to }) => {
                let Some(old) = self.frames.insert(window.clone(), moved_to) else {
                    return macos::TabDetachResult::Error("window is gone".into());
                };
                self.open_window(&next, old);
            }
            Some(FakeTabDetach::Silent) => {}
            Some(FakeTabDetach::Failed) => {
                return macos::TabDetachResult::Error("AXPress: Failure".into());
            }
        }
        macos::TabDetachResult::Detached
    }

    fn pointer_location(&mut self) -> Option<(i32, i32)> {
        self.pointer
    }

    fn mouse_down(&mut self) -> bool {
        self.mouse_down
    }

    fn any_mouse_button_down(&mut self) -> bool {
        self.mouse_down || self.other_button_down
    }

    fn warp_pointer(&mut self, point: (i32, i32)) -> bool {
        if self.mouse_down {
            return false;
        }
        self.calls.push(BackendCall::WarpPointer(point));
        self.pointer = Some(point);
        true
    }

    fn apply_managed(
        &mut self,
        placements: &[crate::Placement],
        _: &Settings,
        cancelled: &AtomicBool,
    ) -> Vec<macos::ApplyResult> {
        placements
            .iter()
            .take_while(|_| !cancelled.load(Ordering::Acquire))
            .filter_map(|placement| {
                let window = placement.window.clone()?;
                self.calls
                    .push(BackendCall::Apply(window.clone(), placement.frame));
                let mut outcome = self.managed_outcome.unwrap_or(macos::ApplyOutcome::Applied);
                if let Some(&width) = self.minimum_widths.get(&window)
                    && self.managed_outcome.is_none()
                    && placement.frame.width < width
                {
                    outcome = macos::ApplyOutcome::Constrained;
                    self.frames.insert(
                        window.clone(),
                        crate::Rect {
                            width,
                            ..placement.frame
                        },
                    );
                }
                if matches!(outcome, macos::ApplyOutcome::Applied)
                    && let Some(clamped) = self.fit_to_departed_display(&window, placement.frame)
                {
                    outcome = macos::ApplyOutcome::Constrained;
                    self.frames.insert(window.clone(), clamped);
                }
                if matches!(outcome, macos::ApplyOutcome::Applied) {
                    self.frames.insert(window.clone(), placement.frame);
                    if !self.frozen_server_frames && self.server_frames.contains_key(&window) {
                        self.server_frames.insert(window.clone(), placement.frame);
                    }
                }
                Some(macos::ApplyResult {
                    actual: self.frames.get(&window).copied(),
                    window,
                    target: placement.frame,
                    outcome,
                })
            })
            .collect()
    }

    fn apply_managed_during_drag(
        &mut self,
        placements: &[crate::Placement],
        settings: &Settings,
        dragged: &crate::PhysicalWindowId,
        cancelled: &AtomicBool,
    ) -> Vec<macos::ApplyResult> {
        let placements = placements
            .iter()
            .filter(|placement| placement.window.as_ref() != Some(dragged))
            .cloned()
            .collect::<Vec<_>>();
        self.apply_managed(&placements, settings, cancelled)
    }

    fn persist_parked(
        &mut self,
        parked: &HashMap<crate::PhysicalWindowId, crate::Rect>,
    ) -> Result<(), String> {
        self.calls.push(BackendCall::Persist);
        if self.fail_persist {
            return Err("injected persistence failure".into());
        }
        self.persisted = parked.clone();
        Ok(())
    }

    fn persist_workspace_state(
        &mut self,
        state: &crate::workspace::WorkspaceState,
    ) -> Result<(), String> {
        self.workspace_state = Some(state.clone());
        Ok(())
    }

    fn persist_window_memory(
        &mut self,
        memory: &crate::window_memory::WindowMemory,
    ) -> Result<(), String> {
        self.window_memory = Some(memory.clone());
        Ok(())
    }

    fn persist_workspace_monitor(
        &mut self,
        workspace: WorkspaceId,
        selector: &DisplaySelector,
    ) -> Result<(), String> {
        if self.fail_persist_config {
            return Err("injected config persistence failure".into());
        }
        if let Some(path) = &self.config_path {
            crate::config::save_workspace_monitor_at(path, workspace, selector)?;
        }
        self.persisted_monitors.push((workspace, selector.clone()));
        Ok(())
    }

    fn journal(&mut self, line: &crate::journal::JournalLine) {
        self.journal.push(line.event.clone());
    }
}

fn window(id: u32) -> crate::PhysicalWindowId {
    crate::PhysicalWindowId {
        app: AppInstanceId {
            pid: 1,
            launched_at: 1,
        },
        window_id: id,
    }
}

fn frame(x: i32) -> crate::Rect {
    crate::Rect {
        x,
        y: 20,
        width: 400,
        height: 300,
    }
}

fn outputs() -> macos::DisplayOutputs {
    macos::DisplayOutputs::new(vec![
        macos::DisplayOutput {
            id: "main-id".into(),
            name: "Main".into(),
            frame: crate::Rect {
                width: 1000,
                height: 800,
                ..crate::Rect::default()
            },
            is_main: true,
        },
        macos::DisplayOutput {
            id: "secondary-id".into(),
            name: "Secondary".into(),
            frame: crate::Rect {
                x: 1000,
                width: 1000,
                height: 800,
                ..crate::Rect::default()
            },
            is_main: false,
        },
    ])
    .unwrap()
}

fn three_outputs() -> macos::DisplayOutputs {
    let mut displays = outputs().displays().to_vec();
    displays.push(macos::DisplayOutput {
        id: "third-id".into(),
        name: "Projector".into(),
        frame: crate::Rect {
            x: 2000,
            width: 1000,
            height: 800,
            ..crate::Rect::default()
        },
        is_main: false,
    });
    macos::DisplayOutputs::new(displays).unwrap()
}

fn bind(
    runtime: &mut WorkspaceRuntime<FakeBackend>,
    workspace: WorkspaceId,
    window: &crate::PhysicalWindowId,
) {
    runtime.workspaces.engine_mut(workspace).insert_slot(Slot {
        id: SlotId(u64::from(window.window_id)),
        binding: BindingState::Bound(window.clone()),
    });
}

fn runtime(backend: FakeBackend) -> WorkspaceRuntime<FakeBackend> {
    WorkspaceRuntime::with_backend(ManagementScope::Both, backend, HashMap::new())
}

fn runtime_with_parked(
    backend: FakeBackend,
    parked: impl IntoIterator<Item = (crate::PhysicalWindowId, crate::Rect)>,
) -> WorkspaceRuntime<FakeBackend> {
    WorkspaceRuntime::with_backend(ManagementScope::Both, backend, parked.into_iter().collect())
}

fn main_parked_frame(normal: crate::Rect) -> crate::Rect {
    let outputs = outputs();
    let displays = outputs.frames().collect::<Vec<_>>();
    crate::geometry::parked_frame(outputs.main().frame, &displays, normal)
}

fn runtime_with_config(backend: FakeBackend, config: Config) -> WorkspaceRuntime<FakeBackend> {
    WorkspaceRuntime::with_backend_and_config(
        ManagementScope::Both,
        backend,
        HashMap::new(),
        config,
    )
}

fn observation(window: &crate::PhysicalWindowId, frame: crate::Rect) -> crate::NativeObservation {
    observation_for("com.example.Test", window, frame)
}

fn observation_for(
    bundle_id: &str,
    window: &crate::PhysicalWindowId,
    frame: crate::Rect,
) -> crate::NativeObservation {
    observation_for_windows(bundle_id, window, &[(window.clone(), frame)])
}

fn observation_for_windows(
    bundle_id: &str,
    focused: &crate::PhysicalWindowId,
    windows: &[(crate::PhysicalWindowId, crate::Rect)],
) -> crate::NativeObservation {
    crate::NativeObservation {
        cycle: 0,
        mouse_down: false,
        applications: vec![crate::AppObservation::Complete {
            app: crate::ObservedApplication {
                id: focused.app.clone(),
                bundle_id: bundle_id.into(),
            },
            focused: Some(focused.clone()),
            windows: windows
                .iter()
                .map(|(window, frame)| crate::ObservedWindow {
                    id: window.clone(),
                    frame: *frame,
                    resizable: true,
                    title: None,
                    document: None,
                })
                .collect(),
        }],
    }
}

fn benchmark_backend(window_count: u32) -> FakeBackend {
    let app = AppInstanceId {
        pid: 1,
        launched_at: 1,
    };
    let mut frames = HashMap::new();
    let mut windows = Vec::new();
    for id in 1..=window_count {
        let window = crate::PhysicalWindowId {
            app: app.clone(),
            window_id: id,
        };
        let frame = crate::Rect {
            x: if id <= window_count / 2 { 50 } else { 1050 },
            y: 20,
            width: 400,
            height: 300,
        };
        frames.insert(window.clone(), frame);
        windows.push(crate::ObservedWindow {
            id: window,
            frame,
            resizable: true,
            title: None,
            document: None,
        });
    }
    FakeBackend {
        frames,
        observation: Some(crate::NativeObservation {
            cycle: 0,
            mouse_down: false,
            applications: vec![crate::AppObservation::Complete {
                app: crate::ObservedApplication {
                    id: app,
                    bundle_id: "com.example.Benchmark".into(),
                },
                focused: None,
                windows,
            }],
        }),
        ..FakeBackend::default()
    }
}
