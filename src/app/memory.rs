use std::collections::BTreeMap;

use crate::{
    journal::{CORRECTION_WINDOW, PlacementSource},
    window_memory::{
        RememberedWindow, WindowCandidate, WindowMemory, normalize_document, normalize_title,
        same_window_list,
    },
};

use super::*;

/// How long a newly launched application's windows may claim its remembered
/// placements. Windows it opens later are adopted normally.
pub(super) const RECALL_WINDOW: Duration = Duration::from_secs(60);
/// How soon after an application quits a new instance of it counts as a
/// relaunch (an update or restart) that returns to its remembered placements.
pub(super) const RELAUNCH_WINDOW: Duration = Duration::from_secs(30);
/// How long a changed arrangement must hold before it replaces an
/// application's memory. Quitting closes windows one by one before the
/// process exits; those intermediate states must not overwrite the memory.
pub(super) const MEMORY_SETTLE: Duration = Duration::from_secs(10);

/// The placement fields a recalled window needs once it has claimed an entry.
#[derive(Clone, Copy)]
pub(super) struct RecalledPlacement {
    pub(super) workspace: WorkspaceId,
    pub(super) frame: crate::Rect,
    pub(super) floating: bool,
}

/// Last observed local `AXDocument` and title for a live window, retained while
/// the window is parked off-screen so inactive workspaces keep their identity.
#[derive(Clone, Default)]
pub(super) struct WindowIdentity {
    document: Option<String>,
    title: Option<String>,
}

/// One relaunched application instance claiming its remembered placements.
pub(super) struct Recall {
    bundle_id: String,
    pub(super) opened: Instant,
    /// Remembered placements not yet claimed by a window.
    remaining: Vec<RememberedWindow>,
    claimed: HashMap<crate::PhysicalWindowId, RecalledPlacement>,
}

/// Where and why Fractile put a window it just saw, kept for
/// [`CORRECTION_WINDOW`] so a hand move can be recognized as a correction.
pub(super) struct Placement {
    source: PlacementSource,
    workspace: WorkspaceId,
    pub(super) at: Instant,
}

#[derive(Default)]
pub(super) struct MemoryState {
    pub(super) memory: WindowMemory,
    pub(super) recalls: HashMap<AppInstanceId, Recall>,
    pub(super) pending: HashMap<String, (Vec<RememberedWindow>, Instant)>,
    /// Every observed instance's bundle. An instance without an open recall
    /// may have its ownership recorded.
    pub(super) bundles: HashMap<AppInstanceId, String>,
    /// When window memory was restored for this agent run. Placement recall
    /// applies only during the startup window after Fractile launches.
    pub(super) started_at: Option<Instant>,
    /// Bundles already seen in this session; mid-session launches open on
    /// the active workspace instead of recalling historical placements.
    pub(super) seen_bundles: HashSet<String>,
    /// When an instance of each bundle last quit. A new instance within
    /// [`RELAUNCH_WINDOW`] is a relaunch and recalls the placements.
    pub(super) departed: HashMap<String, Instant>,
    /// Instances already considered for a recall, on their first observation
    /// with windows.
    pub(super) considered: HashSet<AppInstanceId>,
    /// Recent placements of newly seen windows.
    pub(super) placements: HashMap<crate::PhysicalWindowId, Placement>,
    /// Last observed local `AXDocument` and title for managed windows.
    pub(super) identities: HashMap<crate::PhysicalWindowId, WindowIdentity>,
}

impl MemoryState {
    pub(super) fn recalled(&self, window: &crate::PhysicalWindowId) -> Option<RecalledPlacement> {
        self.recalls
            .get(&window.app)
            .and_then(|recall| recall.claimed.get(window))
            .copied()
    }
}

impl<B: WorkspaceBackend> WorkspaceRuntime<B> {
    /// Close recalls whose window lapsed, whose placements are all claimed,
    /// or whose process exited.
    pub(super) fn close_recalls(&mut self) {
        let now = self.backend.now();
        let backend = &mut self.backend;
        let closed: Vec<_> = self
            .memory
            .recalls
            .extract_if(|app, recall| {
                now.saturating_duration_since(recall.opened) >= RECALL_WINDOW
                    || recall.remaining.is_empty()
                    || !backend.is_process_running(app)
            })
            .map(|(_, recall)| recall)
            .filter(|recall| !recall.remaining.is_empty())
            .collect();
        for recall in closed {
            log_event!(
                "recall closed: {} left {} remembered placement(s) unclaimed",
                recall.bundle_id,
                recall.remaining.len()
            );
            self.journal_event(crate::journal::JournalEvent::RecallExpired {
                bundle_id: recall.bundle_id,
                unclaimed: recall.remaining.len(),
            });
        }
    }

