//! The tour's last step: a miniature desktop to learn four shortcuts on, one lesson at a time.
//! Each press plays on the stage (`stage.rs`); the press a lesson asks for ticks it and brings
//! the next one up (`prompt.rs`), and after the last the desktop stays free to play with. Only
//! the keyboard makes a move: nothing here is clicked through, nothing advances by itself, and
//! nothing moves a real window.

use fractile::{
    app::UiLaunchAtLogin,
    shortcuts::{Chord, Modifiers},
};
use rune::{El, Mods, PhysicalInput, PhysicalKeyCode, Tx, col, row, text};
use rune_kit::{Checkbox, SegmentedControl};

use super::{
    Input,
    demo::{self, Demo, Move, Pressed},
    flow::View,
    input, prompt, stage,
    view::title_block,
};
use crate::{
    Ui,
    controls::{Held, Paint},
    panes,
};

/// How long a pressed key stays down, in seconds on the window's clock.
pub const KEY_BEAT: f64 = 0.14;
/// How long a learned lesson keeps its tick before the next one is asked for.
pub const TICK_BEAT: f64 = 0.72;

/// What the prompt under the desktop is showing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Card {
    /// A lesson: being asked for, or just learned and wearing its tick.
    Lesson { index: usize, learned: bool },
    /// Every lesson is learned.
    Ready,
}

/// The miniature desktop, and what is pressed and showing on it right now. Its beats are
/// stamped with the window's clock, so a scripted run plays them exactly.
#[derive(Clone, Copy, Debug, Default)]
pub struct Play {
    pub demo: Demo,
    /// The move whose keys are down, and since when.
    down: Option<(Move, f64)>,
    /// The modifier keys held, while the stage has the keyboard.
    held: Modifiers,
    /// The lesson just learned, while its tick shows, and since when.
    ticked: Option<(usize, f64)>,
}

impl Play {
    /// Presses `mv`'s keys at `now`: the desktop makes the move and the lesson asking for this
    /// press gets its tick.
    pub fn press(&mut self, mv: Move, now: f64) -> Pressed {
        let asked = self.demo.learned();
        let pressed = self.demo.press(mv);
        self.down = Some((mv, now));
        if pressed.learned {
            self.ticked = Some((asked, now));
        }
        pressed
    }

    /// Lets a pressed key up and the tick go once each has had its beat.
    /// Returns whether anything changed.
    pub fn settle(&mut self, now: f64) -> bool {
        let mut changed = false;
        let mut end = |since: Option<f64>, beat: f64| {
            // As `due` adds them, so the wake it asks for always finds its beat over.
            let over = since.is_some_and(|at| now >= at + beat);
            changed |= over;
            over
        };
        if end(self.down.map(|(_, at)| at), KEY_BEAT) {
            self.down = None;
        }
        if end(self.ticked.map(|(_, at)| at), TICK_BEAT) {
            self.ticked = None;
        }
        changed
    }

    /// When the next beat ends, on the window's clock.
    pub fn due(&self) -> Option<f64> {
        [
            self.down.map(|(_, at)| at + KEY_BEAT),
            self.ticked.map(|(_, at)| at + TICK_BEAT),
        ]
        .into_iter()
        .flatten()
        .reduce(f64::min)
    }

    /// Everything at rest, as a scene draws it.
    pub fn rest(&mut self) {
        self.down = None;
        self.ticked = None;
        self.held = Modifiers::default();
    }

    pub fn card(&self) -> Card {
        match self.ticked {
            Some((index, _)) => Card::Lesson {
                index,
                learned: true,
            },
            None if self.demo.complete() => Card::Ready,
            None => Card::Lesson {
                index: self.demo.learned(),
                learned: false,
            },
        }
    }

    /// The modifier keys now held. Returns whether that changed.
    pub fn hold(&mut self, held: Modifiers) -> bool {
        let changed = self.held != held;
        self.held = held;
        changed
    }

    /// `chord`'s keys for `mv`, each with whether it is down: all of them while the move's
    /// press has its beat, and a modifier for as long as it is really held. Each modifier is
    /// itself from one chord to the next, and the last key is always the same key, whatever its
    /// legend.
    pub fn caps(&self, mv: Move, chord: Chord) -> Vec<Held> {
        let pressed = self.down.is_some_and(|(down, _)| down == mv);
        let modifiers = chord.modifiers.keycaps().len();
        chord
            .keycaps()
            .into_iter()
            .enumerate()
            .map(|(index, legend)| {
                let (id, held) = match legend.as_str() {
                    _ if index >= modifiers => ("action", false),
                    "⌃" => ("control", self.held.control),
                    "⌥" => ("option", self.held.option),
                    "⇧" => ("shift", self.held.shift),
                    "⌘" => ("command", self.held.command),
                    _ => ("modifier", false),
                };
                Held {
                    id,
                    legend,
                    down: pressed || held,
                }
            })
            .collect()
    }
}

