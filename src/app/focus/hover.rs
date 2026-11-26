//! Focus follows mouse (`[labs] focus_follows_mouse`), the worker's half. The
//! main thread only times how long the pointer rests and asks the window server
//! which window is in front there; this side decides whether that window is a
//! visible tile and focuses it. Floats, the Scratchpad and unmanaged windows
//! never take focus by hover, and the pointer is never moved.

use super::super::commands::CommandOrigin;
use super::super::*;

impl<B: WorkspaceBackend> WorkspaceRuntime<B> {
    /// Focuses the visible tile `window_id` the pointer rests on at `point`,
    /// unless the Lab is off, the pointer has moved on, a button is held, a
    /// drag or a Space slide is under way, or Fractile put the pointer there.
    pub(in crate::app) fn focus_hovered(
        &mut self,
        window_id: u32,
        point: (i32, i32),
        outputs: macos::DisplayOutputs,
    ) -> Result<CycleStatus, String> {
        if !self.config.labs().focus_follows_mouse
            || self.focus.warped_to(point)
            || self.gesture.is_some()
            || self.backend.any_mouse_button_down()
            || self.backend.pointer_location() != Some(point)
            || self.space_transition_until().is_some()
        {
            return Ok(self.status(0, &outputs));
        }
        let Some(target) = self.visible_tile(window_id, &outputs) else {
            return Ok(self.status(0, &outputs));
        };
        let focused = self.current_focus();
        if focused.as_ref() == Some(&target) {
            return Ok(self.status(0, &outputs));
        }
        // Best effort: a window that refuses focus (an unresponsive app) keeps
        // focus where it is, without an error every time the pointer rests on it.
        self.focus_tile(focused, &target, &outputs, CommandOrigin::Passive)
            .unwrap_or(());
        Ok(self.status(0, &outputs))
    }

    /// The tile bound to window-server window `window_id` on a visible,
    /// managed workspace.
    fn visible_tile(
        &self,
        window_id: u32,
        outputs: &macos::DisplayOutputs,
    ) -> Option<crate::PhysicalWindowId> {
        self.visible_workspaces(outputs)
            .into_iter()
            .filter(|resolved| self.scope.manages_selector(&resolved.selector))
            .flat_map(|resolved| self.workspaces.engine(resolved.workspace).bound_windows())
            .find(|window| window.window_id == window_id)
    }
}
