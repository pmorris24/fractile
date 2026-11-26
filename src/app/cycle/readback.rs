use super::*;

/// How long a freshly applied target stays armed for bounce readback.
/// Electron apps accept `AXSize` writes synchronously and emit `AXResized`
/// before their async resize clamp (~40–100 ms later) snaps the window back;
/// wiping `applied` on an early matching tick would miss that snap-back and
/// restart an infinite resize-flicker loop.
pub(in crate::app) const ASYNC_BOUNCE_SETTLE: Duration = Duration::from_millis(500);

/// Consecutive bounced or rejected applications for the same target frame
/// before Fractile stops retrying that target (even when rejected frames vary
/// each cycle), preventing infinite resize-fight flicker loops.
pub(in crate::app) const APPLY_ATTEMPT_LIMIT: u8 = 3;

/// Points within which a clamped frame's edge meets a display's edge.
const DISPLAY_EDGE_TOLERANCE: i32 = 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::app) struct AppliedFrame {
    pub(in crate::app) target: crate::Rect,
    pub(in crate::app) at: Instant,
}

impl AppliedFrame {
    pub(super) fn is_settling(&self, target: crate::Rect, now: Instant) -> bool {
        self.target == target && now.saturating_duration_since(self.at) < ASYNC_BOUNCE_SETTLE
    }
}

impl<B: WorkspaceBackend> WorkspaceRuntime<B> {
    pub(in crate::app) fn record_results(
        &mut self,
        workspace: WorkspaceId,
        results: &[macos::ApplyResult],
    ) {
        let now = self.backend.now();
        let displays = self.displays.known_outputs();
        let constrained = &mut self.constrained[workspace.index()];
        let applied = &mut self.applied[workspace.index()];
        let attempts = &mut self.apply_attempts[workspace.index()];
        for result in results {
            match (result.outcome, result.actual) {
                (macos::ApplyOutcome::Constrained, Some(actual)) => {
                    applied.remove(&result.window);
                    if !clamped_to_departed_display(displays, result.target, actual) {
                        constrained.insert(result.window.clone(), (result.target, actual));
                    }
                    record_apply_attempt(attempts, workspace, &result.window, result.target);
                }
                (macos::ApplyOutcome::Applied, _) => {
                    constrained.remove(&result.window);
                    if attempts
                        .get(&result.window)
                        .is_some_and(|(target, _)| *target != result.target)
                    {
                        attempts.remove(&result.window);
                    }
                    applied.insert(
                        result.window.clone(),
                        AppliedFrame {
                            target: result.target,
                            at: now,
                        },
                    );
                }
                (macos::ApplyOutcome::Gone, _) => {
                    applied.remove(&result.window);
                    constrained.remove(&result.window);
                    attempts.remove(&result.window);
                }
                (macos::ApplyOutcome::Deferred, _) => {
                    applied.remove(&result.window);
                }
                _ => {
                    applied.remove(&result.window);
                    record_apply_attempt(attempts, workspace, &result.window, result.target);
                }
            }
        }
    }
}

/// macOS fits a size write to the display a window is on when the write
/// lands: a window moved onto a taller display keeps the bottom edge of the
/// display it left until the app sees its new screen (#428). That frame is no
/// refusal by the app, so it is not frozen as one; the next cycle writes the
/// frame again, within the usual attempt limit.
fn clamped_to_departed_display(
    displays: Option<&macos::DisplayOutputs>,
    target: crate::Rect,
    actual: crate::Rect,
) -> bool {
    let Some(displays) = displays else {
        return false;
    };
    if !actual.position_approximately_equals(target, DISPLAY_EDGE_TOLERANCE)
        || actual.height + DISPLAY_EDGE_TOLERANCE >= target.height
    {
        return false;
    }
    let bottom = actual.y + actual.height;
    displays.frames().any(|display| {
        !display.contains_center(target)
            && (display.y + display.height - bottom).abs() <= DISPLAY_EDGE_TOLERANCE
    })
}

pub(in crate::app) fn record_apply_attempt(
    attempts: &mut HashMap<crate::PhysicalWindowId, (crate::Rect, u8)>,
    workspace: WorkspaceId,
    window: &crate::PhysicalWindowId,
    target: crate::Rect,
) {
    let entry = attempts.entry(window.clone()).or_insert((target, 0));
    if entry.0 != target {
        *entry = (target, 0);
    }
    let previous = entry.1;
    entry.1 = entry.1.saturating_add(1);
    if previous < APPLY_ATTEMPT_LIMIT && entry.1 >= APPLY_ATTEMPT_LIMIT {
        log_event!(
            "workspace {workspace}: freezing {window:?} after {} bounced/rejected applications for {target:?}",
            APPLY_ATTEMPT_LIMIT
        );
    }
}
