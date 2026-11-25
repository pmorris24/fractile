use super::*;

#[test]
fn closing_the_only_window_of_a_running_app_reflows_surviving_windows() {
    let terminal = window(10);
    let popup = crate::PhysicalWindowId {
        app: AppInstanceId {
            pid: 2,
            launched_at: 1,
        },
        window_id: 20,
    };
    let backend = FakeBackend {
        frames: HashMap::from([(terminal.clone(), frame(50)), (popup.clone(), frame(500))]),
        observation: Some(crate::NativeObservation {
            cycle: 0,
            mouse_down: false,
            applications: vec![
                crate::AppObservation::Complete {
                    app: crate::ObservedApplication {
                        id: terminal.app.clone(),
                        bundle_id: "com.mitchellh.ghostty".into(),
                    },
                    focused: Some(terminal.clone()),
                    windows: vec![crate::ObservedWindow {
                        id: terminal.clone(),
                        frame: frame(50),
                        resizable: true,
                        title: None,
                        document: None,
                    }],
                },
                crate::AppObservation::Complete {
                    app: crate::ObservedApplication {
                        id: popup.app.clone(),
                        bundle_id: "com.example.Popup".into(),
                    },
                    focused: Some(popup.clone()),
                    windows: vec![crate::ObservedWindow {
                        id: popup.clone(),
                        frame: frame(500),
                        resizable: true,
                        title: None,
                        document: None,
                    }],
                },
            ],
        }),
        ..FakeBackend::default()
    };
    let mut runtime = runtime(backend);
    let outputs = outputs();
    let cancelled = AtomicBool::new(false);
    runtime.cycle(outputs.clone(), &cancelled).unwrap();
    assert_eq!(runtime.backend.frames[&terminal].width, 500);

    runtime.backend.frames.remove(&popup);
    runtime.backend.observation = Some(observation_for(
        "com.mitchellh.ghostty",
        &terminal,
        runtime.backend.frames[&terminal],
    ));
    runtime.backend.server_snapshot_unavailable = true;
    runtime.cycle(outputs.clone(), &cancelled).unwrap();
    assert_eq!(runtime.backend.frames[&terminal].width, 500);

    runtime.backend.server_snapshot_unavailable = false;
    runtime.cycle(outputs.clone(), &cancelled).unwrap();

    assert_eq!(runtime.backend.frames[&terminal], outputs.main().frame);
    assert_eq!(
        runtime.workspaces.engine(WorkspaceId::ONE).bound_windows(),
        vec![terminal]
    );
}

#[test]
fn configured_third_display_has_an_independent_visible_workspace() {
    // Workspace 2 outranks the default tertiary lane (8) on the display it is pinned to.
    let config =
        Config::parse("version = 1\n[[workspaces]]\nworkspace = 2\nmonitor = 'third-id'").unwrap();
    let runtime = runtime_with_config(benchmark_backend(0), config);

    let visible = runtime.visible_workspaces(&three_outputs());

    assert_eq!(
        visible
            .iter()
            .filter_map(|resolved| resolved.workspace.number())
            .collect::<Vec<_>>(),
        vec![1, 6, 2]
    );
}

#[test]
fn missing_preferred_display_falls_back_without_double_visibility() {
    let config =
        Config::parse("version = 1\n[[workspaces]]\nworkspace = 9\nmonitor = 'third-id'").unwrap();
    let runtime = runtime_with_config(benchmark_backend(0), config);

    let visible = runtime.visible_workspaces(&outputs());

    assert_eq!(
        visible
            .iter()
            .filter_map(|resolved| resolved.workspace.number())
            .collect::<Vec<_>>(),
        vec![1, 6]
    );
}

