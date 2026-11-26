//! The Scratchpad Lab's native side: hiding windows in the scratchpad, summoning them onto the
//! focused display, and keeping the pure [`crate::scratchpad`] model honest as windows close,
//! workspaces switch, or the Lab is turned off.
//!
//! Hidden scratchpad windows are parked with the same durable recovery frames as inactive
//! workspace windows and owned by no workspace. The cycle leaves them parked (instead of
//! recovering them as stray unowned windows) only while they are in the scratchpad, so pause,
//! quit, restart, a crash or switching the Lab off all bring them back on screen.

use super::parking::RESTORE_CONFIRM_TOLERANCE;
use super::*;
use crate::scratchpad::{ScratchpadCommand, summon_frame};
use crate::workspace::FloatCause;

const LAB_OFF: &str = "Scratchpad is off (set [labs] scratchpad = true in config.toml)";

impl<B: WorkspaceBackend> WorkspaceRuntime<B> {
    pub(super) fn scratchpad_command(
        &mut self,
        command: ScratchpadCommand,
        outputs: macos::DisplayOutputs,
        cancelled: &AtomicBool,
    ) -> Result<CycleStatus, String> {
        if !self.config.labs().scratchpad {
            return Err(LAB_OFF.into());
        }
        match command {
            ScratchpadCommand::Toggle => self.toggle_scratchpad(&outputs, cancelled),
            ScratchpadCommand::Mark => self.mark_scratchpad(outputs, cancelled),
        }
    }

    /// Send the focused window to the scratchpad, or take a shown scratchpad window out of it.
    fn mark_scratchpad(
        &mut self,
        outputs: macos::DisplayOutputs,
        cancelled: &AtomicBool,
    ) -> Result<CycleStatus, String> {
        let window = self
            .current_focus()
            .ok_or("Focused window is unavailable")?;
        if let Some(entry) = self.workspaces.scratchpad().entry(&window) {
            return match entry.shown_on {
                Some(workspace) => {
                    self.release_from_scratchpad(window, workspace, outputs, cancelled)
                }
                None => Err("Focused window is already hidden in the scratchpad".into()),
            };
        }
        if self.workspaces.workspace_of_any(&window).is_none() {
            // A window observed for the first time may not be bound yet.
            self.cycle(outputs.clone(), cancelled)?;
        }
        let workspace = self
            .workspaces
            .workspace_of_any(&window)
            .ok_or("Focused window is not managed by Fractile")?;
        if !self.is_visible(workspace, &outputs) {
            return Err("Focused window is not on an active workspace".into());
        }
        if self.parked.is_exempt(&window) {
            return Err(
                "Focused window keeps moving itself back on screen, so it cannot be hidden".into(),
            );
        }
        // A floating window keeps its size; a tile gets the default summon size.
        let size = self
            .workspaces
            .floating_workspace_of(&window)
            .and_then(|_| self.backend.frame(&window).ok())
            .map(|frame| (frame.width, frame.height));
        let output = output_for(&self.workspaces, workspace, &outputs);
        // Parked while its workspace still owns it: the recovery frame is durable before the
        // window leaves the screen, and a crash before ownership changes restores it in place.
        let canvas = self.layout_settings(output).canvas();
        match self.park_window(&window, output, &outputs) {
            macos::ApplyOutcome::Applied if self.parked.contains_key(&window) => {}
            macos::ApplyOutcome::Gone => {
                self.forget_window_on(workspace, &window, canvas);
                return Err("Focused window has closed".into());
            }
            _ => return Err("Focused window could not be moved off-screen".into()),
        }
        self.forget_window_on(workspace, &window, canvas);
        self.workspaces.stash(window.clone(), size);
        log_event!("scratchpad: hid {window:?} from workspace {workspace}");
        let layout = self.layout_workspace(workspace, output, cancelled);
        self.persist_workspace_state_if_changed()?;
        self.focus_after_scratchpad(workspace, std::slice::from_ref(&window));
        Ok(self.status(layout.failed, &outputs))
    }

    /// A shown scratchpad window leaves the scratchpad and joins the tiles of the workspace it
    /// is shown on, or floats there when a rule or its size keeps it out of the tiles. One
    /// residency transition: its route never applies, since it is not a new window (#414).
    fn release_from_scratchpad(
        &mut self,
        window: crate::PhysicalWindowId,
        workspace: WorkspaceId,
        outputs: macos::DisplayOutputs,
        cancelled: &AtomicBool,
    ) -> Result<CycleStatus, String> {
        // A summon still landing becomes an ordinary restore.
        self.parked.cancel_summon(&window);
        log_event!("scratchpad: released {window:?} on workspace {workspace}");
        if self.keeps_floating(&window, &outputs).is_some() {
            self.workspaces
                .float(window, workspace, FloatCause::Automatic);
            self.persist_workspace_state_if_changed()?;
            return Ok(self.status(0, &outputs));
        }
        self.tile_in_place(window, workspace, &outputs, cancelled)
    }

