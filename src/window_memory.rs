//! Durable, local-only memory of where each application's windows lived.
//!
//! Physical window identity (process, launch time, window number) does not
//! survive an application relaunch or a reboot, so it cannot carry workspace
//! ownership across one. This memory keys on the bundle identifier instead and
//! records, per application, each managed window's workspace, tiling kind, last
//! on-screen frame, and optional local `AXDocument` path and window title (stored
//! strictly in mode-`0600` `window-memory.json`, never in logs, `fractile doctor`,
//! debug output, HUD output, fixtures, snapshots, or control responses).
//!
//! A relaunched application's new windows claim those placements in order:
//! 1. Exact `AXDocument` match
//! 2. Exact normalized title match (case-insensitive trimmed equality)
//! 3. Fuzzy title similarity match (unread counters stripped, shared tokens or
//!    project/site segments)
//! 4. Exact frame match (within [`FRAME_MATCH_TOLERANCE`])
//! 5. Size-only match (within [`FRAME_MATCH_TOLERANCE`])
//! 6. Window creation order
//!
//! Normally only a relaunch (an update or restart) recalls these placements;
//! an app the user opens mid-session opens where they are. Once the user moves
//! one of an app's windows back by hand right after Fractile opened it on the
//! active workspace, that app is marked returning and every new instance of it
//! recalls its placements, however long it was closed.
//!
//! The memory is advisory. It never proves a window exists, never moves a
//! window on its own, and an invalid file is ignored rather than blocking
//! recovery.

use std::{
    cmp::Reverse,
    collections::{BTreeMap, BTreeSet},
    fmt,
};

use serde::{Deserialize, Serialize};

use crate::{Rect, WorkspaceId};

/// Applications remembered at once; the least recently changed is evicted.
pub const MAX_APPS: usize = 128;
/// Windows remembered per application.
pub const MAX_WINDOWS_PER_APP: usize = 32;
const MAX_BUNDLE_ID_LEN: usize = 255;
/// Maximum length (in Unicode scalar values) stored for a window's `AXDocument`.
pub const MAX_DOCUMENT_LEN: usize = 1024;
/// Maximum length (in Unicode scalar values) stored for a window's title.
pub const MAX_TITLE_LEN: usize = 512;
/// Applications restore saved frames with small adjustments (title bars,
/// display rounding), so a remembered frame matches within this tolerance.
pub const FRAME_MATCH_TOLERANCE: i32 = 8;
/// Minimum fuzzy title similarity score (on a 0..=1000 scale) to match in Tier 3.
const FUZZY_MATCH_THRESHOLD: u32 = 500;

/// Generic single-word tokens that never justify a fuzzy title match on their own.
const GENERIC_TOKENS: &[&str] = &[
    "blank",
    "default",
    "document",
    "draft",
    "file",
    "folder",
    "home",
    "new",
    "note",
    "notes",
    "page",
    "preferences",
    "settings",
    "start",
    "tab",
    "untitled",
    "welcome",
    "window",
];

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
pub struct RememberedWindow {
    pub workspace: WorkspaceId,
    pub frame: Rect,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub floating: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

impl fmt::Debug for RememberedWindow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RememberedWindow")
            .field("workspace", &self.workspace)
            .field("frame", &self.frame)
            .field("floating", &self.floating)
            .finish()
    }
}

impl RememberedWindow {
    /// Whether two remembered placements represent the same settled state for
    /// dirty-checking: workspace, frame, floating flag, document, and
    /// case-insensitive trimmed title all agree.
    #[must_use]
    pub fn same_identity(&self, other: &Self) -> bool {
        self.workspace == other.workspace
            && self.frame == other.frame
            && self.floating == other.floating
            && self.document == other.document
            && normalized_exact_title(self.title.as_deref())
                == normalized_exact_title(other.title.as_deref())
    }

    fn normalize(&mut self) {
        self.document = self.document.as_deref().and_then(normalize_document);
        self.title = self.title.as_deref().and_then(normalize_title);
    }
}

/// Trim and bound an observed `AXDocument` value to [`MAX_DOCUMENT_LEN`] chars,
/// returning `None` when empty.
#[must_use]
pub fn normalize_document(raw: &str) -> Option<String> {
    normalize_bounded(raw, MAX_DOCUMENT_LEN)
}

/// Trim and bound an observed window title to [`MAX_TITLE_LEN`] chars, returning
/// `None` when empty.
#[must_use]
pub fn normalize_title(raw: &str) -> Option<String> {
    normalize_bounded(raw, MAX_TITLE_LEN)
}

fn normalize_bounded(raw: &str, max_chars: usize) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let bounded = match trimmed.char_indices().nth(max_chars) {
        Some((byte_index, _)) => trimmed[..byte_index].trim_end(),
        None => trimmed,
    };
    (!bounded.is_empty()).then(|| bounded.to_owned())
}

