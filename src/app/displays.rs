//! Display changes. Parked positions are chosen against the connected displays;
//! when a display is added or removed they may land on screen, and recovery
//! frames recorded on a removed display would restore off every screen.

use super::*;

/// The displays the runtime last saw. Which hidden windows still need a re-park
/// since they changed is the park table's to say.
#[derive(Default)]
pub(super) struct DisplayWatch {
    known: Option<macos::DisplayOutputs>,
    /// The displays before the last change, which pending windows were parked against.
    previous: Option<macos::DisplayOutputs>,
}

impl DisplayWatch {
    /// The displays the runtime last saw.
    pub(super) const fn known_outputs(&self) -> Option<&macos::DisplayOutputs> {
        self.known.as_ref()
    }
}

impl<B: WorkspaceBackend> WorkspaceRuntime<B> {
    /// How tiles fill `output`: its frame with the configured gaps, padding, width cap and layout.
    pub(super) fn layout_settings(&self, output: crate::Rect) -> Settings {
        Settings::for_output(output)
            .spaced(self.config.spacing())
            .with_layout(self.config.layout())
    }

    /// The last known display canvas for `workspace`, or an empty rect before
    /// any display outputs have been observed.
    pub(super) fn known_workspace_canvas(&self, workspace: WorkspaceId) -> crate::Rect {
        self.displays
            .known
            .as_ref()
            .map(|outputs| {
                self.layout_settings(output_for(&self.workspaces, workspace, outputs))
                    .canvas()
            })
            .unwrap_or_default()
    }

    /// Move a tiled window to `target` using the last known source canvas and `target_output`.
    pub(super) fn move_tiled_to(
        &mut self,
        window: &crate::PhysicalWindowId,
        target: WorkspaceId,
        target_output: crate::Rect,
    ) -> Option<WorkspaceId> {
        let source = self.workspaces.workspace_of(window)?;
        let source_canvas = self.known_workspace_canvas(source);
        let target_canvas = self.layout_settings(target_output).canvas();
        self.workspaces.move_window_with(
            window,
            target,
            self.config.layout(),
            source_canvas,
            target_canvas,
        )
    }

    /// Record the current displays. When they differ from the last ones, every
    /// parked window is due a re-park against them.
    pub(super) fn note_outputs(&mut self, outputs: &macos::DisplayOutputs) {
        if self
            .displays
            .known
            .as_ref()
            .is_some_and(|known| !known.frames().eq(outputs.frames()))
        {
            log_event!("displays changed: re-parking hidden windows");
            // An empty workspace chosen on another display stood for focus on
            // that arrangement only; after a display change the reported focus
            // decides again.
            self.focus.set_empty_choice(None);
            // A window still due a re-park never reached its corner for the last
            // displays, so they are no baseline: every window is written again.
            self.displays.previous = if self.parked.displays_changed() {
                None
            } else {
                self.displays.known.clone()
            };
            // macOS moves windows off a removed display, and Fractile parks and
            // restores them: a frame read back against a target applied before
            // the change says nothing about what the app accepts, so it must
            // not freeze the tile as refused.
            self.applied.iter_mut().for_each(HashMap::clear);
            if let Some(before) = self.displays.known.clone() {
                self.carry_floats_with_workspaces(&before, outputs);
            }
        }
        self.displays.known = Some(outputs.clone());
    }

    /// A display change can move a visible workspace to another display (its
    /// own display reconnected, or unplugged). Its tiles follow the layout;
    /// its floats still on the display it left go with it, keeping their size
    /// and place on the display, as when the workspace is moved by hand (#405).
    fn carry_floats_with_workspaces(
        &mut self,
        before: &macos::DisplayOutputs,
        after: &macos::DisplayOutputs,
    ) {
        let shown_before = self.visible_workspaces(before);
        let old_frames: Vec<_> = before.frames().collect();
        for resolved in self.visible_workspaces(after) {
            let Some(from) = shown_before
                .iter()
                .find(|old| old.workspace == resolved.workspace)
                .map(|old| old.output)
                .filter(|from| *from != resolved.output)
            else {
                continue;
            };
            let floats: Vec<_> = self
                .workspaces
                .floating_entries()
                .filter(|(window, owner)| {
                    *owner == resolved.workspace
                        && !self.workspaces.is_pinned(window)
                        && !self.parked.contains_key(window)
                })
                .map(|(window, _)| window.clone())
                .collect();
            for window in floats {
                let Some(frame) = self
                    .backend
                    .window_server_frame(&window)
                    .or_else(|| self.backend.frame(&window).ok())
                else {
                    continue;
                };
                if Self::display_for_frame(frame, &old_frames) != Some(from) {
                    continue;
                }
                let target =
                    super::move_workspace::translate_frame_to_output(frame, from, resolved.output);
                let outcome = self.backend.set_position(&window, target);
                log_event!(
                    "displays changed: carry floating {window:?} with workspace {} to {target:?} ({outcome:?})",
                    resolved.workspace
                );
            }
        }
    }

