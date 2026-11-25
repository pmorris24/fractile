use super::*;
use crate::scratchpad::{ScratchpadCommand, summon_frame};

fn lab() -> Config {
    Config::parse("version = 1\n[labs]\nscratchpad = true\n").unwrap()
}

/// Windows 10 and 20 tiled on Workspace 1 and window 30 parked on Workspace 2, all on the
/// main display; window 10 is focused.
fn scratchpad_runtime(config: Config) -> WorkspaceRuntime<FakeBackend> {
    let windows = [
        (window(10), frame(50)),
        (window(20), frame(500)),
        (window(30), frame(200)),
    ];
    let backend = FakeBackend {
        frames: windows.iter().cloned().collect(),
        focused: Some(window(10)),
        frontmost: Some(window(10).app),
        observation: Some(observation_for_windows(
            "com.example.Test",
            &window(10),
            &windows,
        )),
        ..FakeBackend::default()
    };
    let mut runtime = runtime_with_config(backend, config);
    bind(&mut runtime, WorkspaceId::ONE, &window(10));
    bind(&mut runtime, WorkspaceId::ONE, &window(20));
    bind(&mut runtime, WorkspaceId::TWO, &window(30));
    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();
    assert!(runtime.parked.contains_key(&window(30)));
    runtime.backend.calls.clear();
    runtime
}

fn run(
    runtime: &mut WorkspaceRuntime<FakeBackend>,
    action: WorkerAction,
) -> Result<CycleStatus, String> {
    match runtime.execute(
        WorkerCommand::Action(action, outputs()),
        &AtomicBool::new(false),
    ) {
        Some(WorkerResult::Status { result, .. }) => result,
        _ => panic!("an action returns a status"),
    }
}

fn scratchpad(
    runtime: &mut WorkspaceRuntime<FakeBackend>,
    command: ScratchpadCommand,
) -> Result<CycleStatus, String> {
    run(runtime, WorkerAction::Scratchpad(command))
}

fn error(result: Result<CycleStatus, String>) -> String {
    match result {
        Ok(_) => panic!("expected an error"),
        Err(error) => error,
    }
}

fn on_screen(runtime: &WorkspaceRuntime<FakeBackend>, window: &crate::PhysicalWindowId) -> bool {
    outputs()
        .frames()
        .any(|display| display.contains_center(runtime.backend.frames[window]))
}

/// Where a window without a remembered size is summoned on the main display.
fn summoned() -> crate::Rect {
    summon_frame(outputs().main().frame, None)
}

fn pad(runtime: &WorkspaceRuntime<FakeBackend>) -> crate::scratchpad::Scratchpad {
    runtime.workspaces.scratchpad()
}

fn assert_hidden(runtime: &WorkspaceRuntime<FakeBackend>, window: &crate::PhysicalWindowId) {
    assert!(pad(runtime).contains(window));
    assert_eq!(pad(runtime).entry(window).unwrap().shown_on, None);
    assert!(runtime.parked.contains_key(window), "parked in memory");
    assert!(
        runtime.backend.persisted.contains_key(window),
        "parked-windows.json holds its recovery frame"
    );
    assert_eq!(runtime.workspaces.workspace_of_any(window), None);
    assert!(!on_screen(runtime, window));
}

#[test]
fn marking_a_tiled_window_parks_it_durably_and_reflows_its_siblings() {
    let mut runtime = scratchpad_runtime(lab());
    let marked = window(10);
    let original = runtime.backend.frames[&marked];

    scratchpad(&mut runtime, ScratchpadCommand::Mark).unwrap();

    assert_hidden(&runtime, &marked);
    assert_eq!(runtime.parked[&marked], original);
    assert_eq!(runtime.backend.persisted[&marked], original);
    let persisted = runtime
        .backend
        .calls
        .iter()
        .position(|call| *call == BackendCall::Persist)
        .unwrap();
    let moved = runtime
        .backend
        .calls
        .iter()
        .position(|call| matches!(call, BackendCall::SetPosition(window, _) if *window == marked))
        .unwrap();
    assert!(
        persisted < moved,
        "the recovery frame is durable before the window leaves the screen"
    );
    assert_eq!(
        runtime.workspaces.engine(WorkspaceId::ONE).bound_windows(),
        vec![window(20)]
    );
    assert_eq!(runtime.backend.frames[&window(20)], outputs().main().frame);
    assert_eq!(runtime.backend.focused, Some(window(20)));

    // Later cycles leave it hidden instead of recovering it as a stray window.
    for _ in 0..3 {
        run(&mut runtime, WorkerAction::Retile).unwrap();
        assert_hidden(&runtime, &marked);
    }
}

