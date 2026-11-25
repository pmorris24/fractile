//! Moving windows to a workspace on another display (Option+Shift+number).
use super::*;

/// Where a tiled window on Main's only tile lands.
fn main_tile() -> crate::Rect {
    outputs().main().frame
}

fn at(x: i32) -> crate::Rect {
    crate::Rect { x, ..frame(0) }
}

#[test]
fn moving_a_window_to_the_workspace_on_screen_on_another_display_tiles_it_there() {
    let window = window(20);
    let mut backend = FakeBackend {
        focused: Some(window.clone()),
        ..FakeBackend::default()
    };
    backend.frames.insert(window.clone(), frame(1050));
    let mut runtime = runtime(backend);
    bind(&mut runtime, WorkspaceId::SIX, &window);
    runtime.record_focused_workspace(WorkspaceId::SIX);

    runtime
        .move_focused(WorkspaceId::ONE, outputs(), &AtomicBool::new(false))
        .unwrap();

    assert_eq!(
        runtime.workspaces.workspace_of(&window),
        Some(WorkspaceId::ONE)
    );
    assert_eq!(runtime.backend.frames[&window], main_tile());
    assert!(!runtime.parked.contains_key(&window));
    assert_eq!(runtime.backend.focused, Some(window));
    // Focus followed the window to Main, so Option+Tab goes back to Secondary.
    assert_eq!(runtime.focus.history().current, Some(WorkspaceId::ONE));
    assert_eq!(runtime.focus.history().previous, Some(WorkspaceId::SIX));
}

#[test]
fn moving_a_window_to_a_hidden_workspace_on_another_display_parks_it_where_it_is() {
    let window = window(20);
    let mut backend = FakeBackend {
        focused: Some(window.clone()),
        // macOS clamps a write off Main's edge back onto Secondary, where
        // the window is. Moving it used to fail outright.
        refuses_position: Some(|target| target.x < 0),
        ..FakeBackend::default()
    };
    backend.frames.insert(window.clone(), frame(1050));
    let mut runtime = runtime(backend);
    bind(&mut runtime, WorkspaceId::SIX, &window);
    let target = WorkspaceId::new(2).unwrap();
    let cancelled = AtomicBool::new(false);

    runtime.move_focused(target, outputs(), &cancelled).unwrap();

    assert_eq!(runtime.workspaces.workspace_of(&window), Some(target));
    let displays = outputs().frames().collect::<Vec<_>>();
    let secondary = outputs().displays()[1].frame;
    assert_eq!(
        runtime.backend.frames[&window],
        crate::geometry::parked_frame(secondary, &displays, frame(1050))
    );
    // Recovery brings it back on Main, where its workspace lives.
    assert_eq!(runtime.parked[&window], frame(50));
    assert_eq!(runtime.backend.persisted[&window], frame(50));

    runtime.switch(target, outputs(), &cancelled).unwrap();

    assert_eq!(runtime.backend.frames[&window], main_tile());
    assert!(!runtime.parked.contains_key(&window));
    assert!(runtime.backend.persisted.is_empty());
}

#[test]
fn a_cross_display_move_that_cannot_park_leaves_the_window_where_it_was() {
    let window = window(20);
    let mut backend = FakeBackend {
        focused: Some(window.clone()),
        refuses_position: Some(|target| target.x != 1050),
        ..FakeBackend::default()
    };
    backend.frames.insert(window.clone(), frame(1050));
    let mut runtime = runtime(backend);
    bind(&mut runtime, WorkspaceId::SIX, &window);

    let Err(error) = runtime.move_focused(
        WorkspaceId::new(2).unwrap(),
        outputs(),
        &AtomicBool::new(false),
    ) else {
        panic!("a window that could not be parked was moved");
    };

    assert_eq!(error, "Window could not be parked in the target workspace");
    assert_eq!(
        runtime.workspaces.workspace_of(&window),
        Some(WorkspaceId::SIX)
    );
    assert_eq!(runtime.backend.frames[&window], frame(1050));
    assert!(runtime.parked.is_empty());
    assert!(runtime.backend.persisted.is_empty());
}

