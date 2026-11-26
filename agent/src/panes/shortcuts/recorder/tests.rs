use fractile::shortcuts::parse_chord;
use rune::{Mods, PhysicalKeyCode as K};

use super::*;

const ALT: Mods = Mods {
    shift: false,
    cmd: false,
    alt: true,
    ctrl: false,
    logo: false,
};

fn mods(alt: bool, ctrl: bool, shift: bool, cmd: bool) -> Mods {
    Mods {
        shift,
        cmd,
        alt,
        ctrl,
        logo: cmd,
    }
}

fn us(key: K, held: Mods) -> Press {
    press(key, held, us_character)
}

fn chord(source: &str) -> Press {
    Press::Chord(parse_chord(source).unwrap())
}

#[test]
fn a_press_records_the_held_modifiers_and_its_key() {
    assert_eq!(us(K::KeyL, ALT), chord("alt+l"));
    assert_eq!(
        us(K::KeyL, mods(true, true, true, true)),
        chord("ctrl+alt+shift+cmd+l")
    );
    assert_eq!(us(K::Digit0, ALT), chord("alt+0"));
    assert_eq!(
        us(K::ArrowLeft, mods(false, true, false, false)),
        chord("ctrl+left")
    );
    assert_eq!(
        us(K::Space, mods(true, false, true, false)),
        chord("alt+shift+space")
    );
    assert_eq!(us(K::Slash, ALT), chord("alt+/"));
    assert_eq!(us(K::F5, mods(false, false, false, true)), chord("cmd+f5"));
    assert_eq!(us(K::Backspace, ALT), chord("alt+backspace"));
    // The active layout names a character key: AZERTY types `a` on the US Q key.
    let azerty = |key| (key == K::KeyQ).then_some('A');
    assert_eq!(press(K::KeyQ, ALT, azerty), chord("alt+a"));
}

#[test]
fn bare_and_shift_only_keys_are_refused() {
    assert_eq!(us(K::KeyL, Mods::default()), Press::Refused(NEEDS_MODIFIER));
    assert_eq!(
        us(K::KeyL, mods(false, false, true, false)),
        Press::Refused(SHIFT_ALONE)
    );
    assert_eq!(
        us(K::ShiftLeft, mods(false, false, true, false)),
        Press::Ignore
    );
    assert_eq!(us(K::AltRight, ALT), Press::Ignore);
}

#[test]
fn escape_cancels_and_a_bare_delete_unbinds() {
    assert_eq!(us(K::Escape, Mods::default()), Press::Cancel);
    assert_eq!(us(K::Escape, ALT), Press::Cancel);
    assert_eq!(us(K::Backspace, Mods::default()), Press::Unbind);
    assert_eq!(
        us(K::Delete, mods(false, false, true, false)),
        Press::Unbind
    );
}

fn listening(action: &'static str) -> Ui {
    let mut ui = crate::playground::blank_fixture();
    begin(&mut ui, action);
    ui
}

fn message(ui: &Ui) -> Option<&str> {
    match &ui.shortcut_capture {
        Capture::Listening { message, .. } => message.as_deref(),
        _ => None,
    }
}

#[test]
fn recording_saves_the_chord_and_moves_the_built_in_off_its_old_one() {
    let mut ui = listening("focus-left");
    record(
        &mut ui,
        "focus-left",
        us(K::KeyH, mods(true, true, false, false)),
    );
    assert_eq!(
        ui.shortcut_capture,
        Capture::Saved {
            action: "focus-left"
        }
    );
    let shortcuts = ui.doc.shortcuts();
    let focus_left = catalog::find("focus-left").unwrap();
    assert_eq!(
        focus_left.chords(&shortcuts, &[]),
        [parse_chord("ctrl+alt+h").unwrap()]
    );
    assert_eq!(shortcuts.binding(parse_chord("alt+h").unwrap()), Some(None));
}