#[test]
fn summoning_on_workspace_two_floats_it_centred_without_disturbing_the_tiles() {
    let mut runtime = scratchpad_runtime(lab());
    let marked = window(10);
    scratchpad(&mut runtime, ScratchpadCommand::Mark).unwrap();
    run(
        &mut runtime,
        WorkerAction::SwitchWorkspace(WorkspaceId::TWO),
    )
    .unwrap();
    let tile = runtime.backend.frames[&window(30)];
    assert_eq!(tile, outputs().main().frame);

    let status = scratchpad(&mut runtime, ScratchpadCommand::Toggle).unwrap();

    assert!(status.hud.is_some());
    assert_eq!(runtime.backend.frames[&marked], summoned());
    assert_eq!(
        runtime.workspaces.floating_workspace_of(&marked),
        Some(WorkspaceId::TWO)
    );
    assert!(runtime.workspaces.is_manual_float(&marked));
    assert_eq!(
        pad(&runtime).entry(&marked).unwrap().shown_on,
        Some(WorkspaceId::TWO)
    );
    assert!(!runtime.parked.contains_key(&marked));
    assert!(!runtime.backend.persisted.contains_key(&marked));
    assert_eq!(runtime.backend.focused, Some(marked.clone()));
    assert_eq!(
        runtime.workspaces.engine(WorkspaceId::TWO).bound_windows(),
        vec![window(30)]
    );
    assert_eq!(runtime.backend.frames[&window(30)], tile);
    for _ in 0..3 {
        run(&mut runtime, WorkerAction::Retile).unwrap();
        assert_eq!(runtime.backend.frames[&marked], summoned());
        assert_eq!(runtime.workspaces.workspace_of(&marked), None);
    }

    scratchpad(&mut runtime, ScratchpadCommand::Toggle).unwrap();

    assert_hidden(&runtime, &marked);
    assert_eq!(runtime.parked[&marked], summoned());
    assert_eq!(runtime.backend.focused, Some(window(30)));
    assert_eq!(runtime.backend.frames[&window(30)], tile);
}

#[test]
fn marking_a_shown_scratchpad_window_tiles_it_on_the_current_workspace() {
    let mut runtime = scratchpad_runtime(lab());
    let marked = window(10);
    scratchpad(&mut runtime, ScratchpadCommand::Mark).unwrap();
    run(
        &mut runtime,
        WorkerAction::SwitchWorkspace(WorkspaceId::TWO),
    )
    .unwrap();
    scratchpad(&mut runtime, ScratchpadCommand::Toggle).unwrap();

    scratchpad(&mut runtime, ScratchpadCommand::Mark).unwrap();

    assert!(!pad(&runtime).contains(&marked));
    assert!(!runtime.workspaces.is_manual_float(&marked));
    assert_eq!(
        runtime.workspaces.workspace_of(&marked),
        Some(WorkspaceId::TWO)
    );
    let bound = runtime.workspaces.engine(WorkspaceId::TWO).bound_windows();
    assert_eq!(bound.len(), 2);
    assert!(bound.contains(&window(30)));
    assert!(!runtime.parked.contains_key(&marked));
}

#[test]
fn toggles_cycle_through_several_scratchpad_windows() {
    let mut runtime = scratchpad_runtime(lab());
    scratchpad(&mut runtime, ScratchpadCommand::Mark).unwrap();
    runtime.backend.focused = Some(window(20));
    scratchpad(&mut runtime, ScratchpadCommand::Mark).unwrap();
    assert!(
        runtime
            .workspaces
            .engine(WorkspaceId::ONE)
            .bound_windows()
            .is_empty()
    );

    for expected in [10, 20, 10] {
        scratchpad(&mut runtime, ScratchpadCommand::Toggle).unwrap();
        let shown = pad(&runtime).shown_on(WorkspaceId::ONE);
        assert_eq!(shown, vec![window(expected)]);
        assert!(on_screen(&runtime, &window(expected)));
        scratchpad(&mut runtime, ScratchpadCommand::Toggle).unwrap();
        assert_hidden(&runtime, &window(expected));
    }
    assert_hidden(&runtime, &window(20));
}

#[test]
fn a_closed_scratchpad_window_is_pruned_with_its_recovery_frame() {
    let mut runtime = scratchpad_runtime(lab());
    let closed = window(10);
    scratchpad(&mut runtime, ScratchpadCommand::Mark).unwrap();
    runtime.backend.frames.remove(&closed);

    run(&mut runtime, WorkerAction::Retile).unwrap();

    assert!(!pad(&runtime).contains(&closed));
    assert!(!runtime.parked.contains_key(&closed));
    assert!(!runtime.backend.persisted.contains_key(&closed));
    assert_eq!(
        error(scratchpad(&mut runtime, ScratchpadCommand::Toggle)),
        "Scratchpad is empty"
    );
}

