use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use serde_json::Value;
use spaceman::{
    AppInstanceId, ObservedWindow, PhysicalWindowId, Rect, WorkspaceId,
    config::{Config, HudPosition, HudStyle, MouseFollowsFocus, WindowDisposition},
};

mod builtin_display;
mod labs;
mod regressions;

use super::{Outcome, try_run_with_config_path};

struct TempConfig {
    dir: PathBuf,
    path: PathBuf,
}

impl TempConfig {
    fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "spaceman-cli-test-{label}-{}-{id}",
            std::process::id()
        ));
        let path = dir.join("config.toml");
        Self { dir, path }
    }

    fn write(&self, source: &str) {
        fs::create_dir_all(&self.dir).unwrap();
        fs::write(&self.path, source).unwrap();
    }

    fn read(&self) -> String {
        fs::read_to_string(&self.path).unwrap()
    }

    fn run(&self, args: &[&str]) -> Outcome {
        run_cli(args, Some(&self.path))
    }
}

impl Drop for TempConfig {
    fn drop(&mut self) {
        drop(fs::remove_dir_all(&self.dir));
    }
}

fn run_cli(args: &[&str], config_path: Option<&Path>) -> Outcome {
    let (&command, rest) = args.split_first().expect("args must be non-empty");
    try_run_with_config_path("spaceman", command, rest, config_path)
        .expect("command should be handled by cli module")
}

fn sample_window(title: Option<&str>) -> ObservedWindow {
    ObservedWindow {
        id: PhysicalWindowId {
            app: AppInstanceId {
                pid: 42,
                launched_at: 1,
            },
            window_id: 99,
        },
        frame: Rect::default(),
        resizable: true,
        title: title.map(std::sync::Arc::from),
        document: None,
    }
}

#[test]
fn help_and_version_exit_zero_on_stdout() {
    for args in [
        &["--help"][..],
        &["-h"][..],
        &["help"][..],
        &["help", "config"][..],
        &["config", "--help"][..],
        &["config", "-h"][..],
        &["config", "set", "--help"][..],
        &["rules", "--help"][..],
        &["rules", "add", "--help"][..],
        &["workspaces", "--help"][..],
        &["workspaces", "assign", "--help"][..],
        &["replay", "--help"][..],
        &["doctor", "--help"][..],
        &["config-check", "--help"][..],
    ] {
        let outcome = run_cli(args, None);
        assert_eq!(outcome.exit_code, 0, "args: {args:?}");
        assert!(outcome.stdout.as_deref().is_some_and(|s| !s.is_empty()));
        assert!(outcome.stderr.is_none());
    }

    #[cfg(target_os = "macos")]
    for args in [
        &["ctl", "--help"][..],
        &["ctl", "-h"][..],
        &["ctl", "help"][..],
        &["help", "ctl"][..],
        &["open", "--help"][..],
        &["help", "open"][..],
        &["snapshot", "--help"][..],
        &["displays", "--help"][..],
    ] {
        let outcome = run_cli(args, None);
        assert_eq!(outcome.exit_code, 0, "args: {args:?}");
        assert!(outcome.stdout.as_deref().is_some_and(|s| !s.is_empty()));
        assert!(outcome.stderr.is_none());
    }

    let expected_version = format!("spaceman {}", spaceman::build_version());
    for flag in ["--version", "-V"] {
        let outcome = run_cli(&[flag], None);
        assert_eq!(outcome.exit_code, 0);
        assert_eq!(outcome.stdout.as_deref(), Some(expected_version.as_str()));
        assert!(outcome.stderr.is_none());
    }

    let unknown = run_cli(&["help", "nonexistent"], None);
    assert_eq!(unknown.exit_code, 2);
    assert!(unknown.stderr.unwrap().contains("unknown command"));
}

