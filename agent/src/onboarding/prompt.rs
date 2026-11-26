//! The Try it step's prompt: one lesson at a time under the miniature desktop. It reads as a
//! shortcut's row in Settings does, what it does on the left and its keys on the right, with
//! the step's progress under the title. The card, its title and its keys stay put from one
//! lesson to the next; only what changes moves. The keys sink while they are pressed; after the
//! last lesson, the keys' place is taken by a seal that draws its own check.
//!
//! | What changes         | What moves                                                | Spring |
//! | -------------------- | --------------------------------------------------------- | ------ |
//! | A lesson is learned  | its tick pops in; its progress mark fills from the left   | .28 / .42, .40 / 0 |
//! | The next lesson      | the title rolls up to the next; the tray keeps its keys,  | .30 / 0, .22 / 0 |
//! |                      | a key the chord adds or drops springs in or out as its    | .18 / 0 |
//! |                      | neighbours part or close, and the last key's legend rolls |         |
//! | The last is learned  | the seal pops in, its check draws, a ring widens away     | .42 / .38, .34 / 0, .7 s |

use fractile::shortcuts::Chord;
use rune::{
    Beat, Easing, Edge, El, MotionIntent, Sequence, Shape, Spring, SpringPair, Transition, col, el,
    icon, row, text,
};

use super::{
    demo::{self, LESSONS},
    stage,
    try_it::{Card, Play},
};
use crate::{
    Ui,
    controls::{self, Paint},
};

/// From the card's edge to its title and to its keys.
pub const INSET: f32 = 16.0;
const CARD_H: f32 = 56.0;
const TITLE_H: f32 = 20.0;
const MARK_W: f32 = 12.0;
const MARK_H: f32 = 4.0;
const SEAL: f32 = 28.0;

/// A tick lands with a little bounce.
const TICK: Spring = Spring::new(0.28, 0.42);
const RISE: Spring = Spring::new(0.3, 0.0);
const LEAVE: Spring = Spring::new(0.22, 0.0);
/// A progress mark fills like a bar, without bounce.
const FILL: Spring = Spring::new(0.4, 0.0);
const SEAL_IN: Spring = Spring::new(0.42, 0.38);
const CHECK_DRAW: Spring = Spring::new(0.34, 0.0);
/// The ring the seal sends out as it lands: it widens and fades once.
const RIPPLE: [Beat; 3] = [
    Beat::new(0.0).opacity(0.0).scale(1.0),
    Beat::new(0.12).opacity(0.55).scale(1.15),
    Beat::new(0.82)
        .opacity(0.0)
        .scale(2.1)
        .easing(Easing::EASE_OUT),
];

/// The prompt under the desktop, as wide as the display it belongs to.
pub fn card(ui: &Ui, p: Paint, play: &Play, still: bool) -> El {
    let c = &p.k.c;
    let showing = play.card();
    let chords = demo::chords(ui);
    let (ask, keys, said) = match showing {
        Card::Lesson { index, learned } => {
            let lesson = LESSONS[index];
            let chord = chords[lesson.mv.index()];
            (
                title(p, index, lesson.title, learned),
                keys(p, play, lesson.mv, chord, &chords),
                format!(
                    "{}, {}, step {} of {}",
                    lesson.title,
                    chord.map_or_else(String::new, |chord| chord.keycaps().join(" ")),
                    index + 1,
                    LESSONS.len()
                ),
            )
        }
        Card::Ready => (
            title(p, LESSONS.len(), "You\u{2019}re ready", false),
            seal(p, still),
            "You\u{2019}re ready".to_owned(),
        ),
    };
    row()
        .key("prompt")
        .role("group")
        .label(said)
        .w(stage::DISPLAY_W)
        .h(CARD_H)
        .px(INSET)
        .gap(12.0)
        .center_y()
        .radius(p.k.radius.xl)
        .fill(c.layer_01)
        .border(p.hair, c.border_weak)
        .motion(RISE)
        .child(
            col()
                .key("ask")
                .grow(1.0)
                .w(0.0)
                .gap(7.0)
                .child(ask)
                .child(progress(p, play, showing)),
        )
        .child(
            row()
                .key("keys-slot")
                .h_full()
                .center_y()
                .justify_end()
                .child(keys),
        )
}

/// A lesson's words leave upward and the next rise into their place, inside the title's clip.
fn turn() -> Transition {
    controls::roll().springs(SpringPair::new(RISE, LEAVE))
}

/// What comes in at the end, where the keys were, or leaves them: a fade and a hair of scale.
fn swap() -> Transition {
    Transition::scale(0.96).springs(SpringPair::new(RISE, LEAVE))
}

