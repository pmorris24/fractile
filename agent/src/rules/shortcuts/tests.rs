use fractile::shortcuts::parse_chord;

use super::*;

const SOURCE: &str = r#"# Fractile configuration.
version = 1

[[rules]]
bundle_id = "anomaly.hex"
action = "float"

# My keys.
[shortcuts.bindings]
# Arrows move windows.
"alt+left" = "move-direction left"
"#;

fn chord(source: &str) -> Chord {
    parse_chord(source).unwrap()
}

fn bound(doc: &Doc, source: &str) -> Option<Option<String>> {
    doc.shortcuts()
        .binding(chord(source))
        .map(|command| command.map(Command::description))
}

fn saved(doc: &Doc) -> Doc {
    let source = doc.source();
    fractile::config::Config::parse(&source).expect("Fractile reads the edit");
    Doc::parse(&source)
}

#[test]
fn rebinding_a_built_in_keeps_comments_and_unbinds_its_old_chord() {
    let mut doc = Doc::parse(SOURCE);
    doc.set_shortcut(
        "focus left",
        Some(chord("alt+h")),
        Some(chord("ctrl+alt+h")),
    );
    let doc = saved(&doc);
    let source = doc.source();
    assert!(source.contains("# My keys."), "{source}");
    assert!(source.contains("# Arrows move windows."), "{source}");
    assert!(
        source.contains(r#""alt+left" = "move-direction left""#),
        "{source}"
    );
    assert!(
        source.contains(r#""ctrl+alt+h" = "focus left""#),
        "{source}"
    );
    assert_eq!(bound(&doc, "alt+h"), Some(None));
    assert_eq!(doc.rules().len(), 1);
    assert!(doc.has_custom_shortcuts());
}

#[test]
fn moving_a_built_in_back_home_removes_both_bindings() {
    let mut doc = Doc::parse("version = 1\n");
    doc.set_shortcut(
        "focus left",
        Some(chord("alt+h")),
        Some(chord("ctrl+alt+h")),
    );
    doc.set_shortcut("focus left", Some(chord("alt+h")), Some(chord("alt+h")));
    assert_eq!(doc.source(), "version = 1\n");
    assert!(!doc.has_custom_shortcuts());
}

#[test]
fn unbinding_leaves_none_on_the_built_in_chord() {
    let mut doc = Doc::parse("version = 1\n");
    doc.set_shortcut("float", Some(chord("alt+shift+space")), None);
    let doc = saved(&doc);
    assert_eq!(bound(&doc, "alt+shift+space"), Some(None));
    assert_eq!(doc.shortcuts().bindings.len(), 1);
    assert!(
        doc.source().contains("[shortcuts.bindings]") && !doc.source().contains("[shortcuts]\n"),
        "{}",
        doc.source()
    );
}

#[test]
fn unbinding_a_custom_command_removes_its_binding() {
    let mut doc = Doc::parse(SOURCE);
    doc.set_shortcut("move-direction left", None, None);
    let doc = saved(&doc);
    assert!(!doc.has_custom_shortcuts(), "{}", doc.source());
    assert!(doc.source().contains("action = \"float\""));
}

#[test]
fn taking_a_chord_that_holds_none_replaces_it_whatever_its_spelling() {
    let mut doc = Doc::parse("version = 1\n[shortcuts.bindings]\n\"Option + H\" = \"none\"\n");
    doc.set_shortcut("balance", None, Some(chord("alt+h")));
    let doc = saved(&doc);
    assert_eq!(bound(&doc, "alt+h"), Some(Some("balance layout".into())));
    assert_eq!(doc.shortcuts().bindings.len(), 1);
}

#[test]
fn a_command_keeps_one_chord() {
    let mut doc = Doc::parse(
        "version = 1\n[shortcuts.bindings]\n\"alt+b\" = \"balance\"\n\"ctrl+alt+b\" = \"balance\"\n",
    );
    doc.set_shortcut("balance", None, Some(chord("cmd+alt+b")));
    let doc = saved(&doc);
    assert_eq!(doc.shortcuts().bindings.len(), 1);
    assert_eq!(
        bound(&doc, "cmd+alt+b"),
        Some(Some("balance layout".into()))
    );
}

#[test]
fn modifier_moves_unbound_built_ins_with_it() {
    let mut doc = Doc::parse(
        "version = 1\n[shortcuts.bindings]\n\"alt+h\" = \"none\"\n\"alt+shift+h\" = \"none\"\n\"alt+b\" = \"balance\"\n",
    );
    doc.set_shortcut_modifier(Modifiers {
        control: true,
        option: true,
        ..Modifiers::default()
    });
    let doc = saved(&doc);
    let shortcuts = doc.shortcuts();
    assert!(shortcuts.modifier.control && shortcuts.modifier.option);
    assert_eq!(bound(&doc, "ctrl+alt+h"), Some(None));
    assert_eq!(bound(&doc, "ctrl+alt+shift+h"), Some(None));
    assert_eq!(bound(&doc, "alt+h"), None);
    // A binding is an exact chord, so it stays where it was.
    assert_eq!(bound(&doc, "alt+b"), Some(Some("balance layout".into())));

    let mut doc = doc;
    doc.set_shortcut_modifier(Modifiers::OPTION);
    let doc = saved(&doc);
    assert!(!doc.source().contains("modifier"), "{}", doc.source());
    assert_eq!(bound(&doc, "alt+h"), Some(None));
}

#[test]
fn modifier_keeps_fixed_scratchpad_unbinds_in_place() {
    let mut doc = Doc::parse(
        "version = 1\n[labs]\nscratchpad = true\n[[workspaces]]\nworkspace = \"A\"\nmonitor = \"main\"\n[shortcuts.bindings]\n'alt+\\' = 'none'\n'alt+shift+\\' = 'none'\n'alt+h' = 'none'\n'alt+a' = 'none'\n",
    );
    let ctrl_alt = Modifiers {
        control: true,
        option: true,
        ..Modifiers::default()
    };
    doc.set_shortcut_modifier(ctrl_alt);
    let doc = saved(&doc);
    // Fixed Scratchpad chords stay on Option+\ and Option+Shift+\.
    assert_eq!(bound(&doc, "alt+\\"), Some(None));
    assert_eq!(bound(&doc, "alt+shift+\\"), Some(None));
    assert_eq!(bound(&doc, "ctrl+alt+\\"), None);
    assert_eq!(bound(&doc, "ctrl+alt+shift+\\"), None);
    // Built-in layout and enabled named workspace unbinds still follow the base modifier.
    assert_eq!(bound(&doc, "ctrl+alt+h"), Some(None));
    assert_eq!(bound(&doc, "ctrl+alt+a"), Some(None));
    assert_eq!(bound(&doc, "alt+h"), None);
    assert_eq!(bound(&doc, "alt+a"), None);
}

#[test]
fn reset_removes_the_table_and_keeps_everything_else() {
    let mut doc = Doc::parse(SOURCE);
    doc.set_shortcut_modifier(Modifiers {
        control: true,
        option: true,
        ..Modifiers::default()
    });
    doc.reset_shortcuts();
    let doc = saved(&doc);
    let source = doc.source();
    assert!(!source.contains("shortcuts"), "{source}");
    assert!(source.contains("# Fractile configuration."), "{source}");
    assert_eq!(doc.shortcuts(), Shortcuts::default());
}

#[test]
fn vacating_a_borrowed_built_in_or_scratchpad_chord_leaves_none() {
    let mut doc = Doc::parse(
        "version = 1\n[labs]\nscratchpad = true\n[[workspaces]]\nworkspace = \"A\"\nmonitor = \"main\"\n",
    );
    // Unbind Focus left (alt+h), Workspace A (alt+a), and Scratchpad (alt+\), borrow each with
    // Balance, then move Balance away: each vacated implicit chord stays `none`.
    doc.set_shortcut("focus left", Some(chord("alt+h")), None);
    doc.set_shortcut("balance", None, Some(chord("alt+h")));
    doc.set_shortcut("balance", None, Some(chord("ctrl+cmd+b")));
    let doc = saved(&doc);
    assert_eq!(bound(&doc, "alt+h"), Some(None));
    assert_eq!(
        bound(&doc, "ctrl+cmd+b"),
        Some(Some("balance layout".into()))
    );

    let mut doc = doc;
    doc.set_shortcut("balance", None, Some(chord("alt+a")));
    doc.set_shortcut("balance", None, None);
    let doc = saved(&doc);
    assert_eq!(bound(&doc, "alt+a"), Some(None));

    let mut doc = doc;
    doc.set_shortcut("balance", None, Some(chord("alt+\\")));
    doc.set_shortcut("balance", None, None);
    let doc = saved(&doc);
    assert_eq!(bound(&doc, "alt+\\"), Some(None));
}

#[test]
fn clearing_or_rebinding_explicit_built_in_shortcut_preserves_retained_key_comment() {
    let source = "version = 1\n\n# Shortcut section.\n[shortcuts.bindings]\n# Focus note.\n\"alt+h\" = \"focus left\"\n# Balance note.\n\"alt+shift+b\" = \"balance\"\n";
    let default = parse_chord("alt+h").unwrap();

    // Clearing keeps alt+h = "none" and its comment.
    let mut cleared = Doc::parse(source);
    cleared.set_shortcut("focus left", Some(default), None);
    let saved = cleared.source();
    assert!(fractile::config::Config::parse(&saved).is_ok());
    assert!(saved.contains("\"alt+h\" = \"none\""));
    assert!(saved.contains("# Focus note."));
    assert!(saved.contains("# Balance note."));

    // Rebinding to another chord keeps alt+h = "none" and its comment.
    let mut rebound = Doc::parse(source);
    rebound.set_shortcut(
        "focus left",
        Some(default),
        Some(parse_chord("ctrl+cmd+h").unwrap()),
    );
    let saved = rebound.source();
    assert!(fractile::config::Config::parse(&saved).is_ok());
    assert!(saved.contains("\"alt+h\" = \"none\""));
    assert!(saved.contains("\"ctrl+cmd+h\" = \"focus left\""));
    assert!(saved.contains("# Focus note."));
    assert!(saved.contains("# Balance note."));
}

#[test]
fn modifier_round_trip_preserves_suppression_when_destination_holds_explicit_binding() {
    let mut doc = Doc::parse("version = 1\n");
    let ctrl_alt = Modifiers {
        control: true,
        option: true,
        ..Modifiers::default()
    };
    let ctrl = Modifiers {
        control: true,
        ..Modifiers::default()
    };
    doc.set_shortcut(
        "focus left",
        Some(chord("alt+h")),
        Some(chord("ctrl+alt+h")),
    );
    assert_eq!(bound(&doc, "ctrl+alt+h"), Some(Some("focus left".into())));
    assert_eq!(bound(&doc, "alt+h"), Some(None));

    // Switch Base modifier to Control+Option (colliding with ctrl+alt+h) and back to Option.
    doc.set_shortcut_modifier(ctrl_alt);
    let mut doc = saved(&doc);
    assert_eq!(bound(&doc, "ctrl+alt+h"), Some(Some("focus left".into())));
    assert_eq!(bound(&doc, "alt+h"), None);

    doc.set_shortcut_modifier(Modifiers::OPTION);
    let mut doc = saved(&doc);
    assert_eq!(bound(&doc, "ctrl+alt+h"), Some(Some("focus left".into())));
    assert_eq!(bound(&doc, "alt+h"), Some(None));

    // Two-hop Option -> Control+Option -> Control also keeps focus left on ctrl+alt+h alone.
    doc.set_shortcut_modifier(ctrl_alt);
    doc.set_shortcut_modifier(ctrl);
    let doc = saved(&doc);
    assert_eq!(bound(&doc, "ctrl+alt+h"), Some(Some("focus left".into())));
    assert_eq!(bound(&doc, "ctrl+h"), Some(None));
}

#[test]
fn resetting_shortcuts_or_choosing_option_preserves_detached_section_heading() {
    let source = "version = 1\n\n# Keyboard tuning.\n\n[shortcuts]\nmodifier = \"ctrl+alt\"\n\n# Other settings.\n\n[settings]\nhud = false\n";

    let mut reset = Doc::parse(source);
    reset.reset_shortcuts();
    let reset_saved = saved(&reset).source();
    assert!(reset_saved.contains("# Keyboard tuning."), "{reset_saved}");
    assert!(reset_saved.contains("# Other settings."), "{reset_saved}");
    assert!(!reset_saved.contains("[shortcuts]"), "{reset_saved}");

    let mut option = Doc::parse(source);
    option.set_shortcut_modifier(Modifiers::OPTION);
    let option_saved = saved(&option).source();
    assert!(
        option_saved.contains("# Keyboard tuning."),
        "{option_saved}"
    );
    assert!(option_saved.contains("# Other settings."), "{option_saved}");
    assert!(!option_saved.contains("[shortcuts]"), "{option_saved}");
}

#[test]
fn clearing_rebinding_or_migrating_shortcut_preserves_detached_body_comments() {
    let source = "version = 1\n\n[shortcuts.bindings]\n# Unrelated binding.\n\"alt+shift+c\" = \"float\"\n\n# Navigation actions.\n\n\"ctrl+alt+h\" = \"focus left\"\n";

    // Clear custom binding -> preserves detached heading above the removed key.
    let mut cleared = Doc::parse(source);
    cleared.set_shortcut("focus left", Some(chord("alt+h")), None);
    let cleared_saved = saved(&cleared).source();
    assert!(
        cleared_saved.contains("# Unrelated binding."),
        "{cleared_saved}"
    );
    assert!(
        cleared_saved.contains("# Navigation actions."),
        "{cleared_saved}"
    );
    assert!(
        cleared_saved.contains("\"alt+h\" = \"none\""),
        "{cleared_saved}"
    );
    assert!(!cleared_saved.contains("\"ctrl+alt+h\""), "{cleared_saved}");

    // Rebind custom binding -> preserves detached heading.
    let mut rebound = Doc::parse(source);
    rebound.set_shortcut(
        "focus left",
        Some(chord("alt+h")),
        Some(chord("ctrl+cmd+h")),
    );
    let rebound_saved = saved(&rebound).source();
    assert!(
        rebound_saved.contains("# Navigation actions."),
        "{rebound_saved}"
    );
    assert!(
        rebound_saved.contains("\"ctrl+cmd+h\" = \"focus left\""),
        "{rebound_saved}"
    );
    assert!(
        rebound_saved.contains("\"alt+h\" = \"none\""),
        "{rebound_saved}"
    );

    // Rebind back to default (removes default unbind marker via remove_chord) -> preserves heading.
    let mut restored = Doc::parse(
        "version = 1\n\n[shortcuts.bindings]\n\"alt+x\" = \"balance\"\n\n# Navigation actions.\n\n\"alt+h\" = \"none\"\n",
    );
    restored.set_shortcut("focus left", Some(chord("alt+h")), Some(chord("alt+h")));
    let restored_saved = saved(&restored).source();
    assert!(
        restored_saved.contains("# Navigation actions."),
        "{restored_saved}"
    );
    assert!(!restored_saved.contains("\"alt+h\""), "{restored_saved}");

    // Changing base modifier migrates unbind marker -> preserves detached heading.
    let mut migrated = Doc::parse(
        "version = 1\n\n[shortcuts.bindings]\n\"alt+x\" = \"balance\"\n\n# Navigation actions.\n\n\"alt+h\" = \"none\"\n",
    );
    migrated.set_shortcut_modifier(Modifiers {
        control: true,
        option: true,
        ..Modifiers::default()
    });
    let migrated_saved = saved(&migrated).source();
    assert!(
        migrated_saved.contains("# Navigation actions."),
        "{migrated_saved}"
    );
    assert!(
        migrated_saved.contains("\"ctrl+alt+h\" = \"none\""),
        "{migrated_saved}"
    );
    assert!(!migrated_saved.contains("\"alt+h\""), "{migrated_saved}");

    // Control: directly attached comment (no blank line) still leaves with the removed key.
    let mut attached = Doc::parse(
        "version = 1\n\n[shortcuts.bindings]\n# Unrelated binding.\n\"alt+shift+c\" = \"float\"\n\n# Navigation actions.\n\"ctrl+alt+h\" = \"focus left\"\n",
    );
    attached.set_shortcut("focus left", Some(chord("alt+h")), None);
    let attached_saved = saved(&attached).source();
    assert!(
        attached_saved.contains("# Unrelated binding."),
        "{attached_saved}"
    );
    assert!(
        !attached_saved.contains("# Navigation actions."),
        "{attached_saved}"
    );
}

#[test]
fn choosing_option_or_resetting_shortcuts_preserves_detached_modifier_comment() {
    let source = "version = 1\n\n# Keyboard tuning.\n\n[shortcuts]\n\n# Base modifier note.\n\nmodifier = \"ctrl+alt\"\n\n# Other settings.\n\n[settings]\nhud = false\n";

    // Choosing Option (removes modifier and empties [shortcuts]) preserves all three comments in order.
    let mut option = Doc::parse(source);
    option.set_shortcut_modifier(Modifiers::OPTION);
    let option_saved = saved(&option).source();
    assert!(!option_saved.contains("[shortcuts]"), "{option_saved}");
    assert!(!option_saved.contains("modifier ="), "{option_saved}");
    let kb = option_saved
        .find("# Keyboard tuning.")
        .expect("missing keyboard tuning");
    let note = option_saved
        .find("# Base modifier note.")
        .expect("missing modifier note");
    let other = option_saved
        .find("# Other settings.")
        .expect("missing other settings");
    assert!(kb < note && note < other, "{option_saved}");

    // Pressing Reset to defaults preserves all three comments in order.
    let mut reset = Doc::parse(source);
    reset.reset_shortcuts();
    let reset_saved = saved(&reset).source();
    assert!(!reset_saved.contains("[shortcuts]"), "{reset_saved}");
    let kb = reset_saved
        .find("# Keyboard tuning.")
        .expect("missing keyboard tuning");
    let note = reset_saved
        .find("# Base modifier note.")
        .expect("missing modifier note");
    let other = reset_saved
        .find("# Other settings.")
        .expect("missing other settings");
    assert!(kb < note && note < other, "{reset_saved}");

    // Choosing Option with a surviving binding keeps [shortcuts.bindings] and preserves the modifier note.
    let sibling_source = "version = 1\n\n# Keyboard tuning.\n\n[shortcuts]\n\n# Base modifier note.\n\nmodifier = \"ctrl+alt\"\n\n[shortcuts.bindings]\n\"alt+x\" = \"balance\"\n\n# Other settings.\n\n[settings]\nhud = false\n";
    let mut sibling = Doc::parse(sibling_source);
    sibling.set_shortcut_modifier(Modifiers::OPTION);
    let sibling_saved = saved(&sibling).source();
    assert!(
        sibling_saved.contains("# Keyboard tuning."),
        "{sibling_saved}"
    );
    assert!(
        sibling_saved.contains("# Base modifier note."),
        "{sibling_saved}"
    );
    assert!(
        sibling_saved.contains("\"alt+x\" = \"balance\""),
        "{sibling_saved}"
    );
    assert!(!sibling_saved.contains("modifier ="), "{sibling_saved}");

    // Control: directly attached modifier note (no blank line) leaves with modifier on Option and Reset.
    let attached_source = "version = 1\n\n# Keyboard tuning.\n\n[shortcuts]\n\n# Base modifier note.\nmodifier = \"ctrl+alt\"\n\n# Other settings.\n\n[settings]\nhud = false\n";
    let mut attached_opt = Doc::parse(attached_source);
    attached_opt.set_shortcut_modifier(Modifiers::OPTION);
    assert!(
        !saved(&attached_opt)
            .source()
            .contains("# Base modifier note.")
    );
    let mut attached_reset = Doc::parse(attached_source);
    attached_reset.reset_shortcuts();
    assert!(
        !saved(&attached_reset)
            .source()
            .contains("# Base modifier note.")
    );
}
