use super::*;

const SOURCE: &str = r#"# Spaceman configuration.
version = 1

[[rules]]
bundle_id = "anomaly.hex"
action = "float"

[[rules]]
# OpenCode Browser's agent window.
bundle_id = "com.brave.Browser"
title = "OpenCode Browser agents - Brave"
action = "route"
workspace = 9

[[rules]]
bundle_id = "com.apple.MobileSMS"
action = "route"
workspace = "M"

[[workspaces]]
workspace = "5"
monitor = "main"
"#;

fn ws(name: &str) -> WorkspaceId {
    WorkspaceId::parse(name).unwrap()
}

#[test]
fn reads_rules_in_file_order() {
    let doc = Doc::parse(SOURCE);
    assert_eq!(doc.error, None);
    let rules = doc.rules();
    assert_eq!(rules.len(), 3);
    assert_eq!(rules[0].place, Place::Float);
    assert_eq!(rules[1].place, Place::Workspace(ws("9")));
    assert_eq!(
        rules[1].title.as_deref(),
        Some("OpenCode Browser agents - Brave")
    );
    assert_eq!(rules[2].place, Place::Workspace(ws("M")));
}

#[test]
fn moving_a_rule_keeps_comments_and_the_rest_of_the_file() {
    let mut doc = Doc::parse(SOURCE);
    doc.place(1, Place::Float);
    doc.place(0, Place::Workspace(ws("3")));
    let source = doc.source();
    assert!(source.contains("# OpenCode Browser's agent window."));
    assert!(source.contains("title = \"OpenCode Browser agents - Brave\""));
    let rules = Doc::parse(&source).rules();
    assert_eq!(rules[0].place, Place::Workspace(ws("3")));
    assert_eq!(rules[1].place, Place::Float);
    assert!(spaceman::config::Config::parse(&source).is_ok());
    assert!(
        source.contains("workspace = 3\n"),
        "numbers stay integers:\n{source}"
    );
}

#[test]
fn adds_and_removes_rules() {
    let mut doc = Doc::parse(SOURCE);
    doc.add("com.tinyspeck.slackmacgap", Place::Workspace(ws("2")));
    doc.remove(0);
    let rules = Doc::parse(&doc.source()).rules();
    assert_eq!(rules.len(), 3);
    assert_eq!(rules[0].bundle_id.as_deref(), Some("com.brave.Browser"));
    assert_eq!(
        rules[2].bundle_id.as_deref(),
        Some("com.tinyspeck.slackmacgap")
    );
    assert_eq!(rules[2].place, Place::Workspace(ws("2")));
}

#[test]
fn adds_to_an_empty_file() {
    let mut doc = Doc::parse(EMPTY);
    doc.add("anomaly.hex", Place::Float);
    assert!(spaceman::config::Config::parse(&doc.source()).is_ok());
    assert_eq!(doc.rules().len(), 1);
}

#[test]
fn workspace_displays_round_trip() {
    let mut doc = Doc::parse(SOURCE);
    assert_eq!(doc.monitor(ws("5")).as_deref(), Some("main"));
    doc.set_monitor(ws("M"), Some("secondary"));
    doc.set_monitor(ws("5"), None);
    let doc = Doc::parse(&doc.source());
    assert_eq!(doc.monitor(ws("5")), None);
    assert_eq!(doc.monitor(ws("M")).as_deref(), Some("secondary"));
}

#[test]
fn a_broken_file_is_never_saved_over() {
    let mut doc = Doc::parse("version = 1\n[[rules]]\naction = \"route\"\n");
    assert!(doc.error.is_some());
    assert!(doc.save().is_err());
}

#[test]
fn a_tile_rule_reads_back_and_the_hud_float_replaces_it_in_place() {
    let mut doc = Doc::parse(
        "version = 1\n\n[[rules]]\nbundle_id = \"com.apple.systempreferences\"\naction = \"tile\"\n",
    );
    assert!(doc.error.is_none());
    assert_eq!(
        doc.app_place("com.apple.systempreferences"),
        Some(Place::Tile)
    );
    // Floating it from the HUD rewrites the tile rule rather than adding a second
    // rule the first-match tile rule would shadow.
    doc.set_app_place("com.apple.systempreferences", Some(Place::Float));
    assert_eq!(doc.rules().len(), 1);
    assert_eq!(doc.rules()[0].place, Place::Float);
    doc.place(0, Place::Tile);
    assert!(doc.source().contains("action = \"tile\""));
    assert!(spaceman::config::Config::parse(&doc.source()).is_ok());
}

#[test]
fn set_app_place_updates_bundle_rule_without_touching_title_rules() {
    let mut doc = Doc::parse(SOURCE);
    assert_eq!(doc.app_place("com.brave.Browser"), None);
    doc.set_app_place("com.brave.Browser", Some(Place::Workspace(ws("1"))));
    assert_eq!(
        doc.app_place("com.brave.Browser"),
        Some(Place::Workspace(ws("1")))
    );
    doc.set_app_place("com.brave.Browser", Some(Place::Float));
    assert_eq!(doc.app_place("com.brave.Browser"), Some(Place::Float));
    doc.set_app_place("com.brave.Browser", None);
    assert_eq!(doc.app_place("com.brave.Browser"), None);
    // Title-specific rule on com.brave.Browser stays intact.
    assert!(doc.rules().iter().any(|rule| {
        rule.bundle_id.as_deref() == Some("com.brave.Browser")
            && rule.title.as_deref() == Some("OpenCode Browser agents - Brave")
    }));
}

#[test]
fn hud_setting_round_trips_and_cleans_up_default_table() {
    let mut doc = Doc::parse(SOURCE);
    assert!(doc.hud());
    doc.set_hud(false);
    assert!(!doc.hud());
    let disabled = doc.source();
    assert!(!spaceman::config::Config::parse(&disabled).unwrap().hud());
    assert!(disabled.contains("# OpenCode Browser's agent window."));

    doc.set_hud(true);
    assert!(doc.hud());
    let enabled = doc.source();
    assert!(spaceman::config::Config::parse(&enabled).unwrap().hud());
    assert!(!enabled.contains("[settings]"));
}

