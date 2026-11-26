//! `[shortcuts]`: user-defined global shortcuts, parsed here without touching Carbon so the config
//! stays platform-neutral. `hotkeys.rs` turns each binding into a registration.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::{
    Axis, Direction, DisplaySelector, DisplayTarget, ResizeDirection, WorkspaceId, WorkspaceStep,
};

/// The parsed `[shortcuts]` table.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Shortcuts {
    /// Keep the built-in shortcuts that no binding overrides.
    pub defaults: bool,
    /// The modifiers every built-in shortcut holds (`modifier`, Option by default). The
    /// built-in Move shortcuts add Shift to it.
    pub modifier: Modifiers,
    /// User bindings in chord order. Each claims its chord before any built-in shortcut.
    pub bindings: Vec<Binding>,
    /// Register the Scratchpad Lab's shortcuts. Set from `[labs] scratchpad`, not this table.
    pub scratchpad: bool,
    /// Register `switch next|prev` and `move next|prev` bindings. Set from
    /// `[labs] workspace_steps`, not this table.
    pub workspace_steps: bool,
}

impl Default for Shortcuts {
    fn default() -> Self {
        Self {
            defaults: true,
            modifier: Modifiers::OPTION,
            bindings: Vec::new(),
            scratchpad: false,
            workspace_steps: false,
        }
    }
}

impl Shortcuts {
    /// Whether `other` registers different hotkeys: the same chords and commands spelled
    /// another way (`alt+h` or `option+H`) or in another order are no change.
    #[must_use]
    pub fn differs_from(&self, other: &Self) -> bool {
        fn meaning(shortcuts: &Shortcuts) -> Vec<(Chord, Option<String>)> {
            let mut bindings = shortcuts
                .bindings
                .iter()
                .map(|binding| {
                    (
                        binding.chord,
                        binding.command.as_ref().map(Command::description),
                    )
                })
                .collect::<Vec<_>>();
            bindings.sort();
            bindings
        }
        self.defaults != other.defaults
            || self.modifier != other.modifier
            || self.scratchpad != other.scratchpad
            || self.workspace_steps != other.workspace_steps
            || meaning(self) != meaning(other)
    }

    /// The command a binding gives `chord`: `Some(None)` when it is unbound with `none`.
    #[must_use]
    pub fn binding(&self, chord: Chord) -> Option<Option<&Command>> {
        self.bindings
            .iter()
            .find(|binding| binding.chord == chord)
            .map(|binding| binding.command.as_ref())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Binding {
    pub chord: Chord,
    /// `None` unbinds the chord, so no built-in shortcut claims it either.
    pub command: Option<Command>,
    /// The chord as written, for log messages.
    pub source: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord)]
pub struct Chord {
    pub modifiers: Modifiers,
    pub key: Key,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, PartialOrd, Ord)]
pub struct Modifiers {
    pub command: bool,
    pub control: bool,
    pub option: bool,
    pub shift: bool,
}

impl Modifiers {
    pub const OPTION: Self = Self {
        command: false,
        control: false,
        option: true,
        shift: false,
    };

    /// These modifiers with Shift held too.
    #[must_use]
    pub const fn with_shift(self) -> Self {
        Self {
            shift: true,
            ..self
        }
    }

    /// Whether a chord holding these can be a global shortcut: one of ⌥, ⌘ or ⌃.
    #[must_use]
    pub const fn can_bind(self) -> bool {
        self.option || self.command || self.control
    }

    /// In `config.toml` spelling, `ctrl+alt+shift+cmd`, without a trailing `+`.
    #[must_use]
    pub fn source(self) -> String {
        [
            (self.control, "ctrl"),
            (self.option, "alt"),
            (self.shift, "shift"),
            (self.command, "cmd"),
        ]
        .into_iter()
        .filter_map(|(held, name)| held.then_some(name))
        .collect::<Vec<_>>()
        .join("+")
    }

