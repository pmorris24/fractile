use super::*;
use crate::config::DispositionPatch;
use crate::journal::PlacementSource;
use crate::workspace::{FloatCause, Residency};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum FrozenWindowAffinity {
    Owned(WorkspaceId),
    PendingReplacement(WorkspaceId),
    AmbiguousReplacement,
    Floating,
    ExplicitRoute(WorkspaceId),
    /// The window a `move-tab` detach opened, sent to its target ahead of any
    /// route rule.
    DetachedTab(WorkspaceId),
    /// The app's existing workspace, or a remembered placement a relaunched
    /// application's window claimed.
    Inferred(WorkspaceId),
    CurrentDisplay,
    /// Never tiled, owned, or parked (ignore rule or parking exemption).
    Ignored,
}

/// A float sent to `workspace` from wherever it opened (`observed`).
pub(super) struct FloatHome {
    pub(super) window: crate::PhysicalWindowId,
    pub(super) workspace: WorkspaceId,
    pub(super) observed: crate::Rect,
    /// Where a recalled placement had it.
    pub(super) remembered: Option<crate::Rect>,
}

pub(super) struct FrozenAffinityPolicy {
    pub(super) windows: HashMap<crate::PhysicalWindowId, FrozenWindowAffinity>,
    /// Floats this observation gave a workspace (a route, a recall, a detached tab); routing
    /// brings each onto its workspace's display before reconciliation.
    pub(super) float_homes: Vec<FloatHome>,
    pub(super) visible_workspaces: HashSet<WorkspaceId>,
    /// A visible Fullscreen Layout workspace that gained a tiled window during
    /// affinity healing or pre-reconcile routing.
    pub(super) fullscreen_hud: Option<WorkspaceId>,
}

