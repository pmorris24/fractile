use std::time::SystemTime;

use super::*;

mod actions;
mod automatic;
mod builtin_display;
mod controls;
mod dispatch;
mod hotkey_health;
mod hover;
mod live_shortcuts;
mod probe;
mod setup;

#[cfg(test)]
pub(super) use dispatch::{AfterCommand, WorkerLifecycle};
pub(super) use dispatch::{RestoreKind, WorkerDispatch};
pub(super) use setup::{TilingStart, TilingStartup};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TickOutcome {
    Continue,
    /// Clean quit or restart after parked windows restored: exit the Rune event loop.
    Quit,
    /// AppKit termination (`applicationShouldTerminate:`) finished restoring; reply `true`.
    TerminateAppKit,
}

pub(super) struct AppLoopInit {
    pub(super) actions: Receiver<Action>,
    pub(super) actions_sender: Sender<Action>,
    pub(super) commands: Sender<WorkerCommand>,
    pub(super) results: Receiver<WorkerResult>,
    pub(super) cancelled: Arc<AtomicBool>,
    pub(super) scope: ManagementScope,
    pub(super) events: macos::WindowEventMonitor,
    pub(super) hotkey_workspaces: Vec<WorkspaceId>,
    /// `[shortcuts]` as loaded at startup; later changes apply live (`live_shortcuts`).
    pub(super) shortcuts: crate::shortcuts::Shortcuts,
    pub(super) hud: bool,
    /// `Config::swipe_fingers` as loaded at startup; the config probe keeps it current.
    pub(super) swipe_fingers: Fingers,
    /// `[labs] focus_follows_mouse` as loaded at startup; the config probe keeps it current.
    pub(super) focus_follows_mouse: bool,
    /// `settings.pause_on_builtin_display` as loaded at startup; the config probe keeps it current.
    pub(super) pause_on_builtin_display: bool,
    pub(super) trusted: bool,
    /// A first-run setup tour is open and holds automatic tiling until it ends.
    pub(super) setup_tour_open: bool,
    pub(super) controls: Receiver<ControlRequest>,
    pub(super) control_server: control::Server,
    pub(super) bridge: UiBridge,
    pub(super) callbacks: HostCallbacks,
}

pub struct MainThreadAgent {
    actions: Receiver<Action>,
    actions_sender: Sender<Action>,
    worker: WorkerDispatch,
    results: Receiver<WorkerResult>,
    cancelled: Arc<AtomicBool>,
    scope: ManagementScope,
    events: macos::WindowEventMonitor,
    hotkey_workspaces: Vec<WorkspaceId>,
    shortcuts: crate::shortcuts::Shortcuts,
    controls: Receiver<ControlRequest>,
    _control_server: control::Server,
    bridge: UiBridge,
    callbacks: HostCallbacks,
    hotkeys: Option<Hotkeys>,
    /// Settings is recording a shortcut, since this moment; the hotkeys are off meanwhile.
    recording_since: Option<Instant>,
    /// The hotkeys were unregistered for a recording, plus any table saved meanwhile.
    hotkey_hold: live_shortcuts::HotkeyHold,
    hotkey_delivery: DeliveryWatch,
    last_hotkey_heal: Option<Instant>,
    accessibility: bool,
    automatic: bool,
    hud: bool,
    /// The only finger count whose swipes are queued for the worker.
    swipe_fingers: Fingers,
    /// The Focus follows mouse Lab: hover focuses tiles, timed by `hover`.
    focus_follows_mouse: bool,
    hover: hover::HoverTracker,
    pause_on_builtin_display: bool,
    builtin_display: builtin_display::BuiltinDisplayPolicy,
    last_display_topology_check: Instant,
    display_topology_dirty: bool,
    awaiting_accessibility: bool,
    startup_tiling: TilingStartup,
    accessibility_requested: bool,
    last_accessibility_poll: Instant,
    return_from_settings: permission_guide::ReturnFromSettings,
    exit_intent: Option<ExitIntent>,
    next_restore_retry: Option<Instant>,
    active_workspaces: Vec<WorkspaceId>,
    /// Every workspace holding windows, as the last cycle saw them, for Settings.
    workspace_windows: Vec<UiWorkspaceWindows>,
    last_error: Option<String>,
    latest_debug: Option<RuntimeDebug>,
    pending_inputs: InputQueue,
    gesture_tracker: GestureTracker,
    last_refresh: Instant,
    observation_recheck: Option<Instant>,
    opening_until: Option<Instant>,
    /// Retile every `WINDOW_OPENING_RECHECK` until then after a cancelled cycle.
    cancelled_retry_until: Option<Instant>,
    /// Last input, activation, or window event; idle loops retile less often.
    last_activity: Instant,
    last_frontmost_check: Instant,
    last_hotkey_refresh: Instant,
    last_ui_refresh: Instant,
    config_stamp: Option<SystemTime>,
    launch_at_login: UiLaunchAtLogin,
    /// Login-item and config state, read off the main thread.
    probe: probe::Prober,
    ui_dirty: bool,
    mouse_was_down: bool,
    frontmost_application: Option<AppInstanceId>,
    activation_refresh_at: Option<Instant>,
    /// Installed the first time tiling starts; swipes queue nothing while paused.
    swipes: Option<macos::SwipeMonitor>,
    /// Option key presses, to notice hotkeys macOS stopped delivering.
    keys: Option<macos::KeyMonitor>,
    updater: crate::updater::Updater,
    /// The updater revision last shown.
    update_revision: u64,
    /// Tiling was on when Install & restart began, so a failed install resumes it.
    resume_after_update: bool,
    setup: setup::SetupRequests,
}

