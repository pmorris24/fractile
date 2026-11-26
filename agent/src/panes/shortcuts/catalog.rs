//! The shortcuts Settings › Shortcuts can rebind, each with its built-in chord, and what a
//! chord does now under `[shortcuts]`.

use fractile::{
    WorkspaceId,
    shortcuts::{self, Chord, Command, Key, Modifiers, Shortcuts},
};

/// One rebindable shortcut.
#[derive(Debug, PartialEq, Eq)]
pub struct Action {
    /// Stable, for element keys and the recorder.
    pub id: &'static str,
    pub title: &'static str,
    /// In `fractile ctl` syntax, as `[shortcuts.bindings]` writes it.
    pub command: &'static str,
    /// The built-in chord's key, and whether it adds Shift to the base modifier.
    pub default: Option<(bool, Key)>,
}

impl Action {
    pub fn parsed(&self) -> Option<Command> {
        shortcuts::parse_command(self.command).ok().flatten()
    }

    /// The built-in chord under `modifier`, if this shortcut has one.
    pub fn default_chord(&self, modifier: Modifiers) -> Option<Chord> {
        self.default.map(|(shift, key)| Chord {
            modifiers: if shift {
                modifier.with_shift()
            } else {
                modifier
            },
            key,
        })
    }

    /// The built-in chord under `[shortcuts]`'s modifier, while the built-ins are on and no
    /// enabled named workspace claims its letter.
    pub fn active_default(
        &self,
        shortcuts: &Shortcuts,
        named_workspaces: &[WorkspaceId],
    ) -> Option<Chord> {
        self.default_chord(shortcuts.modifier).filter(|chord| {
            shortcuts.defaults && named_workspace_for_key(named_workspaces, chord.key).is_none()
        })
    }

    /// Every chord that runs this shortcut: its bindings, then its built-in chord unless a
    /// binding or enabled named workspace took that over.
    pub fn chords(&self, shortcuts: &Shortcuts, named_workspaces: &[WorkspaceId]) -> Vec<Chord> {
        let command = self.parsed();
        let mut chords = shortcuts
            .bindings
            .iter()
            .filter(|binding| binding.command.is_some() && binding.command == command)
            .map(|binding| binding.chord)
            .collect::<Vec<_>>();
        if let Some(default) = self.active_default(shortcuts, named_workspaces)
            && shortcuts.binding(default).is_none()
            && !chords.contains(&default)
        {
            chords.push(default);
        }
        chords
    }
}

const fn action(
    id: &'static str,
    title: &'static str,
    command: &'static str,
    default: Option<(bool, Key)>,
) -> Action {
    Action {
        id,
        title,
        command,
        default,
    }
}

const TAB: Key = Key::Code(0x30);
const SPACE: Key = Key::Code(0x31);
const fn letter(shift: bool, character: char) -> Option<(bool, Key)> {
    Some((shift, Key::Character(character)))
}

pub const WORKSPACE_ACTIONS: &[Action] = &[
    action(
        "back-and-forth",
        "Previous workspace",
        "back-and-forth",
        Some((false, TAB)),
    ),
    action(
        "move-workspace",
        "Move workspace to next display",
        "move-workspace next",
        Some((true, TAB)),
    ),
];

/// The Workspace Steps Lab's shortcuts, listed while `[labs] workspace_steps` is on. None has
/// a built-in chord.
pub const WORKSPACE_STEP_ACTIONS: &[Action] = &[
    action(
        "switch-next",
        "Step to next workspace on display",
        "switch next",
        None,
    ),
    action(
        "switch-prev",
        "Step to previous workspace on display",
        "switch prev",
        None,
    ),
    action(
        "move-next",
        "Move window to next workspace on display",
        "move next",
        None,
    ),
    action(
        "move-prev",
        "Move window to previous workspace on display",
        "move prev",
        None,
    ),
];

pub const LAYOUT_ACTIONS: &[Action] = &[
    action(
        "focus-next",
        "Next window",
        "focus next",
        letter(false, '`'),
    ),
    action(
        "focus-prev",
        "Previous window",
        "focus prev",
        letter(true, '`'),
    ),
    action("focus-left", "Focus left", "focus left", letter(false, 'h')),
    action("focus-down", "Focus down", "focus down", letter(false, 'j')),
    action("focus-up", "Focus up", "focus up", letter(false, 'k')),
    action(
        "focus-right",
        "Focus right",
        "focus right",
        letter(false, 'l'),
    ),
    action(
        "move-left",
        "Move left",
        "move-direction left",
        letter(true, 'h'),
    ),
    action(
        "move-down",
        "Move down",
        "move-direction down",
        letter(true, 'j'),
    ),
    action("move-up", "Move up", "move-direction up", letter(true, 'k')),
    action(
        "move-right",
        "Move right",
        "move-direction right",
        letter(true, 'l'),
    ),
    action("layout", "Flip split", "layout", letter(false, '/')),
    action("shrink", "Shrink tile", "resize shrink", letter(false, '-')),
    action("grow", "Grow tile", "resize grow", letter(false, '=')),
    action(
        "float",
        "Toggle floating window",
        "float",
        Some((true, SPACE)),
    ),
    action(
        "fullscreen",
        "Fullscreen layout",
        "fullscreen",
        letter(true, 'f'),
    ),
    action("balance", "Balance layout", "balance", None),
    action("retile", "Retile", "retile", None),
];