    /// The keycaps macOS prints for them, in its order: ⌃ ⌥ ⇧ ⌘.
    #[must_use]
    pub fn keycaps(self) -> Vec<String> {
        [
            (self.control, "⌃"),
            (self.option, "⌥"),
            (self.shift, "⇧"),
            (self.command, "⌘"),
        ]
        .into_iter()
        .filter(|(held, _)| *held)
        .map(|(_, glyph)| glyph.to_owned())
        .collect()
    }
}

impl Chord {
    /// The chord in `config.toml` spelling, which [`parse_chord`] reads back as the same chord.
    #[must_use]
    pub fn source(self) -> String {
        let key = match self.key {
            Key::Character(character) => character.to_string(),
            Key::Number(10) => "0".into(),
            Key::Number(number) => number.to_string(),
            Key::Code(code) => code_name(code).unwrap_or_else(|| format!("{code}")),
        };
        format!("{}+{key}", self.modifiers.source())
    }

    /// The chord's keycaps, modifiers first: `⌃ ⌥ H`, `⌥ ⇧ Space`, `⌘ ←`.
    #[must_use]
    pub fn keycaps(self) -> Vec<String> {
        let mut caps = self.modifiers.keycaps();
        caps.push(match self.key {
            Key::Character(character) => character.to_uppercase().collect(),
            Key::Number(10) => "0".into(),
            Key::Number(number) => number.to_string(),
            Key::Code(code) => code_keycap(code),
        });
        caps
    }
}

/// The name `config.toml` uses for a key that types no character.
fn code_name(code: u16) -> Option<String> {
    if let Some(index) = FUNCTION_KEY_CODES.iter().position(|known| *known == code) {
        return Some(format!("f{}", index + 1));
    }
    NAMED_KEY_CODES
        .iter()
        .find(|(_, known)| *known == code)
        .map(|(names, _)| names[0].to_owned())
}

fn code_keycap(code: u16) -> String {
    let glyph = match code_name(code).as_deref() {
        Some("left") => "←",
        Some("right") => "→",
        Some("down") => "↓",
        Some("up") => "↑",
        Some("return") => "↩",
        Some("tab") => "Tab",
        Some("space") => "Space",
        Some("backspace") => "⌫",
        Some("escape") => "esc",
        Some("forwarddelete") => "⌦",
        Some("home") => "↖",
        Some("end") => "↘",
        Some("pageup") => "⇞",
        Some("pagedown") => "⇟",
        Some(function) => return function.to_uppercase(),
        None => return format!("#{code}"),
    };
    glyph.into()
}

/// Parses `[shortcuts] modifier`: `alt`, `ctrl+alt` or `ctrl+alt+cmd`. Shift is left for the
/// built-in Move shortcuts, which add it.
pub fn parse_modifier(source: &str) -> Result<Modifiers, String> {
    let chord = parse_chord(&format!("{source}+a"))
        .map_err(|error| format!("modifier {source:?} {error}"))?;
    if chord.modifiers.shift {
        return Err(format!(
            "modifier {source:?} can't include shift; the built-in Move shortcuts add it"
        ));
    }
    Ok(chord.modifiers)
}

/// Whether `chord` is a built-in default shortcut under `base` (with `named_workspaces`
/// enabled) — i.e. a shortcut that moves when `[shortcuts] modifier` changes.
///
/// Fixed Lab chords such as Scratchpad's `alt+\` and `alt+shift+\` do not follow
/// `[shortcuts] modifier` and return `false`.
#[must_use]
pub fn is_built_in_chord(chord: Chord, base: Modifiers, named_workspaces: &[WorkspaceId]) -> bool {
    let shift = if chord.modifiers == base {
        false
    } else if chord.modifiers == base.with_shift() {
        true
    } else {
        return false;
    };
    match chord.key {
        Key::Number(1..=10) | Key::Code(0x30) => true,
        Key::Code(0x31) => shift,
        Key::Character(character) => {
            let is_named = named_workspaces.iter().any(|workspace| {
                workspace
                    .letter()
                    .is_some_and(|letter| letter.eq_ignore_ascii_case(&character))
            });
            if is_named {
                return true;
            }
            match character {
                'h' | 'j' | 'k' | 'l' => true,
                'f' => shift,
                '`' => true,
                '/' | '-' | '=' => !shift,
                _ => false,
            }
        }
        _ => false,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord)]
