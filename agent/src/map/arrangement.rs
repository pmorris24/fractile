//! The screens as they sit on the desk, as System Settings › Displays › Arrange draws them: each
//! connected display at its real size and position, scaled to fit one panel, holding a card per
//! workspace that shows on it: its number, Add, and the apps whose new windows open there. Main wears the menu bar. A workspace card drags to another screen to
//! move the workspace there, and an app drags onto any workspace card. It is the pane's one
//! view of the workspaces; the sections below it list only those on an unplugged display.

mod view;

use spaceman::{Rect, WorkspaceId};

pub use view::ScreenDrag;
#[cfg(test)]
pub(super) use view::screen_drag_step;
pub(super) use view::{TOP, fixture_drag};

use crate::panes::workspaces::{Choice, ConnectedDisplay, Section, fixture_three};

/// The panel's height bounds, and the space kept clear around the screens inside it; the
/// bottom margin matches the others.
const MIN_H: f32 = 180.0;
/// The most of the panel's height the screens take at their true scale, as a share of its
/// width and at least `MAX_H`; a screen grows past its share only to fit its workspaces.
const MAX_H: f32 = 420.0;
const MAX_H_SHARE: f32 = 0.6;
const PAD: f32 = 16.0;
const PAD_BOTTOM: f32 = 16.0;
/// Between two screens that touch, so each reads as its own.
const SEAM: f32 = 4.0;
/// The menu bar along Main's top edge.
const MENU_BAR: f32 = 6.0;
/// Inside a screen: the inset around its contents and its name's line.
const INSET: f32 = 8.0;
const NAME_H: f32 = 16.0;
/// A workspace card: its largest size, the shape it keeps (a screen's), and the space between.
const CARD_MAX_W: f32 = 240.0;
const CARD_MAX_H: f32 = 170.0;
const CARD_ASPECT: f32 = 1.45;
const CARD_GAP: f32 = 6.0;
/// The smallest card a screen grows taller to fit: room for the number and two rows of apps.
const CARD_MIN_W: f32 = 60.0;
const CARD_MIN_H: f32 = 76.0;
/// Inside a card: the top line (its number's menu and Add), and the rows of app icons under it.
const CARD_PAD: f32 = 5.0;
const CARD_HEAD: f32 = 26.0;
pub(super) const RULE_ICON: f32 = 20.0;
const RULE_GAP: f32 = 4.0;

/// A box: x, y, width, height, from the panel's top-left.
pub(super) type Frame = (f32, f32, f32, f32);

