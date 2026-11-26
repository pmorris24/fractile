//! Three- and four-finger horizontal trackpad swipes, as workspace steps.

/// The most fingers a swipe may have; a fifth finger spends the gesture.
const MAX_FINGERS: usize = 4;
/// How far the fingers travel, as a fraction of the trackpad's width.
const DISTANCE: f64 = 0.15;
/// Horizontal travel must beat vertical travel by this much, so Mission Control's
/// upward swipe never switches.
const DOMINANCE: f64 = 2.0;
/// A gap this long between frames means the swipe's end was missed.
const STALE_SECONDS: f64 = 0.5;

/// How many fingers made a swipe. Fractile steps workspaces on the count
/// `Config::swipe_fingers` picks and leaves the other to macOS.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Fingers {
    Three,
    Four,
}

impl Fingers {
    const fn count(self) -> usize {
        match self {
            Self::Three => 3,
            Self::Four => 4,
        }
    }

    const fn of(count: usize) -> Option<Self> {
        match count {
            3 => Some(Self::Three),
            4 => Some(Self::Four),
            _ => None,
        }
    }
}

/// Which way a swipe steps: `Left` goes to the next workspace and `Right` to the
/// previous one. The tracker reports the fingers' direction; see
/// [`Swipe::for_scroll_direction`] for how that becomes a step.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Swipe {
    Left,
    Right,
}