pub enum Key {
    /// A character, found on whatever key types it in the active keyboard layout.
    Character(char),
    /// A number-row key, 1 through 10 (the `0` key), by physical position.
    Number(u8),
    /// A key that types no character (arrows, Return, F-keys), by macOS virtual key code.
    Code(u16),
}

/// What a binding does. The vocabulary is `fractile ctl`'s.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Command {
    Switch(WorkspaceId),
    Move(WorkspaceId),
    MoveFollow(WorkspaceId),
    /// Show the next or previous workspace of the focused display (the Workspace Steps Lab).
    SwitchStep(WorkspaceStep),
    /// Send the focused window one workspace along its display (the Workspace Steps Lab).
    MoveStep(WorkspaceStep),
    MoveStepFollow(WorkspaceStep),
    /// Send the focused window's active tab to a workspace (`move-tab`).
    MoveTab(WorkspaceId),
    MoveTabFollow(WorkspaceId),
    BackAndForth,
    MoveWorkspace(DisplayTarget),
    Focus(Direction),
    FocusNext,
    FocusPrev,
    MoveDirection(Direction),
    Layout,
    Fullscreen,
    Float,
    Resize(ResizeDirection),
    /// Resize the focused window to this width (`Horizontal`) or height (`Vertical`) in points.
    ResizeTo(Axis, i32),
    Balance,
    Retile,
}

pub const COMMAND_USAGE: &str = "switch WORKSPACE|switch next|prev|move WORKSPACE [--follow]|move next|prev [--follow]|move-tab WORKSPACE [--follow]|back-and-forth|move-workspace [next|prev|DISPLAY]|focus DIR|focus next|focus prev|move-direction DIR|layout|fullscreen|float|resize shrink|resize grow|resize width|height POINTS|balance|retile|none";

/// `[shortcuts]` as written.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawShortcuts {
    #[serde(default = "default_true")]
    defaults: bool,
    #[serde(default)]
    modifier: Option<String>,
    #[serde(default)]
    bindings: BTreeMap<String, String>,
}

const fn default_true() -> bool {
    true
}

impl Default for RawShortcuts {
    fn default() -> Self {
        Self {
            defaults: true,
            modifier: None,
            bindings: BTreeMap::new(),
        }
    }
}

impl RawShortcuts {
    pub(crate) fn parse(self) -> Result<Shortcuts, String> {
        let modifier = match &self.modifier {
            Some(source) => parse_modifier(source).map_err(|error| format!("shortcuts.{error}"))?,
            None => Modifiers::OPTION,
        };
        let mut bindings: Vec<Binding> = Vec::with_capacity(self.bindings.len());
        for (chord_source, command_source) in self.bindings {
            let chord = parse_chord(&chord_source)
                .map_err(|error| format!("shortcut {chord_source:?}: {error}"))?;
            if let Some(existing) = bindings.iter().find(|binding| binding.chord == chord) {
                return Err(format!(
                    "shortcuts {:?} and {chord_source:?} are the same chord",
                    existing.source
                ));
            }
            let command = parse_command(&command_source)
                .map_err(|error| format!("shortcut {chord_source:?}: {error}"))?;
            bindings.push(Binding {
                chord,
                command,
                source: chord_source,
            });
        }
        // Automatic tiling needs at least one shortcut, so an empty keymap is a config error
        // rather than a silently paused agent.
        if !self.defaults && bindings.iter().all(|binding| binding.command.is_none()) {
            return Err(
                "shortcuts.defaults = false needs at least one binding with a command".into(),
            );
        }
        Ok(Shortcuts {
            defaults: self.defaults,
            modifier,
            bindings,
            scratchpad: false,
            workspace_steps: false,
        })
    }
}