pub(crate) fn normalized_exact_title(raw: Option<&str>) -> Option<String> {
    let trimmed = raw?.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_lowercase())
}

/// Whether two slices of remembered windows are unchanged under
/// [`RememberedWindow::same_identity`].
#[must_use]
pub fn same_window_list(left: &[RememberedWindow], right: &[RememberedWindow]) -> bool {
    left.len() == right.len() && left.iter().zip(right).all(|(a, b)| a.same_identity(b))
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct RememberedApp {
    /// Unix seconds when these placements last changed; drives eviction.
    pub updated_at: u64,
    /// Ordered by window creation (window-server ID) when recorded.
    pub windows: Vec<RememberedWindow>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct WindowMemory {
    #[serde(default)]
    apps: BTreeMap<String, RememberedApp>,
    /// Bundles whose new instances always recall their placements.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    returning: BTreeSet<String>,
}

impl WindowMemory {
    pub fn validate(&self) -> Result<(), String> {
        if self.apps.len() > MAX_APPS {
            return Err(format!(
                "window memory holds more than {MAX_APPS} applications"
            ));
        }
        for (bundle_id, app) in &self.apps {
            if bundle_id.is_empty() || bundle_id.len() > MAX_BUNDLE_ID_LEN {
                return Err("window memory has an invalid bundle identifier".into());
            }
            if app.windows.is_empty() || app.windows.len() > MAX_WINDOWS_PER_APP {
                return Err(format!(
                    "window memory for {bundle_id} must hold 1 through {MAX_WINDOWS_PER_APP} windows"
                ));
            }
            for window in &app.windows {
                if let Some(document) = &window.document
                    && (document.trim().is_empty() || document.chars().count() > MAX_DOCUMENT_LEN)
                {
                    return Err(format!(
                        "window memory for {bundle_id} has an invalid document entry"
                    ));
                }
                if let Some(title) = &window.title
                    && (title.trim().is_empty() || title.chars().count() > MAX_TITLE_LEN)
                {
                    return Err(format!(
                        "window memory for {bundle_id} has an invalid title entry"
                    ));
                }
            }
        }
        if self.returning.len() > MAX_APPS {
            return Err(format!(
                "window memory marks more than {MAX_APPS} applications as returning"
            ));
        }
        if self
            .returning
            .iter()
            .any(|bundle_id| bundle_id.is_empty() || bundle_id.len() > MAX_BUNDLE_ID_LEN)
        {
            return Err("window memory has an invalid returning bundle identifier".into());
        }
        Ok(())
    }

    /// Whether every new instance of `bundle_id` recalls its placements.
    #[must_use]
    pub fn returns(&self, bundle_id: &str) -> bool {
        self.returning.contains(bundle_id)
    }

    /// Mark `bundle_id` as returning. Returns whether the memory changed.
    pub fn mark_returning(&mut self, bundle_id: &str) -> bool {
        if bundle_id.is_empty()
            || bundle_id.len() > MAX_BUNDLE_ID_LEN
            || self.returning.contains(bundle_id)
        {
            return false;
        }
        if self.returning.len() >= MAX_APPS {
            let Some(stale) = self
                .returning
                .iter()
                .find(|candidate| !self.apps.contains_key(*candidate))
                .cloned()
            else {
                return false;
            };
            self.returning.remove(&stale);
        }
        self.returning.insert(bundle_id.to_owned())
    }

    #[must_use]
    pub fn get(&self, bundle_id: &str) -> Option<&RememberedApp> {
        self.apps.get(bundle_id)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.apps.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.apps.is_empty()
    }

    /// Replace an application's remembered placements. Returns whether the
    /// memory changed. Empty placements are ignored: an application with no
    /// managed windows keeps its previous memory for its next launch.
    pub fn record(
        &mut self,
        bundle_id: &str,
        mut windows: Vec<RememberedWindow>,
        now_secs: u64,
    ) -> bool {
        if bundle_id.is_empty() || bundle_id.len() > MAX_BUNDLE_ID_LEN || windows.is_empty() {
            return false;
        }
        windows.truncate(MAX_WINDOWS_PER_APP);
        for window in &mut windows {
            window.normalize();
        }
        if self
            .apps
            .get(bundle_id)
            .is_some_and(|app| same_window_list(&app.windows, &windows))
        {
            return false;
        }
        self.apps.insert(
            bundle_id.to_owned(),
            RememberedApp {
                updated_at: now_secs,
                windows,
            },
        );
        while self.apps.len() > MAX_APPS {
            let Some(oldest) = self
                .apps
                .iter()
                .filter(|(candidate, _)| candidate.as_str() != bundle_id)
                .min_by_key(|(_, app)| app.updated_at)
                .map(|(candidate, _)| candidate.clone())
            else {
                break;
            };
            self.apps.remove(&oldest);
            self.returning.remove(&oldest);
        }
        true
    }
}

/// An observed window competing to claim a remembered placement.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct WindowCandidate<'a, K> {
    pub id: K,
    pub frame: Rect,
    pub document: Option<&'a str>,
    pub title: Option<&'a str>,
}

impl<K: fmt::Debug> fmt::Debug for WindowCandidate<'_, K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WindowCandidate")
            .field("id", &self.id)
            .field("frame", &self.frame)
            .finish()
    }
}

