//! Focus intent as one automaton. Accessibility reports focus asynchronously:
//! after Fractile asks for a window it can keep reporting the window focus
//! left (or nothing) for a few cycles, and a hidden window can keep focus
//! after its workspace is switched away. Every reader asks the tracker what
//! focus *effectively* is, so commands, focus follow, history and diagnostics
//! never disagree about which window a lagging report stands for.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use crate::workspace::WorkspaceId;
use crate::{AppInstanceId, PhysicalWindowId};

mod chosen;
mod hover;
mod requests;
mod target;

pub(super) use chosen::EmptyChoice;

/// How long Accessibility may keep reporting the previously focused window
/// after a focus command. App activation is asynchronous, so a second ⌥L
/// pressed in that window would otherwise step from the stale window and land
/// on the same target again.
pub(super) const FOCUS_SETTLE: Duration = Duration::from_millis(250);

/// How long after a failed focus follow a macOS Space change may still explain it.
pub(super) const FAILED_FOLLOW_RETRY_WINDOW: Duration = Duration::from_secs(3);

/// Focus Fractile just requested, until Accessibility reports it.
#[derive(Clone, Debug)]
pub(super) struct FocusRequest {
    pub(super) target: PhysicalWindowId,
    /// Reports Accessibility may still give while the target takes focus: the
    /// window focus left and earlier targets in a run of quick presses.
    pub(super) stale: Vec<Option<PhysicalWindowId>>,
    pub(super) issued_at: Instant,
}

/// Focus left behind by a workspace switch, a move or a rollback. Focus follow
/// ignores it until Accessibility reports a different focus, so a hidden window
/// that keeps (or has not yet lost) focus cannot pull its workspace back.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct DepartureGuard {
    pub(super) window: Option<PhysicalWindowId>,
    /// The frontmost application then, for when Accessibility reports no
    /// focused window for it (or one that is not frontmost).
    pub(super) app: Option<AppInstanceId>,
}

#[derive(Clone, Debug, Default)]
pub(super) enum FocusPhase {
    /// Accessibility and Fractile agree; an outside activation is followed.
    #[default]
    Synced,
    /// Fractile asked for the request's target; reports it explains stand for it.
    InFlight {
        request: FocusRequest,
        departure: Option<DepartureGuard>,
    },
    /// Focus left behind; only a fresh, different report ends the guard.
    GuardedDeparture {
        guard: DepartureGuard,
        /// When a focus follow that failed (not cancelled) began.
        failed_follow_at: Option<Instant>,
    },
}

/// The workspace focused now and the one before it, across displays:
/// Option+Tab bounces between them wherever they live.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct WorkspaceHistory {
    pub(super) current: Option<WorkspaceId>,
    pub(super) previous: Option<WorkspaceId>,
}

#[derive(Debug, Default)]
pub(super) struct FocusTracker {
    phase: FocusPhase,
    history: WorkspaceHistory,
    /// An empty workspace chosen on another display while a window there kept
    /// focus (`[labs] empty_workspace_focus`).
    empty_choice: Option<EmptyChoice>,
    /// Restored Fullscreen Layout workspaces whose front tile is raised again
    /// on their first cycle after a restart.
    fullscreen_restore: HashSet<WorkspaceId>,
    /// Where mouse follows focus last put the pointer. Focus follows mouse never
    /// counts the pointer resting there as a hover, so the two cannot feed each other.
    last_warp: Option<(i32, i32)>,
}

impl FocusRequest {
    fn live(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.issued_at) < FOCUS_SETTLE
    }

    /// A report that stands for the target while it settles: nothing at all
    /// (Accessibility could not say, #398) or a window this request left behind.
    fn explains(&self, reported: Option<&PhysicalWindowId>) -> bool {
        reported.is_none() || self.stale.iter().any(|entry| entry.as_ref() == reported)
    }
}

impl FocusTracker {
    pub(super) fn with_fullscreen_restore(workspaces: HashSet<WorkspaceId>) -> Self {
        Self {
            fullscreen_restore: workspaces,
            ..Self::default()
        }
    }

    #[cfg(test)]
    pub(super) const fn phase(&self) -> &FocusPhase {
        &self.phase
    }