#[test]
fn moving_a_floating_window_to_the_workspace_on_screen_on_another_display_carries_it_there() {
    let floating = window(20);
    let mut backend = FakeBackend {
        focused: Some(floating.clone()),
        ..FakeBackend::default()
    };
    backend.frames.insert(floating.clone(), at(1100));
    let mut runtime = runtime(backend);
    runtime
        .workspaces
        .own_floating(floating.clone(), WorkspaceId::SIX);

    let status = runtime
        .move_focused(WorkspaceId::ONE, outputs(), &AtomicBool::new(false))
        .unwrap();

    assert_eq!(status.failed, 0);
    assert_eq!(
        runtime.workspaces.floating_workspace_of(&floating),
        Some(WorkspaceId::ONE)
    );
    // Same size, same place relative to the display, now on Main.
    assert_eq!(runtime.backend.frames[&floating], at(100));
}

#[test]
fn a_floating_window_moved_to_a_hidden_workspace_on_another_display_shows_up_there() {
    let floating = window(20);
    let mut backend = FakeBackend {
        focused: Some(floating.clone()),
        ..FakeBackend::default()
    };
    backend.frames.insert(floating.clone(), at(1100));
    let mut runtime = runtime(backend);
    runtime
        .workspaces
        .own_floating(floating.clone(), WorkspaceId::SIX);
    let target = WorkspaceId::new(2).unwrap();
    let cancelled = AtomicBool::new(false);

    runtime.move_focused(target, outputs(), &cancelled).unwrap();
    assert!(runtime.parked.contains_key(&floating));

    runtime.switch(target, outputs(), &cancelled).unwrap();

    assert_eq!(runtime.backend.frames[&floating], at(100));
    assert!(!runtime.parked.contains_key(&floating));
}

/// #121: move-app moves every managed window of the app, and leaves other
/// apps' windows where they are.
#[test]
fn move_app_moves_every_managed_window_of_the_app() {
    let first = window(10);
    let second = window(11);
    let other = crate::PhysicalWindowId {
        app: AppInstanceId {
            pid: 3,
            launched_at: 3,
        },
        window_id: 20,
    };
    let mut backend = FakeBackend {
        focused: Some(first.clone()),
        ..FakeBackend::default()
    };
    backend.frames.insert(first.clone(), frame(50));
    backend.frames.insert(second.clone(), frame(300));
    backend.frames.insert(other.clone(), frame(500));
    let mut runtime = runtime(backend);
    bind(&mut runtime, WorkspaceId::ONE, &first);
    bind(&mut runtime, WorkspaceId::ONE, &second);
    bind(&mut runtime, WorkspaceId::ONE, &other);
    runtime
        .memory
        .bundles
        .insert(first.app.clone(), "com.example.Test".into());
    runtime
        .memory
        .bundles
        .insert(other.app.clone(), "com.example.Other".into());

    let status = runtime
        .move_app(
            "com.example.Test",
            WorkspaceId::SIX,
            outputs(),
            &AtomicBool::new(false),
        )
        .unwrap();

    assert_eq!(status.failed, 0);
    assert_eq!(
        runtime.workspaces.workspace_of(&first),
        Some(WorkspaceId::SIX)
    );
    assert_eq!(
        runtime.workspaces.workspace_of(&second),
        Some(WorkspaceId::SIX)
    );
    assert_eq!(
        runtime.workspaces.workspace_of(&other),
        Some(WorkspaceId::ONE)
    );
}

/// #136: move-app takes the app's floating windows along with its tiles, and
/// finds an app whose only window floats.
#[test]
fn move_app_moves_floating_windows_too() {
    let (tile, floating) = (window(10), window(11));
    let mut backend = FakeBackend::default();
    backend.frames.insert(tile.clone(), frame(50));
    backend.frames.insert(floating.clone(), frame(500));
    let mut runtime = runtime(backend);
    bind(&mut runtime, WorkspaceId::ONE, &tile);
    runtime
        .workspaces
        .own_floating(floating.clone(), WorkspaceId::ONE);
    runtime
        .memory
        .bundles
        .insert(tile.app.clone(), "com.example.Test".into());
    let cancelled = AtomicBool::new(false);

    runtime
        .move_app("com.example.Test", WorkspaceId::SIX, outputs(), &cancelled)
        .unwrap();
    assert_eq!(
        runtime.workspaces.workspace_of(&tile),
        Some(WorkspaceId::SIX)
    );
    assert_eq!(
        runtime.workspaces.floating_workspace_of(&floating),
        Some(WorkspaceId::SIX)
    );

    // Only the float left: it is still the app's window to move.
    runtime.forget_window(WorkspaceId::SIX, &tile);
    runtime
        .move_app("com.example.Test", WorkspaceId::ONE, outputs(), &cancelled)
        .unwrap();
    assert_eq!(
        runtime.workspaces.floating_workspace_of(&floating),
        Some(WorkspaceId::ONE)
    );
}

