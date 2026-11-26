//! Notes, on this quarter's priorities, two of them already done.

use rune::{Color, El, Shape, TextStyle, col, el, oklch, row, text};

use super::{SMALL, TINY, TOOLBAR_H, glyphs, ink, lights, space, tool, words};

const SIDEBAR_W: f32 = 64.0;
fn sidebar() -> Color {
    oklch(0.245, 0.004, 80.0)
}
const HEADING: TextStyle = TextStyle::new(9.0, 720.0).lh(11.0).tracking(-0.15);
const ROW: TextStyle = TextStyle::new(4.8, 450.0).lh(6.0);
const SECTION: TextStyle = TextStyle::new(4.0, 650.0).lh(5.0);
const BOX: f32 = 5.6;

/// A notebook: its folders down the side, the traffic lights over them, then the open note's
/// date, title and checklist.
pub(super) fn notes(focused: bool) -> El {
    let amber = oklch(0.82, 0.15, 86.0);
    let folder = |key: &str, name: &str, count: &str, picked: bool| {
        row()
            .key(key)
            .w_full()
            .h(10.5)
            .px(4.0)
            .gap(3.0)
            .center_y()
            .radius(3.5)
            .fill(if picked { ink(0.1) } else { Color::TRANSPARENT })
            .child(glyphs::folder("glyph", amber))
            .child(
                el().key("name")
                    .grow(1.0)
                    .w(0.0)
                    .child(words("label", name, ROW, ink(0.86))),
            )
            .child(words("count", count, ROW, ink(0.38)))
    };
    let todo = |key: &str, task: &str, done: bool| {
        let check = Shape::new().stroke(
            vec![[1.5, 2.9], [2.4, 3.8], [4.1, 1.8]],
            0.7,
            oklch(0.2, 0.01, 80.0),
        );
        row()
            .key(key)
            .w_full()
            .gap(4.0)
            .center_y()
            .child(
                el().key("box")
                    .size(BOX, BOX)
                    .radius(BOX / 2.0)
                    .fill(if done { amber } else { Color::TRANSPARENT })
                    .border(0.6, if done { amber } else { ink(0.34) })
                    .shape(if done { check } else { Shape::new() }),
            )
            .child(el().key("task").grow(1.0).w(0.0).child(words(
                "text",
                task,
                SMALL,
                ink(if done { 0.5 } else { 0.88 }),
            )))
    };
    let icon = tool(focused);
    let sidebar = col()
        .key("sidebar")
        .w(SIDEBAR_W)
        .h_full()
        .fill(sidebar())
        .border_sides([0.0, 1.0, 0.0, 0.0], Color::BLACK.alpha(0.5))
        .child(
            row()
                .key("title-bar")
                .w_full()
                .h(TOOLBAR_H)
                .px(8.0)
                .center_y()
                .child(lights(focused)),
        )
        .child(
            col()
                .key("folders")
                .w_full()
                .px(3.0)
                .gap(0.5)
                .child(
                    words("section", "iCloud", SECTION, ink(0.38))
                        .ml(4.0)
                        .mb(2.0),
                )
                .child(folder("folder0", "All iCloud", "31", false))
                .child(folder("folder1", "Notes", "24", false))
                .child(folder("folder2", "Anomaly", "6", false))
                .child(folder("folder3", "Totally Fine", "1", true)),
        );
    let note = col()
        .key("note")
        .grow(1.0)
        .w(0.0)
        .h_full()
        .child(
            row()
                .key("toolbar")
                .w_full()
                .h(TOOLBAR_H)
                .px(8.0)
                .gap(7.0)
                .center_y()
                .child(glyphs::compose("compose", icon))
                .child(space("space"))
                .child(
                    text("Aa")
                        .key("format")
                        .font(TextStyle::new(5.6, 560.0).lh(7.0))
                        .color(icon),
                )
                .child(glyphs::checklist("checklist", icon))
                .child(glyphs::table("table", icon))
                .child(glyphs::share("share", icon)),
        )
        .child(
            col()
                .key("page")
                .w_full()
                .grow(1.0)
                .px(11.0)
                .pt(2.0)
                .gap(5.0)
                .child(row().key("dated").w_full().center().child(words(
                    "date",
                    "November 25, 2026 at 1:58 PM",
                    TINY,
                    ink(0.36),
                )))
                .child(words("heading", "Q4 Priorities", HEADING, ink(0.95)).mt(2.0))
                .child(todo("todo0", "Ship Fractile", true))
                .child(todo("todo1", "Mount the speaker above my chair", true))
                .child(todo("todo2", "Buy stronger wall anchors", false))
                .child(todo("todo3", "Call Mom back", false)),
        );
    row().key("panes").child(sidebar).child(note)
}