#[test]
fn hud_style_round_trips_and_leaves_the_default_out() {
    use spaceman::config::{Config, HudStyle};
    let mut doc = Doc::parse(SOURCE);
    assert_eq!(doc.hud_style(), HudStyle::Icons);
    doc.set_hud_style(HudStyle::Layouts);
    assert_eq!(doc.hud_style(), HudStyle::Layouts);
    let layouts = doc.source();
    assert_eq!(
        Config::parse(&layouts).unwrap().hud_style(),
        HudStyle::Layouts
    );
    assert!(layouts.contains("hud_style = \"layouts\""));

    doc.set_hud(false);
    doc.set_hud_style(HudStyle::Icons);
    let icons = doc.source();
    assert!(!icons.contains("hud_style"));
    assert!(!Config::parse(&icons).unwrap().hud());
    doc.set_hud(true);
    assert!(!doc.source().contains("[settings]"));
}

#[test]
fn hud_position_round_trips_and_leaves_bottom_out() {
    use spaceman::config::{Config, HudPosition, HudStyle};
    let mut doc = Doc::parse(SOURCE);
    assert_eq!(doc.hud_position(), HudPosition::Bottom);
    doc.set_hud_position(HudPosition::Top);
    assert_eq!(doc.hud_position(), HudPosition::Top);
    let top = doc.source();
    assert!(top.contains("# OpenCode Browser's agent window."));
    assert!(top.contains("hud_position = \"top\""));
    assert_eq!(
        Config::parse(&top).unwrap().hud_position(),
        HudPosition::Top
    );

    doc.set_hud(false);
    doc.set_hud_style(HudStyle::Layouts);
    doc.set_hud_position(HudPosition::Bottom);
    let bottom = doc.source();
    assert!(!bottom.contains("hud_position"), "{bottom}");
    assert!(bottom.contains("hud_style = \"layouts\""), "{bottom}");
    assert_eq!(
        Config::parse(&bottom).unwrap().hud_position(),
        HudPosition::Bottom
    );

    doc.set_hud(true);
    doc.set_hud_style(HudStyle::Icons);
    assert!(!doc.source().contains("[settings]"));
}

#[test]
fn mouse_follows_focus_round_trips_preserving_comments_and_leaves_off_out() {
    use spaceman::config::{Config, MouseFollowsFocus};
    let mut doc = Doc::parse(SOURCE);
    assert_eq!(doc.mouse_follows_focus(), MouseFollowsFocus::Off);
    doc.set_mouse_follows_focus(MouseFollowsFocus::Display);
    assert_eq!(doc.mouse_follows_focus(), MouseFollowsFocus::Display);
    let display = doc.source();
    assert!(display.contains("# OpenCode Browser's agent window."));
    assert!(display.contains("mouse_follows_focus = \"display\""));
    assert_eq!(
        Config::parse(&display).unwrap().mouse_follows_focus(),
        MouseFollowsFocus::Display
    );

    doc.set_mouse_follows_focus(MouseFollowsFocus::Window);
    assert_eq!(doc.mouse_follows_focus(), MouseFollowsFocus::Window);
    let window = doc.source();
    assert!(window.contains("mouse_follows_focus = \"window\""));
    assert_eq!(
        Config::parse(&window).unwrap().mouse_follows_focus(),
        MouseFollowsFocus::Window
    );

    doc.set_mouse_follows_focus(MouseFollowsFocus::Off);
    assert_eq!(doc.mouse_follows_focus(), MouseFollowsFocus::Off);
    assert!(!doc.source().contains("[settings]"));
    assert!(doc.source().contains("# OpenCode Browser's agent window."));
}

#[test]
fn spacing_round_trips_and_leaves_the_file_as_it_was() {
    use spaceman::{config::Config, spacing::Spacing};
    let mut doc = Doc::parse(SOURCE);
    assert_eq!(doc.spacing(), Spacing::NONE);
    let spacing = Spacing {
        inner: 8,
        single_window_max_width: 1680,
        ..Spacing::NONE.with_edges(12)
    };
    doc.set_spacing(spacing);
    assert_eq!(doc.spacing(), spacing);
    let saved = doc.source();
    assert_eq!(Config::parse(&saved).unwrap().spacing(), spacing);
    assert!(saved.contains("# OpenCode Browser's agent window."));
    assert!(saved.contains("[settings.gaps]\ninner = 8\n"), "{saved}");
    assert!(saved.contains("single_window_max_width = 1680"), "{saved}");
    assert_eq!(Doc::parse(&saved).spacing(), spacing);

    doc.set_spacing(Spacing { top: 30, ..spacing });
    assert_eq!(Config::parse(&doc.source()).unwrap().spacing().top, 30);
    assert_eq!(Config::parse(&doc.source()).unwrap().spacing().left, 12);

    doc.set_spacing(Spacing::NONE);
    assert_eq!(doc.source(), SOURCE);
}

#[test]
fn spacing_lands_beside_existing_settings_and_keeps_their_comments() {
    use spaceman::{config::Config, spacing::Spacing};
    let source = format!(
        "[settings]\n# Quiet.\nhud = false\n\n{}",
        &SOURCE[SOURCE.find("[[rules]]").unwrap()..]
    );
    let source = format!("version = 1\n{source}");
    let mut doc = Doc::parse(&source);
    doc.set_spacing(Spacing {
        inner: 6,
        left: 40,
        ..Spacing::NONE
    });
    let saved = doc.source();
    let config = Config::parse(&saved).unwrap();
    assert!(!config.hud());
    assert_eq!((config.spacing().inner, config.spacing().left), (6, 40));
    assert!(saved.contains("# Quiet.\nhud = false"), "{saved}");
    assert!(
        saved.find("[settings.gaps]").unwrap() < saved.find("[[rules]]").unwrap(),
        "gaps sit with their settings:\n{saved}"
    );
    doc.set_spacing(Spacing::NONE);
    assert_eq!(doc.source(), source);
}

#[test]
fn spacing_ignores_values_the_engine_would_refuse() {
    let doc = Doc::parse("version = 1\n[settings.gaps]\ninner = 999\ntop = 9\n");
    assert!(doc.error.is_some());
    assert_eq!(doc.spacing().inner, 0);
    assert_eq!(doc.spacing().top, 9);
}