#[test]
fn a_chord_another_shortcut_uses_is_refused_by_name() {
    let mut ui = listening("focus-left");
    record(&mut ui, "focus-left", us(K::KeyL, ALT));
    assert_eq!(message(&ui), Some("Used by Focus right"));
    record(
        &mut ui,
        "focus-left",
        us(K::Digit3, mods(true, false, true, false)),
    );
    assert_eq!(message(&ui), Some("Used by Move window to workspace 3"));
    assert_eq!(ui.doc.shortcuts(), Shortcuts::default());
    // Its own chord is no conflict: recording it keeps the built-in.
    record(&mut ui, "focus-left", us(K::KeyH, ALT));
    assert_eq!(
        ui.shortcut_capture,
        Capture::Saved {
            action: "focus-left"
        }
    );
    assert!(!ui.doc.has_custom_shortcuts());
}

#[test]
fn a_refusal_keeps_listening_and_escape_leaves_everything_as_it_was() {
    let mut ui = listening("float");
    record(&mut ui, "float", us(K::KeyV, Mods::default()));
    assert_eq!(message(&ui), Some(NEEDS_MODIFIER));
    record(&mut ui, "float", us(K::Escape, Mods::default()));
    assert_eq!(ui.shortcut_capture, Capture::Idle);
    assert!(!ui.doc.has_custom_shortcuts());
}

#[test]
fn delete_unbinds_to_none() {
    let mut ui = listening("float");
    record(&mut ui, "float", us(K::Backspace, Mods::default()));
    let shortcuts = ui.doc.shortcuts();
    assert!(
        catalog::find("float")
            .unwrap()
            .chords(&shortcuts, &[])
            .is_empty()
    );
    assert_eq!(
        shortcuts.binding(parse_chord("alt+shift+space").unwrap()),
        Some(None)
    );
}

#[test]
fn pressing_the_listening_shortcut_again_cancels() {
    let mut ui = listening("grow");
    begin(&mut ui, "grow");
    assert_eq!(ui.shortcut_capture, Capture::Idle);
    begin(&mut ui, "grow");
    begin(&mut ui, "shrink");
    assert_eq!(ui.shortcut_capture.listening(), Some("shrink"));
}

#[test]
fn held_modifiers_show_and_clear_a_refusal() {
    let mut ui = listening("grow");
    record(&mut ui, "grow", us(K::KeyG, Mods::default()));
    assert!(on_input(&mut ui, &PhysicalInput::Modifiers(ALT)));
    assert_eq!(
        ui.shortcut_capture,
        Capture::Listening {
            action: "grow",
            held: Modifiers::OPTION,
            message: None,
        }
    );
    assert!(on_input(&mut ui, &PhysicalInput::FocusLost));
    assert_eq!(ui.shortcut_capture, Capture::Idle);
    assert!(!on_input(&mut ui, &PhysicalInput::Modifiers(ALT)));
}

#[test]
fn recording_refuses_an_enabled_named_workspace_chord_and_accepts_it_when_absent() {
    let mut ui = listening("balance");
    ui.doc = crate::rules::Doc::parse(
        "version = 1\n[[workspaces]]\nworkspace = 'A'\nmonitor = 'main'\n",
    );
    let before = ui.doc.source();

    record(&mut ui, "balance", us(K::KeyA, ALT));
    assert_eq!(ui.shortcut_capture.listening(), Some("balance"));
    assert_eq!(message(&ui), Some("Used by Switch to workspace A"));
    assert_eq!(ui.doc.source(), before);

    record(
        &mut ui,
        "balance",
        us(K::KeyA, mods(true, false, true, false)),
    );
    assert_eq!(ui.shortcut_capture.listening(), Some("balance"));
    assert_eq!(message(&ui), Some("Used by Move window to workspace A"));
    assert_eq!(ui.doc.source(), before);

    // Named workspace enabled via [[rules]] is also refused, and an overlapping letter (H)
    // refuses Option+H for Focus left without writing an unbind over workspace H on rebind.
    let mut ruled = listening("focus-left");
    ruled.doc = crate::rules::Doc::parse(
        "version = 1\n[[rules]]\nbundle_id = 'com.example.App'\naction = 'route'\nworkspace = 'H'\n",
    );
    record(&mut ruled, "focus-left", us(K::KeyH, ALT));
    assert_eq!(ruled.shortcut_capture.listening(), Some("focus-left"));
    assert_eq!(message(&ruled), Some("Used by Switch to workspace H"));
    record(
        &mut ruled,
        "focus-left",
        us(K::KeyH, mods(true, true, false, false)),
    );
    assert_eq!(
        ruled.shortcut_capture,
        Capture::Saved {
            action: "focus-left"
        }
    );
    assert_eq!(
        ruled.doc.shortcuts().binding(parse_chord("alt+h").unwrap()),
        None
    );

    // Control: when workspace A is absent, Option+A is free for Balance layout.
    let mut absent = listening("balance");
    absent.doc = crate::rules::Doc::parse("version = 1\n");
    record(&mut absent, "balance", us(K::KeyA, ALT));
    assert_eq!(
        absent.shortcut_capture,
        Capture::Saved { action: "balance" }
    );
    assert_eq!(
        absent
            .doc
            .shortcuts()
            .binding(parse_chord("alt+a").unwrap()),
        Some(Some(&shortcuts::Command::Balance))
    );
}