#[test]
fn a_shown_window_hides_when_its_workspace_is_switched_away_and_stays_hidden() {
    let mut runtime = scratchpad_runtime(lab());
    let marked = window(10);
    scratchpad(&mut runtime, ScratchpadCommand::Mark).unwrap();
    scratchpad(&mut runtime, ScratchpadCommand::Toggle).unwrap();
    assert_eq!(
        runtime.workspaces.floating_workspace_of(&marked),
        Some(WorkspaceId::ONE)
    );

    run(
        &mut runtime,
        WorkerAction::SwitchWorkspace(WorkspaceId::TWO),
    )
    .unwrap();
    assert_hidden(&runtime, &marked);

    run(
        &mut runtime,
        WorkerAction::SwitchWorkspace(WorkspaceId::ONE),
    )
    .unwrap();
    run(&mut runtime, WorkerAction::Retile).unwrap();
    assert_hidden(&runtime, &marked);
    assert!(on_screen(&runtime, &window(20)));
}

#[test]
fn pause_restores_hidden_scratchpad_windows_without_leaks() {
    let mut runtime = scratchpad_runtime(lab());
    let marked = window(10);
    let original = runtime.backend.frames[&marked];
    scratchpad(&mut runtime, ScratchpadCommand::Mark).unwrap();

    let result = runtime.execute(
        WorkerCommand::RestoreAll(Some(outputs())),
        &AtomicBool::new(false),
    );

    assert!(matches!(result, Some(WorkerResult::Restored { failed: 0 })));
    assert!(runtime.parked.is_empty());
    assert!(runtime.backend.persisted.is_empty());
    assert!(pad(&runtime).is_empty());
    assert_eq!(runtime.backend.frames[&marked], original);
}

#[test]
fn a_crash_while_hidden_restores_the_window_on_the_next_start() {
    let mut runtime = scratchpad_runtime(lab());
    let marked = window(10);
    let original = runtime.backend.frames[&marked];
    scratchpad(&mut runtime, ScratchpadCommand::Mark).unwrap();
    let persisted = runtime.backend.persisted.clone();
    let mut backend = std::mem::take(&mut runtime.backend);
    backend.calls.clear();

    // A new agent starts from parked-windows.json alone: the scratchpad is not persisted.
    let mut restarted =
        WorkspaceRuntime::with_backend_and_config(ManagementScope::Both, backend, persisted, lab());
    restarted.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    assert!(!restarted.parked.contains_key(&marked));
    assert!(!restarted.backend.persisted.contains_key(&marked));
    assert_eq!(
        (
            restarted.backend.frames[&marked].x,
            restarted.backend.frames[&marked].y
        ),
        (original.x, original.y)
    );
}

#[test]
fn turning_the_lab_off_brings_hidden_windows_back() {
    let mut runtime = scratchpad_runtime(lab());
    let marked = window(10);
    scratchpad(&mut runtime, ScratchpadCommand::Mark).unwrap();

    runtime.replace_config(Config::default());
    run(&mut runtime, WorkerAction::Retile).unwrap();

    assert!(pad(&runtime).is_empty());
    assert!(!runtime.parked.contains_key(&marked));
    assert!(!runtime.backend.persisted.contains_key(&marked));
    assert!(on_screen(&runtime, &marked));
}

#[test]
fn the_scratchpad_does_nothing_while_the_lab_is_off() {
    let mut runtime = scratchpad_runtime(Config::default());

    for command in [ScratchpadCommand::Mark, ScratchpadCommand::Toggle] {
        let error = error(scratchpad(&mut runtime, command));
        assert!(error.starts_with("Scratchpad is off"), "{error}");
    }

    assert!(pad(&runtime).is_empty());
    assert!(!runtime.parked.contains_key(&window(10)));
    assert_eq!(
        runtime
            .workspaces
            .engine(WorkspaceId::ONE)
            .bound_windows()
            .len(),
        2
    );
    assert!(
        !runtime
            .backend
            .calls
            .iter()
            .any(|call| matches!(call, BackendCall::SetPosition(..))),
        "no window moved"
    );
}

#[test]
fn an_empty_scratchpad_toggle_is_a_clean_no_op() {
    let mut runtime = scratchpad_runtime(lab());

    assert_eq!(
        error(scratchpad(&mut runtime, ScratchpadCommand::Toggle)),
        "Scratchpad is empty"
    );
    assert!(runtime.backend.calls.is_empty());
}