#[test]
fn config_path_show_get_set_and_check_round_trip() {
    let temp = TempConfig::new("config");

    let path_out = temp.run(&["config", "path"]);
    assert_eq!(path_out.exit_code, 0);
    assert_eq!(
        path_out.stdout.as_deref(),
        Some(temp.path.display().to_string().as_str())
    );

    let path_json: Value = serde_json::from_str(
        temp.run(&["config", "path", "--json"])
            .stdout
            .as_deref()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(path_json["exists"], false);

    // Defaults when config file does not exist yet
    assert_eq!(
        temp.run(&["config", "get", "hud"]).stdout.as_deref(),
        Some("true")
    );
    assert_eq!(
        temp.run(&["config", "get", "hud_style"]).stdout.as_deref(),
        Some("icons")
    );
    assert_eq!(
        temp.run(&["config", "get", "settings.hud_style"])
            .stdout
            .as_deref(),
        Some("icons")
    );

    let check_default = temp.run(&["config", "check", "--json"]);
    assert_eq!(check_default.exit_code, 0);
    let check_json: Value = serde_json::from_str(&check_default.stdout.unwrap()).unwrap();
    assert_eq!(check_json["valid"], true);
    assert_eq!(check_json["exists"], false);

    // Mutate hud and hud_style atomically
    let set_hud = temp.run(&["config", "set", "hud", "false", "--json"]);
    assert_eq!(set_hud.exit_code, 0);
    let set_hud_json: Value = serde_json::from_str(&set_hud.stdout.unwrap()).unwrap();
    assert_eq!(set_hud_json["key"], "hud");
    assert_eq!(set_hud_json["value"], false);

    let set_style = temp.run(&["config", "set", "hud_style", "layouts"]);
    assert_eq!(set_style.exit_code, 0);
    assert_eq!(set_style.stdout.as_deref(), Some("set hud_style = layouts"));

    // Verify strict Config::parse accepts what we wrote
    let parsed = Config::parse(&temp.read()).unwrap();
    assert!(!parsed.hud());
    assert_eq!(parsed.hud_style(), HudStyle::Layouts);

    // Verify get & show reflect changes
    let get_hud_json: Value = serde_json::from_str(
        temp.run(&["config", "get", "hud", "--json"])
            .stdout
            .as_deref()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(get_hud_json["value"], false);

    let show_toml = temp.run(&["config", "show"]);
    assert_eq!(show_toml.exit_code, 0);
    let show_parsed = Config::parse(&show_toml.stdout.unwrap()).unwrap();
    assert!(!show_parsed.hud());
    assert_eq!(show_parsed.hud_style(), HudStyle::Layouts);

    let show_json: Value = serde_json::from_str(
        temp.run(&["config", "show", "--json"])
            .stdout
            .as_deref()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(show_json["version"], 1);
    assert_eq!(show_json["exists"], true);
    assert_eq!(show_json["settings"]["hud"], false);
    assert_eq!(show_json["settings"]["hud_style"], "layouts");

    let check_explicit = temp.run(&["config", "check", temp.path.to_str().unwrap()]);
    assert_eq!(check_explicit.exit_code, 0);
    assert_eq!(
        check_explicit.stdout.as_deref(),
        Some("configuration is valid")
    );
}

#[test]
fn config_gets_sets_and_shows_mouse_follows_focus() {
    let temp = TempConfig::new("config-mff");
    assert_eq!(
        temp.run(&["config", "get", "mouse_follows_focus"])
            .stdout
            .as_deref(),
        Some("off")
    );
    assert_eq!(
        temp.run(&["config", "get", "settings.mouse_follows_focus"])
            .stdout
            .as_deref(),
        Some("off")
    );

    let set_display = temp.run(&["config", "set", "mouse_follows_focus", "display"]);
    assert_eq!(set_display.exit_code, 0);
    assert_eq!(
        set_display.stdout.as_deref(),
        Some("set mouse_follows_focus = display")
    );
    assert_eq!(
        Config::parse(&temp.read()).unwrap().mouse_follows_focus(),
        MouseFollowsFocus::Display
    );
    assert_eq!(
        temp.run(&["config", "get", "mouse_follows_focus"])
            .stdout
            .as_deref(),
        Some("display")
    );

    let set_window = temp.run(&[
        "config",
        "set",
        "settings.mouse_follows_focus",
        "window",
        "--json",
    ]);
    assert_eq!(set_window.exit_code, 0);
    let set_json: Value = serde_json::from_str(&set_window.stdout.unwrap()).unwrap();
    assert_eq!(set_json["key"], "mouse_follows_focus");
    assert_eq!(set_json["value"], "window");

    let show: Value = serde_json::from_str(
        temp.run(&["config", "show", "--json"])
            .stdout
            .as_deref()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(show["settings"]["mouse_follows_focus"], "window");

    let bad = temp.run(&["config", "set", "mouse_follows_focus", "monitor"]);
    assert_eq!(bad.exit_code, 1);
    assert!(
        bad.stderr
            .unwrap()
            .contains("expected 'off', 'display', or 'window'")
    );
}

#[test]
fn config_gets_sets_and_resets_hud_position() {
    let temp = TempConfig::new("config-hud-position");
    temp.write("# Mine.\nversion = 1\n[settings]\nhud_style = \"layouts\"\n");
    assert_eq!(
        temp.run(&["config", "get", "hud_position"])
            .stdout
            .as_deref(),
        Some("bottom")
    );
    assert_eq!(
        temp.run(&["config", "get", "settings.hud_position"])
            .stdout
            .as_deref(),
        Some("bottom")
    );

    let set_top = temp.run(&["config", "set", "settings.hud_position", "top"]);
    assert_eq!(set_top.exit_code, 0);
    assert_eq!(set_top.stdout.as_deref(), Some("set hud_position = top"));
    assert_eq!(
        Config::parse(&temp.read()).unwrap().hud_position(),
        HudPosition::Top
    );
    assert!(temp.read().contains("hud_style = \"layouts\""));

    assert_eq!(
        temp.run(&["config", "get", "hud_position"])
            .stdout
            .as_deref(),
        Some("top")
    );
    let show: Value = serde_json::from_str(
        temp.run(&["config", "show", "--json"])
            .stdout
            .as_deref()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(show["settings"]["hud_position"], "top");
    assert_eq!(show["settings"]["hud_style"], "layouts");

    let reset = temp.run(&["config", "reset", "hud_position"]);
    assert_eq!(reset.exit_code, 0);
    assert_eq!(reset.stdout.as_deref(), Some("reset hud_position = bottom"));
    let source = temp.read();
    assert!(!source.contains("hud_position"), "{source}");
    assert!(source.contains("hud_style = \"layouts\""), "{source}");
    assert_eq!(
        Config::parse(&source).unwrap().hud_position(),
        HudPosition::Bottom
    );

    // Resetting the only key also removes the table it emptied.
    temp.write("version = 1\n[settings]\nhud_position = \"top\"\n");
    let reset_json: Value = serde_json::from_str(
        temp.run(&["config", "reset", "settings.hud_position", "--json"])
            .stdout
            .as_deref()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(reset_json["key"], "hud_position");
    assert_eq!(reset_json["value"], "bottom");
    assert!(!temp.read().contains("[settings]"), "{}", temp.read());

    let bad = temp.run(&["config", "set", "hud_position", "center"]);
    assert_eq!(bad.exit_code, 1);
    assert!(bad.stderr.unwrap().contains("expected 'bottom' or 'top'"));
}

#[test]
fn config_gets_sets_and_shows_spacing() {
    let temp = TempConfig::new("config-spacing");
    temp.write("# Mine.\nversion = 1\n[settings]\nhud = false\n");
    assert_eq!(
        temp.run(&["config", "get", "gaps.inner"]).stdout.as_deref(),
        Some("0")
    );
    for (key, value) in [
        ("gaps.inner", "8"),
        ("settings.gaps.top", "32"),
        ("gaps.right", "12"),
        ("single_window_max_width", "1600"),
        ("single_window_max_height", "1050"),
        ("labs.single_window_height", "true"),
    ] {
        let set = temp.run(&["config", "set", key, value]);
        assert_eq!(set.exit_code, 0, "{:?}", set.stderr);
    }
    let spacing = Config::parse(&temp.read()).unwrap().spacing();
    assert_eq!(
        (spacing.inner, spacing.top, spacing.right, spacing.bottom),
        (8, 32, 12, 0)
    );
    assert_eq!(spacing.single_window_max_width, 1600);
    assert_eq!(spacing.single_window_max_height, 1050);
    assert!(!Config::parse(&temp.read()).unwrap().hud());
    assert_eq!(
        temp.run(&["config", "get", "gaps.top"]).stdout.as_deref(),
        Some("32")
    );

    let show: Value = serde_json::from_str(
        temp.run(&["config", "show", "--json"])
            .stdout
            .as_deref()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(show["settings"]["gaps"]["inner"], 8);
    assert_eq!(show["settings"]["gaps"]["left"], 0);
    assert_eq!(show["settings"]["single_window_max_width"], 1600);
    assert_eq!(show["settings"]["single_window_max_height"], 1050);
    let shown = temp.run(&["config", "show"]).stdout.unwrap();
    assert_eq!(Config::parse(&shown).unwrap().spacing(), spacing);

    // Zero is the default and leaves the file.
    assert_eq!(temp.run(&["config", "set", "gaps.inner", "0"]).exit_code, 0);
    assert!(!temp.read().contains("inner"), "{}", temp.read());

    let too_big = temp.run(&["config", "set", "gaps.inner", "49"]);
    assert_eq!(too_big.exit_code, 1);
    assert!(
        too_big
            .stderr
            .unwrap()
            .contains("settings.gaps.inner must be between 0 and 48 px, got 49")
    );
    let not_a_number = temp.run(&["config", "set", "gaps.left", "wide"]);
    assert_eq!(not_a_number.exit_code, 1);
    assert!(not_a_number.stderr.unwrap().contains("whole number"));
    assert_eq!(Config::parse(&temp.read()).unwrap().spacing().right, 12);
}

/// `spaceman config get <key>`, which must succeed.
fn get(temp: &TempConfig, key: &str) -> String {
    let got = temp.run(&["config", "get", key]);
    assert_eq!(got.exit_code, 0, "{:?}", got.stderr);
    got.stdout.unwrap()
}

fn set(temp: &TempConfig, key: &str, value: &str) {
    let set = temp.run(&["config", "set", key, value]);
    assert_eq!(set.exit_code, 0, "{:?}", set.stderr);
    assert_eq!(get(temp, key), value);
}

/// `[settings.gaps]` as the file spells it.
fn written_gaps(temp: &TempConfig) -> Value {
    let file: toml::Table = toml::from_str(&temp.read()).unwrap();
    serde_json::to_value(&file["settings"]["gaps"]).unwrap()
}

#[test]
fn config_sets_outer_around_explicit_edges_and_reports_resolved_ones() {
    let temp = TempConfig::new("config-outer");
    assert_eq!(get(&temp, "gaps.outer"), "0");
    set(&temp, "gaps.outer", "8");
    assert_eq!(written_gaps(&temp), serde_json::json!({ "outer": 8 }));
    set(&temp, "gaps.inner", "6");
    set(&temp, "gaps.top", "32");
    assert_eq!(
        written_gaps(&temp),
        serde_json::json!({ "inner": 6, "outer": 8, "top": 32 })
    );
    assert_eq!(get(&temp, "gaps.top"), "32");
    assert_eq!(get(&temp, "settings.gaps.left"), "8");

    // A zero edge stays in the file: without it, `outer` would pad that edge again.
    set(&temp, "gaps.top", "0");
    set(&temp, "gaps.outer", "12");
    assert_eq!(
        written_gaps(&temp),
        serde_json::json!({ "inner": 6, "outer": 12, "top": 0 })
    );
    let edges = ["gaps.top", "gaps.bottom", "gaps.left", "gaps.right"];
    assert_eq!(edges.map(|key| get(&temp, key)), ["0", "12", "12", "12"]);

    let show = temp.run(&["config", "show", "--json"]).stdout.unwrap();
    let show: Value = serde_json::from_str(&show).unwrap();
    assert_eq!(
        show["settings"]["gaps"],
        serde_json::json!({ "inner": 6, "top": 0, "bottom": 12, "left": 12, "right": 12 })
    );
    let shown = temp.run(&["config", "show"]).stdout.unwrap();
    assert_eq!(
        Config::parse(&shown).unwrap().spacing(),
        Config::parse(&temp.read()).unwrap().spacing()
    );

    // An unrelated setting keeps the shorthand, and an edge can match `outer` on its own.
    set(&temp, "hud", "false");
    set(&temp, "gaps.left", "12");
    set(&temp, "gaps.outer", "20");
    assert_eq!(
        written_gaps(&temp),
        serde_json::json!({ "inner": 6, "outer": 20, "top": 0, "left": 12 })
    );

    // Resetting an explicit edge drops its key so it follows `outer` again.
    let reset_top = temp.run(&["config", "reset", "gaps.top"]);
    assert_eq!(reset_top.exit_code, 0, "{:?}", reset_top.stderr);
    assert_eq!(reset_top.stdout.as_deref(), Some("reset gaps.top = 20"));
    assert_eq!(
        written_gaps(&temp),
        serde_json::json!({ "inner": 6, "outer": 20, "left": 12 })
    );
    assert_eq!(edges.map(|key| get(&temp, key)), ["20", "20", "12", "20"]);

    // Back at its default, `outer` leaves the file and every explicit edge stays.
    set(&temp, "gaps.outer", "0");
    assert_eq!(
        written_gaps(&temp),
        serde_json::json!({ "inner": 6, "left": 12 })
    );
    assert_eq!(edges.map(|key| get(&temp, key)), ["0", "0", "12", "0"]);

    // Resetting `gaps.outer` also removes it without touching explicit edges.
    set(&temp, "gaps.outer", "8");
    let reset_outer = temp.run(&["config", "reset", "gaps.outer"]);
    assert_eq!(reset_outer.exit_code, 0, "{:?}", reset_outer.stderr);
    assert_eq!(reset_outer.stdout.as_deref(), Some("reset gaps.outer = 0"));
    assert_eq!(
        written_gaps(&temp),
        serde_json::json!({ "inner": 6, "left": 12 })
    );
}

#[test]
fn config_outer_reports_the_fallback_and_never_one_inferred_from_the_edges() {
    let temp = TempConfig::new("config-outer-get");
    temp.write("version = 1\n[settings.gaps]\ntop = 9\nbottom = 9\nleft = 9\nright = 9\n");
    assert_eq!(get(&temp, "gaps.outer"), "0");
    let json = temp.run(&["config", "get", "settings.gaps.outer", "--json"]);
    let json: Value = serde_json::from_str(&json.stdout.unwrap()).unwrap();
    assert_eq!(json, serde_json::json!({ "key": "gaps.outer", "value": 0 }));
    let json = temp.run(&["config", "set", "gaps.outer", "4", "--json"]);
    let json: Value = serde_json::from_str(&json.stdout.unwrap()).unwrap();
    assert_eq!(
        (&json["key"], &json["value"]),
        (&"gaps.outer".into(), &4.into())
    );
    assert_eq!(get(&temp, "gaps.outer"), "4");
    assert_eq!(get(&temp, "gaps.right"), "9");
}

#[test]
fn config_refuses_an_invalid_outer_and_leaves_the_file_alone() {
    let temp = TempConfig::new("config-outer-invalid");
    temp.write("version = 1\n[settings.gaps]\nouter = 8\ntop = 0\n");
    let before = temp.read();
    for (value, message) in [
        (
            "129",
            "settings.gaps.outer must be between 0 and 128 px, got 129",
        ),
        (
            "wide",
            "invalid value for gaps.outer: expected a whole number of points",
        ),
        (
            "1.5",
            "invalid value for gaps.outer: expected a whole number of points",
        ),
    ] {
        let refused = temp.run(&["config", "set", "gaps.outer", value]);
        assert_eq!(refused.exit_code, 1);
        let error = refused.stderr.unwrap();
        assert!(error.contains(message), "{error}");
    }
    assert_eq!(temp.read(), before);

    let unknown = temp.run(&["config", "get", "gaps.outside"]).stderr.unwrap();
    assert!(
        unknown.contains("gaps.inner, gaps.outer, gaps.top"),
        "{unknown}"
    );
    let help = temp.run(&["config", "--help"]).stdout.unwrap();
    assert!(help.contains("gaps.outer"), "{help}");
}

#[test]
fn config_without_outer_still_drops_a_zero_edge() {
    let temp = TempConfig::new("config-no-outer");
    temp.write("version = 1\n[settings.gaps]\ninner = 6\ntop = 32\nleft = 0\n");
    set(&temp, "gaps.top", "0");
    assert_eq!(
        written_gaps(&temp),
        serde_json::json!({ "inner": 6, "left": 0 })
    );
    set(&temp, "gaps.left", "0");
    assert_eq!(written_gaps(&temp), serde_json::json!({ "inner": 6 }));
}

#[test]
fn config_rejects_unknown_keys_invalid_values_and_corrupt_existing_file() {
    let temp = TempConfig::new("config-invalid");

    let bad_key = temp.run(&["config", "get", "gap"]);
    assert_eq!(bad_key.exit_code, 1);
    assert!(
        bad_key
            .stderr
            .unwrap()
            .contains("valid keys: hud, hud_style")
    );

    let bad_bool = temp.run(&["config", "set", "hud", "maybe"]);
    assert_eq!(bad_bool.exit_code, 1);
    assert!(
        bad_bool
            .stderr
            .unwrap()
            .contains("expected 'true' or 'false'")
    );
    assert!(!temp.path.exists());

    let bad_style = temp.run(&["config", "set", "hud_style", "grid"]);
    assert_eq!(bad_style.exit_code, 1);
    assert!(
        bad_style
            .stderr
            .unwrap()
            .contains("expected 'icons' or 'layouts'")
    );
    assert!(!temp.path.exists());

    // Refuse to overwrite an existing invalid config file
    temp.write(
        "version = 2
",
    );
    let refused = temp.run(&["config", "set", "hud", "false"]);
    assert_eq!(refused.exit_code, 1);
    assert!(
        refused
            .stderr
            .unwrap()
            .contains("unsupported config version")
    );
    assert_eq!(
        temp.read(),
        "version = 2
"
    );
}

#[test]
fn rules_list_add_position_and_remove_by_index_and_bundle_id() {
    let temp = TempConfig::new("rules");

    assert_eq!(
        temp.run(&["rules", "list"]).stdout.as_deref(),
        Some("No rules configured.")
    );

    // Add route rule (creates version = 1 file)
    let r1 = temp.run(&[
        "rules",
        "add",
        "--app",
        "com.tinyspeck.slackmacgap",
        "--workspace",
        "2",
        "--json",
    ]);
    assert_eq!(r1.exit_code, 0);
    let r1_json: Value = serde_json::from_str(&r1.stdout.unwrap()).unwrap();
    assert_eq!(r1_json["rule"]["index"], 1);
    assert_eq!(r1_json["rule"]["bundle_id"], "com.tinyspeck.slackmacgap");
    assert_eq!(r1_json["rule"]["action"], "route");
    assert_eq!(r1_json["rule"]["workspace"], 2);

    // Add named workspace route rule (lowercase normalizes to uppercase)
    let r2 = temp.run(&["rules", "add", "--app=ai.opencode.desktop", "--workspace=d"]);
    assert_eq!(r2.exit_code, 0);
    assert_eq!(
        r2.stdout.as_deref(),
        Some("added rule 2: bundle_id=ai.opencode.desktop -> workspace D")
    );

    // Insert title-specific float rule at position 1 (first-match wins!)
    let r3 = temp.run(&[
        "rules",
        "add",
        "--app",
        "com.tinyspeck.slackmacgap",
        "--title",
        "Preferences",
        "--float",
        "--position",
        "1",
    ]);
    assert_eq!(r3.exit_code, 0);

    // Add ignore rule
    let r4 = temp.run(&["rules", "add", "--app", "com.granola.app", "--ignore"]);
    assert_eq!(r4.exit_code, 0);

    // Validate with real Config::parse and check first-match semantics
    let config = Config::parse(&temp.read()).unwrap();
    assert_eq!(
        config.disposition(
            "com.tinyspeck.slackmacgap",
            &sample_window(Some("Preferences"))
        ),
        WindowDisposition::Floating
    );
    assert_eq!(
        config.disposition("com.tinyspeck.slackmacgap", &sample_window(Some("General"))),
        WindowDisposition::Route(WorkspaceId::TWO)
    );
    assert_eq!(
        config.disposition("ai.opencode.desktop", &sample_window(None)),
        WindowDisposition::Route(WorkspaceId::named('D').unwrap())
    );
    assert_eq!(
        config.disposition("com.granola.app", &sample_window(None)),
        WindowDisposition::Ignored
    );

    // List as JSON
    let listed: Value = serde_json::from_str(
        temp.run(&["rules", "list", "--json"])
            .stdout
            .as_deref()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(listed["rules"].as_array().unwrap().len(), 4);
    assert_eq!(listed["rules"][0]["title"], "Preferences");
    assert_eq!(listed["rules"][0]["action"], "float");
    assert_eq!(listed["rules"][2]["workspace"], "D");

    // Remove by 1-based index (remove com.granola.app at index 4)
    let rem_idx = temp.run(&["rules", "remove", "4", "--json"]);
    assert_eq!(rem_idx.exit_code, 0);
    let rem_idx_json: Value = serde_json::from_str(&rem_idx.stdout.unwrap()).unwrap();
    assert_eq!(rem_idx_json["removed"].as_array().unwrap().len(), 1);
    assert_eq!(rem_idx_json["removed"][0]["bundle_id"], "com.granola.app");
    assert_eq!(rem_idx_json["remaining"], 3);

    // Remove by --app bundle-id (removes both slack rules)
    let rem_app = temp.run(&[
        "rules",
        "remove",
        "--app",
        "com.tinyspeck.slackmacgap",
        "--json",
    ]);
    assert_eq!(rem_app.exit_code, 0);
    let rem_app_json: Value = serde_json::from_str(&rem_app.stdout.unwrap()).unwrap();
    assert_eq!(rem_app_json["removed"].as_array().unwrap().len(), 2);
    assert_eq!(rem_app_json["remaining"], 1);

    // Remove last remaining rule by positional bundle-id
    let rem_pos_app = temp.run(&["rules", "remove", "ai.opencode.desktop"]);
    assert_eq!(rem_pos_app.exit_code, 0);
    assert_eq!(
        temp.run(&["rules", "list"]).stdout.as_deref(),
        Some("No rules configured.")
    );
    assert!(Config::parse(&temp.read()).is_ok());
}

#[test]
fn rules_add_tile_writes_a_tile_rule() {
    let temp = TempConfig::new("rules-tile");

    let added = temp.run(&[
        "rules",
        "add",
        "--app",
        "com.apple.systempreferences",
        "--tile",
    ]);
    assert_eq!(added.exit_code, 0);
    assert_eq!(
        added.stdout.as_deref(),
        Some("added rule 1: bundle_id=com.apple.systempreferences -> tile")
    );
    let config = Config::parse(&temp.read()).unwrap();
    assert_eq!(
        config.disposition("com.apple.systempreferences", &sample_window(None)),
        WindowDisposition::Tiled
    );

    for conflicting in [
        &["--tile", "--float"][..],
        &["--ignore", "--tile"],
        &["--tile=yes"],
    ] {
        let mut args = vec!["rules", "add", "--app", "com.example.App"];
        args.extend_from_slice(conflicting);
        assert_eq!(temp.run(&args).exit_code, 1, "{conflicting:?}");
    }
    assert_eq!(
        temp.run(&[
            "rules",
            "add",
            "--app",
            "com.example.App",
            "--tile",
            "--workspace",
            "2"
        ])
        .exit_code,
        1
    );
}

#[test]
fn rules_rejects_invalid_inputs() {
    let temp = TempConfig::new("rules-invalid");

    // Missing matcher
    assert_eq!(temp.run(&["rules", "add", "--float"]).exit_code, 1);
    // Missing action
    assert_eq!(
        temp.run(&["rules", "add", "--app", "com.example.App"])
            .exit_code,
        1
    );
    // Conflicting actions
    assert_eq!(
        temp.run(&[
            "rules",
            "add",
            "--app",
            "com.example.App",
            "--float",
            "--ignore"
        ])
        .exit_code,
        1
    );
    // Invalid workspace
    assert_eq!(
        temp.run(&[
            "rules",
            "add",
            "--app",
            "com.example.App",
            "--workspace",
            "11"
        ])
        .exit_code,
        1
    );
    // Out of range position (0 or 2 when empty)
    assert_eq!(
        temp.run(&[
            "rules",
            "add",
            "--app",
            "com.example.App",
            "--float",
            "--position",
            "0"
        ])
        .exit_code,
        1
    );
    assert_eq!(
        temp.run(&[
            "rules",
            "add",
            "--app",
            "com.example.App",
            "--float",
            "--position",
            "2"
        ])
        .exit_code,
        1
    );
    // Remove out-of-range / nonexistent
    assert_eq!(temp.run(&["rules", "remove", "0"]).exit_code, 1);
    assert_eq!(temp.run(&["rules", "remove", "1"]).exit_code, 1);
    assert_eq!(
        temp.run(&["rules", "remove", "com.missing.App"]).exit_code,
        1
    );
    assert!(!temp.path.exists());
}

#[test]
fn workspaces_list_assign_and_clear_round_trip() {
    let temp = TempConfig::new("workspaces");

    // Default list has 1..=10 (1-5 main, 6-7 secondary, 8-10 tertiary)
    let initial: Value = serde_json::from_str(
        temp.run(&["workspaces", "list", "--json"])
            .stdout
            .as_deref()
            .unwrap(),
    )
    .unwrap();
    let entries = initial["workspaces"].as_array().unwrap();
    assert_eq!(entries.len(), 10);
    assert_eq!(entries[0]["workspace"], 1);
    assert_eq!(entries[0]["monitor"], "main");
    assert_eq!(entries[0]["configured"], false);
    assert_eq!(entries[5]["workspace"], 6);
    assert_eq!(entries[5]["monitor"], "secondary");
    assert_eq!(entries[5]["configured"], false);
    assert_eq!(entries[7]["workspace"], 8);
    assert_eq!(entries[7]["monitor"], "tertiary");

    // Assign workspace 9 to main and workspace D to a named display
    let assign_9 = temp.run(&["workspaces", "assign", "9", "main", "--json"]);
    assert_eq!(assign_9.exit_code, 0);
    let assign_d = temp.run(&["workspaces", "assign", "d", "Studio Display"]);
    assert_eq!(assign_d.exit_code, 0);
    assert_eq!(
        assign_d.stdout.as_deref(),
        Some("assigned workspace D -> Studio Display")
    );

    // Updating existing workspace replaces in place without creating duplicates
    let update_9 = temp.run(&["workspaces", "assign", "9", "secondary"]);
    assert_eq!(update_9.exit_code, 0);

    let parsed = Config::parse(&temp.read()).unwrap();
    let mut workspaces = spaceman::Workspaces::default();
    parsed.configure_workspaces(&mut workspaces);
    assert_eq!(
        workspaces.assignment(WorkspaceId::named('D').unwrap()),
        &spaceman::DisplaySelector::Preferred("Studio Display".into())
    );

    // List now includes workspace D
    let after_assign: Value = serde_json::from_str(
        temp.run(&["workspaces", "list", "--json"])
            .stdout
            .as_deref()
            .unwrap(),
    )
    .unwrap();
    let entries = after_assign["workspaces"].as_array().unwrap();
    assert_eq!(entries.len(), 11);
    assert_eq!(entries[8]["workspace"], 9);
    assert_eq!(entries[8]["monitor"], "secondary");
    assert_eq!(entries[8]["configured"], true);
    assert_eq!(entries[10]["workspace"], "D");
    assert_eq!(entries[10]["monitor"], "Studio Display");
    assert_eq!(entries[10]["configured"], true);

    // Clear workspace 9
    let clear_9 = temp.run(&["workspaces", "clear", "9", "--json"]);
    assert_eq!(clear_9.exit_code, 0);
    let clear_9_json: Value = serde_json::from_str(&clear_9.stdout.unwrap()).unwrap();
    assert_eq!(clear_9_json["cleared"], true);
    assert_eq!(clear_9_json["monitor"], "tertiary");

    // Clearing again is idempotent
    let clear_9_again = temp.run(&["workspaces", "clear", "9", "--json"]);
    assert_eq!(clear_9_again.exit_code, 0);
    let clear_9_again_json: Value = serde_json::from_str(&clear_9_again.stdout.unwrap()).unwrap();
    assert_eq!(clear_9_again_json["cleared"], false);

    assert!(Config::parse(&temp.read()).is_ok());
}

#[test]
fn workspaces_rejects_invalid_inputs() {
    let temp = TempConfig::new("workspaces-invalid");
    assert_eq!(
        temp.run(&["workspaces", "assign", "11", "main"]).exit_code,
        1
    );
    assert_eq!(temp.run(&["workspaces", "assign", "1", "   "]).exit_code, 1);
    assert_eq!(temp.run(&["workspaces", "clear", "AA"]).exit_code, 1);
    assert!(!temp.path.exists());
}

#[test]
fn edits_preserve_other_sections_and_named_rule_workspaces_appear_in_list() {
    let temp = TempConfig::new("preserve");
    // The example leaves `[shortcuts]` commented out so copying it changes nothing; add one
    // here so every edit below has to carry a real table through untouched.
    let mut source = include_str!("../../config.example.toml").to_owned();
    source.push_str(
        "\n[shortcuts]\ndefaults = true\n\n[shortcuts.bindings]\n\"alt+shift+b\" = \"balance\"\n",
    );
    temp.write(&source);

    // Verify example config parses cleanly through all read commands
    assert_eq!(temp.run(&["config", "check", "--json"]).exit_code, 0);
    let initial_rules: Value = serde_json::from_str(
        temp.run(&["rules", "list", "--json"])
            .stdout
            .as_deref()
            .unwrap(),
    )
    .unwrap();
    let initial_rule_count = initial_rules["rules"].as_array().unwrap().len();
    assert!(initial_rule_count >= 4);

    // Edit settings, add a named-workspace rule (W), and assign workspace 3
    assert_eq!(temp.run(&["config", "set", "hud", "false"]).exit_code, 0);
    assert_eq!(
        temp.run(&[
            "rules",
            "add",
            "--app",
            "com.example.Writer",
            "--workspace",
            "W",
        ])
        .exit_code,
        0
    );
    assert_eq!(
        temp.run(&["workspaces", "assign", "3", "secondary"])
            .exit_code,
        0
    );

    // Re-parse with strict Config::parse and verify all sections survived intact
    let parsed = Config::parse(&temp.read()).unwrap();
    assert!(!parsed.hud());
    assert_eq!(parsed.hud_style(), HudStyle::Icons);
    assert_eq!(
        parsed.disposition("com.example.Writer", &sample_window(None)),
        WindowDisposition::Route(WorkspaceId::named('W').unwrap())
    );
    assert_eq!(
        parsed.disposition("com.granola.app", &sample_window(None)),
        WindowDisposition::Ignored
    );
    // The appended [shortcuts] table survives every edit above.
    assert!(parsed.shortcuts().defaults);
    assert_eq!(parsed.shortcuts().bindings.len(), 1);
    assert_eq!(parsed.shortcuts().bindings[0].source, "alt+shift+b");

    // Named workspace W (only referenced by a rule) appears in workspaces list with default main
    let workspaces: Value = serde_json::from_str(
        temp.run(&["workspaces", "list", "--json"])
            .stdout
            .as_deref()
            .unwrap(),
    )
    .unwrap();
    let w_entry = workspaces["workspaces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["workspace"] == "W")
        .expect("workspace W from rule should appear in workspaces list");
    assert_eq!(w_entry["monitor"], "main");
    assert_eq!(w_entry["configured"], false);
}

#[test]
fn rules_add_accepts_literal_help_flag_as_title() {
    let config = TempConfig::new("rules-add-help-title");

    let added = config.run(&[
        "rules",
        "add",
        "--app",
        "com.example.Editor",
        "--title",
        "--help",
        "--float",
        "--json",
    ]);
    assert_eq!(added.exit_code, 0, "stderr: {:?}", added.stderr);
    let parsed: Value =
        serde_json::from_str(added.stdout.as_deref().unwrap()).expect("valid json output");
    assert_eq!(parsed["rule"]["title"], "--help");
    assert_eq!(parsed["rule"]["action"], "float");

    let listed = config.run(&["rules", "list", "--json"]);
    assert_eq!(listed.exit_code, 0);
    let list_json: Value =
        serde_json::from_str(listed.stdout.as_deref().unwrap()).expect("valid json list");
    assert_eq!(list_json["rules"][0]["title"], "--help");

    let help_only = config.run(&["rules", "add", "--help"]);
    assert_eq!(help_only.exit_code, 0);
    assert!(
        help_only
            .stdout
            .as_deref()
            .is_some_and(|out| out.contains("Manage ordered, first-match window rules"))
    );
}

#[test]
fn config_commands_accept_and_preserve_shortcuts_base_modifier() {
    let temp = TempConfig::new("shortcuts-modifier");
    temp.write("version = 1\n\n[shortcuts]\nmodifier = \"ctrl+alt\"\n");

    let explicit_path = temp.path.to_str().unwrap();
    let explicit_check = temp.run(&["config", "check", explicit_path]);
    assert_eq!(
        explicit_check.exit_code, 0,
        "stderr: {:?}",
        explicit_check.stderr
    );
    assert_eq!(
        explicit_check.stdout.as_deref(),
        Some("configuration is valid")
    );

    let implicit_check = temp.run(&["config", "check"]);
    assert_eq!(
        implicit_check.exit_code, 0,
        "stderr: {:?}",
        implicit_check.stderr
    );
    assert_eq!(
        implicit_check.stdout.as_deref(),
        Some("configuration is valid")
    );

    let hud = temp.run(&["config", "get", "hud"]);
    assert_eq!(hud.exit_code, 0, "stderr: {:?}", hud.stderr);
    assert_eq!(hud.stdout.as_deref(), Some("true"));

    let set_gap = temp.run(&["config", "set", "gaps.inner", "8"]);
    assert_eq!(set_gap.exit_code, 0, "stderr: {:?}", set_gap.stderr);
    assert_eq!(set_gap.stdout.as_deref(), Some("set gaps.inner = 8"));

    let gap = temp.run(&["config", "get", "gaps.inner"]);
    assert_eq!(gap.exit_code, 0, "stderr: {:?}", gap.stderr);
    assert_eq!(gap.stdout.as_deref(), Some("8"));

    let recheck = temp.run(&["config", "check"]);
    assert_eq!(recheck.exit_code, 0, "stderr: {:?}", recheck.stderr);

    let parsed = Config::parse(&temp.read()).expect("saved config stays valid");
    assert_eq!(parsed.spacing().inner, 8);
    assert!(parsed.shortcuts().modifier.control);
    assert!(parsed.shortcuts().modifier.option);
    assert!(!parsed.shortcuts().modifier.command);
    assert!(!parsed.shortcuts().modifier.shift);
}

#[cfg(target_os = "macos")]
#[test]
fn ctl_help_describes_restart_as_exit_without_relaunch() {
    let outcome = run_cli(&["ctl", "--help"], None);
    assert_eq!(outcome.exit_code, 0);
    let stdout = outcome.stdout.expect("ctl --help stdout");
    assert!(
        !stdout.contains("restart or quit"),
        "ctl --help should not promise relaunch:\n{stdout}"
    );
    assert!(
        stdout.contains("does not relaunch"),
        "ctl --help should state restart exits without relaunching:\n{stdout}"
    );
}

#[test]
fn workspaces_clear_keeps_solely_declared_named_workspace_on_main() {
    let temp = TempConfig::new("workspaces-clear-named");
    temp.write("version = 1\n[[workspaces]]\nworkspace = \"D\"\nmonitor = \"secondary\"\n");

    let clear_d = temp.run(&["workspaces", "clear", "D"]);
    assert_eq!(clear_d.exit_code, 0, "stderr: {:?}", clear_d.stderr);
    assert_eq!(
        clear_d.stdout.as_deref(),
        Some("cleared workspace D (default: main)")
    );

    let after_clear: Value = serde_json::from_str(
        temp.run(&["workspaces", "list", "--json"])
            .stdout
            .as_deref()
            .unwrap(),
    )
    .unwrap();
    let d_entry = after_clear["workspaces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["workspace"] == "D")
        .expect("solely declared named workspace D must remain listed");
    assert_eq!(d_entry["monitor"], "main");
    assert_eq!(d_entry["configured"], true);

    let parsed = Config::parse(&temp.read()).unwrap();
    let d = WorkspaceId::named('D').unwrap();
    assert_eq!(parsed.named_workspaces().collect::<Vec<_>>(), vec![d]);

    // Clearing again is idempotent and still preserves D on main.
    let clear_again = temp.run(&["workspaces", "clear", "D", "--json"]);
    assert_eq!(clear_again.exit_code, 0);
    let clear_again_json: Value = serde_json::from_str(&clear_again.stdout.unwrap()).unwrap();
    assert_eq!(clear_again_json["cleared"], false);
    assert_eq!(clear_again_json["monitor"], "main");
    assert_eq!(
        Config::parse(&temp.read())
            .unwrap()
            .named_workspaces()
            .collect::<Vec<_>>(),
        vec![d]
    );

    // Clearing an unconfigured named workspace does not declare it.
    let clear_z = temp.run(&["workspaces", "clear", "Z", "--json"]);
    assert_eq!(clear_z.exit_code, 0);
    let clear_z_json: Value = serde_json::from_str(&clear_z.stdout.unwrap()).unwrap();
    assert_eq!(clear_z_json["cleared"], false);
    assert_eq!(
        Config::parse(&temp.read())
            .unwrap()
            .named_workspaces()
            .collect::<Vec<_>>(),
        vec![d]
    );

    // When a route rule also references D, clearing removes the redundant [[workspaces]] entry.
    assert_eq!(
        temp.run(&[
            "rules",
            "add",
            "--app",
            "com.example.App",
            "--workspace",
            "D"
        ])
        .exit_code,
        0
    );
    let clear_routed = temp.run(&["workspaces", "clear", "D", "--json"]);
    assert_eq!(clear_routed.exit_code, 0);
    let clear_routed_json: Value = serde_json::from_str(&clear_routed.stdout.unwrap()).unwrap();
    assert_eq!(clear_routed_json["cleared"], true);
    assert!(!temp.read().contains("[[workspaces]]"));
    assert_eq!(
        Config::parse(&temp.read())
            .unwrap()
            .named_workspaces()
            .collect::<Vec<_>>(),
        vec![d]
    );
}
