use std::collections::BTreeMap;

use crate::journal::{
    Anomaly, AnomalyKind, AnomalyTracker, ArrangedWindow, Arrangement, JournalEvent, JournalLine,
    WindowRole,
};

use super::*;

/// Arrangement checks run at most this often; each costs one window-server
/// list copy and reuses the latest cycle's Accessibility observation.
pub(super) const JOURNAL_CHECK_INTERVAL: Duration = Duration::from_secs(1);
/// A full snapshot is rewritten this often so diffs never need a long replay.
const JOURNAL_SNAPSHOT_INTERVAL: Duration = Duration::from_secs(30 * 60);
/// Tiles may sit this far from their target (title bars, rounding, apps that
/// size in character cells) before they count as misplaced.
const MISPLACED_TOLERANCE: i32 = 16;

#[derive(Default)]
pub(super) struct JournalState {
    last_written: Option<Arrangement>,
    last_snapshot: Option<Instant>,
    pub(super) last_check: Option<Instant>,
    pub(super) observation: Option<crate::NativeObservation>,
    tracker: AnomalyTracker,
}

impl<B: WorkspaceBackend> WorkspaceRuntime<B> {
    pub(super) fn journal_event(&mut self, event: JournalEvent) {
        self.backend.journal(&JournalLine {
            t: crate::journal::unix_millis(),
            event,
        });
    }

    pub(super) fn journal_start(&mut self, restored_state: bool) {
        let slots = WorkspaceId::all()
            .map(|workspace| {
                self.workspaces
                    .engine(workspace)
                    .model()
                    .workspace
                    .slots
                    .len()
            })
            .sum::<usize>();
        log_event!(
            "loaded state: {}, {slots} saved slot(s), {} parked window(s), memory for {} app(s)",
            if restored_state {
                "resumed workspaces"
            } else {
                "no saved workspaces"
            },
            self.parked.len(),
            self.memory.memory.len()
        );
        self.journal_event(JournalEvent::Start {
            version: crate::build_version().into(),
            pid: std::process::id(),
            restored_state,
            slots,
            parked: self.parked.len(),
            remembered_apps: self.memory.memory.len(),
        });
    }

    pub(super) fn journal_restore(&mut self, reason: &str, requested: usize, failed: usize) {
        log_event!("restore ({reason}): {requested} parked window(s), {failed} failed");
        self.journal_event(JournalEvent::Restore {
            reason: reason.into(),
            requested,
            failed,
        });
    }

    /// The user noticed something wrong. Mark the moment and force a full
    /// snapshot on the next arrangement check, which follows every action.
    pub(super) fn mark_wrong(&mut self) {
        log_event!("marked arrangement wrong");
        self.journal_event(JournalEvent::MarkedWrong);
        self.journal.last_check = None;
        self.journal.last_snapshot = None;
    }

    /// Compare the model with the window server and journal what changed.
    pub(super) fn check_arrangement(&mut self, outputs: &macos::DisplayOutputs) {
        if self.journal.last_check.is_some_and(|checked| {
            self.backend.now().saturating_duration_since(checked) < JOURNAL_CHECK_INTERVAL
        }) {
            return;
        }
        let Some(server_frames) = self.backend.window_server_frames() else {
            return;
        };
        let now = self.backend.now();
        self.journal.last_check = Some(now);
        let visible = self.visible_workspaces(outputs);
        let windows = self.arranged_windows(&server_frames);
        let anomalies = self.anomalies(outputs, &visible, &windows, &server_frames);
        let arrangement = Arrangement {
            displays: outputs.frames().collect(),
            active: visible.iter().map(|resolved| resolved.workspace).collect(),
            windows: windows.into_iter().map(|(_, window)| window).collect(),
        };
        let snapshot_due = self.journal.last_snapshot.is_none_or(|written| {
            now.saturating_duration_since(written) >= JOURNAL_SNAPSHOT_INTERVAL
        });
        let event = match &self.journal.last_written {
            Some(previous) if !snapshot_due => crate::journal::diff(previous, &arrangement),
            _ => {
                self.journal.last_snapshot = Some(now);
                Some(JournalEvent::Snapshot {
                    arrangement: arrangement.clone(),
                })
            }
        };
        if let Some(event) = event {
            self.journal_event(event);
            self.journal.last_written = Some(arrangement);
        }
        for event in self.journal.tracker.update(now, anomalies) {
            self.journal_event(event);
        }
    }

