//! Windows an application reopened at a Fractile parking spot.
//!
//! Quitting an app while some of its windows are parked lets macOS save those parked frames, so
//! the relaunched app reopens them one pixel inside a display corner. Their centers are off
//! screen, so no observation lists them, and their physical identity is new, so no recovery
//! frame covers them. Revealing every such window (not only the focused one) lets observation,
//! recall and reconciliation adopt it like any other window, on the workspace it remembers.

use super::*;

/// Rescued windows on one display cascade by this much, so none hides another.
const RESCUE_CASCADE: i32 = 24;

/// First wait before retrying a window recovery could not move; it doubles per failure.
pub(super) const RESCUE_RETRY_FIRST: Duration = Duration::from_secs(2);
/// Longest wait between retries of a window recovery could not move.
pub(super) const RESCUE_RETRY_CAP: Duration = Duration::from_secs(32);

/// Windows recovery could not move, by window ID. A failed write can wait out the
/// Accessibility timeout, so retrying every cycle would slow every retile; a window that only
/// failed because its app was still launching must still be rescued, so retries back off
/// rather than stop.
#[derive(Default)]
pub(super) struct UnmovableStale {
    /// Display frames when the failures were recorded: a display change retries every window.
    displays: Vec<crate::Rect>,
    windows: HashMap<u32, RescueFailure>,
}

struct RescueFailure {
    /// Window-server frame at the failure: a window that moves is retried at once.
    frame: crate::Rect,
    failures: u32,
    retry_at: Instant,
}

impl UnmovableStale {
    /// Forget failures for windows that closed or moved, and every failure when the displays
    /// changed.
    fn refresh(&mut self, displays: &[crate::Rect], server_frames: &HashMap<u32, crate::Rect>) {
        if self.displays != displays {
            self.displays = displays.to_vec();
            self.windows.clear();
        }
        self.windows
            .retain(|id, failure| server_frames.get(id) == Some(&failure.frame));
    }

    fn waiting(&self, id: u32, now: Instant) -> bool {
        self.windows
            .get(&id)
            .is_some_and(|failure| now < failure.retry_at)
    }

    fn failed(&mut self, id: u32, frame: crate::Rect, now: Instant) {
        let failures = self.windows.get(&id).map_or(0, |failure| failure.failures) + 1;
        let delay = RESCUE_RETRY_FIRST
            .saturating_mul(1 << (failures - 1).min(8))
            .min(RESCUE_RETRY_CAP);
        self.windows.insert(
            id,
            RescueFailure {
                frame,
                failures,
                retry_at: now + delay,
            },
        );
    }

    fn succeeded(&mut self, id: u32) {
        self.windows.remove(&id);
    }
}

impl<B: WorkspaceBackend> WorkspaceRuntime<B> {
    /// Reveal every window sitting exactly at a display's parking spot that Fractile neither
    /// parked nor owns.
    pub(super) fn recover_stale_parked_windows(
        &mut self,
        outputs: &macos::DisplayOutputs,
        server_frames: &HashMap<u32, crate::Rect>,
    ) {
        let displays = outputs.frames().collect::<Vec<_>>();
        let screens = outputs.screens();
        let parked_ids: HashSet<u32> = self.parked.keys().map(|window| window.window_id).collect();
        let now = self.backend.now();
        self.unmovable_stale.refresh(&displays, server_frames);
        // Almost every window is on screen or parked by Fractile: both are skipped before any
        // geometry or window-server lookup. Menu-bar strips (more than the parking strip, give
        // or take its one-pixel tolerance, shows on a screen), one-pixel and empty windows are
        // never at a parking spot.
        let mut stale: Vec<(u32, crate::Rect, crate::Rect)> = server_frames
            .iter()
            .filter(|(id, frame)| {
                !parked_ids.contains(id)
                    && !self.unmovable_stale.waiting(**id, now)
                    && frame.width > 1
                    && frame.height > 1
                    && !displays.iter().any(|d| d.contains_center(**frame))
                    && !crate::geometry::shows_wider_than(**frame, &screens, 2)
            })
            .filter_map(|(&id, &frame)| {
                outputs.displays().iter().find_map(|display| {
                    let parked = crate::geometry::parked_frame_clear_of(
                        display.frame,
                        &displays,
                        &screens,
                        frame,
                    );
                    crate::geometry::parked_at(frame, parked, 1).then_some((
                        id,
                        frame,
                        display.frame,
                    ))
                })
            })
            .collect();
        if stale.is_empty() {
            return;
        }
        // A window Fractile owns but did not park (an app holding it off screen itself) is never
        // rescued; leave it out before asking the window server, so it costs nothing per cycle.
        let owned: HashSet<u32> = WorkspaceId::all()
            .flat_map(|workspace| self.workspaces.windows(workspace))
            .map(|window| window.window_id)
            .collect();
        stale.retain(|(id, ..)| !owned.contains(id));
        if stale.is_empty() {
            return;
        }
        // Rescue in creation order, so the cascade matches the order recall assigns.
        stale.sort_by_key(|(id, ..)| *id);
        let ids = stale.iter().map(|(id, ..)| *id).collect();
        let owners = self.backend.window_owners(&ids);
        let mut cascade: HashMap<(i32, i32), i32> = HashMap::new();
        for (id, frame, output) in stale {
            let Some(app) = owners.get(&id) else {
                continue;
            };
            let window = crate::PhysicalWindowId {
                app: app.clone(),
                window_id: id,
            };
            if self.workspaces.workspace_of_any(&window).is_some()
                || self.workspaces.scratchpad().contains(&window)
                || self
                    .bundle_disposition(&window, frame)
                    .is_some_and(crate::config::DispositionPatch::is_ignored)
            {
                continue;
            }
            let offset = cascade.entry((output.x, output.y)).or_insert(0);
            let target = crate::Rect {
                x: output.x + *offset,
                y: output.y + *offset,
                ..frame
            };
            *offset += RESCUE_CASCADE;
            log_event!(
                "recover stale parked window: {:?} from {:?} to {:?}",
                window,
                frame,
                target
            );
            let outcome = self.backend.set_position(&window, target);
            if matches!(outcome, macos::ApplyOutcome::Applied) {
                self.unmovable_stale.succeeded(id);
            } else {
                log_event!(
                    "stale parked recovery incomplete for {:?}: {:?}",
                    window,
                    outcome
                );
                self.unmovable_stale.failed(id, frame, now);
            }
        }
    }
}