#[test]
fn active_preferred_workspace_falls_back_and_returns_after_hotplug() {
    let config =
        Config::parse("version = 1\n[[workspaces]]\nworkspace = 2\nmonitor = 'third-id'").unwrap();
    let mut runtime = runtime_with_config(benchmark_backend(0), config);

    runtime
        .cycle(three_outputs(), &AtomicBool::new(false))
        .unwrap();
    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();
    assert_eq!(
        runtime
            .visible_workspaces(&outputs())
            .iter()
            .filter_map(|resolved| resolved.workspace.number())
            .collect::<Vec<_>>(),
        vec![2, 6]
    );

    runtime
        .cycle(three_outputs(), &AtomicBool::new(false))
        .unwrap();
    assert_eq!(
        runtime
            .visible_workspaces(&three_outputs())
            .iter()
            .filter_map(|resolved| resolved.workspace.number())
            .collect::<Vec<_>>(),
        vec![1, 6, 2]
    );
}

#[test]
fn transient_frame_failure_preserves_inactive_window_ownership() {
    let window = window(10);
    let mut backend = FakeBackend {
        frame_outcome: Some(macos::WindowAccessError::Unavailable),
        ..FakeBackend::default()
    };
    backend.frames.insert(window.clone(), frame(50));
    let mut runtime = runtime(backend);
    let workspace = WorkspaceId::new(2).unwrap();
    bind(&mut runtime, workspace, &window);

    let outputs = outputs();
    let visible = runtime.visible_workspaces(&outputs);
    let mut failed = 0;
    let server_frames = runtime.backend.window_server_frames().unwrap();
    runtime.park_inactive(
        &outputs,
        &visible,
        &server_frames,
        &AtomicBool::new(false),
        &mut failed,
    );

    assert_eq!(failed, 1);
    assert_eq!(runtime.workspaces.workspace_of(&window), Some(workspace));
    assert!(runtime.parked.is_empty());
}

#[test]
fn cycle_reconciles_and_applies_through_the_fake_backend() {
    let window = window(10);
    let normal = frame(50);
    let mut backend = FakeBackend {
        observation: Some(observation(&window, normal)),
        ..FakeBackend::default()
    };
    backend.frames.insert(window.clone(), normal);
    let mut runtime =
        WorkspaceRuntime::with_backend(ManagementScope::Main, backend, HashMap::new());

    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    assert_eq!(
        runtime
            .workspaces
            .engine(WorkspaceId::ONE)
            .model()
            .workspace
            .slots
            .len(),
        1
    );
    assert_eq!(
        runtime.workspaces.workspace_of(&window),
        Some(WorkspaceId::ONE)
    );
    assert!(
        runtime
            .backend
            .calls
            .iter()
            .any(|call| matches!(call, BackendCall::Apply(applied, _) if applied == &window))
    );
}

#[test]
fn on_screen_window_omitted_from_one_snapshot_keeps_its_slot() {
    let kept = window(10);
    let closed = window(11);
    let mut backend = FakeBackend {
        observation: Some(observation_for_windows("com.example.Test", &kept, &[])),
        ..FakeBackend::default()
    };
    backend.frames.insert(kept.clone(), frame(50));
    let mut runtime = runtime(backend);
    bind(&mut runtime, WorkspaceId::ONE, &kept);
    bind(&mut runtime, WorkspaceId::ONE, &closed);

    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    assert_eq!(
        runtime.workspaces.workspace_of(&kept),
        Some(WorkspaceId::ONE),
        "the window server still shows the window on screen"
    );
    assert_eq!(
        runtime.workspaces.workspace_of(&closed),
        Some(WorkspaceId::ONE),
        "the app's whole snapshot is distrusted while an omission settles"
    );

    runtime.omitted_windows.insert(
        kept.clone(),
        cycle::Omission::new(runtime.backend.now() - OMISSION_SETTLE * 2),
    );
    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    assert_eq!(
        runtime.workspaces.workspace_of(&kept),
        None,
        "an omission that outlasts the settle window is believed"
    );
    assert_eq!(runtime.workspaces.workspace_of(&closed), None);
    assert!(runtime.omitted_windows.is_empty());
}

