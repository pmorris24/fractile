use super::*;

enum MoveWindowOutcome {
    Moved(CycleStatus),
    Retired(CycleStatus),
}

impl<B: WorkspaceBackend> WorkspaceRuntime<B> {
    pub(in crate::app) fn move_focused(
        &mut self,
        target: WorkspaceId,
        outputs: macos::DisplayOutputs,
        cancelled: &AtomicBool,
    ) -> Result<CycleStatus, String> {
        // The window a focus command just targeted, though Accessibility may
        // still report the one focus left (#397).
        let window = self
            .current_focus()
            .ok_or("Focused window is unavailable")?;
        self.move_window_with_focus(
            window,
            target,
            true,
            outputs,
            cancelled,
            CommandOrigin::Explicit,
        )
    }

    pub(in crate::app) fn move_focused_follow(
        &mut self,
        target: WorkspaceId,
        outputs: macos::DisplayOutputs,
        cancelled: &AtomicBool,
    ) -> Result<CycleStatus, String> {
        // The window a focus command just targeted, though Accessibility may
        // still report the one focus left (#397).
        let window = self
            .current_focus()
            .ok_or("Focused window is unavailable")?;
        let was_visible = self.is_visible(target, &outputs);
        let moved = self.move_window_with_focus(
            window.clone(),
            target,
            false,
            outputs.clone(),
            cancelled,
            CommandOrigin::Explicit,
        )?;
        if was_visible {
            return Ok(moved);
        }
        self.workspaces.remember_focus(target, &window);
        let mut switched = self.switch_with(
            target,
            outputs,
            cancelled,
            RepeatSwitch::KeepFocus,
            CommandOrigin::Explicit,
        )?;
        switched.failed += moved.failed;
        Ok(switched)
    }

    pub(in crate::app) fn move_app(
        &mut self,
        bundle_id: &str,
        target: WorkspaceId,
        outputs: macos::DisplayOutputs,
        cancelled: &AtomicBool,
    ) -> Result<CycleStatus, String> {
        let mut windows = self.windows_for_bundle(bundle_id);
        if windows.is_empty() {
            self.cycle(outputs.clone(), cancelled)?;
            windows = self.windows_for_bundle(bundle_id);
        }
        let focused_before = self.backend.focused_window();
        let frontmost_before = self.backend.frontmost_application();
        let focused_source = focused_before
            .as_ref()
            .filter(|focused| windows.contains(focused))
            .and_then(|focused| self.workspaces.workspace_of_any(focused))
            .filter(|source| *source != target);
        let mut status: Option<CycleStatus> = None;
        let mut retired_failed = 0;
        for window in windows {
            match self.move_window_outcome(
                window,
                target,
                false,
                outputs.clone(),
                cancelled,
                CommandOrigin::Passive,
            )? {
                MoveWindowOutcome::Moved(mut moved) => {
                    moved.failed += status.map_or(retired_failed, |earlier| earlier.failed);
                    retired_failed = 0;
                    status = Some(moved);
                }
                MoveWindowOutcome::Retired(mut retired) => {
                    if let Some(earlier) = status.take() {
                        retired.failed += earlier.failed;
                        retired.hud = earlier.hud;
                        status = Some(retired);
                    } else {
                        retired_failed += retired.failed;
                    }
                }
            }
        }
        if status.is_some()
            && !self.is_visible(target, &outputs)
            && let (Some(focused), Some(source)) = (focused_before, focused_source)
            && self.is_visible(source, &outputs)
        {
            self.handoff_focus_after_park(
                source,
                &focused,
                frontmost_before,
                &outputs,
                CommandOrigin::Passive,
            );
            self.persist_workspace_state_if_changed()?;
        }
        status.ok_or_else(|| format!("No managed window found for {bundle_id}"))
    }

    /// Every window of the bundle Fractile owns: its tiles, then its floats.
    fn windows_for_bundle(&self, bundle_id: &str) -> Vec<crate::PhysicalWindowId> {
        let mut floating: Vec<_> = self
            .workspaces
            .floating_entries()
            .map(|(window, _)| window.clone())
            .collect();
        floating.sort();
        WorkspaceId::all()
            .flat_map(|workspace| self.workspaces.engine(workspace).bound_windows())
            .chain(floating)
            .filter(|window| self.bundle_id(&window.app) == Some(bundle_id))
            .collect()
    }

