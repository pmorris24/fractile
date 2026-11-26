use super::*;

#[test]
fn inactive_windows_are_positioned_offscreen_without_a_minimize_operation() {
    let window = window(10);
    let normal = frame(50);
    let mut backend = FakeBackend::default();
    backend.frames.insert(window.clone(), normal);
    let mut runtime = runtime(backend);
    bind(&mut runtime, WorkspaceId::new(2).unwrap(), &window);

    let mut failed = 0;
    let outputs = outputs();
    let visible = runtime.visible_workspaces(&outputs);
    let server_frames = runtime.backend.window_server_frames().unwrap();
    runtime.park_inactive(
        &outputs,
        &visible,
        &server_frames,
        &AtomicBool::new(false),
        &mut failed,
    );

    let displays = outputs.frames().collect::<Vec<_>>();
    let expected = crate::geometry::parked_frame(outputs.main().frame, &displays, normal);
    assert_eq!(failed, 0);
    assert_eq!(runtime.backend.frames[&window], expected);
    assert_eq!(runtime.backend.persisted[&window], normal);
    assert!(matches!(runtime.backend.calls[0], BackendCall::Persist));
    assert!(matches!(
        runtime.backend.calls[1],
        BackendCall::SetPosition(_, target) if target == expected
    ));
    assert_eq!(runtime.backend.parked_writes, vec![window]);
}

#[test]
fn parked_window_observed_back_on_screen_re_homes_and_drops_recovery_frame() {
    let tiled = window(10);
    let overlay = window(11);
    let mut observation = observation_for_windows(
        "com.example.Test",
        &tiled,
        &[(tiled.clone(), frame(50)), (overlay.clone(), frame(60))],
    );
    if let crate::AppObservation::Complete { windows, .. } = &mut observation.applications[0] {
        windows
            .iter_mut()
            .find(|observed| observed.id == overlay)
            .unwrap()
            .resizable = false;
    }
    let mut backend = FakeBackend {
        observation: Some(observation),
        frontmost: Some(overlay.app.clone()),
        ..FakeBackend::default()
    };
    backend.frames.insert(tiled.clone(), frame(50));
    backend.frames.insert(overlay.clone(), frame(60));
    let mut runtime = runtime(backend);
    bind(&mut runtime, WorkspaceId::ONE, &tiled);
    let hidden = WorkspaceId::new(2).unwrap();
    runtime.workspaces.own_floating(overlay.clone(), hidden);
    runtime.parked.insert(overlay.clone(), frame(70), None);

    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    assert!(!runtime.parked.contains_key(&overlay));
    assert!(!runtime.backend.persisted.contains_key(&overlay));
    assert_eq!(
        runtime.workspaces.floating_workspace_of(&overlay),
        Some(WorkspaceId::ONE)
    );
    assert!(
        outputs()
            .union()
            .contains_center(runtime.backend.frames[&overlay])
    );
}

#[test]
fn parked_tiled_window_observed_back_on_screen_re_homes_to_the_visible_workspace() {
    let existing = window(10);
    let returned = window(11);
    let mut backend = FakeBackend {
        observation: Some(observation_for_windows(
            "com.example.Test",
            &existing,
            &[
                (existing.clone(), frame(50)),
                (returned.clone(), frame(400)),
            ],
        )),
        frontmost: Some(window(11).app),
        ..FakeBackend::default()
    };
    backend.frames.insert(existing.clone(), frame(50));
    backend.frames.insert(returned.clone(), frame(400));
    let mut runtime = runtime(backend);
    bind(&mut runtime, WorkspaceId::ONE, &existing);
    let hidden = WorkspaceId::new(2).unwrap();
    bind(&mut runtime, hidden, &returned);
    runtime.parked.insert(returned.clone(), frame(70), None);

    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    assert!(!runtime.parked.contains_key(&returned));
    assert_eq!(
        runtime.workspaces.workspace_of(&returned),
        Some(WorkspaceId::ONE)
    );
    assert!(
        outputs()
            .union()
            .contains_center(runtime.backend.frames[&returned])
    );
}