#[test]
fn omission_settling_ends_when_the_window_is_observed_again() {
    let window = window(10);
    let mut backend = FakeBackend {
        observation: Some(observation_for_windows("com.example.Test", &window, &[])),
        ..FakeBackend::default()
    };
    backend.frames.insert(window.clone(), frame(50));
    let mut runtime = runtime(backend);
    bind(&mut runtime, WorkspaceId::ONE, &window);

    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();
    assert!(runtime.omitted_windows.contains_key(&window));

    runtime.backend.observation = Some(observation(&window, frame(50)));
    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    assert!(runtime.omitted_windows.is_empty());
    assert_eq!(
        runtime.workspaces.workspace_of(&window),
        Some(WorkspaceId::ONE)
    );
}

pub(super) fn studio_display() -> macos::DisplayOutputs {
    macos::DisplayOutputs::new(vec![macos::DisplayOutput {
        id: "studio-display-id".into(),
        name: "Studio Display".into(),
        frame: crate::Rect {
            x: 0,
            y: 30,
            width: 2560,
            height: 1410,
        },
        is_main: true,
    }])
    .unwrap()
}

pub(super) fn column(x: i32, width: i32) -> crate::Rect {
    crate::Rect {
        x,
        y: 30,
        width,
        height: 1410,
    }
}

pub(super) fn apps_observation(
    apps: &[(&str, &crate::PhysicalWindowId, crate::Rect)],
) -> crate::NativeObservation {
    crate::NativeObservation {
        cycle: 0,
        mouse_down: false,
        applications: apps
            .iter()
            .map(
                |(bundle_id, window, frame)| crate::AppObservation::Complete {
                    app: crate::ObservedApplication {
                        id: window.app.clone(),
                        bundle_id: (*bundle_id).into(),
                    },
                    focused: None,
                    windows: vec![crate::ObservedWindow {
                        id: (*window).clone(),
                        frame: *frame,
                        resizable: true,
                        title: None,
                        document: None,
                    }],
                },
            )
            .collect(),
    }
}

pub(super) fn app_window(pid: i32, launched_at: u64, window_id: u32) -> crate::PhysicalWindowId {
    crate::PhysicalWindowId {
        app: AppInstanceId { pid, launched_at },
        window_id,
    }
}

#[test]
fn early_open_event_rechecks_until_the_new_window_can_split_its_sibling() {
    let sibling = app_window(1, 1, 10);
    let opening = app_window(2, 1, 20);
    let full = column(0, 2560);
    let cancelled = AtomicBool::new(false);
    let mut runtime = runtime(FakeBackend {
        frames: [(sibling.clone(), full)].into(),
        observation: Some(apps_observation(&[("com.example.Sibling", &sibling, full)])),
        frontmost: Some(opening.app.clone()),
        ..FakeBackend::default()
    });
    let start = Instant::now();
    let until = Some(start + WINDOW_OPENING_SETTLE);
    let mut now = start;
    // AX announces creation about 180 ms before CG orders this window in.
    // No further native event arrives as it becomes observable.
    while now < start + Duration::from_millis(180) {
        // Activation can precede AX focus, which itself can precede CG visibility.
        if now > start {
            runtime.backend.focused = Some(opening.clone());
        }
        let status = runtime.cycle(studio_display(), &cancelled).unwrap();
        assert_eq!(runtime.workspaces.workspace_of(&opening), None);
        assert_eq!(runtime.backend.frames[&sibling], full);
        now = opening_recheck(
            now,
            until,
            Some(&opening.app),
            status.missing_frontmost_window.as_ref(),
        )
        .unwrap();
    }
    assert!(now < start + Duration::from_millis(180) + WINDOW_OPENING_RECHECK);
    runtime.backend.frames.insert(opening.clone(), full);
    runtime.backend.observation = Some(apps_observation(&[
        ("com.example.Sibling", &sibling, full),
        ("com.example.Opening", &opening, full),
    ]));
    let status = runtime.cycle(studio_display(), &cancelled).unwrap();
    assert_eq!(runtime.backend.frames[&sibling].width, 1280);
    assert_eq!(runtime.backend.frames[&opening].width, 1280);
    assert!(
        opening_recheck(
            now,
            until,
            Some(&opening.app),
            status.missing_frontmost_window.as_ref()
        )
        .is_none()
    );
}

