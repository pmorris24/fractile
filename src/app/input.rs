//! Pure input state: previews may be skipped, but completed gestures and key
//! commands retain their arrival order until dispatched or explicitly cleared.
use std::collections::VecDeque;

use super::{FinalGesture, WorkerAction};
use crate::{PhysicalWindowId, Rect, swipe::Fingers};

const MAX_PENDING_INPUTS: usize = 64;

/// Axis-aligned bounding box of every pointer sample seen during a drag.
/// A rectangle contains every sampled point iff it contains this box, so a
/// display edge band has been left at least once iff `!entirely_inside(band)`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct PointerBounds {
    pub(super) min_x: i32,
    pub(super) max_x: i32,
    pub(super) min_y: i32,
    pub(super) max_y: i32,
}

impl PointerBounds {
    pub(super) const fn point((x, y): (i32, i32)) -> Self {
        Self {
            min_x: x,
            max_x: x,
            min_y: y,
            max_y: y,
        }
    }

    pub(super) fn extend(&mut self, (x, y): (i32, i32)) {
        self.min_x = self.min_x.min(x);
        self.max_x = self.max_x.max(x);
        self.min_y = self.min_y.min(y);
        self.max_y = self.max_y.max(y);
    }

    pub(super) fn merge(&mut self, other: Self) {
        self.min_x = self.min_x.min(other.min_x);
        self.max_x = self.max_x.max(other.max_x);
        self.min_y = self.min_y.min(other.min_y);
        self.max_y = self.max_y.max(other.max_y);
    }

    pub(super) const fn entirely_inside(self, rect: Rect) -> bool {
        self.min_x >= rect.x
            && self.max_x < rect.x + rect.width
            && self.min_y >= rect.y
            && self.max_y < rect.y + rect.height
    }
}

pub(super) fn is_window_drag(start: Rect, current: Rect) -> bool {
    let moved = (start.x - current.x).abs().max((start.y - current.y).abs()) >= 6;
    let resized = (start.width - current.width)
        .abs()
        .max((start.height - current.height).abs())
        > 2;
    moved && !resized
}

pub(super) fn is_window_resize(start: Rect, current: Rect) -> bool {
    (start.width - current.width)
        .abs()
        .max((start.height - current.height).abs())
        >= 6
}

/// The pointer carried the window: on one axis both of its edges moved the way
/// the pointer went, even if the app changed its size as it landed (#429). A
/// border drag leaves the opposite edge where it was, and a window Fractile
/// moved (parking) did not follow the pointer.
fn carried(start: Rect, current: Rect, travel: (i32, i32)) -> bool {
    let along = |leading: i32, trailing: i32, pointer: i32| {
        [leading, trailing, pointer]
            .iter()
            .all(|delta| delta.abs() >= 6 && delta.signum() == pointer.signum())
    };
    along(
        current.x - start.x,
        current.x + current.width - (start.x + start.width),
        travel.0,
    ) || along(
        current.y - start.y,
        current.y + current.height - (start.y + start.height),
        travel.1,
    )
}

/// Whether a three-finger workspace swipe must be ignored because a pointer
/// button is down or a window drag/resize gesture is in progress (such as
/// macOS Accessibility "Three Finger Drag").
pub(super) fn should_ignore_swipe(
    mouse_down: bool,
    mouse_was_down: bool,
    gesture_active: bool,
) -> bool {
    mouse_down || mouse_was_down || gesture_active
}

/// Whether a worker action from the main thread enters the input queue. A swipe
/// enters only with the configured finger count (`Config::swipe_fingers`; the
/// other count is macOS's gesture and must not wake the worker) and while
/// [`should_ignore_swipe`] lets it. `pointer` reads `(mouse_down,
/// mouse_was_down, gesture_active)` and only runs for such a swipe. Every other
/// action enters.
pub(super) fn admits_action(
    action: &WorkerAction,
    swipe_fingers: Fingers,
    pointer: impl FnOnce() -> (bool, bool, bool),
) -> bool {
    let WorkerAction::Swipe(_, fingers, _) = action else {
        return true;
    };
    if *fingers != swipe_fingers {
        return false;
    }
    let (mouse_down, mouse_was_down, gesture_active) = pointer();
    !should_ignore_swipe(mouse_down, mouse_was_down, gesture_active)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum QueuedInput {
    Action(WorkerAction),
    Gesture(FinalGesture),
}

#[derive(Default)]
pub(super) struct InputQueue(VecDeque<QueuedInput>);

impl InputQueue {
    pub(super) fn push(&mut self, input: QueuedInput) -> Result<(), &'static str> {
        if self.0.len() >= MAX_PENDING_INPUTS {
            return Err("Input queue full; command was not queued");
        }
        self.0.push_back(input);
        Ok(())
    }

    pub(super) fn pop(&mut self) -> Option<QueuedInput> {
        self.0.pop_front()
    }

    pub(super) fn front(&self) -> Option<&QueuedInput> {
        self.0.front()
    }

    pub(super) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub(super) fn clear(&mut self) {
        self.0.clear();
    }
}