fn pop() -> Transition {
    Transition::scale(0.2)
        .allow_small_scale()
        .springs(SpringPair::new(TICK, LEAVE))
}

/// What the lesson does, and its tick once it is learned. The title stays; its words roll
/// over to the next lesson's, and the tick slides along beside them as their width springs.
fn title(p: Paint, index: usize, label: &str, learned: bool) -> El {
    let tick = learned.then(|| {
        icon(p.k.glyph.check_small, 16.0)
            .key("tick")
            .color(p.success())
            .motion_intent(MotionIntent::OVERSHOOT)
            .transition(pop())
    });
    // The clip spans the whole row, so it cuts the rolling words only at the top and bottom,
    // never the end of a longer title arriving.
    row()
        .key("title")
        .w_full()
        .h(TITLE_H)
        .gap(6.0)
        .center_y()
        .clip_rounded(0.0)
        .child(
            text(label.to_owned())
                .key(format!("lesson{index}"))
                .font(p.strong())
                .color(p.text())
                .transition(turn()),
        )
        .children(tick)
}

/// The lesson's keys in their tray, each down while it is pressed or held. The tray is the
/// same from lesson to lesson, and its keys keep their places by which key they are. It sits
/// at the end of room for the longest chord the lessons ask for, flush with the card's inset,
/// so its right edge stays put while its width springs and the modifiers slide along.
fn keys(p: Paint, play: &Play, mv: demo::Move, chord: Option<Chord>, chords: &demo::Chords) -> El {
    let Some(chord) = chord else {
        return controls::chords(p, &[]).transition(swap());
    };
    let longest = chords
        .iter()
        .flatten()
        .copied()
        .map(Chord::keycaps)
        .max_by_key(|caps| (caps.len(), caps.concat().chars().count()))
        .unwrap_or_default();
    el().key("keys")
        .transition(swap())
        .child(
            controls::tray(p, "room", &longest)
                .opacity(0.0)
                .a11y_hidden(),
        )
        .child(controls::tray_held(p, "tray", &play.caps(mv, chord)).pin(
            None,
            Some(0.0),
            Some(0.0),
            None,
        ))
}

/// One mark for each lesson: filled as it is learned, a touch brighter for the one asked for.
fn progress(p: Paint, play: &Play, showing: Card) -> El {
    let c = &p.k.c;
    let learned = play.demo.learned();
    let asking = match showing {
        Card::Lesson {
            index,
            learned: false,
        } => Some(index),
        _ => None,
    };
    let mut marks = row().key("progress").h(MARK_H).gap(3.0).a11y_hidden();
    for index in 0..LESSONS.len() {
        // Green slides in from the left, inside the mark, as a bar fills.
        let fill = (index < learned).then(|| {
            el().key("done")
                .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
                .radius(MARK_H / 2.0)
                .fill(p.success())
                .transition(
                    Transition::move_edge(Edge::Leading).springs(SpringPair::new(FILL, LEAVE)),
                )
        });
        marks = marks.child(
            el().key(format!("mark{index}"))
                .size(MARK_W, MARK_H)
                .radius(MARK_H / 2.0)
                .clip_rounded(MARK_H / 2.0)
                .fill(if asking == Some(index) {
                    c.border_strong
                } else {
                    c.border_base
                })
                .children(fill),
        );
    }
    marks
}

/// Every lesson learned: a green seal pops in where the keys were, draws its check and sends
/// one ring out.
fn seal(p: Paint, still: bool) -> El {
    let green = p.success();
    let check = Shape::new().stroke(
        vec![[8.5, 14.5], [12.5, 18.5], [19.5, 10.0]],
        2.2,
        p.k.c.bg_deep,
    );
    let mut ripple = el()
        .key("ripple")
        .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
        .radius(SEAL / 2.0)
        .border(1.5, green)
        .opacity(0.0);
    if !still {
        ripple = ripple.sequence(Sequence::new(&RIPPLE).delay(0.1));
    }
    el().key("seal")
        .size(SEAL, SEAL)
        .transition(swap())
        .a11y_hidden()
        .child(ripple)
        .child(
            el().key("disc")
                .size(SEAL, SEAL)
                .radius(SEAL / 2.0)
                .fill(green)
                .motion_intent(MotionIntent::OVERSHOOT)
                .transition(
                    Transition::scale(0.3)
                        .allow_small_scale()
                        .springs(SpringPair::new(SEAL_IN, LEAVE)),
                )
                .child(
                    el().key("check")
                        .size(SEAL, SEAL)
                        .shape(check)
                        .enter_delay(0.14)
                        .transition(Transition::draw().springs(SpringPair::new(CHECK_DRAW, LEAVE))),
                ),
        )
}