#[test]
fn closed_window_its_app_keeps_ordered_out_gives_up_its_tile() {
    // Frames from a real report: three apps tiled in thirds on a 2560x1410
    // display. The middle app closed its window by ordering it out, so it had
    // no on-screen window and the snapshot omitted the app, but the window
    // server still listed the hidden window, which kept its third empty.
    let editor = app_window(681, 1, 54);
    let voice = app_window(271, 1, 2515);
    let agent = app_window(992, 1, 2583);
    let outputs = studio_display();
    let mut backend = FakeBackend {
        observation: Some(apps_observation(&[
            ("com.example.Editor", &editor, column(0, 853)),
            ("com.example.Voice", &voice, column(853, 860)),
            ("com.example.Agent", &agent, column(1706, 854)),
        ])),
        ..FakeBackend::default()
    };
    backend.frames.insert(editor.clone(), column(0, 853));
    backend.frames.insert(voice.clone(), column(853, 860));
    backend.frames.insert(agent.clone(), column(1706, 854));
    let mut runtime = runtime(backend);
    let workspace = WorkspaceId::ONE;
    bind(&mut runtime, workspace, &editor);
    bind(&mut runtime, workspace, &voice);
    bind(&mut runtime, workspace, &agent);
    let cancelled = AtomicBool::new(false);
    runtime.cycle(outputs.clone(), &cancelled).unwrap();
    assert_eq!(runtime.backend.frames[&voice].x, 853);

    runtime.backend.observation = Some(apps_observation(&[
        ("com.example.Editor", &editor, column(0, 853)),
        ("com.example.Agent", &agent, column(1706, 854)),
    ]));
    runtime.backend.frames.remove(&voice);
    runtime
        .backend
        .server_frames
        .insert(voice.clone(), column(853, 860));
    runtime.backend.ordered_out.insert(voice.window_id);
    runtime.cycle(outputs.clone(), &cancelled).unwrap();
    assert_eq!(
        runtime.workspaces.workspace_of(&voice),
        Some(workspace),
        "a window that just left the screen settles before its tile is given up"
    );

    runtime.omitted_windows.insert(
        voice.clone(),
        cycle::Omission::new(runtime.backend.now() - OMISSION_SETTLE * 2),
    );
    runtime.cycle(outputs.clone(), &cancelled).unwrap();

    assert_eq!(runtime.workspaces.workspace_of(&voice), None);
    assert_eq!(runtime.backend.frames[&editor], column(0, 1280));
    assert_eq!(runtime.backend.frames[&agent], column(1280, 1280));
}