#[test]
fn stale_accessibility_observation_does_not_heal_a_genuinely_parked_window() {
    let tiled = window(10);
    let parked = window(11);
    let offscreen = crate::Rect {
        x: -500,
        y: 1400,
        width: 471,
        height: 1002,
    };
    let mut backend = FakeBackend {
        observation: Some(observation_for_windows(
            "com.example.Test",
            &tiled,
            &[(tiled.clone(), frame(50)), (parked.clone(), frame(400))],
        )),
        ..FakeBackend::default()
    };
    backend.frames.insert(tiled.clone(), frame(50));
    // Accessibility still reports the pre-park frame (stale)...
    backend.frames.insert(parked.clone(), frame(400));
    // ...but the window server knows it is parked off-screen.
    backend.server_frames.insert(parked.clone(), offscreen);
    let mut runtime = runtime(backend);
    bind(&mut runtime, WorkspaceId::ONE, &tiled);
    let hidden = WorkspaceId::new(2).unwrap();
    bind(&mut runtime, hidden, &parked);
    runtime.parked.insert(parked.clone(), frame(400), None);

    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    assert_eq!(runtime.parked.get(&parked), Some(&frame(400)));
    assert_eq!(runtime.workspaces.workspace_of(&parked), Some(hidden));
}

#[test]
fn stale_accessibility_frame_does_not_skip_parking_an_on_screen_window() {
    let window = window(10);
    let stale_offscreen = crate::Rect {
        x: -2559,
        y: 1408,
        width: 2560,
        height: 1409,
    };
    let mut backend = FakeBackend::default();
    // Accessibility reports a stale parked position...
    backend.frames.insert(window.clone(), stale_offscreen);
    // ...but the window server says the window is really on-screen.
    backend.server_frames.insert(window.clone(), frame(50));
    let mut runtime = runtime(backend);

    assert_eq!(
        runtime.park_window(&window, outputs().main().frame, &outputs()),
        macos::ApplyOutcome::Applied
    );
    // The recovery frame is the window server's truth, not the stale read.
    assert_eq!(runtime.parked.get(&window), Some(&frame(50)));
    assert!(
        runtime
            .backend
            .calls
            .iter()
            .any(|call| matches!(call, BackendCall::SetPosition(_, _)))
    );
}

#[test]
fn second_confirmed_self_un_park_keeps_tiling_but_stops_parking_the_window() {
    let tiled = window(10);
    let fighter = window(11);
    let mut backend = FakeBackend {
        observation: Some(observation_for_windows(
            "com.example.Test",
            &tiled,
            &[(tiled.clone(), frame(50)), (fighter.clone(), frame(400))],
        )),
        frontmost: Some(window(11).app),
        ..FakeBackend::default()
    };
    backend.frames.insert(tiled.clone(), frame(50));
    backend.frames.insert(fighter.clone(), frame(400));
    let mut runtime = runtime(backend);
    bind(&mut runtime, WorkspaceId::ONE, &tiled);
    let hidden = WorkspaceId::new(2).unwrap();
    bind(&mut runtime, hidden, &fighter);

    // First confirmed self-un-park: healed and re-homed to workspace 1.
    runtime.parked.insert(fighter.clone(), frame(400), None);
    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();
    assert_eq!(
        runtime.workspaces.workspace_of(&fighter),
        Some(WorkspaceId::ONE)
    );

    // The user switches away (window back on a hidden workspace, parked),
    // and the window un-parks itself again shortly after: parking-exempt.
    runtime.workspaces.move_window(&fighter, hidden);
    runtime.parked.insert(fighter.clone(), frame(400), None);
    runtime.backend.frames.insert(fighter.clone(), frame(400));
    runtime
        .backend
        .server_frames
        .insert(fighter.clone(), frame(400));
    runtime.backend.calls.clear();
    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    assert!(runtime.parked.is_exempt(&fighter));
    assert_eq!(
        runtime.workspaces.workspace_of(&fighter),
        Some(WorkspaceId::ONE)
    );
    assert!(!runtime.parked.contains_key(&fighter));
    assert!(
        runtime
            .backend
            .calls
            .iter()
            .any(|call| matches!(call, BackendCall::Apply(window, _) if window == &fighter)),
        "calls: {:?}",
        runtime.backend.calls
    );

    // Switching away leaves the exempt window exactly where it is without
    // releasing its logical workspace ownership.
    let before = runtime.backend.frames[&fighter];
    runtime
        .switch(hidden, outputs(), &AtomicBool::new(false))
        .unwrap();
    assert!(!runtime.parked.contains_key(&fighter));
    assert_eq!(runtime.backend.frames[&fighter], before);
    assert_eq!(
        runtime.workspaces.workspace_of(&fighter),
        Some(WorkspaceId::ONE)
    );
}