/// Parses `alt+shift+left`, `cmd+ctrl+1`, `alt+-`, or `option+=`: modifiers and one key,
/// joined by `+`. A trailing `++` is the `+` key.
pub fn parse_chord(source: &str) -> Result<Chord, String> {
    let compact = source.split_whitespace().collect::<String>().to_lowercase();
    let (modifier_part, key_part) = if let Some(prefix) = compact.strip_suffix("++") {
        (prefix, "+")
    } else {
        match compact.rsplit_once('+') {
            Some((prefix, key)) => (prefix, key),
            None => ("", compact.as_str()),
        }
    };
    let mut modifiers = Modifiers::default();
    for name in modifier_part.split('+').filter(|name| !name.is_empty()) {
        let slot = match name {
            "alt" | "opt" | "option" => &mut modifiers.option,
            "cmd" | "command" => &mut modifiers.command,
            "ctrl" | "control" => &mut modifiers.control,
            "shift" => &mut modifiers.shift,
            other => {
                return Err(format!(
                    "unknown modifier {other:?}; expected alt, cmd, ctrl, or shift"
                ));
            }
        };
        if *slot {
            return Err(format!("modifier {name:?} appears twice"));
        }
        *slot = true;
    }
    if !modifiers.can_bind() {
        return Err("needs at least one of alt, cmd, or ctrl".into());
    }
    Ok(Chord {
        modifiers,
        key: parse_key(key_part)?,
    })
}

fn parse_key(name: &str) -> Result<Key, String> {
    if name.is_empty() {
        return Err("is missing a key".into());
    }
    let mut characters = name.chars();
    if let (Some(character), None) = (characters.next(), characters.next()) {
        return Ok(match character.to_digit(10).map(u8::try_from) {
            Some(Ok(0)) => Key::Number(10),
            Some(Ok(digit)) => Key::Number(digit),
            _ => Key::Character(character),
        });
    }
    if let Some(number) = name
        .strip_prefix('f')
        .and_then(|rest| rest.parse::<usize>().ok())
        && let Some(code) = number
            .checked_sub(1)
            .and_then(|index| FUNCTION_KEY_CODES.get(index))
    {
        return Ok(Key::Code(*code));
    }
    NAMED_KEY_CODES
        .iter()
        .find(|(names, _)| names.contains(&name))
        .map(|(_, code)| Key::Code(*code))
        .ok_or_else(|| format!("unknown key {name:?}"))
}

/// macOS virtual key codes (`kVK_*`) of the keys that type no character.
const NAMED_KEY_CODES: [(&[&str], u16); 14] = [
    (&["left"], 0x7b),
    (&["right"], 0x7c),
    (&["down"], 0x7d),
    (&["up"], 0x7e),
    (&["return", "enter"], 0x24),
    (&["tab"], 0x30),
    (&["space"], 0x31),
    (&["backspace", "delete"], 0x33),
    (&["escape", "esc"], 0x35),
    (&["forwarddelete"], 0x75),
    (&["home"], 0x73),
    (&["end"], 0x77),
    (&["pageup"], 0x74),
    (&["pagedown"], 0x79),
];

const FUNCTION_KEY_CODES: [u16; 12] = [
    0x7a, 0x78, 0x63, 0x76, 0x60, 0x61, 0x62, 0x64, 0x65, 0x6d, 0x67, 0x6f,
];

