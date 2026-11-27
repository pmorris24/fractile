use super::*;

#[test]
fn switching_leaves_self_hidden_windows_alone_instead_of_failing() {
    let tiled = window(10);
    let stashed = window(11);
    let mut backend = FakeBackend {
        observation: Some(observation(&tiled, frame(50))),
        ..FakeBackend::default()
    };
    backend.frames.insert(tiled.clone(), frame(50));
    backend.frames.insert(stashed.clone(), frame(-4000));
    let mut runtime = runtime(backend);
    bind(&mut runtime, WorkspaceId::ONE, &tiled);
    runtime
        .workspaces
        .own_floating(stashed.clone(), WorkspaceId::ONE);

    runtime
        .switch(
            WorkspaceId::new(2).unwrap(),
            outputs(),
            &AtomicBool::new(false),
        )
        .unwrap();

    assert!(runtime.parked.contains_key(&tiled));
    assert!(!runtime.parked.contains_key(&stashed));
    assert_eq!(runtime.backend.frames[&stashed], frame(-4000));
}

#[test]
fn moving_to_an_inactive_workspace_does_not_switch() {
    let window = window(10);
    let mut backend = FakeBackend {
        focused: Some(window.clone()),
        observation: Some(observation(&window, frame(50))),
        ..FakeBackend::default()
    };
    backend.frames.insert(window.clone(), frame(50));
    let mut runtime = runtime(backend);
    bind(&mut runtime, WorkspaceId::ONE, &window);
    let target = WorkspaceId::new(2).unwrap();

    runtime
        .move_focused(target, outputs(), &AtomicBool::new(false))
        .unwrap();

    assert_eq!(
        runtime.workspaces.active_for(&DisplaySelector::Main),
        Some(WorkspaceId::ONE)
    );
    assert_eq!(runtime.workspaces.workspace_of(&window), Some(target));
    assert!(runtime.parked.contains_key(&window));
    assert!(
        !runtime
            .backend
            .calls
            .iter()
            .any(|call| matches!(call, BackendCall::Focus(_)))
    );

    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    assert_eq!(
        runtime.workspaces.active_for(&DisplaySelector::Main),
        Some(WorkspaceId::ONE)
    );
}

#[test]
fn focusing_a_parked_window_activates_its_workspace() {
    let window = window(10);
    let normal = frame(50);
    let outputs = outputs();
    let displays = outputs.frames().collect::<Vec<_>>();
    let parked_frame = crate::geometry::parked_frame(outputs.main().frame, &displays, normal);
    let mut backend = FakeBackend {
        focused: Some(window.clone()),
        ..FakeBackend::default()
    };
    backend.frames.insert(window.clone(), parked_frame);
    let mut parked = HashMap::new();
    parked.insert(window.clone(), normal);
    let mut runtime = WorkspaceRuntime::with_backend(ManagementScope::Both, backend, parked);
    let workspace = WorkspaceId::new(2).unwrap();
    bind(&mut runtime, workspace, &window);

    let expected = outputs.main().frame;
    let status = runtime.cycle(outputs, &AtomicBool::new(false)).unwrap();

    assert_eq!(status.active, vec![workspace, WorkspaceId::FIVE]);
    assert_eq!(runtime.backend.frames[&window], expected);
}

#[test]
fn frontmost_app_activates_before_ax_focus_catches_up() {
    let old = window(10);
    let current = crate::PhysicalWindowId {
        app: AppInstanceId {
            pid: 2,
            launched_at: 2,
        },
        window_id: 20,
    };
    let normal = frame(50);
    let outputs = outputs();
    let displays = outputs.frames().collect::<Vec<_>>();
    let parked_frame = crate::geometry::parked_frame(outputs.main().frame, &displays, normal);
    let mut backend = FakeBackend {
        focused: Some(old.clone()),
        frontmost: Some(current.app.clone()),
        ..FakeBackend::default()
    };
    backend.frames.insert(old.clone(), normal);
    backend.frames.insert(current.clone(), parked_frame);
    let mut parked = HashMap::new();
    parked.insert(current.clone(), normal);
    let mut runtime = WorkspaceRuntime::with_backend(ManagementScope::Both, backend, parked);
    bind(&mut runtime, WorkspaceId::ONE, &old);
    let target = WorkspaceId::new(2).unwrap();
    bind(&mut runtime, target, &current);

    let status = runtime.cycle(outputs, &AtomicBool::new(false)).unwrap();

    assert_eq!(status.active, vec![target, WorkspaceId::FIVE]);
}

