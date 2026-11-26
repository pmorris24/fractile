use super::parking::RESTORE_CONFIRM_TOLERANCE;
use super::*;

const REVEAL_SETTLE_LIMIT: Duration = Duration::from_millis(300);
/// Poll interval while waiting for incoming windows to arrive. The on-screen
/// window list costs about half a millisecond, so polling this often is cheap
/// and takes most of the wait out of a switch.
const REVEAL_POLL_INTERVAL: Duration = Duration::from_millis(3);
/// How long a pending reveal waits before the restore write is re-issued.
const REVEAL_REWRITE_INTERVAL: Duration = Duration::from_millis(100);

use super::commands::{CommandOrigin, WorkspaceFocusOutcome};

mod moves;
mod repeat;
mod steps;

pub(super) use repeat::RepeatSwitch;

impl<B: WorkspaceBackend> WorkspaceRuntime<B> {
    /// A switch called directly, as unit tests do.
    #[cfg(test)]
    pub(super) fn switch(
        &mut self,
        workspace: WorkspaceId,
        outputs: macos::DisplayOutputs,
        cancelled: &AtomicBool,
    ) -> Result<CycleStatus, String> {
        self.switch_with(
            workspace,
            outputs,
            cancelled,
            RepeatSwitch::KeepFocus,
            CommandOrigin::Direct,
        )
    }

