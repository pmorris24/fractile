use super::*;

mod drag;
mod float;
mod float_drop;
mod gesture;
mod sizing;
#[cfg(test)]
pub(super) use drag::{DISPLAY_EDGE_DROP_BAND, display_edge_bands, display_row_bands};
pub(super) use gesture::GestureOverlay;

/// Who asked for a command, which decides whether the pointer follows the
/// focus it moves. It is passed down explicitly, so an operation nested in
/// another (a refresh cycle, a follow switch) says for itself whether it warps.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum CommandOrigin {
    /// A method called directly, outside `execute_action` (unit tests). A
    /// switch warps only when focus was not already on the workspace.
    Direct,
    /// An explicit keyboard or `ctl` focus-moving action: the pointer follows.
    Explicit,
    /// A background cycle, gesture, swipe, menu selection or action that moves
    /// no focus: the pointer never moves.
    Passive,
}

impl CommandOrigin {
    /// The origin of a worker action dispatched from the app loop.
    pub(super) const fn of(action: &WorkerAction) -> Self {
        match action {
            WorkerAction::SwitchWorkspace(_)
            | WorkerAction::WorkspaceBackAndForth
            | WorkerAction::MoveWorkspaceToDisplay(_)
            | WorkerAction::CycleFocus
            | WorkerAction::CycleFocusPrevious
            | WorkerAction::Focus(_)
            | WorkerAction::MoveFocused(_)
            | WorkerAction::MoveFocusedFollow(_)
            | WorkerAction::StepWorkspace(_)
            | WorkerAction::MoveFocusedStep { .. }
            | WorkerAction::MoveTab { .. } => Self::Explicit,
            _ => Self::Passive,
        }
    }
}

pub(super) enum WorkspaceFocusOutcome {
    Focused(crate::PhysicalWindowId),
    Empty,
    Failed,
}

pub(super) const fn rect_center(rect: crate::Rect) -> (i32, i32) {
    (rect.x + rect.width / 2, rect.y + rect.height / 2)
}

impl<B: WorkspaceBackend> WorkspaceRuntime<B> {
    pub(super) fn focus_window_frame(
        &mut self,
        window: &crate::PhysicalWindowId,
        workspace: WorkspaceId,
        output: crate::Rect,
    ) -> Option<crate::Rect> {
        // A tile that kept a different frame than it was given (a minimum
        // size, an aspect ratio) is where the window is, not its tile (#418).
        if let Some((_, accepted)) = self.constrained[workspace.index()].get(window) {
            return Some(*accepted);
        }
        let settings = self.layout_settings(output);
        let mut placements = self.workspaces.engine(workspace).placements(&settings);
        self.workspaces
            .deck(workspace)
            .place(&mut placements, settings.single());
        let tile_frame = placements
            .into_iter()
            .find(|placement| placement.window.as_ref() == Some(window))
            .map(|placement| placement.frame);
        tile_frame.or_else(|| {
            self.backend
                .window_server_frame(window)
                .or_else(|| self.backend.frame(window).ok())
        })
    }

    pub(super) fn follow_pointer_to(
        &mut self,
        origin: CommandOrigin,
        target_display_id: &str,
        output: crate::Rect,
        focused_window: Option<crate::Rect>,
        outputs: &macos::DisplayOutputs,
    ) {
        let mode = self.config.mouse_follows_focus();
        if mode == crate::config::MouseFollowsFocus::Off
            || origin == CommandOrigin::Passive
            || self.backend.any_mouse_button_down()
        {
            return;
        }
        let target_rect = focused_window.unwrap_or(output);
        let target_display_id = focused_window
            .and_then(|frame| outputs.display_at(rect_center(frame)))
            .map_or(target_display_id, |display| display.id.as_str());
        let already_there = self
            .backend
            .pointer_location()
            .is_some_and(|(x, y)| match mode {
                crate::config::MouseFollowsFocus::Off => true,
                crate::config::MouseFollowsFocus::Display => outputs
                    .display_at((x, y))
                    .is_some_and(|display| display.id == target_display_id),
                crate::config::MouseFollowsFocus::Window => target_rect.contains(x, y),
            });
        if already_there {
            return;
        }
        let point = rect_center(target_rect);
        if self.backend.warp_pointer(point) {
            self.focus.note_warp(point);
        }
    }