#[test]
fn switching_reveals_and_lays_out_incoming_beneath_before_hiding_outgoing() {
    let outgoing = window(10);
    let incoming = window(20);
    let outgoing_normal = frame(50);
    let incoming_normal = frame(100);
    let outputs = outputs();
    let displays = outputs.frames().collect::<Vec<_>>();
    let incoming_parked =
        crate::geometry::parked_frame(outputs.main().frame, &displays, incoming_normal);
    let mut backend = FakeBackend::default();
    backend.frames.insert(outgoing.clone(), outgoing_normal);
    backend.frames.insert(incoming.clone(), incoming_parked);
    let mut parked = HashMap::new();
    parked.insert(incoming.clone(), incoming_normal);
    let mut runtime = WorkspaceRuntime::with_backend(ManagementScope::Both, backend, parked);
    bind(&mut runtime, WorkspaceId::ONE, &outgoing);
    let target = WorkspaceId::new(2).unwrap();
    bind(&mut runtime, target, &incoming);

    runtime
        .switch(target, outputs.clone(), &AtomicBool::new(false))
        .unwrap();

    assert_eq!(
        runtime.workspaces.active_for(&DisplaySelector::Main),
        Some(target)
    );
    assert_eq!(runtime.backend.focused, Some(incoming.clone()));
    assert!(runtime.parked.contains_key(&outgoing));
    assert!(!runtime.parked.contains_key(&incoming));
    let reveal = runtime
        .backend
        .calls
        .iter()
        .position(|call| {
            matches!(call, BackendCall::SetPosition(window, frame) if window == &incoming && frame == &incoming_normal)
        })
        .unwrap();
    let layout = runtime
        .backend
        .calls
        .iter()
        .position(|call| matches!(call, BackendCall::Apply(window, _) if window == &incoming))
        .unwrap();
    let hide = runtime
        .backend
        .calls
        .iter()
        .position(|call| matches!(call, BackendCall::SetPosition(window, _) if window == &outgoing))
        .unwrap();
    // The incoming workspace is fully formed beneath the outgoing windows
    // before anything is hidden, so the desktop never shows.
    assert!(reveal < layout && layout < hide);
}

#[test]
fn switching_away_and_back_restores_last_focused_window() {
    let first = window(10);
    let focused = window(20);
    let incoming = window(30);
    let outputs = outputs();
    let displays = outputs.frames().collect::<Vec<_>>();
    let incoming_normal = frame(100);
    let incoming_parked =
        crate::geometry::parked_frame(outputs.main().frame, &displays, incoming_normal);
    let mut backend = FakeBackend {
        focused: Some(focused.clone()),
        ..FakeBackend::default()
    };
    backend.frames.insert(first.clone(), frame(50));
    backend.frames.insert(focused.clone(), frame(500));
    backend.frames.insert(incoming.clone(), incoming_parked);
    let mut parked = HashMap::new();
    parked.insert(incoming.clone(), incoming_normal);
    let mut runtime = WorkspaceRuntime::with_backend(ManagementScope::Both, backend, parked);
    bind(&mut runtime, WorkspaceId::ONE, &first);
    bind(&mut runtime, WorkspaceId::ONE, &focused);
    let target = WorkspaceId::new(2).unwrap();
    bind(&mut runtime, target, &incoming);
    let cancelled = AtomicBool::new(false);

    runtime.switch(target, outputs.clone(), &cancelled).unwrap();
    runtime
        .switch(WorkspaceId::ONE, outputs.clone(), &cancelled)
        .unwrap();

    assert_eq!(runtime.backend.focused, Some(focused));
}

#[test]
fn selecting_an_active_workspace_focuses_its_window() {
    let target = window(10);
    let other_display = window(20);
    let mut backend = FakeBackend {
        focused: Some(other_display.clone()),
        ..FakeBackend::default()
    };
    backend.frames.insert(target.clone(), frame(50));
    backend.frames.insert(other_display.clone(), frame(1050));
    let mut runtime = runtime(backend);
    bind(&mut runtime, WorkspaceId::ONE, &target);
    bind(&mut runtime, WorkspaceId::SIX, &other_display);

    runtime
        .switch(WorkspaceId::ONE, outputs(), &AtomicBool::new(false))
        .unwrap();

    assert_eq!(runtime.backend.focused, Some(target.clone()));
    assert!(matches!(
        runtime.backend.calls.last(),
        Some(BackendCall::Focus(window)) if window == &target
    ));
}