#[test]
fn cross_display_route_and_recall_park_off_source_display_when_target_edge_is_refused() {
    use crate::window_memory::{RememberedWindow, WindowMemory};

    let target = WorkspaceId::new(2).unwrap();
    for recall in [false, true] {
        let id = window(20);
        let backend = FakeBackend {
            frames: [(id.clone(), frame(1050))].into_iter().collect(),
            observation: Some(observation(&id, frame(1050))),
            focused: Some(id.clone()),
            refuses_position: Some(|rect: crate::Rect| rect.x < 0),
            ..FakeBackend::default()
        };
        let config = if recall {
            Config::default()
        } else {
            Config::parse(
                "version = 1
[[rules]]
bundle_id = 'com.example.Test'
action = 'route'
workspace = 2",
            )
            .unwrap()
        };
        let mut rt = runtime_with_config(backend, config);
        if recall {
            let mut memory = WindowMemory::default();
            memory.record(
                "com.example.Test",
                vec![RememberedWindow {
                    workspace: target,
                    frame: frame(1050),
                    floating: false,
                    document: None,
                    title: None,
                }],
                1,
            );
            rt.restore_window_memory(memory);
        }

        let status = rt.cycle(outputs(), &AtomicBool::new(false)).unwrap();
        assert_eq!(
            status.failed, 0,
            "recall={recall}: parking should succeed off source display"
        );
        assert_eq!(
            rt.workspaces.workspace_of(&id),
            Some(target),
            "recall={recall}: window should transfer to hidden target workspace"
        );
        assert!(
            rt.parked.contains_key(&id),
            "recall={recall}: recovery frame must be persisted"
        );
        assert!(
            !crate::geometry::shows_on(
                rt.backend.frames[&id],
                &outputs().frames().collect::<Vec<_>>(),
            ),
            "recall={recall}: window should be parked off-screen"
        );
    }
}

/// #148: a floating window dragged onto another display parks off that
/// display when leaving its workspace, keeping its exact recovery frame.
#[test]
fn switching_away_parks_a_floating_window_dragged_to_another_display_off_that_display() {
    for refuse in [true, false] {
        let floating = window(20);
        let config = Config::parse(
            "version = 1
[[rules]]
bundle_id = 'com.example.Test'
action = 'float'",
        )
        .unwrap();
        let backend = FakeBackend {
            frames: [(floating.clone(), frame(50))].into_iter().collect(),
            observation: Some(observation(&floating, frame(50))),
            refuses_position: refuse.then_some(|target: crate::Rect| target.x < 0),
            ..FakeBackend::default()
        };
        let mut rt = runtime_with_config(backend, config);
        let cancelled = AtomicBool::new(false);
        rt.cycle(outputs(), &cancelled).unwrap();
        assert_eq!(
            rt.workspaces.floating_workspace_of(&floating),
            Some(WorkspaceId::ONE)
        );

        rt.backend.frames.insert(floating.clone(), frame(1050));
        rt.backend.observation = Some(observation(&floating, frame(1050)));
        rt.drop_window(floating.clone(), (1250, 30), false, outputs(), &cancelled)
            .unwrap();
        assert_eq!(
            rt.workspaces.floating_workspace_of(&floating),
            Some(WorkspaceId::ONE)
        );
        rt.backend.calls.clear();

        let two = WorkspaceId::new(2).unwrap();
        rt.switch(two, outputs(), &cancelled)
            .expect("workspace switch should park its dragged floating window");
        let displays = outputs().frames().collect::<Vec<_>>();
        let secondary = outputs().displays()[1].frame;
        assert_eq!(
            rt.backend.frames[&floating],
            crate::geometry::parked_frame(secondary, &displays, frame(1050))
        );
        assert!(!crate::geometry::shows_on(
            rt.backend.frames[&floating],
            &displays
        ));
        assert_eq!(rt.parked[&floating], frame(1050));
        assert_eq!(rt.backend.persisted[&floating], frame(1050));

        rt.switch(WorkspaceId::ONE, outputs(), &cancelled).unwrap();
        assert_eq!(rt.backend.frames[&floating], frame(1050));
        assert!(!rt.parked.contains_key(&floating));
    }
}