impl MainThreadAgent {
    pub(super) fn new(
        init: AppLoopInit,
        launch_status: Result<login_item::Status, String>,
    ) -> Self {
        let trusted = init.trusted;
        let mut builtin_display = builtin_display::BuiltinDisplayPolicy::default();
        let initial_topology = init
            .pause_on_builtin_display
            .then(macos::display_topology)
            .flatten();
        let start_auto_paused = trusted
            && !init.setup_tour_open
            && builtin_display
                .should_start_auto_paused(init.pause_on_builtin_display, initial_topology);
        let (hotkeys, hotkey_error) = if start_auto_paused {
            (None, None)
        } else if trusted {
            match HotkeyPlan::current(&init.hotkey_workspaces, &init.shortcuts)
                .and_then(|plan| Hotkeys::install(init.actions_sender.clone(), plan))
            {
                Ok(hotkeys) => {
                    log_event!("hotkeys: registered {} shortcuts", hotkeys.len());
                    for skipped in hotkeys.skipped() {
                        log_event!("hotkeys: {skipped}");
                    }
                    (Some(hotkeys), None)
                }
                Err(error) => (None, Some(format!("Hotkeys unavailable: {error}"))),
            }
        } else {
            (None, Some("Accessibility permission is required".into()))
        };
        let automatic = hotkeys.is_some() && !init.setup_tour_open && !start_auto_paused;
        let mut worker = WorkerDispatch::new(init.commands);

        status_item::set_automatic(automatic);
        if automatic {
            status_item::set_status("Automatic tiling enabled");
            worker
                .send_with_outputs(|outputs| WorkerCommand::Action(WorkerAction::Retile, outputs));
        } else {
            status_item::set_status(match hotkey_error.as_deref() {
                Some(error) => error,
                None if init.setup_tour_open => "Setup in progress · tiling starts when you finish",
                None if start_auto_paused => builtin_display::PAUSED_STATUS,
                None => "Paused",
            });
            worker.send_restore();
        }

        let initial_refresh = Instant::now() - FALLBACK_REFRESH_INTERVAL;
        let mut agent = Self {
            actions: init.actions,
            actions_sender: init.actions_sender,
            worker,
            results: init.results,
            cancelled: init.cancelled,
            scope: init.scope,
            events: init.events,
            hotkey_workspaces: init.hotkey_workspaces,
            shortcuts: init.shortcuts,
            controls: init.controls,
            _control_server: init.control_server,
            bridge: init.bridge,
            callbacks: init.callbacks,
            hotkeys,
            recording_since: None,
            hotkey_hold: live_shortcuts::HotkeyHold::None,
            hotkey_delivery: DeliveryWatch::default(),
            last_hotkey_heal: None,
            accessibility: trusted,
            automatic,
            hud: init.hud,
            swipe_fingers: init.swipe_fingers,
            focus_follows_mouse: init.focus_follows_mouse,
            hover: hover::HoverTracker::default(),
            pause_on_builtin_display: init.pause_on_builtin_display,
            builtin_display,
            last_display_topology_check: Instant::now(),
            display_topology_dirty: false,
            awaiting_accessibility: !trusted,
            startup_tiling: TilingStartup::new(init.setup_tour_open),
            accessibility_requested: false,
            last_accessibility_poll: initial_refresh,
            return_from_settings: permission_guide::ReturnFromSettings::default(),
            exit_intent: None,
            next_restore_retry: None,
            active_workspaces: init.scope.initial_workspaces().to_vec(),
            workspace_windows: Vec::new(),
            last_error: hotkey_error,
            latest_debug: None,
            pending_inputs: InputQueue::default(),
            gesture_tracker: GestureTracker::default(),
            last_refresh: initial_refresh,
            observation_recheck: None,
            opening_until: None,
            cancelled_retry_until: None,
            last_activity: Instant::now(),
            last_frontmost_check: Instant::now(),
            last_hotkey_refresh: initial_refresh,
            last_ui_refresh: Instant::now(),
            config_stamp: crate::config::modified(),
            launch_at_login: UiLaunchAtLogin::from_result(launch_status),
            probe: probe::spawn(APPLICATION_REFRESH_INTERVAL),
            ui_dirty: true,
            mouse_was_down: macos::mouse_down(),
            frontmost_application: macos::frontmost_application(),
            activation_refresh_at: None,
            swipes: None,
            keys: None,
            updater: crate::updater::Updater::start(),
            update_revision: 0,
            resume_after_update: false,
            setup: setup::SetupRequests::default(),
        };
        status_item::set_update(agent.updater.status().ready_version());
        if automatic {
            agent.listen_for_input();
        } else if start_auto_paused {
            log_event!("auto-paused tiling at startup: built-in display only");
            agent
                .bridge
                .publish_toast(UiHudToast::paused(builtin_display::TOAST_PAUSED));
        }
        agent.publish_ui(false);
        agent
    }