#[test]
fn selecting_an_active_split_workspace_from_elsewhere_restores_its_remembered_focus() {
    let first = window(10);
    let remembered = window(20);
    let elsewhere = window(30);
    let mut backend = FakeBackend {
        focused: Some(elsewhere.clone()),
        ..FakeBackend::default()
    };
    backend.frames.insert(first.clone(), frame(50));
    backend.frames.insert(remembered.clone(), frame(500));
    backend.frames.insert(elsewhere.clone(), frame(1050));
    let mut runtime = runtime(backend);
    bind(&mut runtime, WorkspaceId::ONE, &first);
    bind(&mut runtime, WorkspaceId::ONE, &remembered);
    bind(&mut runtime, WorkspaceId::SIX, &elsewhere);
    runtime
        .workspaces
        .remember_focus(WorkspaceId::ONE, &remembered);

    runtime
        .switch(WorkspaceId::ONE, outputs(), &AtomicBool::new(false))
        .unwrap();

    assert_eq!(runtime.backend.focused, Some(remembered));
}

#[test]
fn switching_to_an_unconfigured_named_workspace_is_rejected() {
    let mut runtime = runtime(FakeBackend::default());

    let Err(error) = runtime.switch(
        WorkspaceId::named('Z').unwrap(),
        outputs(),
        &AtomicBool::new(false),
    ) else {
        panic!("unconfigured workspace unexpectedly switched");
    };

    assert_eq!(error, "Workspace Z is not configured");
}

#[test]
fn workspace_back_and_forth_bounces_between_the_last_two_focused_workspaces() {
    let main = window(10);
    let main_target = window(11);
    let secondary = window(20);
    let main_target_normal = frame(100);
    let mut backend = FakeBackend {
        focused: Some(main.clone()),
        ..FakeBackend::default()
    };
    backend.frames.insert(main.clone(), frame(50));
    backend
        .frames
        .insert(main_target.clone(), main_parked_frame(main_target_normal));
    backend.frames.insert(secondary.clone(), frame(1050));
    let mut runtime = runtime_with_parked(backend, [(main_target.clone(), main_target_normal)]);
    bind(&mut runtime, WorkspaceId::ONE, &main);
    bind(&mut runtime, WorkspaceId::new(2).unwrap(), &main_target);
    bind(&mut runtime, WorkspaceId::SIX, &secondary);
    let main_target_ws = WorkspaceId::new(2).unwrap();
    let cancelled = AtomicBool::new(false);

    runtime
        .switch(main_target_ws, outputs(), &cancelled)
        .unwrap();
    runtime
        .switch(WorkspaceId::SIX, outputs(), &cancelled)
        .unwrap();
    assert_eq!(runtime.backend.focused, Some(secondary.clone()));

    // Workspace 2 is still on Main, so Option+Tab moves focus there and leaves
    // every display showing what it was showing.
    runtime
        .workspace_back_and_forth(outputs(), &cancelled)
        .unwrap();
    assert_eq!(runtime.backend.focused, Some(main_target.clone()));
    assert_eq!(
        runtime.workspaces.active_for(&DisplaySelector::Main),
        Some(main_target_ws)
    );
    assert_eq!(
        runtime.workspaces.active_for(&DisplaySelector::Secondary),
        Some(WorkspaceId::SIX)
    );
    assert!(!runtime.parked.contains_key(&main_target));
    assert!(!runtime.parked.contains_key(&secondary));

    // A second press bounces back to workspace 6 on Secondary.
    runtime
        .workspace_back_and_forth(outputs(), &cancelled)
        .unwrap();
    assert_eq!(runtime.backend.focused, Some(secondary));
    assert_eq!(
        runtime.workspaces.active_for(&DisplaySelector::Main),
        Some(main_target_ws)
    );
    assert_eq!(
        runtime.workspaces.active_for(&DisplaySelector::Secondary),
        Some(WorkspaceId::SIX)
    );
}

#[test]
fn workspace_back_and_forth_on_one_display_switches_like_before() {
    let first = window(10);
    let second = window(20);
    let second_normal = frame(100);
    let single = macos::DisplayOutputs::new(vec![outputs().main().clone()]).unwrap();
    let mut backend = FakeBackend {
        focused: Some(first.clone()),
        ..FakeBackend::default()
    };
    backend.frames.insert(first.clone(), frame(50));
    backend
        .frames
        .insert(second.clone(), main_parked_frame(second_normal));
    let mut runtime = runtime_with_parked(backend, [(second.clone(), second_normal)]);
    bind(&mut runtime, WorkspaceId::ONE, &first);
    let target = WorkspaceId::new(2).unwrap();
    bind(&mut runtime, target, &second);
    let cancelled = AtomicBool::new(false);

    let Err(error) = runtime.workspace_back_and_forth(single.clone(), &cancelled) else {
        panic!("back-and-forth found a previous workspace without one being focused");
    };
    assert_eq!(error, "No previous workspace");

    runtime.switch(target, single.clone(), &cancelled).unwrap();
    assert_eq!(runtime.backend.focused, Some(second.clone()));

    runtime
        .workspace_back_and_forth(single.clone(), &cancelled)
        .unwrap();
    assert_eq!(
        runtime.workspaces.active_for(&DisplaySelector::Main),
        Some(WorkspaceId::ONE)
    );
    assert_eq!(runtime.backend.focused, Some(first));
}

