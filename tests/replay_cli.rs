use std::process::Command;

use serde_json::Value;

#[test]
fn replay_reports_a_slot_preserving_ghostty_replacement() {
    let output = Command::new(env!("CARGO_BIN_EXE_fractile"))
        .args(["replay", "fixtures/ghostty-tab-replacement.json"])
        .output()
        .expect("fractile should run");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).expect("replay should output JSON");
    let steps = value["steps"].as_array().expect("steps should be an array");

    assert_eq!(steps.len(), 2);
    assert_eq!(
        steps[0]["model"]["workspace"]["slots"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        steps[1]["model"]["workspace"]["slots"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(steps[1]["decisions"][0]["type"], "native_tab_replaced");
    assert_eq!(steps[1]["decisions"][0]["slot"], 1);
}

#[test]
fn read_only_cli_output_exits_cleanly_on_closed_pipe() {
    use std::{
        fmt::Write as _,
        fs,
        io::{BufRead, BufReader, Read},
        process::Stdio,
    };

    let dir = std::env::temp_dir().join(format!("fractile-broken-pipe-{}", std::process::id()));
    let config_path = dir.join("config.toml");
    let support_dir = dir.join("support");
    fs::create_dir_all(&support_dir).unwrap();

    // >81 KB rules output to exceed macOS pipe buffer and trigger EPIPE when reader closes early.
    let mut config = String::from("version = 1\n");
    for i in 0..850 {
        writeln!(
            config,
            "\n[[rules]]\nbundle_id = \"com.example.Editor\"\ntitle = \"Project {i:04} - Planning and implementation notes\"\naction = \"float\""
        )
        .unwrap();
    }
    fs::write(&config_path, &config).unwrap();

    // >81 KB doctor output via a journal with many recalled bundle IDs.
    let mut journal = String::new();
    let now = fractile::journal::unix_millis();
    for i in 0..2000 {
        writeln!(
            journal,
            "{{\"t\":{now},\"kind\":\"recall\",\"bundle\":\"com.example.App{i:04}LongBundleIdentifier\",\"workspace\":1,\"placed\":\"tile\",\"reason\":\"remembered\"}}"
        )
        .unwrap();
    }
    fs::write(support_dir.join(fractile::journal::JOURNAL_FILE), &journal).unwrap();

    for args in [
        &["rules", "list"][..],
        &["config", "show"][..],
        &["doctor"][..],
    ] {
        let mut child = Command::new(env!("CARGO_BIN_EXE_fractile"))
            .args(args)
            .env("FRACTILE_CONFIG", &config_path)
            .env("FRACTILE_APPLICATION_SUPPORT", &support_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("fractile should spawn");

        let mut reader = BufReader::new(child.stdout.take().unwrap());
        let mut first_line = String::new();
        reader.read_line(&mut first_line).unwrap();
        assert!(!first_line.is_empty(), "args {args:?} produced no output");
        // Drop the reader end immediately to close the pipe while fractile writes the rest.
        drop(reader);

        let mut stderr = String::new();
        child
            .stderr
            .take()
            .unwrap()
            .read_to_string(&mut stderr)
            .unwrap();
        let status = child.wait().expect("fractile should exit");
        assert!(
            status.success(),
            "args {args:?} failed with {status:?}, stderr: {stderr}"
        );
        assert!(
            stderr.is_empty(),
            "args {args:?} wrote unexpected stderr: {stderr}"
        );
    }

    drop(fs::remove_dir_all(&dir));
}