/// Assign remembered placements to newly observed windows.
///
/// `windows` carry keys ordered by creation alongside each window's frame and
/// optional `AXDocument` and title. Each remembered placement is claimed at most
/// once, tier by tier:
/// 1. Exact `AXDocument` match
/// 2. Exact normalized title match (case-insensitive trimmed equality)
/// 3. Fuzzy title similarity match (highest-scoring pair first)
/// 4. Exact frame match within [`FRAME_MATCH_TOLERANCE`]
/// 5. Size-only match within [`FRAME_MATCH_TOLERANCE`]
/// 6. Creation order fallback
///
/// Returns `(window, index into remembered)` pairs sorted by `id`; windows
/// beyond the remembered count receive nothing.
#[must_use]
pub fn assign<K: Ord + Clone>(
    remembered: &[RememberedWindow],
    windows: &[WindowCandidate<'_, K>],
) -> Vec<(K, usize)> {
    let mut pending: Vec<WindowCandidate<'_, K>> = windows.to_vec();
    pending.sort_by(|left, right| left.id.cmp(&right.id));
    let mut claimed = vec![false; remembered.len()];
    let mut assignments = Vec::new();

    let rem_docs: Vec<Option<String>> = remembered
        .iter()
        .map(|entry| entry.document.as_deref().and_then(normalize_document))
        .collect();
    let rem_exact_titles: Vec<Option<String>> = remembered
        .iter()
        .map(|entry| normalized_exact_title(entry.title.as_deref()))
        .collect();

    // Tier 1: exact AXDocument match.
    let cand_docs: Vec<Option<String>> = pending
        .iter()
        .map(|cand| cand.document.and_then(normalize_document))
        .collect();
    pending = claim_best_pairs(
        pending,
        remembered,
        &mut claimed,
        &mut assignments,
        |cand_idx, _, rem_idx, _| {
            let cand_doc = cand_docs[cand_idx].as_deref()?;
            let rem_doc = rem_docs[rem_idx].as_deref()?;
            (cand_doc == rem_doc).then_some(1)
        },
    );

    // Tier 2: exact normalized title match (case-insensitive trimmed equality).
    let cand_exact_titles: Vec<Option<String>> = pending
        .iter()
        .map(|cand| normalized_exact_title(cand.title))
        .collect();
    pending = claim_best_pairs(
        pending,
        remembered,
        &mut claimed,
        &mut assignments,
        |cand_idx, _, rem_idx, _| {
            let cand_title = cand_exact_titles[cand_idx].as_deref()?;
            let rem_title = rem_exact_titles[rem_idx].as_deref()?;
            (cand_title == rem_title).then_some(1)
        },
    );

    // Tier 3: fuzzy title similarity match.
    if !pending.is_empty() && claimed.iter().any(|is_claimed| !is_claimed) {
        let mut cand_segs: Vec<Option<Vec<String>>> = pending
            .iter()
            .map(|cand| cand.title.and_then(clean_title_segments))
            .collect();
        let mut rem_segs: Vec<Option<Vec<String>>> = remembered
            .iter()
            .enumerate()
            .map(|(idx, entry)| {
                (!claimed[idx])
                    .then(|| entry.title.as_deref().and_then(clean_title_segments))
                    .flatten()
            })
            .collect();
        strip_common_chrome_segments(&mut cand_segs, &mut rem_segs);
        pending = claim_best_pairs(
            pending,
            remembered,
            &mut claimed,
            &mut assignments,
            |cand_idx, _, rem_idx, _| {
                let left = cand_segs[cand_idx].as_deref()?;
                let right = rem_segs[rem_idx].as_deref()?;
                fuzzy_segments_score(left, right)
            },
        );
    }

    // Tier 4: exact frame match (±FRAME_MATCH_TOLERANCE).
    pending = claim_best_pairs(
        pending,
        remembered,
        &mut claimed,
        &mut assignments,
        |_, cand, _, entry| {
            entry
                .frame
                .approximately_equals(cand.frame, FRAME_MATCH_TOLERANCE)
                .then_some(1)
        },
    );

    // Tier 5: size-only match (±FRAME_MATCH_TOLERANCE).
    pending = claim_best_pairs(
        pending,
        remembered,
        &mut claimed,
        &mut assignments,
        |_, cand, _, entry| {
            ((entry.frame.width - cand.frame.width).abs() <= FRAME_MATCH_TOLERANCE
                && (entry.frame.height - cand.frame.height).abs() <= FRAME_MATCH_TOLERANCE)
                .then_some(1)
        },
    );

    // Tier 6: creation order fallback.
    for candidate in pending {
        let Some(index) = claimed.iter().position(|is_claimed| !is_claimed) else {
            break;
        };
        claimed[index] = true;
        assignments.push((candidate.id, index));
    }

    assignments.sort_by(|(left, _), (right, _)| left.cmp(right));
    assignments
}

fn claim_best_pairs<'a, K: Clone>(
    pending: Vec<WindowCandidate<'a, K>>,
    remembered: &[RememberedWindow],
    claimed: &mut [bool],
    assignments: &mut Vec<(K, usize)>,
    score_pair: impl Fn(usize, &WindowCandidate<'a, K>, usize, &RememberedWindow) -> Option<u32>,
) -> Vec<WindowCandidate<'a, K>> {
    let mut matched_pending = vec![false; pending.len()];
    let mut pairs: Vec<(u32, u64, usize, usize)> = Vec::new();
    for (cand_idx, cand) in pending.iter().enumerate() {
        for (rem_idx, entry) in remembered.iter().enumerate() {
            if claimed[rem_idx] {
                continue;
            }
            if let Some(score) = score_pair(cand_idx, cand, rem_idx, entry) {
                pairs.push((
                    score,
                    frame_distance(entry.frame, cand.frame),
                    cand_idx,
                    rem_idx,
                ));
            }
        }
    }
    pairs
        .sort_by_key(|&(score, dist, cand_idx, rem_idx)| (Reverse(score), dist, cand_idx, rem_idx));
    for (_, _, cand_idx, rem_idx) in pairs {
        if matched_pending[cand_idx] || claimed[rem_idx] {
            continue;
        }
        matched_pending[cand_idx] = true;
        claimed[rem_idx] = true;
        assignments.push((pending[cand_idx].id.clone(), rem_idx));
    }
    pending
        .into_iter()
        .enumerate()
        .filter_map(|(idx, cand)| (!matched_pending[idx]).then_some(cand))
        .collect()
}

