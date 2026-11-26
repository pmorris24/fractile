mod affinity;
mod omission;
mod readback;
mod routing;
pub(super) use omission::Omission;
#[cfg(test)]
pub(super) use readback::ASYNC_BOUNCE_SETTLE;
pub(super) use readback::{APPLY_ATTEMPT_LIMIT, AppliedFrame, record_apply_attempt};

use affinity::{FrozenAffinityPolicy, FrozenWindowAffinity};

use super::*;
use crate::journal::PlacementSource;

/// How long a bound window may be missing from its app's Accessibility snapshot
/// while the window server still shows it on screen before the omission is
/// believed. Electron apps rebuild their accessibility tree under load (Slack
/// starting a huddle) and briefly report a partial window list; one such
/// snapshot must not retire a slot.
pub(super) const OMISSION_SETTLE: Duration = Duration::from_secs(2);

struct ReconcileDisplayContext<'a> {
    observation: &'a crate::NativeObservation,
    affinity_policy: &'a FrozenAffinityPolicy,
    server_frames: Option<&'a HashMap<u32, crate::Rect>>,
    onscreen_frames: Option<&'a HashMap<u32, crate::Rect>>,
    cancelled: &'a AtomicBool,
    failed: &'a mut usize,
    close_recheck: &'a mut Option<Instant>,
    fullscreen_hud: &'a mut Option<WorkspaceId>,
}

impl<B: WorkspaceBackend> WorkspaceRuntime<B> {
    fn restore_visible(
        &mut self,
        visible: &[ResolvedWorkspace],
        cancelled: &AtomicBool,
        failed: &mut usize,
    ) {
        if self.parked.is_empty() {
            return;
        }
        // A window pinned while it was parked (a pin rule reloaded) shows wherever it lives.
        let windows: Vec<_> = visible
            .iter()
            .flat_map(|resolved| self.workspaces.windows(resolved.workspace))
            .chain(
                self.parked
                    .keys()
                    .filter(|window| self.workspaces.is_pinned(window))
                    .cloned(),
            )
            .collect();
        for window in windows {
            if cancelled.load(Ordering::Acquire) {
                return;
            }
            if !self.parked.contains_key(&window) {
                continue;
            }
            let now = self.backend.now();
            let retry_throttled = self.parked.last_write(&window).is_some_and(|attempted_at| {
                now.saturating_duration_since(attempted_at) < RESTORE_RETRY_INTERVAL
            });
            if let Some(target) = self.parked.summon_destination(&window) {
                if !matches!(
                    self.finish_scratchpad_summon(&window, target, retry_throttled),
                    macos::ApplyOutcome::Applied | macos::ApplyOutcome::Gone
                ) {
                    *failed += 1;
                }
                continue;
            }
            // Throttle retries of restores awaiting window-server confirmation
            // so a slow app is not written to and logged about every cycle.
            if retry_throttled {
                continue;
            }
            if !matches!(
                self.restore_window(&window),
                macos::ApplyOutcome::Applied | macos::ApplyOutcome::Gone
            ) {
                *failed += 1;
            }
        }
    }