#[test]
fn frontmost_window_relaunched_at_a_fractile_parking_edge_is_recovered() {
    let outputs = outputs();
    let display = outputs.main().frame;
    let normal = frame(50);
    let displays = outputs.frames().collect::<Vec<_>>();
    let parked = crate::geometry::parked_frame(display, &displays, normal);
    // macOS holds a titled window above the requested corner to keep its title bar on screen.
    let held_up = crate::Rect {
        y: parked.y - 31,
        ..parked
    };
    for at in [parked, held_up] {
        let stranded = window(10);
        let mut backend = FakeBackend {
            observation: Some(observation(&stranded, at)),
            focused: Some(stranded.clone()),
            frontmost: Some(stranded.app.clone()),
            ..FakeBackend::default()
        };
        backend.frames.insert(stranded.clone(), at);
        backend.server_frames.insert(stranded.clone(), at);
        let mut runtime = runtime(backend);

        runtime
            .cycle(outputs.clone(), &AtomicBool::new(false))
            .unwrap();

        assert_eq!(
            runtime.workspaces.workspace_of(&stranded),
            Some(WorkspaceId::ONE)
        );
        assert!(
            outputs
                .union()
                .contains_center(runtime.backend.frames[&stranded])
        );
        assert!(runtime.backend.calls.iter().any(|call| {
            matches!(
                call,
                BackendCall::SetPosition(window, target)
                    if window == &stranded && target.x == display.x && target.y == display.y
            )
        }));
    }
}

#[test]
fn background_reappearance_is_re_parked_instead_of_re_homed() {
    let tiled = window(10);
    let drifter = window(11);
    let mut backend = FakeBackend {
        observation: Some(observation_for_windows(
            "com.example.Test",
            &tiled,
            &[(tiled.clone(), frame(50)), (drifter.clone(), frame(400))],
        )),
        // A different application is frontmost: the reappearance is a
        // background bounds re-assertion, not the user summoning it.
        frontmost: Some(AppInstanceId {
            pid: 999,
            launched_at: 999,
        }),
        ..FakeBackend::default()
    };
    backend.frames.insert(tiled.clone(), frame(50));
    backend.frames.insert(drifter.clone(), frame(400));
    let mut runtime = runtime(backend);
    bind(&mut runtime, WorkspaceId::ONE, &tiled);
    let hidden = WorkspaceId::new(2).unwrap();
    bind(&mut runtime, hidden, &drifter);
    runtime.parked.insert(drifter.clone(), frame(400), None);

    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    // Ownership and recovery metadata are untouched; the window was simply
    // pushed back off-screen.
    assert_eq!(runtime.parked.get(&drifter), Some(&frame(400)));
    assert_eq!(runtime.workspaces.workspace_of(&drifter), Some(hidden));
    assert!(
        !outputs()
            .union()
            .contains_center(runtime.backend.frames[&drifter])
    );
}

#[test]
fn undone_re_parks_escalate_to_honoring_the_reappearance() {
    let tiled = window(10);
    let fighter = window(11);
    let mut backend = FakeBackend {
        observation: Some(observation(&tiled, frame(50))),
        frontmost: Some(AppInstanceId {
            pid: 999,
            launched_at: 999,
        }),
        // The app undoes every position write.
        frozen_server_frames: true,
        ..FakeBackend::default()
    };
    backend.frames.insert(tiled.clone(), frame(50));
    backend.frames.insert(fighter.clone(), frame(400));
    backend.server_frames.insert(fighter.clone(), frame(400));
    let mut runtime = runtime(backend);
    bind(&mut runtime, WorkspaceId::ONE, &tiled);
    let hidden = WorkspaceId::new(2).unwrap();
    bind(&mut runtime, hidden, &fighter);
    runtime.parked.insert(fighter.clone(), frame(400), None);

    for _ in 0..=REPARK_ATTEMPT_LIMIT {
        let server_frames = runtime.backend.window_server_frames().unwrap();
        let visible = runtime.visible_workspaces(&outputs());
        runtime.park_inactive(
            &outputs(),
            &visible,
            &server_frames,
            &AtomicBool::new(false),
            &mut 0,
        );
        // The settle grace would otherwise defer the next attempt.
        runtime.backend.advance(Duration::from_secs(3));
    }

    // The fight ended by honoring the window: recovery frame dropped and
    // the window re-homed to the visible workspace it appears on.
    assert!(!runtime.parked.contains_key(&fighter));
    assert_eq!(
        runtime.workspaces.workspace_of(&fighter),
        Some(WorkspaceId::ONE)
    );
}