#[test]
fn inline_rules_array_reads_adds_moves_and_removes_without_losing_entries() {
    let variants = [
        concat!(
            "version = 1\n",
            "rules = [\n",
            "  { bundle_id = \"com.example.First\", action = \"float\" },\n",
            "  { bundle_id = \"com.example.Second\", action = \"route\", workspace = \"M\" },\n",
            "]\n",
        ),
        concat!(
            "# Spaceman configuration.\n",
            "version = 1\n",
            "\n",
            "rules = [\n",
            "  # Keep this comment.\n",
            "  { bundle_id = \"com.example.First\", action = \"float\" },\n",
            "  { bundle_id = \"com.example.Second\", action = \"route\", workspace = \"M\" },\n",
            "]\n",
        ),
        "version = 1\nrules = [{ bundle_id = \"com.example.First\", action = \"float\" }, { bundle_id = \"com.example.Second\", action = \"route\", workspace = \"M\" }]\n",
    ];

    for source in variants {
        let mut doc = Doc::parse(source);
        assert_eq!(doc.error, None, "source:\n{source}");
        let initial = doc.rules();
        assert_eq!(initial.len(), 2, "source:\n{source}");
        assert_eq!(initial[0].bundle_id.as_deref(), Some("com.example.First"));
        assert_eq!(initial[0].place, Place::Float);
        assert_eq!(initial[1].bundle_id.as_deref(), Some("com.example.Second"));
        assert_eq!(initial[1].place, Place::Workspace(ws("M")));
        assert_eq!(doc.named_workspaces(), vec![ws("M")]);

        // Adding a third app keeps both existing inline rules and produces valid TOML.
        doc.add("com.example.Third", Place::Float);
        let after_add = doc.source();
        assert!(
            spaceman::config::Config::parse(&after_add).is_ok(),
            "invalid TOML after add:\n{after_add}"
        );
        let rules = Doc::parse(&after_add).rules();
        assert_eq!(rules.len(), 3, "after add:\n{after_add}");
        assert_eq!(rules[0].bundle_id.as_deref(), Some("com.example.First"));
        assert_eq!(rules[1].bundle_id.as_deref(), Some("com.example.Second"));
        assert_eq!(rules[2].bundle_id.as_deref(), Some("com.example.Third"));
        assert_eq!(rules[2].place, Place::Float);

        // Moving rules updates action/workspace in place without losing the other rules.
        doc.place(0, Place::Workspace(ws("3")));
        doc.place(1, Place::Float);
        let after_move = doc.source();
        assert!(
            spaceman::config::Config::parse(&after_move).is_ok(),
            "invalid TOML after move:\n{after_move}"
        );
        let moved = Doc::parse(&after_move).rules();
        assert_eq!(moved.len(), 3);
        assert_eq!(moved[0].place, Place::Workspace(ws("3")));
        assert_eq!(moved[1].place, Place::Float);
        assert_eq!(moved[2].place, Place::Float);

        // Removing every rule empties the key cleanly.
        doc.remove(1);
        assert_eq!(doc.rules().len(), 2);
        assert!(spaceman::config::Config::parse(&doc.source()).is_ok());
        doc.remove(1);
        doc.remove(0);
        let empty = doc.source();
        assert!(doc.rules().is_empty());
        assert!(!empty.contains("rules"), "rules key should drop:\n{empty}");
        assert!(spaceman::config::Config::parse(&empty).is_ok());
    }
}

#[test]
fn empty_inline_rules_array_supports_adding_an_app() {
    let mut doc = Doc::parse("version = 1\nrules = []\n");
    assert_eq!(doc.error, None);
    assert!(doc.rules().is_empty());
    doc.add("com.example.Only", Place::Workspace(ws("2")));
    let saved = doc.source();
    assert!(
        spaceman::config::Config::parse(&saved).is_ok(),
        "invalid TOML:\n{saved}"
    );
    let rules = Doc::parse(&saved).rules();
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].bundle_id.as_deref(), Some("com.example.Only"));
    assert_eq!(rules[0].place, Place::Workspace(ws("2")));
}

#[test]
fn inline_workspaces_array_reads_updates_adds_and_clears_assignments() {
    let variants = [
        "version = 1\nworkspaces = [{ workspace = 9, monitor = \"main\" }]\n",
        concat!(
            "# Spaceman configuration.\n",
            "version = 1\n",
            "\n",
            "workspaces = [\n",
            "  # Keep workspace 9 on main.\n",
            "  { workspace = 9, monitor = \"main\" },\n",
            "]\n",
        ),
    ];

    for source in variants {
        let mut doc = Doc::parse(source);
        assert_eq!(doc.error, None, "source:\n{source}");
        assert_eq!(doc.monitor(ws("9")).as_deref(), Some("main"));
        assert_eq!(doc.monitor(ws("2")), None);

        // Assigning workspace 2 keeps workspace 9 and writes valid TOML.
        doc.set_monitor(ws("2"), Some("secondary"));
        let after_add = doc.source();
        assert!(
            spaceman::config::Config::parse(&after_add).is_ok(),
            "invalid TOML after adding workspace:\n{after_add}"
        );
        let reloaded = Doc::parse(&after_add);
        assert_eq!(reloaded.monitor(ws("9")).as_deref(), Some("main"));
        assert_eq!(reloaded.monitor(ws("2")).as_deref(), Some("secondary"));

        // Updating an existing entry and declaring a named workspace both round-trip.
        doc.set_monitor(ws("9"), Some("Studio Display"));
        doc.set_monitor(ws("D"), Some("secondary"));
        let after_update = doc.source();
        assert!(
            spaceman::config::Config::parse(&after_update).is_ok(),
            "invalid TOML after update:\n{after_update}"
        );
        let reloaded = Doc::parse(&after_update);
        assert_eq!(reloaded.monitor(ws("9")).as_deref(), Some("Studio Display"));
        assert_eq!(reloaded.monitor(ws("D")).as_deref(), Some("secondary"));
        assert_eq!(reloaded.named_workspaces(), vec![ws("D")]);

        // Clearing numbered overrides removes them, while solely declared named
        // workspace D stays enabled on its default display ("main").
        doc.set_monitor(ws("2"), None);
        doc.set_monitor(ws("9"), None);
        doc.set_monitor(ws("D"), None);
        assert_eq!(doc.monitor(ws("9")), None);
        assert_eq!(doc.monitor(ws("D")).as_deref(), Some("main"));
        assert_eq!(doc.named_workspaces(), vec![ws("D")]);
        assert!(spaceman::config::Config::parse(&doc.source()).is_ok());

        // Once a route rule references D, clearing drops the now-redundant entry cleanly.
        doc.add("com.example.App", Place::Workspace(ws("D")));
        doc.set_monitor(ws("D"), None);
        let empty = doc.source();
        assert_eq!(doc.monitor(ws("D")), None);
        assert!(
            !empty.contains("workspaces"),
            "workspaces key should drop:\n{empty}"
        );
        assert!(spaceman::config::Config::parse(&empty).is_ok());
    }
}