    /// Hide the scratchpad window shown on the focused display's workspace, or summon the next one.
    fn toggle_scratchpad(
        &mut self,
        outputs: &macos::DisplayOutputs,
        cancelled: &AtomicBool,
    ) -> Result<CycleStatus, String> {
        let scratchpad = self.workspaces.scratchpad();
        if scratchpad.is_empty() {
            return Err("Scratchpad is empty".into());
        }
        let target = self.scratchpad_target(outputs)?;
        let shown = scratchpad.shown_on(target.workspace);
        if !shown.is_empty() {
            return self.hide_scratchpad(&shown, &target, outputs);
        }
        let window = scratchpad
            .next_for(target.workspace)
            .ok_or("Scratchpad is empty")?;
        self.summon_scratchpad(window, &target, outputs, cancelled)
    }

    /// The visible workspace a toggle acts on: the focused window's, else the display it is on,
    /// else the last focused workspace, else the main display's.
    fn scratchpad_target(
        &mut self,
        outputs: &macos::DisplayOutputs,
    ) -> Result<ResolvedWorkspace, String> {
        let visible: Vec<_> = self
            .visible_workspaces(outputs)
            .into_iter()
            .filter(|resolved| self.scope.manages_selector(&resolved.selector))
            .collect();
        let find = |workspace: Option<WorkspaceId>| {
            workspace.and_then(|workspace| {
                visible
                    .iter()
                    .find(|resolved| resolved.workspace == workspace)
            })
        };
        let focused = self.current_focus();
        if let Some(resolved) = find(
            focused
                .as_ref()
                .and_then(|window| self.workspaces.workspace_of_any(window)),
        ) {
            return Ok(resolved.clone());
        }
        let frame = focused
            .as_ref()
            .and_then(|window| self.backend.window_server_frame(window));
        frame
            .and_then(|frame| {
                visible
                    .iter()
                    .find(|resolved| resolved.output.contains_center(frame))
            })
            .or_else(|| find(self.focus.history().current))
            .or_else(|| {
                visible
                    .iter()
                    .find(|resolved| resolved.display_id == outputs.main().id)
            })
            .or_else(|| visible.first())
            .cloned()
            .ok_or_else(|| "No active workspace is available".into())
    }

    fn summon_scratchpad(
        &mut self,
        window: crate::PhysicalWindowId,
        target: &ResolvedWorkspace,
        outputs: &macos::DisplayOutputs,
        cancelled: &AtomicBool,
    ) -> Result<CycleStatus, String> {
        let size = self
            .workspaces
            .scratchpad()
            .entry(&window)
            .and_then(|entry| entry.size);
        let frame = summon_frame(target.output, size);
        let recovery = self.parked.record(&window).cloned();
        let expected = self.restore_target(frame);
        match self.write_restore_position(&window, expected) {
            macos::ApplyOutcome::Applied => {}
            macos::ApplyOutcome::Gone => {
                log_event!("scratchpad: {window:?} closed while hidden");
                self.leave_scratchpad(&window);
                self.persist_removals_or_rollback(recovery.map(|recovery| (window, recovery)));
                return self.toggle_scratchpad(outputs, cancelled);
            }
            _ => return Err("Scratchpad window could not be shown".into()),
        }
        let placement = crate::Placement {
            slot: crate::SlotId(0),
            window: Some(window.clone()),
            frame,
        };
        let settings = self.layout_settings(target.output);
        let failed = failed_results(&self.backend.apply_managed(
            &[placement],
            &settings,
            cancelled,
        ));
        self.workspaces.show_scratchpad(&window, target.workspace);
        // Like any restore, the recovery frame is retired only once the window server
        // confirms the window arrived at its centered summon frame; until then its owner's
        // cycles confirm or retry the centered destination rather than the former tile frame.
        if self.parked.contains_key(&window) {
            if self
                .backend
                .window_server_frame(&window)
                .is_some_and(|actual| {
                    actual.position_approximately_equals(expected, RESTORE_CONFIRM_TOLERANCE)
                })
            {
                let record = self.commit_unparked(&window);
                self.persist_removals_or_rollback(record.map(|record| (window.clone(), record)));
            } else {
                self.parked
                    .summon_written(&window, frame, self.backend.now());
            }
        }
        if !self.focus_scratchpad_target(&window, &[]) {
            log_event!("scratchpad: could not focus {window:?}");
        }
        self.record_focused_workspace(target.workspace);
        log_event!(
            "scratchpad: showed {window:?} on workspace {}",
            target.workspace
        );
        self.persist_workspace_state_if_changed()?;
        let mut status = self.status(failed, outputs);
        status.hud = Some(self.hud_for(target.workspace, UiHudKind::Workspace, None, outputs));
        Ok(status)
    }