/// #148: moving a workspace-1-owned float dragged onto Secondary to hidden
/// workspace 2 on Main parks off Secondary and relocates recovery onto Main.
#[test]
fn moving_a_dragged_floating_window_to_a_hidden_workspace_on_main_parks_off_its_actual_display() {
    let floating = window(20);
    let config = Config::parse(
        "version = 1
[[rules]]
bundle_id = 'com.example.Test'
action = 'float'",
    )
    .unwrap();
    let backend = FakeBackend {
        focused: Some(floating.clone()),
        frames: [(floating.clone(), frame(50))].into_iter().collect(),
        observation: Some(observation(&floating, frame(50))),
        refuses_position: Some(|target: crate::Rect| target.x < 0),
        ..FakeBackend::default()
    };
    let mut rt = runtime_with_config(backend, config);
    let cancelled = AtomicBool::new(false);
    rt.cycle(outputs(), &cancelled).unwrap();
    rt.backend.frames.insert(floating.clone(), frame(1050));
    rt.backend.observation = Some(observation(&floating, frame(1050)));
    rt.drop_window(floating.clone(), (1250, 30), false, outputs(), &cancelled)
        .unwrap();

    let two = WorkspaceId::new(2).unwrap();
    rt.move_focused(two, outputs(), &cancelled).unwrap();
    let displays = outputs().frames().collect::<Vec<_>>();
    let secondary = outputs().displays()[1].frame;
    assert_eq!(
        rt.backend.frames[&floating],
        crate::geometry::parked_frame(secondary, &displays, frame(1050))
    );
    assert_eq!(rt.parked[&floating], frame(50));

    rt.switch(two, outputs(), &cancelled).unwrap();
    assert_eq!(rt.backend.frames[&floating], frame(50));
    assert!(!rt.parked.contains_key(&floating));
}

/// #152: moving a floating window dragged to a third display translates its
/// recovery frame (and direct carry) from the display it actually occupies.
#[test]
fn moving_a_floating_window_dragged_to_a_third_display_restores_it_on_the_target_display() {
    let setup = |x: i32| {
        let id = window(20);
        let initial = frame(1100);
        let mut observed = observation(&id, initial);
        if let crate::AppObservation::Complete { windows, .. } = &mut observed.applications[0] {
            windows[0].resizable = false;
        }
        let backend = FakeBackend {
            focused: Some(id.clone()),
            frontmost: Some(id.app.clone()),
            frames: HashMap::from([(id.clone(), initial)]),
            observation: Some(observed),
            refuses_position: Some(|target: crate::Rect| target.x < 0),
            ..FakeBackend::default()
        };
        let mut rt = runtime(backend);
        let cancelled = AtomicBool::new(false);
        rt.cycle(three_outputs(), &cancelled).unwrap();
        assert_eq!(
            rt.workspaces.floating_workspace_of(&id),
            Some(WorkspaceId::FIVE)
        );
        rt.backend.frames.insert(id.clone(), frame(x));
        rt.cycle(three_outputs(), &cancelled).unwrap();
        assert_eq!(
            rt.workspaces.floating_workspace_of(&id),
            Some(WorkspaceId::FIVE)
        );
        assert_eq!(rt.backend.frames[&id], frame(x));
        (rt, id)
    };

    let cancelled = AtomicBool::new(false);
    let target = WorkspaceId::new(2).unwrap();

    // Direct cross-display move from the 3rd display to hidden workspace 2 on Main.
    let (mut rt, id) = setup(2500);
    assert_eq!(
        rt.move_focused(target, three_outputs(), &cancelled)
            .unwrap()
            .failed,
        0
    );
    assert_eq!(rt.workspaces.floating_workspace_of(&id), Some(target));
    let displays = three_outputs().frames().collect::<Vec<_>>();
    let third = three_outputs().displays()[2].frame;
    assert_eq!(
        rt.backend.frames[&id],
        crate::geometry::parked_frame(third, &displays, frame(2500))
    );
    assert_eq!(rt.parked[&id], frame(500));
    assert_eq!(rt.backend.persisted[&id], frame(500));

    assert_eq!(
        rt.switch(target, three_outputs(), &cancelled)
            .unwrap()
            .failed,
        0
    );
    assert_eq!(rt.backend.frames[&id], frame(500));
    assert!(!rt.parked.contains_key(&id));
    assert!(rt.backend.persisted.is_empty());
    rt.cycle(three_outputs(), &cancelled).unwrap();
    assert_eq!(rt.backend.frames[&id], frame(500));
    assert_eq!(rt.workspaces.floating_workspace_of(&id), Some(target));

    // Moving to visible workspace 1 on Main also translates from the 3rd display.
    let (mut visible_rt, visible_id) = setup(2500);
    assert_eq!(
        visible_rt
            .move_focused(WorkspaceId::ONE, three_outputs(), &cancelled)
            .unwrap()
            .failed,
        0
    );
    assert_eq!(visible_rt.backend.frames[&visible_id], frame(500));
}