    /// Note the displays, then re-park each hidden window still due one. Runs
    /// before focus follow and heal, which would otherwise take a window the
    /// new display uncovered for one the user brought back. A write that fails
    /// (the app is briefly inaccessible) stays due and is retried next time.
    pub(super) fn follow_displays(&mut self, outputs: &macos::DisplayOutputs) {
        self.note_outputs(outputs);
        let due: Vec<_> = self.parked.awaiting_display().cloned().collect();
        if due.is_empty() {
            return;
        }
        let visible: HashSet<_> = self
            .visible_workspaces(outputs)
            .into_iter()
            .map(|resolved| resolved.workspace)
            .collect();
        let displays: Vec<_> = outputs.frames().collect();
        let screens = outputs.screens();
        let previous = self
            .displays
            .previous
            .as_ref()
            .map(|previous| (previous.frames().collect::<Vec<_>>(), previous.screens()));
        for window in due {
            // Hidden scratchpad windows intentionally have no workspace owner, but still need to
            // stay parked off-screen when a newly connected display covers their parking edge.
            let output = match self.workspaces.residency(&window) {
                _ if self.stays_on_screen(&window) => None,
                Some(crate::workspace::Residency::Scratchpad { shown_on: None, .. }) => {
                    Some(outputs.main().frame)
                }
                _ => self
                    .workspaces
                    .workspace_of_any(&window)
                    .filter(|owner| !visible.contains(owner))
                    .map(|owner| output_for(&self.workspaces, owner, outputs)),
            };
            // A write that fails stays due and is retried next time.
            if output.is_none_or(|output| {
                self.repark_for_displays(&window, output, &displays, &screens, previous.as_ref())
            }) {
                self.parked.display_settled(&window);
            }
        }
    }

    /// Whether `window` is parked for `displays` now.
    fn repark_for_displays(
        &mut self,
        window: &crate::PhysicalWindowId,
        output: crate::Rect,
        displays: &[crate::Rect],
        screens: &[crate::Rect],
        previous: Option<&(Vec<crate::Rect>, Vec<crate::Rect>)>,
    ) -> bool {
        let Some(current) = self.backend.window_server_frame(window) else {
            return false;
        };
        let target = crate::geometry::parked_frame_clear_of(output, displays, screens, current);
        // macOS may hold a parked window above its corner, so its position alone
        // cannot tell whether the corner moved (a display that grew at the bottom
        // would show more of it). Leave it only where the corner did not move.
        let corner_unchanged = previous.is_some_and(|(frames, screens)| {
            frames.contains(&output)
                && crate::geometry::parked_frame_clear_of(output, frames, screens, current)
                    == target
        });
        if corner_unchanged
            && crate::geometry::parked_at(current, target, 0)
            && !crate::geometry::shows_on(current, screens)
        {
            return true;
        }
        let outcome = self.backend.set_parked_position(window, target);
        log_event!("displays changed: re-park {window:?} at {target:?} ({outcome:?})");
        if outcome != macos::ApplyOutcome::Applied {
            return false;
        }
        self.parked.park_applied(window, self.backend.now());
        true
    }