/// The move a physical key press asks for, by the chord it spells on the active layout.
pub fn asked(ui: &Ui, key: PhysicalKeyCode, mods: Mods) -> Option<Move> {
    demo::asked(
        &demo::chords(ui),
        panes::shortcuts::pressed_chord(key, mods)?,
    )
}

/// The stage's keyboard, while it has focus: modifiers show on the keys as they are held, and
/// a move's chord makes the move. A chord the key tap also hears is left to the tap, so one
/// press never counts twice.
fn on_input(ui: &mut Ui, event: &PhysicalInput, tx: &mut Tx) -> bool {
    match *event {
        PhysicalInput::FocusLost => hold(ui, Modifiers::default()),
        PhysicalInput::Modifiers(mods) => hold(ui, panes::shortcuts::held_modifiers(mods)),
        PhysicalInput::Key {
            key,
            mods,
            pressed: true,
            repeat,
        } => {
            let Some(mv) = asked(ui, key, mods) else {
                return false;
            };
            let tapped = ui.tour.as_ref().is_some_and(|tour| tour.tap_hears(mv));
            if !repeat && !tapped {
                super::pressed(ui, mv, tx);
            }
            // Taken either way, so the chord types nothing.
            true
        }
        PhysicalInput::Key { .. } => false,
    }
}

fn hold(ui: &mut Ui, held: Modifiers) -> bool {
    ui.tour.as_mut().is_some_and(|tour| tour.play.hold(held))
}

pub fn view(ui: &Ui, p: Paint, view: &View, play: &Play, still: bool) -> El {
    let scene = stage::Scene {
        demo: play.demo,
        still,
    };
    let display = stage::stage(ui, p, scene)
        .focusable(true)
        .focus_ring(2.0, 3.0, p.k.c.border_focus)
        .on_physical_input(on_input);
    col()
        .key("try-it")
        .gap(12.0)
        .center()
        .child(
            col()
                .key("heading")
                .pb(8.0)
                .center()
                .child(title_block(p, "Try it", None)),
        )
        .child(display)
        .child(prompt::card(ui, p, play, still))
        .child(options(ui, p, view))
}

/// Two quiet settings under the prompt, on its edges: the modifier every shortcut holds, to
/// try another on the spot, and Launch at login.
fn options(ui: &Ui, p: Paint, view: &View) -> El {
    let shortcuts = ui.doc.shortcuts();
    let presets = panes::shortcuts::PRESETS;
    // With the built-in shortcuts off there is no base modifier to pick.
    let modifier = shortcuts.defaults.then(|| {
        let symbols = presets.map(|(label, _)| label.split(' ').next().unwrap_or(label));
        let selected = presets
            .iter()
            .position(|(_, modifier)| *modifier == shortcuts.modifier);
        row()
            .key("modifier")
            .gap(8.0)
            .center_y()
            .child(
                text("Modifier")
                    .key("label")
                    .font(p.row_title())
                    .color(p.muted()),
            )
            .child(
                SegmentedControl::new(&symbols, selected)
                    .label("Base modifier")
                    .fit()
                    .on_pick(|ui: &mut Ui, index: usize, tx: &mut Tx| {
                        if let Some((_, modifier)) = panes::shortcuts::PRESETS.get(index) {
                            super::set_modifier(ui, *modifier);
                        }
                        // The stage keeps the keyboard, so the new chord can be pressed.
                        tx.focus(stage::KEY, false);
                    })
                    .build(p.k)
                    .key("picker"),
            )
    });
    let mut login = Checkbox::new(view.launch_at_login).label("Launch at login");
    match ui.agent.as_ref().map(|agent| &agent.launch_at_login) {
        Some(UiLaunchAtLogin::NeedsApproval) => {
            login =
                login.description("Approve Fractile in System Settings › General › Login Items.");
        }
        Some(UiLaunchAtLogin::Failed(error)) => login = login.description(error.clone()),
        _ => {}
    }
    row()
        .key("options")
        .w(stage::DISPLAY_W)
        .min_h(28.0)
        .px(prompt::INSET)
        .center_y()
        .children(modifier)
        .child(row().key("space").grow(1.0))
        .child(
            login
                .on_toggle_tx(|ui: &mut Ui, tx: &mut Tx| {
                    input(ui, Input::ToggleLaunchAtLogin);
                    tx.focus(stage::KEY, false);
                })
                .build(p.k)
                .key("try-login"),
        )
}