#[test]
fn moving_floating_window_back_before_cross_display_park_settles_restores_on_main() {
    let w = window(10);
    let backend = FakeBackend {
        frames: HashMap::from([(w.clone(), frame(50))]),
        server_frames: HashMap::from([(w.clone(), frame(50))]),
        focused: Some(w.clone()),
        frontmost: Some(w.app.clone()),
        observation: Some(observation(&w, frame(50))),
        ..FakeBackend::default()
    };
    let mut rt = runtime(backend);
    bind(&mut rt, WorkspaceId::ONE, &w);
    rt.memory
        .bundles
        .insert(w.app.clone(), "com.example.Test".into());
    let run_action = |rt: &mut WorkspaceRuntime<FakeBackend>, action: WorkerAction| {
        let Some(WorkerResult::Status { result, .. }) = rt.execute(
            WorkerCommand::Action(action, outputs()),
            &AtomicBool::new(false),
        ) else {
            panic!("missing action status");
        };
        assert_eq!(result.unwrap().failed, 0);
    };
    run_action(&mut rt, WorkerAction::ToggleFloat);
    assert_eq!(
        rt.workspaces.floating_workspace_of(&w),
        Some(WorkspaceId::ONE)
    );
    rt.backend.frozen_server_frames = true;

    // Move to hidden workspace 7 on Secondary while park stays in flight.
    run_action(
        &mut rt,
        WorkerAction::MoveApp("com.example.Test".into(), WorkspaceId::new(7).unwrap()),
    );
    let park = rt.backend.frames[&w];
    assert_eq!(rt.parked[&w], frame(1050));
    rt.backend.frames.insert(w.clone(), frame(50));
    rt.backend.focused = None;

    // Move back to visible workspace 1 on Main before the park write settles.
    run_action(
        &mut rt,
        WorkerAction::MoveApp("com.example.Test".into(), WorkspaceId::ONE),
    );
    let newer = rt.backend.frames[&w];
    assert_eq!(newer, frame(50));

    // Delayed park lands first, then the newer restore supersedes it on Main.
    rt.backend.frozen_server_frames = false;
    rt.backend.server_frames.insert(w.clone(), park);
    rt.backend.server_frames.insert(w.clone(), newer);
    rt.backend.advance(Duration::from_secs(3));
    rt.backend.observation = Some(observation(&w, newer));
    assert_eq!(
        rt.cycle(outputs(), &AtomicBool::new(false)).unwrap().failed,
        0
    );
    assert_eq!(
        rt.workspaces.floating_workspace_of(&w),
        Some(WorkspaceId::ONE)
    );
    assert!(!rt.parked.contains_key(&w));
    assert_eq!(rt.backend.server_frames[&w], frame(50));
    assert_eq!(rt.backend.frames[&w], frame(50));
}