#[test]
fn the_hud_gives_each_display_its_own_row_and_only_the_switched_one_moves() {
    let main = window(10);
    let secondary = window(20);
    let mut backend = FakeBackend {
        focused: Some(secondary.clone()),
        ..FakeBackend::default()
    };
    backend.frames.insert(main.clone(), frame(50));
    backend.frames.insert(secondary.clone(), frame(1050));
    let mut runtime = runtime(backend);
    bind(&mut runtime, WorkspaceId::ONE, &main);
    bind(&mut runtime, WorkspaceId::FIVE, &secondary);
    let cancelled = AtomicBool::new(false);

    let target = WorkspaceId::new(7).unwrap();
    let hud = runtime
        .switch(target, outputs(), &cancelled)
        .unwrap()
        .hud
        .unwrap();
    let rows: Vec<_> = hud
        .displays
        .iter()
        .map(|display| {
            (
                display.id.as_str(),
                display.visible,
                display.from,
                display
                    .workspaces
                    .iter()
                    .map(|entry| entry.workspace)
                    .collect::<Vec<_>>(),
            )
        })
        .collect();
    assert_eq!(
        rows,
        vec![
            // Each display lists every one of its workspaces, empty ones included.
            (
                "main-id",
                Some(WorkspaceId::ONE),
                None,
                (1..=4).map(|n| WorkspaceId::new(n).unwrap()).collect()
            ),
            (
                "secondary-id",
                Some(target),
                Some(WorkspaceId::FIVE),
                (5..=10).map(|n| WorkspaceId::new(n).unwrap()).collect()
            ),
        ]
    );
    assert_eq!(
        hud.display_of(target).map(|display| display.id.as_str()),
        Some("secondary-id")
    );
}

#[test]
fn switching_aborts_when_the_incoming_workspace_cannot_be_laid_out() {
    let outgoing = window(10);
    let incoming = window(20);
    let outgoing_normal = frame(50);
    let incoming_normal = frame(100);
    let outputs = outputs();
    let displays = outputs.frames().collect::<Vec<_>>();
    let incoming_parked =
        crate::geometry::parked_frame(outputs.main().frame, &displays, incoming_normal);
    let mut backend = FakeBackend {
        managed_outcome: Some(macos::ApplyOutcome::Unavailable),
        ..FakeBackend::default()
    };
    backend.frames.insert(outgoing.clone(), outgoing_normal);
    backend.frames.insert(incoming.clone(), incoming_parked);
    let mut parked = HashMap::new();
    parked.insert(incoming.clone(), incoming_normal);
    let mut runtime = WorkspaceRuntime::with_backend(ManagementScope::Both, backend, parked);
    bind(&mut runtime, WorkspaceId::ONE, &outgoing);
    let target = WorkspaceId::new(2).unwrap();
    bind(&mut runtime, target, &incoming);

    assert!(
        runtime
            .switch(target, outputs.clone(), &AtomicBool::new(false))
            .is_err()
    );
    assert_eq!(
        runtime.workspaces.active_for(&DisplaySelector::Main),
        Some(WorkspaceId::ONE)
    );
    assert_eq!(runtime.backend.frames[&outgoing], outgoing_normal);
    assert!(!runtime.parked.contains_key(&outgoing));
    assert!(runtime.parked.contains_key(&incoming));
}

#[test]
fn moving_a_floating_window_to_another_workspace_parks_and_transfers_ownership() {
    let floating = window(10);
    let mut backend = FakeBackend {
        focused: Some(floating.clone()),
        ..FakeBackend::default()
    };
    backend.frames.insert(floating.clone(), frame(100));
    let mut runtime = runtime(backend);
    runtime
        .workspaces
        .own_floating(floating.clone(), WorkspaceId::ONE);
    let target = WorkspaceId::new(2).unwrap();

    runtime
        .move_focused(target, outputs(), &AtomicBool::new(false))
        .unwrap();

    assert_eq!(
        runtime.workspaces.floating_workspace_of(&floating),
        Some(target)
    );
    assert!(runtime.parked.contains_key(&floating));
    assert!(
        !outputs()
            .main()
            .frame
            .contains_center(runtime.backend.frames[&floating])
    );
}