    /// Decide whether an observed application instance is a relaunch that may
    /// recall remembered placements, and assign them to its unowned windows.
    pub(super) fn prepare_recall(
        &mut self,
        app: &crate::ObservedApplication,
        windows: &[crate::ObservedWindow],
    ) {
        self.memory
            .bundles
            .insert(app.id.clone(), app.bundle_id.clone());
        if windows.is_empty() {
            return;
        }
        if self.memory.considered.insert(app.id.clone()) {
            let first_seen = self.memory.seen_bundles.insert(app.bundle_id.clone());
            self.open_recall(app, first_seen, windows.len());
        }
        let Some(recall) = self.memory.recalls.get_mut(&app.id) else {
            return;
        };
        let candidates: Vec<_> = windows
            .iter()
            .filter(|window| {
                !recall.claimed.contains_key(&window.id)
                    && self.workspaces.workspace_of_any(&window.id).is_none()
                    && self
                        .workspaces
                        .replacement_workspaces(&window.id)
                        .next()
                        .is_none()
                    && {
                        let patch = self.config.patch(&app.bundle_id, window);
                        !patch.is_ignored() && patch.home.is_none()
                    }
            })
            .map(|window| WindowCandidate {
                id: window.id.clone(),
                frame: window.frame,
                document: window.document.as_deref(),
                title: window.title.as_deref(),
            })
            .collect();
        if candidates.is_empty() {
            return;
        }
        let assignments = crate::window_memory::assign(&recall.remaining, &candidates);
        let mut claimed_indices = Vec::new();
        let mut events = Vec::new();
        for (window, index) in assignments {
            let entry = &recall.remaining[index];
            let placement = RecalledPlacement {
                workspace: entry.workspace,
                frame: entry.frame,
                floating: entry.floating,
            };
            log_event!(
                "recall: {:?} ({}) -> workspace {}{}",
                window,
                recall.bundle_id,
                placement.workspace,
                if placement.floating {
                    " (floating)"
                } else {
                    ""
                }
            );
            events.push(crate::journal::JournalEvent::Recall {
                bundle_id: recall.bundle_id.clone(),
                window: window.window_id,
                workspace: placement.workspace,
                floating: placement.floating,
            });
            recall.claimed.insert(window, placement);
            claimed_indices.push(index);
        }
        claimed_indices.sort_unstable();
        for index in claimed_indices.into_iter().rev() {
            recall.remaining.remove(index);
        }
        for event in events {
            self.journal_event(event);
        }
    }

    /// Record that an instance quit, so a new instance of its bundle soon
    /// after is treated as a relaunch.
    pub(super) fn note_departure(&mut self, app: &AppInstanceId) {
        self.memory.considered.remove(app);
        if let Some(bundle_id) = self.memory.bundles.remove(app)
            && is_rememberable_bundle_id(&bundle_id)
        {
            let now = self.backend.now();
            self.memory.departed.insert(bundle_id, now);
        }
    }

    /// The native snapshot never reports a quit app; it simply stops listing
    /// it. Note the departure of every known instance that is absent and no
    /// longer running, whichever workspace its windows were on.
    pub(super) fn note_absent_departures(&mut self, observation: &crate::NativeObservation) {
        let present: HashSet<_> = observation
            .applications
            .iter()
            .map(crate::AppObservation::app_id)
            .collect();
        let absent: Vec<_> = self
            .memory
            .bundles
            .keys()
            .filter(|app| !present.contains(app))
            .cloned()
            .collect();
        for app in absent {
            if !self.backend.is_process_running(&app) {
                self.note_departure(&app);
            }
        }
    }

    /// Remember why a newly seen window landed on `workspace`.
    pub(super) fn note_placement(
        &mut self,
        window: &crate::PhysicalWindowId,
        source: PlacementSource,
        workspace: WorkspaceId,
    ) {
        let now = self.backend.now();
        self.memory
            .placements
            .retain(|_, placement| now.saturating_duration_since(placement.at) < CORRECTION_WINDOW);
        self.memory.placements.insert(
            window.clone(),
            Placement {
                source,
                workspace,
                at: now,
            },
        );
    }