fn frame_distance(left: Rect, right: Rect) -> u64 {
    u64::from(left.x.abs_diff(right.x))
        + u64::from(left.y.abs_diff(right.y))
        + u64::from(left.width.abs_diff(right.width))
        + u64::from(left.height.abs_diff(right.height))
}

/// Fuzzy similarity score between two raw titles on a 0..=1000 scale, or
/// `None` when below [`FUZZY_MATCH_THRESHOLD`].
#[must_use]
pub fn fuzzy_title_score(left: &str, right: &str) -> Option<u32> {
    let left_segs = clean_title_segments(left)?;
    let right_segs = clean_title_segments(right)?;
    fuzzy_segments_score(&left_segs, &right_segs)
}

fn clean_title_segments(raw: &str) -> Option<Vec<String>> {
    let cleaned = strip_unread_counters(raw);
    if cleaned.is_empty() {
        return None;
    }
    let segments = split_title_segments(&cleaned);
    (!segments.is_empty()).then_some(segments)
}

/// Lowercase `raw`, remove standalone parenthesized/bracketed unread counters
/// like `(3)`, `(12)`, `(99+)`, `[2]`, and collapse whitespace.
fn strip_unread_counters(raw: &str) -> String {
    let lower = raw.to_lowercase();
    let chars: Vec<char> = lower.chars().collect();
    let mut out = String::with_capacity(lower.len());
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        if ch == '(' || ch == '[' {
            let close = if ch == '(' { ')' } else { ']' };
            if let Some(rel_end) = chars[i + 1..].iter().position(|&c| c == close) {
                let inner = &chars[i + 1..i + 1 + rel_end];
                if is_unread_counter(inner) {
                    out.push(' ');
                    i += rel_end + 2;
                    continue;
                }
            }
        }
        out.push(ch);
        i += 1;
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn is_unread_counter(inner: &[char]) -> bool {
    let trimmed: Vec<char> = inner
        .iter()
        .copied()
        .filter(|c| !c.is_whitespace())
        .collect();
    if trimmed.is_empty() {
        return false;
    }
    let digits = trimmed.strip_suffix(&['+']).unwrap_or(&trimmed);
    (1..=5).contains(&digits.len()) && digits.iter().all(char::is_ascii_digit)
}

fn split_title_segments(cleaned: &str) -> Vec<String> {
    let mut segments = Vec::new();
    let mut current = String::new();
    let chars: Vec<char> = cleaned.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        let is_sep = matches!(ch, '—' | '–' | '|' | '·')
            || (ch == '-'
                && i > 0
                && i + 1 < chars.len()
                && chars[i - 1].is_whitespace()
                && chars[i + 1].is_whitespace());
        if is_sep {
            let trimmed = current.trim();
            if !trimmed.is_empty() {
                segments.push(trimmed.to_owned());
            }
            current.clear();
        } else {
            current.push(ch);
        }
        i += 1;
    }
    let trimmed = current.trim();
    if !trimmed.is_empty() {
        segments.push(trimmed.to_owned());
    }
    segments
}