#[derive(Default)]
pub(super) struct GestureTracker {
    active: Option<Gesture>,
}

/// One pointer gesture. macOS lets the pointer grab a window's edge from just
/// outside the window, so mouse-down can't tell which window an edge drag is
/// about: every window whose resize border reaches the pointer is a candidate.
/// The first one to move or change size becomes the gesture's window.
struct Gesture {
    candidates: Vec<Candidate>,
    /// Once set, the other candidates are ignored: previews resize them to
    /// follow this window, and that must not take the gesture over.
    chosen: Option<usize>,
    last_preview: Option<FinalGesture>,
    bounds: Option<PointerBounds>,
    start_point: Option<(i32, i32)>,
}

struct Candidate {
    window: PhysicalWindowId,
    start: Rect,
    dragged: bool,
    resized: bool,
}

impl GestureTracker {
    /// `candidates` are front to back, each with its frame at mouse-down.
    #[cfg(test)]
    pub(super) fn begin(&mut self, candidates: Vec<(PhysicalWindowId, Rect)>) {
        self.begin_at(candidates, None);
    }

    /// `candidates` are front to back, each with its frame at mouse-down, and
    /// `start_point` is the pointer location at mouse-down when known.
    pub(super) fn begin_at(
        &mut self,
        candidates: Vec<(PhysicalWindowId, Rect)>,
        start_point: Option<(i32, i32)>,
    ) {
        self.active = (!candidates.is_empty()).then(|| Gesture {
            candidates: candidates
                .into_iter()
                .map(|(window, start)| Candidate {
                    window,
                    start,
                    dragged: false,
                    resized: false,
                })
                .collect(),
            chosen: None,
            last_preview: None,
            bounds: start_point.map(PointerBounds::point),
            start_point,
        });
    }

    /// Record a pointer sample while the gesture is active, even when the
    /// worker is too busy to accept a preview tick.
    pub(super) fn observe_point(&mut self, point: (i32, i32)) {
        if let Some(gesture) = self.active.as_mut() {
            match &mut gesture.bounds {
                Some(bounds) => bounds.extend(point),
                None => gesture.bounds = Some(PointerBounds::point(point)),
            }
        }
    }

    pub(super) fn is_active(&self) -> bool {
        self.active.is_some()
    }

    pub(super) fn clear(&mut self) {
        self.active = None;
    }

    pub(super) fn preview(
        &mut self,
        frame_of: impl Fn(&PhysicalWindowId) -> Option<Rect>,
        point: Option<(i32, i32)>,
    ) -> Option<FinalGesture> {
        let gesture = self.active.as_mut()?;
        let next = gesture.classify(frame_of, point)?;
        if gesture.last_preview.as_ref() == Some(&next) {
            return None;
        }
        gesture.last_preview = Some(next.clone());
        Some(next)
    }

    pub(super) fn finish(
        &mut self,
        frame_of: impl Fn(&PhysicalWindowId) -> Option<Rect>,
        point: Option<(i32, i32)>,
    ) -> Option<FinalGesture> {
        let mut gesture = self.active.take()?;
        // The final sample, not the existence of a preview tick, establishes
        // intent. Missing final evidence never replays an old preview frame.
        gesture.classify(frame_of, point)
    }
}