#[test]
fn focusing_an_unmanaged_window_of_a_multi_window_app_does_not_switch_workspaces() {
    let main_window = window(10);
    let secondary_window = window(11);
    let parked_frame = main_parked_frame(frame(50));
    let mut backend = FakeBackend {
        focused: Some(secondary_window.clone()),
        frontmost: Some(secondary_window.app.clone()),
        observation: Some(observation(&secondary_window, frame(200))),
        ..FakeBackend::default()
    };
    backend.frames.insert(main_window.clone(), parked_frame);
    backend.frames.insert(secondary_window.clone(), frame(200));
    let mut runtime = runtime_with_parked(backend, [(main_window.clone(), frame(50))]);
    let hidden = WorkspaceId::new(2).unwrap();
    bind(&mut runtime, hidden, &main_window);

    let status = runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    assert_eq!(status.active, vec![WorkspaceId::ONE, WorkspaceId::FIVE]);
    assert_eq!(
        runtime.workspaces.workspace_of(&secondary_window),
        Some(WorkspaceId::ONE)
    );
    assert_eq!(runtime.workspaces.workspace_of(&main_window), Some(hidden));
}

#[test]
fn switch_rollback_preserves_original_recovery_frames_and_suppresses_focus_loop() {
    let outgoing = window(10);
    let incoming_first = window(20);
    let incoming_second = window(30);
    let outputs = outputs();
    let first_original = frame(100);
    let second_original = frame(200);
    let first_parked = main_parked_frame(first_original);
    let second_parked = main_parked_frame(second_original);
    let mut backend = FakeBackend {
        focused: Some(incoming_first.clone()),
        frontmost: Some(incoming_first.app.clone()),
        observation: Some(observation(&outgoing, frame(50))),
        managed_outcome: Some(macos::ApplyOutcome::Unavailable),
        ..FakeBackend::default()
    };
    backend.frames.insert(outgoing.clone(), frame(50));
    backend.frames.insert(incoming_first.clone(), first_parked);
    backend
        .frames
        .insert(incoming_second.clone(), second_parked);
    let mut runtime = runtime_with_parked(
        backend,
        [
            (incoming_first.clone(), first_original),
            (incoming_second.clone(), second_original),
        ],
    );
    bind(&mut runtime, WorkspaceId::ONE, &outgoing);
    let target = WorkspaceId::new(2).unwrap();
    bind(&mut runtime, target, &incoming_first);
    bind(&mut runtime, target, &incoming_second);

    // First cycle attempts focus-follow -> switch to workspace 2, which rolls back
    // when layout_workspace returns Unavailable.
    assert!(
        runtime
            .cycle(outputs.clone(), &AtomicBool::new(false))
            .is_err()
    );
    assert_eq!(runtime.parked.get(&incoming_first), Some(&first_original));
    assert_eq!(runtime.parked.get(&incoming_second), Some(&second_original));
    assert_eq!(
        runtime.focus.guarded_window().cloned(),
        Some(incoming_first)
    );

    // Next cycle does not loop into another focus-follow switch while focus is unchanged.
    runtime.backend.managed_outcome = None;
    let status = runtime.cycle(outputs, &AtomicBool::new(false)).unwrap();
    assert_eq!(status.active, vec![WorkspaceId::ONE, WorkspaceId::FIVE]);
}

#[test]
fn status_menu_lists_every_workspace_of_each_display_with_its_apps() {
    let app_window = |pid, window_id| crate::PhysicalWindowId {
        app: AppInstanceId {
            pid,
            launched_at: 1,
        },
        window_id,
    };
    let mut runtime = runtime(FakeBackend::default());
    bind(&mut runtime, WorkspaceId::ONE, &app_window(7, 10));
    bind(&mut runtime, WorkspaceId::ONE, &app_window(8, 11));
    bind(&mut runtime, WorkspaceId::ONE, &app_window(7, 12));
    let three = WorkspaceId::THREE;
    runtime.workspaces.own_floating(app_window(9, 13), three);

    let menu = runtime.status(0, &outputs()).menu;

    let entry = |workspace, current, apps: &[i32]| status_item::MenuWorkspace {
        workspace,
        current,
        apps: apps.to_vec(),
    };
    let id = |number| WorkspaceId::new(number).unwrap();
    // Two displays: the tertiary workspaces 8-10 show on the secondary display, so its menu
    // section lists them too.
    assert_eq!(
        menu,
        vec![
            status_item::DisplayWorkspaces {
                display_id: "main-id".into(),
                display: "Main".into(),
                workspaces: vec![
                    entry(WorkspaceId::ONE, true, &[7, 8, 7]),
                    entry(id(2), false, &[]),
                    entry(three, false, &[9]),
                    entry(id(4), false, &[]),
                ],
            },
            status_item::DisplayWorkspaces {
                display_id: "secondary-id".into(),
                display: "Secondary".into(),
                workspaces: (5..=10)
                    .map(|number| entry(id(number), number == 5, &[]))
                    .collect(),
            },
        ]
    );

    runtime.workspaces.switch(three);
    let menu = runtime.status(0, &outputs()).menu;

    assert_eq!(
        menu[0].workspaces,
        vec![
            entry(WorkspaceId::ONE, false, &[7, 8, 7]),
            entry(id(2), false, &[]),
            entry(three, true, &[9]),
            entry(id(4), false, &[]),
        ]
    );
}

