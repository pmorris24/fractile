//! Arrangement journal: a bounded, title-free JSONL record of what Fractile
//! believed about every window compared with what the window server showed.
//!
//! The agent writes a full [`Arrangement`] snapshot at startup and
//! periodically, a [`JournalEvent::Diff`] whenever the arrangement changes,
//! and begin/end events for debounced [`Anomaly`] disagreements between the
//! model and the screen. `fractile doctor` summarizes the result.

use std::{
    collections::{BTreeMap, HashMap},
    io::{BufRead, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};

use crate::{Rect, WorkspaceId};

mod doctor;
pub use doctor::{KindSummary, Summary, human_age, human_duration, short_bundle_name, summarize};

pub const JOURNAL_FILE: &str = "journal.jsonl";
pub const ROTATED_JOURNAL_FILE: &str = "journal.jsonl.1";
/// The live journal rotates past this size; one rotated generation is kept.
pub const MAX_JOURNAL_BYTES: u64 = 4 * 1024 * 1024;
/// A disagreement must persist this long before it is journaled. Workspace
/// switches and window creation briefly produce every anomaly kind.
pub const ANOMALY_SETTLE: Duration = Duration::from_secs(3);
/// Frame changes within this tolerance are not journaled as diffs.
pub const FRAME_DIFF_TOLERANCE: i32 = 4;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct JournalLine {
    /// Unix milliseconds.
    pub t: u64,
    #[serde(flatten)]
    pub event: JournalEvent,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum JournalEvent {
    Start {
        version: String,
        pid: u32,
        restored_state: bool,
        slots: usize,
        parked: usize,
        remembered_apps: usize,
    },
    Snapshot {
        arrangement: Arrangement,
    },
    Diff {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        active: Option<Vec<WorkspaceId>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        displays: Option<Vec<Rect>>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        changes: Vec<WindowChange>,
    },
    Anomaly {
        state: AnomalyState,
        anomaly: Anomaly,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        duration_ms: Option<u64>,
    },
    /// A relaunched application's window claimed a remembered placement.
    Recall {
        bundle_id: String,
        window: u32,
        workspace: WorkspaceId,
        floating: bool,
    },
    /// A relaunch's recall window closed with placements left unclaimed.
    RecallExpired {
        bundle_id: String,
        unclaimed: usize,
    },
    /// An application's remembered placements were updated.
    Remembered {
        bundle_id: String,
        windows: usize,
    },
    Restore {
        reason: String,
        requested: usize,
        failed: usize,
    },
    /// The user moved a window to another workspace by hand within
    /// [`CORRECTION_WINDOW`] of Fractile placing it: the placement was wrong.
    WrongPlacement {
        bundle_id: String,
        window: u32,
        source: PlacementSource,
        placed: WorkspaceId,
        moved_to: WorkspaceId,
        after_ms: u64,
    },
    /// The user ran `fractile ctl wrong` (or Save snapshot) when something looked wrong; a full
    /// snapshot follows.
    MarkedWrong,
    Stop,
}

/// A hand move this soon after Fractile placed a window corrects that placement.
pub const CORRECTION_WINDOW: Duration = Duration::from_secs(120);

/// Why Fractile put a newly seen window on its workspace.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlacementSource {
    /// No rule or memory applied: the workspace showing where it opened.
    Active,
    /// A remembered placement from an earlier instance of its app.
    Recall,
    /// A configured route rule.
    Rule,
}

impl PlacementSource {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Active => "opened on the active workspace",
            Self::Recall => "recalled from memory",
            Self::Rule => "routed by a rule",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AnomalyState {
    Begin,
    End,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct Arrangement {
    pub displays: Vec<Rect>,
    pub active: Vec<WorkspaceId>,
    /// Sorted by window-server ID.
    pub windows: Vec<ArrangedWindow>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowRole {
    Tiled,
    Floating,
    /// Observed, managed by configuration, but owned by no workspace.
    Unowned,
    /// Excluded by an `ignore` rule.
    Ignored,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ArrangedWindow {
    pub window: u32,
    pub pid: i32,
    pub bundle_id: String,
    /// Window-server frame; absent when the window server does not list it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame: Option<Rect>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<WorkspaceId>,
    pub role: WindowRole,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub parked: bool,
}

impl ArrangedWindow {
    fn differs(&self, other: &Self) -> bool {
        self.pid != other.pid
            || self.bundle_id != other.bundle_id
            || self.owner != other.owner
            || self.role != other.role
            || self.parked != other.parked
            || match (self.frame, other.frame) {
                (Some(before), Some(after)) => {
                    !before.approximately_equals(after, FRAME_DIFF_TOLERANCE)
                }
                (None, None) => false,
                _ => true,
            }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct WindowChange {
    pub window: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<ArrangedWindow>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<ArrangedWindow>,
}

/// The diff from `before` to `after`, or `None` when nothing material changed.
#[must_use]
pub fn diff(before: &Arrangement, after: &Arrangement) -> Option<JournalEvent> {
    let old: BTreeMap<_, _> = before.windows.iter().map(|w| (w.window, w)).collect();
    let new: BTreeMap<_, _> = after.windows.iter().map(|w| (w.window, w)).collect();
    let mut changes = Vec::new();
    for (id, previous) in &old {
        match new.get(id) {
            None => changes.push(WindowChange {
                window: *id,
                before: Some((*previous).clone()),
                after: None,
            }),
            Some(current) if previous.differs(current) => changes.push(WindowChange {
                window: *id,
                before: Some((*previous).clone()),
                after: Some((*current).clone()),
            }),
            Some(_) => {}
        }
    }
    for (id, current) in &new {
        if !old.contains_key(id) {
            changes.push(WindowChange {
                window: *id,
                before: None,
                after: Some((*current).clone()),
            });
        }
    }
    changes.sort_by_key(|change| change.window);
    let active = (before.active != after.active).then(|| after.active.clone());
    let displays = (before.displays != after.displays).then(|| after.displays.clone());
    (active.is_some() || displays.is_some() || !changes.is_empty()).then_some(JournalEvent::Diff {
        active,
        displays,
        changes,
    })
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AnomalyKind {
    /// Owned by a hidden workspace, not parked, yet on a display.
    Leaked,
    /// Owned by a visible workspace and not parked, yet off every display.
    Stranded,
    /// Recorded as parked, yet on a display.
    ParkedVisible,
    /// Managed by configuration and on a display, but owned by no workspace.
    Unadopted,
    /// Tiled on a visible workspace but away from its layout target.
    Misplaced,
    /// An app-constrained tile overlaps a sibling on its visible workspace.
    ConstrainedOverlap,
    /// Owned, but the window server no longer lists it.
    Missing,
    /// Left visible across switches because it repeatedly rejects parking.
    ParkingExempt,
}

impl AnomalyKind {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Leaked => "leaked",
            Self::Stranded => "stranded",
            Self::ParkedVisible => "parked_visible",
            Self::Unadopted => "unadopted",
            Self::ConstrainedOverlap => "constrained_overlap",
            Self::Misplaced => "misplaced",
            Self::Missing => "missing",
            Self::ParkingExempt => "parking_exempt",
        }
    }

    /// The label shown in Settings: `parked_visible` → `Parked but visible`.
    #[must_use]
    pub const fn display_label(self) -> &'static str {
        match self {
            Self::Leaked => "Leaked",
            Self::Stranded => "Stranded",
            Self::ParkedVisible => "Parked but visible",
            Self::Unadopted => "Unadopted",
            Self::ConstrainedOverlap => "Constrained overlap",
            Self::Misplaced => "Misplaced",
            Self::Missing => "Missing",
            Self::ParkingExempt => "Parking exempt",
        }
    }

    #[must_use]
    pub const fn meaning(self) -> &'static str {
        match self {
            Self::Leaked => "visible although its workspace is hidden",
            Self::Stranded => "off screen although its workspace is visible",
            Self::ParkedVisible => "marked hidden but on screen",
            Self::Unadopted => "on screen but owned by no workspace",
            Self::ConstrainedOverlap => {
                "a tile that resisted resizing overlaps another visible tile on its workspace"
            }
            Self::Misplaced => "not where its layout puts it",
            Self::Missing => "owned but gone from the window server",
            Self::ParkingExempt => "stopped hiding after repeatedly un-parking itself",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Anomaly {
    pub kind: AnomalyKind,
    pub window: u32,
    pub bundle_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<WorkspaceId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame: Option<Rect>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected: Option<Rect>,
}

/// Debounces anomalies: one is journaled only after persisting for
/// [`ANOMALY_SETTLE`], and its end is journaled with the total duration.
#[derive(Debug, Default)]
pub struct AnomalyTracker {
    pending: HashMap<(AnomalyKind, u32), Instant>,
    active: BTreeMap<(AnomalyKind, u32), (Anomaly, Instant)>,
}

impl AnomalyTracker {
    pub fn update(&mut self, now: Instant, current: Vec<Anomaly>) -> Vec<JournalEvent> {
        let mut events = Vec::new();
        let mut present = BTreeMap::new();
        for anomaly in current {
            present.insert((anomaly.kind, anomaly.window), anomaly);
        }
        for (_, (anomaly, began)) in self
            .active
            .extract_if(.., |key, _| !present.contains_key(key))
        {
            events.push(JournalEvent::Anomaly {
                state: AnomalyState::End,
                anomaly,
                duration_ms: Some(duration_ms(now.saturating_duration_since(began))),
            });
        }
        self.pending.retain(|key, _| present.contains_key(key));
        for (key, anomaly) in present {
            if self.active.contains_key(&key) {
                continue;
            }
            let first_seen = *self.pending.entry(key).or_insert(now);
            if now.saturating_duration_since(first_seen) < ANOMALY_SETTLE {
                continue;
            }
            self.pending.remove(&key);
            events.push(JournalEvent::Anomaly {
                state: AnomalyState::Begin,
                anomaly: anomaly.clone(),
                // How long the disagreement had already persisted before it was journaled,
                // so a still-open anomaly reports the same duration as a finished one.
                duration_ms: Some(duration_ms(now.saturating_duration_since(first_seen))),
            });
            self.active.insert(key, (anomaly, first_seen));
        }
        events
    }
}

fn duration_ms(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

#[must_use]
pub fn unix_millis() -> u64 {
    duration_ms(crate::config::now())
}

/// Append one line to the journal at `path`, rotating it to `rotated` first
/// when it has outgrown [`MAX_JOURNAL_BYTES`]. Files are owner-only.
pub fn append(path: &Path, rotated: &Path, line: &JournalLine) -> Result<(), String> {
    use std::os::unix::fs::OpenOptionsExt;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create journal directory: {error}"))?;
    }
    if std::fs::metadata(path).is_ok_and(|metadata| metadata.len() > MAX_JOURNAL_BYTES) {
        std::fs::rename(path, rotated)
            .map_err(|error| format!("failed to rotate journal: {error}"))?;
    }
    let mut bytes =
        serde_json::to_vec(line).map_err(|error| format!("failed to encode journal: {error}"))?;
    bytes.push(b'\n');
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(path)
        .and_then(|mut file| file.write_all(&bytes))
        .map_err(|error| format!("failed to write journal: {error}"))
}

#[must_use]
pub fn default_paths() -> Option<(PathBuf, PathBuf)> {
    Some((
        crate::config::application_support_path(JOURNAL_FILE)?,
        crate::config::application_support_path(ROTATED_JOURNAL_FILE)?,
    ))
}

/// Read the rotated generation followed by the live journal, skipping lines
/// that do not parse (a crash may truncate the final line).
pub fn read(paths: &[impl AsRef<Path>]) -> Result<(Vec<JournalLine>, usize), String> {
    let mut lines = Vec::new();
    let mut skipped = 0;
    for path in paths {
        let path = path.as_ref();
        let file = match std::fs::File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(format!("failed to read {}: {error}", path.display())),
        };
        for line in std::io::BufReader::new(file).lines() {
            let line = line.map_err(|error| format!("failed to read journal: {error}"))?;
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str(&line) {
                Ok(line) => lines.push(line),
                Err(_) => skipped += 1,
            }
        }
    }
    Ok((lines, skipped))
}

#[cfg(test)]
mod tests;
