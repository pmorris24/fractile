//! Click-to-record: a shortcut's keys, pressed, listen for a new chord. The global hotkeys are
//! held off meanwhile (`host::set_shortcut_recording`), so the chord reaches the recorder rather
//! than moving windows. Esc cancels, ⌫ unbinds, and a chord without ⌥, ⌃ or ⌘, or one another
//! shortcut uses, keeps listening and says why.

use fractile::{
    WorkspaceId,
    shortcuts::{self, Chord, Modifiers, Shortcuts},
};
use rune::{Mods, PhysicalInput, PhysicalKeyCode};

use super::catalog::{self, Action, Occupant};
use crate::Ui;

/// What Settings › Shortcuts' recorder is doing.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Capture {
    #[default]
    Idle,
    /// Listening for `action`'s new chord, with the modifiers held so far and why the last
    /// chord was refused.
    Listening {
        action: &'static str,
        held: Modifiers,
        message: Option<String>,
    },
    /// `action` just took a new chord (or none).
    Saved { action: &'static str },
}

impl Capture {
    pub fn listening(&self) -> Option<&'static str> {
        match self {
            Self::Listening { action, .. } => Some(action),
            _ => None,
        }
    }
}

/// A key pressed while listening, before it is checked against `[shortcuts]`.
#[derive(Debug, PartialEq, Eq)]
pub enum Press {
    /// A modifier on its own: the held keys show it.
    Ignore,
    Cancel,
    Unbind,
    Chord(Chord),
    Refused(&'static str),
}

pub const NEEDS_MODIFIER: &str = "Add ⌥, ⌃ or ⌘";
pub const SHIFT_ALONE: &str = "⇧ alone isn't enough: add ⌥, ⌃ or ⌘";
const UNSUPPORTED: &str = "That key can't be a shortcut";

pub fn modifiers(mods: Mods) -> Modifiers {
    Modifiers {
        command: mods.logo || mods.cmd,
        control: mods.ctrl,
        option: mods.alt,
        shift: mods.shift,
    }
}

/// What pressing `key` with `mods` records. `character` names the character the active
/// keyboard layout types on a character key, so a binding follows the key's legend as
/// `config.toml`'s characters do.
pub fn press(
    key: PhysicalKeyCode,
    mods: Mods,
    character: impl Fn(PhysicalKeyCode) -> Option<char>,
) -> Press {
    use PhysicalKeyCode as K;
    let held = modifiers(mods);
    let name = match key {
        K::ShiftLeft
        | K::ShiftRight
        | K::ControlLeft
        | K::ControlRight
        | K::AltLeft
        | K::AltRight
        | K::SuperLeft
        | K::SuperRight
        | K::CapsLock
        | K::Fn => return Press::Ignore,
        K::Escape => return Press::Cancel,
        K::Backspace | K::Delete if !held.can_bind() => return Press::Unbind,
        _ => match key_name(key, character) {
            Some(name) => name,
            None => return Press::Refused(UNSUPPORTED),
        },
    };
    if !held.can_bind() {
        return Press::Refused(if held.shift {
            SHIFT_ALONE
        } else {
            NEEDS_MODIFIER
        });
    }
    match shortcuts::parse_chord(&format!("{}+{name}", held.source())) {
        Ok(chord) => Press::Chord(chord),
        Err(_) => Press::Refused(UNSUPPORTED),
    }
}

/// The chord pressing `key` with `mods` spells on the active keyboard layout, if it could be a
/// shortcut.
pub fn pressed_chord(key: PhysicalKeyCode, mods: Mods) -> Option<Chord> {
    match press(key, mods, layout_character) {
        Press::Chord(chord) => Some(chord),
        _ => None,
    }
}

fn key_name(
    key: PhysicalKeyCode,
    character: impl Fn(PhysicalKeyCode) -> Option<char>,
) -> Option<String> {
    use PhysicalKeyCode as K;
    let named = match key {
        K::Digit0 => "0",
        K::Digit1 => "1",
        K::Digit2 => "2",
        K::Digit3 => "3",
        K::Digit4 => "4",
        K::Digit5 => "5",
        K::Digit6 => "6",
        K::Digit7 => "7",
        K::Digit8 => "8",
        K::Digit9 => "9",
        K::ArrowLeft => "left",
        K::ArrowRight => "right",
        K::ArrowUp => "up",
        K::ArrowDown => "down",
        K::Enter | K::NumpadEnter => "return",
        K::Tab => "tab",
        K::Space => "space",
        K::Backspace => "backspace",
        K::Delete => "forwarddelete",
        K::Home => "home",
        K::End => "end",
        K::PageUp => "pageup",
        K::PageDown => "pagedown",
        K::F1 => "f1",
        K::F2 => "f2",
        K::F3 => "f3",
        K::F4 => "f4",
        K::F5 => "f5",
        K::F6 => "f6",
        K::F7 => "f7",
        K::F8 => "f8",
        K::F9 => "f9",
        K::F10 => "f10",
        K::F11 => "f11",
        K::F12 => "f12",
        _ => {
            let typed = character(key).or_else(|| us_character(key))?;
            return Some(typed.to_lowercase().collect());
        }
    };
    Some(named.into())
}

/// The character a US layout types on a character key, when the active layout can't say.
pub fn us_character(key: PhysicalKeyCode) -> Option<char> {
    use PhysicalKeyCode as K;
    let letters = [
        K::KeyA,
        K::KeyB,
        K::KeyC,
        K::KeyD,
        K::KeyE,
        K::KeyF,
        K::KeyG,
        K::KeyH,
        K::KeyI,
        K::KeyJ,
        K::KeyK,
        K::KeyL,
        K::KeyM,
        K::KeyN,
        K::KeyO,
        K::KeyP,
        K::KeyQ,
        K::KeyR,
        K::KeyS,
        K::KeyT,
        K::KeyU,
        K::KeyV,
        K::KeyW,
        K::KeyX,
        K::KeyY,
        K::KeyZ,
    ];
    if let Some(index) = letters.iter().position(|letter| *letter == key) {
        return char::from_u32(u32::from(b'a') + u32::try_from(index).ok()?);
    }
    Some(match key {
        K::Minus => '-',
        K::Equal => '=',
        K::BracketLeft => '[',
        K::BracketRight => ']',
        K::Backslash => '\\',
        K::Semicolon => ';',
        K::Quote => '\'',
        K::Comma => ',',
        K::Period => '.',
        K::Slash => '/',
        K::Backquote => '`',
        _ => return None,
    })
}

/// The character the active keyboard layout types on `key`, as Rune labels its keycap.
fn layout_character(key: PhysicalKeyCode) -> Option<char> {
    let label = rune::Shortcut::from_physical_key(key, Mods::default())
        .ok()?
        .keycaps()
        .pop()?;
    let mut characters = label.chars();
    match (characters.next(), characters.next()) {
        (Some(character), None) => Some(character),
        _ => None,
    }
}

/// Named workspaces enabled in `config.toml` or active in the live agent snapshot.
pub fn enabled_named_workspaces(ui: &Ui) -> Vec<WorkspaceId> {
    let mut named = ui.doc.named_workspaces();
    if let Some(agent) = &ui.agent {
        for &workspace in &agent.active_workspaces {
            if workspace.letter().is_some() && !named.contains(&workspace) {
                named.push(workspace);
            }
        }
    }
    named.sort();
    named
}

/// Whether `action` may take `chord`: refused while another shortcut runs it.
pub fn check(
    shortcuts: &Shortcuts,
    named_workspaces: &[WorkspaceId],
    action: &Action,
    chord: Chord,
) -> Result<(), String> {
    match catalog::occupant(shortcuts, named_workspaces, chord) {
        Some(Occupant::Action(other)) if other.id == action.id => Ok(()),
        Some(occupant) => Err(format!("Used by {}", occupant.title())),
        None => Ok(()),
    }
}

pub fn begin(ui: &mut Ui, action: &'static str) {
    if ui.shortcut_capture.listening() == Some(action) {
        cancel(ui);
        return;
    }
    ui.shortcut_capture = Capture::Listening {
        action,
        held: Modifiers::default(),
        message: None,
    };
    tell_agent(ui, true);
}

pub fn cancel(ui: &mut Ui) {
    if ui.shortcut_capture.listening().is_some() {
        ui.shortcut_capture = Capture::Idle;
        tell_agent(ui, false);
    }
}

/// The recorder's physical input while it listens; everything else passes by.
pub fn on_input(ui: &mut Ui, event: &PhysicalInput) -> bool {
    let Some(id) = ui.shortcut_capture.listening() else {
        return false;
    };
    match *event {
        PhysicalInput::FocusLost => cancel(ui),
        PhysicalInput::Modifiers(mods) => {
            if let Capture::Listening { held, message, .. } = &mut ui.shortcut_capture {
                *held = modifiers(mods);
                if held.can_bind() {
                    *message = None;
                }
            }
        }
        PhysicalInput::Key {
            key,
            mods,
            pressed: true,
            repeat: false,
        } => {
            let outcome = press(key, mods, layout_character);
            record(ui, id, outcome);
        }
        PhysicalInput::Key { .. } => {}
    }
    true
}

/// Saves what a press recorded, or keeps listening with why it was refused.
pub fn record(ui: &mut Ui, id: &'static str, outcome: Press) {
    let Some(action) = catalog::find(id) else {
        cancel(ui);
        return;
    };
    let shortcuts = ui.doc.shortcuts();
    let named = enabled_named_workspaces(ui);
    let default = action
        .default_chord(shortcuts.modifier)
        .filter(|chord| catalog::named_workspace_for_key(&named, chord.key).is_none());
    let refused = match outcome {
        Press::Ignore => return,
        Press::Cancel => {
            cancel(ui);
            return;
        }
        Press::Unbind => {
            save(ui, id, |doc| {
                doc.set_shortcut(action.command, default, None)
            });
            return;
        }
        Press::Chord(chord) => match check(&shortcuts, &named, action, chord) {
            Ok(()) => {
                save(ui, id, |doc| {
                    doc.set_shortcut(action.command, default, Some(chord));
                });
                return;
            }
            Err(message) => message,
        },
        Press::Refused(message) => message.to_owned(),
    };
    if let Capture::Listening { message, .. } = &mut ui.shortcut_capture {
        *message = Some(refused);
    }
}

fn save(ui: &mut Ui, id: &'static str, edit: impl FnOnce(&mut crate::rules::Doc)) {
    edit_shortcuts(ui, edit);
    let listening = ui.shortcut_capture.listening().is_some();
    ui.shortcut_capture = if ui.error.is_none() {
        Capture::Saved { action: id }
    } else {
        Capture::Idle
    };
    if listening {
        tell_agent(ui, false);
    }
}

/// Saves a `[shortcuts]` edit and hands the agent the new table, so it registers on the next
/// tick instead of when the config probe next looks.
pub fn edit_shortcuts(ui: &mut Ui, edit: impl FnOnce(&mut crate::rules::Doc)) {
    ui.edit(edit);
    #[cfg(target_os = "macos")]
    if ui.error.is_none() && !ui.fixture {
        crate::host::shortcuts_saved(ui.doc.shortcuts());
    }
}

fn tell_agent(ui: &Ui, recording: bool) {
    #[cfg(target_os = "macos")]
    if !ui.fixture {
        crate::host::set_shortcut_recording(recording);
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (ui, recording);
}

#[cfg(test)]
mod tests;