#[test]
fn adding_a_display_reparks_hidden_scratchpad_windows_without_dropping_membership() {
    let single = macos::DisplayOutputs::new(vec![outputs().main().clone()]).unwrap();
    let mut runtime = scratchpad_runtime(lab());

    let Some(WorkerResult::Status { result, .. }) = runtime.execute(
        WorkerCommand::Action(WorkerAction::Retile, single.clone()),
        &AtomicBool::new(false),
    ) else {
        panic!("expected retile status");
    };
    result.unwrap();

    let Some(WorkerResult::Status { result, .. }) = runtime.execute(
        WorkerCommand::Action(
            WorkerAction::Scratchpad(ScratchpadCommand::Mark),
            single.clone(),
        ),
        &AtomicBool::new(false),
    ) else {
        panic!("expected mark status");
    };
    result.unwrap();
    assert!(pad(&runtime).hidden().contains(&window(10)));
    assert!(
        !crate::geometry::shows_on(
            runtime.backend.frames[&window(10)],
            &single.frames().collect::<Vec<_>>(),
        ),
        "window should be parked off the single display"
    );

    // Connect a second display over the right parking edge: hidden scratchpad window stays hidden.
    let Some(WorkerResult::Status { result, .. }) = runtime.execute(
        WorkerCommand::Action(WorkerAction::Retile, outputs()),
        &AtomicBool::new(false),
    ) else {
        panic!("expected retile status after display add");
    };
    result.unwrap();

    let all_displays: Vec<_> = outputs().frames().collect();
    assert!(
        !crate::geometry::shows_on(runtime.backend.frames[&window(10)], &all_displays),
        "hidden scratchpad window leaked onto newly added display: {:?}",
        runtime.backend.frames[&window(10)]
    );
    assert!(pad(&runtime).hidden().contains(&window(10)));
    assert_eq!(runtime.workspaces.workspace_of_any(&window(10)), None);
    assert!(runtime.parked.contains_key(&window(10)));

    // Summoning it still reveals it cleanly.
    let Some(WorkerResult::Status { result, .. }) = runtime.execute(
        WorkerCommand::Action(
            WorkerAction::Scratchpad(ScratchpadCommand::Toggle),
            outputs(),
        ),
        &AtomicBool::new(false),
    ) else {
        panic!("expected toggle status");
    };
    result.unwrap();
    assert!(
        pad(&runtime)
            .entry(&window(10))
            .is_some_and(|entry| entry.shown_on.is_some())
    );
    assert!(crate::geometry::shows_on(
        runtime.backend.frames[&window(10)],
        &all_displays
    ));
}

#[test]
fn delayed_scratchpad_summon_stays_centered_when_a_later_cycle_confirms_it() {
    let mut runtime = scratchpad_runtime(lab());
    let w = window(10);

    // Tile and then hide into scratchpad.
    let _ = runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();
    scratchpad(&mut runtime, ScratchpadCommand::Mark).unwrap();
    assert!(runtime.parked.contains_key(&w));
    let parked_pos = runtime.backend.frames[&w];

    // Simulate a lagging window server: reports the old parked frame during summon's immediate check.
    runtime.backend.server_frames.insert(w.clone(), parked_pos);
    runtime.backend.frozen_server_frames = true;
    scratchpad(&mut runtime, ScratchpadCommand::Toggle).unwrap();
    runtime.backend.frozen_server_frames = false;
    let centered = runtime.backend.frames[&w];
    assert_eq!(
        centered,
        crate::Rect {
            x: 125,
            y: 100,
            width: 750,
            height: 600
        }
    );
    assert!(
        runtime.parked.contains_key(&w),
        "recovery frame stays persisted until confirmed"
    );
    assert_eq!(runtime.parked.summon_destination(&w), Some(centered));

    // Expire the retry throttle and let the window server catch up to the centered frame.
    runtime.backend.advance(Duration::from_secs(5));
    runtime.backend.server_frames.insert(w.clone(), centered);
    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    assert_eq!(
        runtime.backend.frames[&w], centered,
        "later cycle must confirm the centered summon frame, not restore the old tile position"
    );
    assert!(!runtime.parked.contains_key(&w));
    assert_eq!(runtime.parked.summon_destination(&w), None);
}

#[test]
fn second_toggle_supersedes_an_unconfirmed_scratchpad_summon_with_a_park_write() {
    let mut rt = scratchpad_runtime(lab());
    let w = window(10);
    scratchpad(&mut rt, ScratchpadCommand::Mark).unwrap();
    assert_hidden(&rt, &w);

    let parked = rt.backend.frames[&w];
    rt.backend.server_frames.insert(w.clone(), parked);
    rt.backend.frozen_server_frames = true;
    scratchpad(&mut rt, ScratchpadCommand::Toggle).unwrap();
    assert!(rt.parked.summon_destination(&w).is_some());

    rt.backend.calls.clear();
    scratchpad(&mut rt, ScratchpadCommand::Toggle).unwrap();
    assert!(
        rt.backend.calls.iter().any(|call| matches!(
            call,
            BackendCall::SetPosition(target_window, target)
                if *target_window == w
                    && !crate::geometry::shows_on(*target, &outputs().frames().collect::<Vec<_>>())
        )),
        "hide must supersede the unconfirmed summon with an off-screen park write"
    );
    assert_eq!(rt.parked.summon_destination(&w), None);
    assert_hidden(&rt, &w);
}