    pub(in crate::app) fn move_window(
        &mut self,
        window: crate::PhysicalWindowId,
        target: WorkspaceId,
        outputs: macos::DisplayOutputs,
        cancelled: &AtomicBool,
    ) -> Result<CycleStatus, String> {
        self.move_window_with_focus(
            window,
            target,
            true,
            outputs,
            cancelled,
            CommandOrigin::Passive,
        )
    }

    fn move_window_with_focus(
        &mut self,
        window: crate::PhysicalWindowId,
        target: WorkspaceId,
        handoff_focus: bool,
        outputs: macos::DisplayOutputs,
        cancelled: &AtomicBool,
        origin: CommandOrigin,
    ) -> Result<CycleStatus, String> {
        match self.move_window_outcome(window, target, handoff_focus, outputs, cancelled, origin)? {
            MoveWindowOutcome::Moved(status) | MoveWindowOutcome::Retired(status) => Ok(status),
        }
    }

    fn move_window_outcome(
        &mut self,
        window: crate::PhysicalWindowId,
        target: WorkspaceId,
        handoff_focus: bool,
        outputs: macos::DisplayOutputs,
        cancelled: &AtomicBool,
        origin: CommandOrigin,
    ) -> Result<MoveWindowOutcome, String> {
        self.ensure_managed_workspace(target)?;
        let source = if let Some(source) = self.workspaces.workspace_of_any(&window) {
            source
        } else {
            self.cycle(outputs.clone(), cancelled)?;
            self.workspaces
                .workspace_of_any(&window)
                .ok_or("Window is not managed by Fractile")?
        };
        if source == target {
            return Ok(MoveWindowOutcome::Moved(self.status(0, &outputs)));
        }
        let reported = self.backend.focused_window();
        let was_focused = self.effective_focus(reported.clone()).as_ref() == Some(&window);
        let frontmost_before = self.backend.frontmost_application();
        let is_floating = self.workspaces.floating_workspace_of(&window).is_some();
        if is_floating {
            self.settle_pending_summon_for_move(&window);
        }
        let source_output = output_for(&self.workspaces, source, &outputs);
        let target_output = output_for(&self.workspaces, target, &outputs);
        let window_output = if is_floating {
            self.window_display_output(&window, &outputs)
                .unwrap_or(source_output)
        } else {
            source_output
        };
        let target_is_active = self.is_visible(target, &outputs);
        if !target_is_active {
            let parked = if window_output == target_output {
                self.park_window(&window, target_output, &outputs)
            } else {
                self.park_window_across(&window, window_output, target_output, &outputs)
            };
            if parked == macos::ApplyOutcome::Gone
                && self.retire_confirmed_gone_window(source, &window)
            {
                let mut failed = 0;
                if !is_floating && self.is_visible(source, &outputs) {
                    failed += self
                        .layout_workspace(source, source_output, cancelled)
                        .failed;
                }
                self.persist_workspace_state_if_changed()?;
                return Ok(MoveWindowOutcome::Retired(self.status(failed, &outputs)));
            }
            if !matches!(parked, macos::ApplyOutcome::Applied) {
                return Err("Window could not be parked in the target workspace".into());
            }
        }
        if is_floating {
            self.workspaces.own_floating(window.clone(), target);
        } else {
            let source_canvas = self.layout_settings(source_output).canvas();
            let target_canvas = self.layout_settings(target_output).canvas();
            self.workspaces
                .move_window_with(
                    &window,
                    target,
                    self.config.layout(),
                    source_canvas,
                    target_canvas,
                )
                .ok_or("Window could not be moved")?;
        }
        self.clear_fullscreen_restore_if_inactive(source);
        self.clear_relocation_evidence(source, target, &window);
        self.note_hand_move(&window, source, target);
        let mut failed = 0;
        if !is_floating && self.is_visible(source, &outputs) {
            let layout = self.layout_workspace(source, source_output, cancelled);
            failed += layout.failed;
        }
        if !target_is_active {
            if handoff_focus && was_focused && self.is_visible(source, &outputs) {
                self.handoff_focus_after_park(source, &window, frontmost_before, &outputs, origin);
            } else {
                self.guard_departure(Some(window.clone()), Some(window.app.clone()));
            }
        }
        if target_is_active {
            let was_parked = self.parked.contains_key(&window);
            if was_parked {
                if is_floating && let Some(&recovery) = self.parked.get(&window) {
                    let displays: Vec<_> = outputs.frames().collect();
                    let recovery_output =
                        Self::display_for_frame(recovery, &displays).unwrap_or(source_output);
                    if recovery_output != target_output {
                        let relocated = super::super::move_workspace::translate_frame_to_output(
                            recovery,
                            recovery_output,
                            target_output,
                        );
                        let _: bool = self.record_recovery(&window, relocated);
                    }
                }
                if !matches!(
                    self.restore_window(&window),
                    macos::ApplyOutcome::Applied | macos::ApplyOutcome::Gone
                ) {
                    failed += 1;
                }
            }
            if !is_floating {
                let layout = self.layout_workspace(target, target_output, cancelled);
                failed += layout.failed;
            } else if !was_parked
                && window_output != target_output
                && !self.carry_floating_window(&window, window_output, target_output)
            {
                failed += 1;
            }
            if self.request_focus(&window, [reported]) {
                if !is_floating && self.workspaces.deck(target).is_monocle() {
                    self.workspaces.remember_focus(target, &window);
                }
                // Focus followed the window to the workspace on screen
                // elsewhere; Option+Tab goes back to where it came from.
                self.record_focused_workspace(target);
                let frame = self.focus_window_frame(&window, target, target_output);
                let target_display_id = outputs
                    .resolve(self.workspaces.assignment(target))
                    .id
                    .as_str();
                self.follow_pointer_to(origin, target_display_id, target_output, frame, &outputs);
            } else {
                log_event!("move focus follow failed for {:?}", window);
            }
        }
        self.persist_workspace_state_if_changed()?;
        let mut status = self.status(failed, &outputs);
        status.hud = Some(self.hud_for(target, UiHudKind::Moved, None, &outputs));
        Ok(MoveWindowOutcome::Moved(status))
    }