    /// Confirms an arrived scratchpad summon or promotes its pending target over
    /// the stale pre-summon tile recovery frame before moving the float to another
    /// workspace so its floating position is preserved (#304).
    pub(super) fn settle_pending_summon_for_move(&mut self, window: &crate::PhysicalWindowId) {
        let Some(summon) = self.parked.summon_destination(window) else {
            return;
        };
        let expected = self.restore_target(summon);
        if self.parked.contains_key(window) {
            let _: macos::ApplyOutcome = self.finish_scratchpad_summon(window, summon, true);
            if self.parked.contains_key(window) {
                let _: bool = self.record_recovery(window, expected);
            }
        }
    }

    /// Confirms or retries a summoned scratchpad window's centered destination without
    /// replacing it with the old parked recovery frame.
    pub(super) fn finish_scratchpad_summon(
        &mut self,
        window: &crate::PhysicalWindowId,
        target: crate::Rect,
        retry_throttled: bool,
    ) -> macos::ApplyOutcome {
        let Some(recovery) = self.parked.record(window).cloned() else {
            return macos::ApplyOutcome::Applied;
        };
        let target = self.restore_target(target);
        let arrived = |runtime: &mut Self| {
            runtime
                .backend
                .window_server_frame(window)
                .is_some_and(|actual| {
                    actual.position_approximately_equals(target, RESTORE_CONFIRM_TOLERANCE)
                })
        };
        let outcome = if arrived(self) {
            macos::ApplyOutcome::Applied
        } else if retry_throttled {
            return macos::ApplyOutcome::Applied;
        } else {
            // Applied keeps the summon pending (and throttles the next retry)
            // until the window server confirms the window arrived.
            match self.write_restore_position(window, target) {
                macos::ApplyOutcome::Applied if !arrived(self) => {
                    return macos::ApplyOutcome::Applied;
                }
                outcome => outcome,
            }
        };
        if matches!(
            outcome,
            macos::ApplyOutcome::Applied | macos::ApplyOutcome::Gone
        ) {
            let _: Option<ParkRecord> = self.commit_unparked(window);
            self.persist_removals_or_rollback([(window.clone(), recovery)]);
        }
        outcome
    }

    fn hide_scratchpad(
        &mut self,
        windows: &[crate::PhysicalWindowId],
        target: &ResolvedWorkspace,
        outputs: &macos::DisplayOutputs,
    ) -> Result<CycleStatus, String> {
        let mut failed = 0;
        for window in windows {
            let size = self
                .backend
                .frame(window)
                .ok()
                .map(|frame| (frame.width, frame.height));
            match self.park_scratchpad_window(window, target.output, outputs) {
                macos::ApplyOutcome::Applied => self.workspaces.hide_scratchpad(window, size),
                macos::ApplyOutcome::Gone => {}
                _ => failed += 1,
            }
        }
        self.persist_workspace_state_if_changed()?;
        self.focus_after_scratchpad(target.workspace, windows);
        Ok(self.status(failed, outputs))
    }

    /// Park a shown scratchpad window; the caller then hides it. A closed window leaves the
    /// scratchpad (`Gone`); any other failure leaves it shown.
    fn park_scratchpad_window(
        &mut self,
        window: &crate::PhysicalWindowId,
        output: crate::Rect,
        outputs: &macos::DisplayOutputs,
    ) -> macos::ApplyOutcome {
        let displays: Vec<_> = outputs.frames().collect();
        let already_hidden = self.parked.contains_key(window)
            && self.parked.summon_destination(window).is_none()
            && self
                .backend
                .window_server_frame(window)
                .is_some_and(|frame| !crate::geometry::shows_on(frame, &displays));
        if !already_hidden {
            match self.park_window(window, output, outputs) {
                macos::ApplyOutcome::Applied if self.parked.contains_key(window) => {}
                macos::ApplyOutcome::Gone => {
                    self.leave_scratchpad(window);
                    self.workspaces.forget_floating(window);
                    return macos::ApplyOutcome::Gone;
                }
                macos::ApplyOutcome::Applied => return macos::ApplyOutcome::Unavailable,
                outcome => return outcome,
            }
        }
        log_event!("scratchpad: hid {window:?}");
        macos::ApplyOutcome::Applied
    }