    pub(super) fn follow_pointer_for_workspace(
        &mut self,
        origin: CommandOrigin,
        workspace: WorkspaceId,
        output: crate::Rect,
        outcome: WorkspaceFocusOutcome,
        outputs: &macos::DisplayOutputs,
    ) {
        let target_display_id = outputs
            .resolve(self.workspaces.assignment(workspace))
            .id
            .as_str();
        match outcome {
            WorkspaceFocusOutcome::Focused(window) => {
                let frame = self.focus_window_frame(&window, workspace, output);
                self.follow_pointer_to(origin, target_display_id, output, frame, outputs);
            }
            WorkspaceFocusOutcome::Empty => {
                self.follow_pointer_to(origin, target_display_id, output, None, outputs);
            }
            WorkspaceFocusOutcome::Failed => {}
        }
    }
    pub(super) fn reorder_focused(
        &mut self,
        direction: Direction,
        outputs: macos::DisplayOutputs,
        cancelled: &AtomicBool,
    ) -> Result<CycleStatus, String> {
        let (window, workspace) = self.focused_active_workspace(&outputs, cancelled, true)?;
        self.workspaces
            .deck(workspace)
            .require_split("reordering")?;
        let windows = self.workspaces.engine(workspace).bound_windows();
        let axis = self.workspaces.engine(workspace).root_axis();
        let perpendicular = matches!(
            (axis, direction),
            (crate::Axis::Horizontal, Direction::Down | Direction::Up)
                | (crate::Axis::Vertical, Direction::Left | Direction::Right)
        );
        let tiles = self.navigation_tiles(&outputs);
        let local_tiles = tiles
            .iter()
            .filter(|tile| tile.workspace == workspace)
            .cloned()
            .collect::<Vec<_>>();
        if let Some(target) = crate::navigation::target(&local_tiles, &window, direction) {
            if !self
                .workspaces
                .engine_mut(workspace)
                .swap_windows(&window, &target.window)
            {
                return Ok(self.status(0, &outputs));
            }
            return self.relayout_persist(workspace, &outputs, cancelled);
        }
        if windows.len() >= 2
            && perpendicular
            && self
                .workspaces
                .engine_mut(workspace)
                .create_implicit_split(&window, direction)
        {
            return self.relayout_persist(workspace, &outputs, cancelled);
        }
        let mut candidates = self.empty_display_targets(&outputs, &tiles);
        candidates.extend(tiles);
        let Some(target) = crate::navigation::target(&candidates, &window, direction) else {
            return Ok(self.status(0, &outputs));
        };
        self.move_window(window, target.workspace, outputs, cancelled)
    }

    /// A display whose workspace has no tiles offers no window to move toward,
    /// so its single-tile frame stands in for one. The stand-in's window id
    /// is never a real window; only its workspace is used.
    fn empty_display_targets(
        &self,
        outputs: &macos::DisplayOutputs,
        tiles: &[NavigationTile],
    ) -> Vec<NavigationTile> {
        self.visible_workspaces(outputs)
            .into_iter()
            .filter(|resolved| {
                self.scope.manages_selector(&resolved.selector)
                    && !tiles
                        .iter()
                        .any(|tile| tile.workspace == resolved.workspace)
            })
            .map(|resolved| {
                let settings = self.layout_settings(resolved.output);
                NavigationTile {
                    workspace: resolved.workspace,
                    window: crate::PhysicalWindowId {
                        app: crate::AppInstanceId {
                            pid: -1,
                            launched_at: 0,
                        },
                        window_id: u32::try_from(resolved.workspace.index()).unwrap_or(u32::MAX),
                    },
                    frame: settings.single(),
                    display: Some(settings.usable()),
                }
            })
            .collect()
    }

    pub(super) fn cycle_focus(
        &mut self,
        outputs: macos::DisplayOutputs,
        cancelled: &AtomicBool,
    ) -> Result<CycleStatus, String> {
        self.cycle_focus_by(outputs, cancelled, false)
    }

    pub(super) fn cycle_focus_previous(
        &mut self,
        outputs: macos::DisplayOutputs,
        cancelled: &AtomicBool,
    ) -> Result<CycleStatus, String> {
        self.cycle_focus_by(outputs, cancelled, true)
    }

    /// Steps through the workspace's tiles in stable order, then its floats,
    /// wrapping either way, so a workspace of floats (or one tile plus floats)
    /// can still be cycled. Focus outside that ring enters at the first window
    /// going forward and the last going backward.
    fn cycle_focus_by(
        &mut self,
        outputs: macos::DisplayOutputs,
        cancelled: &AtomicBool,
        backward: bool,
    ) -> Result<CycleStatus, String> {
        let (focused, workspace) = self.focused_owner_workspace(&outputs, cancelled)?;
        let windows = self.workspaces.windows(workspace);
        let len = windows.len();
        let target = match windows.iter().position(|window| window == &focused) {
            Some(_) if len < 2 => return Ok(self.status(0, &outputs)),
            Some(index) if backward => windows[(index + len - 1) % len].clone(),
            Some(index) => windows[(index + 1) % len].clone(),
            None => match if backward {
                windows.last()
            } else {
                windows.first()
            } {
                Some(target) => target.clone(),
                None => return Ok(self.status(0, &outputs)),
            },
        };
        self.focus_tile(Some(focused), &target, &outputs, CommandOrigin::Explicit)?;
        Ok(self.status(0, &outputs))
    }