#[test]
fn heal_waits_for_a_fresh_park_to_settle_before_trusting_reappearance() {
    let tiled = window(10);
    let parked = window(11);
    let mut backend = FakeBackend {
        observation: Some(observation_for_windows(
            "com.example.Test",
            &tiled,
            &[(tiled.clone(), frame(50)), (parked.clone(), frame(400))],
        )),
        ..FakeBackend::default()
    };
    backend.frames.insert(tiled.clone(), frame(50));
    backend.frames.insert(parked.clone(), frame(400));
    let mut runtime = runtime(backend);
    bind(&mut runtime, WorkspaceId::ONE, &tiled);
    let hidden = WorkspaceId::new(2).unwrap();
    bind(&mut runtime, hidden, &parked);
    // The park write just happened; the app is still applying it, so both
    // Accessibility and the window server still report the old frame.
    runtime.parked.insert(parked.clone(), frame(400), None);
    runtime.parked.park_applied(&parked, runtime.backend.now());

    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    assert_eq!(runtime.parked.get(&parked), Some(&frame(400)));
    assert_eq!(runtime.workspaces.workspace_of(&parked), Some(hidden));
}

#[test]
fn restore_keeps_recovery_frame_until_the_window_server_confirms_arrival() {
    let window = window(10);
    let parked_position = crate::Rect {
        x: -2559,
        y: 1408,
        width: 2560,
        height: 1409,
    };
    let mut backend = FakeBackend {
        frozen_server_frames: true,
        ..FakeBackend::default()
    };
    backend.frames.insert(window.clone(), parked_position);
    backend
        .server_frames
        .insert(window.clone(), parked_position);
    let mut runtime = runtime(backend);
    runtime.parked.insert(window.clone(), frame(50), None);

    // Accessibility read-back reports success, but the window server still
    // shows the window parked: the recovery frame must survive.
    assert_eq!(
        runtime.restore_window(&window),
        macos::ApplyOutcome::Applied
    );
    assert_eq!(runtime.parked.get(&window), Some(&frame(50)));

    // Once the real move lands, the next restore confirms and commits.
    runtime.backend.frozen_server_frames = false;
    assert_eq!(
        runtime.restore_window(&window),
        macos::ApplyOutcome::Applied
    );
    assert!(!runtime.parked.contains_key(&window));
    assert!(!runtime.backend.persisted.contains_key(&window));
}

#[test]
fn non_resizable_window_parks_and_restores_with_its_workspace() {
    let window = window(10);
    let normal = frame(50);
    let mut observed = observation(&window, normal);
    let crate::AppObservation::Complete { windows, .. } = &mut observed.applications[0] else {
        unreachable!();
    };
    windows[0].resizable = false;
    let mut backend = FakeBackend {
        observation: Some(observed),
        ..FakeBackend::default()
    };
    backend.frames.insert(window.clone(), normal);
    let mut runtime = runtime(backend);

    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();
    assert_eq!(
        runtime.workspaces.floating_workspace_of(&window),
        Some(WorkspaceId::ONE)
    );
    assert!(
        runtime
            .workspaces
            .engine(WorkspaceId::ONE)
            .model()
            .workspace
            .slots
            .is_empty()
    );

    let other = WorkspaceId::new(2).unwrap();
    runtime
        .switch(other, outputs(), &AtomicBool::new(false))
        .unwrap();
    assert!(runtime.parked.contains_key(&window));
    assert!(
        !outputs()
            .union()
            .contains_center(runtime.backend.frames[&window])
    );

    runtime
        .switch(WorkspaceId::ONE, outputs(), &AtomicBool::new(false))
        .unwrap();
    assert_eq!(runtime.backend.frames[&window], normal);
    assert!(!runtime.parked.contains_key(&window));
}

/// A route is applied before reconciliation: a park that fails leaves the window owned by no
/// workspace (never tiled on the one on screen), and the next cycle routes it again.
#[test]
fn failed_route_parking_leaves_the_window_unowned_until_a_later_cycle_routes_it() {
    let window = window(10);
    let normal = frame(50);
    let mut backend = FakeBackend {
        observation: Some(observation(&window, normal)),
        fail_persist: true,
        ..FakeBackend::default()
    };
    backend.frames.insert(window.clone(), normal);
    let config = Config::parse(
        "version = 1\n[[rules]]\nbundle_id = 'com.example.Test'\naction = 'route'\nworkspace = 2",
    )
    .unwrap();
    let mut runtime = runtime_with_config(backend, config);

    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    assert_eq!(runtime.workspaces.workspace_of_any(&window), None);
    assert!(!runtime.parked.contains_key(&window));
    assert_eq!(runtime.backend.frames[&window], normal);

    runtime.backend.fail_persist = false;
    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    assert_eq!(
        runtime.workspaces.workspace_of(&window),
        Some(WorkspaceId::TWO)
    );
    assert!(runtime.parked.contains_key(&window));
    assert!(
        runtime
            .workspaces
            .engine(WorkspaceId::ONE)
            .bound_windows()
            .is_empty()
    );
}

