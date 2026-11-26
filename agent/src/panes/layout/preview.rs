//! The miniature displays at the top of Settings › Layout, drawn with the engine's own spacing
//! math so they match what the windows will do: a bezel, a dark wallpaper under a menu bar, and
//! tiny windows with title bars and traffic lights, the same in light and dark mode.

use std::f32::consts::PI;

use fractile::{Rect, config::LayoutMode, spacing::Spacing};
use rune::{Color, El, Shadow, Spring, col, el, oklch, oklcha, row};

use crate::controls::{self, Paint};

/// A 16:10 display's visible frame, sized so the gaps read at preview scale.
const DISPLAY: Rect = Rect {
    x: 0,
    y: 0,
    width: 1280,
    height: 800,
};
/// A 21:9 ultrawide, where the single-window width shows.
const ULTRAWIDE: Rect = Rect {
    x: 0,
    y: 0,
    width: 3440,
    height: 1440,
};
const DISPLAY_WIDTH: f32 = 300.0;
const ULTRAWIDE_WIDTH: f32 = 196.0;
/// The bezel round the glass, and the corner radii of each.
const BEZEL: f32 = 3.0;
const BEZEL_RADIUS: f32 = 9.0;
const GLASS_RADIUS: f32 = 6.0;
const MENU_BAR: f32 = 7.0;
const TITLE_BAR: f32 = 7.0;
const WINDOW_RADIUS: f32 = 3.5;
const LIGHT: f32 = 3.5;
const SETTLE: Spring = Spring::new(0.22, 0.0);

/// Three tiles as the engine places them for `layout`: left half + two stacked on the right
/// for Dwindle, or three equal columns for Columns.
pub(super) fn three_tiles(layout: LayoutMode, spacing: Spacing, display: Rect) -> Vec<Rect> {
    let usable = spacing.usable(display);
    let canvas = spacing.canvas(usable);
    let cells = match layout {
        LayoutMode::Dwindle => {
            let half = canvas.width / 2;
            vec![
                Rect {
                    width: half,
                    ..canvas
                },
                Rect {
                    x: canvas.x + half,
                    width: canvas.width - half,
                    height: canvas.height / 2,
                    ..canvas
                },
                Rect {
                    x: canvas.x + half,
                    y: canvas.y + canvas.height / 2,
                    width: canvas.width - half,
                    height: canvas.height - canvas.height / 2,
                },
            ]
        }
        LayoutMode::Columns => {
            let first = canvas.width / 3;
            let second = canvas.width * 2 / 3;
            vec![
                Rect {
                    width: first,
                    ..canvas
                },
                Rect {
                    x: canvas.x + first,
                    width: second - first,
                    ..canvas
                },
                Rect {
                    x: canvas.x + second,
                    width: canvas.width - second,
                    ..canvas
                },
            ]
        }
    };
    spacing
        .place(usable, cells.into_iter().map(|cell| ((), cell)).collect())
        .into_iter()
        .map(|(_, frame)| frame)
        .collect()
}

/// The preview card: the three-window display, joined by an ultrawide holding one capped window
/// while a single-window width or height is set.
pub(super) fn card(p: Paint, layout: LayoutMode, spacing: Spacing) -> El {
    let mut displays = row()
        .key("displays")
        .gap(28.0)
        .justify_center()
        .items_end()
        .child(display(
            p,
            "split",
            "Three windows",
            DISPLAY,
            DISPLAY_WIDTH,
            &three_tiles(layout, spacing, DISPLAY),
        ));
    if spacing.single_window_max_width > 0 || spacing.single_window_max_height > 0 {
        displays = displays.child(display(
            p,
            "single",
            "One window on an ultrawide display",
            ULTRAWIDE,
            ULTRAWIDE_WIDTH,
            &[spacing.single(spacing.usable(ULTRAWIDE))],
        ));
    }
    controls::surface(p, col().key("preview").py(24.0).px(16.0).items_center())
        .label("Layout preview")
        .child(displays)
}

/// One display: the bezel, then the glass holding the wallpaper, the menu bar and the windows.
fn display(p: Paint, key: &str, label: &str, screen: Rect, width: f32, windows: &[Rect]) -> El {
    let scale = width / screen.width as f32;
    let height = screen.height as f32 * scale;
    let mut glass = el()
        .key("screen")
        .size(width, MENU_BAR + height)
        .radius(GLASS_RADIUS)
        .clip_rounded(GLASS_RADIUS)
        .child(wallpaper(width, MENU_BAR + height))
        .child(menu_bar(p, width));
    for (index, window) in windows.iter().enumerate() {
        glass = glass.child(
            tile(p, &format!("window-{index}"))
                .absolute(window.x as f32 * scale, MENU_BAR + window.y as f32 * scale)
                .size(window.width as f32 * scale, window.height as f32 * scale),
        );
    }
    let mut bezel = el()
        .key(key)
        .role("image")
        .label(label)
        .padding(BEZEL)
        .radius(BEZEL_RADIUS)
        .fill(oklch(0.17, 0.006, 270.0))
        .border_gradient(p.hair, Color::WHITE.alpha(0.16), Color::BLACK.alpha(0.45))
        .child(glass);
    for shadow in [
        Shadow {
            y: 1.0,
            blur: 2.0,
            color: Color::BLACK.alpha(0.18),
            ..Shadow::default()
        },
        Shadow {
            y: 6.0,
            blur: 18.0,
            color: Color::BLACK.alpha(0.16),
            ..Shadow::default()
        },
    ] {
        bezel = bezel.box_shadow(shadow);
    }
    bezel
}