    pub(super) fn cycle(
        &mut self,
        outputs: macos::DisplayOutputs,
        cancelled: &AtomicBool,
    ) -> Result<CycleStatus, String> {
        let mut failed = 0;
        if cancelled.load(Ordering::Acquire) {
            return Err("workspace operation cancelled".into());
        }
        if let Some(until) = self.space_transition_until() {
            return Ok(self.defer_for_space_transition(until, &outputs));
        }
        self.note_outputs(&outputs);
        // Parking is durable before workspace ownership. A crash between those
        // writes must not strand an unowned window behind an otherwise valid state.
        // Hidden scratchpad windows are unowned on purpose until summoned.
        let unowned = self
            .parked
            .keys()
            .filter(|window| {
                self.workspaces.workspace_of_any(window).is_none()
                    && !self.workspaces.scratchpad().contains(window)
            })
            .cloned()
            .collect::<Vec<_>>();
        let pending = self.restore_windows(unowned);
        if pending > 0 {
            return Err(format!(
                "Recovery pending for {pending} unowned parked window(s)"
            ));
        }
        self.follow_displays(&outputs);
        let visible = self.visible_workspaces(&outputs);
        let frontmost = self.backend.frontmost_application();
        let focused = self.backend.focused_window();
        let focused_is_frontmost = focused.as_ref().is_some_and(|focused| {
            frontmost
                .as_ref()
                .is_none_or(|frontmost| &focused.app == frontmost)
        });
        // A pinned window shows on every workspace, so focusing it never switches.
        let focus_workspace = if focused_is_frontmost {
            focused
                .as_ref()
                .filter(|focused| !self.workspaces.is_pinned(focused))
                .and_then(|focused| self.workspaces.workspace_of_any(focused))
        } else {
            frontmost
                .as_ref()
                .and_then(|app| self.workspaces.workspace_for_app(app))
        };
        // A guard lasts until Accessibility reports a different focus, and a
        // report a focus request in flight explains is no focus change at all:
        // it must neither switch a display nor rewrite history (#399).
        self.retry_follow_after_space_change();
        let suppressed = self.focus.holds_back(
            self.backend.now(),
            focused.as_ref(),
            frontmost.as_ref(),
            focused_is_frontmost,
        );
        if !suppressed {
            self.clear_suppressed_focus();
            if let Some(workspace) = focus_workspace {
                if !visible
                    .iter()
                    .any(|resolved| resolved.workspace == workspace)
                {
                    let input = self
                        .backend
                        .recent_input()
                        .map_or_else(|| "input unknown".into(), |input| input.to_string());
                    log_event!(
                        "focus follow: {:?} (frontmost {:?}) -> workspace {} ({})",
                        focused,
                        frontmost,
                        workspace,
                        input
                    );
                    let follow_started = self.backend.now();
                    // A follow answers focus the user already moved: the pointer stays.
                    let followed = self.switch_with(
                        workspace,
                        outputs.clone(),
                        cancelled,
                        super::switching::RepeatSwitch::KeepFocus,
                        super::commands::CommandOrigin::Passive,
                    );
                    // An activation (or a finished drag) cancels background
                    // cycles, often the very one this follow answers (a
                    // notification click). A cancelled follow lets the next cycle
                    // follow again; a switch that fails for any other reason keeps
                    // focus suppressed so it can't loop, unless a macOS Space
                    // slide explains the failure.
                    if matches!(&followed, Err(error) if error.ends_with("cancelled")) {
                        self.clear_suppressed_focus();
                    } else if followed.is_err() {
                        self.focus.follow_failed(follow_started);
                        if self.retry_follow_after_space_change() {
                            let until = self
                                .space_transition_until()
                                .unwrap_or_else(|| self.backend.now() + SPACE_TRANSITION_SETTLE);
                            return Ok(self.defer_for_space_transition(until, &outputs));
                        }
                    }
                    return followed;
                }
                // Focus moved to a workspace already on screen, such as a click
                // on another display. Option+Tab goes back from here.
                self.record_focused_workspace(workspace);
            }
        }
        for resolved in &visible {
            self.workspaces
                .seed_display_front(&resolved.display_id, &resolved.selector);
        }
        // One window-server snapshot serves every authoritative-frame check in
        // this cycle; per-window queries would each copy the full window list.
        let server_snapshot = self.backend.window_server_frames();
        let empty_server_frames = HashMap::new();
        let server_frames = server_snapshot.as_ref().unwrap_or(&empty_server_frames);
        self.recover_stale_parked_windows(&outputs, server_frames);
        self.pin_parked_floats_by_rule();
        self.restore_visible(&visible, cancelled, &mut failed);
        let mut fullscreen_hud =
            self.park_inactive(&outputs, &visible, server_frames, cancelled, &mut failed);
        self.recover_stranded_visible(&outputs, &visible, server_frames, cancelled, &mut failed);
        if cancelled.load(Ordering::Acquire) {
            return Err("workspace operation cancelled".into());
        }
        let observation = self
            .backend
            .snapshot(0, &outputs)
            .map_err(|error| format!("Observation error: {error}"))?;
        let mut affinity_policy =
            self.freeze_window_affinities(&observation, &visible, server_frames)?;
        self.route_before_reconcile(
            &mut affinity_policy,
            &observation,
            &visible,
            &outputs,
            &mut failed,
        );
        if let Some(workspace) = affinity_policy.fullscreen_hud {
            fullscreen_hud = Some(workspace);
        }
        // Taken after the Accessibility snapshot so a window listed here was
        // on screen no earlier than the snapshot that may have omitted it.
        let onscreen_snapshot = self.backend.onscreen_window_frames();
        let mut close_recheck = None;
        for resolved in &visible {
            if !self.scope.manages_selector(&resolved.selector) {
                continue;
            }
            self.reconcile_display(
                resolved,
                ReconcileDisplayContext {
                    observation: &observation,
                    affinity_policy: &affinity_policy,
                    server_frames: server_snapshot.as_ref(),
                    onscreen_frames: onscreen_snapshot.as_ref(),
                    cancelled,
                    failed: &mut failed,
                    close_recheck: &mut close_recheck,
                    fullscreen_hud: &mut fullscreen_hud,
                },
            )?;
        }
        let mut attempted_focus_restore = HashSet::new();
        for resolved in &visible {
            if !self.focus.restores_fullscreen(resolved.workspace) {
                continue;
            }
            attempted_focus_restore.insert(resolved.workspace);
            let Some(window) = self.workspaces.preferred_focus(resolved.workspace) else {
                self.focus.settle_fullscreen(resolved.workspace);
                continue;
            };
            let restored = if focus_workspace == Some(resolved.workspace) {
                self.backend.focus_window(&window)
            } else {
                self.backend.raise_window(&window)
            };
            self.focus.settle_fullscreen(resolved.workspace);
            if !restored {
                failed += 1;
            }
        }
        // New tiles split the effectively focused tile: a report that lags a
        // focus command must not move the split back to the tile it left (#411).
        let remembered_focus = self.resolve_focus(focused.clone());
        if focused_is_frontmost
            && let Some(focused) = remembered_focus.as_ref()
            && let Some(workspace) = self.workspaces.workspace_of(focused)
        {
            self.workspaces
                .engine_mut(workspace)
                .remember_insertion_focus(focused);
        }
        if focused_is_frontmost
            && let Some(focused) = remembered_focus.as_ref()
            && let Some(workspace) = self.workspaces.workspace_of(focused)
            && self.workspaces.deck(workspace).is_monocle()
            && !attempted_focus_restore.contains(&workspace)
            && visible
                .iter()
                .any(|resolved| resolved.workspace == workspace)
        {
            self.workspaces.remember_focus(workspace, focused);
        }
        self.record_window_memory(server_frames, &observation);
        // Creation can wake the worker before CG orders the window on screen.
        // Only a recent create/activation on the main thread enables retries.
        let missing_frontmost_window = frontmost.filter(|app| {
            let windows = &affinity_policy.windows;
            match focused.as_ref().filter(|window| &window.app == app) {
                Some(window) => !windows.contains_key(window),
                None => !windows.keys().any(|window| &window.app == app),
            }
        });
        self.journal.observation = Some(observation);
        self.persist_workspace_state_if_changed()?;
        let mut status = self.status_for_visible(failed, &visible, &outputs);
        if let Some(workspace) = fullscreen_hud.filter(|&ws| self.is_stacked_fullscreen(ws)) {
            status.hud = Some(self.hud_for(
                workspace,
                UiHudKind::Fullscreen { enabled: true },
                None,
                &outputs,
            ));
        }
        self.keep_hud_current(&mut status, &visible, &outputs);
        status.close_recheck = close_recheck;
        status.missing_frontmost_window = missing_frontmost_window;
        self.settle_tab_detach(status, &outputs, cancelled)
    }