#[test]
fn restore_all_repositions_windows_at_their_original_frames() {
    let window = window(10);
    let normal = frame(50);
    let mut backend = FakeBackend::default();
    backend.frames.insert(window.clone(), normal);
    let mut runtime = runtime(backend);
    bind(&mut runtime, WorkspaceId::new(2).unwrap(), &window);
    let outputs = outputs();
    let visible = runtime.visible_workspaces(&outputs);
    let server_frames = runtime.backend.window_server_frames().unwrap();
    runtime.park_inactive(
        &outputs,
        &visible,
        &server_frames,
        &AtomicBool::new(false),
        &mut 0,
    );

    assert_eq!(runtime.restore_all(), 0);
    assert_eq!(runtime.backend.frames[&window], normal);
    assert!(runtime.backend.persisted.is_empty());
}

#[test]
fn restore_persistence_failure_keeps_retry_state() {
    let window = window(10);
    let normal = frame(50);
    let mut backend = FakeBackend {
        fail_persist: true,
        ..FakeBackend::default()
    };
    backend.frames.insert(window.clone(), frame(5000));
    let mut parked = HashMap::new();
    parked.insert(window.clone(), normal);
    let mut runtime = WorkspaceRuntime::with_backend_and_config(
        ManagementScope::Both,
        backend,
        parked,
        Config::default(),
    );

    assert_eq!(
        runtime.restore_window(&window),
        macos::ApplyOutcome::Applied
    );

    assert_eq!(runtime.backend.frames[&window], normal);
    assert_eq!(runtime.parked[&window], normal);
    assert_eq!(runtime.restore_all(), 1);
}

#[test]
fn persistence_failure_prevents_offscreen_positioning() {
    let window = window(10);
    let normal = frame(50);
    let mut backend = FakeBackend {
        fail_persist: true,
        ..FakeBackend::default()
    };
    backend.frames.insert(window.clone(), normal);
    let mut runtime = runtime(backend);

    assert_eq!(
        runtime.park_window(&window, outputs().main().frame, &outputs()),
        macos::ApplyOutcome::Unavailable
    );
    assert_eq!(runtime.backend.frames[&window], normal);
    assert!(
        !runtime
            .backend
            .calls
            .iter()
            .any(|call| matches!(call, BackendCall::SetPosition(_, _)))
    );
}

#[test]
fn already_offscreen_window_is_not_recorded_as_a_recovery_frame() {
    let window = window(10);
    let offscreen = crate::Rect {
        x: -500,
        y: 100,
        width: 400,
        height: 300,
    };
    let mut backend = FakeBackend::default();
    backend.frames.insert(window.clone(), offscreen);
    let mut runtime = runtime(backend);

    assert_eq!(
        runtime.park_window(&window, outputs().main().frame, &outputs()),
        macos::ApplyOutcome::Applied
    );
    assert!(!runtime.parked.contains_key(&window));
    assert!(runtime.backend.persisted.is_empty());
    assert!(
        !runtime
            .backend
            .calls
            .iter()
            .any(|call| matches!(call, BackendCall::SetPosition(_, _)))
    );
}

#[test]
fn persisted_frames_recover_after_a_simulated_crash() {
    let window = window(10);
    let normal = frame(50);
    let mut backend = FakeBackend::default();
    backend.frames.insert(window.clone(), normal);
    let mut first = runtime(backend);
    assert_eq!(
        first.park_window(&window, outputs().main().frame, &outputs()),
        macos::ApplyOutcome::Applied
    );
    let parked_frame = first.backend.frames[&window];
    let persisted = first.backend.persisted.clone();

    let mut second_backend = FakeBackend::default();
    second_backend.frames.insert(window.clone(), parked_frame);
    let mut second =
        WorkspaceRuntime::with_backend(ManagementScope::Both, second_backend, persisted);

    assert_eq!(second.restore_all(), 0);
    assert_eq!(second.backend.frames[&window], normal);
    assert!(second.backend.persisted.is_empty());
}

