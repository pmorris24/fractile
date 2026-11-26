//! The Try it step's miniature desktop as plain state: two workspaces of two windows each, the
//! four shortcuts that move it, and the seven lessons the step teaches with them, one at a time.
//! It is a sandbox: nothing here touches a real window.

#[cfg(test)]
mod tests;

use fractile::{
    WorkspaceId,
    shortcuts::{Chord, Key, Modifiers},
};

use crate::{Ui, panes};

/// The windows on the two workspaces, by the app each one is, in tile order: the first tiles on
/// the left and has focus to begin with.
pub const APPS: [[&str; 2]; 2] = [
    ["com.apple.Safari", "com.apple.Terminal"],
    ["com.apple.Notes", "com.apple.mail"],
];

/// A shortcut the step listens for.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Move {
    /// Switch to workspace 2, or, on it already, focus its next window.
    There,
    /// The same for workspace 1.
    Back,
    /// Toggle the focused window's floating.
    Float,
    /// Toggle Fullscreen Layout.
    Fullscreen,
}

impl Move {
    pub const ALL: [Self; 4] = [Self::There, Self::Back, Self::Float, Self::Fullscreen];

    pub const fn index(self) -> usize {
        match self {
            Self::There => 0,
            Self::Back => 1,
            Self::Float => 2,
            Self::Fullscreen => 3,
        }
    }
}

/// One thing to try: a shortcut, and what pressing it now does.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Lesson {
    pub mv: Move,
    pub title: &'static str,
}

/// The lessons in the order they are asked for. Float and Fullscreen Layout are each pressed
/// twice: once to make the change and once to undo it, and in between the workspace's own
/// number, pressed again, brings the window behind forward, as it does in Fractile.
pub const LESSONS: [Lesson; 7] = [
    lesson(Move::There, "Switch to workspace 2"),
    lesson(Move::Back, "Switch back to workspace 1"),
    lesson(Move::Float, "Float this window"),
    lesson(Move::Float, "Tile it again"),
    lesson(Move::Fullscreen, "Fill the screen"),
    lesson(Move::Back, "Cycle to the window behind"),
    lesson(Move::Fullscreen, "Back to tiles"),
];

/// The lesson that cycles the windows of a workspace in Fullscreen Layout.
const CYCLE: usize = 5;

const fn lesson(mv: Move, title: &'static str) -> Lesson {
    Lesson { mv, title }
}

/// How a workspace's focused window sits.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Mode {
    /// Both windows share the screen.
    #[default]
    Tiled,
    /// The focused window floats over the other, which takes the whole screen.
    Floated,
    /// Fullscreen Layout: the focused window fills the screen with the other behind it.
    Fullscreen,
}

/// One workspace: how it is laid out and which of its windows has focus.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Desk {
    pub mode: Mode,
    /// The focused window's place in `APPS`.
    pub focus: usize,
}

/// How a window sits in its frame.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Pose {
    Tiled,
    /// Floating above the tiles.
    Lifted,
    /// Behind the window that fills the screen.
    Behind,
}

/// A window's place on its workspace, in unit space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub pose: Pose,
}

const fn frame(x: f32, y: f32, w: f32, h: f32, pose: Pose) -> Frame {
    Frame { x, y, w, h, pose }
}

/// Where a workspace's two windows sit, in `APPS` order. Tiles keep their places whichever has
/// focus; floating lifts the focused one, and Fullscreen Layout puts it in front.
pub const fn frames(desk: Desk) -> [Frame; 2] {
    const WHOLE: Frame = frame(0.0, 0.0, 1.0, 1.0, Pose::Tiled);
    let [focused, other] = match desk.mode {
        Mode::Tiled => {
            return [
                frame(0.0, 0.0, 0.5, 1.0, Pose::Tiled),
                frame(0.5, 0.0, 0.5, 1.0, Pose::Tiled),
            ];
        }
        Mode::Floated => [frame(0.2, 0.13, 0.6, 0.7, Pose::Lifted), WHOLE],
        Mode::Fullscreen => [WHOLE, frame(0.0, 0.0, 1.0, 1.0, Pose::Behind)],
    };
    if desk.focus == 0 {
        [focused, other]
    } else {
        [other, focused]
    }
}

/// What a press did.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Pressed {
    /// The desktop changed.
    pub moved: bool,
    /// The press was the lesson being asked for, so the next one is up.
    pub learned: bool,
}

/// The miniature desktop and how many lessons have been learned on it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Demo {
    /// The workspace showing: 0 or 1.
    workspace: usize,
    desks: [Desk; 2],
    learned: usize,
}

impl Demo {
    pub const fn workspace(self) -> usize {
        self.workspace
    }

    pub const fn desk(self, workspace: usize) -> Desk {
        self.desks[workspace]
    }

    pub const fn mode(self, workspace: usize) -> Mode {
        self.desks[workspace].mode
    }

    /// How many lessons are done: the place in `LESSONS` of the one being asked for.
    pub const fn learned(self) -> usize {
        self.learned
    }

    pub const fn complete(self) -> bool {
        self.learned == LESSONS.len()
    }