/// Strip a shared leading or trailing app-chrome segment (such as
/// `"google chrome"` or `"visual studio code"`) when at least 3 titles are
/// present and every non-empty title has `>= 2` segments sharing that exact
/// leading or trailing segment.
fn strip_common_chrome_segments(
    cand_segs: &mut [Option<Vec<String>>],
    rem_segs: &mut [Option<Vec<String>>],
) {
    let total = cand_segs.iter().flatten().count() + rem_segs.iter().flatten().count();
    if total < 3 {
        return;
    }
    loop {
        let all_multi = cand_segs
            .iter()
            .chain(rem_segs.iter())
            .flatten()
            .all(|segs| segs.len() >= 2);
        if !all_multi {
            break;
        }
        let common_tail = {
            let mut iter = cand_segs.iter().chain(rem_segs.iter()).flatten();
            let first = iter.next().and_then(|s| s.last()).cloned();
            first.filter(|tail| iter.all(|segs| segs.last() == Some(tail)))
        };
        if common_tail.is_some() {
            for segs in cand_segs.iter_mut().chain(rem_segs.iter_mut()).flatten() {
                segs.pop();
            }
            continue;
        }
        let common_head = {
            let mut iter = cand_segs.iter().chain(rem_segs.iter()).flatten();
            let first = iter.next().and_then(|s| s.first()).cloned();
            first.filter(|head| iter.all(|segs| segs.first() == Some(head)))
        };
        if common_head.is_some() {
            for segs in cand_segs.iter_mut().chain(rem_segs.iter_mut()).flatten() {
                segs.remove(0);
            }
            continue;
        }
        break;
    }
}

fn fuzzy_segments_score(left: &[String], right: &[String]) -> Option<u32> {
    if left.is_empty() || right.is_empty() {
        return None;
    }
    let left_tokens = tokenize_segments(left);
    let right_tokens = tokenize_segments(right);
    if left_tokens.is_empty() || right_tokens.is_empty() {
        return None;
    }

    // Counter-stripped exact equality (e.g. "Inbox (4) - Gmail" vs "Inbox (3) - Gmail").
    if left == right {
        return has_non_generic_token(&left_tokens).then_some(1000);
    }

    let shared_tokens: Vec<&str> = left_tokens
        .intersection(&right_tokens)
        .map(String::as_str)
        .collect();
    if !shared_tokens.iter().any(|token| !is_generic_token(token)) {
        return None;
    }

    let dice_millis =
        u32::try_from((2_000 * shared_tokens.len()) / (left_tokens.len() + right_tokens.len()))
            .unwrap_or(1000);

    let mut best = if dice_millis >= FUZZY_MATCH_THRESHOLD {
        dice_millis
    } else {
        0
    };

    // Multi-segment titles sharing their leading or trailing project/site segment
    // (e.g. "fractile — Cargo.toml" vs "fractile — main.rs").
    if left.len() >= 2 && right.len() >= 2 {
        let matched_anchor = if left.first() == right.first() {
            left.first()
        } else if left.last() == right.last() {
            left.last()
        } else {
            None
        };
        if let Some(anchor) = matched_anchor {
            let anchor_tokens = tokenize_str(anchor);
            if has_non_generic_token(&anchor_tokens) {
                let anchor_score = (FUZZY_MATCH_THRESHOLD + dice_millis / 2).min(999);
                best = best.max(anchor_score);
            }
        }
    }

    (best >= FUZZY_MATCH_THRESHOLD).then_some(best)
}

fn tokenize_segments(segments: &[String]) -> BTreeSet<String> {
    let mut tokens = BTreeSet::new();
    for segment in segments {
        tokens.extend(tokenize_str(segment));
    }
    tokens
}

fn tokenize_str(text: &str) -> BTreeSet<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|token| token.chars().count() >= 2 && !token.chars().all(|c| c.is_ascii_digit()))
        .map(str::to_owned)
        .collect()
}

fn has_non_generic_token(tokens: &BTreeSet<String>) -> bool {
    tokens.iter().any(|token| !is_generic_token(token))
}