    pub(super) fn switch_with(
        &mut self,
        workspace: WorkspaceId,
        outputs: macos::DisplayOutputs,
        cancelled: &AtomicBool,
        repeat: RepeatSwitch,
        origin: CommandOrigin,
    ) -> Result<CycleStatus, String> {
        let selector = self.ensure_managed_workspace(workspace)?;
        let output = output_for(&self.workspaces, workspace, &outputs);
        let display_id = outputs.resolve(&selector).id.clone();
        let previous = self
            .visible_workspaces(&outputs)
            .into_iter()
            .find(|resolved| resolved.display_id == display_id)
            .map_or(workspace, |resolved| resolved.workspace);
        if previous == workspace {
            // A repeat press steps from the window the previous press focused,
            // which Accessibility may not report yet. Every other request
            // trusts the reported focus exactly as before.
            let focused = if repeat == RepeatSwitch::CycleFocus {
                self.current_focus()
            } else {
                self.backend.focused_window()
            };
            // A repeat press of the shortcut for the workspace already up and
            // focused walks its tiles. With focus somewhere else (another
            // display's workspace, an unmanaged window) the press keeps its old
            // job of bringing focus back to this workspace, and only the press
            // after that cycles.
            if repeat == RepeatSwitch::CycleFocus
                && let Some(focused) = focused
                    .as_ref()
                    .filter(|focused| self.workspaces.workspace_of_any(focused) == Some(workspace))
                && self.cycle_focus_on_repeat(workspace, focused, &outputs, origin)?
            {
                let mut status = self.status(0, &outputs);
                status.hud = Some(self.hud_for(workspace, UiHudKind::Workspace, None, &outputs));
                return Ok(status);
            }
            let windows = self.workspaces.windows(workspace);
            if self.workspaces.deck(workspace).is_monocle()
                && let Some(focused) = focused
                    .as_ref()
                    .filter(|focused| self.workspaces.workspace_of(focused) == Some(workspace))
            {
                self.workspaces.remember_focus(workspace, focused);
                self.record_focused_workspace(workspace);
                self.focus.settle_fullscreen(workspace);
                let frame = self.focus_window_frame(focused, workspace, output);
                self.follow_pointer_to(origin, &display_id, output, frame, &outputs);
                self.persist_workspace_state_if_changed()?;
                let mut status = self.status(0, &outputs);
                status.hud = Some(self.hud_for(workspace, UiHudKind::Workspace, None, &outputs));
                return Ok(status);
            }
            let frontmost = self.backend.frontmost_application();
            // A tile of this workspace that already has focus keeps it; only
            // focus elsewhere moves to the remembered tile.
            let outcome = self.focus_workspace_entry(workspace, &windows, focused, frontmost, true);
            self.follow_pointer_for_workspace(origin, workspace, output, outcome, &outputs);
            // Selecting a workspace that is already up still moves focus there
            // when it lives on another display, so it belongs in the history.
            self.record_focused_workspace(workspace);
            let mut status = self.status(0, &outputs);
            status.hud = Some(self.hud_for(workspace, UiHudKind::Workspace, None, &outputs));
            return Ok(status);
        }
        // The focus before the switch. A keyboard switch trusts an in-flight
        // focus command over a stale Accessibility report; a selection (menu,
        // HUD, swipe, back-and-forth) remembers the tile focus is reported on.
        // Keep the raw report separately so focus_workspace_entry still guards
        // the stale window AX reports.
        let reported_focused_before = self.backend.focused_window();
        let focused_before = if repeat == RepeatSwitch::CycleFocus {
            self.resolve_focus(reported_focused_before.clone())
        } else {
            reported_focused_before.clone()
        };
        let switch_started = self.backend.now();
        let mut outgoing = self.workspaces.windows(previous);
        self.order_back_to_front(&mut outgoing, |window| window.window_id);
        let incoming = self.workspaces.windows(workspace);
        log_event!(
            "switch: workspace {} -> {} (outgoing {}, incoming {})",
            previous,
            workspace,
            outgoing.len(),
            incoming.len()
        );
        let frontmost_before = self.backend.frontmost_application();
        if let Some(focused) = focused_before
            .as_ref()
            .filter(|focused| outgoing.contains(focused))
        {
            self.workspaces.remember_focus(previous, focused);
        }
        // Reveal incoming windows first: position writes do not raise, so they
        // arrive and get laid out beneath the outgoing windows. Hiding the
        // outgoing workspace afterwards exposes a fully formed workspace and
        // the bare desktop never shows. Failures before parking leave the
        // screen untouched.
        let incoming_originals: HashMap<_, _> = incoming
            .iter()
            .filter_map(|window| {
                self.parked
                    .get(window)
                    .copied()
                    .map(|frame| (window.clone(), frame))
            })
            .collect();
        let mut restored = Vec::new();
        for window in &incoming {
            if cancelled.load(Ordering::Acquire) {
                log_event!("switch cancelled while revealing incoming windows");
                self.rollback_switch(Vec::new(), &restored, &incoming_originals, output, &outputs);
                return Err("workspace switch cancelled".into());
            }
            if !self.parked.contains_key(window) {
                continue;
            }
            restored.push(window.clone());
            match self.restore_window(window) {
                macos::ApplyOutcome::Applied => {}
                macos::ApplyOutcome::Gone => {
                    self.forget_window(workspace, window);
                }
                outcome => {
                    log_event!(
                        "switch rollback: could not restore {:?} ({:?})",
                        window,
                        outcome
                    );
                    self.rollback_switch(
                        Vec::new(),
                        &restored,
                        &incoming_originals,
                        output,
                        &outputs,
                    );
                    return Err(format!(
                        "Workspace {} could not restore all of its windows",
                        workspace
                    ));
                }
            }
        }
        if !self.wait_for_reveal(&incoming, cancelled) {
            self.rollback_switch(Vec::new(), &restored, &incoming_originals, output, &outputs);
            if cancelled.load(Ordering::Acquire) {
                return Err("workspace switch cancelled".into());
            }
            return Err(format!(
                "Workspace {workspace} stayed inactive because its windows did not finish restoring"
            ));
        }
        let layout = self.layout_workspace(workspace, output, cancelled);
        if cancelled.load(Ordering::Acquire) {
            log_event!("switch cancelled while laying out incoming windows");
            self.rollback_switch(Vec::new(), &incoming, &incoming_originals, output, &outputs);
            return Err("workspace switch cancelled".into());
        }
        if layout.blocked {
            log_event!("switch rollback: workspace {} layout blocked", workspace);
            self.rollback_switch(Vec::new(), &incoming, &incoming_originals, output, &outputs);
            return Err(format!(
                "Workspace {} stayed inactive because its windows could not be laid out",
                workspace
            ));
        }
        let mut parked_outgoing = Vec::new();
        for window in &outgoing {
            if cancelled.load(Ordering::Acquire) {
                log_event!("switch cancelled while parking outgoing windows");
                self.rollback_switch(
                    parked_outgoing,
                    &incoming,
                    &incoming_originals,
                    output,
                    &outputs,
                );
                return Err("workspace switch cancelled".into());
            }
            match self.park_window(window, output, &outputs) {
                macos::ApplyOutcome::Applied => parked_outgoing.push(window.clone()),
                macos::ApplyOutcome::Gone => {
                    self.forget_window(previous, window);
                }
                outcome => {
                    log_event!(
                        "switch rollback: could not park {:?} ({:?})",
                        window,
                        outcome
                    );
                    self.rollback_switch(
                        parked_outgoing,
                        &incoming,
                        &incoming_originals,
                        output,
                        &outputs,
                    );
                    return Err(format!(
                        "Workspace {} stayed active because a window could not be parked",
                        previous
                    ));
                }
            }
        }
        let focus_already_in_workspace = reported_focused_before
            .as_ref()
            .is_some_and(|window| self.workspaces.workspace_of_any(window) == Some(workspace))
            || frontmost_before
                .as_ref()
                .is_some_and(|app| self.workspaces.workspace_for_app(app) == Some(workspace));
        self.workspaces
            .switch_on(&display_id, workspace, Some(previous));
        self.record_focused_workspace(workspace);
        // Re-read incoming windows after reveal/layout so a tile retired as Gone
        // during restore is never chosen over a surviving tile (#308).
        let live_incoming = self.workspaces.windows(workspace);
        let outcome = self.focus_workspace_entry(
            workspace,
            &live_incoming,
            reported_focused_before,
            frontmost_before,
            true,
        );
        if origin == CommandOrigin::Explicit
            || (origin == CommandOrigin::Direct && !focus_already_in_workspace)
        {
            self.follow_pointer_for_workspace(origin, workspace, output, outcome, &outputs);
        }
        self.persist_workspace_state_if_changed()?;
        log_event!(
            "switch complete: workspace {} in {}ms",
            workspace,
            self.backend
                .now()
                .saturating_duration_since(switch_started)
                .as_millis()
        );
        let mut status = self.status(layout.failed, &outputs);
        status.hud = Some(self.hud_for(workspace, UiHudKind::Workspace, Some(previous), &outputs));
        Ok(status)
    }