#[test]
fn background_bounds_reassertion_reparks_hidden_scratchpad_window_while_frontmost_honors_it() {
    let target = window(10);
    let mut other = window(20);
    other.app.pid = 2;
    let mut snapshot = observation_for("com.example.Target", &target, frame(50));
    snapshot
        .applications
        .extend(observation_for("com.example.Other", &other, frame(500)).applications);
    let backend = FakeBackend {
        frames: [(target.clone(), frame(50)), (other.clone(), frame(500))]
            .into_iter()
            .collect(),
        focused: Some(target.clone()),
        frontmost: Some(target.app.clone()),
        observation: Some(snapshot),
        ..FakeBackend::default()
    };
    let mut rt = runtime_with_config(backend, lab());
    run(&mut rt, WorkerAction::Retile).unwrap();
    scratchpad(&mut rt, ScratchpadCommand::Mark).unwrap();
    assert_hidden(&rt, &target);
    let recovery = rt.parked[&target];

    // Expire settle grace and simulate a background app reasserting saved bounds
    // while another app stays frontmost.
    rt.backend.focused = Some(other.clone());
    rt.backend.frontmost = Some(other.app.clone());
    rt.backend.advance(Duration::from_secs(5));
    rt.backend.frames.insert(target.clone(), recovery);
    rt.backend.server_frames.insert(target.clone(), recovery);

    for _ in 0..3 {
        run(&mut rt, WorkerAction::Retile).unwrap();
    }
    assert_hidden(&rt, &target);
    assert_eq!(rt.parked.get(&target), Some(&recovery));
    assert_eq!(rt.backend.persisted.get(&target), Some(&recovery));
    assert_eq!(rt.workspaces.workspace_of_any(&target), None);

    // Still summonable via Toggle.
    scratchpad(&mut rt, ScratchpadCommand::Toggle).unwrap();
    assert_eq!(
        pad(&rt).entry(&target).and_then(|e| e.shown_on),
        Some(WorkspaceId::ONE)
    );
    scratchpad(&mut rt, ScratchpadCommand::Toggle).unwrap();
    assert_hidden(&rt, &target);

    // Control: frontmost activation still honors the reappearance and adopts it.
    let recovery = rt.parked[&target];
    rt.backend.advance(Duration::from_secs(5));
    rt.backend.frames.insert(target.clone(), recovery);
    rt.backend.server_frames.insert(target.clone(), recovery);
    rt.backend.focused = Some(target.clone());
    rt.backend.frontmost = Some(target.app.clone());
    run(&mut rt, WorkerAction::Retile).unwrap();
    assert!(!pad(&rt).contains(&target));
    assert!(!rt.parked.contains_key(&target));
    assert_eq!(
        rt.workspaces.workspace_of_any(&target),
        Some(WorkspaceId::ONE)
    );
}

#[test]
fn tiling_an_unconfirmed_scratchpad_summon_keeps_tiles_separate_after_arrival() {
    let mut rt = scratchpad_runtime(lab());
    let w = window(10);
    let sibling = window(20);
    scratchpad(&mut rt, ScratchpadCommand::Mark).unwrap();
    assert_hidden(&rt, &w);
    rt.backend
        .server_frames
        .insert(w.clone(), rt.backend.frames[&w]);
    rt.backend.frozen_server_frames = true;
    scratchpad(&mut rt, ScratchpadCommand::Toggle).unwrap();
    assert!(rt.parked.summon_destination(&w).is_some());

    // Tile while the summon is still unconfirmed.
    run(&mut rt, WorkerAction::ToggleFloat).unwrap();
    assert_eq!(rt.workspaces.workspace_of(&w), Some(WorkspaceId::ONE));
    assert!(!pad(&rt).contains(&w));
    let tile_frame = rt.backend.frames[&w];
    assert_ne!(tile_frame, rt.backend.frames[&sibling]);

    // Delayed tile move arrives and retry throttle expires: later cycle must not
    // overwrite the tile with its pre-summon recovery frame.
    rt.backend.server_frames.insert(w.clone(), tile_frame);
    rt.backend.frozen_server_frames = false;
    rt.backend.advance(Duration::from_secs(5));
    run(&mut rt, WorkerAction::Retile).unwrap();
    assert_eq!(rt.backend.frames[&w], tile_frame);
    assert_ne!(rt.backend.frames[&w], rt.backend.frames[&sibling]);
    assert!(!rt.parked.contains_key(&w));
    assert!(!rt.backend.persisted.contains_key(&w));
}