impl Swipe {
    /// Maps the fingers' direction to a workspace step the way macOS Spaces does.
    /// With Natural scrolling on the fingers drag the desktop strip, so swiping
    /// left goes to the next workspace; with it off they move the view, so
    /// swiping right does.
    #[must_use]
    pub const fn for_scroll_direction(self, natural: bool) -> Self {
        match (self, natural) {
            (swipe, true) => swipe,
            (Self::Left, false) => Self::Right,
            (Self::Right, false) => Self::Left,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum State {
    #[default]
    Ready,
    Tracking {
        fingers: Fingers,
        x: f64,
        y: f64,
    },
    /// Fired, a fifth finger landed, or a finger lifted from four: nothing more
    /// until every finger lifts.
    Spent,
}

/// Fires at most once per swipe, from the touching fingers' normalized positions.
#[derive(Debug, Default)]
pub struct SwipeTracker {
    state: State,
    last_frame: Option<f64>,
}

impl SwipeTracker {
    /// `at` is the frame's timestamp in seconds; `touches` are the fingers on the
    /// trackpad, each `(x, y)` from 0 to 1. Three fingers joined by a fourth
    /// start over as a four-finger swipe; four dropping to three never fires.
    pub fn frame(&mut self, at: f64, touches: &[(f64, f64)]) -> Option<(Swipe, Fingers)> {
        if self
            .last_frame
            .is_some_and(|last| at - last > STALE_SECONDS)
        {
            self.state = State::Ready;
        }
        self.last_frame = Some(at);
        if touches.is_empty() {
            self.state = State::Ready;
            return None;
        }
        if touches.len() > MAX_FINGERS {
            self.state = State::Spent;
            return None;
        }
        if self.state == State::Spent {
            return None;
        }
        let Some(fingers) = Fingers::of(touches.len()) else {
            self.state = State::Ready;
            return None;
        };
        let count = fingers.count() as f64;
        let x = touches.iter().map(|touch| touch.0).sum::<f64>() / count;
        let y = touches.iter().map(|touch| touch.1).sum::<f64>() / count;
        let (start_x, start_y) = match self.state {
            State::Tracking {
                fingers: tracked,
                x,
                y,
            } if tracked == fingers => (x, y),
            State::Tracking {
                fingers: Fingers::Four,
                ..
            } => {
                self.state = State::Spent;
                return None;
            }
            _ => {
                self.state = State::Tracking { fingers, x, y };
                return None;
            }
        };
        let (dx, dy) = (x - start_x, y - start_y);
        if dx.abs() < DISTANCE || dx.abs() < DOMINANCE * dy.abs() {
            return None;
        }
        self.state = State::Spent;
        let swipe = if dx < 0.0 { Swipe::Left } else { Swipe::Right };
        Some((swipe, fingers))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fingers(x: f64, y: f64, count: usize) -> Vec<(f64, f64)> {
        (0..count)
            .map(|finger| (x + finger as f64 * 0.05, y))
            .collect()
    }

    /// Feeds a swipe from `from` to `to` in small steps, 10 ms apart.
    fn drag(
        tracker: &mut SwipeTracker,
        start: f64,
        from: (f64, f64),
        to: (f64, f64),
        count: usize,
    ) -> Vec<(Swipe, Fingers)> {
        (0..=20)
            .filter_map(|step| {
                let t = f64::from(step) / 20.0;
                let x = from.0 + (to.0 - from.0) * t;
                let y = from.1 + (to.1 - from.1) * t;
                tracker.frame(start + f64::from(step) * 0.01, &fingers(x, y, count))
            })
            .collect()
    }

    #[test]
    fn three_fingers_left_and_right_fire_once_each() {
        let mut tracker = SwipeTracker::default();
        assert_eq!(
            drag(&mut tracker, 0.0, (0.6, 0.5), (0.2, 0.5), 3),
            [(Swipe::Left, Fingers::Three)]
        );
        assert_eq!(tracker.frame(0.3, &[]), None);
        assert_eq!(
            drag(&mut tracker, 0.4, (0.2, 0.5), (0.6, 0.5), 3),
            [(Swipe::Right, Fingers::Three)]
        );
    }

    #[test]
    fn four_finger_swipes_report_four_fingers() {
        let mut tracker = SwipeTracker::default();
        assert_eq!(
            drag(&mut tracker, 0.0, (0.6, 0.5), (0.1, 0.5), 4),
            [(Swipe::Left, Fingers::Four)]
        );
        tracker.frame(0.3, &[]);
        // Three fingers land first, then the fourth: one four-finger swipe.
        tracker.frame(0.4, &fingers(0.2, 0.5, 3));
        assert_eq!(
            drag(&mut tracker, 0.41, (0.2, 0.5), (0.6, 0.5), 4),
            [(Swipe::Right, Fingers::Four)]
        );
    }

    #[test]
    fn short_vertical_and_five_finger_swipes_do_nothing() {
        let mut tracker = SwipeTracker::default();
        assert!(drag(&mut tracker, 0.0, (0.5, 0.5), (0.4, 0.5), 3).is_empty());
        tracker.frame(0.3, &[]);
        assert!(drag(&mut tracker, 0.4, (0.5, 0.2), (0.3, 0.8), 3).is_empty());
        tracker.frame(0.7, &[]);
        assert!(drag(&mut tracker, 0.8, (0.5, 0.2), (0.3, 0.8), 4).is_empty());
        tracker.frame(1.1, &[]);
        assert!(drag(&mut tracker, 1.2, (0.6, 0.5), (0.1, 0.5), 5).is_empty());
    }

    #[test]
    fn lifting_from_four_fingers_to_three_does_not_fire() {
        let mut tracker = SwipeTracker::default();
        tracker.frame(0.0, &fingers(0.5, 0.5, 4));
        assert!(drag(&mut tracker, 0.01, (0.6, 0.5), (0.1, 0.5), 3).is_empty());
    }

    #[test]
    fn a_missed_lift_does_not_block_the_next_swipe() {
        let mut tracker = SwipeTracker::default();
        assert_eq!(drag(&mut tracker, 0.0, (0.6, 0.5), (0.2, 0.5), 3).len(), 1);
        assert_eq!(
            drag(&mut tracker, 2.0, (0.6, 0.5), (0.2, 0.5), 3),
            [(Swipe::Left, Fingers::Three)]
        );
    }

    #[test]
    fn swipes_follow_the_natural_scrolling_setting() {
        assert_eq!(Swipe::Left.for_scroll_direction(true), Swipe::Left);
        assert_eq!(Swipe::Right.for_scroll_direction(true), Swipe::Right);
        assert_eq!(Swipe::Left.for_scroll_direction(false), Swipe::Right);
        assert_eq!(Swipe::Right.for_scroll_direction(false), Swipe::Left);
    }
}
