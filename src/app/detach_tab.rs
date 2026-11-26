//! Send Tab to Workspace (`move-tab`): presses the app's own "Move Tab to New
//! Window", then claims the window that press opens for the target workspace
//! the first time an observation sees it. A window with a single tab (or an
//! app without native tabs) moves whole, like `move`.

use super::commands::CommandOrigin;
use super::switching::RepeatSwitch;
use super::*;

/// How long a pressed detach waits for its new window before it is dropped.
pub(super) const TAB_DETACH_TIMEOUT: Duration = Duration::from_millis(800);
/// How soon to observe again while a detach waits for its window.
pub(super) const TAB_DETACH_RECHECK: Duration = Duration::from_millis(15);
/// Frames within this many points are the same place on screen.
const SAME_PLACE: i32 = 8;

/// A pressed "Move Tab to New Window" awaiting the window it opens.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct PendingTabDetach {
    pub(super) app: AppInstanceId,
    pub(super) source_window: crate::PhysicalWindowId,
    pub(super) source_workspace: WorkspaceId,
    pub(super) target_workspace: WorkspaceId,
    pub(super) follow: bool,
    pub(super) expires_at: Instant,
    /// Where `source_window` was before the press.
    source_frame: Option<crate::Rect>,
    /// Every window on screen before the press; none of them is the new one.
    onscreen_before: HashSet<u32>,
    phase: DetachPhase,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum DetachPhase {
    /// The press went through; no new window has been observed yet.
    Awaiting,
    /// This cycle routed `detached` to the target; `remaining` holds the
    /// source slot. Focus settles at the end of the cycle.
    Claimed {
        detached: crate::PhysicalWindowId,
        remaining: crate::PhysicalWindowId,
    },
}

impl<B: WorkspaceBackend> WorkspaceRuntime<B> {
    pub(super) fn move_tab(
        &mut self,
        target: WorkspaceId,
        follow: bool,
        outputs: macos::DisplayOutputs,
        cancelled: &AtomicBool,
    ) -> Result<CycleStatus, String> {
        self.ensure_managed_workspace(target)?;
        if let Some(superseded) = self.tab_detach.take() {
            log_event!(
                "move tab: dropped the detach of {:?}: a newer move tab began",
                superseded.source_window
            );
        }
        let window = self
            .current_focus()
            .ok_or("Focused window is unavailable")?;
        let source = if let Some(source) = self.workspaces.workspace_of_any(&window) {
            source
        } else {
            self.cycle(outputs.clone(), cancelled)?;
            self.workspaces
                .workspace_of_any(&window)
                .ok_or("Window is not managed by Fractile")?
        };
        let onscreen_before = self
            .backend
            .onscreen_window_frames()
            .map(|frames| frames.into_keys().collect())
            .unwrap_or_default();
        let source_frame = self.backend.frame(&window).ok();
        match self.backend.detach_tab(&window) {
            macos::TabDetachResult::SingleTabOrUnsupported => {
                log_event!("move tab: {window:?} has no tab to detach; moving the window");
                if follow {
                    self.move_focused_follow(target, outputs, cancelled)
                } else {
                    self.move_focused(target, outputs, cancelled)
                }
            }
            macos::TabDetachResult::Error(error) => {
                Err(format!("Move Tab to New Window failed: {error}"))
            }
            macos::TabDetachResult::Detached => {
                log_event!("move tab: detached a tab of {window:?} for workspace {target}");
                self.tab_detach = Some(PendingTabDetach {
                    app: window.app.clone(),
                    source_window: window,
                    source_workspace: source,
                    target_workspace: target,
                    follow,
                    expires_at: self.backend.now() + TAB_DETACH_TIMEOUT,
                    source_frame,
                    onscreen_before,
                    phase: DetachPhase::Awaiting,
                });
                self.cycle(outputs, cancelled)
            }
        }
    }

    /// Before `app`'s window affinities freeze: its first window that was not on
    /// screen before the press, unowned and not ignored, came from the detach.
    /// Returns the window to route to the target and the target.
    ///
    /// Chromium opens the tab in that new window. An AppKit tab group instead
    /// moves the tab's own window (`source_window`) out and shows its next
    /// tab's window in place, so that one takes over the source slot.
    pub(super) fn claim_detached_tab(
        &mut self,
        app: &crate::ObservedApplication,
        windows: &[crate::ObservedWindow],
    ) -> Option<(crate::PhysicalWindowId, WorkspaceId)> {
        let pending = self.tab_detach.as_ref()?;
        if pending.app != app.id
            || pending.phase != DetachPhase::Awaiting
            || self.backend.now() >= pending.expires_at
        {
            return None;
        }
        let new = windows.iter().find(|window| {
            window.id != pending.source_window
                && !pending.onscreen_before.contains(&window.id.window_id)
                && self.workspaces.residency(&window.id).is_none()
                && !self.parked.contains_key(&window.id)
                && !self.config.patch(&app.bundle_id, window).is_ignored()
        })?;
        let source = pending.source_window.clone();
        let target = pending.target_workspace;
        let source_now = windows
            .iter()
            .find(|window| window.id == source)
            .map(|window| window.frame);
        let tab_group = match (pending.source_frame, source_now) {
            (Some(before), Some(now)) => {
                match (
                    new.frame.approximately_equals(before, SAME_PLACE),
                    now.approximately_equals(before, SAME_PLACE),
                ) {
                    (true, false) => true,
                    (false, true) => false,
                    // Both or neither in place: AppKit keeps the moved tab's
                    // window key, Chromium focuses its new window.
                    _ => self.backend.focused_window().as_ref() == Some(&source),
                }
            }
            _ => false,
        } && self.hand_slot_to(&source, &new.id);
        let (detached, remaining) = if tab_group {
            (source, new.id.clone())
        } else {
            (new.id.clone(), source)
        };
        log_event!(
            "move tab: claimed {detached:?} for workspace {target} ({}); {remaining:?} stays",
            if tab_group { "tab group" } else { "new window" }
        );
        if let Some(pending) = self.tab_detach.as_mut() {
            pending.phase = DetachPhase::Claimed {
                detached: detached.clone(),
                remaining,
            };
        }
        Some((detached, target))
    }