    /// The user moved `window` from `from` to `to` by hand. Soon after Fractile
    /// placed it there, that placement was wrong: journal it, and when the
    /// window had opened on the active workspace, make its app return to its
    /// remembered placements from now on.
    pub(super) fn note_hand_move(
        &mut self,
        window: &crate::PhysicalWindowId,
        from: WorkspaceId,
        to: WorkspaceId,
    ) {
        let Some(placement) = self.memory.placements.remove(window) else {
            return;
        };
        let elapsed = self.backend.now().saturating_duration_since(placement.at);
        if elapsed >= CORRECTION_WINDOW || placement.workspace != from {
            return;
        }
        let bundle_id = self.bundle_id(&window.app).unwrap_or_default().to_owned();
        log_event!(
            "wrong placement: {window:?} ({bundle_id}, {:?}) moved by hand {from} -> {to} after {}ms",
            placement.source,
            elapsed.as_millis()
        );
        self.journal_event(crate::journal::JournalEvent::WrongPlacement {
            bundle_id: bundle_id.clone(),
            window: window.window_id,
            source: placement.source,
            placed: from,
            moved_to: to,
            after_ms: u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX),
        });
        if placement.source == PlacementSource::Active
            && is_rememberable_bundle_id(&bundle_id)
            && self.memory.memory.mark_returning(&bundle_id)
        {
            log_event!("{bundle_id} now returns to its remembered workspace on every launch");
            if let Err(error) = self.backend.persist_window_memory(&self.memory.memory) {
                log_event!("failed to persist window memory: {error}");
            }
        }
    }

    /// Whether a new instance replaces one of its bundle: another instance
    /// quit within [`RELAUNCH_WINDOW`], or still owns windows (a relaunch that
    /// starts the new process before quitting the old one).
    fn relaunched(&mut self, app: &crate::ObservedApplication) -> bool {
        let now = self.backend.now();
        self.memory
            .departed
            .retain(|_, quit| now.saturating_duration_since(*quit) < RELAUNCH_WINDOW);
        if self.workspaces.app_owns_windows(&app.id) {
            return false;
        }
        self.memory.departed.contains_key(&app.bundle_id)
            || self.memory.bundles.iter().any(|(other, bundle_id)| {
                other != &app.id
                    && bundle_id == &app.bundle_id
                    && self.workspaces.app_owns_windows(other)
            })
    }

    /// Open a recall for an instance that owns no windows yet and whose bundle
    /// has remembered placements it may claim: one seen for the first time
    /// during startup, a relaunch, any new instance of a returning app, or one
    /// restoring a multi-workspace arrangement however long it was closed.
    fn open_recall(
        &mut self,
        app: &crate::ObservedApplication,
        first_seen: bool,
        presented: usize,
    ) {
        if !is_rememberable_bundle_id(&app.bundle_id) {
            return;
        }
        let now = self.backend.now();
        let startup = first_seen
            && self
                .memory
                .started_at
                .is_some_and(|started| now.saturating_duration_since(started) < RECALL_WINDOW);
        let returning = self.memory.memory.returns(&app.bundle_id);
        let restoring = self.restores_arrangement(&app.bundle_id, presented);
        if !(startup || returning || restoring || self.relaunched(app))
            || self.config.has_bundle_route(&app.bundle_id)
        {
            return;
        }
        if self.workspaces.app_owns_windows(&app.id) {
            return;
        }
        self.memory.departed.remove(&app.bundle_id);
        let remembered = self
            .memory
            .memory
            .get(&app.bundle_id)
            .map(|remembered| {
                remembered
                    .windows
                    .iter()
                    .filter(|placement| self.can_recall_to(placement.workspace))
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if remembered.is_empty() {
            return;
        }
        log_event!(
            "recall: {} launched as {:?}; {} remembered placement(s) available",
            app.bundle_id,
            app.id,
            remembered.len()
        );
        self.memory.recalls.insert(
            app.id.clone(),
            Recall {
                bundle_id: app.bundle_id.clone(),
                opened: now,
                remaining: remembered,
                claimed: HashMap::new(),
            },
        );
    }

    /// Whether a new instance presenting `presented` windows at once is macOS restoring a
    /// multi-workspace arrangement: it opened two or more windows together and its memory spans
    /// two or more workspaces. A cold launch opens one window, which stays on the active
    /// workspace.
    fn restores_arrangement(&self, bundle_id: &str, presented: usize) -> bool {
        presented >= 2
            && self.memory.memory.get(bundle_id).is_some_and(|remembered| {
                let mut workspaces = remembered
                    .windows
                    .iter()
                    .map(|placement| placement.workspace);
                workspaces
                    .next()
                    .is_some_and(|first| workspaces.any(|workspace| workspace != first))
            })
    }

    fn can_recall_to(&self, workspace: WorkspaceId) -> bool {
        self.workspaces.is_enabled(workspace)
            && self
                .scope
                .manages_selector(self.workspaces.assignment(workspace))
    }

    /// Fold settled ownership back into the memory once it has held for
    /// [`MEMORY_SETTLE`], then persist any change.
    pub(super) fn record_window_memory(
        &mut self,
        server_frames: &HashMap<u32, crate::Rect>,
        observation: &crate::NativeObservation,
    ) {
        for app in &observation.applications {
            let crate::AppObservation::Complete { windows, .. } = app else {
                continue;
            };
            for window in windows {
                self.memory.identities.insert(
                    window.id.clone(),
                    WindowIdentity {
                        document: window.document.as_deref().and_then(normalize_document),
                        title: window.title.as_deref().and_then(normalize_title),
                    },
                );
            }
        }
        let recalling: HashSet<_> = self
            .memory
            .recalls
            .values()
            .map(|recall| recall.bundle_id.clone())
            .collect();
        let mut live_windows: HashSet<crate::PhysicalWindowId> =
            self.parked.keys().cloned().collect();
        let mut current: BTreeMap<String, Vec<(u32, RememberedWindow)>> = BTreeMap::new();
        let identities = &self.memory.identities;
        let bundles = &self.memory.bundles;
        let parked = &self.parked;
        let mut add = |window: &crate::PhysicalWindowId, workspace, floating| {
            live_windows.insert(window.clone());
            let Some(bundle_id) = bundles.get(&window.app).filter(|bundle_id| {
                is_rememberable_bundle_id(bundle_id) && !recalling.contains(*bundle_id)
            }) else {
                return;
            };
            let Some(frame) = parked
                .get(window)
                .or_else(|| server_frames.get(&window.window_id))
                .copied()
            else {
                return;
            };
            let identity = identities.get(window);
            current.entry(bundle_id.clone()).or_default().push((
                window.window_id,
                RememberedWindow {
                    workspace,
                    frame,
                    floating,
                    document: identity.and_then(|entry| entry.document.clone()),
                    title: identity.and_then(|entry| entry.title.clone()),
                },
            ));
        };
        for workspace in WorkspaceId::all() {
            for window in self.workspaces.engine(workspace).bound_windows() {
                add(&window, workspace, false);
            }
        }
        for (window, workspace) in self.workspaces.floating_entries() {
            add(window, workspace, true);
        }
        self.memory
            .identities
            .retain(|window, _| live_windows.contains(window));
        let now = self.backend.now();
        self.memory
            .pending
            .retain(|bundle_id, _| current.contains_key(bundle_id));
        let mut recorded = Vec::new();
        for (bundle_id, mut windows) in current {
            windows.sort_by_key(|(window_id, _)| *window_id);
            let windows: Vec<_> = windows
                .into_iter()
                .map(|(_, placement)| placement)
                .take(crate::window_memory::MAX_WINDOWS_PER_APP)
                .collect();
            if self
                .memory
                .memory
                .get(&bundle_id)
                .is_some_and(|remembered| same_window_list(&remembered.windows, &windows))
            {
                self.memory.pending.remove(&bundle_id);
                continue;
            }
            match self.memory.pending.get(&bundle_id) {
                Some((pending, since)) if same_window_list(pending, &windows) => {
                    if now.duration_since(*since) >= MEMORY_SETTLE {
                        self.memory.pending.remove(&bundle_id);
                        let count = windows.len();
                        if self.memory.memory.record(
                            &bundle_id,
                            windows,
                            crate::journal::unix_millis() / 1_000,
                        ) {
                            recorded.push((bundle_id, count));
                        }
                    }
                }
                _ => {
                    self.memory.pending.insert(bundle_id, (windows, now));
                }
            }
        }
        if recorded.is_empty() {
            return;
        }
        for (bundle_id, windows) in recorded {
            self.journal_event(crate::journal::JournalEvent::Remembered { bundle_id, windows });
        }
        // Advisory state: a failed write is not retried until the memory
        // changes again, since retrying every cycle would only repeat the error.
        if let Err(error) = self.backend.persist_window_memory(&self.memory.memory) {
            log_event!("failed to persist window memory: {error}");
        }
    }
}

/// Bare executables without a macOS bundle identifier all observe as `"unknown"`.
/// Pooling them under one placement-memory key would make unrelated programs recall
/// each other's workspaces as relaunches.
fn is_rememberable_bundle_id(bundle_id: &str) -> bool {
    !bundle_id.is_empty() && bundle_id != "unknown"
}
