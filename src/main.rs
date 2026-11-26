use std::{
    collections::{HashMap, HashSet},
    env, fs,
    io::{self, Write},
    process::ExitCode,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

mod cli;

use fractile::{
    Engine, Model, NativeObservation, Reconciliation, ReconciliationEvidence, Settings, reconcile,
};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct Replay {
    #[serde(default)]
    model: Model,
    #[serde(default)]
    evidence: ReconciliationEvidence,
    settings: Settings,
    observations: Vec<NativeObservation>,
}

#[derive(Serialize)]
struct ReplayOutput {
    steps: Vec<fractile::Reconciliation>,
}

#[cfg(target_os = "macos")]
#[derive(Serialize)]
struct SnapshotOutput {
    observation: NativeObservation,
    reconciliation: Reconciliation,
    #[serde(skip_serializing_if = "Option::is_none")]
    apply_results: Option<Vec<fractile::macos::ApplyResult>>,
}

#[cfg(target_os = "macos")]
#[derive(Serialize)]
struct WatchOutput {
    steps: Vec<SnapshotOutput>,
}

fn main() -> ExitCode {
    let mut args = env::args();
    let executable = args.next().unwrap_or_else(|| "fractile".into());
    let Some(command) = args.next() else {
        print_usage(&executable);
        return ExitCode::from(2);
    };
    let arguments = args.collect::<Vec<_>>();
    let rest = arguments.iter().map(String::as_str).collect::<Vec<_>>();
    if let Some(outcome) = cli::try_run(&executable, &command, &rest) {
        return outcome.into_exit_code();
    }
    #[cfg(target_os = "macos")]
    let apply = rest.last() == Some(&"--apply");

    let result = match (command.as_str(), rest.as_slice()) {
        ("replay", [path]) => run_replay(path).and_then(to_json),
        #[cfg(target_os = "macos")]
        ("snapshot", []) => run_retile(false).and_then(to_json),
        #[cfg(target_os = "macos")]
        ("displays", []) => fractile::macos::outputs()
            .map(|outputs| outputs.displays().to_vec())
            .map_err(|error| error.to_string())
            .and_then(to_json),
        ("doctor", []) => run_doctor(None),
        ("doctor", [hours]) => hours
            .parse::<u64>()
            .ok()
            .filter(|hours| *hours > 0)
            .ok_or_else(|| format!("hours must be a positive integer, got {hours:?}"))
            .and_then(|hours| run_doctor(Some(hours))),
        ("config-check", [path]) => fractile::config::load_path(std::path::Path::new(path))
            .map(|_| "configuration is valid".into()),
        #[cfg(target_os = "macos")]
        ("ctl", _) => fractile::control::send(&arguments),
        #[cfg(target_os = "macos")]
        ("open", [target]) => fractile::deep_link::parse_input(target)
            .map_err(|error| error.to_string())
            .and_then(|link| fractile::control::send(&["open".into(), link.canonical_url()])),
        #[cfg(target_os = "macos")]
        ("hud", []) => run_hud(),
        #[cfg(target_os = "macos")]
        ("retile", [] | ["--apply"]) => run_retile(apply).and_then(to_json),
        // A lone positional "--apply" sets `apply` but always fails cycle parsing first.
        #[cfg(target_os = "macos")]
        ("watch", [cycles] | [cycles, "--apply"]) => parse_cycles(cycles)
            .and_then(|cycles| run_watch(cycles, apply, None))
            .and_then(to_json),
        #[cfg(target_os = "macos")]
        ("watch-window", [window_id, cycles] | [window_id, cycles, "--apply"]) => {
            parse_window_id(window_id)
                .and_then(|window_id| parse_cycles(cycles).map(|cycles| (window_id, cycles)))
                .and_then(|(window_id, cycles)| run_watch(cycles, apply, Some(window_id)))
                .and_then(to_json)
        }
        #[cfg(target_os = "macos")]
        ("run", [] | ["--apply"]) => run_persistent(apply),
        (known, _) if KNOWN_COMMANDS.contains(&known) => {
            print_usage(&executable);
            return ExitCode::from(2);
        }
        _ => {
            drop(writeln!(io::stderr().lock(), "unknown command: {command}"));
            print_usage(&executable);
            return ExitCode::from(2);
        }
    };

    match result {
        Ok(output) => cli::write_stdout_line(&output),
        Err(error) => {
            drop(writeln!(io::stderr().lock(), "{error}"));
            ExitCode::FAILURE
        }
    }
}

const KNOWN_COMMANDS: &[&str] = &[
    "replay",
    "config-check",
    "doctor",
    #[cfg(target_os = "macos")]
    "snapshot",
    #[cfg(target_os = "macos")]
    "displays",
    #[cfg(target_os = "macos")]
    "ctl",
    #[cfg(target_os = "macos")]
    "open",
    #[cfg(target_os = "macos")]
    "hud",
    #[cfg(target_os = "macos")]
    "retile",
    #[cfg(target_os = "macos")]
    "watch",
    #[cfg(target_os = "macos")]
    "watch-window",
    #[cfg(target_os = "macos")]
    "run",
];

fn print_usage(executable: &str) {
    drop(writeln!(
        io::stderr().lock(),
        "{}",
        cli::help::root_usage(executable)
    ));
}

fn run_doctor(hours: Option<u64>) -> Result<String, String> {
    let (live, rotated) = fractile::journal::default_paths().ok_or("HOME is unavailable")?;
    let (lines, skipped) = fractile::journal::read(&[&rotated, &live])?;
    if lines.is_empty() && skipped == 0 {
        return Ok(format!(
            "No arrangement journal at {} yet. It is written by a running Fractile agent.",
            live.display()
        ));
    }
    let now = fractile::journal::unix_millis();
    let since = doctor_since(now, hours);
    Ok(fractile::journal::summarize(&lines, since, now, skipped))
}

fn doctor_since(now: u64, hours: Option<u64>) -> u64 {
    hours.map_or(0, |hours| {
        now.saturating_sub(hours.saturating_mul(3_600_000))
    })
}

#[cfg(target_os = "macos")]
fn write_hud_frame(out: &mut impl Write, debug: &serde_json::Value) -> io::Result<()> {
    write!(out, "\x1b[2J\x1b[HFractile HUD  (Ctrl-C to close)\n\n")?;
    for display in debug["displays"].as_array().into_iter().flatten() {
        let frame = &display["frame"];
        let layout = if display["fullscreen_layout"].as_bool() == Some(true) {
            "Fullscreen Layout"
        } else {
            display["axis"].as_str().unwrap_or("unknown")
        };
        let workspace = display["workspace"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| display["workspace"].to_string());
        writeln!(
            out,
            "{}  workspace {}  {}  @ {},{} {}x{}",
            display["name"].as_str().unwrap_or("Display"),
            workspace,
            layout,
            frame["x"].as_i64().unwrap_or_default(),
            frame["y"].as_i64().unwrap_or_default(),
            frame["width"].as_i64().unwrap_or_default(),
            frame["height"].as_i64().unwrap_or_default(),
        )?;
        for tile in debug["tiles"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|tile| tile["workspace"] == display["workspace"])
        {
            let frame = &tile["frame"];
            let window = &tile["window"];
            let focused = if window == &debug["focused"] {
                "*"
            } else {
                " "
            };
            writeln!(
                out,
                "  {focused} window {} (pid {})  {},{} {}x{}",
                window["window_id"].as_u64().unwrap_or_default(),
                window["app"]["pid"].as_i64().unwrap_or_default(),
                frame["x"].as_i64().unwrap_or_default(),
                frame["y"].as_i64().unwrap_or_default(),
                frame["width"].as_i64().unwrap_or_default(),
                frame["height"].as_i64().unwrap_or_default(),
            )?;
        }
        writeln!(out)?;
    }
    writeln!(out, "Directional targets:")?;
    for direction in ["left", "down", "up", "right"] {
        let target = &debug["targets"][direction];
        if target.is_null() {
            writeln!(out, "  {direction:>5}: none")?;
        } else {
            writeln!(
                out,
                "  {direction:>5}: window {} (pid {})",
                target["window_id"].as_u64().unwrap_or_default(),
                target["app"]["pid"].as_i64().unwrap_or_default(),
            )?;
        }
    }
    out.flush()
}

#[cfg(target_os = "macos")]
fn run_hud() -> Result<String, String> {
    loop {
        let source = fractile::control::send(&["debug".into()])?;
        let debug: serde_json::Value =
            serde_json::from_str(&source).map_err(|error| error.to_string())?;
        let mut stdout = io::BufWriter::new(io::stdout().lock());
        if let Err(error) = write_hud_frame(&mut stdout, &debug) {
            if error.kind() == io::ErrorKind::BrokenPipe {
                return Ok(String::new());
            }
            return Err(format!("failed to render HUD: {error}"));
        }
        thread::sleep(Duration::from_millis(150));
    }
}

fn parse_window_id(value: &str) -> Result<u32, String> {
    value
        .parse::<u32>()
        .ok()
        .filter(|window_id| *window_id > 0)
        .ok_or_else(|| "window ID must be a positive integer".into())
}

fn parse_cycles(value: &str) -> Result<u64, String> {
    value
        .parse::<u64>()
        .ok()
        .filter(|cycles| *cycles > 0)
        .ok_or_else(|| "cycles must be a positive integer".into())
}

fn to_json(value: impl Serialize) -> Result<String, String> {
    serde_json::to_string_pretty(&value)
        .map_err(|error| format!("failed to encode output: {error}"))
}

/// The probes tile like the agent: with `config.toml`'s spacing, or none when it can't be read.
#[cfg(target_os = "macos")]
fn configured_spacing() -> fractile::spacing::Spacing {
    fractile::config::Config::load()
        .map(|config| config.spacing())
        .unwrap_or_default()
}

#[cfg(target_os = "macos")]
fn configured_layout() -> fractile::config::LayoutMode {
    fractile::config::Config::load()
        .map(|config| config.layout())
        .unwrap_or_default()
}

#[cfg(target_os = "macos")]
fn run_retile(should_apply: bool) -> Result<SnapshotOutput, String> {
    let (observation, settings) =
        fractile::macos::snapshot(1).map_err(|error| error.to_string())?;
    let settings = settings
        .spaced(configured_spacing())
        .with_layout(configured_layout());
    if should_apply
        && observation
            .applications
            .iter()
            .any(|app| matches!(app, fractile::AppObservation::Unavailable { .. }))
    {
        return Err("refusing to apply an incomplete window observation".into());
    }
    let reconciliation = reconcile(
        &Model::default(),
        &observation,
        &ReconciliationEvidence::default(),
        &settings,
    );
    let apply_results =
        should_apply.then(|| fractile::macos::apply(&reconciliation.placements, &settings));
    Ok(SnapshotOutput {
        observation,
        reconciliation,
        apply_results,
    })
}

#[cfg(target_os = "macos")]
fn run_watch(
    cycles: u64,
    should_apply: bool,
    initial_window_id: Option<u32>,
) -> Result<WatchOutput, String> {
    let mut engine = Engine::default();
    let mut known_apps = HashMap::new();
    let mut scope: Option<WindowScope> = None;
    let mut steps = Vec::new();

    for cycle in 1..=cycles {
        engine.invalidate();
        let ticket = engine.begin_cycle();
        let (mut observation, mut settings) = match (initial_window_id, scope.as_ref()) {
            (Some(window_id), None) => {
                fractile::macos::snapshot_for_window(ticket.cycle, window_id)
            }
            (Some(_), Some(scope)) => {
                fractile::macos::snapshot_in_output(ticket.cycle, scope.output)
            }
            (None, _) => fractile::macos::snapshot(ticket.cycle),
        }
        .map_err(|error| error.to_string())?;
        settings.spacing = configured_spacing();
        settings.layout = configured_layout();
        if let Some(initial_window_id) = initial_window_id {
            let active_scope = match &mut scope {
                Some(scope) => scope,
                None => scope.insert(WindowScope::discover(
                    &observation,
                    initial_window_id,
                    settings.output,
                )?),
            };
            active_scope.filter(&mut observation);
            settings.output = active_scope.frame;
        } else {
            fractile::live::add_absent_applications(&mut observation, &mut known_apps);
        }
        let reconciliation = engine
            .accept(ticket, &observation, &settings)
            .map_err(|error| format!("stale synchronous cycle: {error:?}"))?;
        let placements =
            fractile::live::applicable_placements(&observation, &reconciliation, &settings);
        let apply_results = should_apply.then(|| fractile::macos::apply(&placements, &settings));

        steps.push(SnapshotOutput {
            observation,
            reconciliation,
            apply_results,
        });

        if cycle < cycles {
            thread::sleep(Duration::from_millis(250));
        }
    }
    Ok(WatchOutput { steps })
}

#[cfg(target_os = "macos")]
fn run_persistent(should_apply: bool) -> Result<String, String> {
    let running = Arc::new(AtomicBool::new(true));
    let stop = Arc::clone(&running);
    ctrlc::set_handler(move || stop.store(false, Ordering::Release))
        .map_err(|error| format!("failed to install signal handler: {error}"))?;

    let mut engine = Engine::default();
    let mut known_apps = HashMap::new();
    let spacing = configured_spacing();
    let layout = configured_layout();
    let mut output = io::BufWriter::new(io::stdout().lock());
    let mut completed_cycles = 0_u64;

    while running.load(Ordering::Acquire) {
        engine.invalidate();
        let ticket = engine.begin_cycle();
        let (mut observation, mut settings) = match fractile::macos::snapshot(ticket.cycle) {
            Ok(snapshot) => snapshot,
            Err(fractile::macos::SnapshotError::AccessibilityPermissionRequired) => {
                return Err(
                    "Accessibility permission is required for the persistent runtime".into(),
                );
            }
            Err(error) => {
                eprintln!(
                    "{}",
                    serde_json::json!({
                        "type": "snapshot_error",
                        "cycle": ticket.cycle,
                        "error": error.to_string(),
                    })
                );
                thread::sleep(Duration::from_millis(250));
                continue;
            }
        };
        if !running.load(Ordering::Acquire) {
            engine.invalidate();
            break;
        }
        fractile::live::add_absent_applications(&mut observation, &mut known_apps);
        settings.spacing = spacing;
        settings.layout = layout;
        let reconciliation = engine
            .accept(ticket, &observation, &settings)
            .map_err(|error| format!("stale synchronous cycle: {error:?}"))?;
        let placements =
            fractile::live::applicable_placements(&observation, &reconciliation, &settings);
        let apply_results = (should_apply && running.load(Ordering::Acquire)).then(|| {
            fractile::macos::apply_while(&placements, &settings, || running.load(Ordering::Acquire))
        });
        let step = SnapshotOutput {
            observation,
            reconciliation,
            apply_results,
        };
        serde_json::to_writer(
            &mut output,
            &serde_json::json!({ "type": "cycle", "step": step }),
        )
        .map_err(|error| format!("failed to encode runtime step: {error}"))?;
        output
            .write_all(b"\n")
            .and_then(|()| output.flush())
            .map_err(|error| format!("failed to write runtime step: {error}"))?;
        completed_cycles += 1;

        for _ in 0..10 {
            if !running.load(Ordering::Acquire) {
                break;
            }
            thread::sleep(Duration::from_millis(25));
        }
    }

    Ok(serde_json::json!({
        "type": "stopped",
        "cycles": completed_cycles,
        "revision": engine.revision(),
    })
    .to_string())
}

#[cfg(target_os = "macos")]
struct WindowScope {
    app: fractile::AppInstanceId,
    bundle_id: String,
    frame: fractile::Rect,
    output: fractile::Rect,
    excluded_window_ids: HashSet<u32>,
    authorized_window_ids: HashSet<u32>,
    missing_cycles: u8,
}

#[cfg(target_os = "macos")]
impl WindowScope {
    fn discover(
        observation: &NativeObservation,
        window_id: u32,
        output: fractile::Rect,
    ) -> Result<Self, String> {
        observation
            .applications
            .iter()
            .find_map(|app| match app {
                fractile::AppObservation::Complete { app, windows, .. } => windows
                    .iter()
                    .find(|window| window.id.window_id == window_id)
                    .map(|window| Self {
                        app: app.id.clone(),
                        bundle_id: app.bundle_id.clone(),
                        frame: window.frame,
                        output,
                        excluded_window_ids: windows
                            .iter()
                            .filter(|existing| existing.id.window_id != window_id)
                            .map(|existing| existing.id.window_id)
                            .collect(),
                        authorized_window_ids: HashSet::from([window_id]),
                        missing_cycles: 0,
                    }),
                _ => None,
            })
            .ok_or_else(|| format!("window {window_id} is not currently eligible"))
    }

    fn filter(&mut self, observation: &mut NativeObservation) {
        let mut scoped = observation
            .applications
            .drain(..)
            .find(|app| app.app_id() == &self.app);
        if let Some(fractile::AppObservation::Complete {
            focused, windows, ..
        }) = &mut scoped
        {
            windows.retain(|window| {
                !self.excluded_window_ids.contains(&window.id.window_id)
                    && window.frame.approximately_equals(self.frame, 100)
            });
            let selected = focused
                .as_ref()
                .and_then(|focused| windows.iter().find(|window| window.id == *focused))
                .or_else(|| {
                    windows
                        .iter()
                        .find(|window| self.authorized_window_ids.contains(&window.id.window_id))
                })
                .or_else(|| (windows.len() == 1).then(|| &windows[0]))
                .map(|window| window.id.clone());
            windows.retain(|window| selected.as_ref() == Some(&window.id));
            if let Some(selected) = selected {
                self.authorized_window_ids.insert(selected.window_id);
                *focused = focused.take().filter(|focused| focused == &selected);
                self.missing_cycles = 0;
                if let Some(scoped) = scoped {
                    observation.applications = vec![scoped];
                }
                return;
            }
        }
        self.missing_cycles = self.missing_cycles.saturating_add(1);
        observation.applications = vec![if self.missing_cycles <= 8 {
            fractile::AppObservation::Unavailable {
                app: self.app.clone(),
                error: "scoped window is settling or absent".into(),
            }
        } else {
            fractile::AppObservation::Complete {
                app: fractile::ObservedApplication {
                    id: self.app.clone(),
                    bundle_id: self.bundle_id.clone(),
                },
                focused: None,
                windows: Vec::new(),
            }
        }];
    }
}

fn run_replay(path: &str) -> Result<ReplayOutput, String> {
    let input =
        fs::read_to_string(path).map_err(|error| format!("failed to read {path}: {error}"))?;
    let replay: Replay =
        serde_json::from_str(&input).map_err(|error| format!("invalid fixture {path}: {error}"))?;
    replay
        .model
        .validate()
        .map_err(|error| format!("invalid fixture model: {error}"))?;
    if replay.settings.output.width <= 0
        || replay.settings.output.height <= 0
        || replay.settings.frame_tolerance < 0
    {
        return Err("invalid fixture settings".into());
    }
    let mut model = replay.model;
    let mut evidence = replay.evidence;
    let mut steps = Vec::new();

    for observation in replay.observations {
        let result = reconcile(&model, &observation, &evidence, &replay.settings);
        evidence.record(&observation, &result);
        model = result.model.clone();
        steps.push(result);
    }

    Ok(ReplayOutput { steps })
}

#[cfg(test)]
mod tests {
    use super::doctor_since;

    #[test]
    fn doctor_since_saturates_extreme_hour_counts_instead_of_overflowing() {
        let now = 1_795_500_000_000;
        assert_eq!(doctor_since(now, None), 0);
        assert_eq!(doctor_since(now, Some(1)), now - 3_600_000);
        assert_eq!(doctor_since(now, Some(144_115_188_075_855_872)), 0);
        assert_eq!(doctor_since(now, Some(u64::MAX)), 0);
    }
}