#[test]
fn swipes_step_through_the_occupied_workspaces_of_the_display_under_the_pointer() {
    let tiled = window(10);
    let floating = window(11);
    let mut backend = FakeBackend {
        observation: Some(observation(&tiled, frame(50))),
        ..FakeBackend::default()
    };
    backend.frames.insert(tiled.clone(), frame(50));
    backend.frames.insert(floating.clone(), frame(60));
    let mut runtime = runtime(backend);
    bind(&mut runtime, WorkspaceId::ONE, &tiled);
    runtime
        .workspaces
        .own_floating(floating, WorkspaceId::THREE);
    let main_display = (500, 400);
    let swipe = |runtime: &mut WorkspaceRuntime<FakeBackend>, swipe, point| {
        runtime
            .swipe(
                swipe,
                Fingers::Three,
                point,
                outputs(),
                &AtomicBool::new(false),
            )
            .unwrap()
            .active
    };

    assert_eq!(
        swipe(&mut runtime, Swipe::Left, main_display),
        [WorkspaceId::THREE, WorkspaceId::FIVE]
    );
    assert_eq!(
        swipe(&mut runtime, Swipe::Left, main_display),
        [WorkspaceId::THREE, WorkspaceId::FIVE]
    );
    assert_eq!(
        swipe(&mut runtime, Swipe::Left, (1500, 400)),
        [WorkspaceId::THREE, WorkspaceId::FIVE]
    );
    assert_eq!(
        swipe(&mut runtime, Swipe::Right, main_display),
        [WorkspaceId::ONE, WorkspaceId::FIVE]
    );
}

fn app_window(pid: i32, window_id: u32) -> crate::PhysicalWindowId {
    crate::PhysicalWindowId {
        app: AppInstanceId {
            pid,
            launched_at: 1,
        },
        window_id,
    }
}

/// One complete application per window, so each window belongs to its own app.
fn observation_for_apps(
    windows: &[(crate::PhysicalWindowId, crate::Rect)],
) -> crate::NativeObservation {
    crate::NativeObservation {
        cycle: 0,
        mouse_down: false,
        applications: windows
            .iter()
            .map(|(window, frame)| crate::AppObservation::Complete {
                app: crate::ObservedApplication {
                    id: window.app.clone(),
                    bundle_id: format!("com.example.App{}", window.app.pid),
                },
                focused: Some(window.clone()),
                windows: vec![crate::ObservedWindow {
                    id: window.clone(),
                    frame: *frame,
                    resizable: true,
                    title: None,
                    document: None,
                }],
            })
            .collect(),
    }
}

fn main_workspace(runtime: &WorkspaceRuntime<FakeBackend>) -> Option<WorkspaceId> {
    runtime.workspaces.active_for(&DisplaySelector::Main)
}

#[test]
fn switching_across_empty_workspaces_does_not_snap_back_to_workspace_one() {
    let first = app_window(1, 10);
    let mut backend = FakeBackend {
        focused: Some(first.clone()),
        frontmost: Some(first.app.clone()),
        observation: Some(observation_for_apps(&[(first.clone(), frame(50))])),
        ..FakeBackend::default()
    };
    backend.frames.insert(first.clone(), frame(50));
    let mut runtime = runtime(backend);
    bind(&mut runtime, WorkspaceId::ONE, &first);
    let four = WorkspaceId::new(4).unwrap();
    let cancelled = AtomicBool::new(false);

    runtime
        .switch(WorkspaceId::THREE, outputs(), &cancelled)
        .unwrap();
    runtime.cycle(outputs(), &cancelled).unwrap();
    assert_eq!(main_workspace(&runtime), Some(WorkspaceId::THREE));

    // Workspace 3 is empty, so the hidden window on 1 still has focus.
    runtime.switch(four, outputs(), &cancelled).unwrap();
    runtime.cycle(outputs(), &cancelled).unwrap();
    runtime.cycle(outputs(), &cancelled).unwrap();

    assert_eq!(main_workspace(&runtime), Some(four));
    assert!(runtime.parked.contains_key(&first));
}