    /// Hands keyboard focus to a remaining visible window on `source` after
    /// `moved` was parked on an inactive workspace, so focus never stays
    /// stranded on the off-screen window (#323). When `source` is now empty,
    /// focus stays suppressed on `moved` so focus-follow does not snap back.
    fn handoff_focus_after_park(
        &mut self,
        source: WorkspaceId,
        moved: &crate::PhysicalWindowId,
        frontmost_before: Option<AppInstanceId>,
        outputs: &macos::DisplayOutputs,
        origin: CommandOrigin,
    ) {
        let remaining = self.workspaces.windows(source);
        if remaining.is_empty() {
            self.guard_departure(Some(moved.clone()), Some(moved.app.clone()));
            return;
        }
        let outcome = self.focus_workspace_entry(
            source,
            &remaining,
            Some(moved.clone()),
            frontmost_before.or_else(|| Some(moved.app.clone())),
            false,
        );
        if let WorkspaceFocusOutcome::Focused(ref next) = outcome {
            if self.workspaces.deck(source).is_monocle() {
                self.workspaces.remember_focus(source, next);
            }
            let source_output = output_for(&self.workspaces, source, outputs);
            self.follow_pointer_for_workspace(origin, source, source_output, outcome, outputs);
        }
    }

    /// A floating window has no tile to carry it, so moving it to a workspace
    /// on screen on another display moves it there directly, keeping its size
    /// and its place relative to the display. A window already there stays.
    pub(in crate::app) fn carry_floating_window(
        &mut self,
        window: &crate::PhysicalWindowId,
        from: crate::Rect,
        to: crate::Rect,
    ) -> bool {
        let frame = match self.backend.frame(window) {
            Ok(frame) => self.backend.window_server_frame(window).unwrap_or(frame),
            Err(macos::WindowAccessError::Gone) => return true,
            Err(macos::WindowAccessError::Unavailable) => return false,
        };
        if to.contains_center(frame) {
            return true;
        }
        let target = super::super::move_workspace::translate_frame_to_output(frame, from, to);
        let outcome = self.backend.set_position(window, target);
        log_event!(
            "move floating: {:?} to {:?} ({:?})",
            window,
            target,
            outcome
        );
        matches!(
            outcome,
            macos::ApplyOutcome::Applied | macos::ApplyOutcome::Gone
        )
    }
}