#[test]
fn choosing_main_in_settings_retains_solely_declared_named_workspace() {
    let d = ws("D");
    for source in [
        "version = 1\n[[workspaces]]\nworkspace = \"D\"\nmonitor = \"secondary\"\n",
        "version = 1\nworkspaces = [{ workspace = \"D\", monitor = \"secondary\" }]\n",
    ] {
        let mut doc = Doc::parse(source);
        assert_eq!(doc.error, None, "{source}");
        assert_eq!(doc.monitor(d).as_deref(), Some("secondary"));

        // Choosing Main in Settings passes `None` to `set_monitor`.
        doc.set_monitor(d, None);
        let saved = doc.source();
        let config = spaceman::config::Config::parse(&saved)
            .unwrap_or_else(|err| panic!("invalid TOML ({err}):\n{saved}"));
        let mut workspaces = spaceman::Workspaces::default();
        config.configure_workspaces(&mut workspaces);
        assert!(
            workspaces.is_enabled(d),
            "named workspace D must stay enabled:\n{saved}"
        );
        assert_eq!(
            workspaces.assignment(d),
            &spaceman::DisplaySelector::Main,
            "{saved}"
        );
        let reloaded = Doc::parse(&saved);
        assert!(
            reloaded
                .workspaces_with_referenced(WorkspaceId::numbered())
                .contains(&d),
            "Settings row for D must remain visible:\n{saved}"
        );
        assert_eq!(reloaded.monitor(d).as_deref(), Some("main"));

        // Clearing an unconfigured named workspace does not declare it.
        doc.set_monitor(ws("Z"), None);
        assert!(!doc.named_workspaces().contains(&ws("Z")));
    }

    // Control: when a route rule also references D, choosing Main removes the redundant entry
    // while keeping D enabled through the route rule.
    let mut routed = Doc::parse(concat!(
        "version = 1\n",
        "[[workspaces]]\n",
        "workspace = \"D\"\n",
        "monitor = \"secondary\"\n",
        "[[rules]]\n",
        "title = \"Synthetic\"\n",
        "action = \"route\"\n",
        "workspace = \"D\"\n",
    ));
    routed.set_monitor(d, None);
    let saved = routed.source();
    assert!(!saved.contains("[[workspaces]]"), "{saved}");
    let config = spaceman::config::Config::parse(&saved).unwrap();
    let mut workspaces = spaceman::Workspaces::default();
    config.configure_workspaces(&mut workspaces);
    assert!(workspaces.is_enabled(d));
    assert!(
        Doc::parse(&saved)
            .workspaces_with_referenced(WorkspaceId::numbered())
            .contains(&d)
    );
}