    /// With the HUD kept on screen, a cycle that changes what it shows (a window opened or
    /// closed, a workspace switched by other means) sends it the new rows.
    fn keep_hud_current(
        &mut self,
        status: &mut CycleStatus,
        visible: &[super::runtime::ResolvedWorkspace],
        outputs: &macos::DisplayOutputs,
    ) {
        if !self.config.hud_stay() {
            self.stay_hud = None;
            return;
        }
        if let Some(hud) = &status.hud {
            self.stay_hud = Some(hud.displays.clone());
            return;
        }
        let workspace = visible
            .first()
            .map_or(WorkspaceId::ONE, |resolved| resolved.workspace);
        let hud = self.hud_for(workspace, super::UiHudKind::Workspace, None, outputs);
        if self.stay_hud.as_ref() != Some(&hud.displays) {
            self.stay_hud = Some(hud.displays.clone());
            status.hud = Some(hud);
        }
    }

    /// Bookkeeping shared by every path that moves a window between workspaces.
    pub(super) fn clear_relocation_evidence(
        &mut self,
        source: WorkspaceId,
        target: WorkspaceId,
        window: &crate::PhysicalWindowId,
    ) {
        self.constrained[source.index()].remove(window);
        self.constrained[target.index()].remove(window);
        self.applied[source.index()].remove(window);
        self.applied[target.index()].remove(window);
        self.apply_attempts[source.index()].remove(window);
        self.apply_attempts[target.index()].remove(window);
    }