    /// Focus the workspace's entry window and suppress focus follow for the
    /// focus left behind. That focus stays put when the workspace is empty,
    /// and macOS activates the entry window asynchronously, so for a few
    /// cycles Accessibility can still report the old window or app; following
    /// it would snap the display straight back. Suppression keeps the earlier
    /// stale focus when Accessibility reports none, so switching on through
    /// empty workspaces keeps it too.
    ///
    /// With `keep_reported`, a window of the workspace that already has focus
    /// is the entry window: a hidden workspace revealed because one of its
    /// windows took focus (a notification click, the Dock, Cmd+Tab) must not
    /// hand focus to its remembered tile instead.
    pub(super) fn focus_workspace_entry(
        &mut self,
        workspace: WorkspaceId,
        windows: &[crate::PhysicalWindowId],
        focused: Option<crate::PhysicalWindowId>,
        frontmost: Option<crate::AppInstanceId>,
        keep_reported: bool,
    ) -> WorkspaceFocusOutcome {
        let reported_window = focused.as_ref().filter(|window| windows.contains(window));
        let reported_inside = reported_window.is_some();
        let workspace_focus = reported_window
            .filter(|_| keep_reported)
            .cloned()
            .or_else(|| {
                self.workspaces
                    .preferred_float_focus(workspace)
                    .filter(|window| windows.contains(window))
            })
            .or_else(|| {
                self.workspaces
                    .preferred_focus(workspace)
                    .filter(|window| windows.contains(window))
            })
            .or_else(|| windows.first().cloned());
        let guard = self.focus.guard().cloned().unwrap_or_default();
        let stale_window = focused.or(guard.window);
        let stale_app = frontmost.or(guard.app);
        let outcome = if let Some(window) = workspace_focus {
            let focused = if reported_inside {
                // Focus is already here; nothing lags behind a request.
                let accepted = self.backend.focus_window(&window);
                self.focus.end_request();
                accepted
            } else {
                self.request_focus(&window, [stale_window.clone()])
            };
            if focused {
                self.focus.settle_fullscreen(workspace);
                WorkspaceFocusOutcome::Focused(window)
            } else {
                // Keys may still be in the app focus left; the pending restore
                // stays so the next cycle tries again.
                log_event!(
                    "switch focus failed: workspace {} window {:?}",
                    workspace,
                    window
                );
                WorkspaceFocusOutcome::Failed
            }
        } else {
            self.focus.end_request();
            WorkspaceFocusOutcome::Empty
        };
        // Focus already inside the workspace (focus follow switched to it) is
        // where the display is going, not focus left behind. Only the focus
        // Accessibility reports now proves that: an earlier suppressed window
        // that lives here must not drop the guard on the frontmost app.
        if reported_inside {
            self.clear_suppressed_focus();
        } else {
            self.guard_departure(
                stale_window.filter(|window| !windows.contains(window)),
                stale_app,
            );
        }
        outcome
    }