#[test]
fn moving_workspace_relocates_pending_parked_float_onto_target_display() {
    for temporarily_unavailable in [true, false] {
        let w = window(10);
        let backend = FakeBackend {
            focused: Some(w.clone()),
            frontmost: Some(w.app.clone()),
            frames: HashMap::from([(w.clone(), frame(50))]),
            observation: Some(observation(&w, frame(50))),
            ..FakeBackend::default()
        };
        let config = Config::parse(
            "version = 1\n[[rules]]\nbundle_id = 'com.example.Test'\naction = 'float'",
        )
        .unwrap();
        let mut rt = runtime_with_config(backend, config);
        let act = |rt: &mut WorkspaceRuntime<FakeBackend>, action: WorkerAction| {
            let Some(WorkerResult::Status { result, .. }) = rt.execute(
                WorkerCommand::Action(action, outputs()),
                &AtomicBool::new(false),
            ) else {
                panic!("expected status");
            };
            result.unwrap()
        };
        assert_eq!(act(&mut rt, WorkerAction::Retile).failed, 0);
        assert_eq!(
            act(
                &mut rt,
                WorkerAction::MoveApp("com.example.Test".into(), WorkspaceId::TWO)
            )
            .failed,
            0
        );
        rt.backend.focused = None;
        rt.backend.position_outcome =
            temporarily_unavailable.then_some(macos::ApplyOutcome::Unavailable);
        assert_eq!(
            act(
                &mut rt,
                WorkerAction::MoveApp("com.example.Test".into(), WorkspaceId::ONE)
            )
            .failed,
            usize::from(temporarily_unavailable)
        );
        rt.backend.position_outcome = None;
        assert_eq!(
            act(
                &mut rt,
                WorkerAction::MoveWorkspaceToDisplay(DisplayTarget::Next)
            )
            .failed,
            0
        );
        assert_eq!(
            rt.workspaces.assignment(WorkspaceId::ONE),
            &DisplaySelector::Secondary
        );
        rt.backend.advance(Duration::from_secs(3));
        for _ in 0..2 {
            rt.backend.observation = Some(observation(&w, rt.backend.frames[&w]));
            assert_eq!(act(&mut rt, WorkerAction::Retile).failed, 0);
        }
        assert!(!rt.parked.contains_key(&w));
        assert!(!rt.backend.persisted.contains_key(&w));
        assert_eq!(rt.backend.frames[&w], frame(1050));
    }
}

#[test]
fn display_pointer_follow_uses_dragged_floats_actual_display() {
    let warps = |rt: &WorkspaceRuntime<FakeBackend>| -> Vec<(i32, i32)> {
        rt.backend
            .calls
            .iter()
            .filter_map(|call| match call {
                BackendCall::WarpPointer(point) => Some(*point),
                _ => None,
            })
            .collect()
    };
    let setup = |pointer: (i32, i32)| {
        let windows = [
            (window(10), frame(50)),
            (window(20), frame(500)),
            (window(30), frame(700)),
        ];
        let backend = FakeBackend {
            frames: windows.iter().cloned().collect(),
            focused: Some(window(30)),
            frontmost: Some(window(30).app),
            pointer: Some(pointer),
            observation: Some(observation_for_windows(
                "com.example.Test",
                &window(30),
                &windows,
            )),
            ..FakeBackend::default()
        };
        let config =
            Config::parse("version = 1\n[settings]\nmouse_follows_focus = \"display\"\n").unwrap();
        let mut rt = runtime_with_config(backend, config);
        let run = |rt: &mut WorkspaceRuntime<FakeBackend>, cmd: WorkerCommand| {
            let Some(WorkerResult::Status { result, .. }) =
                rt.execute(cmd, &AtomicBool::new(false))
            else {
                panic!("expected status");
            };
            result.unwrap();
        };
        run(
            &mut rt,
            WorkerCommand::Action(WorkerAction::Retile, outputs()),
        );
        run(
            &mut rt,
            WorkerCommand::Action(WorkerAction::ToggleFloat, outputs()),
        );
        rt.backend.frames.insert(window(30), frame(1050));
        run(
            &mut rt,
            WorkerCommand::Gesture(FinalGesture::drop(window(30), (1250, 30)), false, outputs()),
        );
        rt.backend.calls.clear();
        rt
    };

    // Pointer on Main, cycle onto dragged float on Secondary -> warps to float center.
    let mut rt = setup((100, 100));
    rt.backend.focused = Some(window(20));
    rt.execute(
        WorkerCommand::Action(WorkerAction::CycleFocus, outputs()),
        &AtomicBool::new(false),
    );
    assert_eq!(warps(&rt), vec![(1250, 170)]);

    // Pointer already on Secondary -> does not warp when cycling onto dragged float.
    let mut rt = setup((1500, 300));
    rt.backend.focused = Some(window(20));
    rt.execute(
        WorkerCommand::Action(WorkerAction::CycleFocus, outputs()),
        &AtomicBool::new(false),
    );
    assert!(warps(&rt).is_empty());

    // Switching away and back to workspace 1 (remembering the float on Secondary) warps from Main.
    let mut rt = setup((100, 100));
    rt.execute(
        WorkerCommand::Action(WorkerAction::SwitchWorkspace(WorkspaceId::TWO), outputs()),
        &AtomicBool::new(false),
    );
    rt.backend.calls.clear();
    rt.execute(
        WorkerCommand::Action(WorkerAction::SwitchWorkspace(WorkspaceId::ONE), outputs()),
        &AtomicBool::new(false),
    );
    assert_eq!(warps(&rt), vec![(1250, 170)]);
}

