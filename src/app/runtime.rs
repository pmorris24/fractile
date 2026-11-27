use super::switching::RepeatSwitch;
use super::*;

mod debug;
pub(super) use debug::RuntimeDebug;

pub(super) struct CycleStatus {
    pub(super) missing_frontmost_window: Option<AppInstanceId>,
    pub(super) close_recheck: Option<Instant>,
    pub(super) active: Vec<WorkspaceId>,
    /// The visible workspace focus is on: the last one switched to or focused, else the first.
    pub(super) focused: Option<WorkspaceId>,
    pub(super) layouts: Vec<status_item::WorkspaceLayout>,
    pub(super) menu: Vec<status_item::DisplayWorkspaces>,
    /// Every workspace holding windows, previewed for Settings.
    pub(super) windows: Vec<UiWorkspaceWindows>,
    pub(super) failed: usize,
    pub(super) hud: Option<UiHudEvent>,
}

pub(super) struct LayoutStatus {
    pub(super) failed: usize,
    pub(super) blocked: bool,
}

#[derive(Clone)]
pub(super) struct ResolvedWorkspace {
    pub(super) workspace: WorkspaceId,
    pub(super) selector: DisplaySelector,
    pub(super) display_id: String,
    pub(super) output: crate::Rect,
    /// The display's full bounds, menu bar and Dock strips included.
    pub(super) screen: crate::Rect,
}

pub(super) struct WorkspaceRuntime<B = MacWorkspaceBackend> {
    pub(super) backend: B,
    pub(super) scope: ManagementScope,
    pub(super) workspaces: Workspaces,
    pub(super) known_apps: [HashMap<AppInstanceId, String>; WorkspaceId::COUNT],
    pub(super) constrained:
        [HashMap<crate::PhysicalWindowId, (crate::Rect, crate::Rect)>; WorkspaceId::COUNT],
    /// Target frames reported as `Applied` on the latest layout pass, awaiting
    /// observation readback. Electron apps accept `AXSize` writes synchronously
    /// and clamp to their aspect-ratio or size limits asynchronously afterward.
    pub(super) applied: [HashMap<crate::PhysicalWindowId, cycle::AppliedFrame>; WorkspaceId::COUNT],
    /// Consecutive bounced or rejected applications per window for the same
    /// target frame; past `APPLY_ATTEMPT_LIMIT` the target is frozen.
    pub(super) apply_attempts:
        [HashMap<crate::PhysicalWindowId, (crate::Rect, u8)>; WorkspaceId::COUNT],
    /// Parked windows' recovery frames and parking phases, and the windows
    /// exempt from parking because they keep un-parking themselves.
    pub(super) parked: ParkTable,
    /// Bound windows omitted from the AX snapshot but still listed by CG,
    /// with their settling and close-recheck state.
    pub(super) omitted_windows: HashMap<crate::PhysicalWindowId, cycle::Omission>,
    /// Owned windows Accessibility reports gone while the window server still
    /// lists them, and when that began.
    pub(super) inaccessible_since: HashMap<crate::PhysicalWindowId, Instant>,
    /// Smallest width and height each window has refused to shrink below.
    pub(super) minimum_sizes: HashMap<crate::PhysicalWindowId, (i32, i32)>,
    /// Windows stale-parked recovery could not move, and when to try them again.
    pub(super) unmovable_stale: super::stale_parked::UnmovableStale,
    pub(super) config: Config,
    /// Focus intent: a request in flight, focus left behind, and the
    /// cross-display workspace history Option+Tab reads.
    pub(super) focus: super::focus::FocusTracker,
    /// The pointer drag or resize being previewed, until mouse-up commits it.
    pub(super) gesture: Option<super::commands::GestureOverlay>,
    pub(super) exact_spacing_workspaces: HashSet<WorkspaceId>,
    /// With the HUD kept on screen, the rows it last got, so a cycle sends it only changes.
    pub(super) stay_hud: Option<Vec<super::UiHudDisplay>>,
    pub(super) persisted_workspace_state: crate::workspace::WorkspaceState,
    /// Title-free placements that survive application relaunches and reboots.
    pub(super) memory: MemoryState,
    pub(super) journal: JournalState,
    pub(super) displays: super::displays::DisplayWatch,
    /// A pressed "Move Tab to New Window" awaiting its new window.
    pub(super) tab_detach: Option<super::detach_tab::PendingTabDetach>,
    /// The setup tour's arrangement: where it sends unowned windows, and how to undo it.
    pub(super) arranging: super::arrange::ArrangeState,
}