#[test]
fn doc_edit_and_save_preserves_external_edits_symlinks_and_detects_conflicts() {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "spaceman-agent-doc-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let target = dir.join("dotfiles.toml");
    let link = dir.join("config.toml");
    std::fs::write(
        &target,
        concat!(
            "version = 1\n\n",
            "[settings]\n",
            "hud = true\n\n",
            "[[rules]]\n",
            "bundle_id = \"com.apple.TextEdit\"\n",
            "action = \"route\"\n",
            "workspace = 1\n",
        ),
    )
    .unwrap();
    std::os::unix::fs::symlink("dotfiles.toml", &link).unwrap();

    let mut doc = Doc::load_from(link.clone());
    let dragged = doc.rules()[0].clone();

    // Simulate an external save changing hud = true -> hud = false while the user holds the card.
    std::fs::write(
        &target,
        concat!(
            "version = 1\n\n",
            "[settings]\n",
            "hud = false\n\n",
            "[[rules]]\n",
            "bundle_id = \"com.apple.TextEdit\"\n",
            "action = \"route\"\n",
            "workspace = 1\n",
        ),
    )
    .unwrap();

    doc.edit_and_save(|fresh| fresh.place_matching(&dragged, Place::Workspace(ws("2"))))
        .unwrap();

    // Symlink stays intact and both the external HUD edit and workspace move survive.
    assert!(
        std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    let on_disk = std::fs::read_to_string(&target).unwrap();
    let parsed = spaceman::config::Config::parse(&on_disk).unwrap();
    assert!(
        !parsed.hud(),
        "external hud = false must survive:\n{on_disk}"
    );
    assert!(!doc.hud());
    assert_eq!(doc.rules()[0].place, Place::Workspace(ws("2")));

    // Shifted index: external edit inserts a new rule above TextEdit before another move.
    let dragged_again = doc.rules()[0].clone();
    std::fs::write(
        &target,
        concat!(
            "version = 1\n\n",
            "[settings]\n",
            "hud = false\n\n",
            "[[rules]]\n",
            "bundle_id = \"com.apple.Safari\"\n",
            "action = \"float\"\n\n",
            "[[rules]]\n",
            "bundle_id = \"com.apple.TextEdit\"\n",
            "action = \"route\"\n",
            "workspace = 2\n",
        ),
    )
    .unwrap();
    doc.edit_and_save(|fresh| fresh.place_matching(&dragged_again, Place::Workspace(ws("3"))))
        .unwrap();
    assert_eq!(doc.rules().len(), 2);
    assert_eq!(
        doc.rules()[0].bundle_id.as_deref(),
        Some("com.apple.Safari")
    );
    assert_eq!(doc.rules()[0].place, Place::Float);
    assert_eq!(
        doc.rules()[1].bundle_id.as_deref(),
        Some("com.apple.TextEdit")
    );
    assert_eq!(doc.rules()[1].place, Place::Workspace(ws("3")));

    // Conflict: external edit deletes TextEdit's rule and changes hud back to true.
    let stale_rule = doc.rules()[1].clone();
    std::fs::write(
        &target,
        concat!(
            "version = 1\n\n",
            "[settings]\n",
            "hud = true\n\n",
            "[[rules]]\n",
            "bundle_id = \"com.apple.Safari\"\n",
            "action = \"float\"\n",
        ),
    )
    .unwrap();
    let err = doc
        .edit_and_save(|fresh| fresh.place_matching(&stale_rule, Place::Workspace(ws("4"))))
        .unwrap_err();
    assert!(err.contains("changed externally"), "{err}");
    // External file survives untouched and doc refreshes to match disk so the user can retry.
    assert!(doc.hud());
    assert_eq!(doc.rules().len(), 1);
    assert_eq!(
        doc.rules()[0].bundle_id.as_deref(),
        Some("com.apple.Safari")
    );
    let disk_after_conflict = std::fs::read_to_string(&target).unwrap();
    assert!(
        spaceman::config::Config::parse(&disk_after_conflict)
            .unwrap()
            .hud()
    );
    assert!(!disk_after_conflict.contains("TextEdit"));

    drop(std::fs::remove_dir_all(dir));
}

#[test]
fn empty_inline_workspaces_array_supports_assigning_a_display() {
    let mut doc = Doc::parse("version = 1\nworkspaces = []\n");
    assert_eq!(doc.error, None);
    assert_eq!(doc.monitor(ws("2")), None);
    doc.set_monitor(ws("2"), Some("secondary"));
    let saved = doc.source();
    assert!(
        spaceman::config::Config::parse(&saved).is_ok(),
        "invalid TOML:\n{saved}"
    );
    assert_eq!(
        Doc::parse(&saved).monitor(ws("2")).as_deref(),
        Some("secondary")
    );
}

const COMMENTED: &str = "version = 1\n\n# My window rules.\n\n[[rules]]\nbundle_id = \"com.example.A\"\naction = \"float\"\n\n# Disabled for now:\n# [[rules]]\n# bundle_id = \"com.example.B\"\n# action = \"ignore\"\n\n[[rules]]\nbundle_id = \"com.example.C\"\naction = \"float\"\n";

#[test]
fn removing_a_rule_keeps_a_commented_out_rule() {
    let mut doc = Doc::parse(COMMENTED);
    doc.remove(1);
    let source = doc.source();
    assert!(source.contains("Disabled for now"), "{source}");
    assert!(source.contains("My window rules"), "{source}");
    assert!(
        spaceman::config::Config::parse(&source).is_ok(),
        "invalid TOML:\n{source}"
    );
    assert_eq!(Doc::parse(&source).rules().len(), 1);
}

#[test]
fn removing_the_first_rule_keeps_the_heading_comment() {
    let mut doc = Doc::parse(COMMENTED);
    doc.remove(0);
    let source = doc.source();
    assert!(source.contains("My window rules"), "{source}");
    assert!(source.contains("Disabled for now"), "{source}");
    assert!(
        spaceman::config::Config::parse(&source).is_ok(),
        "invalid TOML:\n{source}"
    );
    let rules = Doc::parse(&source).rules();
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].bundle_id.as_deref(), Some("com.example.C"));
}

#[test]
fn control_changing_a_rule_keeps_both_comments() {
    let mut doc = Doc::parse(COMMENTED);
    doc.place(1, Place::Ignore);
    assert!(doc.source().contains("Disabled for now") && doc.source().contains("My window rules"));
}

#[test]
fn removing_all_rules_or_rules_before_another_section_keeps_detached_comments_in_place() {
    // Removing both rules in either order preserves both comments.
    for order in [[0, 0], [1, 0]] {
        let mut doc = Doc::parse(COMMENTED);
        for index in order {
            doc.remove(index);
        }
        let source = doc.source();
        assert!(doc.rules().is_empty(), "{source}");
        assert!(source.contains("My window rules"), "{source}");
        assert!(source.contains("Disabled for now"), "{source}");
        assert!(
            spaceman::config::Config::parse(&source).is_ok(),
            "invalid TOML:\n{source}"
        );
    }

    // With another section following [[rules]], preserved comments stay before that section.
    let with_following = format!("{COMMENTED}\n[shortcuts.bindings]\n\"alt+h\" = \"none\"\n");
    let mut doc = Doc::parse(&with_following);
    doc.remove(1);
    let source = doc.source();
    assert!(
        source.find("Disabled for now").unwrap() < source.find("[shortcuts.bindings]").unwrap(),
        "comment should stay before following section:\n{source}"
    );
    assert!(spaceman::config::Config::parse(&source).is_ok());

    // A comment directly above a removed rule (no blank line) goes with it, while a heading
    // separated by a blank line above it survives.
    let attached = concat!(
        "version = 1\n\n",
        "# Section heading.\n\n",
        "# Float app A.\n",
        "[[rules]]\n",
        "bundle_id = \"com.example.A\"\n",
        "action = \"float\"\n\n",
        "[[rules]]\n",
        "bundle_id = \"com.example.C\"\n",
        "action = \"float\"\n",
    );
    let mut doc = Doc::parse(attached);
    doc.remove(0);
    let source = doc.source();
    assert!(source.contains("Section heading"), "{source}");
    assert!(!source.contains("Float app A"), "{source}");
    assert!(spaceman::config::Config::parse(&source).is_ok());

    // A trailing comment at EOF after the only rule survives removing that rule.
    let eof_comment = concat!(
        "version = 1\n\n",
        "# Heading.\n\n",
        "[[rules]]\n",
        "bundle_id = \"com.example.A\"\n",
        "action = \"float\"\n\n",
        "# Footer at EOF.\n",
    );
    let mut doc = Doc::parse(eof_comment);
    doc.remove(0);
    let source = doc.source();
    assert!(source.contains("Heading."), "{source}");
    assert!(source.contains("Footer at EOF."), "{source}");
    assert!(
        source.find("Heading.").unwrap() < source.find("Footer at EOF.").unwrap(),
        "{source}"
    );
    assert!(spaceman::config::Config::parse(&source).is_ok());

    // Detached comments around and inside an inline rules array also survive rule removal.
    let inline = concat!(
        "version = 1\n\n",
        "# Inline rules heading.\n\n",
        "rules = [\n",
        "  # First block.\n\n",
        "  { bundle_id = \"com.example.A\", action = \"float\" },\n\n",
        "  # Disabled inline rule:\n",
        "  # { bundle_id = \"com.example.B\", action = \"ignore\" },\n\n",
        "  { bundle_id = \"com.example.C\", action = \"float\" },\n",
        "]\n",
    );
    let mut doc = Doc::parse(inline);
    doc.remove(1);
    let source = doc.source();
    assert!(source.contains("Inline rules heading."), "{source}");
    assert!(source.contains("First block."), "{source}");
    assert!(source.contains("Disabled inline rule:"), "{source}");
    assert!(spaceman::config::Config::parse(&source).is_ok());
    doc.remove(0);
    let empty_inline = doc.source();
    assert!(
        empty_inline.contains("Inline rules heading."),
        "{empty_inline}"
    );
    assert!(empty_inline.contains("First block."), "{empty_inline}");
    assert!(
        empty_inline.contains("Disabled inline rule:"),
        "{empty_inline}"
    );
    assert!(spaceman::config::Config::parse(&empty_inline).is_ok());
}