/// Parses a command in `fractile ctl` syntax, or `none` to unbind.
pub fn parse_command(source: &str) -> Result<Option<Command>, String> {
    let trimmed = source.trim();
    if let Some(rest) = trimmed
        .strip_prefix("move-workspace")
        .filter(|rest| rest.is_empty() || rest.starts_with(char::is_whitespace))
    {
        let target = match rest.trim() {
            "" | "next" => DisplayTarget::Next,
            "prev" => DisplayTarget::Prev,
            display => DisplayTarget::Selector(DisplaySelector::parse(display)),
        };
        return Ok(Some(Command::MoveWorkspace(target)));
    }
    let parts = trimmed.split_whitespace().collect::<Vec<_>>();
    let command = match parts.as_slice() {
        ["none"] => return Ok(None),
        ["switch", target] => match WorkspaceStep::parse(target) {
            Some(step) => Command::SwitchStep(step),
            None => Command::Switch(WorkspaceId::parse(target)?),
        },
        ["move", target] => match WorkspaceStep::parse(target) {
            Some(step) => Command::MoveStep(step),
            None => Command::Move(WorkspaceId::parse(target)?),
        },
        ["move", target, "--follow"] | ["move", "--follow", target] => {
            match WorkspaceStep::parse(target) {
                Some(step) => Command::MoveStepFollow(step),
                None => Command::MoveFollow(WorkspaceId::parse(target)?),
            }
        }
        ["move-tab", workspace] => Command::MoveTab(WorkspaceId::parse(workspace)?),
        ["move-tab", workspace, "--follow"] | ["move-tab", "--follow", workspace] => {
            Command::MoveTabFollow(WorkspaceId::parse(workspace)?)
        }
        ["back-and-forth"] => Command::BackAndForth,
        ["focus", "next"] => Command::FocusNext,
        ["focus", "prev"] => Command::FocusPrev,
        ["focus", direction] => Command::Focus(direction.parse()?),
        ["move-direction", direction] => Command::MoveDirection(direction.parse()?),
        ["layout"] => Command::Layout,
        ["fullscreen"] => Command::Fullscreen,
        ["float"] => Command::Float,
        ["resize", "shrink"] => Command::Resize(ResizeDirection::Shrink),
        ["resize", "grow"] => Command::Resize(ResizeDirection::Grow),
        ["resize", dimension, points] => {
            let (axis, points) = crate::parse_resize_to(dimension, points)?;
            Command::ResizeTo(axis, points)
        }
        ["balance"] => Command::Balance,
        ["retile"] => Command::Retile,
        _ => {
            return Err(format!(
                "unknown command {trimmed:?}; expected {COMMAND_USAGE}"
            ));
        }
    };
    Ok(Some(command))
}

impl Command {
    /// The workspace this command needs enabled, if any.
    #[must_use]
    pub const fn workspace(&self) -> Option<WorkspaceId> {
        match self {
            Self::Switch(workspace)
            | Self::Move(workspace)
            | Self::MoveFollow(workspace)
            | Self::MoveTab(workspace)
            | Self::MoveTabFollow(workspace) => Some(*workspace),
            _ => None,
        }
    }

    /// Whether this command belongs to the Workspace Steps Lab.
    #[must_use]
    pub const fn is_workspace_step(&self) -> bool {
        matches!(
            self,
            Self::SwitchStep(_) | Self::MoveStep(_) | Self::MoveStepFollow(_)
        )
    }