    /// `window` left the scratchpad; a summon still landing becomes an ordinary restore.
    pub(super) fn leave_scratchpad(&mut self, window: &crate::PhysicalWindowId) {
        self.workspaces.leave_scratchpad(window);
        self.parked.cancel_summon(window);
    }

    pub(super) fn clear_scratchpad(&mut self) {
        for window in self.workspaces.clear_scratchpad() {
            self.parked.cancel_summon(&window);
        }
    }

    /// Hand focus back to the workspace's tiles once a scratchpad window leaves the screen.
    fn focus_after_scratchpad(
        &mut self,
        workspace: WorkspaceId,
        hidden: &[crate::PhysicalWindowId],
    ) {
        let target = self.workspaces.preferred_focus(workspace).or_else(|| {
            self.workspaces
                .engine(workspace)
                .bound_windows()
                .into_iter()
                .next()
        });
        if let Some(target) = target {
            if !self.focus_scratchpad_target(&target, hidden) {
                log_event!("scratchpad: could not focus {target:?} on workspace {workspace}");
            }
        } else {
            self.focus.end_request();
        }
    }

    /// Requests focus on `target` and records the request so immediate
    /// follow-up commands address `target` while Accessibility still reports the
    /// previous or just-hidden window.
    fn focus_scratchpad_target(
        &mut self,
        target: &crate::PhysicalWindowId,
        extra_stale: &[crate::PhysicalWindowId],
    ) -> bool {
        let reported = self.backend.focused_window();
        let hidden = extra_stale.iter().cloned().map(Some);
        self.request_focus(target, std::iter::once(reported).chain(hidden))
    }

    /// After every worker command: hide windows whose workspace was switched away (i3 keeps them
    /// hidden until summoned again), let go of hidden windows that closed or came back on screen
    /// by other means, and bring everything back when the Lab is turned off. A window that
    /// closed, tiled, or moved while shown already left the scratchpad with its residency.
    pub(super) fn sync_scratchpad(&mut self, outputs: &macos::DisplayOutputs) {
        let scratchpad = self.workspaces.scratchpad();
        if scratchpad.is_empty() {
            return;
        }
        if !self.config.labs().scratchpad {
            let hidden = scratchpad.hidden();
            self.clear_scratchpad();
            log_event!("scratchpad: Lab off; restoring {} window(s)", hidden.len());
            self.restore_windows(hidden);
            return;
        }
        let visible: HashSet<_> = self
            .visible_workspaces(outputs)
            .into_iter()
            .map(|resolved| resolved.workspace)
            .collect();
        let mut closed = Vec::new();
        let mut hid = false;
        for entry in scratchpad.entries() {
            let window = &entry.window;
            match entry.shown_on {
                Some(workspace) if visible.contains(&workspace) => {}
                Some(workspace) => {
                    let output = output_for(&self.workspaces, workspace, outputs);
                    // Prefer the window's live (or pending-summon) size over `self.parked`:
                    // when a workspace switch happens before a delayed summon is confirmed,
                    // `self.parked` still holds the pre-summon tile recovery frame (#258).
                    let size = self
                        .backend
                        .frame(window)
                        .ok()
                        .or_else(|| self.parked.summon_destination(window))
                        .or_else(|| self.parked.get(window).copied())
                        .map(|frame| (frame.width, frame.height));
                    if self.park_scratchpad_window(window, output, outputs)
                        == macos::ApplyOutcome::Applied
                    {
                        self.workspaces.hide_scratchpad(window, size);
                        hid = true;
                    }
                }
                // Back on screen by other means (healed, recovered): an ordinary window.
                None if !self.parked.contains_key(window) => self.leave_scratchpad(window),
                None => {
                    if self.backend.is_process_running(&window.app)
                        && self.backend.window_server_frame(window).is_some()
                    {
                        continue;
                    }
                    log_event!("scratchpad: {window:?} closed while hidden");
                    if let Some(record) = self.commit_unparked(window) {
                        closed.push((window.clone(), record));
                    }
                    self.leave_scratchpad(window);
                }
            }
        }
        self.persist_removals_or_rollback(closed);
        if hid && let Err(error) = self.persist_workspace_state_if_changed() {
            log_event!("scratchpad: could not persist workspace state: {error}");
        }
    }
}