    fn reconcile_display(
        &mut self,
        resolved: &ResolvedWorkspace,
        context: ReconcileDisplayContext<'_>,
    ) -> Result<(), String> {
        let ReconcileDisplayContext {
            observation,
            affinity_policy,
            server_frames,
            onscreen_frames,
            cancelled,
            failed,
            close_recheck,
            fullscreen_hud,
        } = context;
        let workspace = resolved.workspace;
        let output = resolved.output;
        // A new window centered over this display's Dock or menu-bar strip is
        // on this display, though outside its tiling area (#394).
        let screen = resolved.screen;
        let ticket = {
            let engine = self.workspaces.engine_mut(workspace);
            engine.invalidate();
            engine.begin_cycle()
        };
        let mut scoped_observation = observation.clone();
        scoped_observation.cycle = ticket.cycle;
        let settings = self.layout_settings(output);
        for app in &mut scoped_observation.applications {
            if let crate::AppObservation::Complete {
                app: _,
                focused,
                windows,
            } = app
            {
                windows.retain(|window| {
                    let affinity = affinity_policy
                        .windows
                        .get(&window.id)
                        .copied()
                        .unwrap_or(FrozenWindowAffinity::CurrentDisplay);
                    match affinity {
                        FrozenWindowAffinity::Owned(owner)
                        | FrozenWindowAffinity::PendingReplacement(owner) => owner == workspace,
                        FrozenWindowAffinity::AmbiguousReplacement
                        | FrozenWindowAffinity::Floating
                        | FrozenWindowAffinity::Ignored => false,
                        FrozenWindowAffinity::Inferred(target)
                            if affinity_policy.visible_workspaces.contains(&target) =>
                        {
                            target == workspace
                        }
                        FrozenWindowAffinity::ExplicitRoute(_)
                        | FrozenWindowAffinity::DetachedTab(_)
                        | FrozenWindowAffinity::Inferred(_)
                        | FrozenWindowAffinity::CurrentDisplay => {
                            screen.contains_center(window.frame)
                                && self
                                    .workspaces
                                    .workspace_of(&window.id)
                                    .is_none_or(|owner| owner == workspace)
                        }
                    }
                });
                if focused
                    .as_ref()
                    .is_some_and(|focused| !windows.iter().any(|window| &window.id == focused))
                {
                    *focused = None;
                }
            }
        }
        self.guard_omitted_windows(
            workspace,
            &mut scoped_observation,
            onscreen_frames,
            close_recheck,
        );
        let apps_with_window_server_evidence = self.apps_with_window_server_evidence(
            workspace,
            &scoped_observation,
            server_frames,
            onscreen_frames,
            close_recheck,
        );
        live::add_absent_applications_with(
            &mut scoped_observation,
            &mut self.known_apps[workspace.index()],
            |app| self.backend.is_process_running(app),
            |app| server_frames.is_none() || apps_with_window_server_evidence.contains(app),
        );
        let reconciliation = self
            .workspaces
            .engine_mut(workspace)
            .accept(ticket, &scoped_observation, &settings)
            .map_err(|error| format!("stale workspace cycle: {error:?}"))?;
        self.log_binding_decisions(workspace, &reconciliation.decisions, affinity_policy);
        for decision in &reconciliation.decisions {
            let Decision::WindowInserted { window, .. } = decision else {
                continue;
            };
            let source = match affinity_policy.windows.get(window) {
                Some(FrozenWindowAffinity::ExplicitRoute(_)) => PlacementSource::Rule,
                Some(FrozenWindowAffinity::Inferred(_)) => PlacementSource::Recall,
                Some(FrozenWindowAffinity::CurrentDisplay) | None => PlacementSource::Active,
                Some(_) => continue,
            };
            if let Some(owner) = self.workspaces.workspace_of(window) {
                self.note_placement(window, source, owner);
            }
        }
        let inserted_here = reconciliation
            .decisions
            .iter()
            .any(|decision| matches!(decision, Decision::WindowInserted { .. }));
        let bound: HashSet<_> = self
            .workspaces
            .engine(workspace)
            .bound_windows()
            .into_iter()
            .collect();
        if inserted_here && self.workspaces.deck(workspace).is_monocle() {
            *fullscreen_hud = Some(workspace);
        }
        self.workspaces.prune_intents(workspace);
        self.clear_fullscreen_restore_if_inactive(workspace);
        self.constrained[workspace.index()].retain(|window, _| bound.contains(window));
        self.applied[workspace.index()].retain(|window, _| bound.contains(window));
        self.apply_attempts[workspace.index()].retain(|window, _| bound.contains(window));
        let workspaces = &self.workspaces;
        self.minimum_sizes
            .retain(|window, _| workspaces.workspace_of(window).is_some());
        let observed_frames = live::observed_frames(&scoped_observation);
        let engine = self.workspaces.engine_mut(workspace);
        engine.fit_minimums(&settings, &self.minimum_sizes);
        let mut placements = engine.placements(&settings);
        for placement in &placements {
            engine.record_placement_target(placement.slot, placement.frame);
        }
        self.project_deck(workspace, &mut placements, &settings);
        let now = self.backend.now();
        if !scoped_observation.mouse_down {
            for placement in &placements {
                let Some(window) = placement.window.as_ref() else {
                    continue;
                };
                let Some(&observed) = observed_frames.get(window) else {
                    continue;
                };
                let target = placement.frame;
                if observed.approximately_equals(target, settings.frame_tolerance) {
                    let settling = self.applied[workspace.index()]
                        .get(window)
                        .is_some_and(|applied| applied.is_settling(target, now));
                    if !settling {
                        self.applied[workspace.index()].remove(window);
                        self.constrained[workspace.index()].remove(window);
                        self.apply_attempts[workspace.index()].remove(window);
                    }
                    continue;
                }
                if self.constrained[workspace.index()]
                    .get(window)
                    .is_some_and(|(constrained_target, _)| *constrained_target != target)
                {
                    self.constrained[workspace.index()].remove(window);
                }
                if self.apply_attempts[workspace.index()]
                    .get(window)
                    .is_some_and(|(attempted_target, _)| *attempted_target != target)
                {
                    self.apply_attempts[workspace.index()].remove(window);
                }
                // A window that kept its size but left this display was moved
                // there, not refused its frame: the next write brings it back (#392).
                let moved_away = observed.width.abs_diff(target.width)
                    <= settings.frame_tolerance.unsigned_abs()
                    && observed.height.abs_diff(target.height)
                        <= settings.frame_tolerance.unsigned_abs()
                    && !screen.contains_center(observed);
                if self.applied[workspace.index()]
                    .remove(window)
                    .is_some_and(|applied| applied.target == target)
                    && !moved_away
                {
                    self.constrained[workspace.index()].insert(window.clone(), (target, observed));
                    record_apply_attempt(
                        &mut self.apply_attempts[workspace.index()],
                        workspace,
                        window,
                        target,
                    );
                }
                if self.apply_attempts[workspace.index()]
                    .get(window)
                    .is_some_and(|(attempted_target, count)| {
                        *attempted_target == target && *count >= APPLY_ATTEMPT_LIMIT
                    })
                {
                    self.constrained[workspace.index()]
                        .entry(window.clone())
                        .or_insert((target, observed));
                }
            }
        }
        placements.retain(|placement| {
            live::needs_application(placement, &observed_frames, settings.frame_tolerance)
        });
        placements.retain(|placement| {
            let Some(window) = placement.window.as_ref() else {
                return false;
            };
            if self.apply_attempts[workspace.index()]
                .get(window)
                .is_some_and(|(target, count)| {
                    *target == placement.frame && *count >= APPLY_ATTEMPT_LIMIT
                })
            {
                return false;
            }
            !self.constrained[workspace.index()]
                .get(window)
                .zip(observed_frames.get(window))
                .is_some_and(|((target, accepted), observed)| {
                    *target == placement.frame
                        && observed.approximately_equals(*accepted, settings.frame_tolerance)
                })
        });
        if !placements.is_empty() {
            let results = self
                .backend
                .apply_observed(&placements, &settings, cancelled);
            self.record_results(workspace, &results);
            let initially_failed = failed_results(&results);
            *failed += self
                .refit_minimum_sizes(workspace, output, results, cancelled)
                .map_or(initially_failed, |status| status.failed);
        }
        Ok(())
    }