    pub(super) fn set_callbacks(&mut self, callbacks: HostCallbacks) {
        self.callbacks = callbacks;
    }

    fn listen_for_input(&mut self) {
        if self.swipes.is_none() {
            let sender = self.actions_sender.clone();
            match macos::SwipeMonitor::install(move |swipe, fingers, point| {
                let swipe = WorkerAction::Swipe(swipe, fingers, point);
                drop(sender.send(Action::Worker(swipe)));
            }) {
                Ok(monitor) => self.swipes = Some(monitor),
                Err(error) => log_event!("trackpad swipes unavailable: {error}"),
            }
        }
        if self.keys.is_none() {
            let sender = self.actions_sender.clone();
            match macos::KeyMonitor::install(move |key_code, modifiers| {
                drop(sender.send(Action::OptionKeyDown(key_code, modifiers)));
            }) {
                Ok(monitor) => self.keys = Some(monitor),
                Err(error) => log_event!("hotkey delivery check unavailable: {error}"),
            }
        }
    }

    pub fn tick(&mut self) -> TickOutcome {
        loop {
            let result = match self.results.try_recv() {
                Ok(result) => result,
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.hotkeys = None;
                    self.release_hotkey_hold_on_stop();
                    self.pending_inputs.clear();
                    self.gesture_tracker.clear();
                    self.builtin_display.clear_auto_pause();
                    status_item::set_automatic(false);
                    self.automatic = false;
                    self.worker.reset_disconnected();
                    self.next_restore_retry = None;
                    self.ui_dirty = true;
                    if self.exit_intent.is_some() {
                        self.cancel_unavailable_exit();
                    } else {
                        status_item::set_status("Worker stopped unexpectedly");
                    }
                    break;
                }
            };
            match result {
                WorkerResult::Status { result, debug } if self.automatic => {
                    self.worker.finish_command();
                    self.cancelled.store(false, Ordering::Release);
                    self.latest_debug = Some(*debug);
                    // Newer input routinely cancels background work; only
                    // genuine failures are worth a log line.
                    if let Err(error) = &result
                        && self.last_error.as_ref() != Some(error)
                        && !is_cancelled_error(error)
                    {
                        log_event!("worker error: {error}");
                    }
                    let keep_shortcuts = self.is_shortcuts_not_applied_error();
                    let status_text = worker_status_update(
                        &mut self.last_error,
                        keep_shortcuts,
                        result
                            .as_ref()
                            .map(|status| status.failed)
                            .map_err(String::as_str),
                    );
                    let now = Instant::now();
                    if matches!(&result, Err(error) if is_cancelled_error(error)) {
                        self.cancelled_retry_until = Some(now + CANCELLED_CYCLE_RETRY);
                    }
                    let cancelled_retry = cancelled_cycle_recheck(now, self.cancelled_retry_until);
                    if cancelled_retry.is_none() {
                        self.cancelled_retry_until = None;
                    }
                    self.observation_recheck = [self.observation_recheck, cancelled_retry]
                        .into_iter()
                        .flatten()
                        .min();
                    if let Ok(status) = &result {
                        let opening = opening_recheck(
                            Instant::now(),
                            self.opening_until,
                            self.frontmost_application.as_ref(),
                            status.missing_frontmost_window.as_ref(),
                        );
                        self.observation_recheck =
                            [self.observation_recheck, status.close_recheck, opening]
                                .into_iter()
                                .flatten()
                                .min();
                        self.active_workspaces.clone_from(&status.active);
                        self.workspace_windows.clone_from(&status.windows);
                        if self.hud
                            && let Some(hud) = status.hud.clone()
                        {
                            self.bridge.publish_hud(hud);
                        }
                    }
                    self.ui_dirty = true;
                    show_result(result, status_text.as_deref());
                }
                WorkerResult::Status { result, debug } => {
                    self.worker.finish_command();
                    self.latest_debug = Some(*debug);
                    if let Err(error) = result
                        && record_paused_worker_error(&mut self.last_error, error)
                    {
                        self.ui_dirty = true;
                    }
                }
                // Every parked window is back on screen: only now may the
                // bundle be swapped and Spaceman relaunch into it.
                WorkerResult::Restored { failed: 0 }
                    if self.exit_intent == Some(ExitIntent::Update) =>
                {
                    match self.updater.install() {
                        Ok(version) => {
                            log_event!("update {version} installed; relaunching");
                            self.worker.stop();
                            termination::allow_next();
                            return TickOutcome::Quit;
                        }
                        Err(error) => self.update_failed(&error),
                    }
                }
                // Every parked window is back: a ready update may now be
                // swapped in for the next launch.
                WorkerResult::Restored { failed: 0 }
                    if self.exit_intent == Some(ExitIntent::Terminate) =>
                {
                    self.install_update_on_quit();
                    self.worker.stop();
                    return TickOutcome::TerminateAppKit;
                }
                WorkerResult::Restored { failed: 0 } if self.exit_intent.is_some() => {
                    self.install_update_on_quit();
                    self.worker.stop();
                    termination::allow_next();
                    return TickOutcome::Quit;
                }
                WorkerResult::Restored { failed: 0 } => {
                    self.worker.finish_restore();
                    status_item::set_status(self.paused_status());
                }
                WorkerResult::Inventory(inventory) => {
                    self.worker.finish_command();
                    self.bridge.publish_inventory(*inventory);
                }
                WorkerResult::Restored { failed } => {
                    self.worker.defer_restore_retry();
                    self.next_restore_retry = Some(Instant::now() + RESTORE_RETRY_INTERVAL);
                    status_item::set_status(&format!("Restoring windows · {failed} failed"));
                }
            }
            if let Some(sent) = self.worker.start_pending_restore()
                && !sent
                && self.exit_intent.is_some()
            {
                self.cancel_unavailable_exit();
            }
        }