    /// A short description for logs, matching the built-in shortcuts' wording.
    #[must_use]
    pub fn description(&self) -> String {
        match self {
            Self::Switch(workspace) => format!("switch workspace {workspace}"),
            Self::Move(workspace) => format!("move window to workspace {workspace}"),
            Self::MoveFollow(workspace) => {
                format!("move window to workspace {workspace} and follow")
            }
            Self::SwitchStep(WorkspaceStep::Next) => "switch to next workspace on display".into(),
            Self::SwitchStep(WorkspaceStep::Prev) => {
                "switch to previous workspace on display".into()
            }
            Self::MoveStep(WorkspaceStep::Next) => {
                "move window to next workspace on display".into()
            }
            Self::MoveStep(WorkspaceStep::Prev) => {
                "move window to previous workspace on display".into()
            }
            Self::MoveStepFollow(WorkspaceStep::Next) => {
                "move window to next workspace on display and follow".into()
            }
            Self::MoveStepFollow(WorkspaceStep::Prev) => {
                "move window to previous workspace on display and follow".into()
            }
            Self::MoveTab(workspace) => format!("move tab to workspace {workspace}"),
            Self::MoveTabFollow(workspace) => {
                format!("move tab to workspace {workspace} and follow")
            }
            Self::BackAndForth => "workspace back-and-forth".into(),
            Self::MoveWorkspace(DisplayTarget::Next) => "move workspace to next display".into(),
            Self::MoveWorkspace(DisplayTarget::Prev) => "move workspace to previous display".into(),
            Self::MoveWorkspace(DisplayTarget::Selector(selector)) => {
                format!("move workspace to display {}", selector.as_str())
            }
            Self::Focus(direction) => format!("focus {direction}"),
            Self::FocusNext => "cycle workspace windows".into(),
            Self::FocusPrev => "cycle workspace windows backward".into(),
            Self::MoveDirection(direction) => format!("move window {direction}"),
            Self::Layout => "toggle layout".into(),
            Self::Fullscreen => "toggle fullscreen layout".into(),
            Self::Float => "toggle floating window".into(),
            Self::Resize(ResizeDirection::Shrink) => "shrink layout".into(),
            Self::Resize(ResizeDirection::Grow) => "grow layout".into(),
            Self::ResizeTo(Axis::Horizontal, points) => {
                format!("resize focused window to {points} wide")
            }
            Self::ResizeTo(Axis::Vertical, points) => {
                format!("resize focused window to {points} tall")
            }
            Self::Balance => "balance layout".into(),
            Self::Retile => "retile".into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chord(option: bool, shift: bool, control: bool, command: bool, key: Key) -> Chord {
        Chord {
            modifiers: Modifiers {
                command,
                control,
                option,
                shift,
            },
            key,
        }
    }

    #[test]
    fn chords_parse_modifiers_in_any_order_and_spelling() {
        assert_eq!(
            parse_chord("alt+left").unwrap(),
            chord(true, false, false, false, Key::Code(0x7b))
        );
        assert_eq!(
            parse_chord("Shift + Option + Ctrl + Down").unwrap(),
            chord(true, true, true, false, Key::Code(0x7d))
        );
        assert_eq!(
            parse_chord("cmd+ctrl+J").unwrap(),
            chord(false, false, true, true, Key::Character('j'))
        );
        assert_eq!(
            parse_chord("alt+shift+0").unwrap(),
            chord(true, true, false, false, Key::Number(10))
        );
        assert_eq!(
            parse_chord("alt+-").unwrap(),
            chord(true, false, false, false, Key::Character('-'))
        );
        assert_eq!(
            parse_chord("alt++").unwrap(),
            chord(true, false, false, false, Key::Character('+'))
        );
        assert_eq!(
            parse_chord("ctrl+f12").unwrap(),
            chord(false, false, true, false, Key::Code(0x6f))
        );
        assert_eq!(
            parse_chord("alt+return").unwrap(),
            parse_chord("alt+enter").unwrap()
        );
    }

    #[test]
    fn chords_reject_unknown_names_and_shift_only_chords() {
        for (source, message) in [
            ("super+q", "unknown modifier"),
            ("alt+banana", "unknown key"),
            ("shift+left", "needs at least one"),
            ("left", "needs at least one"),
            ("alt+alt+left", "appears twice"),
            ("alt+", "missing a key"),
            ("alt+f13", "unknown key"),
        ] {
            let error = parse_chord(source).unwrap_err();
            assert!(error.contains(message), "{source}: {error}");
        }
    }

    #[test]
    fn commands_use_the_ctl_vocabulary() {
        let three = WorkspaceId::new(3).unwrap();
        let s = WorkspaceId::named('S').unwrap();
        for (source, expected) in [
            ("switch 3", Some(Command::Switch(three))),
            ("move S", Some(Command::Move(s))),
            ("move S --follow", Some(Command::MoveFollow(s))),
            ("move --follow 3", Some(Command::MoveFollow(three))),
            ("move-tab 3", Some(Command::MoveTab(three))),
            ("move-tab S --follow", Some(Command::MoveTabFollow(s))),
            ("move-tab --follow 3", Some(Command::MoveTabFollow(three))),
            ("focus left", Some(Command::Focus(Direction::Left))),
            ("focus next", Some(Command::FocusNext)),
            ("focus prev", Some(Command::FocusPrev)),
            (
                "switch next",
                Some(Command::SwitchStep(WorkspaceStep::Next)),
            ),
            (
                "switch prev",
                Some(Command::SwitchStep(WorkspaceStep::Prev)),
            ),
            ("move next", Some(Command::MoveStep(WorkspaceStep::Next))),
            (
                "move prev --follow",
                Some(Command::MoveStepFollow(WorkspaceStep::Prev)),
            ),
            (
                "move --follow next",
                Some(Command::MoveStepFollow(WorkspaceStep::Next)),
            ),
            (
                "move-direction up",
                Some(Command::MoveDirection(Direction::Up)),
            ),
            ("layout", Some(Command::Layout)),
            ("float", Some(Command::Float)),
            ("balance", Some(Command::Balance)),
            (
                "resize width 1200",
                Some(Command::ResizeTo(Axis::Horizontal, 1200)),
            ),
            (
                "resize  height   480",
                Some(Command::ResizeTo(Axis::Vertical, 480)),
            ),
            (
                "move-workspace",
                Some(Command::MoveWorkspace(DisplayTarget::Next)),
            ),
            (
                "move-workspace prev",
                Some(Command::MoveWorkspace(DisplayTarget::Prev)),
            ),
            ("resize grow", Some(Command::Resize(ResizeDirection::Grow))),
            ("none", None),
        ] {
            assert_eq!(parse_command(source).unwrap(), expected, "{source}");
        }
        assert!(parse_command("explode").is_err());
        assert!(parse_command("switch 11").is_err());
        assert!(parse_command("switch previous").is_err());
        assert!(parse_command("move-tab next").is_err());
        assert!(parse_command("focus sideways").is_err());
        for invalid in [
            "resize width",
            "resize width -5",
            "resize width 99999",
            "balance now",
        ] {
            assert!(parse_command(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn table_rejects_two_spellings_of_one_chord() {
        let raw: RawShortcuts = toml::from_str(
            r#"
            [bindings]
            "alt+v" = "float"
            "option+V" = "layout"
            "#,
        )
        .unwrap();
        let error = raw.parse().unwrap_err();
        assert!(error.contains("same chord"), "{error}");
    }

    #[test]
    fn table_defaults_to_keeping_built_in_shortcuts() {
        let raw: RawShortcuts = toml::from_str(
            r#"
            [bindings]
            "alt+j" = "layout"
            "alt+h" = "none"
            "#,
        )
        .unwrap();
        let shortcuts = raw.parse().unwrap();
        assert!(shortcuts.defaults);
        assert_eq!(shortcuts.bindings.len(), 2);
        assert!(
            shortcuts
                .bindings
                .iter()
                .any(|binding| binding.source == "alt+h" && binding.command.is_none())
        );
    }

    #[test]
    fn chord_source_reads_back_as_the_same_chord() {
        for source in [
            "alt+h",
            "ctrl+alt+shift+cmd+left",
            "alt+0",
            "alt+7",
            "ctrl+f12",
            "alt+shift+space",
            "alt+-",
            "alt++",
            "cmd+alt+return",
            "alt+`",
        ] {
            let chord = parse_chord(source).unwrap();
            assert_eq!(parse_chord(&chord.source()).unwrap(), chord, "{source}");
        }
        assert_eq!(
            parse_chord("Shift+Option+H").unwrap().source(),
            "alt+shift+h"
        );
        assert_eq!(parse_chord("option+enter").unwrap().source(), "alt+return");
    }

    #[test]
    fn chord_keycaps_follow_macos_order_and_glyphs() {
        let caps = |source: &str| parse_chord(source).unwrap().keycaps();
        assert_eq!(caps("cmd+shift+alt+ctrl+h"), ["⌃", "⌥", "⇧", "⌘", "H"]);
        assert_eq!(caps("alt+shift+space"), ["⌥", "⇧", "Space"]);
        assert_eq!(caps("alt+left"), ["⌥", "←"]);
        assert_eq!(caps("alt+0"), ["⌥", "0"]);
        assert_eq!(caps("ctrl+f5"), ["⌃", "F5"]);
        assert_eq!(caps("alt+/"), ["⌥", "/"]);
    }

    #[test]
    fn modifier_names_the_built_in_shortcuts_base_and_refuses_shift() {
        let parse = |source: &str| {
            let raw: RawShortcuts = toml::from_str(source).unwrap();
            raw.parse()
        };
        assert_eq!(parse("").unwrap().modifier, Modifiers::OPTION);
        let hyper = parse(r#"modifier = "ctrl+alt+cmd""#).unwrap().modifier;
        assert!(hyper.control && hyper.option && hyper.command && !hyper.shift);
        for (source, message) in [
            (r#"modifier = "ctrl+shift""#, "can't include shift"),
            (r#"modifier = "shift""#, "needs at least one"),
            (r#"modifier = "hyper""#, "unknown modifier"),
        ] {
            let error = parse(source).unwrap_err();
            assert!(error.contains(message), "{source}: {error}");
        }
    }

    #[test]
    fn shortcuts_differ_only_when_they_register_something_else() {
        let parse = |source: &str| {
            let raw: RawShortcuts = toml::from_str(source).unwrap();
            raw.parse().unwrap()
        };
        let base = parse("[bindings]\n\"alt+h\" = \"none\"\n\"alt+v\" = \"float\"");
        let respelled = parse("[bindings]\n\"option+V\" = \"float\"\n\"alt+H\" = \"none\"");
        assert!(!base.differs_from(&respelled));
        assert!(!base.differs_from(&base.clone()));
        let rebound = parse("[bindings]\n\"alt+h\" = \"none\"\n\"alt+v\" = \"layout\"");
        assert!(base.differs_from(&rebound));
        let moved = parse("[bindings]\n\"alt+h\" = \"none\"\n\"ctrl+alt+v\" = \"float\"");
        assert!(base.differs_from(&moved));
        let modifier = parse(
            "modifier = \"ctrl+alt\"\n[bindings]\n\"alt+h\" = \"none\"\n\"alt+v\" = \"float\"",
        );
        assert!(base.differs_from(&modifier));
        assert!(Shortcuts::default().differs_from(&base));
        let scratchpad = Shortcuts {
            scratchpad: true,
            ..base.clone()
        };
        assert!(base.differs_from(&scratchpad));
    }

    #[test]
    fn is_built_in_chord_matches_base_shortcuts_and_excludes_fixed_scratchpad_chords() {
        let base = Modifiers::OPTION;
        let named = [
            WorkspaceId::named('A').unwrap(),
            WorkspaceId::named('F').unwrap(),
        ];
        for source in [
            "alt+1",
            "alt+shift+0",
            "alt+h",
            "alt+shift+l",
            "alt+shift+f",
            "alt+shift+space",
            "alt+tab",
            "alt+shift+tab",
            "alt+/",
            "alt+`",
            "alt+shift+`",
            "alt+-",
            "alt+=",
            "alt+a",
            "alt+shift+a",
            "alt+f",
        ] {
            assert!(
                is_built_in_chord(parse_chord(source).unwrap(), base, &named),
                "{source} should follow the base modifier"
            );
        }
        for source in [
            "alt+\\",
            "alt+shift+\\",
            "alt+space",
            "alt+shift+/",
            "alt+b",
            "ctrl+alt+h",
        ] {
            assert!(
                !is_built_in_chord(parse_chord(source).unwrap(), base, &named),
                "{source} should not follow the base modifier"
            );
        }
        // Without workspace F enabled, plain Option+F is not a built-in (only Option+Shift+F is).
        assert!(!is_built_in_chord(parse_chord("alt+f").unwrap(), base, &[]));
        assert!(is_built_in_chord(
            parse_chord("alt+shift+f").unwrap(),
            base,
            &[]
        ));
    }
}
