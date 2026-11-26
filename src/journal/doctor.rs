use std::collections::{BTreeMap, HashMap};

use crate::{Rect, WorkspaceId};

use super::{
    ANOMALY_SETTLE, AnomalyKind, AnomalyState, ArrangedWindow, Arrangement, CORRECTION_WINDOW,
    JournalEvent, JournalLine, PlacementSource, WindowChange, WindowRole,
};

/// One anomaly kind's begin count, time spent, and the apps it touched.
#[derive(Debug, Default)]
pub struct KindSummary {
    pub count: usize,
    pub total_ms: u64,
    pub longest_ms: u64,
    pub open: usize,
    pub bundles: BTreeMap<String, usize>,
}

/// Counts over the journal lines in one period, shared by `fractile doctor` and the Activity
/// pane.
#[derive(Debug, Default)]
pub struct Summary<'a> {
    /// Every line in the period, oldest first.
    pub lines: Vec<&'a JournalLine>,
    /// `Start` lines.
    pub starts: Vec<&'a JournalLine>,
    pub stops: usize,
    /// `Restore` lines.
    pub restores: Vec<&'a JournalLine>,
    pub restore_failures: usize,
    pub recalls: BTreeMap<&'a str, usize>,
    pub unclaimed: BTreeMap<&'a str, usize>,
    pub wrong: BTreeMap<PlacementSource, BTreeMap<&'a str, usize>>,
    /// When the arrangement was marked wrong (a snapshot was saved by hand).
    pub marked: Vec<u64>,
    pub anomalies: BTreeMap<AnomalyKind, KindSummary>,
    /// The arrangement rebuilt from the last snapshot and the diffs after it.
    pub arrangement: Option<Arrangement>,
}

/// The settle window in milliseconds, the least a journaled anomaly has already persisted.
fn settle_ms() -> u64 {
    u64::try_from(ANOMALY_SETTLE.as_millis()).unwrap_or(u64::MAX)
}

impl<'a> Summary<'a> {
    #[must_use]
    pub fn new(lines: &'a [JournalLine], since_ms: u64, now_ms: u64) -> Self {
        let mut summary = Self::default();
        let mut open: HashMap<(AnomalyKind, u32), u64> = HashMap::new();
        for line in lines {
            if line.t < since_ms {
                apply_arrangement(&mut summary.arrangement, &line.event);
                continue;
            }
            summary.lines.push(line);
            match &line.event {
                JournalEvent::Start { .. } => {
                    open.clear();
                    summary.starts.push(line);
                }
                JournalEvent::Stop => {
                    open.clear();
                    summary.stops += 1;
                }
                JournalEvent::Restore { failed, .. } => {
                    summary.restores.push(line);
                    summary.restore_failures += failed;
                }
                JournalEvent::Recall { bundle_id, .. } => {
                    *summary.recalls.entry(bundle_id).or_default() += 1;
                }
                JournalEvent::WrongPlacement {
                    bundle_id, source, ..
                } => {
                    *summary
                        .wrong
                        .entry(*source)
                        .or_default()
                        .entry(bundle_id)
                        .or_default() += 1;
                }
                JournalEvent::RecallExpired {
                    bundle_id,
                    unclaimed,
                } => *summary.unclaimed.entry(bundle_id).or_default() += unclaimed,
                JournalEvent::Anomaly {
                    state,
                    anomaly,
                    duration_ms,
                } => {
                    let key = (anomaly.kind, anomaly.window);
                    match state {
                        AnomalyState::Begin => {
                            let kind = summary.anomalies.entry(anomaly.kind).or_default();
                            kind.count += 1;
                            *kind.bundles.entry(anomaly.bundle_id.clone()).or_default() += 1;
                            // A begin is journaled only after the settle window, and records how
                            // long the disagreement had already persisted. Older journals wrote
                            // no value: assume the minimum settle window.
                            let settled = duration_ms.unwrap_or_else(settle_ms);
                            open.insert(key, line.t.saturating_sub(settled));
                        }
                        AnomalyState::End => {
                            if open.remove(&key).is_some() {
                                let kind = summary.anomalies.entry(anomaly.kind).or_default();
                                let duration = duration_ms.unwrap_or_default();
                                kind.total_ms += duration;
                                kind.longest_ms = kind.longest_ms.max(duration);
                            }
                        }
                    }
                }
                JournalEvent::Snapshot { .. } | JournalEvent::Diff { .. } => {
                    apply_arrangement(&mut summary.arrangement, &line.event);
                }
                JournalEvent::MarkedWrong => summary.marked.push(line.t),
                JournalEvent::Remembered { .. } => {}
            }
        }
        for ((kind, _), began) in &open {
            let kind = summary.anomalies.entry(*kind).or_default();
            kind.open += 1;
            kind.longest_ms = kind.longest_ms.max(now_ms.saturating_sub(*began));
        }
        summary
    }

