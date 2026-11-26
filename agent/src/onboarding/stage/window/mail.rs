//! Mail, where the building has noticed.

use rune::{Color, El, TextStyle, col, el, oklch, row, text};

use super::{SMALL, TINY, glyphs, ink, space, tool, toolbar, words};

const LIST_W: f32 = 92.0;
fn list() -> Color {
    oklch(0.225, 0.003, 265.0)
}
const TITLE: TextStyle = TextStyle::new(5.8, 700.0).lh(7.0);
const SENDER: TextStyle = TextStyle::new(4.9, 680.0).lh(6.2);
const LINE: TextStyle = TextStyle::new(4.4, 450.0).lh(5.8);
const BODY: TextStyle = TextStyle::new(5.2, 430.0).lh(7.6);

/// A mail client: the inbox with one message picked, and that message open beside it.
pub(super) fn mail(focused: bool) -> El {
    let blue = oklch(0.58, 0.16, 256.0);
    let icon = tool(focused);
    let message = |key: &str,
                   from: &str,
                   time: &str,
                   subject: &str,
                   preview: &str,
                   picked: bool,
                   unread: bool| {
        let (strong, soft, faint) = if picked {
            (ink(0.98), ink(0.9), ink(0.72))
        } else {
            (ink(0.9), ink(0.76), ink(0.42))
        };
        row()
            .key(key)
            .w_full()
            .px(3.0)
            .py(4.0)
            .gap(2.5)
            .radius(4.0)
            .fill(if picked {
                blue.alpha(if focused { 1.0 } else { 0.55 })
            } else {
                Color::TRANSPARENT
            })
            .child(
                el().key("unread-slot").w(3.5).h(6.2).center().child(
                    el().key("unread")
                        .size(3.5, 3.5)
                        .radius(1.75)
                        .fill(if unread { blue } else { Color::TRANSPARENT }),
                ),
            )
            .child(
                col()
                    .key("text")
                    .grow(1.0)
                    .w(0.0)
                    .gap(0.5)
                    .child(
                        row()
                            .key("head")
                            .w_full()
                            .gap(3.0)
                            .center_y()
                            .child(
                                el().key("from-slot")
                                    .grow(1.0)
                                    .w(0.0)
                                    .child(words("from", from, SENDER, strong)),
                            )
                            .child(words("time", time, LINE, faint)),
                    )
                    .child(words("subject", subject, LINE, soft))
                    // Two lines of the message, cut where the second runs out.
                    .child(
                        el().key("preview-box")
                            .w_full()
                            .h(11.6)
                            .clip_rounded(0.0)
                            .child(
                                text(preview.to_owned())
                                    .key("preview")
                                    .w_full()
                                    .font(LINE)
                                    .color(faint)
                                    .wrap(),
                            ),
                    ),
            )
    };
    let rule = |key: &str| {
        el().key(key.to_owned())
            .w_full()
            .h(0.5)
            .ml(9.0)
            .fill(ink(0.08))
    };
    col()
        .child(toolbar(
            focused,
            list(),
            vec![
                col()
                    .key("mailbox")
                    .ml(2.0)
                    .child(words("title", "Inbox", TITLE, ink(0.92)))
                    .child(words("count", "47 messages, 3 unread", TINY.lh(5.0), ink(0.42))),
                space("lead"),
                glyphs::compose("compose", icon),
                glyphs::archive("archive", icon),
                glyphs::trash("trash", icon),
                glyphs::reply("reply", icon),
                row()
                    .key("search")
                    .w(30.0)
                    .h(10.0)
                    .px(3.0)
                    .gap(2.0)
                    .center_y()
                    .radius(5.0)
                    .fill(ink(0.07))
                    .child(glyphs::search("glass", oklch(0.6, 0.004, 265.0)))
                    .child(words("hint", "Search", TINY.lh(5.0), ink(0.38))),
            ],
        ))
        .child(
            row()
                .key("panes")
                .w_full()
                .grow(1.0)
                .child(
                    col()
                        .key("inbox")
                        .w(LIST_W)
                        .h_full()
                        .padding(3.0)
                        .gap(1.0)
                        .fill(list())
                        .border_sides([0.0, 1.0, 0.0, 0.0], Color::BLACK.alpha(0.5))
                        .child(message(
                            "mail0",
                            "Building Management",
                            "2:11 PM",
                            "Re: Apt 4B \u{2014} the bass",
                            "Hi Greg, the ceiling plaster in 3B is vibrating again. Please keep it down before 2:15.",
                            true,
                            false,
                        ))
                        .child(message(
                            "mail1",
                            "Mom",
                            "1:47 PM",
                            "Thanksgiving?",
                            "Are you coming home this year? Dad bought a new folding table and wants you to see it.",
                            false,
                            true,
                        ))
                        .child(rule("rule1"))
                        .child(message(
                            "mail2",
                            "Residents\u{2019} Board",
                            "12:30 PM",
                            "Vote result: 7\u{2013}0",
                            "The board has voted on the noise complaint from units 3B, 5B and the dentist downstairs.",
                            false,
                            true,
                        ))
                        .child(rule("rule2"))
                        .child(message(
                            "mail3",
                            "Fractile",
                            "Yesterday",
                            "Welcome to Fractile",
                            "Your windows have a place now. Press Option and a number to switch workspaces.",
                            false,
                            false,
                        )),
                )
                .child(reading()),
        )
}

/// The open message: who it is from, its subject, and what it says.
fn reading() -> El {
    let para = |key: &str, said: &str| {
        text(said.to_owned())
            .key(key.to_owned())
            .w_full()
            .font(BODY)
            .color(ink(0.8))
            .wrap()
    };
    col()
        .key("reading")
        .grow(1.0)
        .w(0.0)
        .h_full()
        .px(9.0)
        .pt(8.0)
        .gap(4.0)
        .child(
            row()
                .key("sender")
                .w_full()
                .gap(4.5)
                .child(
                    el().key("monogram")
                        .size(14.0, 14.0)
                        .radius(7.0)
                        .fill(oklch(0.55, 0.01, 265.0))
                        .gradient(oklch(0.64, 0.008, 265.0), oklch(0.46, 0.01, 265.0))
                        .center()
                        .child(
                            text("BM")
                                .key("initials")
                                .font(TextStyle::new(5.0, 650.0).lh(6.0))
                                .color(ink(0.95)),
                        ),
                )
                .child(
                    col()
                        .key("who")
                        .grow(1.0)
                        .w(0.0)
                        .gap(0.5)
                        .child(
                            row()
                                .key("head")
                                .w_full()
                                .gap(3.0)
                                .child(el().key("name-slot").grow(1.0).w(0.0).child(words(
                                    "name",
                                    "Building Management",
                                    SENDER,
                                    ink(0.94),
                                )))
                                .child(words("time", "2:11 PM", LINE, ink(0.42))),
                        )
                        .child(words("subject", "Re: Apt 4B \u{2014} the bass", LINE, ink(0.82)))
                        .child(words("to", "To: Greg", LINE, ink(0.42))),
                ),
        )
        .child(el().key("rule").w_full().h(0.5).my(2.0).fill(ink(0.1)))
        .child(para("body0", "Hi Greg,"))
        .child(para(
            "body1",
            "The ceiling plaster in 3B is vibrating again. Please keep the volume down, especially before 2:15.",
        ))
        .child(para("body2", "Thanks,"))
        .child(text("Building Management").key("sign").font(SMALL).color(ink(0.8)))
}