impl<B: WorkspaceBackend> WorkspaceRuntime<B> {
    pub(super) fn freeze_window_affinities(
        &mut self,
        observation: &crate::NativeObservation,
        visible: &[ResolvedWorkspace],
        server_frames: &HashMap<u32, crate::Rect>,
    ) -> Result<FrozenAffinityPolicy, String> {
        let has_floating = self.workspaces.floating_entries().next().is_some();
        if has_floating {
            let floating_apps = self
                .workspaces
                .floating_entries()
                .map(|(window, _)| window.app.clone())
                .collect::<HashSet<_>>();
            for app in floating_apps {
                if !self.backend.is_process_running(&app) {
                    for window in self.workspaces.floating_windows_for_app(&app) {
                        log_event!("forget floating {:?}: app exited", window);
                        self.workspaces.forget_floating(&window);
                    }
                }
            }
        }
        self.forget_exited_apps_on_hidden_workspaces(visible);

        self.close_recalls();
        // Before any new instance below is considered: a relaunch's new
        // process can appear in the same observation that reports the old
        // one terminated.
        self.note_absent_departures(observation);
        let mut affinities = HashMap::new();
        let mut float_homes = Vec::new();
        let mut fullscreen_hud = None;
        // Read once, and only if a Complete snapshot omits an owned float.
        let mut onscreen_frames = None;
        for application in &observation.applications {
            let crate::AppObservation::Complete { app, windows, .. } = application else {
                continue;
            };
            if has_floating {
                let floating_windows = self.workspaces.floating_windows_for_app(&app.id);
                for window in floating_windows {
                    if windows.iter().any(|observed| observed.id == window)
                        || self.parked.contains_key(&window)
                    {
                        self.omitted_windows.remove(&window);
                    } else if !self.omitted_float_is_settling(&window, &mut onscreen_frames) {
                        log_event!("forget floating {:?}: not observed and not parked", window);
                        self.workspaces.forget_floating(&window);
                        self.omitted_windows.remove(&window);
                    }
                }
            }
            self.prepare_recall(app, windows);
            let detached_tab = self.claim_detached_tab(app, windows);
            let mut app_owns_window = self.workspaces.app_owns_windows(&app.id);
            let mut app_owns_tiled = self.workspaces.app_owns_tiled_windows(&app.id);
            for window in windows {
                if let Some(ws) = self.heal_reappeared_window(window, visible, server_frames) {
                    fullscreen_hud = Some(ws);
                }
                if self.parked.contains_key(&window.id)
                    && matches!(
                        self.workspaces.residency(&window.id),
                        Some(Residency::Scratchpad { shown_on: None, .. })
                    )
                {
                    // Still hidden in the scratchpad (re-parked or settling); only an
                    // honored reappearance clears its parked record and lets it adopt.
                    affinities.insert(window.id.clone(), FrozenWindowAffinity::Ignored);
                    continue;
                }
                // A window that already holds a tile keeps it while it temporarily looks
                // non-resizable (an attached window-modal sheet), and a tile rule tiles it
                // whatever it looks like: user rule ⊕ built-in default ⊕ window traits.
                let tiled = self.workspaces.workspace_of(&window.id).is_some();
                let rules = self.config.patch(&app.bundle_id, window);
                let patch = rules.over(DispositionPatch::traits(window.resizable, tiled));
                if patch.is_ignored() {
                    if let Some(owner) = self.workspaces.workspace_of_any(&window.id) {
                        log_event!("release {:?} from workspace {}: ignored", window.id, owner);
                        self.forget_window(owner, &window.id);
                    }
                    affinities.insert(window.id.clone(), FrozenWindowAffinity::Ignored);
                    continue;
                }
                // An agent restart restores every float without saying why it floated, so its
                // first observation hands it to a matching float rule (removing the rule then
                // tiles it again, #160) or keeps it floating by hand. A float set by hand this
                // session stays manual whatever its rules say (#493). A window no workspace owns
                // yet floats by hand when the setup tour or a recalled placement floated it.
                self.workspaces
                    .settle_restored_float(&window.id, rules.floats());
                let recalled = self.memory.recalled(&window.id);
                let by_hand = self.workspaces.is_manual_float(&window.id)
                    || (self.workspaces.residency(&window.id).is_none()
                        && !rules.floats()
                        && (self.arranged_float(&window.id)
                            || recalled.is_some_and(|placement| placement.floating)));
                let floating = patch.floats() || by_hand;
                let cause = if by_hand {
                    FloatCause::Manual
                } else {
                    FloatCause::Automatic
                };
                if floating {
                    if let Some(owner) = self.workspaces.workspace_of(&window.id) {
                        self.forget_window(owner, &window.id);
                        self.workspaces.float(window.id.clone(), owner, cause);
                    }
                } else if self.workspaces.floating_workspace_of(&window.id).is_some() {
                    self.workspaces.forget_floating(&window.id);
                }
                let mut float_home = |runtime: &mut Self, target, frame| {
                    runtime.workspaces.float(window.id.clone(), target, cause);
                    float_homes.push(FloatHome {
                        window: window.id.clone(),
                        workspace: target,
                        observed: window.frame,
                        remembered: frame,
                    });
                    FrozenWindowAffinity::Floating
                };
                let affinity = if let Some(owner) = self.workspaces.workspace_of(&window.id) {
                    FrozenWindowAffinity::Owned(owner)
                } else if let Some((_, target)) = detached_tab
                    .as_ref()
                    .filter(|(detached, _)| *detached == window.id)
                {
                    app_owns_window = true;
                    if floating {
                        float_home(self, *target, None)
                    } else {
                        app_owns_tiled = true;
                        FrozenWindowAffinity::DetachedTab(*target)
                    }
                } else if !floating
                    && let Some(affinity) = {
                        let mut reserved = self.workspaces.replacement_workspaces(&window.id);
                        reserved.next().map(|workspace| {
                            if reserved.next().is_some() {
                                FrozenWindowAffinity::AmbiguousReplacement
                            } else {
                                FrozenWindowAffinity::PendingReplacement(workspace)
                            }
                        })
                    }
                {
                    affinity
                } else if floating && self.workspaces.floating_workspace_of(&window.id).is_some() {
                    FrozenWindowAffinity::Floating
                } else if floating && let Some(placement) = recalled {
                    app_owns_window = true;
                    self.note_placement(&window.id, PlacementSource::Recall, placement.workspace);
                    float_home(self, placement.workspace, Some(placement.frame))
                } else if floating {
                    let routed = patch.home.filter(|target| {
                        (!app_owns_window
                            || self.config.routes_every_window(&app.bundle_id, window))
                            && self
                                .scope
                                .manages_selector(self.workspaces.assignment(*target))
                    });
                    if let Some(target) = routed {
                        app_owns_window = true;
                        log_event!("route floating {:?} -> workspace {}", window.id, target);
                        self.note_placement(&window.id, PlacementSource::Rule, target);
                        float_home(self, target, None)
                    } else if let Some(resolved) = visible
                        .iter()
                        .find(|resolved| resolved.screen.contains_center(window.frame))
                    {
                        app_owns_window = true;
                        log_event!(
                            "adopt floating {:?} -> workspace {}",
                            window.id,
                            resolved.workspace
                        );
                        self.workspaces
                            .float(window.id.clone(), resolved.workspace, cause);
                        self.note_placement(
                            &window.id,
                            PlacementSource::Active,
                            resolved.workspace,
                        );
                        FrozenWindowAffinity::Floating
                    } else {
                        FrozenWindowAffinity::CurrentDisplay
                    }
                } else if let Some(target) = self.arranged_target(&window.id) {
                    // The setup tour's plan placed this open window: a route for it alone.
                    app_owns_window = true;
                    app_owns_tiled = true;
                    FrozenWindowAffinity::ExplicitRoute(target)
                } else if let Some(target) = patch.home {
                    if !self
                        .scope
                        .manages_selector(self.workspaces.assignment(target))
                    {
                        return Err(format!(
                            "window rule routes to unmanaged workspace {}",
                            target
                        ));
                    }
                    if !app_owns_tiled || self.config.routes_every_window(&app.bundle_id, window) {
                        app_owns_window = true;
                        app_owns_tiled = true;
                        FrozenWindowAffinity::ExplicitRoute(target)
                    } else {
                        FrozenWindowAffinity::CurrentDisplay
                    }
                } else {
                    app_owns_window = true;
                    app_owns_tiled = true;
                    recalled.map_or(FrozenWindowAffinity::CurrentDisplay, |placement| {
                        FrozenWindowAffinity::Inferred(placement.workspace)
                    })
                };
                self.workspaces.set_rule_pinned(&window.id, patch.pins());
                affinities.insert(window.id.clone(), affinity);
            }
        }
        Ok(FrozenAffinityPolicy {
            windows: affinities,
            float_homes,
            visible_workspaces: visible.iter().map(|resolved| resolved.workspace).collect(),
            fullscreen_hud,
        })
    }