#[test]
fn switching_away_before_delayed_summon_confirmation_remembers_floated_size() {
    let mut rt = scratchpad_runtime(lab());
    let w = window(10);
    scratchpad(&mut rt, ScratchpadCommand::Mark).unwrap();
    assert_hidden(&rt, &w);
    rt.backend
        .server_frames
        .insert(w.clone(), rt.backend.frames[&w]);
    rt.backend.frozen_server_frames = true;
    scratchpad(&mut rt, ScratchpadCommand::Toggle).unwrap();
    assert_eq!(rt.backend.frames[&w], summoned());
    assert!(rt.parked.summon_destination(&w).is_some());

    // Summon arrives on the window server, then the user switches away before a cycle confirms it.
    rt.backend.frozen_server_frames = false;
    rt.backend.server_frames.insert(w.clone(), summoned());
    run(&mut rt, WorkerAction::SwitchWorkspace(WorkspaceId::TWO)).unwrap();
    assert_hidden(&rt, &w);
    assert_eq!(pad(&rt).entry(&w).unwrap().size, Some((750, 600)));

    run(&mut rt, WorkerAction::SwitchWorkspace(WorkspaceId::ONE)).unwrap();
    scratchpad(&mut rt, ScratchpadCommand::Toggle).unwrap();
    run(&mut rt, WorkerAction::Retile).unwrap();
    assert_eq!(rt.backend.frames[&w], summoned());
    assert_eq!(rt.backend.server_frames[&w], summoned());
    assert_eq!(rt.parked.summon_destination(&w), None);
    assert!(!rt.parked.contains_key(&w));
}

#[test]
fn scratchpad_actions_honor_and_track_pending_focus_during_activation_lag() {
    let w = |id: u32| {
        let mut win = window(id);
        win.app.pid = id as i32;
        win
    };
    let report = |rt: &mut WorkspaceRuntime<FakeBackend>, id: u32| {
        rt.backend.focused = Some(w(id));
        rt.backend.frontmost = Some(w(id).app);
    };
    let act = |rt: &mut WorkspaceRuntime<FakeBackend>, action: WorkerAction| {
        let mut observation = crate::NativeObservation {
            cycle: 0,
            mouse_down: false,
            applications: Vec::new(),
        };
        for id in [10, 20, 30] {
            let win = w(id);
            let mut part = observation_for_windows(
                &format!("com.example.App{id}"),
                &win,
                &[(win.clone(), rt.backend.frames[&win])],
            );
            if rt.backend.focused.as_ref() != Some(&win)
                && let crate::AppObservation::Complete { focused, .. } = &mut part.applications[0]
            {
                *focused = None;
            }
            observation.applications.extend(part.applications);
        }
        rt.backend.observation = Some(observation);
        run(rt, action)
    };
    let setup = || {
        let backend = FakeBackend {
            frames: HashMap::from([
                (w(10), frame(50)),
                (w(20), frame(500)),
                (w(30), frame(1050)),
            ]),
            focused: Some(w(10)),
            frontmost: Some(w(10).app),
            ..FakeBackend::default()
        };
        let mut rt = runtime_with_config(backend, lab());
        bind(&mut rt, WorkspaceId::ONE, &w(10));
        bind(&mut rt, WorkspaceId::ONE, &w(20));
        bind(&mut rt, WorkspaceId::FIVE, &w(30));
        rt
    };
    let hidden_ids = |rt: &WorkspaceRuntime<FakeBackend>| {
        let mut ids: Vec<_> = pad(rt)
            .entries()
            .iter()
            .filter(|e| e.shown_on.is_none())
            .map(|e| e.window.window_id)
            .collect();
        ids.sort();
        ids
    };
    let shown = || {
        let mut rt = setup();
        report(&mut rt, 20);
        act(&mut rt, WorkerAction::Scratchpad(ScratchpadCommand::Mark)).unwrap();
        act(&mut rt, WorkerAction::Scratchpad(ScratchpadCommand::Toggle)).unwrap();
        assert_eq!(
            pad(&rt).entry(&w(20)).unwrap().shown_on,
            Some(WorkspaceId::ONE)
        );
        rt
    };

    for stale in [true, false] {
        // 1. Focus right 10 -> 20, then Mark hides 20.
        let mut rt = setup();
        act(&mut rt, WorkerAction::Focus(Direction::Right)).unwrap();
        if stale {
            report(&mut rt, 10);
        }
        act(&mut rt, WorkerAction::Scratchpad(ScratchpadCommand::Mark)).unwrap();
        assert_eq!(hidden_ids(&rt), vec![20]);

        // 2. Hide 20, focus right 10 -> 30 on secondary, Toggle summons onto ws 5.
        let mut rt = setup();
        report(&mut rt, 20);
        act(&mut rt, WorkerAction::Scratchpad(ScratchpadCommand::Mark)).unwrap();
        act(&mut rt, WorkerAction::Focus(Direction::Right)).unwrap();
        if stale {
            report(&mut rt, 10);
        }
        act(&mut rt, WorkerAction::Scratchpad(ScratchpadCommand::Toggle)).unwrap();
        assert_eq!(
            pad(&rt).entry(&w(20)).unwrap().shown_on,
            Some(WorkspaceId::FIVE)
        );

        // 3. Summon 20, then Mark tiles 20 instead of hiding 10.
        let mut rt = shown();
        if stale {
            report(&mut rt, 10);
        }
        act(&mut rt, WorkerAction::Scratchpad(ScratchpadCommand::Mark)).unwrap();
        assert!(hidden_ids(&rt).is_empty());
        assert_eq!(rt.workspaces.workspace_of(&w(20)), Some(WorkspaceId::ONE));

        // 4. Summon 20, then ToggleFloat tiles 20 instead of floating 10.
        let mut rt = shown();
        if stale {
            report(&mut rt, 10);
        }
        act(&mut rt, WorkerAction::ToggleFloat).unwrap();
        assert_eq!(rt.workspaces.workspace_of(&w(20)), Some(WorkspaceId::ONE));
        assert_eq!(rt.workspaces.floating_workspace_of(&w(10)), None);

        // 5. Hide shown 20 (focuses 10), then Mark hides 10 while AX still reports 20.
        let mut rt = shown();
        act(&mut rt, WorkerAction::Scratchpad(ScratchpadCommand::Toggle)).unwrap();
        if stale {
            report(&mut rt, 20);
        }
        act(&mut rt, WorkerAction::Scratchpad(ScratchpadCommand::Mark)).unwrap();
        assert_eq!(hidden_ids(&rt), vec![10, 20]);
    }
}

