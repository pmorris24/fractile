//! Activity pane: the last 24 hours of the arrangement journal (`journal.jsonl`), summarized
//! with the same [`Summary`] as `fractile doctor`, plus `procwatch` process vitals.

#[cfg(test)]
mod tests;

use std::time::SystemTime;

use fractile::journal::{
    AnomalyKind, AnomalyState, JournalEvent, JournalLine, Summary, human_age, human_duration,
    short_bundle_name,
};
use rune::{El, Tx, row, text};

use crate::{
    Pane, Ui,
    controls::{self, ButtonKind, Paint},
    playground::{Scene, reset_fixture},
    view::scroll_body,
};

pub(crate) const SCENES: &[Scene] = &[("activity", |ui| {
    reset_fixture(ui, Pane::Activity);
    ui.activity = ActivityPane::fixture_healthy();
})];

const WINDOW_24H_MS: u64 = 24 * 3_600 * 1_000;
const MAX_ROWS: usize = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Tone {
    Normal,
    Success,
    Warning,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActivityRow {
    pub title: String,
    pub detail: String,
    pub age: String,
    pub tone: Tone,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DoctorReport {
    pub total_events: usize,
    pub skipped_lines: usize,
    pub starts: usize,
    pub restores: usize,
    pub restore_failures: usize,
    pub recalled_windows: usize,
    pub wrong_placements: usize,
    pub anomalies: usize,
    pub open_anomalies: usize,
    pub top_anomaly: Option<(AnomalyKind, usize)>,
    /// How long ago the last snapshot was saved by hand.
    pub last_snapshot: Option<String>,
    pub rows: Vec<ActivityRow>,
}

impl DoctorReport {
    #[must_use]
    pub fn from_lines(lines: &[JournalLine], now_ms: u64, skipped_lines: usize) -> Self {
        let summary = Summary::new(lines, now_ms.saturating_sub(WINDOW_24H_MS), now_ms);
        let rows = summary
            .lines
            .iter()
            .rev()
            .filter_map(|line| activity_row(line, now_ms))
            .take(MAX_ROWS)
            .collect();
        Self {
            total_events: summary.lines.len(),
            skipped_lines,
            starts: summary.starts.len(),
            restores: summary.restores.len(),
            restore_failures: summary.restore_failures,
            recalled_windows: summary.recalled(),
            wrong_placements: summary.wrong_placements(),
            anomalies: summary.anomaly_count(),
            open_anomalies: summary.open_anomalies(),
            top_anomaly: summary.top_anomaly(),
            last_snapshot: summary.marked.last().map(|t| human_age(now_ms, *t)),
            rows,
        }
    }
}

/// The Recent activity row for `line`, or `None` for bookkeeping (snapshots, diffs, memory).
fn activity_row(line: &JournalLine, now_ms: u64) -> Option<ActivityRow> {
    let row = |title: String, detail: String, tone| ActivityRow {
        title,
        detail,
        age: human_age(now_ms, line.t),
        tone,
    };
    Some(match &line.event {
        JournalEvent::Start {
            version,
            restored_state,
            slots,
            parked,
            remembered_apps,
            ..
        } => {
            let state = if *restored_state {
                "Resumed saved workspaces"
            } else {
                "Fresh workspace state"
            };
            row(
                format!("Agent started (v{version})"),
                format!(
                    "{state} · {slots} slots, {parked} parked, {remembered_apps} apps in memory"
                ),
                Tone::Normal,
            )
        }
        JournalEvent::Stop => row(
            "Agent stopped cleanly".into(),
            "Parked windows restored before exit.".into(),
            Tone::Success,
        ),
        JournalEvent::Restore {
            reason,
            requested,
            failed: 0,
        } => row(
            format!("Restored {requested} parked window{}", plural(*requested)),
            human_restore_reason(reason),
            Tone::Success,
        ),
        JournalEvent::Restore { reason, failed, .. } => row(
            format!("Window restore had {failed} failure{}", plural(*failed)),
            human_restore_reason(reason),
            Tone::Warning,
        ),
        JournalEvent::Recall {
            bundle_id,
            workspace,
            floating,
            ..
        } => {
            let mode = if *floating { "floating" } else { "tiled" };
            row(
                format!(
                    "Recalled {} to Workspace {workspace}",
                    short_bundle_name(bundle_id)
                ),
                format!("Matched remembered {mode} placement ({bundle_id})"),
                Tone::Normal,
            )
        }
        JournalEvent::WrongPlacement {
            bundle_id,
            source,
            placed,
            moved_to,
            ..
        } => row(
            format!(
                "Hand-moved {} from {placed} to {moved_to}",
                short_bundle_name(bundle_id)
            ),
            format!("Originally {}", source.label()),
            Tone::Warning,
        ),
        JournalEvent::Anomaly {
            state: AnomalyState::Begin,
            anomaly,
            ..
        } => {
            let workspace = anomaly
                .workspace
                .map_or_else(String::new, |ws| format!(" on Workspace {ws}"));
            row(
                format!(
                    "{}: {}{workspace}",
                    anomaly.kind.display_label(),
                    short_bundle_name(&anomaly.bundle_id)
                ),
                capitalized(anomaly.kind.meaning()),
                Tone::Warning,
            )
        }
        JournalEvent::Anomaly {
            state: AnomalyState::End,
            anomaly,
            duration_ms,
        } => row(
            format!(
                "Resolved {}: {}",
                anomaly.kind.display_label().to_lowercase(),
                short_bundle_name(&anomaly.bundle_id)
            ),
            format!(
                "Settled after {}",
                human_duration(duration_ms.unwrap_or_default())
            ),
            Tone::Success,
        ),
        JournalEvent::MarkedWrong => row(
            "Snapshot saved".into(),
            "Recorded the full arrangement for fractile doctor.".into(),
            Tone::Normal,
        ),
        JournalEvent::Diff { .. }
        | JournalEvent::Snapshot { .. }
        | JournalEvent::RecallExpired { .. }
        | JournalEvent::Remembered { .. } => return None,
    })
}

#[derive(Clone, Default)]
pub struct ActivityPane {
    stamp: Option<(Option<SystemTime>, u64)>,
    loaded: bool,
    pub error: Option<String>,
    pub report: DoctorReport,
    fixture_vitals: Option<String>,
}

impl ActivityPane {
    /// Reloads the journal when the pane is visible and the file changed since the last read.
    pub fn poll(&mut self, visible: bool, fixture: bool) -> bool {
        if !visible || fixture {
            return false;
        }
        if self.loaded && self.stamp == journal_stamp() {
            return false;
        }
        self.reload();
        true
    }

    fn reload(&mut self) {
        self.loaded = true;
        self.stamp = journal_stamp();
        let Some((live, rotated)) = fractile::journal::default_paths() else {
            self.error = Some("HOME is unavailable".into());
            return;
        };
        match fractile::journal::read(&[&rotated, &live]) {
            Ok((lines, skipped)) => {
                self.error = None;
                let now_ms = fractile::journal::unix_millis();
                self.report = DoctorReport::from_lines(&lines, now_ms, skipped);
            }
            Err(error) => {
                self.error = Some(error);
            }
        }
    }

    #[must_use]
    fn fixture_healthy() -> Self {
        let now_ms = 1_700_000_000_000;
        let at = |secs_ago: u64| now_ms - secs_ago * 1_000;
        let lines = vec![
            JournalLine {
                t: at(3_400),
                event: JournalEvent::Start {
                    version: fractile::build_version().into(),
                    pid: 4120,
                    restored_state: true,
                    slots: 6,
                    parked: 3,
                    remembered_apps: 9,
                },
            },
            JournalLine {
                t: at(3_380),
                event: JournalEvent::Recall {
                    bundle_id: "com.apple.Safari".into(),
                    window: 101,
                    workspace: fractile::WorkspaceId::ONE,
                    floating: false,
                },
            },
            JournalLine {
                t: at(3_378),
                event: JournalEvent::Recall {
                    bundle_id: "com.apple.MobileSMS".into(),
                    window: 108,
                    workspace: fractile::WorkspaceId::TWO,
                    floating: false,
                },
            },
            JournalLine {
                t: at(420),
                event: JournalEvent::Restore {
                    reason: "pause".into(),
                    requested: 3,
                    failed: 0,
                },
            },
        ];
        Self {
            stamp: None,
            loaded: true,
            error: None,
            report: DoctorReport::from_lines(&lines, now_ms, 0),
            fixture_vitals: Some("28 MB · 34 fds".into()),
        }
    }
}

pub fn pane(ui: &Ui, p: Paint) -> El {
    let report = &ui.activity.report;
    let vitals = ui
        .activity
        .fixture_vitals
        .clone()
        .unwrap_or_else(process_vitals);

    let (runtime_desc, runtime_dot) = match ui.agent.as_ref() {
        Some(agent) if agent.automatic => {
            let active = if agent.active_workspaces.is_empty() {
                "Automatic tiling active".to_owned()
            } else {
                let list = agent
                    .active_workspaces
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(" · ");
                format!("Automatic tiling · Workspaces {list}")
            };
            (active, p.success())
        }
        Some(agent) if !agent.accessibility => (
            "Paused · Accessibility permission required".into(),
            p.warning(),
        ),
        Some(_) => ("Paused · Parked windows restored".into(), p.muted()),
        None => ("Agent disconnected".into(), p.muted()),
    };

    let vitals_badge = row()
        .key("vitals-badge")
        .gap(8.0)
        .center_y()
        .children((!vitals.is_empty()).then(|| {
            text(vitals)
                .key("vitals")
                .font(p.tabular(p.meta()))
                .color(p.faint())
        }))
        .child(controls::status_dot(runtime_dot));

    let (anomaly_desc, anomaly_color, anomaly_dot) = if report.anomalies == 0 {
        ("None".to_owned(), p.muted(), p.success())
    } else {
        let mut parts = vec![format!("{} lasting over 3s", report.anomalies)];
        if let Some((kind, count)) = report.top_anomaly {
            parts.push(format!("mostly {} ({count}×)", kind.display_label()));
        }
        if report.open_anomalies > 0 {
            parts.push(format!("{} still open", report.open_anomalies));
        }
        (parts.join(" · "), p.warning(), p.warning())
    };

    let memory_desc = format!(
        "{} window{} recalled on relaunch · {} hand-moved within 2m",
        report.recalled_windows,
        plural(report.recalled_windows),
        report.wrong_placements
    );
    let mut recovery_desc = format!(
        "{} start{} · {} restore{}",
        report.starts,
        plural(report.starts),
        report.restores,
        plural(report.restores)
    );
    if report.restore_failures > 0 {
        recovery_desc.push_str(&format!(" ({} failed)", report.restore_failures));
    }
    let recovery_dot = if report.restore_failures == 0 {
        p.success()
    } else {
        p.warning()
    };

    let mut sections = vec![controls::section(
        p,
        "health",
        "Health (last 24 hours)",
        vec![
            controls::setting_row(
                p,
                "runtime",
                "Runtime",
                Some((runtime_desc, p.muted())),
                vitals_badge,
            ),
            controls::setting_row(
                p,
                "anomalies",
                "Layout issues",
                Some((anomaly_desc, anomaly_color)),
                controls::status_dot(anomaly_dot),
            ),
            controls::setting_row(
                p,
                "memory",
                "Placement recall",
                Some((memory_desc, p.muted())),
                text(format!("{} events", report.total_events))
                    .key("event-count")
                    .font(p.tabular(p.meta()))
                    .color(p.faint()),
            ),
            controls::setting_row(
                p,
                "recovery",
                "Window recovery",
                Some((recovery_desc, p.muted())),
                controls::status_dot(recovery_dot),
            ),
        ],
    )];

    let journal_rows = if let Some(error) = &ui.activity.error {
        vec![controls::setting_row(
            p,
            "journal-error",
            "Couldn't read arrangement journal",
            Some((error.clone(), p.danger())),
            controls::status_dot(p.danger()),
        )]
    } else if report.rows.is_empty() {
        vec![controls::setting_row(
            p,
            "journal-empty",
            "No arrangement events yet",
            Some((
                "Starts, restores, recalls, and layout issues show up here".into(),
                p.muted(),
            )),
            controls::status_dot(p.k.c.border_strong),
        )]
    } else {
        report
            .rows
            .iter()
            .enumerate()
            .map(|(index, entry)| {
                let detail_color = match entry.tone {
                    Tone::Warning => p.warning(),
                    Tone::Normal | Tone::Success => p.muted(),
                };
                controls::setting_row(
                    p,
                    &format!("entry-{index}"),
                    &entry.title,
                    Some((entry.detail.clone(), detail_color)),
                    text(entry.age.clone())
                        .key("age")
                        .font(p.tabular(p.meta()))
                        .color(p.faint()),
                )
            })
            .collect()
    };
    sections.push(controls::section(
        p,
        "journal",
        "Recent activity",
        journal_rows,
    ));

    let journal_path = fractile::journal::default_paths().map_or_else(
        || "journal.jsonl".into(),
        |(live, _)| controls::home_relative(&live),
    );
    let mut diagnostics = vec![controls::setting_row(
        p,
        "journal-file",
        "Journal",
        Some((journal_path, p.muted())),
        controls::button(
            p,
            "reveal-journal",
            "Show in Finder",
            ButtonKind::Secondary,
            |_: &mut Ui, _: &mut Tx| {
                if let Some((live, _)) = fractile::journal::default_paths() {
                    controls::reveal_in_finder(&live);
                }
            },
        ),
    )];
    if ui.agent.as_ref().is_some_and(|agent| agent.automatic) {
        let snapshot_desc = report.last_snapshot.as_ref().map_or_else(
            || "Records the current arrangement for fractile doctor".to_owned(),
            |age| format!("Last saved {age}"),
        );
        diagnostics.push(controls::setting_row(
            p,
            "snapshot",
            "Snapshot",
            Some((snapshot_desc, p.muted())),
            controls::button(
                p,
                "save-snapshot",
                "Save snapshot",
                ButtonKind::Secondary,
                |ui: &mut Ui, _: &mut Tx| ui.mark_wrong(),
            ),
        ));
    }
    sections.push(controls::section(
        p,
        "diagnostics",
        "Diagnostics",
        diagnostics,
    ));

    scroll_body("activity-body", controls::page("sections", sections))
}

fn journal_stamp() -> Option<(Option<SystemTime>, u64)> {
    let (live, _) = fractile::journal::default_paths()?;
    let metadata = std::fs::metadata(live).ok()?;
    Some((metadata.modified().ok(), metadata.len()))
}

pub(crate) fn process_vitals() -> String {
    let usage = procwatch::usage();
    match (usage.footprint, usage.fds) {
        (Some(footprint), Some(fds)) => format!("{} MB · {fds} fds", footprint >> 20),
        (Some(footprint), None) => format!("{} MB", footprint >> 20),
        _ => String::new(),
    }
}

const fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}

fn human_restore_reason(reason: &str) -> String {
    match reason {
        "pause" => "Paused automatic tiling".into(),
        "restore all" => "Paused or quitting Fractile".into(),
        "startup without saved workspaces" => "Startup recovery without saved workspaces".into(),
        other => other.into(),
    }
}

fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}