    /// Bounded wait until every incoming window's restore is confirmed by the
    /// window server (pending restores keep their recovery frames). Slow
    /// applications apply moves asynchronously; hiding the outgoing workspace
    /// before they arrive would flash the bare desktop. Polling is
    /// confirmation-only and starts immediately, so apps that move
    /// synchronously finish without a sleep; the restore write is re-issued
    /// only for windows that still have not arrived after
    /// [`REVEAL_REWRITE_INTERVAL`].
    pub(super) fn wait_for_reveal(
        &mut self,
        incoming: &[crate::PhysicalWindowId],
        cancelled: &AtomicBool,
    ) -> bool {
        let started = self.backend.now();
        let mut last_rewrite = started;
        loop {
            let pending: Vec<_> = incoming
                .iter()
                .filter(|window| self.parked.contains_key(window))
                .cloned()
                .collect();
            if pending.is_empty() {
                return !cancelled.load(Ordering::Acquire);
            }
            if self.backend.now().saturating_duration_since(started) >= REVEAL_SETTLE_LIMIT
                || cancelled.load(Ordering::Acquire)
            {
                log_event!(
                    "reveal interrupted: {} window(s) still arriving; keeping outgoing workspace",
                    pending.len()
                );
                return false;
            }
            let Some(frames) = self.reveal_frames(&pending) else {
                self.backend.pause(REVEAL_POLL_INTERVAL);
                continue;
            };
            let mut removed = Vec::new();
            for window in &pending {
                let Some(target) = self.recovery_target(window) else {
                    continue;
                };
                if frames.get(&window.window_id).is_some_and(|actual| {
                    actual.position_approximately_equals(target, RESTORE_CONFIRM_TOLERANCE)
                }) {
                    removed.extend(self.commit_unparked(window).map(|r| (window.clone(), r)));
                }
            }
            let now = self.backend.now();
            if now.saturating_duration_since(last_rewrite) >= REVEAL_REWRITE_INTERVAL {
                last_rewrite = now;
                for window in &pending {
                    let (Some(record), Some(target)) = (
                        self.parked.record(window).cloned(),
                        self.recovery_target(window),
                    ) else {
                        continue;
                    };
                    if matches!(
                        self.write_restore_position(window, target),
                        macos::ApplyOutcome::Gone
                    ) {
                        removed.push((window.clone(), record));
                    }
                }
            }
            self.persist_removals_or_rollback(removed);
            if incoming
                .iter()
                .any(|window| self.parked.contains_key(window))
            {
                self.backend.pause(REVEAL_POLL_INTERVAL);
            }
        }
    }

