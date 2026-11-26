use super::*;
use fractile::{DisplaySelector, config::save_workspace_monitor_at};

#[test]
fn parses_pause_on_builtin_display_with_false_default_and_preserves_on_workspace_save() {
    assert!(!Config::default().pause_on_builtin_display());
    assert!(
        !Config::parse("version = 1")
            .unwrap()
            .pause_on_builtin_display()
    );
    assert!(
        !Config::parse("version = 1\n[settings]\npause_on_builtin_display = false\n")
            .unwrap()
            .pause_on_builtin_display()
    );
    assert!(
        Config::parse("version = 1\n[settings]\npause_on_builtin_display = true\n")
            .unwrap()
            .pause_on_builtin_display()
    );
    assert!(
        Config::parse("version = 1\n[settings]\npause_on_builtin_display = \"yes\"\n").is_err()
    );

    let temp = TempConfig::new("builtin-preserve");
    temp.write("version = 1\n[settings]\npause_on_builtin_display = true\n");
    let saved = save_workspace_monitor_at(
        &temp.path,
        WorkspaceId::new(9).unwrap(),
        &DisplaySelector::Main,
    )
    .unwrap();
    assert!(saved.pause_on_builtin_display());
    assert!(
        temp.read().contains("pause_on_builtin_display = true"),
        "{}",
        temp.read()
    );
}

#[test]
fn config_gets_sets_shows_and_resets_pause_on_builtin_display() {
    let temp = TempConfig::new("pause-on-builtin-display");

    assert_eq!(
        temp.run(&["config", "get", "pause_on_builtin_display"])
            .stdout
            .as_deref(),
        Some("false")
    );
    assert_eq!(
        temp.run(&["config", "get", "settings.pause_on_builtin_display"])
            .stdout
            .as_deref(),
        Some("false")
    );

    let set_true = temp.run(&["config", "set", "pause_on_builtin_display", "true"]);
    assert_eq!(set_true.exit_code, 0);
    assert_eq!(
        set_true.stdout.as_deref(),
        Some("set pause_on_builtin_display = true")
    );
    assert!(
        Config::parse(&temp.read())
            .unwrap()
            .pause_on_builtin_display()
    );
    assert_eq!(
        temp.run(&["config", "get", "pause_on_builtin_display"])
            .stdout
            .as_deref(),
        Some("true")
    );

    let show: Value = serde_json::from_str(
        temp.run(&["config", "show", "--json"])
            .stdout
            .as_deref()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(show["settings"]["pause_on_builtin_display"], true);

    let set_false = temp.run(&[
        "config",
        "set",
        "settings.pause_on_builtin_display",
        "false",
        "--json",
    ]);
    assert_eq!(set_false.exit_code, 0);
    let set_json: Value = serde_json::from_str(set_false.stdout.as_deref().unwrap()).unwrap();
    assert_eq!(set_json["key"], "pause_on_builtin_display");
    assert_eq!(set_json["value"], false);
    assert!(
        !Config::parse(&temp.read())
            .unwrap()
            .pause_on_builtin_display()
    );

    let bad = temp.run(&["config", "set", "pause_on_builtin_display", "yes"]);
    assert_eq!(bad.exit_code, 1);
    assert!(
        bad.stderr
            .as_deref()
            .unwrap_or_default()
            .contains("expected 'true' or 'false'"),
        "{:?}",
        bad.stderr
    );

    let reset = temp.run(&["config", "reset", "pause_on_builtin_display"]);
    assert_eq!(reset.exit_code, 0);
    assert_eq!(
        reset.stdout.as_deref(),
        Some("reset pause_on_builtin_display = false")
    );
    assert!(
        !temp.read().contains("pause_on_builtin_display"),
        "{}",
        temp.read()
    );
}