    /// Makes the request in flight look by older (tests only).
    #[cfg(test)]
    pub(super) fn age_request(&mut self, by: Duration) {
        if let FocusPhase::InFlight { request, .. } = &mut self.phase {
            request.issued_at -= by;
        }
    }

    #[cfg(test)]
    pub(super) fn guarded_app(&self) -> Option<&AppInstanceId> {
        self.guard().and_then(|guard| guard.app.as_ref())
    }

    #[cfg(test)]
    pub(super) fn restore_fullscreen_on(&mut self, workspace: WorkspaceId) {
        self.fullscreen_restore.insert(workspace);
    }

    #[cfg(test)]
    pub(super) fn restores_any_fullscreen(&self) -> bool {
        !self.fullscreen_restore.is_empty()
    }

    /// Mouse follows focus just moved the pointer to `point`.
    pub(super) const fn note_warp(&mut self, point: (i32, i32)) {
        self.last_warp = Some(point);
    }

    /// Whether the pointer resting at `point` is where Fractile last warped it.
    pub(super) fn warped_to(&self, point: (i32, i32)) -> bool {
        self.last_warp == Some(point)
    }

    /// The request still settling at now.
    pub(super) fn in_flight(&self, now: Instant) -> Option<&FocusRequest> {
        match &self.phase {
            FocusPhase::InFlight { request, .. } if request.live(now) => Some(request),
            _ => None,
        }
    }

    /// The window focus effectively is: the in-flight target while
    /// Accessibility gives a report the request explains, else the report.
    pub(super) fn effective(
        &self,
        now: Instant,
        reported: Option<PhysicalWindowId>,
    ) -> Option<PhysicalWindowId> {
        match self.in_flight(now) {
            Some(request) if request.explains(reported.as_ref()) => Some(request.target.clone()),
            _ => reported,
        }
    }

    /// Whether the report is one the in-flight request explains, so it says
    /// nothing about where the user put focus.
    pub(super) fn explains(&self, now: Instant, reported: Option<&PhysicalWindowId>) -> bool {
        self.in_flight(now)
            .is_some_and(|request| request.explains(reported))
    }

    /// [Self::effective], then settles the request: it ends once
    /// Accessibility reports its target, a focus it does not explain, or it
    /// times out. A departure guard it carried stays.
    pub(super) fn resolve(
        &mut self,
        now: Instant,
        reported: Option<PhysicalWindowId>,
    ) -> Option<PhysicalWindowId> {
        let effective = self.effective(now, reported.clone());
        if let FocusPhase::InFlight { request, .. } = &self.phase
            && !(request.live(now) && request.explains(reported.as_ref()))
        {
            self.end_request();
        }
        effective
    }

    /// Records that Fractile focused the target, superseding any request still
    /// settling: its target and stale reports become stale for this one, along
    /// with what Accessibility may keep reporting (left).
    pub(super) fn request(
        &mut self,
        now: Instant,
        target: PhysicalWindowId,
        left: impl IntoIterator<Item = Option<PhysicalWindowId>>,
    ) {
        let mut stale = Vec::new();
        let departure = match std::mem::take(&mut self.phase) {
            FocusPhase::Synced => None,
            FocusPhase::GuardedDeparture { guard, .. } => Some(guard),
            FocusPhase::InFlight { request, departure } => {
                if request.live(now) {
                    stale = request.stale;
                    stale.push(Some(request.target));
                }
                departure
            }
        };
        stale.extend(left);
        // Fractile focused a window, which ends any choice of an empty workspace.
        self.empty_choice = None;
        let mut unique = Vec::with_capacity(stale.len());
        for entry in stale {
            if entry.as_ref() != Some(&target) && !unique.contains(&entry) {
                unique.push(entry);
            }
        }
        self.phase = FocusPhase::InFlight {
            request: FocusRequest {
                target,
                stale: unique,
                issued_at: now,
            },
            departure,
        };
    }

