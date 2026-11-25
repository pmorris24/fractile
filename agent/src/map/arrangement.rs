//! The screens as they sit on the desk, as System Settings › Displays › Arrange draws them: each
//! connected display at its real size and position, scaled to fit one panel, with its name and
//! the workspaces that show on it. Main wears the menu bar. Drawn only with two or more screens.

use rune::{El, el, text};
use spaceman::{Rect, WorkspaceId};

use crate::{
    controls::Paint,
    panes::workspaces::{Choice, ConnectedDisplay, Section, fixture_three},
};

/// The panel's height, and the space kept clear around the screens inside it.
pub(super) const HEIGHT: f32 = 200.0;
const PAD: f32 = 24.0;
/// Between two screens that touch, so each reads as its own.
const SEAM: f32 = 2.0;
/// The menu bar along Main's top edge.
const MENU_BAR: f32 = 6.0;
/// Inside a screen: its name's inset, and its workspace numbers' size and spacing.
const INSET: f32 = 8.0;
const NAME_H: f32 = 16.0;
const NUMBER: f32 = 20.0;
const NUMBER_GAP: f32 = 4.0;

/// One connected screen and the workspaces that show on it.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Screen {
    pub key: String,
    pub title: String,
    pub caption: String,
    pub frame: Rect,
    pub is_main: bool,
    pub workspaces: Vec<WorkspaceId>,
}

