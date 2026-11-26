//! Safari, on Hex's site, with the afternoon's other two tabs still open beside it.

use rune::{Align, Color, El, Shape, TextStyle, col, el, oklch, row, text};

use super::{SMALL, TINY, TOOLBAR_H, WINDOW, glyphs, ink, lights, space, tool, words};

/// The page's own near-black, a little warm.
pub(super) fn page_color() -> Color {
    oklch(0.145, 0.004, 60.0)
}
fn bar_color() -> Color {
    oklch(0.255, 0.003, 285.0)
}
fn tab_bar() -> Color {
    oklch(0.195, 0.003, 285.0)
}
fn tab() -> Color {
    oklch(0.28, 0.003, 285.0)
}
/// Hex's amber.
fn amber() -> Color {
    oklch(0.79, 0.16, 62.0)
}

const WORDMARK: TextStyle = TextStyle::new(6.2, 680.0).lh(8.0).tracking(-0.1);
const NAV: TextStyle = TextStyle::new(4.6, 500.0).lh(6.0);
const HEADLINE: TextStyle = TextStyle::new(15.0, 680.0).lh(16.0).tracking(-0.5);
const LEDE: TextStyle = TextStyle::new(5.4, 430.0).lh(7.4);
const BUTTON: TextStyle = TextStyle::new(4.8, 620.0).lh(6.0);
const FEATURE: TextStyle = TextStyle::new(4.9, 620.0).lh(6.5);
const BLURB: TextStyle = TextStyle::new(4.2, 430.0).lh(5.8);

/// A browser: the toolbar with its address field, three tabs, and the page. Lifted off the grid
/// the window is shorter, and the page's feature row gives up its place.
pub(super) fn safari(focused: bool, lifted: bool) -> El {
    col()
        .child(bar(focused))
        .child(tabs(focused))
        .child(page(lifted))
}

fn bar(focused: bool) -> El {
    let icon = tool(focused);
    let address = row()
        .key("address")
        .grow(6.0)
        .max_w(130.0)
        .h(12.5)
        .px(4.0)
        .gap(2.5)
        .center_y()
        .radius(4.0)
        .fill(ink(0.075))
        .child(el().key("lead").grow(1.0))
        .child(glyphs::lock("lock", oklch(0.62, 0.004, 265.0)))
        .child(
            text("example.com")
                .key("host")
                .font(TINY.lh(6.0))
                .color(ink(if focused { 0.88 } else { 0.55 })),
        )
        .child(el().key("trail").grow(1.0))
        .child(glyphs::refresh("refresh", oklch(0.6, 0.004, 265.0)));
    row()
        .key("toolbar")
        .w_full()
        .h(TOOLBAR_H)
        .px(8.0)
        .gap(6.0)
        .center_y()
        .fill(bar_color())
        .child(lights(focused))
        .child(glyphs::sidebar("sidebar", icon).ml(2.0))
        .child(
            row()
                .key("history")
                .gap(4.0)
                .center_y()
                .child(glyphs::chevron("back", false, icon))
                .child(glyphs::chevron("forward", true, tool(false))),
        )
        .child(space("lead"))
        .child(address)
        .child(space("trail"))
        .child(glyphs::share("share", icon))
        .child(glyphs::plus("new-tab", icon))
        .child(glyphs::tabs("tabs", icon))
}