    /// Forgets the request, keeping any departure guard it carried.
    pub(super) fn end_request(&mut self) {
        self.phase = match std::mem::take(&mut self.phase) {
            FocusPhase::InFlight {
                departure: Some(guard),
                ..
            } => FocusPhase::GuardedDeparture {
                guard,
                failed_follow_at: None,
            },
            FocusPhase::InFlight { .. } => FocusPhase::Synced,
            other => other,
        };
    }

    pub(super) fn guard(&self) -> Option<&DepartureGuard> {
        match &self.phase {
            FocusPhase::Synced => None,
            FocusPhase::InFlight { departure, .. } => departure.as_ref(),
            FocusPhase::GuardedDeparture { guard, .. } => Some(guard),
        }
    }

    /// The window a departure guard holds back, if any.
    pub(super) fn guarded_window(&self) -> Option<&PhysicalWindowId> {
        self.guard().and_then(|guard| guard.window.as_ref())
    }

    /// Guards the focus left behind, keeping any request in flight.
    pub(super) fn guard_departure(&mut self, guard: DepartureGuard) {
        match &mut self.phase {
            FocusPhase::InFlight { departure, .. } => *departure = Some(guard),
            phase => {
                *phase = FocusPhase::GuardedDeparture {
                    guard,
                    failed_follow_at: None,
                };
            }
        }
    }

    /// Drops the departure guard: Accessibility reported a new focus.
    pub(super) fn clear_guard(&mut self) {
        match &mut self.phase {
            FocusPhase::InFlight { departure, .. } => *departure = None,
            phase => *phase = FocusPhase::Synced,
        }
    }

    /// Whether this report is still the focus left behind (or one a request in
    /// flight explains), so focus follow must not chase it. When the focused
    /// window is missing or lags the frontmost app, the app is the evidence;
    /// with neither reported nothing has changed.
    pub(super) fn holds_back(
        &self,
        now: Instant,
        focused: Option<&PhysicalWindowId>,
        frontmost: Option<&AppInstanceId>,
        focused_is_frontmost: bool,
    ) -> bool {
        if focused.is_some() && self.explains(now, focused) {
            return true;
        }
        let guard = self.guard().cloned().unwrap_or_default();
        if focused_is_frontmost {
            focused == guard.window.as_ref()
        } else if let Some(frontmost) = frontmost {
            guard.app.as_ref() == Some(frontmost)
        } else {
            true
        }
    }

    /// A focus follow failed; a Space slide may still explain it.
    pub(super) fn follow_failed(&mut self, started: Instant) {
        let guard = self.guard().cloned().unwrap_or_default();
        self.phase = FocusPhase::GuardedDeparture {
            guard,
            failed_follow_at: Some(started),
        };
    }

    pub(super) const fn failed_follow_at(&self) -> Option<Instant> {
        match &self.phase {
            FocusPhase::GuardedDeparture {
                failed_follow_at, ..
            } => *failed_follow_at,
            _ => None,
        }
    }

    pub(super) fn forget_failed_follow(&mut self) {
        if let FocusPhase::GuardedDeparture {
            failed_follow_at, ..
        } = &mut self.phase
        {
            *failed_follow_at = None;
        }
    }

    pub(super) const fn history(&self) -> WorkspaceHistory {
        self.history
    }

    /// Remembers that the workspace took focus. The first record after a launch
    /// seeds the previous workspace from seed, so Option+Tab works at once.
    pub(super) fn record_workspace(
        &mut self,
        workspace: WorkspaceId,
        seed: impl FnOnce() -> Option<WorkspaceId>,
    ) {
        if self
            .empty_choice
            .as_ref()
            .is_some_and(|choice| choice.workspace != workspace)
        {
            self.empty_choice = None;
        }
        if self.history.current == Some(workspace) {
            return;
        }
        self.history.previous = match self.history.current {
            Some(current) => Some(current),
            None => seed().filter(|previous| *previous != workspace),
        };
        self.history.current = Some(workspace);
    }

    pub(super) fn restores_fullscreen(&self, workspace: WorkspaceId) -> bool {
        self.fullscreen_restore.contains(&workspace)
    }

    pub(super) fn settle_fullscreen(&mut self, workspace: WorkspaceId) {
        self.fullscreen_restore.remove(&workspace);
    }
}

#[cfg(test)]
mod tests;