impl WorkspaceRuntime<MacWorkspaceBackend> {
    pub(super) fn new(
        scope: ManagementScope,
        parked: HashMap<crate::PhysicalWindowId, crate::Rect>,
        config: Config,
        workspace_state: Option<crate::workspace::WorkspaceState>,
    ) -> Self {
        let backend = MacWorkspaceBackend::new();
        Self::with_backend_config_state(scope, backend, parked, config, workspace_state)
    }
}

impl<B: WorkspaceBackend> WorkspaceRuntime<B> {
    /// Rules apply to windows from now on; named workspaces and display assignments update.
    /// New named workspaces get their hotkeys at the next restart.
    pub(super) fn replace_config(&mut self, config: Config) {
        if config.spacing() != self.config.spacing() {
            self.exact_spacing_workspaces.extend(WorkspaceId::all());
        }
        // A HUD kept on screen (again) gets its rows on the next cycle.
        self.stay_hud = None;
        // An edit that only renames the selector a display is known by (its
        // UUID instead of secondary) keeps that display's workspace in front.
        let known = self.displays.known_outputs().cloned();
        config.configure_workspaces_with(&mut self.workspaces, |a, b| {
            known.as_ref().is_some_and(|outputs| {
                match (outputs.resolve_exact(a), outputs.resolve_exact(b)) {
                    (Some(a), Some(b)) => a.id == b.id,
                    _ => false,
                }
            })
        });
        self.config = config;
    }

    pub(super) fn execute(
        &mut self,
        command: WorkerCommand,
        cancelled: &AtomicBool,
    ) -> Option<WorkerResult> {
        if let WorkerCommand::Action(_, outputs) | WorkerCommand::Gesture(_, _, outputs) = &command
        {
            self.follow_displays(outputs);
            self.settle_empty_choice(outputs);
        }
        // Once the button is up, a gesture whose mouse-up never arrived (an
        // unreadable final frame, #395) is over: its previews are undone.
        if let WorkerCommand::Action(_, outputs) = &command
            && !self.backend.mouse_down()
        {
            self.abandon_gesture(Some(outputs), cancelled);
        }
        let (result, outputs) = match command {
            WorkerCommand::Action(action, outputs) => (
                self.execute_action(action, outputs.clone(), cancelled),
                outputs,
            ),
            WorkerCommand::Gesture(gesture, preview, outputs) => {
                let window = match &gesture {
                    FinalGesture::Drop { window, .. } | FinalGesture::Resize(window, _) => window,
                };
                for attempts in &mut self.apply_attempts {
                    attempts.remove(window);
                }
                // Gestures never move the pointer: their moves run as passive.
                let result = match gesture {
                    FinalGesture::Drop {
                        window,
                        point,
                        bounds,
                    } => self.drop_window_tracked(
                        window,
                        point,
                        bounds,
                        preview,
                        outputs.clone(),
                        cancelled,
                    ),
                    FinalGesture::Resize(window, frame) => {
                        self.resize_window(window, frame, preview, outputs.clone(), cancelled)
                    }
                };
                (result, outputs)
            }
            WorkerCommand::RestoreAll(outputs) => {
                if let Some(outputs) = &outputs {
                    self.note_outputs(outputs);
                }
                // Pausing mid-drag discards the gesture and its previews (#387, #389).
                self.abandon_gesture(outputs.as_ref(), cancelled);
                let requested = self.parked.len();
                // Restored scratchpad windows are ordinary windows when tiling resumes.
                self.clear_scratchpad();
                let failed = self.restore_all();
                self.journal_restore("restore all", requested, failed);
                // Pause, quit and restart all restore first; the layout they
                // resume from must be on disk before the process can exit.
                self.flush_workspace_state();
                return Some(WorkerResult::Restored { failed });
            }
            WorkerCommand::UndoArrangement(outputs) => {
                return Some(self.undo_arrangement(outputs.as_ref(), cancelled));
            }
            WorkerCommand::Inventory(outputs) => {
                return Some(WorkerResult::Inventory(Box::new(self.inventory(&outputs))));
            }
            WorkerCommand::Stop => {
                self.journal_event(crate::journal::JournalEvent::Stop);
                self.flush_workspace_state();
                return None;
            }
        };
        self.sync_scratchpad(&outputs);
        self.check_arrangement(&outputs);
        Some(WorkerResult::Status {
            result,
            debug: Box::new(self.debug_state(&outputs)),
        })
    }