    /// Gives `from`'s tile or float to `to`, leaving `from` unowned.
    fn hand_slot_to(
        &mut self,
        from: &crate::PhysicalWindowId,
        to: &crate::PhysicalWindowId,
    ) -> bool {
        if let Some(owner) = self.workspaces.workspace_of(from) {
            if !self.workspaces.engine_mut(owner).rebind_window(from, to) {
                return false;
            }
            self.forget_window_state(owner, from);
            return true;
        }
        self.workspaces.hand_float_to(from, to)
    }

    /// Per-window evidence about `window` on `workspace`, which no longer owns it.
    fn forget_window_state(&mut self, workspace: WorkspaceId, window: &crate::PhysicalWindowId) {
        self.constrained[workspace.index()].remove(window);
        self.applied[workspace.index()].remove(window);
        self.apply_attempts[workspace.index()].remove(window);
        self.minimum_sizes.remove(window);
    }

    /// At the end of a cycle: hands focus where `move-tab` promised once the
    /// detached window is claimed, keeps waiting (with a prompt re-observation)
    /// until the detach expires, and then drops it.
    pub(super) fn settle_tab_detach(
        &mut self,
        mut status: CycleStatus,
        outputs: &macos::DisplayOutputs,
        cancelled: &AtomicBool,
    ) -> Result<CycleStatus, String> {
        let Some(pending) = self.tab_detach.take() else {
            return Ok(status);
        };
        let (detached, remaining) = match &pending.phase {
            DetachPhase::Claimed {
                detached,
                remaining,
            } => (detached.clone(), remaining.clone()),
            DetachPhase::Awaiting if self.backend.now() >= pending.expires_at => {
                log_event!(
                    "move tab: no window appeared for the tab detached from {:?}; dropped",
                    pending.source_window
                );
                return Ok(status);
            }
            DetachPhase::Awaiting => {
                let recheck = self.backend.now() + TAB_DETACH_RECHECK;
                status.close_recheck =
                    Some(status.close_recheck.map_or(recheck, |at| at.min(recheck)));
                self.tab_detach = Some(pending);
                return Ok(status);
            }
        };
        let target = pending.target_workspace;
        if self.workspaces.workspace_of_any(&detached) != Some(target) {
            log_event!("move tab: {detached:?} did not reach workspace {target}");
            return Ok(status);
        }
        // Routing already put it on the target's display, parked there if the target is hidden.
        let target_visible = self.is_visible(target, outputs);
        if pending.follow && !target_visible {
            self.workspaces.remember_focus(target, &detached);
            let failed = status.failed;
            status = self.switch_with(
                target,
                outputs.clone(),
                cancelled,
                RepeatSwitch::KeepFocus,
                CommandOrigin::Explicit,
            )?;
            status.failed += failed;
        } else {
            let focus = if pending.follow {
                &detached
            } else {
                &remaining
            };
            let reported = self.backend.focused_window();
            if self.request_focus(focus, [reported]) {
                if pending.follow {
                    self.record_focused_workspace(target);
                    // The pointer follows the tab the command sent and
                    // followed, onto its display (#417).
                    let display = outputs.resolve(self.workspaces.assignment(target));
                    let (display_id, output) = (display.id.clone(), display.frame);
                    let frame = self.focus_window_frame(focus, target, output);
                    self.follow_pointer_to(
                        CommandOrigin::Explicit,
                        &display_id,
                        output,
                        frame,
                        outputs,
                    );
                }
            } else {
                log_event!("move tab: could not focus {focus:?}");
            }
            if !target_visible {
                // Focus follow must not chase the parked tab back to its workspace.
                self.guard_departure(Some(detached.clone()), Some(detached.app.clone()));
            }
        }
        self.persist_workspace_state_if_changed()?;
        status.hud = Some(self.hud_for(target, UiHudKind::Moved, None, outputs));
        Ok(status)
    }
}