/// The tab bar: this page, and two looked up on the way.
fn tabs(focused: bool) -> El {
    let tab = |key: &str, favicon: El, label: &str, active: bool| {
        row()
            .key(key)
            .grow(1.0)
            .w(0.0)
            .h(10.0)
            .px(5.0)
            .gap(3.0)
            .center()
            .radius(3.0)
            .fill(if active { tab() } else { Color::TRANSPARENT })
            .child(favicon)
            .child(words(
                "title",
                label,
                TINY,
                ink(if active && focused {
                    0.86
                } else if active {
                    0.6
                } else {
                    0.42
                }),
            ))
    };
    let favicon = |key: &str, color: Color| el().key(key).size(4.5, 4.5).radius(1.2).fill(color);
    row()
        .key("tab-bar")
        .w_full()
        .h(14.0)
        .px(4.0)
        .gap(2.0)
        .center_y()
        .fill(tab_bar())
        .border_sides([0.0, 0.0, 1.0, 0.0], Color::BLACK.alpha(0.55))
        .child(tab(
            "tab0",
            glyphs::hex_mark("favicon", 5.0, ink(0.85), amber()),
            "Hex \u{2014} Voice to Text",
            true,
        ))
        .child(tab(
            "tab1",
            favicon("favicon", oklch(0.32, 0.03, 265.0)),
            "Fractile \u{2014} For your windows",
            false,
        ))
        .child(tab(
            "tab2",
            favicon("favicon", oklch(0.62, 0.15, 45.0)),
            "Drywall Anchors, 15 lb (4-Pack)",
            false,
        ))
}

fn page(lifted: bool) -> El {
    let centered = |key: &str, label: &str, style: TextStyle, color: Color| {
        text(label.to_owned())
            .key(key)
            .w_full()
            .font(style)
            .color(color)
            .align(Align::Center)
            .wrap()
    };
    col()
        .key("page")
        .w_full()
        .grow(1.0)
        .px(12.0)
        .center_y()
        // The warm light Hex's page sits in, behind its headline.
        .child(
            el().key("glow")
                .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
                .radial_gradient(
                    [0.5, 0.3],
                    [150.0, 92.0],
                    [
                        (0.0, amber().alpha(0.2)),
                        (0.35, amber().alpha(0.11)),
                        (0.7, amber().alpha(0.03)),
                        (1.0, amber().alpha(0.0)),
                    ],
                ),
        )
        .child(nav())
        .child(
            row()
                .key("eyebrow")
                .mt(10.0)
                .h(9.0)
                .px(4.5)
                .gap(2.5)
                .center_y()
                .radius(4.5)
                .fill(ink(0.04))
                .border(0.5, ink(0.1))
                .child(el().key("dot").size(2.5, 2.5).radius(1.25).fill(amber()))
                .child(words(
                    "label",
                    "Hex 2 is here \u{2014} see what\u{2019}s new",
                    TINY.lh(5.5),
                    ink(0.7),
                )),
        )
        .child(centered("headline", "Speak. It appears.", HEADLINE, ink(0.97)).mt(6.0))
        .child(
            el().key("lede-box")
                .w_full()
                .max_w(150.0)
                .mt(4.0)
                .child(centered(
                    "lede",
                    "Hold a key, say it, let go. Your words land in any app.",
                    LEDE,
                    ink(0.56),
                )),
        )
        .child(actions().mt(8.0))
        .child(
            centered(
                "meta",
                "macOS 14+ \u{b7} Free & Open Source",
                BLURB,
                ink(0.36),
            )
            .mt(4.0),
        )
        .child(dictation().mt(10.0))
        .child(
            features()
                .mt(12.0)
                .opacity(if lifted { 0.0 } else { 1.0 })
                .opacity_motion(WINDOW),
        )
}

fn nav() -> El {
    let link = |key: &str, label: &str| words(key, label, NAV, ink(0.58));
    row()
        .key("nav")
        .w_full()
        .h(18.0)
        .gap(7.0)
        .center_y()
        .child(
            row()
                .key("wordmark")
                .gap(2.5)
                .center_y()
                .child(glyphs::hex_mark("mark", 7.5, ink(0.92), amber()))
                .child(words("name", "Hex", WORDMARK, ink(0.95))),
        )
        .child(space("space"))
        .child(link("features", "Features"))
        .child(link("pricing", "Pricing"))
        .child(link("download", "Download"))
}