    /// Window-server frames to confirm pending reveals against. A revealed
    /// window is on screen, so the on-screen list (about 5x cheaper than the
    /// full one) settles every window it contains; the full list is read only
    /// when a pending window is missing from it (another Space, ordered out),
    /// so such windows confirm exactly as before.
    fn reveal_frames(
        &mut self,
        pending: &[crate::PhysicalWindowId],
    ) -> Option<HashMap<u32, crate::Rect>> {
        if let Some(frames) = self.backend.onscreen_window_frames()
            && pending
                .iter()
                .all(|window| frames.contains_key(&window.window_id))
        {
            return Some(frames);
        }
        self.backend.window_server_frames()
    }

    /// Park the window furthest back first. Hiding the front window first
    /// uncovers the windows stacked beneath it (every Fullscreen Layout tile
    /// shares one frame) until they are parked too, which reads as a flash.
    pub(super) fn order_back_to_front<T>(
        &mut self,
        items: &mut [T],
        window_id: impl Fn(&T) -> u32,
    ) {
        if items.len() < 2 {
            return;
        }
        let Some(order) = self.backend.onscreen_window_order() else {
            return;
        };
        let rank: HashMap<u32, usize> = order
            .iter()
            .enumerate()
            .map(|(rank, id)| (*id, rank))
            .collect();
        // Off-screen windows uncover nothing; they sort first.
        items.sort_by_key(|item| {
            std::cmp::Reverse(rank.get(&window_id(item)).copied().unwrap_or(usize::MAX))
        });
    }

    /// Undo a partially applied switch: bring the outgoing windows back and
    /// hide any incoming windows that were already revealed.
    fn rollback_switch(
        &mut self,
        parked_outgoing: Vec<crate::PhysicalWindowId>,
        revealed: &[crate::PhysicalWindowId],
        incoming_originals: &HashMap<crate::PhysicalWindowId, crate::Rect>,
        output: crate::Rect,
        outputs: &macos::DisplayOutputs,
    ) {
        self.guard_reported_focus();
        for window in parked_outgoing {
            self.restore_window(&window);
        }
        for window in revealed {
            if let Some(&original) = incoming_originals.get(window)
                && !self.mark_parked(window.clone(), original)
            {
                log_event!("rollback mark_parked failed for {:?}", window);
            }
            // A pending restore may land after rollback. Supersede that write
            // even when its recovery entry has not yet been retired.
            let _: macos::ApplyOutcome = self.park_window(window, output, outputs);
            // Fractile revealed this window itself and issued its park. Even
            // when that write could not land (mid Space slide), seeing it on
            // screen right after is not the app or the user summoning it, so it
            // settles like any park and heal leaves it alone (#329).
            self.parked.park_issued(window, self.backend.now());
        }
    }

