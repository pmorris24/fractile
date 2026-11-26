//! A Ghostty-style terminal, where an agent has been asked to automate a whole life and has
//! taken it literally.

use rune::{Color, El, TextStyle, col, el, oklch, row};

use super::{LIGHT, TINY, TOOLBAR_H, ink, lights, space, words};

const TERM: TextStyle = TextStyle::new(5.0, 450.0).lh(7.2).mono();
const BOLD: TextStyle = TextStyle::new(5.0, 680.0).lh(7.2).mono();
const LINE_H: f32 = 7.2;

/// A check mark, from the system font: the monospace one has none.
const MARK: TextStyle = TextStyle::new(5.0, 700.0).lh(7.2);

/// A run of one colour and cut on a line.
#[derive(Clone, Copy)]
struct Run(&'static str, Color, TextStyle);

/// A terminal: a build, then one command typed at the prompt, the jobs it scheduled, and a
/// warning nobody will read until a quarter past two.
pub(super) fn terminal(focused: bool) -> El {
    let cyan = oklch(0.8, 0.085, 212.0);
    let green = oklch(0.8, 0.13, 152.0);
    let violet = oklch(0.76, 0.11, 305.0);
    let amber = oklch(0.84, 0.12, 82.0);
    let dim = ink(0.4);
    let text = ink(0.8);
    let bright = ink(0.96);
    let prompt = [
        Run("~/code/fractile", cyan, BOLD),
        Run(" on ", dim, TERM),
        Run("dev", violet, BOLD),
    ];
    let lines: [&[Run]; 17] = [
        &prompt,
        &[
            Run("\u{276f} ", green, BOLD),
            Run("cargo build --release", bright, TERM),
        ],
        &[
            Run("   Compiling", green, BOLD),
            Run(" fractile v0.3.4", text, TERM),
        ],
        &[
            Run("    Finished", green, BOLD),
            Run(" release [optimized] in 41.27s", text, TERM),
        ],
        &[],
        &prompt,
        &[
            Run("\u{276f} ", green, BOLD),
            Run("opencode run ", bright, TERM),
            Run("\"automate my whole life\"", amber, TERM),
        ],
        &[Run("  \u{25c7} Planning 3 jobs\u{2026}", dim, TERM)],
        &[
            Run("  \u{2713} ", green, MARK),
            Run("cron  water-fern        ", text, TERM),
            Run("daily 09:00", dim, TERM),
        ],
        &[
            Run("  \u{2713} ", green, MARK),
            Run("cron  text-mom          ", text, TERM),
            Run("sun 18:00", dim, TERM),
        ],
        &[
            Run("  \u{2713} ", green, MARK),
            Run("cron  standup-bass-drop ", text, TERM),
            Run("today 14:15", dim, TERM),
        ],
        &[
            Run("  ! ", amber, BOLD),
            Run("drywall load at 612% of rating", amber.alpha(0.8), TERM),
        ],
        &[Run("  Done in 4.2s. Have a great afternoon.", dim, TERM)],
        &[],
        &prompt,
        &[Run("\u{276f}", green, BOLD)],
        &[],
    ];
    let mut body = col()
        .key("lines")
        .w_full()
        .grow(1.0)
        .px(8.0)
        .pt(3.0)
        .gap(0.6);
    for (index, runs) in lines.iter().enumerate() {
        let mut line = row()
            .key(format!("line{index}"))
            .w_full()
            .h(LINE_H)
            .center_y();
        for (part, Run(said, color, style)) in runs.iter().enumerate() {
            line = line.child(words(&format!("part{part}"), said, *style, *color));
        }
        if index == 15 {
            // The block cursor, after the last prompt.
            line = line.child(
                el().key("cursor")
                    .ml(3.0)
                    .size(3.0, 6.0)
                    .fill(if focused {
                        ink(0.82)
                    } else {
                        Color::TRANSPARENT
                    })
                    .border(0.5, ink(0.6)),
            );
        }
        body = body.child(line);
    }
    col()
        .child(
            row()
                .key("title-bar")
                .w_full()
                .h(TOOLBAR_H - 4.0)
                .px(8.0)
                .center_y()
                .child(lights(focused))
                .child(space("lead"))
                .child(words(
                    "title",
                    "greg@macbook \u{2014} ~/code/fractile",
                    TINY,
                    ink(if focused { 0.62 } else { 0.38 }),
                ))
                .child(space("trail"))
                // As wide as the lights, so the title sits in the middle.
                .child(el().key("balance").w(3.0 * LIGHT + 6.4)),
        )
        .child(body)
}
