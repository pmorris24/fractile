//! The screens as they sit on the desk, as System Settings › Displays › Arrange draws them: each
//! connected display at its real size and position, scaled to fit one panel, holding a card per
//! workspace that shows on it with the apps whose new windows open there. Main wears the menu
//! bar. A workspace card drags to another screen to move the workspace there, and an app drags
//! onto any workspace card. Drawn only with two or more screens.

mod view;

use spaceman::{Rect, WorkspaceId};

pub use view::ScreenDrag;
#[cfg(test)]
pub(super) use view::screen_drag_step;
pub(super) use view::{TOP, fixture_drag};

use crate::panes::workspaces::{Choice, ConnectedDisplay, Section, fixture_three};

/// The panel's height bounds, and the space kept clear around the screens inside it; the
/// bottom margin holds the hint.
const MIN_H: f32 = 180.0;
const MAX_H: f32 = 380.0;
const PAD: f32 = 20.0;
const PAD_BOTTOM: f32 = 32.0;
/// Between two screens that touch, so each reads as its own.
const SEAM: f32 = 4.0;
/// The menu bar along Main's top edge.
const MENU_BAR: f32 = 6.0;
/// Inside a screen: the inset around its contents and its name's line.
const INSET: f32 = 8.0;
const NAME_H: f32 = 16.0;
/// A workspace card: its largest and smallest side, and the space between cards.
const CARD_MAX: f32 = 64.0;
const CARD_MIN: f32 = 36.0;
const CARD_GAP: f32 = 6.0;

/// A box: x, y, width, height, from the panel's top-left.
pub(super) type Frame = (f32, f32, f32, f32);

/// One connected screen, where it sits in the panel, and its workspace cards.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Monitor {
    pub key: String,
    pub title: String,
    pub caption: String,
    /// The `monitor` selector a workspace dropped here gets.
    pub selector: String,
    pub is_main: bool,
    pub frame: Frame,
    /// Its workspaces, each with its card's frame.
    pub cards: Vec<(WorkspaceId, Frame)>,
    /// The cards' side.
    pub card: f32,
}

/// The panel: its height and its screens. Empty, and no height, with fewer than two displays.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Panel {
    pub width: f32,
    pub height: f32,
    pub monitors: Vec<Monitor>,
}

impl Panel {
    /// The panel for the connected `displays`, `width` wide, each holding the workspaces its
    /// section in the pane lists.
    pub fn new(
        sections: &[Section],
        choices: &[Choice],
        displays: &[ConnectedDisplay],
        width: f32,
    ) -> Panel {
        if displays.len() < 2 {
            return Panel::default();
        }
        let found: Vec<(&Section, &Choice, &ConnectedDisplay)> = sections
            .iter()
            .filter_map(|section| {
                let index: usize = section.key.strip_prefix("display-")?.parse().ok()?;
                let choice = choices.get(index)?;
                let display = displays.iter().find(|d| d.selector == choice.selector)?;
                Some((section, choice, display))
            })
            .collect();
        let frames: Vec<Rect> = found.iter().map(|(_, _, display)| display.frame).collect();
        let (height, boxes) = fit(&frames, width);
        let monitors = found
            .into_iter()
            .zip(boxes)
            .map(|((section, choice, display), frame)| {
                let top = if display.is_main { MENU_BAR } else { 0.0 } + INSET + NAME_H;
                let (card, cards) = cards(frame, top, &section.workspaces);
                Monitor {
                    key: section.key.clone(),
                    title: section.title.clone(),
                    caption: section.caption.clone(),
                    selector: choice.selector.clone(),
                    is_main: display.is_main,
                    frame,
                    cards,
                    card,
                }
            })
            .collect();
        Panel {
            width,
            height,
            monitors,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.monitors.is_empty()
    }

    /// The workspace whose card is under a point in the panel.
    pub fn card_at(&self, x: f32, y: f32) -> Option<WorkspaceId> {
        self.monitors
            .iter()
            .flat_map(|monitor| &monitor.cards)
            .find(|(_, frame)| contains(*frame, x, y))
            .map(|(workspace, _)| *workspace)
    }

    /// The screen under a point in the panel, or the nearest one while the point is still over
    /// the panel, so a card let go in a seam still lands.
    pub fn monitor_at(&self, x: f32, y: f32) -> Option<usize> {
        if !contains((0.0, 0.0, self.width, self.height), x, y) {
            return None;
        }
        let distance = |(fx, fy, fw, fh): Frame| {
            let dx = (fx - x).max(x - (fx + fw)).max(0.0);
            let dy = (fy - y).max(y - (fy + fh)).max(0.0);
            dx * dx + dy * dy
        };
        self.monitors
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| distance(a.frame).total_cmp(&distance(b.frame)))
            .map(|(index, _)| index)
    }

    /// The screen `workspace` shows on, by its index.
    pub fn monitor_of(&self, workspace: WorkspaceId) -> Option<usize> {
        self.monitors
            .iter()
            .position(|monitor| monitor.cards.iter().any(|(ws, _)| *ws == workspace))
    }
}

fn contains((fx, fy, fw, fh): Frame, x: f32, y: f32) -> bool {
    (fx..=fx + fw).contains(&x) && (fy..=fy + fh).contains(&y)
}

