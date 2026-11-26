use std::{
    fs,
    os::unix::fs::symlink,
    sync::{Arc, Barrier},
    thread,
};

use fractile::{
    WorkspaceId,
    config::{Config, HudStyle, WindowDisposition},
    shortcuts::{Command, Key, Modifiers},
};
use serde_json::Value;

use super::{TempConfig, run_cli, sample_window};

#[test]
fn concurrent_config_set_commands_preserve_both_edits() {
    let temp = TempConfig::new("concurrent-set");
    let attempts = 40;

    for attempt in 0..attempts {
        temp.write("version = 1\n");
        let barrier = Arc::new(Barrier::new(2));
        let path_a = temp.path.clone();
        let path_b = temp.path.clone();
        let b1 = Arc::clone(&barrier);
        let b2 = Arc::clone(&barrier);

        let left = thread::spawn(move || {
            b1.wait();
            run_cli(&["config", "set", "hud", "false"], Some(&path_a))
        });
        let right = thread::spawn(move || {
            b2.wait();
            run_cli(&["config", "set", "hud_style", "layouts"], Some(&path_b))
        });

        let out_a = left.join().unwrap();
        let out_b = right.join().unwrap();
        assert_eq!(out_a.exit_code, 0, "attempt {attempt}: {:?}", out_a.stderr);
        assert_eq!(out_b.exit_code, 0, "attempt {attempt}: {:?}", out_b.stderr);

        let saved = temp.read();
        let parsed = Config::parse(&saved)
            .unwrap_or_else(|err| panic!("attempt {attempt}: invalid config ({err}):\n{saved}"));
        assert!(
            !parsed.hud(),
            "attempt {attempt}: lost hud = false:\n{saved}"
        );
        assert_eq!(
            parsed.hud_style(),
            HudStyle::Layouts,
            "attempt {attempt}: lost hud_style = layouts:\n{saved}"
        );
    }

    // Sequential control.
    temp.write("version = 1\n");
    assert_eq!(temp.run(&["config", "set", "hud", "false"]).exit_code, 0);
    assert_eq!(
        temp.run(&["config", "set", "hud_style", "layouts"])
            .exit_code,
        0
    );
    let parsed = Config::parse(&temp.read()).unwrap();
    assert!(!parsed.hud());
    assert_eq!(parsed.hud_style(), HudStyle::Layouts);
}

#[test]
fn concurrent_mixed_cli_config_mutations_preserve_every_edit() {
    let temp = TempConfig::new("concurrent-mixed");

    for attempt in 0..20 {
        temp.write("version = 1\n");
        let barrier = Arc::new(Barrier::new(3));
        let p1 = temp.path.clone();
        let p2 = temp.path.clone();
        let p3 = temp.path.clone();
        let b1 = Arc::clone(&barrier);
        let b2 = Arc::clone(&barrier);
        let b3 = Arc::clone(&barrier);

        let t1 = thread::spawn(move || {
            b1.wait();
            run_cli(&["config", "set", "hud", "false"], Some(&p1))
        });
        let t2 = thread::spawn(move || {
            b2.wait();
            run_cli(
                &[
                    "rules",
                    "add",
                    "--app",
                    "com.example.App",
                    "--workspace",
                    "2",
                ],
                Some(&p2),
            )
        });
        let t3 = thread::spawn(move || {
            b3.wait();
            run_cli(&["workspaces", "assign", "3", "secondary"], Some(&p3))
        });

        assert_eq!(t1.join().unwrap().exit_code, 0);
        assert_eq!(t2.join().unwrap().exit_code, 0);
        assert_eq!(t3.join().unwrap().exit_code, 0);

        let saved = temp.read();
        let parsed = Config::parse(&saved)
            .unwrap_or_else(|err| panic!("attempt {attempt}: invalid config ({err}):\n{saved}"));
        assert!(!parsed.hud(), "attempt {attempt}:\n{saved}");
        assert_eq!(
            parsed.disposition("com.example.App", &sample_window(None)),
            WindowDisposition::Route(WorkspaceId::TWO),
            "attempt {attempt}:\n{saved}"
        );
        let mut workspaces = fractile::Workspaces::default();
        parsed.configure_workspaces(&mut workspaces);
        assert_eq!(
            workspaces.assignment(WorkspaceId::new(3).unwrap()),
            &fractile::DisplaySelector::Secondary,
            "attempt {attempt}:\n{saved}"
        );
    }
}