/// A dusk wallpaper, deep indigo into violet and teal, lit from the upper right so the gaps and
/// the padded edges read as depth rather than a flat tint.
fn wallpaper(width: f32, height: f32) -> El {
    let base = el()
        .key("wallpaper")
        .absolute(0.0, 0.0)
        .size(width, height)
        .fill(oklch(0.24, 0.07, 280.0))
        .linear_gradient(
            PI * 0.75,
            [
                (0.0, oklch(0.27, 0.09, 285.0)),
                (0.55, oklch(0.30, 0.10, 315.0)),
                (1.0, oklch(0.25, 0.06, 225.0)),
            ],
        );
    let glow = |key: &str, center: [f32; 2], tint: Color| {
        el().key(key)
            .absolute(0.0, 0.0)
            .size(width, height)
            .radial_gradient(
                center,
                [width * 0.6, height * 0.8],
                [(0.0, tint), (1.0, tint.alpha(0.0))],
            )
    };
    base.child(glow("glow", [0.78, 0.12], oklcha(0.62, 0.15, 330.0, 0.42)))
        .child(glow("pool", [0.1, 1.0], oklcha(0.55, 0.10, 210.0, 0.30)))
}

/// The menu bar: a translucent strip with a logo and a menu on the left, status items on the right.
fn menu_bar(p: Paint, width: f32) -> El {
    let middle = MENU_BAR / 2.0;
    let pill = |key: &str, x: f32, w: f32, alpha: f32| {
        el().key(key)
            .absolute(x, middle - 1.0)
            .size(w, 2.0)
            .radius(1.0)
            .fill(Color::WHITE.alpha(alpha))
    };
    el().key("menu-bar")
        .absolute(0.0, 0.0)
        .size(width, MENU_BAR)
        .fill(Color::BLACK.alpha(0.26))
        .border_sides([0.0, 0.0, p.hair, 0.0], Color::WHITE.alpha(0.06))
        .child(
            el().key("logo")
                .absolute(6.0, middle - 1.25)
                .size(2.5, 2.5)
                .radius(1.25)
                .fill(Color::WHITE.alpha(0.8)),
        )
        .child(pill("app", 11.0, 9.0, 0.55))
        .child(pill("menu", 22.0, 14.0, 0.3))
        .child(pill("status", width - 30.0, 8.0, 0.3))
        .child(pill("clock", width - 19.0, 13.0, 0.5))
}

/// A window: a hairline lit from above, a tinted title bar with muted traffic lights, and a
/// body in the theme's own surface colour. Its frame springs as the gaps change.
fn tile(p: Paint, key: &str) -> El {
    let c = &p.k.c;
    let lights = [
        oklch(0.68, 0.15, 25.0),
        oklch(0.80, 0.13, 85.0),
        oklch(0.72, 0.13, 145.0),
    ];
    let mut bar = el()
        .key("title-bar")
        .w_full()
        .h(TITLE_BAR)
        .fill(c.layer_02)
        .border_sides([0.0, 0.0, p.hair, 0.0], c.border_weak);
    for (index, color) in lights.into_iter().enumerate() {
        bar = bar.child(
            el().key(format!("light-{index}"))
                .absolute(
                    3.0 + index as f32 * (LIGHT + 1.0),
                    (TITLE_BAR - LIGHT) / 2.0,
                )
                .size(LIGHT, LIGHT)
                .radius(LIGHT / 2.0)
                .fill(color.alpha(0.85)),
        );
    }
    el().key(key)
        .radius(WINDOW_RADIUS)
        .fill(c.bg_base)
        .gradient(Color::WHITE.alpha(0.035), Color::WHITE.alpha(0.0))
        .border_gradient(p.hair, Color::WHITE.alpha(0.24), Color::BLACK.alpha(0.3))
        .box_shadow(Shadow {
            y: 1.0,
            blur: 4.0,
            color: Color::BLACK.alpha(0.35),
            ..Shadow::default()
        })
        .clip_rounded(WINDOW_RADIUS)
        .motion(SETTLE)
        .child(bar)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_tiles_meet_the_padded_edges_and_sit_one_gap_apart() {
        let spacing = Spacing {
            inner: 10,
            top: 30,
            ..Spacing::NONE
        };
        let [left, top, bottom] = three_tiles(LayoutMode::Dwindle, spacing, DISPLAY)[..] else {
            panic!("three tiles")
        };
        assert_eq!((left.x, left.y), (0, 30));
        assert_eq!(top.x - (left.x + left.width), 10);
        assert_eq!(bottom.y - (top.y + top.height), 10);
        assert_eq!(bottom.y + bottom.height, DISPLAY.height);
        assert_eq!(top.x + top.width, DISPLAY.width);

        let [first, middle, last] = three_tiles(LayoutMode::Columns, spacing, DISPLAY)[..] else {
            panic!("three columns")
        };
        assert_eq!((first.x, first.y), (0, 30));
        assert_eq!(first.height, DISPLAY.height - 30);
        assert_eq!(middle.height, first.height);
        assert_eq!(last.height, first.height);
        assert_eq!(middle.x - (first.x + first.width), 10);
        assert_eq!(last.x - (middle.x + middle.width), 10);
        assert_eq!(last.x + last.width, DISPLAY.width);
    }
}