#[test]
fn workspace_whose_windows_all_leave_the_screen_keeps_its_tiles() {
    // Switching to another macOS Space or a native fullscreen Space orders out
    // every window on the display; that is not every window closing.
    let editor = app_window(681, 1, 54);
    let agent = app_window(992, 1, 2583);
    let outputs = studio_display();
    let mut backend = FakeBackend {
        observation: Some(apps_observation(&[
            ("com.example.Editor", &editor, column(0, 1280)),
            ("com.example.Agent", &agent, column(1280, 1280)),
        ])),
        ..FakeBackend::default()
    };
    backend.frames.insert(editor.clone(), column(0, 1280));
    backend.frames.insert(agent.clone(), column(1280, 1280));
    let mut runtime = runtime(backend);
    let workspace = WorkspaceId::ONE;
    bind(&mut runtime, workspace, &editor);
    bind(&mut runtime, workspace, &agent);
    let cancelled = AtomicBool::new(false);
    runtime.cycle(outputs.clone(), &cancelled).unwrap();

    runtime.backend.observation = Some(apps_observation(&[]));
    for window in [&editor, &agent] {
        let frame = runtime.backend.frames.remove(window).unwrap();
        runtime.backend.server_frames.insert(window.clone(), frame);
        runtime.backend.ordered_out.insert(window.window_id);
    }
    runtime.cycle(outputs.clone(), &cancelled).unwrap();
    for window in [&editor, &agent] {
        runtime.omitted_windows.insert(
            window.clone(),
            cycle::Omission::new(runtime.backend.now() - OMISSION_SETTLE * 2),
        );
    }
    runtime.cycle(outputs.clone(), &cancelled).unwrap();

    assert!(runtime.backend.close_probes.is_empty());
    assert_eq!(runtime.workspaces.workspace_of(&editor), Some(workspace));
    assert_eq!(runtime.workspaces.workspace_of(&agent), Some(workspace));
    assert!(
        runtime.omitted_windows.is_empty(),
        "time off screen on another Space does not count toward the settle"
    );
}

#[test]
fn restored_slots_for_a_dead_application_are_retired() {
    let dead = window(10);
    let mut persisted = Workspaces::default();
    persisted.engine_mut(WorkspaceId::ONE).insert_slot(Slot {
        id: SlotId(1),
        binding: BindingState::Bound(dead.clone()),
    });
    let backend = FakeBackend {
        observation: Some(crate::NativeObservation {
            cycle: 0,
            mouse_down: false,
            applications: Vec::new(),
        }),
        running_apps: Some(HashSet::new()),
        ..FakeBackend::default()
    };
    let mut runtime = WorkspaceRuntime::with_backend_config_state(
        ManagementScope::Both,
        backend,
        HashMap::new(),
        Config::default(),
        Some(persisted.state()),
    );

    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    assert_eq!(runtime.workspaces.workspace_of(&dead), None);
    assert!(
        runtime
            .workspaces
            .engine(WorkspaceId::ONE)
            .model()
            .workspace
            .slots
            .is_empty()
    );
    assert!(runtime.backend.workspace_state.is_some());
}

#[test]
fn an_app_that_quits_while_hidden_releases_its_tiles_and_recovery_metadata() {
    let survivor = window(10);
    let dead = crate::PhysicalWindowId {
        app: AppInstanceId {
            pid: 2,
            launched_at: 2,
        },
        window_id: 20,
    };
    let backend = FakeBackend {
        observation: Some(crate::NativeObservation {
            cycle: 0,
            mouse_down: false,
            applications: Vec::new(),
        }),
        running_apps: Some(HashSet::from([survivor.app.clone()])),
        ..FakeBackend::default()
    };
    let mut runtime = runtime_with_parked(
        backend,
        [(survivor.clone(), frame(50)), (dead.clone(), frame(500))],
    );
    let hidden = WorkspaceId::new(3).unwrap();
    bind(&mut runtime, hidden, &survivor);
    bind(&mut runtime, hidden, &dead);
    runtime.backend.frames.insert(survivor.clone(), frame(50));
    let cancelled = AtomicBool::new(false);

    runtime.cycle(outputs(), &cancelled).unwrap();

    assert_eq!(runtime.workspaces.workspace_of(&dead), None);
    assert_eq!(runtime.workspaces.workspace_of(&survivor), Some(hidden));
    assert_eq!(
        runtime
            .workspaces
            .engine(hidden)
            .model()
            .workspace
            .slots
            .len(),
        1
    );
    assert!(runtime.backend.workspace_state.is_some());
    assert!(!runtime.parked.contains_key(&dead));
    assert!(!runtime.backend.persisted.contains_key(&dead));
    assert!(runtime.backend.persisted.contains_key(&survivor));
    assert!(runtime.parked.contains_key(&survivor));
}