    /// The lesson being asked for.
    pub fn next(self) -> Option<Lesson> {
        LESSONS.get(self.learned).copied()
    }

    /// Makes `mv` on the desktop. A switch to the workspace already showing brings focus to its
    /// next window, as a repeat press does in Fractile, unless a window floats there. Float and
    /// Fullscreen toggle, each taking over from the other. Every shortcut moves the desktop whichever lesson is up, and after the last;
    /// the lesson is learned when its own shortcut does what it asks for.
    pub fn press(&mut self, mv: Move) -> Pressed {
        let before = *self;
        match mv {
            Move::There => self.switch(1),
            Move::Back => self.switch(0),
            Move::Float => self.toggle(Mode::Floated),
            Move::Fullscreen => self.toggle(Mode::Fullscreen),
        }
        let moved = *self != before;
        let was = before.desks[before.workspace];
        let now = self.desks[self.workspace];
        let cycled = before.workspace == self.workspace && was.focus != now.focus;
        let learned = moved
            && self.next().is_some_and(|lesson| lesson.mv == mv)
            && match self.learned {
                0 | 1 => self.workspace != before.workspace,
                2 => now.mode == Mode::Floated,
                // Coming back out of each layout is its own lesson.
                3 => was.mode == Mode::Floated && now.mode == Mode::Tiled,
                4 => now.mode == Mode::Fullscreen,
                CYCLE => cycled && now.mode == Mode::Fullscreen,
                _ => was.mode == Mode::Fullscreen && now.mode == Mode::Tiled,
            };
        if learned {
            self.learned += 1;
        }
        Pressed { moved, learned }
    }

    fn switch(&mut self, workspace: usize) {
        let desk = &mut self.desks[workspace];
        if self.workspace != workspace {
            self.workspace = workspace;
        } else if desk.mode != Mode::Floated {
            desk.focus = (desk.focus + 1) % APPS[workspace].len();
        }
    }

    fn toggle(&mut self, to: Mode) {
        let mode = &mut self.desks[self.workspace].mode;
        *mode = if *mode == to { Mode::Tiled } else { to };
    }
}

/// The chord each move asks for, from the shortcuts in force.
pub type Chords = [Option<Chord>; 4];

pub fn chords(ui: &Ui) -> Chords {
    let shortcuts = ui.doc.shortcuts();
    let named = ui.doc.named_workspaces();
    [
        panes::shortcuts::switch_chord(&shortcuts, WorkspaceId::TWO),
        panes::shortcuts::switch_chord(&shortcuts, WorkspaceId::ONE),
        panes::shortcuts::action_chord(&shortcuts, &named, "float"),
        panes::shortcuts::action_chord(&shortcuts, &named, "fullscreen"),
    ]
}

/// The move `chord` asks for, if it is one of `chords`.
pub fn asked(chords: &Chords, chord: Chord) -> Option<Move> {
    Move::ALL
        .into_iter()
        .zip(chords)
        .find_map(|(mv, wanted)| (*wanted == Some(chord)).then_some(mv))
}

/// Each move's chord as the key tap reports a press: a virtual key and Carbon modifiers.
pub type Presses = [Option<(u32, u32)>; 4];

// Carbon's modifier bits.
const COMMAND: u32 = 1 << 8;
const SHIFT: u32 = 1 << 9;
const OPTION: u32 = 1 << 11;
const CONTROL: u32 = 1 << 12;
/// The number row's virtual key codes, 1 through 0.
const NUMBER_ROW: [u32; 10] = [18, 19, 20, 21, 23, 22, 26, 28, 25, 29];

fn carbon(modifiers: Modifiers) -> u32 {
    [
        (modifiers.command, COMMAND),
        (modifiers.shift, SHIFT),
        (modifiers.option, OPTION),
        (modifiers.control, CONTROL),
    ]
    .into_iter()
    .filter(|(held, _)| *held)
    .fold(0, |bits, (_, bit)| bits | bit)
}

/// How each of `chords` is pressed. Number-row and named keys have fixed key codes; a
/// character's key depends on the keyboard layout, which `on_layout` answers.
pub fn presses(chords: &Chords, on_layout: impl Fn(Chord) -> Option<(u32, u32)>) -> Presses {
    chords.map(|chord| {
        let chord = chord?;
        let code = match chord.key {
            Key::Number(number) => *NUMBER_ROW.get(usize::from(number).checked_sub(1)?)?,
            Key::Code(code) => u32::from(code),
            Key::Character(_) => return on_layout(chord),
        };
        Some((code, carbon(chord.modifiers)))
    })
}

/// Whether the key tap can hear a press: it reports only presses with Option held.
pub fn hearable(press: Option<(u32, u32)>) -> bool {
    press.is_some_and(|(_, modifiers)| modifiers & OPTION != 0)
}

/// The move a press the key tap heard asks for, if it is one of `presses`.
pub fn heard(presses: &Presses, code: u32, modifiers: u32) -> Option<Move> {
    Move::ALL
        .into_iter()
        .zip(presses)
        .find_map(|(mv, press)| (*press == Some((code, modifiers))).then_some(mv))
}
