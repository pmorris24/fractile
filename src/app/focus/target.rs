//! Where a directional focus command goes. One resolver serves the command
//! itself and the targets fractile ctl debug and the terminal HUD print, so
//! the diagnostics can never drift from the action (#382): split tiles,
//! Fullscreen Layout stacks, a focused float, an empty workspace and a focus
//! request still in flight all resolve the same way in both.

use super::super::commands::{CommandOrigin, rect_center};
use super::super::*;

impl<B: WorkspaceBackend> WorkspaceRuntime<B> {
    pub(in crate::app) fn focus_relative(
        &mut self,
        direction: Direction,
        outputs: macos::DisplayOutputs,
        cancelled: &AtomicBool,
    ) -> Result<CycleStatus, String> {
        // While an empty workspace chosen on another display stands for focus,
        // origin is None but Accessibility keeps reporting the window that kept
        // focus; the request must treat that lagging report as stale too.
        let left = self
            .focus
            .empty_choice()
            .and_then(|choice| choice.left.clone());
        let origin = self.current_focus();
        if let Some(window) = &origin
            && self.workspaces.workspace_of(window).is_none()
        {
            // A window reconciliation has not adopted yet may be a new tile.
            // A failed refresh only means navigating from where focus is now.
            if let Err(error) = self.cycle(outputs.clone(), cancelled) {
                log_event!("focus: refresh before navigating failed: {error}");
            }
        }
        let Some(target) = self.focus_target(origin.as_ref(), direction, &outputs) else {
            return Ok(self.status(0, &outputs));
        };
        self.focus_tile(origin.or(left), &target, &outputs, CommandOrigin::Explicit)?;
        Ok(self.status(0, &outputs))
    }

    /// The tile a directional focus command moves to from origin, the
    /// effective focus. None when the command would request no focus.
    pub(in crate::app) fn focus_target(
        &mut self,
        origin: Option<&crate::PhysicalWindowId>,
        direction: Direction,
        outputs: &macos::DisplayOutputs,
    ) -> Option<crate::PhysicalWindowId> {
        let managed = origin
            .and_then(|window| self.workspaces.workspace_of(window))
            .filter(|workspace| self.is_visible(*workspace, outputs));
        match (origin, managed) {
            (Some(focused), Some(workspace)) => self
                .stacked_target(workspace, focused, direction)
                .or_else(|| {
                    self.spatial_target(focused, direction, outputs)
                        .map(|(_, target)| target)
                }),
            // Focus is on a floating window, dialog or popup, or nowhere: the
            // key brings it back onto the tiles instead of doing nothing.
            _ => self.rescue_target(origin, direction, outputs),
        }
    }

    fn rescue_target(
        &mut self,
        origin: Option<&crate::PhysicalWindowId>,
        direction: Direction,
        outputs: &macos::DisplayOutputs,
    ) -> Option<crate::PhysicalWindowId> {
        let tiles = self.navigation_tiles(outputs);
        // A parked or guarded window left behind after switching to an empty
        // workspace sits off a display corner (possibly another display's), so
        // never navigate from its off-screen frame (#355).
        let on_screen_origin = origin.filter(|window| {
            !self.parked.contains_key(window)
                && self.focus.guarded_window() != Some(*window)
                && self
                    .workspaces
                    .workspace_of_any(window)
                    .is_none_or(|workspace| self.is_visible(workspace, outputs))
        });
        if let Some(frame) = on_screen_origin.and_then(|window| {
            self.backend
                .window_server_frame(window)
                .or_else(|| self.backend.frame(window).ok())
        }) {
            let from_display = outputs
                .display_at(rect_center(frame))
                .map(|display| display.frame);
            return crate::navigation::rescue_from_display(&tiles, frame, from_display, direction)
                .map(|tile| tile.window.clone());
        }
        // When focus sits on an empty workspace (or nowhere), navigate from the
        // display the user is currently on in the requested direction (#355).
        let current_display = self
            .focus
            .history()
            .current
            .filter(|workspace| self.is_visible(*workspace, outputs))
            .map(|workspace| output_for(&self.workspaces, workspace, outputs))
            .or_else(|| {
                self.backend
                    .pointer_location()
                    .and_then(|point| outputs.display_at(point))
                    .map(|display| display.frame)
            })
            .or_else(|| {
                origin
                    .and_then(|window| self.workspaces.workspace_of_any(window))
                    .map(|workspace| output_for(&self.workspaces, workspace, outputs))
            });
        if let Some(display_frame) = current_display {
            return crate::navigation::rescue_from_display(
                &tiles,
                display_frame,
                Some(display_frame),
                direction,
            )
            .map(|tile| tile.window.clone());
        }
        self.visible_workspaces(outputs)
            .iter()
            .find_map(|resolved| {
                self.workspaces
                    .preferred_focus(resolved.workspace)
                    .filter(|window| tiles.iter().any(|tile| &tile.window == window))
                    .or_else(|| {
                        tiles
                            .iter()
                            .find(|tile| tile.workspace == resolved.workspace)
                            .map(|tile| tile.window.clone())
                    })
            })
    }

    /// Fullscreen Layout tiles share one frame, so spatial navigation cannot
    /// tell them apart. Left/up and right/down instead step backward and
    /// forward through the stable tile order, without wrapping, so the ends of
    /// the stack still lead to the adjacent display.
    fn stacked_target(
        &self,
        workspace: WorkspaceId,
        focused: &crate::PhysicalWindowId,
        direction: Direction,
    ) -> Option<crate::PhysicalWindowId> {
        if !self.workspaces.deck(workspace).is_monocle() {
            return None;
        }
        let windows = self.workspaces.engine(workspace).bound_windows();
        let index = windows.iter().position(|window| window == focused)?;
        let target = match direction {
            Direction::Left | Direction::Up => index.checked_sub(1)?,
            Direction::Right | Direction::Down => index + 1,
        };
        windows.get(target).cloned()
    }

    /// Focuses target for a focus command issued from origin and lets the
    /// pointer follow it.
    pub(in crate::app) fn focus_tile(
        &mut self,
        origin: Option<crate::PhysicalWindowId>,
        target: &crate::PhysicalWindowId,
        outputs: &macos::DisplayOutputs,
        command: CommandOrigin,
    ) -> Result<(), String> {
        if !self.request_focus(target, [origin]) {
            return Err("Target window could not be focused".into());
        }
        if let Some(workspace) = self.workspaces.workspace_of(target) {
            self.remember_fullscreen_focus(workspace, target)?;
        }
        if let Some(workspace) = self.workspaces.workspace_of_any(target) {
            self.record_focused_workspace(workspace);
            let display = outputs.resolve(self.workspaces.assignment(workspace));
            let output = display.frame;
            let frame = self.focus_window_frame(target, workspace, output);
            self.follow_pointer_to(command, &display.id, output, frame, outputs);
        }
        Ok(())
    }
}