    /// Recover visible, non-parked owned windows whose on-screen window-server
    /// center sits outside every connected display (for example, macOS leaving
    /// a visible window with a bottom-right sliver after disconnecting a larger
    /// external display). Runs before `snapshot` so the window is observed back
    /// on its display instead of omitted and retired.
    pub(super) fn recover_stranded_visible(
        &mut self,
        outputs: &macos::DisplayOutputs,
        visible: &[ResolvedWorkspace],
        server_frames: &HashMap<u32, crate::Rect>,
        cancelled: &AtomicBool,
        failed: &mut usize,
    ) {
        if cancelled.load(Ordering::Acquire) || self.backend.mouse_down() {
            return;
        }
        // A window is stranded only off every physical display: one released
        // over a Dock or menu-bar strip is still on its display (#393).
        let displays = outputs.screens();
        let has_candidate = visible.iter().any(|resolved| {
            self.scope.manages_selector(&resolved.selector)
                && self
                    .workspaces
                    .windows(resolved.workspace)
                    .into_iter()
                    .any(|window| {
                        !self.parked.contains_key(&window)
                            && server_frames.get(&window.window_id).is_some_and(|frame| {
                                !displays
                                    .iter()
                                    .any(|display| display.contains_center(*frame))
                            })
                    })
        });
        if !has_candidate {
            return;
        }
        let Some(onscreen_frames) = self.backend.onscreen_window_frames() else {
            return;
        };
        for resolved in visible {
            if cancelled.load(Ordering::Acquire) {
                return;
            }
            if !self.scope.manages_selector(&resolved.selector) {
                continue;
            }
            let workspace = resolved.workspace;
            let output = resolved.output;
            let settings = self.layout_settings(output);
            let engine = self.workspaces.engine_mut(workspace);
            engine.fit_minimums(&settings, &self.minimum_sizes);
            let mut placements = engine.placements(&settings);
            self.project_deck(workspace, &mut placements, &settings);
            let mut recover_tiles = Vec::new();
            for placement in placements {
                let Some(window) = placement.window.as_ref() else {
                    continue;
                };
                if self.parked.contains_key(window) {
                    continue;
                }
                let Some(&frame) = onscreen_frames.get(&window.window_id) else {
                    continue;
                };
                if displays
                    .iter()
                    .any(|display| display.contains_center(frame))
                {
                    continue;
                }
                log_event!(
                    "recover stranded tiled {:?} on workspace {} from {:?} to {:?}",
                    window,
                    workspace,
                    frame,
                    placement.frame
                );
                recover_tiles.push(placement);
            }
            if !recover_tiles.is_empty() {
                let results = self
                    .backend
                    .apply_managed(&recover_tiles, &settings, cancelled);
                self.record_results(workspace, &results);
                let initially_failed = failed_results(&results);
                *failed += self
                    .refit_minimum_sizes(workspace, output, results, cancelled)
                    .map_or(initially_failed, |status| status.failed);
            }
            let floating: Vec<_> = self
                .workspaces
                .floating_entries()
                .filter(|(_, owner)| *owner == workspace)
                .map(|(window, _)| window.clone())
                .collect();
            for window in floating {
                if cancelled.load(Ordering::Acquire) {
                    return;
                }
                if self.parked.contains_key(&window) {
                    continue;
                }
                let Some(&frame) = onscreen_frames.get(&window.window_id) else {
                    continue;
                };
                if displays
                    .iter()
                    .any(|display| display.contains_center(frame))
                {
                    continue;
                }
                let target = clamp_into_output(frame, output);
                let outcome = self.backend.set_position(&window, target);
                log_event!(
                    "recover stranded floating {:?} on workspace {} from {:?} to {:?} ({:?})",
                    window,
                    workspace,
                    frame,
                    target,
                    outcome
                );
                if !matches!(
                    outcome,
                    macos::ApplyOutcome::Applied | macos::ApplyOutcome::Gone
                ) {
                    *failed += 1;
                }
            }
        }
    }

    /// Where a recovery frame is restored: itself while it shows on a connected
    /// display, otherwise moved onto the main display (its own is gone).
    pub(super) fn restore_target(&self, frame: crate::Rect) -> crate::Rect {
        let Some(outputs) = &self.displays.known else {
            return frame;
        };
        let displays: Vec<_> = outputs.frames().collect();
        crate::geometry::onto_displays(frame, &displays, outputs.main().frame)
    }

    /// Where a parked window is restored. When the display its recovery frame
    /// was recorded on is unplugged it goes to the main display, even if
    /// another display now covers the old position (#406).
    pub(super) fn recovery_target(&self, window: &crate::PhysicalWindowId) -> Option<crate::Rect> {
        let record = self.parked.record(window)?;
        let outputs = self.displays.known.as_ref();
        let display_gone = record.original_display.as_ref().is_some_and(|id| {
            outputs.is_some_and(|outputs| outputs.displays().iter().all(|d| &d.id != id))
        });
        Some(match outputs {
            Some(outputs) if display_gone => {
                crate::geometry::onto_displays(record.original, &[], outputs.main().frame)
            }
            _ => self.restore_target(record.original),
        })
    }

    /// The connected display a recovery frame is recorded on.
    pub(super) fn recovery_display(&self, frame: crate::Rect) -> Option<String> {
        let outputs = self.displays.known.as_ref()?;
        let displays: Vec<_> = outputs.frames().collect();
        let display = Self::display_for_frame(frame, &displays)?;
        outputs
            .displays()
            .iter()
            .find(|output| output.frame == display)
            .map(|output| output.id.clone())
    }
}

/// Move `frame` (keeping its size) so its top-left sits inside `output`.
fn clamp_into_output(frame: crate::Rect, output: crate::Rect) -> crate::Rect {
    crate::Rect {
        x: frame
            .x
            .clamp(output.x, output.x + (output.width - frame.width).max(0)),
        y: frame
            .y
            .clamp(output.y, output.y + (output.height - frame.height).max(0)),
        ..frame
    }
}