#[test]
fn config_edits_preserve_symlink_and_update_target() {
    let temp = TempConfig::new("symlink-preserve");
    fs::create_dir_all(&temp.dir).unwrap();
    let target = temp.dir.join("dotfiles.toml");
    fs::write(&target, "version = 1\n").unwrap();
    symlink("dotfiles.toml", &temp.path).unwrap();

    let check = run_cli(&["config", "check"], Some(&temp.path));
    assert_eq!(check.exit_code, 0, "stderr: {:?}", check.stderr);
    assert_eq!(check.stdout.as_deref(), Some("configuration is valid"));

    let set = run_cli(&["config", "set", "hud", "false"], Some(&temp.path));
    assert_eq!(set.exit_code, 0, "stderr: {:?}", set.stderr);
    assert_eq!(set.stdout.as_deref(), Some("set hud = false"));

    assert!(
        fs::symlink_metadata(&temp.path)
            .unwrap()
            .file_type()
            .is_symlink(),
        "config.toml must remain a symlink"
    );
    assert_eq!(
        fs::read_link(&temp.path).unwrap(),
        std::path::PathBuf::from("dotfiles.toml")
    );
    assert_eq!(
        run_cli(&["config", "get", "hud"], Some(&temp.path))
            .stdout
            .as_deref(),
        Some("false")
    );
    assert_eq!(
        run_cli(&["config", "get", "hud"], Some(&target))
            .stdout
            .as_deref(),
        Some("false")
    );

    // Rules and workspace edits also preserve the symlink and leave no sidecar lock files.
    assert_eq!(
        run_cli(
            &["rules", "add", "--app", "com.example.App", "--float"],
            Some(&temp.path)
        )
        .exit_code,
        0
    );
    assert_eq!(
        run_cli(
            &["workspaces", "assign", "2", "secondary"],
            Some(&temp.path)
        )
        .exit_code,
        0
    );
    assert!(
        fs::symlink_metadata(&temp.path)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    let target_parsed = Config::parse(&fs::read_to_string(&target).unwrap()).unwrap();
    assert!(!target_parsed.hud());
    assert_eq!(
        target_parsed.disposition("com.example.App", &sample_window(None)),
        WindowDisposition::Floating
    );
    let entries: Vec<_> = fs::read_dir(&temp.dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(
        entries.len(),
        2,
        "only symlink and target should exist, got {entries:?}"
    );
}

#[test]
fn config_set_follows_chained_and_dangling_symlinks() {
    let temp = TempConfig::new("symlink-chain");
    let nested = temp.dir.join("repo");
    fs::create_dir_all(&nested).unwrap();

    // Chained symlink: config.toml -> mid.toml -> repo/real.toml
    let real = nested.join("real.toml");
    fs::write(&real, "version = 1\n").unwrap();
    let mid = temp.dir.join("mid.toml");
    symlink("repo/real.toml", &mid).unwrap();
    symlink("mid.toml", &temp.path).unwrap();

    let set = run_cli(&["config", "set", "hud", "false"], Some(&temp.path));
    assert_eq!(set.exit_code, 0, "stderr: {:?}", set.stderr);
    assert!(
        fs::symlink_metadata(&temp.path)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(fs::symlink_metadata(&mid).unwrap().file_type().is_symlink());
    assert!(
        !Config::parse(&fs::read_to_string(&real).unwrap())
            .unwrap()
            .hud()
    );

    // Dangling symlink: dangling.toml -> repo/created.toml (does not exist yet)
    let created = nested.join("created.toml");
    let dangling = temp.dir.join("dangling.toml");
    symlink("repo/created.toml", &dangling).unwrap();

    let set_dangling = run_cli(&["config", "set", "hud_style", "layouts"], Some(&dangling));
    assert_eq!(
        set_dangling.exit_code, 0,
        "stderr: {:?}",
        set_dangling.stderr
    );
    assert!(
        fs::symlink_metadata(&dangling)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        Config::parse(&fs::read_to_string(&created).unwrap())
            .unwrap()
            .hud_style(),
        HudStyle::Layouts
    );
}

#[test]
fn config_show_includes_shortcuts_and_labs_in_toml_and_json() {
    let temp = TempConfig::new("show-shortcuts-labs");
    temp.write(concat!(
        "version = 1\n",
        "[settings]\n",
        "hud = false\n",
        "[shortcuts]\n",
        "defaults = false\n",
        "modifier = \"ctrl+alt\"\n",
        "[shortcuts.bindings]\n",
        "\"ctrl+alt+f\" = \"fullscreen\"\n",
        "[labs]\n",
        "scratchpad = true\n",
    ));

    let check = temp.run(&["config", "check"]);
    assert_eq!(check.exit_code, 0, "stderr: {:?}", check.stderr);

    let show_toml = temp.run(&["config", "show"]);
    assert_eq!(show_toml.exit_code, 0, "stderr: {:?}", show_toml.stderr);
    let rendered = show_toml.stdout.unwrap();

    // Shown TOML validates with both Config::parse and `fractile config check <file>\`.
    let shown_file = temp.dir.join("shown.toml");
    fs::write(&shown_file, &rendered).unwrap();
    let recheck = temp.run(&["config", "check", shown_file.to_str().unwrap()]);
    assert_eq!(
        recheck.exit_code, 0,
        "shown TOML failed check ({:?}):\n{rendered}",
        recheck.stderr
    );

    let parsed = Config::parse(&rendered).unwrap();
    assert!(!parsed.hud());
    assert!(parsed.labs().scratchpad);
    assert!(!parsed.shortcuts().defaults);
    assert_eq!(
        parsed.shortcuts().modifier,
        Modifiers {
            control: true,
            option: true,
            command: false,
            shift: false,
        }
    );
    assert_eq!(parsed.shortcuts().bindings.len(), 1);
    assert_eq!(parsed.shortcuts().bindings[0].source, "ctrl+alt+f");
    assert_eq!(
        parsed.shortcuts().bindings[0].command,
        Some(Command::Fullscreen)
    );
    assert_eq!(
        parsed.shortcuts().bindings[0].chord.key,
        Key::Character('f')
    );

    let show_json_out = temp.run(&["config", "show", "--json"]);
    assert_eq!(
        show_json_out.exit_code, 0,
        "stderr: {:?}",
        show_json_out.stderr
    );
    let shown_json: Value = serde_json::from_str(&show_json_out.stdout.unwrap()).unwrap();
    assert_eq!(shown_json["settings"]["hud"], false);
    assert_eq!(shown_json["labs"]["scratchpad"], true);
    assert_eq!(shown_json["shortcuts"]["defaults"], false);
    assert_eq!(shown_json["shortcuts"]["modifier"], "ctrl+alt");
    assert_eq!(
        shown_json["shortcuts"]["bindings"]["ctrl+alt+f"],
        "fullscreen"
    );

    // Control: config without [shortcuts] or [labs] omits both optional tables and stays valid.
    let plain = TempConfig::new("show-plain");
    plain.write("version = 1\n[settings]\nhud = false\n");
    let plain_toml = plain.run(&["config", "show"]).stdout.unwrap();
    assert!(!plain_toml.contains("[labs]"), "{plain_toml}");
    assert!(!plain_toml.contains("[shortcuts"), "{plain_toml}");
    let plain_parsed = Config::parse(&plain_toml).unwrap();
    assert!(!plain_parsed.hud());
    assert!(!plain_parsed.labs().scratchpad);
    assert!(plain_parsed.shortcuts().defaults);

    let plain_json: Value =
        serde_json::from_str(&plain.run(&["config", "show", "--json"]).stdout.unwrap()).unwrap();
    assert!(plain_json.get("labs").is_none());
    assert!(plain_json.get("shortcuts").is_none());
}

#[test]
fn write_stdout_line_handles_broken_pipe_cleanly_and_reports_other_write_errors() {
    use std::io::{self, Write};
    use std::process::ExitCode;

    struct FailingWriter(io::ErrorKind);
    impl Write for FailingWriter {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::new(self.0, "simulated write failure"))
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    assert_eq!(
        super::super::write_stdout_line_to(&mut FailingWriter(io::ErrorKind::BrokenPipe), "hello"),
        ExitCode::SUCCESS
    );
    assert_eq!(
        super::super::write_stdout_line_to(&mut FailingWriter(io::ErrorKind::Other), "hello"),
        ExitCode::FAILURE
    );
}

#[test]
fn config_commands_keep_and_show_the_workspace_order() {
    let temp = TempConfig::new("workspace-order");
    temp.write("version = 1\n[settings]\nworkspace_order = [1, \"Q\", \"W\"]\n");
    let order = [
        WorkspaceId::ONE,
        WorkspaceId::named('Q').unwrap(),
        WorkspaceId::named('W').unwrap(),
    ];

    let set = temp.run(&["config", "set", "hud", "false"]);
    assert_eq!(set.exit_code, 0, "{:?}", set.stderr);
    assert_eq!(
        Config::parse(&temp.read()).unwrap().workspace_order(),
        order
    );

    let show = temp.run(&["config", "show", "--json"]);
    assert_eq!(show.exit_code, 0, "{:?}", show.stderr);
    let show: Value = serde_json::from_str(show.stdout.as_deref().unwrap()).unwrap();
    assert_eq!(
        show["settings"]["workspace_order"],
        serde_json::json!([1, "Q", "W"])
    );
}

#[test]
fn config_gets_sets_and_resets_layout() {
    let temp = TempConfig::new("layout");
    temp.write("# Mine.\nversion = 1\n[settings]\nhud_style = \"layouts\"\n");
    assert_eq!(
        temp.run(&["config", "get", "layout"]).stdout.as_deref(),
        Some("dwindle")
    );
    assert_eq!(
        temp.run(&["config", "get", "settings.layout"])
            .stdout
            .as_deref(),
        Some("dwindle")
    );

    let set_columns = temp.run(&["config", "set", "settings.layout", "columns"]);
    assert_eq!(set_columns.exit_code, 0, "{:?}", set_columns.stderr);
    assert_eq!(set_columns.stdout.as_deref(), Some("set layout = columns"));
    assert_eq!(
        Config::parse(&temp.read()).unwrap().layout(),
        fractile::config::LayoutMode::Columns
    );
    assert!(temp.read().contains("hud_style = \"layouts\""));

    assert_eq!(
        temp.run(&["config", "get", "layout"]).stdout.as_deref(),
        Some("columns")
    );
    let show: Value = serde_json::from_str(
        temp.run(&["config", "show", "--json"])
            .stdout
            .as_deref()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(show["settings"]["layout"], "columns");
    assert_eq!(show["settings"]["hud_style"], "layouts");

    let reset = temp.run(&["config", "reset", "layout"]);
    assert_eq!(reset.exit_code, 0, "{:?}", reset.stderr);
    assert_eq!(reset.stdout.as_deref(), Some("reset layout = dwindle"));
    let source = temp.read();
    assert!(!source.contains("layout ="), "{source}");
    assert!(source.contains("hud_style = \"layouts\""), "{source}");
    assert_eq!(
        Config::parse(&source).unwrap().layout(),
        fractile::config::LayoutMode::Dwindle
    );

    temp.write("version = 1\n[settings]\nlayout = \"columns\"\n");
    let reset_json: Value = serde_json::from_str(
        temp.run(&["config", "reset", "settings.layout", "--json"])
            .stdout
            .as_deref()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(reset_json["key"], "layout");
    assert_eq!(reset_json["value"], "dwindle");
    assert!(!temp.read().contains("[settings]"));

    let bad = temp.run(&["config", "set", "layout", "bsp"]);
    assert_ne!(bad.exit_code, 0);
    assert!(
        bad.stderr
            .unwrap()
            .contains("expected 'dwindle' or 'columns'")
    );
}

#[test]
fn rules_add_writes_pin_and_title_contains_rules_and_lists_them_back() {
    let temp = TempConfig::new("rules-pin-title-contains");

    let pinned = temp.run(&["rules", "add", "--app", "com.apple.Stickies", "--pin"]);
    assert_eq!(pinned.exit_code, 0);
    assert_eq!(
        pinned.stdout.as_deref(),
        Some("added rule 1: bundle_id=com.apple.Stickies -> pin")
    );
    let routed = temp.run(&[
        "rules",
        "add",
        "--app",
        "com.apple.Safari",
        "--title-contains",
        "Work",
        "--workspace",
        "2",
    ]);
    assert_eq!(routed.exit_code, 0);
    assert_eq!(
        routed.stdout.as_deref(),
        Some("added rule 2: bundle_id=com.apple.Safari title_contains=\"Work\" -> workspace 2")
    );

    let config = Config::parse(&temp.read()).unwrap();
    assert_eq!(
        config.disposition("com.apple.Stickies", &sample_window(None)),
        WindowDisposition::Pinned
    );
    assert_eq!(
        config.disposition("com.apple.Safari", &sample_window(Some("work — PR"))),
        WindowDisposition::Route(WorkspaceId::TWO)
    );
    let listed = temp.run(&["rules", "list", "--json"]);
    let json: Value = serde_json::from_str(listed.stdout.as_deref().unwrap()).unwrap();
    assert_eq!(json["rules"][1]["title_contains"], "Work");

    let both = temp.run(&["rules", "add", "--app", "x", "--pin", "--float"]);
    assert_ne!(both.exit_code, 0);
}

#[test]
fn config_gets_sets_shows_and_resets_trackpad_swipe() {
    let temp = TempConfig::new("config-trackpad-swipe");
    temp.write("version = 1\n");
    assert_eq!(
        temp.run(&["config", "get", "trackpad_swipe"])
            .stdout
            .as_deref(),
        Some("true")
    );

    let bad = temp.run(&["config", "set", "trackpad_swipe", "off"]);
    assert_eq!(bad.exit_code, 1);
    assert!(bad.stderr.unwrap().contains("expected 'true' or 'false'"));

    let set = temp.run(&["config", "set", "trackpad_swipe", "false"]);
    assert_eq!(set.exit_code, 0, "{:?}", set.stderr);
    // Read only with its Lab on.
    assert!(Config::parse(&temp.read()).unwrap().trackpad_swipe());
    let lab = temp.run(&["config", "set", "labs.swipe_switch", "true"]);
    assert_eq!(lab.exit_code, 0, "{:?}", lab.stderr);
    assert!(!Config::parse(&temp.read()).unwrap().trackpad_swipe());
    let shown = temp.run(&["config", "show", "--json"]).stdout.unwrap();
    let show: Value = serde_json::from_str(&shown).unwrap();
    assert_eq!(show["settings"]["trackpad_swipe"], false);

    let reset = temp.run(&["config", "reset", "trackpad_swipe"]);
    assert_eq!(reset.exit_code, 0, "{:?}", reset.stderr);
    assert!(!temp.read().contains("trackpad_swipe"));
}