        self.drain_controls();
        self.drain_actions();

        if self.worker.is_restoring()
            && !self.worker.is_busy()
            && self
                .next_restore_retry
                .is_some_and(|retry| Instant::now() >= retry)
        {
            self.next_restore_retry = None;
            if !self.worker.send_restore() {
                if self.exit_intent.is_some() {
                    self.cancel_unavailable_exit();
                } else {
                    status_item::set_status("Paused · worker unavailable");
                }
            }
        }

        self.send_wanted_inventory();
        self.tick_builtin_display();
        if self.automatic {
            self.tick_automatic();
        }

        let revision = self.updater.revision();
        if revision != self.update_revision {
            self.update_revision = revision;
            let update_status = self.updater.status();
            status_item::set_update(update_status.ready_version());
            if update_status != UiUpdateStatus::Failed {
                self.clear_update_failed_error();
            }
            self.ui_dirty = true;
        }
        self.maybe_publish_ui();
        TickOutcome::Continue
    }

    /// What the menu reports while tiling has not started.
    fn paused_status(&self) -> &str {
        match self.last_error.as_deref() {
            Some(error) => error,
            None if self.startup_tiling.held() => {
                "Setup in progress · tiling starts when you finish"
            }
            None if self.builtin_display.auto_paused_for_builtin => builtin_display::PAUSED_STATUS,
            None => "Paused",
        }
    }

    fn stop_automatic_and_restore(&mut self, kind: RestoreKind) {
        self.automatic = false;
        self.builtin_display.clear_auto_pause();
        self.cancelled.store(true, Ordering::Release);
        self.hotkeys = None;
        self.release_hotkey_hold_on_stop();
        self.hotkey_delivery.clear();
        self.pending_inputs.clear();
        self.gesture_tracker.clear();
        self.ui_dirty = true;
        self.worker.request_restore(kind);
        status_item::set_automatic(false);
    }

    fn cancel_unavailable_exit(&mut self) {
        if self.exit_intent.take() == Some(ExitIntent::Terminate) {
            termination::cancel_pending();
        }
        self.last_error = Some("Exit blocked because window restoration is unavailable".into());
        self.ui_dirty = true;
        status_item::set_status("Exit blocked · restoration unavailable");
    }

    /// Quitting with an update ready installs it for the next launch. Called
    /// only once every parked window is restored; a failure never holds the quit.
    fn install_update_on_quit(&self) {
        match self.updater.install_on_quit() {
            Some(Ok(version)) => log_event!("update {version} installed; it starts next launch"),
            Some(Err(error)) => log_event!("update install at quit failed: {error}"),
            None => {}
        }
    }

    /// The update couldn't be installed after windows were restored: stay
    /// running and resume tiling as it was.
    fn update_failed(&mut self, error: &str) {
        log_event!("update install failed: {error}");
        self.worker.finish_restore();
        self.exit_intent = None;
        let message = format!("{UPDATE_FAILED_PREFIX}{error}");
        self.last_error = Some(message.clone());
        self.ui_dirty = true;
        status_item::set_status(&message);
        if std::mem::take(&mut self.resume_after_update) {
            drop(self.actions_sender.send(Action::ToggleAutomatic));
        }
    }

    pub(super) fn clear_update_failed_error(&mut self) {
        if is_update_failed_error(self.last_error.as_deref()) {
            self.last_error = None;
            self.ui_dirty = true;
            if self.automatic {
                status_item::clear_status();
            } else {
                status_item::set_status(self.paused_status());
            }
        }
    }

    fn queue_worker_input(&mut self, input: QueuedInput) {
        self.last_activity = Instant::now();
        if let Err(error) = self.pending_inputs.push(input) {
            log_event!("{error}");
            self.last_error = Some(error.into());
            status_item::set_status(error);
        }
        self.ui_dirty = true;
    }

    fn maybe_publish_ui(&mut self) {
        let periodic = self.last_ui_refresh.elapsed() >= APPLICATION_REFRESH_INTERVAL;
        let accessibility_poll = !self.accessibility
            && (self.accessibility_requested || permission_guide::active())
            && self.last_accessibility_poll.elapsed() >= ACCESSIBILITY_POLL_INTERVAL;
        if !self.ui_dirty && !periodic && !accessibility_poll {
            self.poll_return_from_settings();
            return;
        }
        let mut config_changed = false;
        if periodic {
            self.last_ui_refresh = Instant::now();
            while let Ok(probe) = self.probe.results.try_recv() {
                if let Some(status) = probe.launch_at_login {
                    apply_login_probe(
                        &mut self.launch_at_login,
                        status,
                        status_item::set_launch_at_login,
                    );
                }
                if probe.config_stamp != self.config_stamp {
                    config_changed = true;
                    self.config_stamp = probe.config_stamp;
                    if let Some(hud) = probe.hud {
                        self.hud = hud;
                        status_item::set_hud(hud);
                    }
                    if let Some(shortcuts) = probe.shortcuts {
                        self.apply_shortcuts(shortcuts);
                    }
                    if let Some(fingers) = probe.swipe_fingers {
                        self.swipe_fingers = fingers;
                    }
                    if let Some(icon) = probe.menu_bar_icon {
                        status_item::set_visible(icon != crate::config::MenuBarIcon::Off);
                    }
                    if let Some(on) = probe.focus_follows_mouse {
                        self.focus_follows_mouse = on;
                        self.hover.interrupt();
                    }
                    if let Some(on) = probe.pause_on_builtin_display {
                        self.set_pause_on_builtin_display(on);
                    }
                }
            }
            // Approval happens in System Settings; watch for it closely until then.
            if self.launch_at_login == UiLaunchAtLogin::NeedsApproval {
                self.probe.refresh_login();
            }
        }
        if accessibility_poll {
            self.last_accessibility_poll = Instant::now();
        }
        self.ui_dirty = false;
        let was_trusted = self.accessibility;
        let trusted = macos::accessibility_trusted();
        self.accessibility = trusted;
        status_item::set_accessibility(trusted);
        if trusted && !was_trusted {
            let guide_was_active = permission_guide::active();
            permission_guide::close();
            if self.accessibility_requested || guide_was_active {
                self.accessibility_requested = false;
                self.return_from_settings.note_granted(Instant::now());
            }
        }
        if self.awaiting_accessibility
            && trusted
            && !self.automatic
            && !self.worker.is_restoring_or_pending()
            && self.exit_intent.is_none()
        {
            match self.startup_tiling.accessibility_granted() {
                TilingStart::Now => {
                    self.awaiting_accessibility = false;
                    drop(self.actions_sender.send(Action::ToggleAutomatic));
                }
                TilingStart::AfterSetup => {
                    status_item::set_status("Setup in progress · tiling starts when you finish");
                }
            }
        }
        self.publish_ui(config_changed);
        self.poll_return_from_settings();
    }

    fn poll_return_from_settings(&mut self) {
        if !self.return_from_settings.is_pending() {
            return;
        }
        let focus = permission_guide::settings_focus();
        if self.return_from_settings.poll(Instant::now(), focus) {
            log_event!("accessibility granted in System Settings; bringing Spaceman forward");
            if let Err(error) = (self.callbacks.open_settings)() {
                status_item::set_status(&format!("Could not open Settings: {error}"));
            }
        }
    }

    pub(super) fn publish_ui(&self, config_changed: bool) {
        self.bridge.publish(self.snapshot(), config_changed);
    }

    pub(super) fn snapshot(&self) -> UiAgentSnapshot {
        UiAgentSnapshot {
            accessibility: self.accessibility,
            automatic: self.automatic,
            hud: self.hud,
            launch_at_login: self.launch_at_login.clone(),
            update: self.updater.status(),
            active_workspaces: self.active_workspaces.clone(),
            windows: self.workspace_windows.clone(),
            last_error: self.last_error.clone(),
        }
    }
}