    fn focused_active_workspace(
        &mut self,
        outputs: &macos::DisplayOutputs,
        cancelled: &AtomicBool,
        refresh_if_unmanaged: bool,
    ) -> Result<(crate::PhysicalWindowId, WorkspaceId), String> {
        let window = self
            .current_focus()
            .ok_or("Focused window is unavailable")?;
        let workspace =
            self.active_workspace_of(&window, outputs, cancelled, refresh_if_unmanaged)?;
        Ok((window, workspace))
    }

    /// The visible workspace that owns the focused window, as a tile or a
    /// float, for commands that act on the whole workspace.
    fn focused_owner_workspace(
        &mut self,
        outputs: &macos::DisplayOutputs,
        cancelled: &AtomicBool,
    ) -> Result<(crate::PhysicalWindowId, WorkspaceId), String> {
        let window = self
            .current_focus()
            .ok_or("Focused window is unavailable")?;
        if self.workspaces.workspace_of_any(&window).is_none() {
            self.cycle(outputs.clone(), cancelled)?;
        }
        let workspace = self
            .workspaces
            .workspace_of_any(&window)
            .ok_or("Focused window is not managed by Fractile")?;
        if !self.is_visible(workspace, outputs) {
            return Err("Focused window is not on an active workspace".into());
        }
        Ok((window, workspace))
    }

    fn active_workspace_of(
        &mut self,
        window: &crate::PhysicalWindowId,
        outputs: &macos::DisplayOutputs,
        cancelled: &AtomicBool,
        refresh_if_unmanaged: bool,
    ) -> Result<WorkspaceId, String> {
        let workspace = match self.workspaces.workspace_of(window) {
            Some(workspace) => workspace,
            None if refresh_if_unmanaged => {
                self.cycle(outputs.clone(), cancelled)?;
                self.workspaces
                    .workspace_of(window)
                    .ok_or("Focused window is not managed by Fractile")?
            }
            None => return Err("Focused window is not managed by Fractile".into()),
        };
        if !self.is_visible(workspace, outputs) {
            return Err("Focused window is not on an active workspace".into());
        }
        Ok(workspace)
    }

    pub(super) fn remember_fullscreen_focus(
        &mut self,
        workspace: WorkspaceId,
        window: &crate::PhysicalWindowId,
    ) -> Result<(), String> {
        if !self.workspaces.deck(workspace).is_monocle() {
            return Ok(());
        }
        self.focus.settle_fullscreen(workspace);
        self.workspaces.remember_focus(workspace, window);
        self.persist_workspace_state_if_changed()
    }

    pub(super) fn toggle_layout(
        &mut self,
        outputs: macos::DisplayOutputs,
        cancelled: &AtomicBool,
    ) -> Result<CycleStatus, String> {
        let (_, workspace) = self.focused_owner_workspace(&outputs, cancelled)?;
        if self.workspaces.deck(workspace).is_monocle() {
            self.workspaces
                .set_deck(workspace, crate::workspace::DeckMode::Split);
            self.focus.settle_fullscreen(workspace);
        }
        self.workspaces.engine_mut(workspace).toggle_axis();
        let axis = self.workspaces.engine(workspace).root_axis();
        let mut status = self.relayout_persist(workspace, &outputs, cancelled)?;
        status.hud = Some(self.hud_for(workspace, UiHudKind::Layout { axis }, None, &outputs));
        Ok(status)
    }

    pub(super) fn toggle_fullscreen_layout(
        &mut self,
        outputs: macos::DisplayOutputs,
        cancelled: &AtomicBool,
    ) -> Result<CycleStatus, String> {
        let (window, workspace) = self.focused_owner_workspace(&outputs, cancelled)?;
        self.workspaces.remember_focus(workspace, &window);
        let deck = self.workspaces.deck(workspace).toggled();
        self.workspaces.set_deck(workspace, deck);
        let enabled = deck.is_monocle();
        self.focus.settle_fullscreen(workspace);
        let mut status = self.relayout_persist(workspace, &outputs, cancelled)?;
        status.hud =
            Some(self.hud_for(workspace, UiHudKind::Fullscreen { enabled }, None, &outputs));
        Ok(status)
    }

    fn relayout_persist(
        &mut self,
        workspace: WorkspaceId,
        outputs: &macos::DisplayOutputs,
        cancelled: &AtomicBool,
    ) -> Result<CycleStatus, String> {
        let layout = self.layout_workspace(
            workspace,
            output_for(&self.workspaces, workspace, outputs),
            cancelled,
        );
        self.persist_workspace_state_if_changed()?;
        Ok(self.status(layout.failed, outputs))
    }
}