fn is_generic_token(token: &str) -> bool {
    GENERIC_TOKENS.binary_search(&token).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(x: i32) -> Rect {
        Rect {
            x,
            y: 0,
            width: 400,
            height: 300,
        }
    }

    fn placed(workspace: u8, x: i32) -> RememberedWindow {
        RememberedWindow {
            workspace: WorkspaceId::new(workspace).unwrap(),
            frame: frame(x),
            floating: false,
            document: None,
            title: None,
        }
    }

    fn placed_with(
        workspace: u8,
        x: i32,
        document: Option<&str>,
        title: Option<&str>,
    ) -> RememberedWindow {
        RememberedWindow {
            workspace: WorkspaceId::new(workspace).unwrap(),
            frame: frame(x),
            floating: false,
            document: document.map(str::to_owned),
            title: title.map(str::to_owned),
        }
    }

    fn cand<'a>(
        id: u32,
        frame: Rect,
        document: Option<&'a str>,
        title: Option<&'a str>,
    ) -> WindowCandidate<'a, u32> {
        WindowCandidate {
            id,
            frame,
            document,
            title,
        }
    }

    #[test]
    fn generic_tokens_table_is_sorted() {
        assert!(GENERIC_TOKENS.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn exact_document_wins_over_same_size_windows_in_reverse_order() {
        let same = frame(0);
        let remembered = [
            placed_with(
                1,
                0,
                Some("file:///Users/example/code/fractile"),
                Some("fractile — Cargo.toml"),
            ),
            placed_with(
                3,
                0,
                Some("file:///Users/example/code/opencode"),
                Some("opencode — package.json"),
            ),
        ];
        // Reopened in reverse creation order with different active file tabs (titles changed),
        // identical frames, and matching AXDocument URLs.
        let assignments = assign(
            &remembered,
            &[
                cand(
                    10,
                    same,
                    Some("file:///Users/example/code/opencode"),
                    Some("opencode — README.md"),
                ),
                cand(
                    20,
                    same,
                    Some("file:///Users/example/code/fractile"),
                    Some("fractile — src/main.rs"),
                ),
            ],
        );
        assert_eq!(assignments, vec![(10, 1), (20, 0)]);
    }

    #[test]
    fn exact_normalized_title_wins_over_same_size_windows_in_reverse_order() {
        let same = frame(0);
        let remembered = [
            placed_with(1, 0, None, Some("Fractile Docs")),
            placed_with(4, 0, None, Some("Linear — Roadmap")),
        ];
        let assignments = assign(
            &remembered,
            &[
                cand(10, same, None, Some("  linear — roadmap  ")),
                cand(20, same, None, Some("FRACTILE DOCS")),
            ],
        );
        assert_eq!(assignments, vec![(10, 1), (20, 0)]);
    }

    #[test]
    fn fuzzy_title_matches_unread_counters_and_project_segments_without_false_positives() {
        assert_eq!(
            fuzzy_title_score("Inbox (4) - Gmail", "Inbox (3) - Gmail"),
            Some(1000)
        );
        assert!(fuzzy_title_score("fractile — Cargo.toml", "fractile — main.rs").is_some());
        assert!(fuzzy_title_score("Cargo.toml — fractile", "main.rs — fractile").is_some());

        // Unrelated titles and generic placeholders must not match.
        assert_eq!(
            fuzzy_title_score("Slack — #general", "Figma — Dashboard"),
            None
        );
        assert_eq!(fuzzy_title_score("Untitled (1)", "Untitled (2)"), None);
        assert_eq!(fuzzy_title_score("Untitled 1", "Untitled 2"), None);
        assert_eq!(
            fuzzy_title_score("Settings - Linear", "Settings - Figma"),
            None
        );
        assert_eq!(fuzzy_title_score("Window 1", "Window 2"), None);

        // End-to-end assign with identical frames reopened in reverse order.
        let same = frame(0);
        let remembered = [
            placed_with(2, 0, None, Some("Inbox (3) - Gmail")),
            placed_with(5, 0, None, Some("fractile — Cargo.toml")),
        ];
        let assignments = assign(
            &remembered,
            &[
                cand(10, same, None, Some("fractile — main.rs")),
                cand(20, same, None, Some("Inbox (4) - Gmail")),
            ],
        );
        assert_eq!(assignments, vec![(10, 1), (20, 0)]);
    }

    #[test]
    fn common_browser_suffix_does_not_cause_false_fuzzy_match() {
        // Two Chrome windows whose titles share only "- Google Chrome".
        // Distinct frames should decide the unmatched one instead of a bogus fuzzy hit.
        let remembered = [
            placed_with(1, 0, None, Some("Inbox (3) - Gmail - Google Chrome")),
            placed_with(2, 600, None, Some("Dashboard - Figma - Google Chrome")),
            placed_with(3, 1200, None, Some("PRs - GitHub - Google Chrome")),
        ];
        let assignments = assign(
            &remembered,
            &[
                // Window 10 navigated to an unrelated site, but kept workspace 3's frame.
                cand(
                    10,
                    frame(1200),
                    None,
                    Some("Notes - Notion - Google Chrome"),
                ),
                // Window 20 is still on Gmail with a different unread count.
                cand(
                    20,
                    frame(0),
                    None,
                    Some("Inbox (9) - Gmail - Google Chrome"),
                ),
                // Window 30 kept workspace 2's frame with a new site.
                cand(30, frame(600), None, Some("Tasks - Linear - Google Chrome")),
            ],
        );
        assert_eq!(assignments, vec![(10, 2), (20, 0), (30, 1)]);
    }

    #[test]
    fn frame_matches_win_before_creation_order() {
        let remembered = [placed(1, 0), placed(3, 500), placed(5, 900)];
        // Windows were recreated in a different order: 20 at the second
        // frame, 30 at the first, 10 somewhere new.
        let assignments = assign(
            &remembered,
            &[
                cand(20, frame(502), None, None),
                cand(30, frame(0), None, None),
                cand(10, frame(1500), None, None),
            ],
        );
        assert_eq!(assignments, vec![(10, 2), (20, 1), (30, 0)]);
    }

    #[test]
    fn sizes_match_before_creation_order() {
        // Three notes of different sizes on workspaces 1, 2 and 3, reopened at one rescued
        // position, in a different creation order.
        let note = |workspace, width, height| RememberedWindow {
            workspace: WorkspaceId::new(workspace).unwrap(),
            frame: Rect {
                x: 40 * i32::from(workspace),
                y: 60,
                width,
                height,
            },
            floating: true,
            document: None,
            title: None,
        };
        let remembered = [note(1, 220, 180), note(2, 300, 240), note(3, 180, 400)];
        let at_origin = |width, height| Rect {
            x: 0,
            y: 0,
            width,
            height,
        };
        let assignments = assign(
            &remembered,
            &[
                cand(7, at_origin(180, 400), None, None),
                cand(8, at_origin(222, 178), None, None),
                cand(9, at_origin(300, 240), None, None),
            ],
        );
        assert_eq!(assignments, vec![(7, 2), (8, 0), (9, 1)]);
    }

    #[test]
    fn identical_frames_fall_back_to_creation_order() {
        let remembered = [placed(1, 0), placed(2, 0)];
        let assignments = assign(
            &remembered,
            &[cand(9, frame(0), None, None), cand(4, frame(0), None, None)],
        );
        assert_eq!(assignments, vec![(4, 0), (9, 1)]);
    }

    #[test]
    fn surplus_windows_are_left_unassigned() {
        let remembered = [placed(4, 0)];
        let assignments = assign(
            &remembered,
            &[
                cand(1, frame(700), None, None),
                cand(2, frame(0), None, None),
            ],
        );
        assert_eq!(assignments, vec![(2, 0)]);
    }

    #[test]
    fn recording_ignores_empty_and_unchanged_placements() {
        let mut memory = WindowMemory::default();
        assert!(!memory.record("com.example.App", Vec::new(), 1));
        assert!(memory.record(
            "com.example.App",
            vec![placed_with(
                2,
                0,
                Some("file:///Users/example/a"),
                Some("Alpha")
            )],
            1
        ));
        // Case and outer whitespace differences on title do not mark dirty.
        assert!(!memory.record(
            "com.example.App",
            vec![placed_with(
                2,
                0,
                Some("file:///Users/example/a"),
                Some("  alpha  ")
            )],
            2
        ));
        assert_eq!(memory.get("com.example.App").unwrap().updated_at, 1);
        // Changing the title or document marks dirty.
        assert!(memory.record(
            "com.example.App",
            vec![placed_with(
                2,
                0,
                Some("file:///Users/example/a"),
                Some("Beta")
            )],
            3
        ));
        assert_eq!(memory.get("com.example.App").unwrap().updated_at, 3);
        assert!(!memory.record("com.example.App", Vec::new(), 4));
        assert_eq!(memory.get("com.example.App").unwrap().windows.len(), 1);
    }

    #[test]
    fn recording_evicts_the_least_recently_changed_application() {
        let mut memory = WindowMemory::default();
        for index in 0..MAX_APPS {
            memory.record(
                &format!("com.example.{index}"),
                vec![placed(1, 0)],
                index as u64 + 10,
            );
        }
        memory.record("com.example.0", vec![placed(2, 0)], 1_000);
        memory.record("com.example.new", vec![placed(3, 0)], 1_001);
        assert_eq!(memory.len(), MAX_APPS);
        assert!(memory.get("com.example.0").is_some());
        assert!(memory.get("com.example.1").is_none());
        assert!(memory.get("com.example.new").is_some());
        memory.validate().unwrap();
    }

    #[test]
    fn returning_marks_persist_and_older_files_still_load() {
        let mut memory = WindowMemory::default();
        memory.record("com.example.App", vec![placed(3, 0)], 1);
        assert!(!memory.returns("com.example.App"));
        assert!(memory.mark_returning("com.example.App"));
        assert!(!memory.mark_returning("com.example.App"));
        assert!(!memory.mark_returning(""));
        let json = serde_json::to_string(&memory).unwrap();
        assert!(
            json.ends_with(r#""returning":["com.example.App"]}"#),
            "{json}"
        );
        let restored: WindowMemory = serde_json::from_str(&json).unwrap();
        restored.validate().unwrap();
        assert!(restored.returns("com.example.App"));

        // Files written before returning marks, titles, or documents existed load unchanged.
        let older: WindowMemory = serde_json::from_str(
            r#"{"apps":{"com.example.App":{"updated_at":1,"windows":[{"workspace":3,"frame":{"x":0,"y":0,"width":400,"height":300}}]}}}"#,
        )
        .unwrap();
        older.validate().unwrap();
        assert!(!older.returns("com.example.App"));
        let window = &older.get("com.example.App").unwrap().windows[0];
        assert!(window.document.is_none());
        assert!(window.title.is_none());
    }

    #[test]
    fn persisted_form_round_trips_validates_and_never_debugs_title_or_document() {
        let mut memory = WindowMemory::default();
        let secret_doc = "file:///Users/example/private/secret.rs";
        let secret_title = "Quarterly Secret Plan";
        memory.record(
            "com.example.App",
            vec![
                placed(1, 0),
                RememberedWindow {
                    workspace: WorkspaceId::named('K').unwrap(),
                    frame: frame(10),
                    floating: true,
                    document: Some(format!("  {secret_doc}  ")),
                    title: Some(format!("  {secret_title}  ")),
                },
            ],
            7,
        );
        let json = serde_json::to_string(&memory).unwrap();
        assert_eq!(
            json,
            r#"{"apps":{"com.example.App":{"updated_at":7,"windows":[{"workspace":1,"frame":{"x":0,"y":0,"width":400,"height":300}},{"workspace":"K","frame":{"x":10,"y":0,"width":400,"height":300},"floating":true,"document":"file:///Users/example/private/secret.rs","title":"Quarterly Secret Plan"}]}}}"#
        );
        let restored: WindowMemory = serde_json::from_str(&json).unwrap();
        restored.validate().unwrap();
        assert_eq!(restored, memory);

        // Debug formatting must never leak title or document strings.
        let candidate = cand(1, frame(0), Some(secret_doc), Some(secret_title));
        for debug in [
            format!("{memory:?}"),
            format!("{memory:#?}"),
            format!("{candidate:?}"),
        ] {
            assert!(!debug.contains(secret_doc), "Debug leaked document");
            assert!(!debug.contains(secret_title), "Debug leaked title");
        }

        // Validation rejects empty windows, empty/blank title or document, and over-long strings.
        let invalid: WindowMemory =
            serde_json::from_str(r#"{"apps":{"com.example.App":{"updated_at":1,"windows":[]}}}"#)
                .unwrap();
        assert!(invalid.validate().is_err());

        let too_long_doc = "x".repeat(MAX_DOCUMENT_LEN + 1);
        let overlong_doc: WindowMemory = serde_json::from_str(&format!(
            r#"{{"apps":{{"com.example.App":{{"updated_at":1,"windows":[{{"workspace":1,"frame":{{"x":0,"y":0,"width":400,"height":300}},"document":"{too_long_doc}"}}]}}}}}}"#
        ))
        .unwrap();
        assert!(overlong_doc.validate().is_err());

        let too_long_title = "y".repeat(MAX_TITLE_LEN + 1);
        let overlong_title: WindowMemory = serde_json::from_str(&format!(
            r#"{{"apps":{{"com.example.App":{{"updated_at":1,"windows":[{{"workspace":1,"frame":{{"x":0,"y":0,"width":400,"height":300}},"title":"{too_long_title}"}}]}}}}}}"#
        ))
        .unwrap();
        assert!(overlong_title.validate().is_err());

        let blank_title: WindowMemory = serde_json::from_str(
            r#"{"apps":{"com.example.App":{"updated_at":1,"windows":[{"workspace":1,"frame":{"x":0,"y":0,"width":400,"height":300},"title":"   "}]}}}"#,
        )
        .unwrap();
        assert!(blank_title.validate().is_err());

        // Normalization truncates safely on UTF-8 char boundaries.
        let emoji = "😀".repeat(MAX_TITLE_LEN + 10);
        let normalized = normalize_title(&emoji).unwrap();
        assert_eq!(normalized.chars().count(), MAX_TITLE_LEN);
    }
}