pub(super) fn is_update_failed_error(error: Option<&str>) -> bool {
    error.is_some_and(|error| error.starts_with(UPDATE_FAILED_PREFIX))
}

pub(super) fn is_shortcuts_not_applied_error(error: Option<&str>) -> bool {
    error.is_some_and(|error| error.starts_with(live_shortcuts::NOT_APPLIED))
}

/// Updates `last_error` when automatic tiling starts and returns the status text
/// to show in the menu bar. A failed update installation is unrelated to tiling
/// and survives the automatic resume that follows it; an unapplied shortcut table
/// also stays retained while the previous table remains active.
pub(super) fn automatic_started_status<'a>(
    last_error: &'a mut Option<String>,
    automatic_status: &'a str,
) -> &'a str {
    if is_update_failed_error(last_error.as_deref()) {
        last_error.as_deref().unwrap_or(automatic_status)
    } else {
        if !is_shortcuts_not_applied_error(last_error.as_deref()) {
            *last_error = None;
        }
        automatic_status
    }
}

/// Applies a fresh background login-item status probe to both the published
/// Settings snapshot and the menu bar item (when the status read succeeded).
fn apply_login_probe(
    launch_at_login: &mut UiLaunchAtLogin,
    status: Result<login_item::Status, String>,
    update_menu: impl FnOnce(login_item::Status),
) {
    if let Ok(login_status) = status {
        update_menu(login_status);
    }
    *launch_at_login = UiLaunchAtLogin::from_result(status);
}