pub fn all() -> impl Iterator<Item = &'static Action> {
    WORKSPACE_ACTIONS
        .iter()
        .chain(WORKSPACE_STEP_ACTIONS)
        .chain(LAYOUT_ACTIONS)
}

pub fn find(id: &str) -> Option<&'static Action> {
    all().find(|action| action.id == id)
}

/// What `chord` runs now: a shortcut in the catalog, or another command by its name.
#[derive(Debug, PartialEq, Eq)]
pub enum Occupant {
    Action(&'static Action),
    Other(String),
}

impl Occupant {
    pub fn title(&self) -> &str {
        match self {
            Self::Action(action) => action.title,
            Self::Other(title) => title,
        }
    }
}

/// The enabled named workspace whose letter `key` types, if any.
pub fn named_workspace_for_key(named_workspaces: &[WorkspaceId], key: Key) -> Option<WorkspaceId> {
    let Key::Character(character) = key else {
        return None;
    };
    named_workspaces.iter().copied().find(|workspace| {
        workspace
            .letter()
            .is_some_and(|letter| letter.eq_ignore_ascii_case(&character))
    })
}

/// What `chord` runs under `shortcuts` and `named_workspaces`, if anything.
pub fn occupant(
    shortcuts: &Shortcuts,
    named_workspaces: &[WorkspaceId],
    chord: Chord,
) -> Option<Occupant> {
    if let Some(bound) = shortcuts.binding(chord) {
        let command = bound?;
        return Some(
            all()
                .find(|action| action.parsed().as_ref() == Some(command))
                .map_or_else(
                    || Occupant::Other(sentence(&command.description())),
                    Occupant::Action,
                ),
        );
    }
    if shortcuts.scratchpad
        && chord.key == Key::Character('\\')
        && !chord.modifiers.control
        && !chord.modifiers.command
        && chord.modifiers.option
    {
        return Some(Occupant::Other(
            if chord.modifiers.shift {
                "Move window to or from the scratchpad"
            } else {
                "Show or hide the scratchpad"
            }
            .into(),
        ));
    }
    if !shortcuts.defaults {
        return None;
    }
    // Enabled named workspaces take precedence over built-in letter shortcuts when they overlap.
    if let Some(workspace) = named_workspace_for_key(named_workspaces, chord.key) {
        if chord.modifiers == shortcuts.modifier {
            return Some(Occupant::Other(format!("Switch to workspace {workspace}")));
        }
        if chord.modifiers == shortcuts.modifier.with_shift() {
            return Some(Occupant::Other(format!(
                "Move window to workspace {workspace}"
            )));
        }
    }
    if let Some(action) =
        all().find(|action| action.default_chord(shortcuts.modifier) == Some(chord))
    {
        return Some(Occupant::Action(action));
    }
    let Key::Number(number) = chord.key else {
        return None;
    };
    if chord.modifiers == shortcuts.modifier {
        Some(Occupant::Other(format!("Switch to workspace {number}")))
    } else if chord.modifiers == shortcuts.modifier.with_shift() {
        Some(Occupant::Other(format!(
            "Move window to workspace {number}"
        )))
    } else {
        None
    }
}

fn sentence(description: &str) -> String {
    let mut characters = description.chars();
    characters.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(characters).collect()
    })
}