    fn execute_action(
        &mut self,
        action: WorkerAction,
        outputs: macos::DisplayOutputs,
        cancelled: &AtomicBool,
    ) -> Result<CycleStatus, String> {
        let origin = super::commands::CommandOrigin::of(&action);
        match action {
            WorkerAction::Retile => {
                let mut status = self.cycle(outputs.clone(), cancelled)?;
                if !self.exact_spacing_workspaces.is_empty()
                    && !self.backend.mouse_down()
                    && self.space_transition_until().is_none()
                {
                    for r in self.visible_workspaces(&outputs) {
                        if self.scope.manages_selector(&r.selector)
                            && self.exact_spacing_workspaces.contains(&r.workspace)
                            && !self.workspaces.windows(r.workspace).is_empty()
                        {
                            status.failed += self
                                .layout_workspace(r.workspace, r.output, cancelled)
                                .failed;
                        }
                    }
                }
                Ok(status)
            }
            WorkerAction::SwitchWorkspace(workspace) => {
                let status = self.switch_with(
                    workspace,
                    outputs.clone(),
                    cancelled,
                    RepeatSwitch::CycleFocus,
                    origin,
                )?;
                self.note_workspace_choice(workspace, &outputs);
                Ok(status)
            }
            // A menu-bar or HUD click on the workspace already up re-focuses it;
            // clicking it again must not jump to another window.
            WorkerAction::SelectWorkspace(workspace) => {
                let status = self.switch_with(
                    workspace,
                    outputs.clone(),
                    cancelled,
                    RepeatSwitch::KeepFocus,
                    origin,
                )?;
                self.note_workspace_choice(workspace, &outputs);
                Ok(status)
            }
            WorkerAction::WorkspaceBackAndForth => {
                self.workspace_back_and_forth(outputs, cancelled)
            }
            WorkerAction::MoveWorkspaceToDisplay(target) => {
                self.move_workspace_to_display(target, outputs, cancelled)
            }
            WorkerAction::Swipe(swipe, fingers, point) => {
                self.swipe(swipe, fingers, point, outputs, cancelled)
            }
            WorkerAction::MoveFocused(workspace) => {
                self.move_focused(workspace, outputs, cancelled)
            }
            WorkerAction::StepWorkspace(step) => self.step_workspace(step, outputs, cancelled),
            WorkerAction::MoveFocusedStep { step, follow } => {
                self.move_focused_step(step, follow, outputs, cancelled)
            }
            WorkerAction::MoveFocusedFollow(workspace) => {
                self.move_focused_follow(workspace, outputs, cancelled)
            }
            WorkerAction::MoveApp(bundle_id, workspace) => {
                self.move_app(&bundle_id, workspace, outputs, cancelled)
            }
            WorkerAction::MoveTab { target, follow } => {
                self.move_tab(target, follow, outputs, cancelled)
            }
            WorkerAction::CycleFocus => self.cycle_focus(outputs, cancelled),
            WorkerAction::CycleFocusPrevious => self.cycle_focus_previous(outputs, cancelled),
            WorkerAction::Focus(direction) => self.focus_relative(direction, outputs, cancelled),
            WorkerAction::Reorder(direction) => self.reorder_focused(direction, outputs, cancelled),
            WorkerAction::ToggleLayout => self.toggle_layout(outputs, cancelled),
            WorkerAction::Resize(direction) => self.resize_focused(direction, outputs, cancelled),
            WorkerAction::ResizeTo(axis, points) => {
                self.resize_focused_to(axis, points, outputs, cancelled)
            }
            WorkerAction::Balance => self.balance_focused(outputs, cancelled),
            WorkerAction::ToggleFullscreenLayout => {
                self.toggle_fullscreen_layout(outputs, cancelled)
            }
            WorkerAction::ToggleFloat => self.toggle_float(outputs, cancelled),
            WorkerAction::TogglePin => self.toggle_pin(outputs, cancelled),
            WorkerAction::Scratchpad(command) => {
                self.scratchpad_command(command, outputs, cancelled)
            }
            WorkerAction::MarkWrong => {
                self.mark_wrong();
                Ok(self.status(0, &outputs))
            }
            WorkerAction::Arrange(arrangement) => self.arrange(&arrangement, outputs, cancelled),
            WorkerAction::FocusHovered { window_id, point } => {
                self.focus_hovered(window_id, point, outputs)
            }
        }
    }