#[test]
fn hidden_window_accessibility_cannot_find_keeps_its_slot_while_the_window_server_lists_it() {
    // Right after launch, Accessibility may fail to find windows that exist.
    // That must not silently drop every hidden workspace's windows.
    let hidden = window(10);
    let mut backend = FakeBackend {
        observation: Some(observation(&hidden, frame(50))),
        frame_outcome: Some(macos::WindowAccessError::Gone),
        ..FakeBackend::default()
    };
    backend.server_frames.insert(hidden.clone(), frame(50));
    let mut runtime = runtime(backend);
    let workspace = WorkspaceId::new(2).unwrap();
    bind(&mut runtime, workspace, &hidden);
    let cancelled = AtomicBool::new(false);

    runtime.cycle(outputs(), &cancelled).unwrap();
    assert_eq!(runtime.workspaces.workspace_of(&hidden), Some(workspace));

    // Accessibility recovers: the slot is intact and parking proceeds.
    runtime.backend.frame_outcome = None;
    runtime.backend.frames.insert(hidden.clone(), frame(50));
    runtime.cycle(outputs(), &cancelled).unwrap();
    assert_eq!(runtime.workspaces.workspace_of(&hidden), Some(workspace));
    assert!(runtime.parked.contains_key(&hidden));
    assert!(runtime.inaccessible_since.is_empty());
}

#[test]
fn persistently_inaccessible_or_closed_hidden_windows_are_forgotten() {
    let stuck = window(10);
    let closed = window(11);
    let mut backend = FakeBackend {
        observation: Some(observation(&stuck, frame(50))),
        frame_outcome: Some(macos::WindowAccessError::Gone),
        ..FakeBackend::default()
    };
    backend.server_frames.insert(stuck.clone(), frame(50));
    let mut runtime = runtime(backend);
    let workspace = WorkspaceId::new(2).unwrap();
    bind(&mut runtime, workspace, &stuck);
    bind(&mut runtime, workspace, &closed);
    let cancelled = AtomicBool::new(false);

    runtime.cycle(outputs(), &cancelled).unwrap();
    // The window server does not list the closed window: forgotten at once.
    assert_eq!(runtime.workspaces.workspace_of(&closed), None);
    assert_eq!(runtime.workspaces.workspace_of(&stuck), Some(workspace));

    *runtime.inaccessible_since.get_mut(&stuck).unwrap() -= crate::app::parking::INACCESSIBLE_GRACE;
    runtime.cycle(outputs(), &cancelled).unwrap();
    assert_eq!(runtime.workspaces.workspace_of(&stuck), None);
}

#[test]
fn mission_control_thumbnail_frames_do_not_trigger_repark_or_escalation() {
    let tiled = window(10);
    let parked_window = window(11);
    let outputs = outputs();
    let original = outputs.main().frame;
    let parked_frame = main_parked_frame(original);
    let expose_thumbnail = crate::Rect {
        x: 200,
        y: 200,
        width: 300,
        height: 200,
    };
    let mut backend = FakeBackend {
        observation: Some(observation(&tiled, frame(50))),
        frontmost: Some(tiled.app.clone()),
        ..FakeBackend::default()
    };
    backend.frames.insert(tiled.clone(), frame(50));
    // Accessibility still reports the parked off-screen frame...
    backend.frames.insert(parked_window.clone(), parked_frame);
    // ...while the window server reports a scaled-down Mission Control thumbnail on screen.
    backend
        .server_frames
        .insert(parked_window.clone(), expose_thumbnail);
    let mut runtime = runtime_with_parked(backend, [(parked_window.clone(), original)]);
    bind(&mut runtime, WorkspaceId::ONE, &tiled);
    let hidden = WorkspaceId::new(2).unwrap();
    bind(&mut runtime, hidden, &parked_window);

    for _ in 0..=REPARK_ATTEMPT_LIMIT {
        let server_frames = runtime.backend.window_server_frames().unwrap();
        let visible = runtime.visible_workspaces(&outputs);
        runtime.park_inactive(
            &outputs,
            &visible,
            &server_frames,
            &AtomicBool::new(false),
            &mut 0,
        );
    }

    assert!(runtime.parked.reparks(&parked_window) == 0);
    assert_eq!(runtime.parked.get(&parked_window), Some(&original));
    assert_eq!(
        runtime.workspaces.workspace_of(&parked_window),
        Some(hidden)
    );
    assert!(!runtime.backend.calls.iter().any(
        |call| matches!(call, BackendCall::SetPosition(window, _) if window == &parked_window)
    ));
}