/// The panel's screens: each connected display with its card's title and workspaces. None
/// with fewer than two displays, where there is nothing to arrange.
pub(super) fn screens(
    sections: &[Section],
    choices: &[Choice],
    displays: &[ConnectedDisplay],
) -> Vec<Screen> {
    if displays.len() < 2 {
        return Vec::new();
    }
    sections
        .iter()
        .filter_map(|section| {
            let index: usize = section.key.strip_prefix("display-")?.parse().ok()?;
            let choice = choices.get(index)?;
            let display = displays.iter().find(|d| d.selector == choice.selector)?;
            Some(Screen {
                key: section.key.clone(),
                title: section.title.clone(),
                caption: section.caption.clone(),
                frame: display.frame,
                is_main: display.is_main,
                workspaces: section.workspaces.clone(),
            })
        })
        .collect()
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

/// Each screen's box in a `width` × [`HEIGHT`] panel: one scale for all, so their sizes compare,
/// and the group centred.
pub(super) fn boxes(screens: &[Screen], width: f32) -> Vec<(f32, f32, f32, f32)> {
    let Some(first) = screens.first() else {
        return Vec::new();
    };
    let (mut left, mut top, mut right, mut bottom) = (
        first.frame.x,
        first.frame.y,
        first.frame.x + first.frame.width,
        first.frame.y + first.frame.height,
    );
    for screen in screens {
        let f = screen.frame;
        left = left.min(f.x);
        top = top.min(f.y);
        right = right.max(f.x + f.width);
        bottom = bottom.max(f.y + f.height);
    }
    let (span_w, span_h) = ((right - left).max(1) as f32, (bottom - top).max(1) as f32);
    let scale = ((width - 2.0 * PAD) / span_w).min((HEIGHT - 2.0 * PAD) / span_h);
    let (ox, oy) = (
        (width - span_w * scale) / 2.0,
        (HEIGHT - span_h * scale) / 2.0,
    );
    screens
        .iter()
        .map(|screen| {
            let f = screen.frame;
            (
                ox + (f.x - left) as f32 * scale + SEAM / 2.0,
                oy + (f.y - top) as f32 * scale + SEAM / 2.0,
                f.width as f32 * scale - SEAM,
                f.height as f32 * scale - SEAM,
            )
        })
        .collect()
}

/// The panel, `width` wide, with `active` the workspaces in view on their screens.
pub(super) fn view(p: Paint, screens: &[Screen], width: f32, active: &[WorkspaceId]) -> El {
    let c = &p.k.c;
    let mut panel = el()
        .key("arrangement")
        .role("group")
        .label("Display arrangement")
        .size(width, HEIGHT)
        .radius(p.k.radius.lg)
        .fill(c.layer_01)
        .border(p.hair, c.border_weak);
    for (screen, (x, y, w, h)) in screens.iter().zip(boxes(screens, width)) {
        panel = panel.child(display(p, screen, (w, h), active).absolute(x, y));
    }
    panel
}

fn display(p: Paint, screen: &Screen, (w, h): (f32, f32), active: &[WorkspaceId]) -> El {
    let c = &p.k.c;
    let on_view = screen.workspaces.iter().any(|ws| active.contains(ws));
    let mut out = el()
        .key(format!("arrange-{}", screen.key))
        .role("img")
        .label(format!("{}: {}", screen.title, screen.caption))
        .help(format!("{} · {}", screen.title, screen.caption))
        .size(w, h)
        .radius(p.k.radius.md)
        .fill(c.layer_02)
        .border(
            if on_view { 1.5 } else { p.hair },
            if on_view {
                c.border_strong
            } else {
                c.border_base
            },
        );
    let mut top = INSET;
    if screen.is_main {
        out = out.child(
            el().key("menu-bar")
                .size(w, MENU_BAR)
                .fill(c.layer_03)
                .absolute(0.0, 0.0),
        );
        top += MENU_BAR;
    }
    if w > 2.0 * INSET && h > top + NAME_H {
        out = out.child(
            text(screen.title.clone())
                .key("name")
                .font(p.meta())
                .color(p.muted())
                .truncate()
                .w(w - 2.0 * INSET)
                .absolute(INSET, top),
        );
    }
    // The workspace numbers, centred in rows below the name.
    let per_row =
        (((w - 2.0 * INSET + NUMBER_GAP) / (NUMBER + NUMBER_GAP)).floor() as usize).max(1);
    let rows = screen.workspaces.len().div_ceil(per_row);
    let block_h = rows as f32 * NUMBER + rows.saturating_sub(1) as f32 * NUMBER_GAP;
    let numbers_top = (top + NAME_H).max((h - block_h) / 2.0 + MENU_BAR / 2.0);
    for (slot, workspace) in screen.workspaces.iter().enumerate() {
        let (row, col) = (slot / per_row, slot % per_row);
        let in_row = (screen.workspaces.len() - row * per_row).min(per_row);
        let row_w = in_row as f32 * NUMBER + (in_row - 1) as f32 * NUMBER_GAP;
        let x = (w - row_w) / 2.0 + col as f32 * (NUMBER + NUMBER_GAP);
        let y = numbers_top + row as f32 * (NUMBER + NUMBER_GAP);
        if y + NUMBER > h {
            break;
        }
        let shown = active.contains(workspace);
        out = out.child(
            el().key(format!("ws-{workspace}"))
                .size(NUMBER, NUMBER)
                .radius(p.k.radius.sm)
                .fill(if shown { c.layer_03 } else { c.layer_01 })
                .border(
                    p.hair,
                    if shown {
                        c.border_strong
                    } else {
                        c.border_weak
                    },
                )
                .center()
                .child(
                    text(workspace.to_string())
                        .font(p.tabular(p.tag()))
                        .color(if shown { p.text() } else { p.muted() }),
                )
                .absolute(x, y),
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen(key: &str, x: i32, y: i32, width: i32, height: i32) -> Screen {
        Screen {
            key: key.into(),
            title: key.into(),
            caption: String::new(),
            frame: Rect {
                x,
                y,
                width,
                height,
            },
            is_main: key == "main",
            workspaces: vec![WorkspaceId::ONE],
        }
    }

    #[test]
    fn screens_keep_their_order_size_and_offsets_and_fit_the_panel() {
        // A portrait screen to the right of Main, sitting higher; a small laptop to the left.
        let screens = [
            screen("main", 0, 0, 2560, 1440),
            screen("right", 2560, -400, 1440, 2560),
            screen("left", -1728, 400, 1728, 1117),
        ];
        let width = 600.0;
        let boxes = boxes(&screens, width);
        let (main, right, left) = (boxes[0], boxes[1], boxes[2]);
        assert!(left.0 < main.0 && main.0 < right.0, "left to right");
        assert!(right.1 < main.1 && main.1 < left.1, "top to bottom");
        assert!(right.3 > right.2, "portrait stays portrait");
        assert!(((main.2 + SEAM) / (left.2 + SEAM) - 2560.0 / 1728.0).abs() < 0.01);
        for (x, y, w, h) in boxes {
            assert!(x >= PAD - 0.01 && x + w <= width - PAD + 0.01);
            assert!(y >= PAD - 0.01 && y + h <= HEIGHT - PAD + 0.01);
        }
    }
}