fn actions() -> El {
    row()
        .key("actions")
        .gap(5.0)
        .center_y()
        .child(
            row()
                .key("download")
                .h(12.0)
                .px(6.5)
                .gap(2.5)
                .center_y()
                .radius(6.0)
                .fill(ink(0.95))
                .child(glyphs::apple("apple", 5.0, Color::BLACK.alpha(0.88)))
                .child(words(
                    "label",
                    "Download for Mac",
                    BUTTON,
                    Color::BLACK.alpha(0.88),
                )),
        )
        .child(
            row()
                .key("github")
                .h(12.0)
                .px(6.5)
                .center_y()
                .radius(6.0)
                .fill(ink(0.05))
                .border(0.5, ink(0.16))
                .child(words("label", "View on GitHub", BUTTON, ink(0.86))),
        )
}

/// The product, working: a message being dictated into a text field, and Hex's own capsule
/// under it, listening.
fn dictation() -> El {
    let bars: [f32; 15] = [
        0.25, 0.5, 0.8, 0.45, 1.0, 0.7, 0.35, 0.9, 0.6, 0.3, 0.75, 0.5, 0.2, 0.4, 0.15,
    ];
    let mut wave = Shape::new();
    for (index, level) in bars.iter().enumerate() {
        let x = 0.6 + index as f32 * 1.5;
        let half = 0.4 + level * 2.2;
        wave = wave.line(
            [x, 3.0 - half],
            [x, 3.0 + half],
            0.75,
            rune::LineCap::Round,
            oklch(0.93, 0.03, 70.0),
        );
    }
    col()
        .key("dictation")
        .w_full()
        .max_w(178.0)
        .padding(6.0)
        .gap(6.0)
        .center_y()
        .radius(6.0)
        .fill(ink(0.035))
        .border(0.5, ink(0.09))
        .child(
            row()
                .key("field")
                .w_full()
                .h(13.0)
                .px(5.0)
                .gap(3.0)
                .center_y()
                .radius(6.5)
                .fill(oklch(0.2, 0.004, 60.0))
                .border(0.5, ink(0.1))
                .child(
                    el().key("avatar")
                        .size(6.0, 6.0)
                        .radius(3.0)
                        .fill(oklch(0.6, 0.1, 230.0))
                        .gradient(oklch(0.68, 0.1, 220.0), oklch(0.5, 0.12, 250.0)),
                )
                .child(words(
                    "said",
                    "Can we move standup to 2:15?",
                    SMALL.lh(7.0),
                    ink(0.86),
                ))
                .child(el().key("caret").size(0.7, 6.0).fill(amber())),
        )
        .child(
            row()
                .key("capsule")
                .h(11.0)
                .px(5.0)
                .gap(3.0)
                .center_y()
                .radius(5.5)
                .fill(oklch(0.1, 0.004, 60.0))
                .border(0.5, ink(0.14))
                .child(
                    el().key("live")
                        .size(3.0, 3.0)
                        .radius(1.5)
                        .fill(amber())
                        .box_shadow(rune::Shadow {
                            blur: 4.0,
                            color: amber().alpha(0.7),
                            ..rune::Shadow::default()
                        }),
                )
                .child(el().key("wave").size(22.0, 6.0).shape(wave))
                .child(words("hint", "Listening", TINY.lh(5.5), ink(0.6))),
        )
}

fn features() -> El {
    let feature = |key: &str, title: &str, blurb: &str| {
        col()
            .key(key)
            .grow(1.0)
            .w(0.0)
            .gap(1.5)
            .child(
                el().key("rule")
                    .w(8.0)
                    .h(0.8)
                    .radius(0.4)
                    .fill(amber().alpha(0.8)),
            )
            .child(words("title", title, FEATURE, ink(0.88)))
            .child(
                text(blurb.to_owned())
                    .key("blurb")
                    .w_full()
                    .font(BLURB)
                    .color(ink(0.46))
                    .wrap(),
            )
    };
    row()
        .key("features")
        .w_full()
        .max_w(200.0)
        .gap(9.0)
        .child(feature(
            "private",
            "On your Mac",
            "Transcribed on device. Nothing leaves it.",
        ))
        .child(feature(
            "anywhere",
            "In any app",
            "Types wherever your cursor is.",
        ))
        .child(feature(
            "quick",
            "Instant",
            "Words on screen as you finish speaking.",
        ))
}