#[test]
fn asynchronously_clamped_window_settles_without_reapplying_every_cycle() {
    let main = window(10);
    let huddle = window(11);
    let settings = window(12);
    let mut backend = FakeBackend {
        observation: Some(observation_for_windows(
            "com.tinyspeck.slackmacgap",
            &settings,
            &[
                (main.clone(), frame(50)),
                (huddle.clone(), frame(350)),
                (settings.clone(), frame(650)),
            ],
        )),
        ..FakeBackend::default()
    };
    backend.frames.insert(main.clone(), frame(50));
    backend.frames.insert(huddle.clone(), frame(350));
    backend.frames.insert(settings.clone(), frame(650));
    let columns = Config::parse("version = 1\n[settings]\nlayout = \"columns\"\n").unwrap();
    let mut runtime = runtime_with_config(backend, columns);
    let outputs = outputs();
    let cancelled = AtomicBool::new(false);

    runtime.cycle(outputs.clone(), &cancelled).unwrap();
    assert_eq!(runtime.backend.observed_apply_count, 1);
    let target = runtime.backend.frames[&huddle];
    assert_eq!(target.height, 800);

    // Electron reports AXSize writes as Applied synchronously, then clamps the
    // popped-out huddle window's height asynchronously before the next cycle.
    let clamped = crate::Rect {
        height: 520,
        ..target
    };
    runtime.backend.frames.insert(huddle.clone(), clamped);

    for _ in 0..3 {
        runtime.cycle(outputs.clone(), &cancelled).unwrap();
        assert_eq!(runtime.backend.observed_apply_count, 1);
        assert_eq!(runtime.backend.frames[&huddle], clamped);
        assert_eq!(
            runtime.constrained[WorkspaceId::ONE.index()].get(&huddle),
            Some(&(target, clamped))
        );
    }

    let visible = runtime.visible_workspaces(&outputs);
    let server = runtime.backend.window_server_frames().unwrap();
    let windows = runtime.arranged_windows(&server);
    assert!(
        runtime
            .anomalies(&outputs, &visible, &windows, &server)
            .is_empty()
    );

    // Closing the third window widens the tile, so the old constraint is re-evaluated.
    runtime.backend.frames.remove(&settings);
    runtime.cycle(outputs.clone(), &cancelled).unwrap();
    assert_eq!(runtime.backend.observed_apply_count, 2);
    assert_eq!(
        runtime.backend.frames[&huddle],
        crate::Rect {
            x: 500,
            y: 0,
            width: 500,
            height: 800,
        }
    );
    runtime.cycle(outputs, &cancelled).unwrap();
    assert!(!runtime.constrained[WorkspaceId::ONE.index()].contains_key(&huddle));
}