    #[cfg(test)]
    pub(super) fn with_backend(
        scope: ManagementScope,
        backend: B,
        parked: HashMap<crate::PhysicalWindowId, crate::Rect>,
    ) -> Self {
        Self::with_backend_and_config(scope, backend, parked, Config::default())
    }

    #[cfg(test)]
    pub(super) fn with_backend_and_config(
        scope: ManagementScope,
        backend: B,
        parked: HashMap<crate::PhysicalWindowId, crate::Rect>,
        config: Config,
    ) -> Self {
        Self::with_backend_config_state(scope, backend, parked, config, None)
    }

    pub(super) fn with_backend_config_state(
        scope: ManagementScope,
        backend: B,
        parked: HashMap<crate::PhysicalWindowId, crate::Rect>,
        config: Config,
        workspace_state: Option<crate::workspace::WorkspaceState>,
    ) -> Self {
        let restored_workspace_state = workspace_state.is_some();
        let mut workspaces = Workspaces::default();
        config.configure_workspaces(&mut workspaces);
        if let Some(state) = workspace_state {
            workspaces.restore(state);
        }
        let fullscreen_restore = if restored_workspace_state {
            WorkspaceId::all()
                .filter(|workspace| {
                    workspaces.deck(*workspace).is_monocle()
                        && scope.manages_selector(workspaces.assignment(*workspace))
                })
                .collect()
        } else {
            HashSet::new()
        };
        let known_apps = std::array::from_fn(|index| {
            workspaces
                .app_instances(WorkspaceId::from_index(index))
                .map(|app| (app.clone(), String::new()))
                .collect()
        });
        let persisted_workspace_state = workspaces.state();
        let has_spacing = !config.spacing().is_none();
        Self {
            backend,
            scope,
            workspaces,
            known_apps,
            constrained: std::array::from_fn(|_| HashMap::new()),
            applied: std::array::from_fn(|_| HashMap::new()),
            apply_attempts: std::array::from_fn(|_| HashMap::new()),
            parked: parked.into(),
            omitted_windows: HashMap::new(),
            inaccessible_since: HashMap::new(),
            minimum_sizes: HashMap::new(),
            unmovable_stale: super::stale_parked::UnmovableStale::default(),
            config,
            focus: super::focus::FocusTracker::with_fullscreen_restore(fullscreen_restore),
            gesture: None,
            exact_spacing_workspaces: WorkspaceId::all().filter(|_| has_spacing).collect(),
            stay_hud: None,
            persisted_workspace_state,
            memory: MemoryState::default(),
            journal: JournalState::default(),
            displays: super::displays::DisplayWatch::default(),
            tab_detach: None,
            arranging: super::arrange::ArrangeState::default(),
        }
    }

    pub(super) fn restore_window_memory(&mut self, memory: crate::window_memory::WindowMemory) {
        self.memory = MemoryState {
            memory,
            started_at: Some(self.backend.now()),
            ..MemoryState::default()
        };
    }

    /// What each connected display shows: the lane its own record (or the
    /// record of an unplugged display that falls back onto it) brought forward
    /// last; with no record, the lowest-numbered lane assigned to it exactly.
    pub(super) fn visible_workspaces(
        &self,
        outputs: &macos::DisplayOutputs,
    ) -> Vec<ResolvedWorkspace> {
        let connected = |id: &str| outputs.displays().iter().any(|display| display.id == id);
        let screens = outputs.screens();
        outputs
            .displays()
            .iter()
            .zip(screens)
            .filter_map(|(display, screen)| {
                let lands_here =
                    |selector: &DisplaySelector| outputs.resolve(selector).id == display.id;
                let candidates = || {
                    self.workspaces
                        .active_lanes()
                        .filter(|(selector, _)| lands_here(selector))
                };
                let selected = self
                    .workspaces
                    .display_fronts()
                    .filter(|(id, lane, _)| {
                        (*id == display.id || !connected(id)) && lands_here(lane)
                    })
                    .max_by_key(|(_, _, at)| *at)
                    .and_then(|(_, lane, _)| candidates().find(|(selector, _)| *selector == lane))
                    .or_else(|| {
                        candidates()
                            .filter(|(selector, _)| selector_available(selector, outputs))
                            .min_by_key(|(_, workspace)| *workspace)
                    })
                    .or_else(|| candidates().min_by_key(|(_, workspace)| *workspace))?;
                Some(ResolvedWorkspace {
                    workspace: selected.1,
                    selector: selected.0.clone(),
                    display_id: display.id.clone(),
                    output: display.frame,
                    screen,
                })
            })
            .collect()
    }