impl Gesture {
    fn classify(
        &mut self,
        frame_of: impl Fn(&PhysicalWindowId) -> Option<Rect>,
        point: Option<(i32, i32)>,
    ) -> Option<FinalGesture> {
        if let (Some(bounds), Some(point)) = (&mut self.bounds, point) {
            bounds.extend(point);
        }
        let bounds = self.bounds;
        let travel = self
            .start_point
            .zip(point)
            .map(|(start, point)| (point.0 - start.0, point.1 - start.1));
        if let Some(index) = self.chosen {
            let candidate = &mut self.candidates[index];
            return candidate.classify(frame_of(&candidate.window)?, point, bounds, travel);
        }
        // Until a preview is sent nothing has moved the candidates but the
        // user, so the first one that changed is the one being dragged.
        for (index, candidate) in self.candidates.iter_mut().enumerate() {
            let Some(frame) = frame_of(&candidate.window) else {
                continue;
            };
            if let Some(gesture) = candidate.classify(frame, point, bounds, travel) {
                self.chosen = Some(index);
                return Some(gesture);
            }
        }
        None
    }
}

impl Candidate {
    fn classify(
        &mut self,
        frame: Rect,
        point: Option<(i32, i32)>,
        bounds: Option<PointerBounds>,
        travel: Option<(i32, i32)>,
    ) -> Option<FinalGesture> {
        // A window carried by its title bar stays a drag: an app that changes
        // its own size as it lands is not a border resize (#429).
        let carried_along = travel.is_some_and(|travel| carried(self.start, frame, travel));
        self.resized |= !self.dragged && !carried_along && is_window_resize(self.start, frame);
        self.dragged |= carried_along || is_window_drag(self.start, frame);
        if self.resized {
            Some(FinalGesture::Resize(self.window.clone(), frame))
        } else if self.dragged {
            point.map(|point| FinalGesture::Drop {
                window: self.window.clone(),
                point,
                bounds,
            })
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AppInstanceId, Direction, ResizeDirection, WorkspaceId};

    fn window() -> PhysicalWindowId {
        PhysicalWindowId {
            app: AppInstanceId {
                pid: 1,
                launched_at: 1,
            },
            window_id: 10,
        }
    }

    fn start() -> Rect {
        Rect {
            x: 50,
            y: 20,
            width: 400,
            height: 300,
        }
    }

    /// Window-server readback where the gesture's only window is at `rect`.
    fn frame(rect: Rect) -> impl Fn(&PhysicalWindowId) -> Option<Rect> {
        move |_| Some(rect)
    }

    fn neighbor() -> PhysicalWindowId {
        PhysicalWindowId {
            window_id: 20,
            ..window()
        }
    }

    /// The window to the right of `start()`, across an 8-point gap.
    fn neighbor_start() -> Rect {
        Rect { x: 458, ..start() }
    }

    fn frames(left: Rect, right: Rect) -> impl Fn(&PhysicalWindowId) -> Option<Rect> {
        move |candidate| Some(if *candidate == window() { left } else { right })
    }

    #[test]
    fn edge_grabbed_from_outside_the_window_resizes_it_not_the_window_in_front() {
        // The pointer is in the gap or over the neighbor, which sorts first.
        let mut tracker = GestureTracker::default();
        tracker.begin(vec![(neighbor(), neighbor_start()), (window(), start())]);
        let wider = Rect {
            width: 450,
            ..start()
        };
        assert_eq!(
            tracker.preview(frames(wider, neighbor_start()), Some((500, 100))),
            Some(FinalGesture::Resize(window(), wider))
        );
        let end = Rect {
            width: 520,
            ..start()
        };
        assert_eq!(
            tracker.finish(frames(end, neighbor_start()), Some((570, 100))),
            Some(FinalGesture::Resize(window(), end))
        );
    }

    #[test]
    fn neighbors_resized_by_the_preview_do_not_take_over_the_gesture() {
        let mut tracker = GestureTracker::default();
        tracker.begin(vec![(neighbor(), neighbor_start()), (window(), start())]);
        let wider = Rect {
            width: 450,
            ..start()
        };
        assert!(
            tracker
                .preview(frames(wider, neighbor_start()), None)
                .is_some()
        );
        // The preview narrowed the neighbor to make room; the user then
        // dragged the edge back to where it started.
        let narrowed = Rect {
            x: 508,
            width: 350,
            ..neighbor_start()
        };
        assert_eq!(
            tracker.finish(frames(start(), narrowed), None),
            Some(FinalGesture::Resize(window(), start()))
        );
    }

    #[test]
    fn edge_drag_seen_only_at_mouse_up_still_finds_its_window() {
        let mut tracker = GestureTracker::default();
        tracker.begin(vec![(neighbor(), neighbor_start()), (window(), start())]);
        let narrower = Rect {
            x: 500,
            width: 308,
            ..neighbor_start()
        };
        assert_eq!(
            tracker.finish(frames(start(), narrower), None),
            Some(FinalGesture::Resize(neighbor(), narrower))
        );
    }

    #[test]
    fn busy_input_preserves_relative_commands_and_mixed_order() {
        let inputs = [
            WorkerAction::Resize(ResizeDirection::Grow),
            WorkerAction::Resize(ResizeDirection::Grow),
            WorkerAction::Resize(ResizeDirection::Grow),
            WorkerAction::SwitchWorkspace(WorkspaceId::new(2).unwrap()),
            WorkerAction::SwitchWorkspace(WorkspaceId::new(3).unwrap()),
            WorkerAction::ToggleLayout,
            WorkerAction::ToggleLayout,
            WorkerAction::Focus(Direction::Right),
            WorkerAction::CycleFocus,
        ]
        .map(QueuedInput::Action);
        let mut queue = InputQueue::default();
        for input in &inputs {
            queue.push(input.clone()).unwrap();
        }
        for input in inputs {
            assert_eq!(queue.pop(), Some(input));
        }
        assert!(queue.is_empty());
    }

    #[test]
    fn queue_overflow_is_explicit_and_never_overwrites_accepted_input() {
        let mut queue = InputQueue::default();
        for _ in 0..MAX_PENDING_INPUTS {
            queue
                .push(QueuedInput::Action(WorkerAction::CycleFocus))
                .unwrap();
        }
        assert!(
            queue
                .push(QueuedInput::Action(WorkerAction::ToggleLayout))
                .is_err()
        );
        for _ in 0..MAX_PENDING_INPUTS {
            assert_eq!(
                queue.pop(),
                Some(QueuedInput::Action(WorkerAction::CycleFocus))
            );
        }
        assert!(queue.is_empty());
    }

    #[test]
    fn failed_dispatch_retains_the_head_until_it_can_be_sent() {
        let mut queue = InputQueue::default();
        let first = QueuedInput::Action(WorkerAction::CycleFocus);
        queue.push(first.clone()).unwrap();
        // No displays or an unavailable worker must not consume the intent.
        assert_eq!(queue.front(), Some(&first));
        queue
            .push(QueuedInput::Action(WorkerAction::Balance))
            .unwrap();
        assert_eq!(queue.front(), Some(&first));
        assert_eq!(queue.pop(), Some(first));
        assert_eq!(
            queue.pop(),
            Some(QueuedInput::Action(WorkerAction::Balance))
        );
    }

    #[test]
    fn entire_gesture_during_busy_work_survives_without_a_preview_tick() {
        for end in [
            Rect { x: 1500, ..start() },
            Rect {
                width: 550,
                ..start()
            },
        ] {
            let mut tracker = GestureTracker::default();
            let mut queue = InputQueue::default();
            queue
                .push(QueuedInput::Action(WorkerAction::CycleFocus))
                .unwrap();
            tracker.begin(vec![(window(), start())]);
            // Worker stays busy from mouse-down through mouse-up: no preview.
            let completed = tracker.finish(frame(end), Some((1700, 100))).unwrap();
            let expected = if end.width != start().width {
                FinalGesture::Resize(window(), end)
            } else {
                FinalGesture::drop(window(), (1700, 100))
            };
            assert_eq!(completed, expected);
            queue.push(QueuedInput::Gesture(completed)).unwrap();
            queue
                .push(QueuedInput::Action(WorkerAction::Balance))
                .unwrap();
            assert_eq!(
                queue.pop(),
                Some(QueuedInput::Action(WorkerAction::CycleFocus))
            );
            assert_eq!(queue.pop(), Some(QueuedInput::Gesture(expected)));
            assert_eq!(
                queue.pop(),
                Some(QueuedInput::Action(WorkerAction::Balance))
            );
            assert!(!tracker.is_active());
        }
    }

    #[test]
    fn busy_preview_worker_queues_final_cross_display_drop() {
        let mut tracker = GestureTracker::default();
        tracker.begin(vec![(window(), start())]);
        assert!(
            tracker
                .preview(frame(Rect { x: 500, ..start() }), Some((700, 400)))
                .is_some()
        );
        let drop = tracker
            .finish(frame(Rect { x: 1500, ..start() }), Some((1700, 400)))
            .unwrap();
        let mut queue = InputQueue::default();
        queue.push(QueuedInput::Gesture(drop)).unwrap();
        assert_eq!(
            queue.pop(),
            Some(QueuedInput::Gesture(FinalGesture::drop(
                window(),
                (1700, 400)
            )))
        );
    }

    #[test]
    fn mouse_up_uses_final_resize_even_when_preview_was_already_sent() {
        let mut tracker = GestureTracker::default();
        tracker.begin(vec![(window(), start())]);
        let preview = Rect {
            width: 450,
            ..start()
        };
        assert!(tracker.preview(frame(preview), None).is_some());
        assert!(tracker.preview(frame(preview), None).is_none());
        let end = Rect {
            width: 600,
            ..start()
        };
        assert_eq!(
            tracker.finish(frame(end), None),
            Some(FinalGesture::Resize(window(), end))
        );
    }

    #[test]
    fn clicks_missing_readback_and_pause_do_not_replay_gestures() {
        let mut tracker = GestureTracker::default();
        tracker.begin(vec![(window(), start())]);
        assert_eq!(tracker.finish(frame(start()), Some((60, 40))), None);
        tracker.begin(vec![(window(), start())]);
        assert!(
            tracker
                .preview(
                    frame(Rect {
                        width: 600,
                        ..start()
                    }),
                    None
                )
                .is_some()
        );
        assert_eq!(tracker.finish(|_| None, None), None);
        tracker.begin(vec![(window(), start())]);
        tracker.clear();
        assert_eq!(
            tracker.finish(frame(Rect { x: 1500, ..start() }), Some((1700, 100))),
            None
        );
        let mut queue = InputQueue::default();
        queue
            .push(QueuedInput::Action(WorkerAction::CycleFocus))
            .unwrap();
        queue.clear();
        assert_eq!(queue.pop(), None);
    }

    #[test]
    fn three_finger_swipes_are_ignored_while_a_pointer_button_or_drag_is_active() {
        let mut tracker = GestureTracker::default();
        // Normal three-finger swipe: no mouse button down, no active gesture.
        assert!(!should_ignore_swipe(false, false, tracker.is_active()));

        // Left button pressed (e.g. macOS Three Finger Drag starts dragging a window).
        assert!(should_ignore_swipe(true, false, tracker.is_active()));

        // Active window drag tracked in GestureTracker suppresses swipes even
        // on the boundary tick where mouse_down just transitioned.
        tracker.begin(vec![(window(), start())]);
        assert!(should_ignore_swipe(true, true, tracker.is_active()));
        assert!(should_ignore_swipe(false, true, tracker.is_active()));
        assert!(should_ignore_swipe(false, false, tracker.is_active()));

        // Finishing the drag clears GestureTracker so subsequent normal
        // three-finger swipes are accepted again.
        let moved = Rect { x: 200, ..start() };
        assert_eq!(
            tracker.finish(frame(moved), Some((260, 40))),
            Some(FinalGesture::drop(window(), (260, 40)))
        );
        assert!(!should_ignore_swipe(false, false, tracker.is_active()));
    }

    #[test]
    fn suppressed_swipes_never_enter_the_input_queue_while_normal_swipes_do() {
        let mut queue = InputQueue::default();
        let swipe = WorkerAction::Swipe(
            crate::swipe::Swipe::Right,
            crate::swipe::Fingers::Three,
            (200, 100),
        );

        // While dragging with three fingers (mouse_down = true), a swipe is dropped.
        if !should_ignore_swipe(true, true, true) {
            queue.push(QueuedInput::Action(swipe.clone())).unwrap();
        }
        assert!(queue.is_empty());

        // Ordinary three-finger swipe with no drag in progress is queued normally.
        if !should_ignore_swipe(false, false, false) {
            queue.push(QueuedInput::Action(swipe.clone())).unwrap();
        }
        assert_eq!(queue.pop(), Some(QueuedInput::Action(swipe)));
    }

    #[test]
    fn only_swipes_with_the_configured_finger_count_are_admitted() {
        use crate::swipe::{Fingers, Swipe};
        let three = WorkerAction::Swipe(Swipe::Left, Fingers::Three, (200, 100));
        let four = WorkerAction::Swipe(Swipe::Left, Fingers::Four, (200, 100));
        let idle = || (false, false, false);
        fn never() -> (bool, bool, bool) {
            unreachable!("pointer read for a dropped swipe")
        }

        // With [labs] four_finger_swipe off, macOS's four-finger swipe never reaches
        // the queue (and never reads the pointer); three fingers do.
        assert!(!admits_action(&four, Fingers::Three, never));
        assert!(admits_action(&three, Fingers::Three, idle));
        // With it on, the other way round.
        assert!(!admits_action(&three, Fingers::Four, never));
        assert!(admits_action(&four, Fingers::Four, idle));
        // A pointer drag still drops the configured swipe.
        let dragging = || (true, false, false);
        assert!(!admits_action(&four, Fingers::Four, dragging));
        // Other actions are untouched.
        let switch = WorkerAction::SwitchWorkspace(crate::WorkspaceId::TWO);
        assert!(admits_action(&switch, Fingers::Four, never));
    }

    #[test]
    fn gesture_tracker_records_pointer_bounds_even_when_worker_is_busy() {
        let band = Rect {
            x: 0,
            y: 0,
            width: 64,
            height: 800,
        };
        let moved = Rect { x: 60, ..start() };

        // Grabbed inside the band and nudged inside it -> bounds stay inside the band.
        let mut tracker = GestureTracker::default();
        tracker.begin_at(vec![(window(), start())], Some((10, 40)));
        tracker.observe_point((18, 40));
        let Some(FinalGesture::Drop {
            bounds: Some(nudge_bounds),
            ..
        }) = tracker.finish(frame(moved), Some((20, 40)))
        else {
            panic!("expected tracked drop");
        };
        assert!(nudge_bounds.entirely_inside(band));

        // Grabbed inside the band, dragged outside while worker was busy (no preview),
        // then returned inside -> bounds extend outside the band.
        let mut tracker = GestureTracker::default();
        tracker.begin_at(vec![(window(), start())], Some((10, 40)));
        tracker.observe_point((300, 40));
        tracker.observe_point((15, 40));
        let Some(FinalGesture::Drop {
            bounds: Some(armed_bounds),
            ..
        }) = tracker.finish(frame(moved), Some((15, 40)))
        else {
            panic!("expected tracked drop");
        };
        assert!(!armed_bounds.entirely_inside(band));
    }

    // #429: reproduction contributed by @coygeek. A title-bar drag whose
    // window changes width at mouse-up is still a drop.
    #[test]
    fn portrait_drag_release_classification() {
        let start = Rect {
            x: 1707,
            y: 30,
            width: 853,
            height: 1410,
        };
        let before_release = Rect {
            x: -1304,
            y: 565,
            ..start
        };
        let after_release = Rect {
            x: -1118,
            width: 480,
            ..before_release
        };
        let pointer = (-720, 550);
        for final_frame in [before_release, after_release] {
            let id = window();
            let mut tracker = GestureTracker::default();
            tracker.begin_at(vec![(id.clone(), start)], Some((2133, 44)));
            assert!(matches!(
                tracker.preview(|_| Some(before_release), Some(pointer)),
                Some(FinalGesture::Drop { .. }),
            ));
            let final_gesture = tracker.finish(|_| Some(final_frame), Some(pointer));
            assert!(
                matches!(final_gesture, Some(FinalGesture::Drop { .. })),
                "{final_frame:?}: {final_gesture:?}"
            );
            // Without a preview tick in between, the release alone still reads as a drop.
            tracker.begin_at(vec![(id.clone(), start)], Some((2133, 44)));
            let final_gesture = tracker.finish(|_| Some(final_frame), Some(pointer));
            assert!(
                matches!(final_gesture, Some(FinalGesture::Drop { .. })),
                "no preview, {final_frame:?}: {final_gesture:?}"
            );
        }
    }
}