fn is_cancelled_error(error: &str) -> bool {
    error.ends_with("cancelled")
}

/// Records a worker error that arrived after automatic tiling paused, ignoring
/// routine cancellations caused by pausing or superseding input.
fn record_paused_worker_error(last_error: &mut Option<String>, error: String) -> bool {
    if is_cancelled_error(&error) {
        return false;
    }
    *last_error = Some(error);
    true
}

/// Applies a worker cycle result to `last_error` and returns the status line for
/// the menu bar (`None` hides the status item).
fn worker_status_update(
    last_error: &mut Option<String>,
    keep_shortcuts: bool,
    result: Result<usize, &str>,
) -> Option<String> {
    match result {
        Err(error) if is_cancelled_error(error) => {
            // Routine cancellation is not a failure: keep any retained status
            // error intact instead of overwriting it or flashing an error.
            is_update_failed_error(last_error.as_deref())
                .then(|| last_error.clone())
                .flatten()
        }
        Err(error) => {
            *last_error = Some(error.to_owned());
            Some(error.to_owned())
        }
        Ok(failed) => {
            let keep_update = is_update_failed_error(last_error.as_deref());
            if !keep_shortcuts && !keep_update {
                *last_error = None;
            }
            if failed > 0 {
                Some(format!(
                    "{failed} window operation{} failed",
                    if failed == 1 { "" } else { "s" },
                ))
            } else if keep_update {
                last_error.clone()
            } else {
                None
            }
        }
    }
}