#[test]
fn workspace_display_edits_preserve_key_and_detached_comments() {
    let nine = WorkspaceId::new(9).unwrap();

    // 1. Updating monitor in a [[workspaces]] table preserves the comment above monitor.
    let mut doc = Doc::parse(
        "version = 1\n\n[[workspaces]]\n# Unchanged workspace key.\nworkspace = 9\n# Display note.\nmonitor = \"Studio Display\"\n",
    );
    doc.set_monitor(nine, Some("main"));
    assert!(doc.source().contains("# Unchanged workspace key."));
    assert!(doc.source().contains("# Display note."));
    assert_eq!(doc.monitor(nine).as_deref(), Some("main"));
    assert!(spaceman::config::Config::parse(&doc.source()).is_ok());

    // 2. Clearing a [[workspaces]] table preserves detached heading comments above it.
    let mut doc = Doc::parse(
        "version = 1\n\n# Display note.\n\n[[workspaces]]\nworkspace = 9\nmonitor = \"Studio Display\"\n\n[[workspaces]]\nworkspace = 4\nmonitor = \"secondary\"\n",
    );
    doc.set_monitor(nine, None);
    assert!(doc.source().contains("# Display note."));
    assert_eq!(doc.monitor(nine), None);
    assert!(spaceman::config::Config::parse(&doc.source()).is_ok());

    // 3. Clearing the last entry of an inline workspaces array preserves detached comments.
    let mut doc = Doc::parse(
        "version = 1\n\n# Display note.\n\nworkspaces = [\n  { workspace = 9, monitor = \"Studio Display\" },\n]\n",
    );
    doc.set_monitor(nine, None);
    assert!(doc.source().contains("# Display note."));
    assert_eq!(doc.monitor(nine), None);
    assert!(spaceman::config::Config::parse(&doc.source()).is_ok());

    // 4. Control: an attached comment with no blank line above a removed entry still goes with it.
    let mut doc = Doc::parse(
        "version = 1\n\n# Display note.\n[[workspaces]]\nworkspace = 9\nmonitor = \"Studio Display\"\n",
    );
    doc.set_monitor(nine, None);
    assert!(!doc.source().contains("# Display note."));
    assert!(spaceman::config::Config::parse(&doc.source()).is_ok());
}

#[test]
fn rule_placement_and_settings_edits_preserve_retained_key_comments() {
    const SOURCE: &str = "version = 1\n\n# Detached rules heading.\n\n[[rules]]\n# Bundle note.\nbundle_id = \"com.example.App\"\n# Action note.\naction = \"route\"\n# Workspace note.\nworkspace = 2\n";

    // Route -> route keeps comments above both retained action and workspace keys.
    let mut doc = Doc::parse(SOURCE);
    doc.place(0, Place::Workspace(WorkspaceId::new(3).unwrap()));
    let saved = doc.source();
    assert!(spaceman::config::Config::parse(&saved).is_ok());
    assert!(saved.contains("# Detached rules heading."));
    assert!(saved.contains("# Bundle note."));
    assert!(saved.contains("# Action note."));
    assert!(saved.contains("# Workspace note."));
    assert_eq!(
        doc.rules()[0].place,
        Place::Workspace(WorkspaceId::new(3).unwrap())
    );

    // Route -> float removes workspace and keeps the comment above retained action.
    let mut doc = Doc::parse(SOURCE);
    doc.place(0, Place::Float);
    let saved = doc.source();
    assert!(spaceman::config::Config::parse(&saved).is_ok());
    assert!(!saved.contains("workspace ="));
    assert!(saved.contains("# Bundle note."));
    assert!(saved.contains("# Action note."));
    assert_eq!(doc.rules()[0].place, Place::Float);

    // Settings / gaps / shortcut modifier updates also preserve comments above retained keys.
    let mut doc = Doc::parse(
        "version = 1\n\n[settings]\n# Mode note.\nmouse_follows_focus = \"display\"\n\n[settings.gaps]\n# Gap note.\ninner = 8\n\n[shortcuts]\n# Modifier note.\nmodifier = \"control\"\n",
    );
    doc.set_mouse_follows_focus(spaceman::config::MouseFollowsFocus::Window);
    let mut spacing = doc.spacing();
    spaceman::spacing::SpacingKey::Inner.set(&mut spacing, 12);
    doc.set_spacing(spacing);
    doc.set_shortcut_modifier(spaceman::shortcuts::Modifiers {
        control: true,
        option: true,
        command: false,
        shift: false,
    });
    let saved = doc.source();
    assert!(spaceman::config::Config::parse(&saved).is_ok());
    assert!(saved.contains("# Mode note."));
    assert!(saved.contains("# Gap note."));
    assert!(saved.contains("# Modifier note."));
}