#[test]
fn varying_async_bounces_freeze_after_attempt_limit_and_clear_on_target_change() {
    let main = window(10);
    let huddle = window(11);
    let settings = window(12);
    let mut backend = FakeBackend {
        observation: Some(observation_for_windows(
            "com.tinyspeck.slackmacgap",
            &settings,
            &[
                (main.clone(), frame(50)),
                (huddle.clone(), frame(350)),
                (settings.clone(), frame(650)),
            ],
        )),
        ..FakeBackend::default()
    };
    backend.frames.insert(main.clone(), frame(50));
    backend.frames.insert(huddle.clone(), frame(350));
    backend.frames.insert(settings.clone(), frame(650));
    let columns = Config::parse("version = 1\n[settings]\nlayout = \"columns\"\n").unwrap();
    let mut runtime = runtime_with_config(backend, columns);
    let outputs = outputs();
    let cancelled = AtomicBool::new(false);

    // Cycle 1: initial layout applies target to huddle.
    runtime.cycle(outputs.clone(), &cancelled).unwrap();
    assert_eq!(runtime.backend.observed_apply_count, 1);
    let target = runtime.backend.frames[&huddle];
    assert_eq!(target.height, 800);

    // Electron bounces huddle asynchronously to a different frame on every cycle,
    // never staying within frame_tolerance of the last constrained frame.
    let varying_heights = [520, 560, 480, 600, 440, 640, 420, 660, 400, 680, 380, 700];
    for height in varying_heights {
        let bounced = crate::Rect { height, ..target };
        runtime.backend.frames.insert(huddle.clone(), bounced);
        runtime.cycle(outputs.clone(), &cancelled).unwrap();
    }

    let limit = usize::from(super::super::cycle::APPLY_ATTEMPT_LIMIT);
    assert_eq!(runtime.backend.observed_apply_count, limit);
    assert_eq!(
        runtime.apply_attempts[WorkspaceId::ONE.index()].get(&huddle),
        Some(&(target, super::super::cycle::APPLY_ATTEMPT_LIMIT))
    );
    assert!(runtime.constrained[WorkspaceId::ONE.index()].contains_key(&huddle));

    // Frozen windows do not spam Misplaced anomalies.
    let visible = runtime.visible_workspaces(&outputs);
    let server = runtime.backend.window_server_frames().unwrap();
    let windows = runtime.arranged_windows(&server);
    assert!(
        runtime
            .anomalies(&outputs, &visible, &windows, &server)
            .is_empty()
    );

    // Closing the third window changes huddle's target frame, clearing the backoff.
    runtime.backend.frames.remove(&settings);
    runtime.cycle(outputs.clone(), &cancelled).unwrap();
    assert_eq!(runtime.backend.observed_apply_count, limit + 1);
    assert_eq!(
        runtime.backend.frames[&huddle],
        crate::Rect {
            x: 500,
            y: 0,
            width: 500,
            height: 800,
        }
    );
    runtime.cycle(outputs, &cancelled).unwrap();
    assert!(!runtime.apply_attempts[WorkspaceId::ONE.index()].contains_key(&huddle));
    assert!(!runtime.constrained[WorkspaceId::ONE.index()].contains_key(&huddle));
}

#[test]
fn varying_constrained_rejections_freeze_and_clear_on_user_gesture() {
    let main = window(10);
    let stubborn = window(11);
    let mut backend = FakeBackend {
        observation: Some(observation_for_windows(
            "com.tinyspeck.slackmacgap",
            &main,
            &[(main.clone(), frame(50)), (stubborn.clone(), frame(550))],
        )),
        managed_outcome: Some(macos::ApplyOutcome::Constrained),
        ..FakeBackend::default()
    };
    backend.frames.insert(main.clone(), frame(50));
    backend.frames.insert(stubborn.clone(), frame(550));
    let mut runtime = runtime(backend);
    let outputs = outputs();
    let cancelled = AtomicBool::new(false);

    // First cycle binds both windows; stubborn rejects with varying heights across cycles.
    let target = crate::Rect {
        x: 500,
        y: 0,
        width: 500,
        height: 800,
    };
    for height in [520, 560, 480, 600, 440, 640] {
        runtime
            .backend
            .frames
            .insert(stubborn.clone(), crate::Rect { height, ..target });
        runtime.cycle(outputs.clone(), &cancelled).unwrap();
    }

    let limit = usize::from(super::super::cycle::APPLY_ATTEMPT_LIMIT);
    assert_eq!(runtime.backend.observed_apply_count, limit);
    assert_eq!(
        runtime.apply_attempts[WorkspaceId::ONE.index()].get(&stubborn),
        Some(&(target, super::super::cycle::APPLY_ATTEMPT_LIMIT))
    );

    // User gesture clears the backoff even when the window's target stays the same.
    runtime.backend.managed_outcome = None;
    let _ = runtime.execute(
        WorkerCommand::Gesture(
            FinalGesture::drop(stubborn.clone(), (750, 400)),
            false,
            outputs.clone(),
        ),
        &cancelled,
    );
    assert_eq!(runtime.backend.frames[&stubborn], target);
    runtime.cycle(outputs, &cancelled).unwrap();
    assert!(!runtime.apply_attempts[WorkspaceId::ONE.index()].contains_key(&stubborn));
    assert!(!runtime.constrained[WorkspaceId::ONE.index()].contains_key(&stubborn));
}