    pub(super) fn is_visible(
        &self,
        workspace: WorkspaceId,
        outputs: &macos::DisplayOutputs,
    ) -> bool {
        self.visible_workspaces(outputs)
            .iter()
            .any(|resolved| resolved.workspace == workspace)
    }

    pub(super) fn bundle_id(&self, app: &AppInstanceId) -> Option<&str> {
        self.memory
            .bundles
            .get(app)
            .or_else(|| {
                self.known_apps
                    .iter()
                    .find_map(|apps| apps.get(app))
                    .filter(|bundle| !bundle.is_empty())
            })
            .map(String::as_str)
    }

    pub(super) fn ensure_managed_workspace(
        &self,
        workspace: WorkspaceId,
    ) -> Result<DisplaySelector, String> {
        if !self.workspaces.is_enabled(workspace) {
            return Err(format!("Workspace {workspace} is not configured"));
        }
        let selector = self.workspaces.assignment(workspace).clone();
        if !self.scope.manages_selector(&selector) {
            return Err(format!(
                "Workspace {workspace} is outside the managed display scope"
            ));
        }
        Ok(selector)
    }

    /// Whether `workspace` is in Fullscreen Layout with more than one bound
    /// tile — the case where a newly arriving tile needs a HUD cue so it does
    /// not silently join a stack.
    pub(super) fn is_stacked_fullscreen(&self, workspace: WorkspaceId) -> bool {
        self.workspaces.deck(workspace).is_monocle()
            && self
                .workspaces
                .engine(workspace)
                .model()
                .workspace
                .slots
                .iter()
                .filter(|slot| slot.bound_window().is_some())
                .count()
                > 1
    }

    pub(super) fn spatial_target(
        &self,
        focused: &crate::PhysicalWindowId,
        direction: Direction,
        outputs: &macos::DisplayOutputs,
    ) -> Option<(WorkspaceId, crate::PhysicalWindowId)> {
        let tiles = self.navigation_tiles(outputs);
        // `[labs] recent_focus`: among tiles bordering the focused one, the
        // most recently focused wins (#374).
        let engine = if self.config.labs().recent_focus {
            self.workspaces
                .workspace_of(focused)
                .map(|workspace| self.workspaces.engine(workspace))
        } else {
            None
        };
        crate::navigation::target_by_recency(&tiles, focused, direction, |window| {
            engine.and_then(|engine| engine.focus_recency(window))
        })
        .map(|target| (target.workspace, target.window.clone()))
    }

    pub(super) fn navigation_tiles(&self, outputs: &macos::DisplayOutputs) -> Vec<NavigationTile> {
        self.navigation_tiles_for(&self.visible_workspaces(outputs))
    }

    pub(super) fn navigation_tiles_for(
        &self,
        visible: &[ResolvedWorkspace],
    ) -> Vec<NavigationTile> {
        let mut tiles = Vec::new();
        for resolved in visible {
            let settings = self.layout_settings(resolved.output);
            let mut placements = self
                .workspaces
                .engine(resolved.workspace)
                .placements(&settings);
            self.workspaces
                .deck(resolved.workspace)
                .place(&mut placements, settings.single());
            let display = Some(settings.usable());
            tiles.extend(placements.into_iter().filter_map(|placement| {
                placement.window.map(|window| NavigationTile {
                    workspace: resolved.workspace,
                    window,
                    frame: placement.frame,
                    display,
                })
            }));
        }
        tiles
    }