#[test]
fn move_app_retires_confirmed_closed_hidden_window_and_moves_live_siblings() {
    let third = WorkspaceId::new(3).unwrap();
    let other_app_win = crate::PhysicalWindowId {
        app: AppInstanceId {
            pid: 2,
            launched_at: 2,
        },
        window_id: 90,
    };
    let make_fixture = || {
        let windows = [
            (window(10), frame(50)),
            (window(20), frame(350)),
            (window(30), frame(650)),
        ];
        let mut obs = observation_for_windows("com.example.Test", &window(10), &windows);
        obs.applications
            .extend(observation_for("com.example.Other", &other_app_win, frame(50)).applications);
        let backend = FakeBackend {
            frames: windows
                .into_iter()
                .chain([(other_app_win.clone(), frame(50))])
                .collect(),
            focused: Some(other_app_win.clone()),
            frontmost: Some(other_app_win.app.clone()),
            observation: Some(obs),
            running_apps: Some([other_app_win.app.clone(), window(10).app].into()),
            ..FakeBackend::default()
        };
        let mut rt = runtime(backend);
        bind(&mut rt, WorkspaceId::ONE, &other_app_win);
        for id in [10, 20, 30] {
            bind(&mut rt, WorkspaceId::TWO, &window(id));
        }
        rt.switch(WorkspaceId::TWO, outputs(), &AtomicBool::new(false))
            .unwrap();
        rt.switch(WorkspaceId::ONE, outputs(), &AtomicBool::new(false))
            .unwrap();
        rt
    };

    // Closed window in first, middle, or last position is retired while live siblings move to 3.
    for closed_id in [10, 20, 30] {
        let mut rt = make_fixture();
        rt.backend.frames.remove(&window(closed_id));
        for _ in 0..3 {
            rt.cycle(outputs(), &AtomicBool::new(false)).unwrap();
        }
        let status = rt
            .move_app(
                "com.example.Test",
                third,
                outputs(),
                &AtomicBool::new(false),
            )
            .expect("closed hidden window must not abort moving live windows");
        assert_eq!(status.failed, 0);
        assert!(status.hud.is_some());
        assert_eq!(rt.workspaces.workspace_of(&window(closed_id)), None);
        assert!(!rt.parked.contains_key(&window(closed_id)));
        assert!(!rt.backend.persisted.contains_key(&window(closed_id)));
        for live_id in [10, 20, 30].into_iter().filter(|id| *id != closed_id) {
            assert_eq!(rt.workspaces.workspace_of(&window(live_id)), Some(third));
            assert!(rt.parked.contains_key(&window(live_id)));
        }
    }

    // If every retained binding of the app is confirmed closed, retire them and report none found.
    let mut all_closed = make_fixture();
    for id in [10, 20, 30] {
        all_closed.backend.frames.remove(&window(id));
    }
    let err = all_closed
        .move_app(
            "com.example.Test",
            third,
            outputs(),
            &AtomicBool::new(false),
        )
        .err()
        .unwrap();
    assert_eq!(err, "No managed window found for com.example.Test");
    for id in [10, 20, 30] {
        assert_eq!(all_closed.workspaces.workspace_of(&window(id)), None);
        assert!(!all_closed.parked.contains_key(&window(id)));
    }

    // Control: transient AX Gone while the window server still lists the window must NOT retire it.
    let mut transient = make_fixture();
    transient
        .backend
        .server_frames
        .insert(window(10), transient.backend.frames[&window(10)]);
    transient.backend.frames.remove(&window(10));
    let err = transient
        .move_app(
            "com.example.Test",
            third,
            outputs(),
            &AtomicBool::new(false),
        )
        .err()
        .unwrap();
    assert_eq!(err, "Window could not be parked in the target workspace");
    assert_eq!(
        transient.workspaces.workspace_of(&window(10)),
        Some(WorkspaceId::TWO)
    );
    assert!(transient.parked.contains_key(&window(10)));
}