    #[must_use]
    pub fn recalled(&self) -> usize {
        self.recalls.values().sum()
    }

    #[must_use]
    pub fn wrong_placements(&self) -> usize {
        self.wrong.values().flat_map(BTreeMap::values).sum()
    }

    /// Anomalies that began in the period.
    #[must_use]
    pub fn anomaly_count(&self) -> usize {
        self.anomalies.values().map(|kind| kind.count).sum()
    }

    /// Anomalies that have not ended yet.
    #[must_use]
    pub fn open_anomalies(&self) -> usize {
        self.anomalies.values().map(|kind| kind.open).sum()
    }

    /// The kind that began most often; ties go to the earlier kind.
    #[must_use]
    pub fn top_anomaly(&self) -> Option<(AnomalyKind, usize)> {
        self.anomalies
            .iter()
            .filter(|(_, kind)| kind.count > 0)
            .map(|(kind, summary)| (*kind, summary.count))
            .max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.cmp(&a.0)))
    }
}

/// Human-readable summary of journal lines at or after `since_ms`.
#[must_use]
pub fn summarize(lines: &[JournalLine], since_ms: u64, now_ms: u64, skipped: usize) -> String {
    let summary = Summary::new(lines, since_ms, now_ms);
    let (Some(first), Some(last)) = (summary.lines.first(), summary.lines.last()) else {
        let mut out = "No journal events in this period.".to_owned();
        if skipped > 0 {
            out.push_str(&format!(" ({skipped} unreadable lines skipped)"));
        }
        if let Some(arrangement) = &summary.arrangement {
            out.push('\n');
            append_arrangement(&mut out, arrangement);
        }
        return out.trim_end().to_owned();
    };
    let mut out = format!(
        "{} events from {} to {}",
        summary.lines.len(),
        human_age(now_ms, first.t),
        human_age(now_ms, last.t)
    );
    if skipped > 0 {
        out.push_str(&format!(" ({skipped} unreadable lines skipped)"));
    }
    out.push('\n');

    out.push_str(&format!(
        "\nAgent starts: {}, clean stops: {}\n",
        summary.starts.len(),
        summary.stops
    ));
    for line in summary.starts.iter().rev().take(5) {
        if let JournalEvent::Start {
            version,
            restored_state,
            slots,
            parked,
            remembered_apps,
            ..
        } = &line.event
        {
            out.push_str(&format!(
                "  {}: v{version}, {}, {slots} saved slots, {parked} parked, memory for {remembered_apps} apps\n",
                human_age(now_ms, line.t),
                if *restored_state { "resumed saved state" } else { "no saved state" },
            ));
        }
    }
    if !summary.restores.is_empty() {
        out.push_str("Window restorations:\n");
        for line in summary.restores.iter().rev().take(5) {
            if let JournalEvent::Restore {
                reason,
                requested,
                failed,
            } = &line.event
            {
                out.push_str(&format!(
                    "  {}: {reason}, {requested} parked, {failed} failed\n",
                    human_age(now_ms, line.t)
                ));
            }
        }
    }
    if !summary.recalls.is_empty() || !summary.unclaimed.is_empty() {
        out.push_str(&format!(
            "Remembered placements: {} windows recalled",
            summary.recalled()
        ));
        let unclaimed = summary.unclaimed.values().sum::<usize>();
        if unclaimed > 0 {
            out.push_str(&format!(", {unclaimed} left unclaimed"));
        }
        out.push('\n');
        for (bundle, count) in &summary.recalls {
            out.push_str(&format!("  {bundle}: {count} recalled\n"));
        }
        for (bundle, count) in &summary.unclaimed {
            out.push_str(&format!("  {bundle}: {count} unclaimed\n"));
        }
    }

    out.push_str(&format!(
        "\nWrong placements (moved by hand within {}s of being placed): {}\n",
        CORRECTION_WINDOW.as_secs(),
        summary.wrong_placements()
    ));
    for (source, bundles) in &summary.wrong {
        out.push_str(&format!(
            "  {:>3}x {}: {}\n",
            bundles.values().sum::<usize>(),
            source.label(),
            top_bundles(bundles.iter().map(|(bundle, count)| (*bundle, *count)))
        ));
    }

    if !summary.marked.is_empty() {
        out.push_str(&format!(
            "Marked wrong by hand (fractile ctl wrong): {}\n",
            summary
                .marked
                .iter()
                .rev()
                .take(5)
                .map(|t| human_age(now_ms, *t))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    out.push_str("\nAnomalies (lasting at least 3s):\n");
    if summary.anomalies.is_empty() {
        out.push_str("  none\n");
    }
    for (kind, totals) in &summary.anomalies {
        out.push_str(&format!(
            "  {:<15} {:>3}x  total {:>7}  longest {:>7}{}  ({})\n",
            kind.label(),
            totals.count,
            human_duration(totals.total_ms),
            human_duration(totals.longest_ms),
            if totals.open > 0 {
                format!(", {} still open", totals.open)
            } else {
                String::new()
            },
            kind.meaning()
        ));
        out.push_str(&format!(
            "                  {}\n",
            top_bundles(
                totals
                    .bundles
                    .iter()
                    .map(|(bundle, count)| (bundle.as_str(), *count))
            )
        ));
    }

    if let Some(arrangement) = &summary.arrangement {
        append_arrangement(&mut out, arrangement);
    }
    out.trim_end().to_owned()
}

fn append_arrangement(out: &mut String, arrangement: &Arrangement) {
    out.push_str(&format!(
        "\nLast known arrangement: active {}\n",
        arrangement
            .active
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    ));
    let mut workspaces: BTreeMap<WorkspaceId, Vec<&ArrangedWindow>> = BTreeMap::new();
    let mut loose = Vec::new();
    for window in &arrangement.windows {
        match window.owner {
            Some(owner) => workspaces.entry(owner).or_default().push(window),
            None if window.role == WindowRole::Unowned => loose.push(window),
            None => {}
        }
    }
    for (workspace, windows) in workspaces {
        out.push_str(&format!("  {workspace}: {}\n", describe(&windows)));
    }
    if !loose.is_empty() {
        out.push_str(&format!("  unowned: {}\n", describe(&loose)));
    }
}

/// The four most frequent bundles, most frequent first: `a (3), b (1)`.
fn top_bundles<'a>(bundles: impl Iterator<Item = (&'a str, usize)>) -> String {
    let mut bundles: Vec<_> = bundles.collect();
    bundles.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    bundles
        .iter()
        .take(4)
        .map(|(bundle, count)| format!("{bundle} ({count})"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn apply_arrangement(arrangement: &mut Option<Arrangement>, event: &JournalEvent) {
    match event {
        JournalEvent::Snapshot {
            arrangement: snapshot,
        } => *arrangement = Some(snapshot.clone()),
        JournalEvent::Diff {
            active,
            displays,
            changes,
        } => {
            if let Some(current) = arrangement.as_mut() {
                apply_diff(current, active.as_ref(), displays.as_ref(), changes);
            }
        }
        _ => {}
    }
}

fn apply_diff(
    arrangement: &mut Arrangement,
    active: Option<&Vec<WorkspaceId>>,
    displays: Option<&Vec<Rect>>,
    changes: &[WindowChange],
) {
    if let Some(active) = active {
        arrangement.active.clone_from(active);
    }
    if let Some(displays) = displays {
        arrangement.displays.clone_from(displays);
    }
    for change in changes {
        arrangement
            .windows
            .retain(|window| window.window != change.window);
        if let Some(after) = &change.after {
            arrangement.windows.push(after.clone());
        }
    }
    arrangement.windows.sort_by_key(|window| window.window);
}

fn describe(windows: &[&ArrangedWindow]) -> String {
    let mut bundles: BTreeMap<&str, usize> = BTreeMap::new();
    for window in windows {
        *bundles
            .entry(short_bundle_name(&window.bundle_id))
            .or_default() += 1;
    }
    bundles
        .into_iter()
        .map(|(name, count)| {
            if count == 1 {
                name.to_owned()
            } else {
                format!("{name} x{count}")
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// The last segment of a bundle ID: `com.apple.Safari` → `Safari`.
#[must_use]
pub fn short_bundle_name(bundle_id: &str) -> &str {
    bundle_id
        .rsplit('.')
        .next()
        .filter(|segment| !segment.is_empty())
        .unwrap_or(bundle_id)
}

/// How long ago `at_ms` was: `just now`, `45s ago`, `3m ago`.
#[must_use]
pub fn human_age(now_ms: u64, at_ms: u64) -> String {
    let elapsed = now_ms.saturating_sub(at_ms);
    if elapsed < 2_000 {
        "just now".into()
    } else {
        format!("{} ago", human_duration(elapsed))
    }
}

/// A coarse duration: `45s`, `3m`, `2h 5m`, `4d 3h`.
#[must_use]
pub fn human_duration(ms: u64) -> String {
    let secs = ms / 1_000;
    match secs {
        0..60 => format!("{secs}s"),
        60..3_600 => format!("{}m", secs / 60),
        3_600..86_400 => format!("{}h {}m", secs / 3_600, secs % 3_600 / 60),
        _ => format!("{}d {}h", secs / 86_400, secs % 86_400 / 3_600),
    }
}