    pub(super) fn layout_workspace(
        &mut self,
        workspace: WorkspaceId,
        output: crate::Rect,
        cancelled: &AtomicBool,
    ) -> LayoutStatus {
        self.layout_workspace_during_drag(workspace, output, None, cancelled)
    }

    pub(super) fn layout_workspace_during_drag(
        &mut self,
        workspace: WorkspaceId,
        output: crate::Rect,
        dragged: Option<&crate::PhysicalWindowId>,
        cancelled: &AtomicBool,
    ) -> LayoutStatus {
        let (status, results) = self.apply_layout(workspace, output, dragged, cancelled);
        if dragged.is_some() {
            return status;
        }
        self.refit_minimum_sizes(workspace, output, results, cancelled)
            .unwrap_or(status)
    }

    pub(super) fn apply_layout(
        &mut self,
        workspace: WorkspaceId,
        output: crate::Rect,
        dragged: Option<&crate::PhysicalWindowId>,
        cancelled: &AtomicBool,
    ) -> (LayoutStatus, Vec<macos::ApplyResult>) {
        let mut settings = self.layout_settings(output);
        if dragged.is_none() && self.exact_spacing_workspaces.contains(&workspace) {
            settings.frame_tolerance = 0;
        }
        let engine = self.workspaces.engine_mut(workspace);
        engine.fit_minimums(&settings, &self.minimum_sizes);
        let mut placements = engine.placements(&settings);
        self.project_deck(workspace, &mut placements, &settings);
        let attempts = &mut self.apply_attempts[workspace.index()];
        for placement in &placements {
            if let Some(window) = placement.window.as_ref() {
                attempts.remove(window);
            }
        }
        let results = match dragged {
            Some(d) => self
                .backend
                .apply_managed_during_drag(&placements, &settings, d, cancelled),
            None => self
                .backend
                .apply_managed(&placements, &settings, cancelled),
        };
        if settings.frame_tolerance == 0 && !cancelled.load(Ordering::Acquire) {
            self.exact_spacing_workspaces.remove(&workspace);
        }
        // Managed layout runs without a fresh observation; the next cycle validates readback.
        self.record_results(workspace, &results);
        self.retire_laid_out(&results);
        let failed = failed_results(&results);
        let expected = placements.iter().filter(|p| p.window.is_some()).count();
        // Deferred means a drag is in progress and layout retries next cycle;
        // it must not be treated as a failure that rolls back a switch.
        let blocked = results.len() != expected
            || results.iter().any(|result| {
                matches!(
                    result.outcome,
                    macos::ApplyOutcome::Gone | macos::ApplyOutcome::Unavailable
                )
            });
        (LayoutStatus { failed, blocked }, results)
    }

    pub(super) fn forget_window(
        &mut self,
        workspace: WorkspaceId,
        window: &crate::PhysicalWindowId,
    ) {
        let canvas = self.known_workspace_canvas(workspace);
        self.forget_window_on(workspace, window, canvas);
    }

    pub(super) fn forget_window_on(
        &mut self,
        workspace: WorkspaceId,
        window: &crate::PhysicalWindowId,
        canvas: crate::Rect,
    ) {
        self.workspaces
            .forget_window_with(workspace, window, self.config.layout(), canvas);
        self.clear_fullscreen_restore_if_inactive(workspace);
        self.workspaces.forget_floating(window);
        self.constrained[workspace.index()].remove(window);
        self.applied[workspace.index()].remove(window);
        self.apply_attempts[workspace.index()].remove(window);
        self.omitted_windows.remove(window);
        self.parked.forget_heals(window);
        self.inaccessible_since.remove(window);
        self.minimum_sizes.remove(window);
    }

    pub(super) fn clear_fullscreen_restore_if_inactive(&mut self, workspace: WorkspaceId) {
        if !self.workspaces.deck(workspace).is_monocle() {
            self.focus.settle_fullscreen(workspace);
        }
    }

    /// Project the split tree's placements through the workspace's deck mode,
    /// recording each tile's projected frame as its target.
    pub(super) fn project_deck(
        &mut self,
        workspace: WorkspaceId,
        placements: &mut [crate::Placement],
        settings: &Settings,
    ) {
        let deck = self.workspaces.deck(workspace);
        if deck == crate::workspace::DeckMode::Split {
            return;
        }
        deck.place(placements, settings.single());
        let engine = self.workspaces.engine_mut(workspace);
        for placement in placements.iter() {
            engine.record_placement_target(placement.slot, placement.frame);
        }
    }