#[test]
fn switching_when_focused_window_is_none_but_frontmost_is_workspace_one_does_not_snap_back() {
    let first = app_window(1, 10);
    let mut backend = FakeBackend {
        focused: Some(first.clone()),
        frontmost: Some(first.app.clone()),
        observation: Some(observation_for_apps(&[(first.clone(), frame(50))])),
        ..FakeBackend::default()
    };
    backend.frames.insert(first.clone(), frame(50));
    let mut runtime = runtime(backend);
    bind(&mut runtime, WorkspaceId::ONE, &first);
    let four = WorkspaceId::new(4).unwrap();
    let cancelled = AtomicBool::new(false);

    runtime
        .switch(WorkspaceId::THREE, outputs(), &cancelled)
        .unwrap();
    // Accessibility loses the parked window but its app stays frontmost.
    runtime.backend.focused = None;
    runtime.cycle(outputs(), &cancelled).unwrap();
    assert_eq!(main_workspace(&runtime), Some(WorkspaceId::THREE));

    runtime.switch(four, outputs(), &cancelled).unwrap();
    runtime.cycle(outputs(), &cancelled).unwrap();
    assert_eq!(main_workspace(&runtime), Some(four));

    // The window reports focus again: still the same stale focus.
    runtime.backend.focused = Some(first.clone());
    runtime.cycle(outputs(), &cancelled).unwrap();
    assert_eq!(main_workspace(&runtime), Some(four));
}

#[test]
fn switching_with_async_focus_lag_does_not_snap_back_to_previous_workspace() {
    let two = WorkspaceId::new(2).unwrap();
    let four = WorkspaceId::new(4).unwrap();
    let on_two = app_window(2, 20);
    let on_three = app_window(3, 30);
    let on_four = app_window(4, 40);
    let mut backend = FakeBackend {
        observation: Some(observation_for_apps(&[
            (on_two.clone(), frame(100)),
            (on_three.clone(), frame(50)),
            (on_four.clone(), frame(200)),
        ])),
        ..FakeBackend::default()
    };
    backend
        .frames
        .insert(on_two.clone(), main_parked_frame(frame(100)));
    backend
        .frames
        .insert(on_three.clone(), main_parked_frame(frame(50)));
    backend
        .frames
        .insert(on_four.clone(), main_parked_frame(frame(200)));
    let mut runtime = runtime_with_parked(
        backend,
        [
            (on_two.clone(), frame(100)),
            (on_three.clone(), frame(50)),
            (on_four.clone(), frame(200)),
        ],
    );
    bind(&mut runtime, two, &on_two);
    bind(&mut runtime, WorkspaceId::THREE, &on_three);
    bind(&mut runtime, four, &on_four);
    let cancelled = AtomicBool::new(false);
    runtime
        .switch(WorkspaceId::THREE, outputs(), &cancelled)
        .unwrap();
    assert_eq!(runtime.backend.focused, Some(on_three.clone()));

    runtime.switch(four, outputs(), &cancelled).unwrap();
    assert!(
        runtime
            .backend
            .calls
            .contains(&BackendCall::Focus(on_four.clone()))
    );
    // macOS activates the incoming app asynchronously: the next cycles still
    // see the window from workspace 3, first without its app, then with it.
    runtime.backend.focused = None;
    runtime.backend.frontmost = Some(on_three.app.clone());
    runtime.cycle(outputs(), &cancelled).unwrap();
    assert_eq!(main_workspace(&runtime), Some(four));
    runtime.backend.focused = Some(on_three.clone());
    runtime.cycle(outputs(), &cancelled).unwrap();
    assert_eq!(main_workspace(&runtime), Some(four));

    // Focus lands on workspace 4 and the guard clears.
    runtime.backend.focused = Some(on_four.clone());
    runtime.backend.frontmost = Some(on_four.app.clone());
    runtime.cycle(outputs(), &cancelled).unwrap();
    assert_eq!(main_workspace(&runtime), Some(four));
    assert_eq!(runtime.focus.guarded_window().cloned(), None);
    assert_eq!(runtime.focus.guarded_app().cloned(), None);

    // Activating an app on workspace 2 is intentional and still followed.
    runtime.backend.focused = Some(on_two.clone());
    runtime.backend.frontmost = Some(on_two.app.clone());
    runtime.cycle(outputs(), &cancelled).unwrap();
    assert_eq!(main_workspace(&runtime), Some(two));
}