/// One screen, connected or remembered, where it sits in the panel, and its workspace cards.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Monitor {
    pub key: String,
    pub title: String,
    pub caption: String,
    /// The `monitor` selector a workspace dropped here gets.
    pub selector: String,
    /// The display's own ID, and whether it is plugged in. An unplugged screen keeps the
    /// place and workspaces it had; they show on Main until it returns.
    pub id: String,
    pub connected: bool,
    pub is_main: bool,
    pub frame: Frame,
    /// Its workspaces, each with its card's frame.
    pub cards: Vec<(WorkspaceId, Frame)>,
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
    /// section in the pane lists. `apps` says how many apps open on a workspace: a screen
    /// grows until its cards show every one.
    pub fn new(
        sections: &[Section],
        choices: &[Choice],
        displays: &[ConnectedDisplay],
        away: &[String],
        width: f32,
        apps: impl Fn(WorkspaceId) -> usize,
    ) -> Panel {
        if displays.is_empty() {
            return Panel::default();
        }
        let found: Vec<(&Section, String, &ConnectedDisplay, bool)> = sections
            .iter()
            .filter_map(|section| {
                let index: usize = section.key.strip_prefix("display-")?.parse().ok()?;
                let choice = choices.get(index)?;
                let display = displays.iter().find(|d| d.selector == choice.selector)?;
                let connected = !away.contains(&display.id);
                Some((section, choice.selector.clone(), display, connected))
            })
            .collect();
        let card_hs: Vec<f32> = found
            .iter()
            .map(|(section, ..)| {
                let most = section.workspaces.iter().map(|ws| apps(*ws)).max();
                card_min_h(most.unwrap_or(0))
            })
            .collect();
        let screens: Vec<Fit> = found
            .iter()
            .zip(&card_hs)
            .map(|((section, _, display, _), card_h)| Fit {
                frame: display.frame,
                head: head(display.is_main),
                count: section.workspaces.len(),
                card_h: *card_h,
            })
            .collect();
        let (height, boxes) = fit(&screens, width);
        let monitors = found
            .into_iter()
            .zip(boxes)
            .zip(card_hs)
            .map(
                |(((section, selector, display, connected), frame), card_h)| {
                    let cards = cards(frame, head(display.is_main), &section.workspaces, card_h);
                    Monitor {
                        key: section.key.clone(),
                        title: section.title.clone(),
                        caption: section.caption.clone(),
                        selector,
                        id: display.id.clone(),
                        connected,
                        is_main: display.is_main,
                        frame,
                        cards,
                    }
                },
            )
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

    /// `workspace`'s card frame.
    pub fn card(&self, workspace: WorkspaceId) -> Option<Frame> {
        self.monitors
            .iter()
            .flat_map(|monitor| &monitor.cards)
            .find(|(ws, _)| *ws == workspace)
            .map(|(_, frame)| *frame)
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

/// The space over a screen's cards: Main's menu bar, and the name.
fn head(is_main: bool) -> f32 {
    (if is_main { MENU_BAR } else { 0.0 }) + INSET + NAME_H
}

/// What sizing a screen needs: its frame, the space over its cards, how many it holds, and how
/// tall each must be.
struct Fit {
    frame: Rect,
    head: f32,
    count: usize,
    card_h: f32,
}

/// The smallest card that shows `apps` icons in rows under its number, at [`CARD_MIN_W`].
fn card_min_h(apps: usize) -> f32 {
    let rows = apps.div_ceil(per_row(CARD_MIN_W)).max(2) as f32;
    (CARD_HEAD + rows * (RULE_ICON + RULE_GAP) - RULE_GAP + CARD_PAD).max(CARD_MIN_H)
}

/// The height a screen `width` wide needs under its `head` for `count` cards of at least
/// [`CARD_MIN_W`] × `card_h`.
fn needed_h(width: f32, head: f32, count: usize, card_h: f32) -> f32 {
    let cols =
        (((width - 2.0 * INSET + CARD_GAP) / (CARD_MIN_W + CARD_GAP)).floor() as usize).max(1);
    let rows = count.max(1).div_ceil(cols) as f32;
    head + rows * card_h + (rows - 1.0) * CARD_GAP + INSET
}

/// The panel's height and each screen's box in it. Every screen keeps its true width and
/// place at one scale, so their sizes and the way they sit compare; a screen too short for its
/// workspaces' cards grows taller, and pushes down any screen under it.
fn fit(screens: &[Fit], width: f32) -> (f32, Vec<Frame>) {
    let Some(Fit { frame: first, .. }) = screens.first() else {
        return (0.0, Vec::new());
    };
    let (mut left, mut top, mut right, mut bottom) = (
        first.x,
        first.y,
        first.x + first.width,
        first.y + first.height,
    );
    for Fit { frame: f, .. } in screens {
        left = left.min(f.x);
        top = top.min(f.y);
        right = right.max(f.x + f.width);
        bottom = bottom.max(f.y + f.height);
    }
    let (span_w, span_h) = ((right - left).max(1) as f32, (bottom - top).max(1) as f32);
    let max_h = MAX_H.max(width * MAX_H_SHARE);
    let scale = ((width - 2.0 * PAD) / span_w).min((max_h - PAD - PAD_BOTTOM) / span_h);
    let ox = (width - span_w * scale) / 2.0;
    let mut boxes: Vec<Frame> = screens
        .iter()
        .map(|screen| {
            let f = screen.frame;
            let w = f.width as f32 * scale - SEAM;
            let h = (f.height as f32 * scale - SEAM).max(needed_h(
                w,
                screen.head,
                screen.count,
                screen.card_h,
            ));
            (
                ox + (f.x - left) as f32 * scale + SEAM / 2.0,
                PAD + (f.y - top) as f32 * scale + SEAM / 2.0,
                w,
                h,
            )
        })
        .collect();
    // A screen that grew must not cover one below it: top to bottom, push each screen under
    // any it shares columns with.
    let mut order: Vec<usize> = (0..boxes.len()).collect();
    order.sort_by(|a, b| boxes[*a].1.total_cmp(&boxes[*b].1));
    for (n, &i) in order.iter().enumerate() {
        for &above in &order[..n] {
            let (ax, ay, aw, ah) = boxes[above];
            let (bx, by, bw, _) = boxes[i];
            let shares_columns = bx < ax + aw - SEAM && ax < bx + bw - SEAM;
            if shares_columns && by < ay + ah + SEAM {
                boxes[i].1 = ay + ah + SEAM;
            }
        }
    }
    let height = boxes
        .iter()
        .map(|(_, y, _, h)| y + h)
        .fold(0.0, f32::max)
        .max(MIN_H - PAD_BOTTOM)
        + PAD_BOTTOM;
    (height, boxes)
}

/// Every workspace's card in a screen's box under its name: the grid of columns that gives the
/// largest cards in a screen's shape, centred.
fn cards(
    screen: Frame,
    top: f32,
    workspaces: &[WorkspaceId],
    card_h: f32,
) -> Vec<(WorkspaceId, Frame)> {
    let (sx, sy, sw, sh) = screen;
    let (avail_w, avail_h) = (sw - 2.0 * INSET, sh - top - INSET);
    let n = workspaces.len().max(1);
    let size = |cols: usize| {
        let rows = n.div_ceil(cols) as f32;
        let w = ((avail_w - (cols - 1) as f32 * CARD_GAP) / cols as f32).min(CARD_MAX_W);
        let h = ((avail_h - (rows - 1.0) * CARD_GAP) / rows).min(CARD_MAX_H);
        // Never much wider than a screen's shape; as tall as its rows of apps need.
        (w.min(h * CARD_ASPECT * 1.4), h)
    };
    // The largest cards among the grids whose cards all keep the minimum size, which a screen
    // grown by `needed_h` always has; failing that, the largest cards at all.
    let largest = |fits: bool| {
        (1..=n)
            .filter(|cols| {
                let (w, h) = size(*cols);
                !fits || (w >= CARD_MIN_W - 0.5 && h >= card_h - 0.5)
            })
            .max_by(|a, b| {
                let (aw, ah) = size(*a);
                let (bw, bh) = size(*b);
                (aw * ah).total_cmp(&(bw * bh))
            })
    };
    let cols = largest(true).or_else(|| largest(false)).unwrap_or(1);
    let (w, h) = size(cols);
    let (w, h) = (w.max(1.0), h.max(1.0));
    let rows = n.div_ceil(cols) as f32;
    let block_h = rows * h + (rows - 1.0) * CARD_GAP;
    let y0 = sy + top + ((avail_h - block_h) / 2.0).max(0.0);
    workspaces
        .iter()
        .enumerate()
        .map(|(slot, workspace)| {
            let (row, col) = (slot / cols, slot % cols);
            let in_row = (workspaces.len() - row * cols).min(cols) as f32;
            let row_w = in_row * w + (in_row - 1.0) * CARD_GAP;
            let x = sx + (sw - row_w) / 2.0 + col as f32 * (w + CARD_GAP);
            let y = y0 + row as f32 * (h + CARD_GAP);
            (*workspace, (x, y, w, h))
        })
        .collect()
}

/// How many app icons a card of `size` holds: rows of them under its number.
pub(super) fn rule_room((w, h): (f32, f32)) -> usize {
    let per_row = per_row(w);
    let rows =
        (((h - CARD_HEAD - CARD_PAD + RULE_GAP) / (RULE_ICON + RULE_GAP)).floor() as usize).max(1);
    per_row * rows
}

fn per_row(width: f32) -> usize {
    (((width - 2.0 * CARD_PAD + RULE_GAP) / (RULE_ICON + RULE_GAP)).floor() as usize).max(1)
}

/// The `slot`th app icon in a card `width` wide, from the card's top-left.
pub(super) fn rule_offset(slot: usize, width: f32) -> (f32, f32) {
    let per_row = per_row(width);
    let (col, row) = (slot % per_row, slot / per_row);
    (
        CARD_PAD + col as f32 * (RULE_ICON + RULE_GAP),
        CARD_HEAD + row as f32 * (RULE_ICON + RULE_GAP),
    )
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

/// The desk's laptop unplugged from both monitors: Main now, at the origin.
pub(super) fn fixture_laptop_alone() -> Vec<ConnectedDisplay> {
    let laptop = fixture_desk().remove(2);
    vec![ConnectedDisplay {
        selector: crate::panes::workspaces::MAIN_SELECTOR.to_owned(),
        frame: Rect {
            x: 0,
            y: 0,
            ..laptop.frame
        },
        is_main: true,
        is_secondary: false,
        ..laptop
    }]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fit_of(frame: Rect, is_main: bool, count: usize, apps: usize) -> Fit {
        Fit {
            frame,
            head: head(is_main),
            count,
            card_h: card_min_h(apps),
        }
    }

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
        let screens: Vec<Fit> = frames.iter().map(|f| fit_of(*f, false, 1, 0)).collect();
        let (height, boxes) = fit(&screens, width);
        assert!(height >= MIN_H);
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
    fn a_screen_grows_to_fit_its_cards_and_pushes_down_the_one_under_it() {
        // A laptop under a wide monitor, holding seven workspaces.
        let screens = [
            fit_of(rect(0, 0, 3440, 1440), true, 7, 0),
            fit_of(rect(800, 1440, 1728, 1117), false, 3, 0),
        ];
        let (height, boxes) = fit(&screens, 600.0);
        let (top, under) = (boxes[0], boxes[1]);
        assert!(top.3 >= needed_h(top.2, head(true), 7, CARD_MIN_H) - 0.01);
        assert!(under.1 >= top.1 + top.3, "no overlap");
        assert!(height >= under.1 + under.3 + PAD_BOTTOM - 0.01);
        // Each card in the grown screen is at least the minimum.
        let cards = cards(
            top,
            head(true),
            &WorkspaceId::numbered().take(7).collect::<Vec<_>>(),
            CARD_MIN_H,
        );
        for (_, (_, _, w, h)) in cards {
            assert!(w >= CARD_MIN_W - 0.01 && h >= CARD_MIN_H - 0.01, "{w}x{h}");
        }
    }

    #[test]
    fn cards_fit_inside_their_screen_and_never_overlap() {
        let all: Vec<WorkspaceId> = WorkspaceId::numbered().collect();
        for (screen, count) in [
            ((10.0, 10.0, 340.0, 190.0), 4),
            ((10.0, 10.0, 150.0, 270.0), 4),
            ((10.0, 10.0, 180.0, 115.0), 2),
            ((10.0, 10.0, 340.0, 190.0), 7),
        ] {
            let cards = cards(screen, INSET + NAME_H, &all[..count], CARD_MIN_H);
            assert_eq!(cards.len(), count);
            for (i, (_, (x, y, w, h))) in cards.iter().enumerate() {
                assert!(
                    *x >= screen.0 && x + w <= screen.0 + screen.2 + 0.01,
                    "{i} in x"
                );
                assert!(
                    *y >= screen.1 && y + h <= screen.1 + screen.3 + 0.01,
                    "{i} in y"
                );
                for (_, (ox, oy, _, _)) in &cards[i + 1..] {
                    assert!((x - ox).abs() >= *w - 0.01 || (y - oy).abs() >= *h - 0.01);
                }
            }
        }
        // Where the screen has room, every card keeps the minimum size.
        for (_, (_, _, w, h)) in cards(
            (0.0, 0.0, 340.0, 190.0),
            INSET + NAME_H,
            &all[..4],
            CARD_MIN_H,
        ) {
            assert!(w >= CARD_MIN_W - 0.5 && h >= CARD_MIN_H - 0.5, "{w}x{h}");
        }
    }

    #[test]
    fn a_workspace_with_many_apps_gets_a_card_that_shows_every_one() {
        let all: Vec<WorkspaceId> = WorkspaceId::numbered().collect();
        let apps = 9;
        let screens = [fit_of(rect(0, 0, 2560, 1440), true, 4, apps)];
        let (_, boxes) = fit(&screens, 600.0);
        for (_, (_, _, w, h)) in cards(boxes[0], head(true), &all[..4], card_min_h(apps)) {
            assert!(
                rule_room((w, h)) >= apps,
                "{w}x{h} holds {}",
                rule_room((w, h))
            );
        }
    }
}