fn show_result(result: Result<CycleStatus, String>, status_text: Option<&str>) {
    if let Ok(status) = result {
        status_item::set_workspace_layouts(&status.layouts);
        status_item::set_workspace_menu(&status.menu);
    }
    match status_text {
        Some(status) => status_item::set_status(status),
        None => status_item::clear_status(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn automatic_resume_and_clean_cycles_preserve_update_install_failure() {
        let reason = format!("{UPDATE_FAILED_PREFIX}could not write update receipt");
        let mut last_error = Some(reason.clone());

        let resumed_status = automatic_started_status(
            &mut last_error,
            "Automatic tiling enabled · Workspaces 1 / 6",
        );
        assert_eq!(resumed_status, reason);
        assert_eq!(last_error.as_deref(), Some(reason.as_str()));

        let cycle_status = worker_status_update(&mut last_error, false, Ok(0));
        assert_eq!(cycle_status.as_deref(), Some(reason.as_str()));
        assert_eq!(last_error.as_deref(), Some(reason.as_str()));

        let failed_ops_status = worker_status_update(&mut last_error, false, Ok(2));
        assert_eq!(
            failed_ops_status.as_deref(),
            Some("2 window operations failed")
        );
        assert_eq!(last_error.as_deref(), Some(reason.as_str()));
    }

    #[test]
    fn automatic_start_and_clean_cycles_still_clear_non_update_errors() {
        let mut last_error = Some("Accessibility permission is required".to_owned());
        let status = automatic_started_status(
            &mut last_error,
            "Automatic tiling enabled · Workspaces 1 / 6",
        );
        assert_eq!(status, "Automatic tiling enabled · Workspaces 1 / 6");
        assert_eq!(last_error, None);

        let mut worker_error = Some("Workspace 2 could not restore all of its windows".to_owned());
        assert_eq!(worker_status_update(&mut worker_error, false, Ok(0)), None);
        assert_eq!(worker_error, None);

        let mut shortcuts_error = Some("Shortcuts not applied: conflict".to_owned());
        assert_eq!(
            automatic_started_status(
                &mut shortcuts_error,
                "Automatic tiling enabled · Workspaces 1 / 6",
            ),
            "Automatic tiling enabled · Workspaces 1 / 6"
        );
        assert_eq!(
            shortcuts_error.as_deref(),
            Some("Shortcuts not applied: conflict")
        );
        assert_eq!(
            worker_status_update(&mut shortcuts_error, true, Ok(0)),
            None
        );
        assert_eq!(
            shortcuts_error.as_deref(),
            Some("Shortcuts not applied: conflict")
        );
    }

    #[test]
    fn routine_cancellations_preserve_retained_errors_and_never_store_cancelled() {
        let reason = format!("{UPDATE_FAILED_PREFIX}could not write update receipt");
        let mut last_error = Some(reason.clone());

        // Routine cancellation while automatic preserves the retained install failure.
        assert_eq!(
            worker_status_update(&mut last_error, false, Err("workspace operation cancelled")),
            Some(reason.clone())
        );
        assert_eq!(last_error.as_deref(), Some(reason.as_str()));

        // The following clean cycle still retains the install failure.
        assert_eq!(
            worker_status_update(&mut last_error, false, Ok(0)),
            Some(reason.clone())
        );
        assert_eq!(last_error.as_deref(), Some(reason.as_str()));

        // Routine cancellation arriving after pause does not overwrite or populate last_error.
        assert!(!record_paused_worker_error(
            &mut last_error,
            "workspace operation cancelled".into()
        ));
        assert_eq!(last_error.as_deref(), Some(reason.as_str()));

        let mut empty = None;
        assert_eq!(
            worker_status_update(&mut empty, false, Err("workspace switch cancelled")),
            None
        );
        assert_eq!(empty, None);
        assert!(!record_paused_worker_error(
            &mut empty,
            "workspace operation cancelled".into()
        ));
        assert_eq!(empty, None);

        // Genuine failures still report and store their error in both states.
        assert_eq!(
            worker_status_update(&mut empty, false, Err("Observation error: unavailable")),
            Some("Observation error: unavailable".into())
        );
        assert_eq!(empty.as_deref(), Some("Observation error: unavailable"));
        assert!(record_paused_worker_error(
            &mut empty,
            "Restore error".into()
        ));
        assert_eq!(empty.as_deref(), Some("Restore error"));
    }

    #[test]
    fn login_probe_updates_both_snapshot_and_menu_item() {
        let mut ui = UiLaunchAtLogin::Off;
        let mut menu = None;
        apply_login_probe(&mut ui, Ok(login_item::Status::Enabled), |s| menu = Some(s));
        assert_eq!(ui, UiLaunchAtLogin::On);
        assert_eq!(menu, Some(login_item::Status::Enabled));

        menu = None;
        apply_login_probe(&mut ui, Ok(login_item::Status::RequiresApproval), |s| {
            menu = Some(s)
        });
        assert_eq!(ui, UiLaunchAtLogin::NeedsApproval);
        assert_eq!(menu, Some(login_item::Status::RequiresApproval));

        menu = None;
        apply_login_probe(&mut ui, Ok(login_item::Status::Disabled), |s| {
            menu = Some(s)
        });
        assert_eq!(ui, UiLaunchAtLogin::Off);
        assert_eq!(menu, Some(login_item::Status::Disabled));

        menu = None;
        apply_login_probe(&mut ui, Err("unknown".into()), |s| menu = Some(s));
        assert_eq!(ui, UiLaunchAtLogin::Failed("unknown".into()));
        assert_eq!(menu, None);
    }
}