#[test]
fn delayed_async_bounce_after_matching_ticks_freezes_and_clears_on_target_change() {
    let (main, mini, settings) = (window(10), window(11), window(12));
    let initial = [
        (main.clone(), frame(50)),
        (mini.clone(), frame(350)),
        (settings.clone(), frame(650)),
    ];
    let mut runtime = runtime(FakeBackend {
        frames: initial.iter().cloned().collect(),
        observation: Some(observation_for_windows(
            "com.example.Electron",
            &settings,
            &initial,
        )),
        ..FakeBackend::default()
    });
    let (outputs, cancelled, ws) = (outputs(), AtomicBool::new(false), WorkspaceId::ONE.index());

    // Cycle 1 applies target; two early matching ticks must keep applied armed.
    runtime.cycle(outputs.clone(), &cancelled).unwrap();
    let target = runtime.backend.frames[&mini];
    for _ in 0..2 {
        runtime.cycle(outputs.clone(), &cancelled).unwrap();
        assert_eq!(runtime.backend.observed_apply_count, 1);
        assert!(runtime.applied[ws].contains_key(&mini));
    }

    // Fixed-height delayed snap-back settles on the first bounce without re-applying.
    let clamped = crate::Rect {
        height: 520,
        ..target
    };
    runtime.backend.frames.insert(mini.clone(), clamped);
    for _ in 0..2 {
        runtime.cycle(outputs.clone(), &cancelled).unwrap();
        assert_eq!(runtime.backend.observed_apply_count, 1);
        assert_eq!(runtime.constrained[ws].get(&mini), Some(&(target, clamped)));
        assert_eq!(runtime.apply_attempts[ws].get(&mini), Some(&(target, 1)));
    }

    // Varying-height delayed bounces (each preceded by matching early ticks)
    // still increment apply_attempts and freeze cleanly at APPLY_ATTEMPT_LIMIT.
    for height in [560, 480, 600, 440, 640, 420] {
        runtime
            .backend
            .frames
            .insert(mini.clone(), crate::Rect { height, ..target });
        let before = runtime.backend.observed_apply_count;
        runtime.cycle(outputs.clone(), &cancelled).unwrap();
        if runtime.backend.observed_apply_count > before {
            assert_eq!(runtime.backend.frames[&mini], target);
            runtime.cycle(outputs.clone(), &cancelled).unwrap();
            runtime.cycle(outputs.clone(), &cancelled).unwrap();
        }
    }
    let limit = usize::from(super::super::cycle::APPLY_ATTEMPT_LIMIT);
    assert_eq!(runtime.backend.observed_apply_count, limit);
    assert_eq!(
        runtime.apply_attempts[ws].get(&mini),
        Some(&(target, super::super::cycle::APPLY_ATTEMPT_LIMIT))
    );
    assert!(runtime.constrained[ws].contains_key(&mini));

    // Closing a sibling changes target, clearing the freeze and re-applying cleanly.
    runtime.backend.frames.remove(&settings);
    runtime.cycle(outputs.clone(), &cancelled).unwrap();
    assert_eq!(runtime.backend.observed_apply_count, limit + 1);
    assert_eq!(runtime.backend.frames[&mini].width, 500);
    assert_eq!(runtime.backend.frames[&mini].height, 800);
    assert!(!runtime.apply_attempts[ws].contains_key(&mini));
    assert!(!runtime.constrained[ws].contains_key(&mini));

    // Once ASYNC_BOUNCE_SETTLE elapses at the matching frame, applied clears cleanly.
    runtime.applied[ws].get_mut(&mini).unwrap().at =
        runtime.backend.now() - cycle::ASYNC_BOUNCE_SETTLE;
    runtime.cycle(outputs, &cancelled).unwrap();
    assert!(!runtime.applied[ws].contains_key(&mini));
}