    pub(super) fn arranged_windows(
        &self,
        server_frames: &HashMap<u32, crate::Rect>,
    ) -> Vec<(crate::PhysicalWindowId, ArrangedWindow)> {
        let mut windows = BTreeMap::new();
        let mut push = |id: &crate::PhysicalWindowId, owner, role| {
            windows.entry(id.window_id).or_insert_with(|| {
                (
                    id.clone(),
                    ArrangedWindow {
                        window: id.window_id,
                        pid: id.app.pid,
                        bundle_id: self.bundle_id(&id.app).unwrap_or_default().to_owned(),
                        frame: server_frames.get(&id.window_id).copied(),
                        owner,
                        role,
                        parked: self.parked.contains_key(id),
                    },
                )
            });
        };
        for workspace in WorkspaceId::all() {
            for window in self.workspaces.engine(workspace).bound_windows() {
                push(&window, Some(workspace), WindowRole::Tiled);
            }
        }
        for (window, workspace) in self.workspaces.floating_entries() {
            push(window, Some(workspace), WindowRole::Floating);
        }
        if let Some(observation) = &self.journal.observation {
            for application in &observation.applications {
                let crate::AppObservation::Complete { app, windows, .. } = application else {
                    continue;
                };
                for window in windows {
                    if !server_frames.contains_key(&window.id.window_id) {
                        continue;
                    }
                    let role = if self.config.patch(&app.bundle_id, window).is_ignored() {
                        WindowRole::Ignored
                    } else {
                        WindowRole::Unowned
                    };
                    push(&window.id, None, role);
                }
            }
        }
        windows.into_values().collect()
    }

    pub(super) fn anomalies(
        &self,
        outputs: &macos::DisplayOutputs,
        visible_workspaces: &[ResolvedWorkspace],
        windows: &[(crate::PhysicalWindowId, ArrangedWindow)],
        server_frames: &HashMap<u32, crate::Rect>,
    ) -> Vec<Anomaly> {
        let displays: Vec<_> = outputs.frames().collect();
        let on_display = |frame: crate::Rect| crate::geometry::shows_on(frame, &displays);
        let visible: HashSet<_> = visible_workspaces
            .iter()
            .map(|resolved| resolved.workspace)
            .collect();
        let mut anomalies = Vec::new();
        let mut report = |kind, arranged: &ArrangedWindow, expected| {
            anomalies.push(Anomaly {
                kind,
                window: arranged.window,
                bundle_id: arranged.bundle_id.clone(),
                workspace: arranged.owner,
                frame: arranged.frame,
                expected,
            });
        };
        for (id, arranged) in windows {
            let exempt = self.parked.is_exempt(id);
            if exempt {
                report(AnomalyKind::ParkingExempt, arranged, None);
            }
            let Some(frame) = arranged.frame else {
                if arranged.owner.is_some() {
                    report(AnomalyKind::Missing, arranged, None);
                }
                continue;
            };
            let shown = on_display(frame);
            match arranged.owner {
                Some(owner) if !visible.contains(&owner) => {
                    if shown && !arranged.parked && !exempt {
                        report(AnomalyKind::Leaked, arranged, None);
                    }
                }
                Some(_) => {
                    if !shown && !arranged.parked {
                        report(AnomalyKind::Stranded, arranged, None);
                    }
                }
                None => {
                    if shown && arranged.role == WindowRole::Unowned {
                        report(AnomalyKind::Unadopted, arranged, None);
                    }
                }
            }
            if arranged.parked && shown && !exempt {
                report(AnomalyKind::ParkedVisible, arranged, None);
            }
        }
        for tile in self.navigation_tiles_for(visible_workspaces) {
            if self.parked.contains_key(&tile.window) {
                continue;
            }
            let Some(frame) = server_frames.get(&tile.window.window_id).copied() else {
                continue;
            };
            if self.constrained[tile.workspace.index()].contains_key(&tile.window) {
                if !self.workspaces.deck(tile.workspace).is_monocle()
                    && let Some((_, arranged)) = windows.iter().find(|(id, _)| id == &tile.window)
                    && windows.iter().any(|(other, entry)| {
                        other != &tile.window
                            && entry.owner == Some(tile.workspace)
                            && entry.role == WindowRole::Tiled
                            && !entry.parked
                            && entry.frame.is_some_and(|rect| overlaps(frame, rect))
                    })
                {
                    report(AnomalyKind::ConstrainedOverlap, arranged, Some(tile.frame));
                }
                continue;
            }
            if !frame.approximately_equals(tile.frame, MISPLACED_TOLERANCE)
                && let Some((_, arranged)) = windows.iter().find(|(id, _)| id == &tile.window)
            {
                report(AnomalyKind::Misplaced, arranged, Some(tile.frame));
            }
        }
        anomalies
    }
}

/// Ignore tiny edge differences from rounding and native readback.
fn overlaps(a: crate::Rect, b: crate::Rect) -> bool {
    let span = |a: i32, length: i32, b: i32, other: i32| {
        (i64::from(a) + i64::from(length)).min(i64::from(b) + i64::from(other))
            - i64::from(a.max(b))
    };
    span(a.x, a.width, b.x, b.width) > 2 && span(a.y, a.height, b.y, b.height) > 2
}