#[test]
fn activating_another_app_after_switching_to_an_empty_workspace_still_follows_it() {
    let two = WorkspaceId::new(2).unwrap();
    let four = WorkspaceId::new(4).unwrap();
    let first = app_window(1, 10);
    let on_two = app_window(2, 20);
    let mut backend = FakeBackend {
        focused: Some(first.clone()),
        frontmost: Some(first.app.clone()),
        observation: Some(observation_for_apps(&[
            (first.clone(), frame(50)),
            (on_two.clone(), frame(100)),
        ])),
        ..FakeBackend::default()
    };
    backend.frames.insert(first.clone(), frame(50));
    backend
        .frames
        .insert(on_two.clone(), main_parked_frame(frame(100)));
    let mut runtime = runtime_with_parked(backend, [(on_two.clone(), frame(100))]);
    bind(&mut runtime, WorkspaceId::ONE, &first);
    bind(&mut runtime, two, &on_two);
    let cancelled = AtomicBool::new(false);

    runtime.switch(four, outputs(), &cancelled).unwrap();
    runtime.cycle(outputs(), &cancelled).unwrap();
    assert_eq!(main_workspace(&runtime), Some(four));

    // Cmd+Tab: the app on workspace 2 is frontmost before Accessibility
    // reports its window, and focus follows it.
    runtime.backend.frontmost = Some(on_two.app.clone());
    runtime.cycle(outputs(), &cancelled).unwrap();
    assert_eq!(main_workspace(&runtime), Some(two));
}

#[test]
fn reactivating_the_stale_app_after_another_app_follows_it_back() {
    let four = WorkspaceId::new(4).unwrap();
    let first = app_window(1, 10);
    let elsewhere = app_window(9, 90);
    let mut backend = FakeBackend {
        focused: Some(first.clone()),
        frontmost: Some(first.app.clone()),
        observation: Some(observation_for_apps(&[(first.clone(), frame(50))])),
        ..FakeBackend::default()
    };
    backend.frames.insert(first.clone(), frame(50));
    let mut runtime = runtime(backend);
    bind(&mut runtime, WorkspaceId::ONE, &first);
    let cancelled = AtomicBool::new(false);

    runtime.switch(four, outputs(), &cancelled).unwrap();
    runtime.cycle(outputs(), &cancelled).unwrap();
    assert_eq!(main_workspace(&runtime), Some(four));

    // An unmanaged app takes focus, then the user Cmd+Tabs back.
    runtime.backend.focused = Some(elsewhere.clone());
    runtime.backend.frontmost = Some(elsewhere.app.clone());
    runtime.cycle(outputs(), &cancelled).unwrap();
    assert_eq!(main_workspace(&runtime), Some(four));
    runtime.backend.focused = Some(first.clone());
    runtime.backend.frontmost = Some(first.app.clone());
    runtime.cycle(outputs(), &cancelled).unwrap();
    assert_eq!(main_workspace(&runtime), Some(WorkspaceId::ONE));
}

#[test]
fn reveal_confirms_windows_that_already_arrived_without_sleeping_or_rewriting() {
    let incoming = window(20);
    let normal = frame(100);
    let mut backend = FakeBackend::default();
    // The app applied the restore synchronously: the window is already back.
    backend.frames.insert(incoming.clone(), normal);
    let mut runtime = WorkspaceRuntime::with_backend(
        ManagementScope::Both,
        backend,
        HashMap::from([(incoming.clone(), normal)]),
    );

    let started = Instant::now();
    assert!(runtime.wait_for_reveal(std::slice::from_ref(&incoming), &AtomicBool::new(false)));

    // The old loop always slept 10 ms before its first check.
    assert!(started.elapsed() < Duration::from_millis(10));
    assert!(!runtime.parked.contains_key(&incoming));
    assert!(
        !runtime
            .backend
            .calls
            .iter()
            .any(|call| matches!(call, BackendCall::SetPosition(..)))
    );
}

#[test]
fn reveal_confirms_a_window_missing_from_the_onscreen_list_from_the_full_list() {
    let incoming = window(20);
    let normal = frame(100);
    let mut backend = FakeBackend::default();
    backend.frames.insert(incoming.clone(), normal);
    // Another Space or ordered out: only the full window list has it.
    backend.ordered_out.insert(incoming.window_id);
    let mut runtime = WorkspaceRuntime::with_backend(
        ManagementScope::Both,
        backend,
        HashMap::from([(incoming.clone(), normal)]),
    );

    assert!(runtime.wait_for_reveal(std::slice::from_ref(&incoming), &AtomicBool::new(false)));
    assert!(!runtime.parked.contains_key(&incoming));
}