/// The base modifiers Settings offers, with their segment labels.
pub const PRESETS: [(&str, Modifiers); 3] = [
    ("⌥ Option", Modifiers::OPTION),
    (
        "⌃⌥ Control-Option",
        Modifiers {
            command: false,
            control: true,
            option: true,
            shift: false,
        },
    ),
    (
        "⌃⌥⌘ Hyper",
        Modifiers {
            command: true,
            control: true,
            option: true,
            shift: false,
        },
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(bindings: &str) -> Shortcuts {
        fractile::config::Config::parse(&format!("version = 1\n[shortcuts.bindings]\n{bindings}"))
            .unwrap()
            .shortcuts()
            .clone()
    }

    fn chord(source: &str) -> Chord {
        shortcuts::parse_chord(source).unwrap()
    }

    #[test]
    fn every_action_command_parses_and_ids_are_unique() {
        let mut ids = all().map(|action| action.id).collect::<Vec<_>>();
        for action in all() {
            assert!(action.parsed().is_some(), "{}", action.command);
        }
        let count = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), count);
    }

    #[test]
    fn chords_follow_bindings_and_the_base_modifier() {
        let focus_left = find("focus-left").unwrap();
        assert_eq!(
            focus_left.chords(&Shortcuts::default(), &[]),
            [chord("alt+h")]
        );
        let moved = parse("\"alt+h\" = \"none\"\n\"ctrl+alt+h\" = \"focus left\"");
        assert_eq!(focus_left.chords(&moved, &[]), [chord("ctrl+alt+h")]);
        let taken = parse("\"alt+h\" = \"layout\"");
        assert!(focus_left.chords(&taken, &[]).is_empty());
        let hyper = Shortcuts {
            modifier: PRESETS[2].1,
            ..Shortcuts::default()
        };
        let move_left = find("move-left").unwrap();
        assert_eq!(
            move_left.chords(&hyper, &[]),
            [chord("ctrl+alt+shift+cmd+h")]
        );
        assert!(find("balance").unwrap().chords(&hyper, &[]).is_empty());
        // An enabled named workspace H takes precedence over built-in Focus left.
        let h = [WorkspaceId::named('H').unwrap()];
        assert!(focus_left.chords(&Shortcuts::default(), &h).is_empty());
    }

    #[test]
    fn occupant_names_built_ins_bindings_and_workspace_chords() {
        let defaults = Shortcuts::default();
        assert_eq!(
            occupant(&defaults, &[], chord("alt+l")),
            Some(Occupant::Action(find("focus-right").unwrap()))
        );
        assert_eq!(
            occupant(&defaults, &[], chord("alt+shift+3"))
                .unwrap()
                .title(),
            "Move window to workspace 3"
        );
        assert_eq!(occupant(&defaults, &[], chord("ctrl+alt+l")), None);
        let bound = parse("\"alt+l\" = \"none\"\n\"ctrl+alt+v\" = \"switch 4\"");
        assert_eq!(occupant(&bound, &[], chord("alt+l")), None);
        assert_eq!(
            occupant(&bound, &[], chord("ctrl+alt+v")).unwrap().title(),
            "Switch workspace 4"
        );
        let scratchpad = Shortcuts {
            scratchpad: true,
            ..Shortcuts::default()
        };
        assert_eq!(
            occupant(&scratchpad, &[], chord("alt+\\")).unwrap().title(),
            "Show or hide the scratchpad"
        );
        assert_eq!(occupant(&defaults, &[], chord("alt+\\")), None);
    }

    #[test]
    fn workspace_steps_have_no_built_in_chord_and_name_their_bindings() {
        for action in WORKSPACE_STEP_ACTIONS {
            assert_eq!(action.default, None, "{}", action.id);
        }
        let bound = parse("\"ctrl+alt+n\" = \"switch next\"");
        let switch_next = find("switch-next").unwrap();
        assert_eq!(
            occupant(&bound, &[], chord("ctrl+alt+n")),
            Some(Occupant::Action(switch_next))
        );
        assert_eq!(switch_next.chords(&bound, &[]), [chord("ctrl+alt+n")]);
        // Option+Tab's row is "Previous workspace": no step row may read like it, or like
        // another row.
        let back_and_forth = find("back-and-forth").unwrap().title;
        for action in WORKSPACE_STEP_ACTIONS {
            assert!(action.title.ends_with(" on display"), "{}", action.title);
            assert!(
                !action.title.eq_ignore_ascii_case(back_and_forth)
                    && !action.title.starts_with(back_and_forth),
                "{}",
                action.title
            );
            assert_eq!(
                all().filter(|other| other.title == action.title).count(),
                1,
                "{}",
                action.title
            );
        }
        assert_eq!(
            find("switch-prev").unwrap().title,
            "Step to previous workspace on display"
        );
    }

    #[test]
    fn occupant_names_enabled_named_workspaces_and_gives_them_precedence() {
        let defaults = Shortcuts::default();
        let named = [
            WorkspaceId::named('A').unwrap(),
            WorkspaceId::named('H').unwrap(),
        ];
        assert_eq!(
            occupant(&defaults, &named, chord("alt+a")).unwrap().title(),
            "Switch to workspace A"
        );
        assert_eq!(
            occupant(&defaults, &named, chord("alt+shift+a"))
                .unwrap()
                .title(),
            "Move window to workspace A"
        );
        assert_eq!(occupant(&defaults, &[], chord("alt+a")), None);
        // Workspace H wins over Focus left on alt+h.
        assert_eq!(
            occupant(&defaults, &named, chord("alt+h")).unwrap().title(),
            "Switch to workspace H"
        );
        // Explicit unbind frees the named workspace chord.
        let unbound = parse("\"alt+a\" = \"none\"");
        assert_eq!(occupant(&unbound, &named, chord("alt+a")), None);
        // With defaults = false, built-in named workspace shortcuts are inactive.
        let no_defaults = Shortcuts {
            defaults: false,
            ..Shortcuts::default()
        };
        assert_eq!(occupant(&no_defaults, &named, chord("alt+a")), None);
    }
}