    pub(super) fn status(&self, failed: usize, outputs: &macos::DisplayOutputs) -> CycleStatus {
        self.status_for_visible(failed, &self.visible_workspaces(outputs), outputs)
    }

    pub(super) fn flush_workspace_state(&mut self) {
        if let Err(error) = self.persist_workspace_state_if_changed() {
            log_event!("failed to persist workspace state: {error}");
        }
        self.backend.flush_state();
    }

    pub(super) fn persist_workspace_state_if_changed(&mut self) -> Result<(), String> {
        let state = self.workspaces.state();
        if state == self.persisted_workspace_state {
            return Ok(());
        }
        self.backend.persist_workspace_state(&state)?;
        self.persisted_workspace_state = state;
        Ok(())
    }

    fn preview_layout(&self, workspace: WorkspaceId, output: crate::Rect) -> Vec<crate::Rect> {
        let mut layout = self.workspaces.engine(workspace).preview(output);
        if self.workspaces.deck(workspace).is_monocle() {
            layout.fill(output);
        }
        layout
    }

    pub(super) fn status_for_visible(
        &self,
        failed: usize,
        visible: &[ResolvedWorkspace],
        outputs: &macos::DisplayOutputs,
    ) -> CycleStatus {
        let active: Vec<_> = visible
            .iter()
            .filter(|resolved| self.scope.manages_selector(&resolved.selector))
            .map(|resolved| resolved.workspace)
            .collect();
        let current = self.focus.history().current;
        let focused = active
            .iter()
            .copied()
            .find(|workspace| Some(*workspace) == current)
            .or_else(|| active.first().copied());
        CycleStatus {
            missing_frontmost_window: None,
            close_recheck: None,
            focused,
            layouts: self.status_layouts(visible, outputs),
            menu: self.workspace_menu(visible, outputs),
            windows: self.workspace_windows(outputs),
            active,
            failed,
            hud: None,
        }
    }

    /// A display's visible workspace and every other one holding windows, in order:
    /// the rows of its menu section and the stops of a swipe.
    pub(super) fn listed_workspaces(
        &self,
        display_id: &str,
        current: Option<WorkspaceId>,
        outputs: &macos::DisplayOutputs,
    ) -> Vec<WorkspaceId> {
        self.owned_workspaces(display_id, outputs)
            .into_iter()
            .filter(|workspace| {
                current == Some(*workspace) || !self.workspaces.windows(*workspace).is_empty()
            })
            .collect()
    }

    /// Every enabled, managed workspace that shows on `display_id`, empty ones included, in the
    /// configured order: the workspaces the status menu offers for that display.
    pub(super) fn owned_workspaces(
        &self,
        display_id: &str,
        outputs: &macos::DisplayOutputs,
    ) -> Vec<WorkspaceId> {
        self.config
            .ordered_workspaces()
            .filter(|workspace| {
                let selector = self.workspaces.assignment(*workspace);
                self.workspaces.is_enabled(*workspace)
                    && self.scope.manages_selector(selector)
                    && outputs.resolve(selector).id == display_id
            })
            .collect()
    }

    /// `workspace`'s menu-bar glyph, its tiles in `preview` space.
    pub(super) fn workspace_preview(
        &self,
        workspace: WorkspaceId,
        preview: crate::Rect,
    ) -> UiHudLayout {
        if self.workspaces.deck(workspace).is_monocle() {
            UiHudLayout::Fullscreen
        } else {
            UiHudLayout::Split(self.preview_layout(workspace, preview))
        }
    }
}

pub(super) fn output_for(
    workspaces: &Workspaces,
    workspace: WorkspaceId,
    outputs: &macos::DisplayOutputs,
) -> crate::Rect {
    outputs.resolve(workspaces.assignment(workspace)).frame
}

fn selector_available(selector: &DisplaySelector, outputs: &macos::DisplayOutputs) -> bool {
    outputs.resolve_exact(selector).is_some()
}

pub(super) fn failed_results(results: &[macos::ApplyResult]) -> usize {
    results
        .iter()
        .filter(|r| r.outcome != macos::ApplyOutcome::Applied)
        .count()
}