#[test]
fn changing_base_modifier_keeps_unbound_scratchpad_chord_in_place() {
    let mut ui = listening("balance");
    ui.doc = crate::rules::Doc::parse(
        "version = 1\n[labs]\nscratchpad = true\n[shortcuts.bindings]\n'alt+\\' = 'none'\n",
    );
    assert!(ui.doc.shortcuts().scratchpad);
    assert_eq!(
        ui.doc.shortcuts().binding(parse_chord("alt+\\").unwrap()),
        Some(None)
    );

    super::super::set_modifier(&mut ui, catalog::PRESETS[1].1);

    assert_eq!(
        ui.doc.shortcuts().binding(parse_chord("alt+\\").unwrap()),
        Some(None)
    );
    assert_eq!(
        ui.doc
            .shortcuts()
            .binding(parse_chord("ctrl+alt+\\").unwrap()),
        None
    );
}

fn focus_left_after(presses: Vec<(&'static str, Press)>) -> (Vec<String>, Ui) {
    let mut ui = crate::playground::blank_fixture();
    ui.doc = crate::rules::Doc::parse("version = 1\n");
    for (action, press) in presses {
        record(&mut ui, action, press);
    }
    let action = catalog::find("focus-left").unwrap();
    let chords = action
        .chords(&ui.doc.shortcuts(), &[])
        .iter()
        .map(|chord| chord.source())
        .collect();
    (chords, ui)
}

fn ctrl_cmd(key: K) -> Press {
    us(key, mods(false, true, false, true))
}

#[test]
fn a_cleared_shortcut_stays_cleared_after_its_old_chord_is_reused() {
    let (chords, mut ui) = focus_left_after(vec![
        ("focus-left", Press::Unbind),
        ("balance", us(K::KeyH, ALT)),
        ("balance", ctrl_cmd(K::KeyB)),
    ]);
    assert_eq!(chords, Vec::<String>::new());
    assert_eq!(
        ui.doc.shortcuts().binding(parse_chord("alt+h").unwrap()),
        Some(None)
    );
    // Moving Focus left back to its built-in chord still restores it cleanly.
    record(&mut ui, "focus-left", us(K::KeyH, ALT));
    assert_eq!(
        catalog::find("focus-left")
            .unwrap()
            .chords(&ui.doc.shortcuts(), &[])
            .iter()
            .map(|c| c.source())
            .collect::<Vec<_>>(),
        ["alt+h"]
    );
    // Reset to defaults restores every built-in shortcut.
    ui.doc.reset_shortcuts();
    assert!(!ui.doc.has_custom_shortcuts());
}

#[test]
fn a_moved_shortcut_keeps_one_chord_after_its_old_chord_is_reused() {
    let (chords, ui) = focus_left_after(vec![
        ("focus-left", ctrl_cmd(K::KeyH)),
        ("balance", us(K::KeyH, ALT)),
        ("balance", Press::Unbind),
    ]);
    assert_eq!(chords, ["ctrl+cmd+h"]);
    assert_eq!(
        ui.doc.shortcuts().binding(parse_chord("alt+h").unwrap()),
        Some(None)
    );
}

#[test]
fn control_an_unrelated_chord_changes_nothing() {
    let (chords, _) = focus_left_after(vec![
        ("focus-left", Press::Unbind),
        ("balance", ctrl_cmd(K::KeyG)),
        ("balance", ctrl_cmd(K::KeyB)),
    ]);
    assert_eq!(chords, Vec::<String>::new());
}