#[test]
fn clearing_last_layout_spacing_preserves_detached_section_headings() {
    use spaceman::spacing::Spacing;
    let width_source =
        "version = 1\n\n# Layout notes.\n\n[settings]\nsingle_window_max_width = 1680\n";
    let mut doc = Doc::parse(width_source);
    doc.set_spacing(Spacing::NONE);
    let saved = doc.source();
    assert!(spaceman::config::Config::parse(&saved).is_ok(), "{saved}");
    assert!(saved.contains("# Layout notes."), "{saved}");
    assert!(!saved.contains("[settings]"), "{saved}");

    let gaps_source = "version = 1\n\n# Gap notes.\n\n[settings.gaps]\ninner = 6\n";
    let mut doc = Doc::parse(gaps_source);
    doc.set_spacing(Spacing::NONE);
    let saved = doc.source();
    assert!(spaceman::config::Config::parse(&saved).is_ok(), "{saved}");
    assert!(saved.contains("# Gap notes."), "{saved}");
    assert!(!saved.contains("[settings.gaps]"), "{saved}");
}

/// Settings › Layout's view of `[settings.gaps]` with `gaps` inside it.
fn gaps(gaps: &str) -> Doc {
    let doc = Doc::parse(&format!("# Mine.\nversion = 1\n\n[settings.gaps]\n{gaps}"));
    assert_eq!(doc.error, None);
    doc
}

/// What the running agent would tile with once `doc` is saved.
fn saved_spacing(doc: &Doc) -> spaceman::spacing::Spacing {
    spaceman::config::Config::parse(&doc.source())
        .unwrap()
        .spacing()
}

#[test]
fn spacing_shows_the_edges_outer_resolves_to() {
    use spaceman::spacing::Spacing;
    let doc = gaps("inner = 6\nouter = 8\ntop = 0\n");
    assert_eq!(
        doc.spacing(),
        Spacing {
            inner: 6,
            top: 0,
            ..Spacing::NONE.with_edges(8)
        }
    );
    assert_eq!(doc.spacing(), saved_spacing(&doc));
}

#[test]
fn unrelated_edits_keep_outer_and_its_explicit_edges() {
    use spaceman::spacing::Spacing;
    for source in [
        "inner = 6\nouter = 8 # Everywhere.\ntop = 0\nleft = 8\n",
        "inner = 6\nouter = 0\ntop = 0\nleft = 8\n",
    ] {
        let mut doc = gaps(source);
        let before = doc.spacing();
        doc.set_spacing(before);
        doc.set_hud(false);
        doc.set_hud(true);
        doc.set_monitor(ws("2"), Some("secondary"));
        doc.set_monitor(ws("2"), None);
        assert_eq!(doc.source(), gaps(source).source());

        doc.set_spacing(Spacing {
            inner: 10,
            ..before
        });
        assert_eq!(
            doc.source(),
            gaps(&source.replace("inner = 6", "inner = 10")).source()
        );
        doc.set_spacing(Spacing {
            single_window_max_width: 1680,
            ..doc.spacing()
        });
        assert!(
            doc.source()
                .contains(&source.replace("inner = 6", "inner = 10"))
        );
        assert_eq!(
            saved_spacing(&doc),
            Spacing {
                inner: 10,
                single_window_max_width: 1680,
                ..before
            }
        );
    }
}

#[test]
fn an_edge_set_to_zero_does_not_fall_back_to_outer() {
    use spaceman::spacing::{Spacing, SpacingKey};
    for key in SpacingKey::EDGES {
        let mut doc = gaps("outer = 8\n");
        let mut spacing = doc.spacing();
        key.set(&mut spacing, 0);
        doc.set_spacing(spacing);
        assert_eq!(
            doc.source(),
            gaps(&format!("outer = 8\n{} = 0\n", key.gap_name().unwrap())).source()
        );
        assert_eq!(saved_spacing(&doc), spacing);

        // Another slider's save leaves it at zero, and it can be raised again.
        let inner = Spacing {
            inner: 4,
            ..doc.spacing()
        };
        doc.set_spacing(inner);
        assert_eq!(saved_spacing(&doc), inner);
        doc.set_spacing(Spacing::NONE.with_edges(8));
        assert_eq!(saved_spacing(&doc), Spacing::NONE.with_edges(8));
    }

    let mut doc = gaps("inner = 6\nouter = 8\n");
    doc.set_spacing(Spacing::NONE);
    assert_eq!(
        doc.source(),
        gaps("outer = 8\ntop = 0\nbottom = 0\nleft = 0\nright = 0\n").source()
    );
    assert_eq!(saved_spacing(&doc), Spacing::NONE);
}

#[test]
fn editing_one_edge_leaves_the_others_following_outer() {
    use spaceman::spacing::Spacing;
    let mut doc = gaps("inner = 6\nouter = 8\n");
    doc.set_spacing(Spacing {
        top: 30,
        ..doc.spacing()
    });
    assert_eq!(
        doc.source(),
        gaps("inner = 6\nouter = 8\ntop = 30\n").source()
    );
    // Every edge together still overrides, and the shorthand is there once they go.
    doc.set_spacing(doc.spacing().with_edges(12));
    assert_eq!(saved_spacing(&doc).uniform_edges(), Some(12));
    assert!(doc.source().contains("outer = 8\n"), "{}", doc.source());
}