/// The panel's height and each display's box in it: one scale for all, so their sizes compare,
/// as large as the width allows within [`MAX_H`], and the group centred.
fn fit(frames: &[Rect], width: f32) -> (f32, Vec<Frame>) {
    let Some(first) = frames.first() else {
        return (0.0, Vec::new());
    };
    let (mut left, mut top, mut right, mut bottom) = (
        first.x,
        first.y,
        first.x + first.width,
        first.y + first.height,
    );
    for f in frames {
        left = left.min(f.x);
        top = top.min(f.y);
        right = right.max(f.x + f.width);
        bottom = bottom.max(f.y + f.height);
    }
    let (span_w, span_h) = ((right - left).max(1) as f32, (bottom - top).max(1) as f32);
    let scale = ((width - 2.0 * PAD) / span_w).min((MAX_H - PAD - PAD_BOTTOM) / span_h);
    let height = (span_h * scale + PAD + PAD_BOTTOM).max(MIN_H);
    let (ox, oy) = (
        (width - span_w * scale) / 2.0,
        PAD + (height - PAD - PAD_BOTTOM - span_h * scale) / 2.0,
    );
    let boxes = frames
        .iter()
        .map(|f| {
            (
                ox + (f.x - left) as f32 * scale + SEAM / 2.0,
                oy + (f.y - top) as f32 * scale + SEAM / 2.0,
                f.width as f32 * scale - SEAM,
                f.height as f32 * scale - SEAM,
            )
        })
        .collect();
    (height, boxes)
}

/// The largest card side that fits every workspace in a screen's box under its name, and each
/// card's frame in the panel, the rows centred.
fn cards(screen: Frame, top: f32, workspaces: &[WorkspaceId]) -> (f32, Vec<(WorkspaceId, Frame)>) {
    let (sx, sy, sw, sh) = screen;
    let (avail_w, avail_h) = (sw - 2.0 * INSET, sh - top - INSET);
    let n = workspaces.len().max(1);
    let per_row = |side: f32| (((avail_w + CARD_GAP) / (side + CARD_GAP)).floor() as usize).max(1);
    let rows_h = |side: f32| {
        let rows = n.div_ceil(per_row(side)) as f32;
        rows * side + (rows - 1.0) * CARD_GAP
    };
    let mut side = CARD_MAX;
    while side > CARD_MIN && (rows_h(side) > avail_h || side > avail_w) {
        side -= 2.0;
    }
    let cols = per_row(side);
    let block_h = rows_h(side);
    let y0 = sy + top + ((avail_h - block_h) / 2.0).max(0.0);
    let frames = workspaces
        .iter()
        .enumerate()
        .map(|(slot, workspace)| {
            let (row, col) = (slot / cols, slot % cols);
            let in_row = (workspaces.len() - row * cols).min(cols) as f32;
            let row_w = in_row * side + (in_row - 1.0) * CARD_GAP;
            let x = sx + (sw - row_w) / 2.0 + col as f32 * (side + CARD_GAP);
            let y = y0 + row as f32 * (side + CARD_GAP);
            (*workspace, (x, y, side, side))
        })
        .collect();
    (side, frames)
}

/// Three screens of different sizes at different heights: a laptop low on the left, Main in
/// the middle, and a portrait monitor raised on the right.
pub(super) fn fixture_desk() -> Vec<ConnectedDisplay> {
    let mut displays = fixture_three();
    for (display, (x, y, width, height)) in displays.iter_mut().zip([
        (0, 0, 2560, 1440),
        (2560, -420, 1440, 2560),
        (-1728, 300, 1728, 1117),
    ]) {
        display.frame = Rect {
            x,
            y,
            width,
            height,
        };
    }
    displays
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: i32, y: i32, width: i32, height: i32) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn screens_keep_their_order_size_and_offsets_and_fit_the_panel() {
        // A portrait screen to the right of Main, sitting higher; a small laptop to the left.
        let frames = [
            rect(0, 0, 2560, 1440),
            rect(2560, -400, 1440, 2560),
            rect(-1728, 400, 1728, 1117),
        ];
        let width = 600.0;
        let (height, boxes) = fit(&frames, width);
        assert!((MIN_H..=MAX_H).contains(&height));
        let (main, right, left) = (boxes[0], boxes[1], boxes[2]);
        assert!(left.0 < main.0 && main.0 < right.0, "left to right");
        assert!(right.1 < main.1 && main.1 < left.1, "top to bottom");
        assert!(right.3 > right.2, "portrait stays portrait");
        assert!(((main.2 + SEAM) / (left.2 + SEAM) - 2560.0 / 1728.0).abs() < 0.01);
        for (x, y, w, h) in boxes {
            assert!(x >= PAD - 0.01 && x + w <= width - PAD + 0.01);
            assert!(y >= PAD - 0.01 && y + h <= height - PAD_BOTTOM + 0.01);
        }
    }

    #[test]
    fn cards_fit_inside_their_screen_and_never_overlap() {
        let all: Vec<WorkspaceId> = WorkspaceId::numbered().collect();
        let screen = (10.0, 10.0, 260.0, 150.0);
        let (side, cards) = cards(screen, INSET + NAME_H, &all[..6]);
        assert!((CARD_MIN..=CARD_MAX).contains(&side));
        for (i, (_, (x, y, w, h))) in cards.iter().enumerate() {
            assert!(
                *x >= screen.0 && x + w <= screen.0 + screen.2,
                "card {i} in x"
            );
            assert!(
                *y >= screen.1 && y + h <= screen.1 + screen.3,
                "card {i} in y"
            );
            for (_, (ox, oy, _, _)) in &cards[i + 1..] {
                assert!((x - ox).abs() >= side || (y - oy).abs() >= side);
            }
        }
    }
}