#[test]
fn mark_after_quick_focus_and_summon_ignores_intermediate_focus_report() {
    let w = |id: u32| {
        let mut win = window(id);
        win.app.pid = id as i32;
        win
    };
    let report = |rt: &mut WorkspaceRuntime<FakeBackend>, id: u32| {
        rt.backend.focused = Some(w(id));
        rt.backend.frontmost = Some(w(id).app);
    };
    let act = |rt: &mut WorkspaceRuntime<FakeBackend>, action: WorkerAction| {
        let mut observation = crate::NativeObservation {
            cycle: 0,
            mouse_down: false,
            applications: Vec::new(),
        };
        for id in [10, 20, 30] {
            let win = w(id);
            let mut part = observation_for_windows(
                &format!("com.example.App{id}"),
                &win,
                &[(win.clone(), rt.backend.frames[&win])],
            );
            if rt.backend.focused.as_ref() != Some(&win)
                && let crate::AppObservation::Complete { focused, .. } = &mut part.applications[0]
            {
                *focused = None;
            }
            observation.applications.extend(part.applications);
        }
        rt.backend.observation = Some(observation);
        run(rt, action).unwrap();
    };
    for reported_after_summon in [20, 30] {
        let backend = FakeBackend {
            frames: HashMap::from([(w(10), frame(50)), (w(20), frame(500)), (w(30), frame(200))]),
            focused: Some(w(10)),
            frontmost: Some(w(10).app),
            ..FakeBackend::default()
        };
        let mut rt = runtime_with_config(backend, lab());
        bind(&mut rt, WorkspaceId::ONE, &w(10));
        bind(&mut rt, WorkspaceId::ONE, &w(20));
        bind(&mut rt, WorkspaceId::TWO, &w(30));
        act(&mut rt, WorkerAction::SwitchWorkspace(WorkspaceId::TWO));
        report(&mut rt, 30);
        act(&mut rt, WorkerAction::Scratchpad(ScratchpadCommand::Mark));
        act(&mut rt, WorkerAction::SwitchWorkspace(WorkspaceId::ONE));
        report(&mut rt, 10);
        // 10 -> 20, AX lags at 10, summon 30, AX catches up only to intermediate 20 before Mark.
        act(&mut rt, WorkerAction::Focus(Direction::Right));
        report(&mut rt, 10);
        act(&mut rt, WorkerAction::Scratchpad(ScratchpadCommand::Toggle));
        report(&mut rt, reported_after_summon);
        act(&mut rt, WorkerAction::Scratchpad(ScratchpadCommand::Mark));
        report(&mut rt, 30);
        act(&mut rt, WorkerAction::Retile);
        assert!(!pad(&rt).contains(&w(30)));
        assert!(!pad(&rt).contains(&w(20)));
        assert_eq!(rt.workspaces.workspace_of(&w(30)), Some(WorkspaceId::ONE));
        assert_eq!(rt.workspaces.workspace_of(&w(20)), Some(WorkspaceId::ONE));
        assert!(rt.parked.is_empty());
    }
}