    /// Slot-identity changes are the events that explain "my window moved"; the
    /// steady-state decisions (unavailable, deferred) would only add noise.
    fn log_binding_decisions(
        &self,
        workspace: WorkspaceId,
        decisions: &[Decision],
        affinity_policy: &FrozenAffinityPolicy,
    ) {
        for decision in decisions {
            match decision {
                Decision::WindowInserted { slot, window } => {
                    let bundle = self.bundle_id(&window.app).unwrap_or("unknown");
                    let affinity = affinity_policy.windows.get(window);
                    log_event!(
                        "workspace {workspace}: bound {window:?} ({bundle}, {affinity:?}) to slot {slot:?}"
                    );
                }
                Decision::WindowRemoved { slot, window } => {
                    let bundle = self.bundle_id(&window.app).unwrap_or("unknown");
                    log_event!(
                        "workspace {workspace}: retired slot {slot:?} ({window:?} / {bundle} absent)"
                    );
                }
                Decision::NativeTabReplaced {
                    slot,
                    previous,
                    current,
                } => {
                    log_event!("workspace {workspace}: slot {slot:?} {previous:?} -> {current:?}");
                }
                Decision::ApplicationTerminated { app } => {
                    log_event!("workspace {workspace}: {app:?} terminated");
                }
                Decision::NativeTabAmbiguous { .. }
                | Decision::ObservationUnavailable { .. }
                | Decision::MouseDownDeferred => {}
            }
        }
    }
}
