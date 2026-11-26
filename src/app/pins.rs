//! Pinned floats at runtime: `fractile ctl pin` and the parking exemption a pin carries.
//!
//! Ownership lives in [`crate::workspace::Workspaces`]; this module only decides what a pin
//! means for parking and focus: a pinned window is never parked by a switch (so it shows on
//! every workspace of its display) and never pulls focus to its owning workspace. Pause, quit
//! and restart still restore every parked window, a pinned one included.

use super::*;

impl<B: WorkspaceBackend> WorkspaceRuntime<B> {
    /// Whether `window` stays on screen through workspace switches: it is pinned, or its app
    /// keeps un-parking it.
    pub(super) fn stays_on_screen(&self, window: &crate::PhysicalWindowId) -> bool {
        self.parked.is_exempt(window) || self.workspaces.is_pinned(window)
    }

    /// What rules and defaults say about an unobserved window, judged by its app's bundle alone
    /// (a title rule cannot be checked without a title). `None` for an app never observed.
    pub(super) fn bundle_disposition(
        &self,
        window: &crate::PhysicalWindowId,
        frame: crate::Rect,
    ) -> Option<crate::config::DispositionPatch> {
        let bundle_id = self.memory.bundles.get(&window.app)?;
        let probe = crate::ObservedWindow {
            id: window.clone(),
            frame,
            resizable: true,
            title: None,
            document: None,
        };
        Some(self.config.patch(bundle_id, &probe))
    }

    /// A `pin` rule that arrived while a float was parked: no observation lists a parked
    /// window, so judge it by its app's bundle and let restore_visible bring it back.
    pub(super) fn pin_parked_floats_by_rule(&mut self) {
        let parked: Vec<_> = self
            .parked
            .iter()
            .filter(|(window, _)| {
                self.workspaces.floating_workspace_of(window).is_some()
                    && !self.workspaces.is_pinned(window)
            })
            .map(|(window, frame)| (window.clone(), *frame))
            .collect();
        for (window, frame) in parked {
            if self
                .bundle_disposition(&window, frame)
                .is_some_and(crate::config::DispositionPatch::pins)
            {
                log_event!("pin rule now matches parked {:?}; showing it", window);
                self.workspaces.set_rule_pinned(&window, true);
            }
        }
    }

    /// Pin the focused window on every workspace (floating it first if it is tiled), or drop
    /// its hand pin.
    pub(super) fn toggle_pin(
        &mut self,
        outputs: macos::DisplayOutputs,
        cancelled: &AtomicBool,
    ) -> Result<CycleStatus, String> {
        let window = self
            .current_focus()
            .ok_or("Focused window is unavailable")?;
        if self.workspaces.workspace_of_any(&window).is_none() {
            // A window observed for the first time may not be bound yet.
            self.cycle(outputs.clone(), cancelled)?;
        }
        let workspace = self
            .workspaces
            .workspace_of_any(&window)
            .ok_or("Focused window is not managed by Fractile")?;
        if self.workspaces.is_pinned_by_hand(&window) {
            self.workspaces.unpin(&window);
            // Keep it where it is seen: the workspace now showing it on its display owns it.
            if let Some(output) = self.window_display_output(&window, &outputs)
                && let Some(shown) = self
                    .visible_workspaces(&outputs)
                    .into_iter()
                    .find(|resolved| resolved.output == output)
            {
                self.workspaces
                    .own_floating(window.clone(), shown.workspace);
            }
            log_event!("unpin {:?}", window);
            self.persist_workspace_state_if_changed()?;
            return Ok(self.status(0, &outputs));
        }
        if self.workspaces.is_pinned(&window) {
            return Err("Focused window is pinned by a window rule".into());
        }
        if !self.is_visible(workspace, &outputs) {
            return Err("Focused window is not on an active workspace".into());
        }
        if self.workspaces.workspace_of(&window).is_some() {
            self.float_window(window.clone(), workspace, outputs.clone(), cancelled)?;
        }
        if !self.workspaces.pin(window.clone()) {
            return Err("Focused window could not float, so it cannot be pinned".into());
        }
        log_event!("pin {:?} (workspace {})", window, workspace);
        self.persist_workspace_state_if_changed()?;
        Ok(self.status(0, &outputs))
    }
}