#[test]
fn settled_scratchpad_summon_confirms_translated_target_after_display_removal() {
    let w = window(10);
    let other = window(20);
    let backend = FakeBackend {
        frames: HashMap::from([(w.clone(), frame(500)), (other.clone(), frame(1050))]),
        focused: Some(w.clone()),
        frontmost: Some(w.app.clone()),
        observation: Some(observation_for_windows(
            "com.example.Test",
            &w,
            &[(w.clone(), frame(500)), (other.clone(), frame(1050))],
        )),
        ..FakeBackend::default()
    };
    let mut rt = runtime_with_config(backend, lab());
    bind(&mut rt, WorkspaceId::ONE, &w);
    bind(&mut rt, WorkspaceId::SIX, &other);
    let cancelled = AtomicBool::new(false);
    let exec = |rt: &mut WorkspaceRuntime<FakeBackend>, action, out| {
        let Some(WorkerResult::Status { result, .. }) =
            rt.execute(WorkerCommand::Action(action, out), &cancelled)
        else {
            panic!("expected status");
        };
        result.unwrap();
    };
    exec(&mut rt, WorkerAction::Retile, outputs());
    exec(
        &mut rt,
        WorkerAction::Scratchpad(ScratchpadCommand::Mark),
        outputs(),
    );
    let parked = rt.backend.frames[&w];
    rt.backend.server_frames.insert(w.clone(), parked);
    rt.backend.frozen_server_frames = true;
    rt.backend.focused = Some(other.clone());
    rt.backend.frontmost = Some(other.app.clone());
    exec(
        &mut rt,
        WorkerAction::Scratchpad(ScratchpadCommand::Toggle),
        outputs(),
    );
    let target = rt.parked.summon_destination(&w).expect("delayed summon");

    // Secondary display disconnects and the window settles at the translated frame on main.
    rt.backend.frozen_server_frames = false;
    let main_only = macos::DisplayOutputs::new(vec![outputs().main().clone()]).unwrap();
    let settled = crate::Rect {
        x: 250,
        y: 100,
        ..target
    };
    rt.backend.frames.insert(w.clone(), settled);
    rt.backend.server_frames.insert(w.clone(), settled);
    rt.backend.advance(Duration::from_secs(5));
    exec(&mut rt, WorkerAction::Retile, main_only.clone());
    assert_eq!(rt.parked.summon_destination(&w), None);
    assert!(!rt.parked.contains_key(&w));

    // A subsequent drag and pause both preserve the dragged frame.
    let dragged = crate::Rect {
        x: 100,
        y: 150,
        ..target
    };
    rt.backend.frames.insert(w.clone(), dragged);
    rt.backend.server_frames.insert(w.clone(), dragged);
    rt.backend.focused = Some(w.clone());
    rt.backend.frontmost = Some(w.app.clone());
    let gesture = rt.execute(
        WorkerCommand::Gesture(
            FinalGesture::drop(w.clone(), (150, 200)),
            false,
            main_only.clone(),
        ),
        &cancelled,
    );
    assert!(matches!(
        gesture,
        Some(WorkerResult::Status { result: Ok(_), .. })
    ));
    exec(&mut rt, WorkerAction::Retile, main_only.clone());
    assert_eq!(rt.backend.frames[&w], dragged);
    let restored = rt.execute(WorkerCommand::RestoreAll(Some(main_only)), &cancelled);
    assert!(matches!(
        restored,
        Some(WorkerResult::Restored { failed: 0 })
    ));
    assert_eq!(rt.backend.frames[&w], dragged);
}

#[test]
fn moving_unconfirmed_scratchpad_float_preserves_summoned_position() {
    for target in [WorkspaceId::SIX, WorkspaceId::new(7).unwrap()] {
        for arrived in [true, false] {
            let mut rt = scratchpad_runtime(lab());
            let w = window(10);
            scratchpad(&mut rt, ScratchpadCommand::Mark).unwrap();
            rt.backend
                .server_frames
                .insert(w.clone(), rt.backend.frames[&w]);
            rt.backend.frozen_server_frames = true;
            assert_eq!(
                scratchpad(&mut rt, ScratchpadCommand::Toggle)
                    .unwrap()
                    .failed,
                0
            );
            let summoned = rt.backend.frames[&w];
            assert!(rt.parked.summon_destination(&w).is_some());
            rt.backend.frozen_server_frames = false;
            if arrived {
                rt.backend.server_frames.insert(w.clone(), summoned);
            }
            assert_eq!(
                run(&mut rt, WorkerAction::MoveFocused(target))
                    .unwrap()
                    .failed,
                0
            );
            assert!(!pad(&rt).contains(&w));
            assert_eq!(rt.workspaces.floating_workspace_of(&w), Some(target));
            if !rt.is_visible(target, &outputs()) {
                assert_eq!(
                    run(&mut rt, WorkerAction::SwitchWorkspace(target))
                        .unwrap()
                        .failed,
                    0
                );
            }
            rt.backend.advance(Duration::from_secs(5));
            for _ in 0..2 {
                assert_eq!(run(&mut rt, WorkerAction::Retile).unwrap().failed, 0);
            }
            let expected = super::super::move_workspace::translate_frame_to_output(
                summoned,
                outputs().main().frame,
                outputs().secondary().unwrap().frame,
            );
            assert_eq!(rt.backend.frames[&w], expected);
            assert!(!rt.parked.contains_key(&w));
            assert!(!rt.backend.persisted.contains_key(&w));
        }
    }
}