#[test]
fn inline_settings_keep_outer_when_something_else_changes() {
    use spaceman::spacing::Spacing;
    for source in [
        "version = 1\n[settings]\nhud = true\ngaps = { inner = 6, outer = 8, top = 0 }\n",
        "version = 1\nsettings = { hud = true, gaps = { inner = 6, outer = 8, top = 0 } }\n",
        "version = 1\n[settings]\nhud = true\ngaps.inner = 6\ngaps.outer = 8\ngaps.top = 0\n",
        "version = 1\nsettings.hud = true\nsettings.gaps.inner = 6\nsettings.gaps.outer = 8\nsettings.gaps.top = 0\n",
    ] {
        let mut doc = Doc::parse(source);
        assert_eq!(doc.error, None);
        let before = doc.spacing();
        assert_eq!(before, saved_spacing(&doc));
        let wider = Spacing {
            inner: 10,
            ..before
        };
        doc.set_spacing(wider);
        assert_eq!(doc.source(), source.replace("inner = 6", "inner = 10"));
        doc.set_hud(false);
        assert_eq!(saved_spacing(&doc), wider, "{}", doc.source());
        assert!(
            !spaceman::config::Config::parse(&doc.source())
                .unwrap()
                .hud()
        );
        doc.set_spacing(Spacing { left: 0, ..wider });
        assert_eq!(saved_spacing(&doc), Spacing { left: 0, ..wider });
        doc.set_spacing(Spacing::NONE);
        assert_eq!(saved_spacing(&doc), Spacing::NONE, "{}", doc.source());
        assert!(doc.source().contains("outer = 8"), "{}", doc.source());
    }

    let mut doc = Doc::parse("version = 1\nsettings = { hud = false }\n");
    let spacing = Spacing::NONE.with_edges(8);
    doc.set_spacing(spacing);
    assert_eq!(saved_spacing(&doc), spacing, "{}", doc.source());
    assert!(!doc.hud());
    doc.set_spacing(Spacing::NONE);
    doc.set_hud(true);
    assert_eq!(doc.source(), "version = 1\n");
}

#[test]
fn spacing_without_outer_still_drops_an_edge_set_to_zero() {
    use spaceman::spacing::Spacing;
    let mut doc = gaps("inner = 6\nleft = 20\n");
    doc.set_spacing(Spacing {
        left: 0,
        ..doc.spacing()
    });
    assert_eq!(doc.source(), gaps("inner = 6\n").source());
    doc.set_spacing(Spacing::NONE);
    assert_eq!(doc.source(), "# Mine.\nversion = 1\n");
}

#[test]
fn unrelated_edits_leave_an_explicit_zero_for_a_later_outer() {
    use spaceman::spacing::Spacing;
    for source in [
        "version = 1\n\n[settings] # Displays.\n\n[settings.gaps]\ninner = 6\ntop = 0\n",
        "version = 1\n\n[settings] # Displays.\ngaps = { inner = 6, top = 0 } # Flush.\n",
        "version = 1\n\n[settings] # Displays.\ngaps = { top = 0 } # Flush.\n",
    ] {
        let mut doc = Doc::parse(source);
        doc.set_spacing(Spacing {
            single_window_max_width: 1680,
            ..doc.spacing()
        });
        let saved = doc.source();
        assert_eq!(
            saved.replace("single_window_max_width = 1680\n", ""),
            source
        );
        let padded = Doc::parse(&saved.replace("top = 0", "outer = 8, top = 0").replace(
            "inner = 6\nouter = 8, top = 0",
            "inner = 6\nouter = 8\ntop = 0",
        ));
        assert_eq!(padded.error, None);
        assert_eq!((padded.spacing().top, padded.spacing().left), (0, 8));
    }
}

#[test]
fn edits_keep_the_comments_around_outer() {
    use spaceman::spacing::Spacing;
    for source in [
        "version = 1\n[settings.gaps]\nouter = 8 # Around.\n# Between.\ninner = 6 # Tight.\ntop = 0\n",
        "version = 1\n[settings]\ngaps = {\n  outer = 8, # Around.\n  inner = 6, # Tight.\n  top = 0,\n}\n",
    ] {
        let mut doc = Doc::parse(source);
        assert_eq!(doc.error, None);
        // `toml_edit` reflows a multiline inline table even unedited.
        let unedited = doc.source();
        assert!(unedited.contains("outer = 8"), "{unedited}");
        doc.set_spacing(Spacing {
            inner: 10,
            ..doc.spacing()
        });
        assert_eq!(doc.source(), unedited.replace("inner = 6", "inner = 10"));
    }

    let source = "version = 1\nsettings = {\n  gaps = { outer = 8 }, # Around.\n  single_window_max_width = 1440, # Wide.\n}\n";
    let mut doc = Doc::parse(source);
    assert_eq!(doc.error, None);
    let unedited = doc.source();
    assert!(unedited.contains("1440"), "{unedited}");
    doc.set_spacing(Spacing {
        single_window_max_width: 1680,
        ..doc.spacing()
    });
    assert_eq!(doc.source(), unedited.replace("1440", "1680"));
}

#[test]
fn all_new_windows_writes_windows_all_and_turns_its_lab_on() {
    let mut doc = Doc::parse(
        "version = 1\n\n[[rules]]\nbundle_id = \"org.mozilla.firefox\"\naction = \"route\"\nworkspace = 2\n",
    );
    let rule = doc.rules().remove(0);
    assert!(!rule.all_windows);

    doc.set_all_windows_matching(&rule, true).unwrap();
    let source = doc.source();
    assert!(source.contains("windows = \"all\""), "{source}");
    assert!(doc.lab(LabFeature::RouteAllWindows), "{source}");
    let config = spaceman::config::Config::parse(&source).unwrap();
    assert!(config.labs().route_all_windows);
    let rule = doc.rules().remove(0);
    assert!(rule.all_windows);

    // Back to the first window only; the Lab stays on for the app's other rules.
    doc.set_all_windows_matching(&rule, false).unwrap();
    assert!(
        !doc.source().contains("windows = \"all\""),
        "{}",
        doc.source()
    );
    assert!(!doc.rules()[0].all_windows);

    // Floating the app drops `windows`, which only route rules take.
    let rule = doc.rules().remove(0);
    doc.set_all_windows_matching(&rule, true).unwrap();
    let rule = doc.rules().remove(0);
    doc.place_matching(&rule, Place::Float).unwrap();
    assert!(!doc.rules()[0].all_windows);
    spaceman::config::Config::parse(&doc.source()).unwrap();
}