    /// Global back-and-forth: move focus to the workspace focused before this
    /// one, wherever it lives. A workspace already on screen is focused in
    /// place, so the display the focus came from keeps showing its workspace
    /// (pressing it again bounces straight back). Only a hidden target switches
    /// its own display to it.
    pub(super) fn workspace_back_and_forth(
        &mut self,
        outputs: macos::DisplayOutputs,
        cancelled: &AtomicBool,
    ) -> Result<CycleStatus, String> {
        // Accessibility can still report the window a switch or a cross-display
        // focus just left behind: the effective focus is the one requested,
        // and history names the workspace that took focus (#399).
        let focused = self.current_focus();
        let reported = focused
            .as_ref()
            .filter(|window| self.focus.guarded_window() != Some(*window))
            .and_then(|window| self.workspaces.workspace_of_any(window));
        if let Some(reported) = reported {
            // A click on another display is a genuine focus change even before
            // the next observation cycle records it.
            self.record_focused_workspace(reported);
        }
        let history = self.focus.history();
        let current = reported.or(history.current);
        let target = history
            .previous
            .filter(|target| Some(*target) != current && self.workspaces.is_enabled(*target))
            .ok_or("No previous workspace")?;
        if self.is_visible(target, &outputs) {
            return self.focus_visible_workspace(target, current, &outputs);
        }
        self.switch_with(
            target,
            outputs,
            cancelled,
            RepeatSwitch::KeepFocus,
            CommandOrigin::Explicit,
        )
    }

    /// Focus a workspace that is already on screen instead of switching any
    /// display. An empty one has nothing to focus, so it only shows its HUD and
    /// leaves the history pointing where it was.
    fn focus_visible_workspace(
        &mut self,
        workspace: WorkspaceId,
        from: Option<WorkspaceId>,
        outputs: &macos::DisplayOutputs,
    ) -> Result<CycleStatus, String> {
        let windows = self.workspaces.windows(workspace);
        let output = output_for(&self.workspaces, workspace, outputs);
        if !windows.is_empty() {
            let focused = self.backend.focused_window();
            let frontmost = self.backend.frontmost_application();
            let outcome =
                self.focus_workspace_entry(workspace, &windows, focused, frontmost, false);
            self.follow_pointer_for_workspace(
                CommandOrigin::Explicit,
                workspace,
                output,
                outcome,
                outputs,
            );
            self.record_focused_workspace(workspace);
            log_event!(
                "back-and-forth: focused visible workspace {} without switching",
                workspace
            );
        } else {
            let target_display_id = outputs
                .resolve(self.workspaces.assignment(workspace))
                .id
                .as_str();
            self.follow_pointer_to(
                CommandOrigin::Explicit,
                target_display_id,
                output,
                None,
                outputs,
            );
        }
        let mut status = self.status(0, outputs);
        status.hud = Some(self.hud_for(workspace, UiHudKind::Workspace, from, outputs));
        Ok(status)
    }

    /// Left goes to the next workspace in the display's menu section, right to
    /// the previous one. Nothing happens past either end, while
    /// `settings.trackpad_swipe` is off, or for a finger count other than
    /// [`crate::config::Config::swipe_fingers`] (that one stays with macOS).
    pub(super) fn swipe(
        &mut self,
        swipe: Swipe,
        fingers: Fingers,
        point: (i32, i32),
        outputs: macos::DisplayOutputs,
        cancelled: &AtomicBool,
    ) -> Result<CycleStatus, String> {
        if !self.config.trackpad_swipe() || fingers != self.config.swipe_fingers() {
            return Ok(self.status(0, &outputs));
        }
        let display_id = outputs.display_for_point(point).id.clone();
        let current = self
            .visible_workspaces(&outputs)
            .into_iter()
            .find(|resolved| resolved.display_id == display_id)
            .map(|resolved| resolved.workspace);
        let listed = self.listed_workspaces(&display_id, current, &outputs);
        let target = listed
            .iter()
            .position(|workspace| Some(*workspace) == current)
            .and_then(|index| match swipe {
                Swipe::Left => listed.get(index + 1),
                Swipe::Right => index.checked_sub(1).and_then(|index| listed.get(index)),
            });
        match target {
            Some(&target) => self.switch_with(
                target,
                outputs,
                cancelled,
                RepeatSwitch::KeepFocus,
                CommandOrigin::Passive,
            ),
            None => Ok(self.status(0, &outputs)),
        }
    }
}