    /// Hidden workspaces are not reconciled, so an app that quit while its
    /// windows were parked would otherwise keep their tiles until the
    /// workspace was shown again.
    fn forget_exited_apps_on_hidden_workspaces(&mut self, visible: &[ResolvedWorkspace]) {
        let mut running = HashMap::new();
        for workspace in WorkspaceId::all() {
            if visible
                .iter()
                .any(|resolved| resolved.workspace == workspace)
            {
                continue;
            }
            let apps = self
                .workspaces
                .app_instances(workspace)
                .cloned()
                .collect::<HashSet<_>>();
            for app in apps {
                let alive = *running
                    .entry(app.clone())
                    .or_insert_with(|| self.backend.is_process_running(&app));
                if alive {
                    continue;
                }
                let windows = self
                    .workspaces
                    .engine(workspace)
                    .model()
                    .workspace
                    .slots
                    .iter()
                    .filter(|slot| slot.belongs_to(&app))
                    .map(|slot| slot.identity_window().clone())
                    .collect::<Vec<_>>();
                self.note_departure(&app);
                let canvas = self.known_workspace_canvas(workspace);
                self.workspaces
                    .forget_app_with(workspace, &app, self.config.layout(), canvas);
                for window in &windows {
                    self.forget_window(workspace, window);
                }
                self.known_apps[workspace.index()].remove(&app);
                log_event!(
                    "workspace {workspace}: forgot {app:?}: app exited while hidden ({} tile(s))",
                    windows.len()
                );
                let parked = windows
                    .into_iter()
                    .filter(|window| self.parked.contains_key(window))
                    .collect();
                self.restore_windows(parked);
            }
        }
    }
}
