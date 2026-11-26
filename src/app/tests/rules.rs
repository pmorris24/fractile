use super::*;

#[test]
fn new_ghostty_window_opens_on_its_current_display_active_workspace() {
    let existing = window(10);
    let created = window(11);
    let main_frame = frame(50);
    let secondary_frame = frame(1050);
    let mut backend = FakeBackend {
        focused: Some(created.clone()),
        frontmost: Some(created.app.clone()),
        observation: Some(observation_for_windows(
            "com.mitchellh.ghostty",
            &created,
            &[
                (existing.clone(), main_frame),
                (created.clone(), secondary_frame),
            ],
        )),
        ..FakeBackend::default()
    };
    backend.frames.insert(existing.clone(), main_frame);
    backend.frames.insert(created.clone(), secondary_frame);
    let mut runtime = runtime(backend);
    bind(&mut runtime, WorkspaceId::ONE, &existing);

    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    assert_eq!(
        runtime.workspaces.workspace_of(&existing),
        Some(WorkspaceId::ONE)
    );
    assert_eq!(
        runtime.workspaces.workspace_of(&created),
        Some(WorkspaceId::FIVE)
    );
    assert!(runtime.backend.frames[&created].x >= 1000);
}

#[test]
fn bundle_route_applies_to_initial_window_while_secondary_windows_stay_on_active_workspace() {
    let existing = window(10);
    let created = window(11);
    let mut backend = FakeBackend {
        observation: Some(observation_for_windows(
            "com.example.Test",
            &created,
            &[(existing.clone(), frame(50)), (created.clone(), frame(100))],
        )),
        ..FakeBackend::default()
    };
    backend.frames.insert(existing.clone(), frame(50));
    backend.frames.insert(created.clone(), frame(100));
    let config = Config::parse(
        "version = 1\n[[rules]]\nbundle_id = 'com.example.Test'\naction = 'route'\nworkspace = 6",
    )
    .unwrap();
    let mut runtime = runtime_with_config(backend, config);
    bind(&mut runtime, WorkspaceId::ONE, &existing);

    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    assert_eq!(
        runtime.workspaces.workspace_of(&existing),
        Some(WorkspaceId::ONE)
    );
    assert_eq!(
        runtime.workspaces.workspace_of(&created),
        Some(WorkspaceId::ONE)
    );
    assert!(runtime.backend.frames[&created].x < 1000);
}

/// Where a second window of `com.example.Test` lands, beside one already tiled on workspace 1,
/// under `config`'s rules. Asserts the tiled window stays put.
fn second_window_home(config: &str) -> Option<WorkspaceId> {
    let tiled = window(20);
    let opened = window(21);
    let mut backend = FakeBackend {
        observation: Some(observation_for_windows(
            "com.example.Test",
            &opened,
            &[(tiled.clone(), frame(50)), (opened.clone(), frame(100))],
        )),
        ..FakeBackend::default()
    };
    backend.frames.insert(tiled.clone(), frame(50));
    backend.frames.insert(opened.clone(), frame(100));
    let mut runtime = runtime_with_config(backend, Config::parse(config).unwrap());
    bind(&mut runtime, WorkspaceId::ONE, &tiled);
    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();
    assert_eq!(
        runtime.workspaces.workspace_of(&tiled),
        Some(WorkspaceId::ONE)
    );
    runtime.workspaces.workspace_of(&opened)
}

#[test]
fn windows_all_sends_a_later_window_to_the_route_once_its_lab_is_on() {
    const RULE: &str =
        "[[rules]]\nbundle_id = 'com.example.Test'\naction = 'route'\nworkspace = 6\n";
    const LAB: &str = "[labs]\nroute_all_windows = true\n";
    let cases = [
        ("", "windows = 'all'", WorkspaceId::ONE),
        (LAB, "windows = 'first'", WorkspaceId::ONE),
        (LAB, "", WorkspaceId::ONE),
        (LAB, "windows = 'all'", WorkspaceId::SIX),
    ];
    for (lab, windows, home) in cases {
        let config = format!("version = 1\n{lab}{RULE}{windows}\n");
        assert_eq!(second_window_home(&config), Some(home), "{config}");
    }
}

#[test]
fn title_route_rule_routes_matching_secondary_window() {
    let existing = window(10);
    let created = window(11);
    let mut observed = observation_for_windows(
        "com.example.Test",
        &created,
        &[(existing.clone(), frame(50)), (created.clone(), frame(100))],
    );
    if let crate::AppObservation::Complete { windows, .. } = &mut observed.applications[0] {
        windows[1].title = Some("Huddle".into());
    }
    let mut backend = FakeBackend {
        observation: Some(observed),
        ..FakeBackend::default()
    };
    backend.frames.insert(existing.clone(), frame(50));
    backend.frames.insert(created.clone(), frame(100));
    let config = Config::parse(
        "version = 1\n[[rules]]\nbundle_id = 'com.example.Test'\ntitle = 'Huddle'\naction = 'route'\nworkspace = 5",
    )
    .unwrap();
    let mut runtime = runtime_with_config(backend, config);
    bind(&mut runtime, WorkspaceId::ONE, &existing);

    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    assert_eq!(
        runtime.workspaces.workspace_of(&existing),
        Some(WorkspaceId::ONE)
    );
    assert_eq!(
        runtime.workspaces.workspace_of(&created),
        Some(WorkspaceId::FIVE)
    );
    assert!(runtime.backend.frames[&created].x >= 1000);
}

/// Routes a new `Huddle` window from the main display into visible workspace 5 (Columns),
/// which already tiles `tiled` and is first arranged by `arrange`. Returns the routed window.
fn route_huddle_into_columns(
    tiled: &[crate::PhysicalWindowId],
    arrange: impl FnOnce(&mut WorkspaceRuntime<FakeBackend>),
) -> (WorkspaceRuntime<FakeBackend>, crate::PhysicalWindowId) {
    let created = window(40);
    let mut windows: Vec<_> = tiled
        .iter()
        .zip((1050..).step_by(250))
        .map(|(tiled, x)| (tiled.clone(), frame(x)))
        .collect();
    windows.push((created.clone(), frame(100)));
    let mut observed = observation_for_windows("com.example.Test", &created, &windows);
    if let crate::AppObservation::Complete {
        windows: observed_windows,
        ..
    } = &mut observed.applications[0]
    {
        observed_windows.last_mut().unwrap().title = Some("Huddle".into());
    }
    let mut backend = FakeBackend {
        observation: Some(observed),
        ..FakeBackend::default()
    };
    backend.frames.extend(windows);
    let config = Config::parse(
        "version = 1\n[settings]\nlayout = 'columns'\n[[rules]]\nbundle_id = 'com.example.Test'\ntitle = 'Huddle'\naction = 'route'\nworkspace = 5",
    )
    .unwrap();
    let mut runtime = runtime_with_config(backend, config);
    for window in tiled {
        bind(&mut runtime, WorkspaceId::FIVE, window);
    }
    arrange(&mut runtime);

    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    assert_eq!(
        runtime.workspaces.workspace_of(&created),
        Some(WorkspaceId::FIVE)
    );
    (runtime, created)
}

#[test]
fn columns_route_into_a_visible_workspace_joins_its_focused_tile_split() {
    let (left, right) = (window(30), window(31));
    let (runtime, created) = route_huddle_into_columns(&[left.clone(), right.clone()], |rt| {
        rt.workspaces
            .engine_mut(WorkspaceId::FIVE)
            .remember_insertion_focus(&left);
    });

    // Beside the focused tile, as if it had opened there, not appended after `right` (#494).
    let frames = &runtime.backend.frames;
    assert!(frames[&left].x < frames[&created].x, "{frames:?}");
    assert!(frames[&created].x < frames[&right].x, "{frames:?}");
}

#[test]
fn columns_route_joins_a_nested_focused_split_and_keeps_the_other_column() {
    let (top, right, below) = (window(30), window(31), window(32));
    let tiled = [top.clone(), right.clone(), below.clone()];
    let (runtime, created) = route_huddle_into_columns(&tiled, |rt| {
        let engine = rt.workspaces.engine_mut(WorkspaceId::FIVE);
        // (top / below) | right, with `below` focused.
        assert!(engine.move_window_relative(&below, &top, crate::Direction::Down));
        engine.remember_insertion_focus(&below);
    });

    // A third row under `below`; `right` keeps half the display at full height (#494).
    let frames = &runtime.backend.frames;
    let column = [&top, &below, &created].map(|window| frames[window]);
    assert!(
        column.iter().all(|frame| frame.x == column[0].x),
        "{frames:?}"
    );
    assert!(
        column[0].y < column[1].y && column[1].y < column[2].y,
        "{frames:?}"
    );
    assert_eq!(frames[&right].width, column[0].width, "{frames:?}");
    assert!(frames[&right].x > column[0].x, "{frames:?}");
    assert!(frames[&right].height > column[0].height * 2, "{frames:?}");
}

#[test]
fn floating_rule_converts_existing_and_new_windows_to_floating_ownership() {
    let existing = window(10);
    let created = window(11);
    let mut backend = FakeBackend {
        observation: Some(observation_for_windows(
            "com.example.Test",
            &created,
            &[
                (existing.clone(), frame(50)),
                (created.clone(), frame(1050)),
            ],
        )),
        ..FakeBackend::default()
    };
    backend.frames.insert(existing.clone(), frame(50));
    backend.frames.insert(created.clone(), frame(1050));
    let config =
        Config::parse("version = 1\n[[rules]]\nbundle_id = 'com.example.Test'\naction = 'float'")
            .unwrap();
    let mut runtime = runtime_with_config(backend, config);
    bind(&mut runtime, WorkspaceId::ONE, &existing);

    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    assert_eq!(runtime.workspaces.workspace_of(&existing), None);
    assert_eq!(
        runtime.workspaces.floating_workspace_of(&existing),
        Some(WorkspaceId::ONE)
    );
    assert_eq!(runtime.workspaces.workspace_of(&created), None);
    assert_eq!(
        runtime.workspaces.floating_workspace_of(&created),
        Some(WorkspaceId::FIVE)
    );
}

#[test]
fn ignore_rule_releases_the_window_and_never_adopts_it() {
    let existing = window(10);
    let ignored = window(11);
    let mut backend = FakeBackend {
        observation: Some(observation_for_windows(
            "com.example.Test",
            &existing,
            &[(existing.clone(), frame(50)), (ignored.clone(), frame(400))],
        )),
        ..FakeBackend::default()
    };
    backend.frames.insert(existing.clone(), frame(50));
    backend.frames.insert(ignored.clone(), frame(400));
    let config =
        Config::parse("version = 1\n[[rules]]\nbundle_id = 'com.example.Test'\naction = 'ignore'")
            .unwrap();
    let mut runtime = runtime_with_config(backend, config);
    bind(&mut runtime, WorkspaceId::ONE, &existing);

    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    // Both windows match the ignore rule: the owned one is released and
    // the new one is never adopted; neither is ever parked.
    assert_eq!(runtime.workspaces.workspace_of_any(&existing), None);
    assert_eq!(runtime.workspaces.workspace_of_any(&ignored), None);
    assert!(runtime.parked.is_empty());
}

#[test]
fn app_spanning_workspaces_keeps_current_display_routing() {
    let main = window(10);
    let secondary = window(11);
    let created = window(12);
    let mut backend = FakeBackend {
        observation: Some(observation_for_windows(
            "com.example.Test",
            &created,
            &[
                (main.clone(), frame(50)),
                (secondary.clone(), frame(1050)),
                (created.clone(), frame(1100)),
            ],
        )),
        ..FakeBackend::default()
    };
    backend.frames.insert(main.clone(), frame(50));
    backend.frames.insert(secondary.clone(), frame(1050));
    backend.frames.insert(created.clone(), frame(1100));
    let mut runtime = runtime(backend);
    bind(&mut runtime, WorkspaceId::ONE, &main);
    bind(&mut runtime, WorkspaceId::FIVE, &secondary);

    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    assert_eq!(
        runtime.workspaces.workspace_of(&created),
        Some(WorkspaceId::FIVE)
    );
}

#[test]
fn simultaneous_new_windows_stay_on_their_display_active_workspace() {
    let existing = window(10);
    let first = window(11);
    let second = window(12);
    let mut backend = FakeBackend {
        observation: Some(observation_for_windows(
            "com.example.Test",
            &second,
            &[
                (existing.clone(), frame(50)),
                (first.clone(), frame(1050)),
                (second.clone(), frame(1100)),
            ],
        )),
        ..FakeBackend::default()
    };
    backend.frames.insert(existing.clone(), frame(50));
    backend.frames.insert(first.clone(), frame(1050));
    backend.frames.insert(second.clone(), frame(1100));
    let mut runtime = runtime(backend);
    bind(&mut runtime, WorkspaceId::ONE, &existing);

    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    assert_eq!(
        runtime.workspaces.workspace_of(&first),
        Some(WorkspaceId::FIVE)
    );
    assert_eq!(
        runtime.workspaces.workspace_of(&second),
        Some(WorkspaceId::FIVE)
    );
    assert!(runtime.backend.frames[&first].x >= 1000);
    assert!(runtime.backend.frames[&second].x >= 1000);
}

#[test]
fn new_window_opens_on_active_workspace_even_when_app_has_window_on_inactive_workspace() {
    let existing = window(10);
    let created = window(11);
    let mut backend = FakeBackend {
        observation: Some(observation_for_windows(
            "com.example.Test",
            &created,
            &[
                (existing.clone(), frame(50)),
                (created.clone(), frame(1050)),
            ],
        )),
        ..FakeBackend::default()
    };
    backend.frames.insert(existing.clone(), frame(50));
    backend.frames.insert(created.clone(), frame(1050));
    let mut runtime = runtime(backend);
    let inactive = WorkspaceId::new(2).unwrap();
    bind(&mut runtime, inactive, &existing);

    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    assert_eq!(
        runtime.workspaces.workspace_of(&created),
        Some(WorkspaceId::FIVE)
    );
    assert!(!runtime.parked.contains_key(&created));
    assert!(
        outputs()
            .secondary()
            .unwrap()
            .frame
            .contains_center(runtime.backend.frames[&created])
    );
}

#[test]
fn floating_rule_owns_window_without_adding_it_to_layout() {
    let window = window(10);
    let normal = frame(50);
    let mut backend = FakeBackend {
        observation: Some(observation(&window, normal)),
        ..FakeBackend::default()
    };
    backend.frames.insert(window.clone(), normal);
    let config =
        Config::parse("version = 1\n[[rules]]\nbundle_id = 'com.example.Test'\naction = 'float'")
            .unwrap();
    let mut runtime = runtime_with_config(backend, config);

    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    assert!(
        runtime
            .workspaces
            .engine(WorkspaceId::ONE)
            .model()
            .workspace
            .slots
            .is_empty()
    );
    assert_eq!(runtime.workspaces.workspace_of(&window), None);
    assert_eq!(
        runtime.workspaces.floating_workspace_of(&window),
        Some(WorkspaceId::ONE)
    );
    assert!(
        !runtime
            .backend
            .calls
            .iter()
            .any(|call| matches!(call, BackendCall::Apply(_, _)))
    );
}

#[test]
fn tiled_window_keeps_its_tile_through_modal_sheet_non_resizable_state() {
    let first = window(10);
    let sibling = window(11);
    let cancelled = AtomicBool::new(false);
    let mut backend = FakeBackend {
        focused: Some(first.clone()),
        frontmost: Some(first.app.clone()),
        observation: Some(observation_for_windows(
            "com.example.Test",
            &first,
            &[(first.clone(), frame(50)), (sibling.clone(), frame(550))],
        )),
        ..FakeBackend::default()
    };
    backend.frames.insert(first.clone(), frame(50));
    backend.frames.insert(sibling.clone(), frame(550));
    let mut runtime = runtime(backend);

    runtime.cycle(outputs(), &cancelled).unwrap();
    runtime
        .resize_focused(ResizeDirection::Grow, outputs(), &cancelled)
        .unwrap();

    let snapshot_slots = |runtime: &WorkspaceRuntime<FakeBackend>| {
        let engine = runtime.workspaces.engine(WorkspaceId::ONE);
        engine
            .model()
            .workspace
            .slots
            .iter()
            .map(|slot| {
                let weight = engine.layout().weight(slot.id);
                (slot.id, slot.bound_window().cloned(), weight)
            })
            .collect::<Vec<_>>()
    };
    let slots_before = snapshot_slots(&runtime);
    let first_frame = runtime.backend.frames[&first];
    let sibling_frame = runtime.backend.frames[&sibling];
    assert!(first_frame.width > sibling_frame.width);
    assert!(sibling_frame.width < outputs().main().frame.width);

    // A window-modal close/save sheet temporarily flips the tiled window non-resizable.
    let Some(crate::NativeObservation { applications, .. }) = &mut runtime.backend.observation
    else {
        unreachable!();
    };
    let crate::AppObservation::Complete { windows, .. } = &mut applications[0] else {
        unreachable!();
    };
    windows[0].resizable = false;

    runtime.cycle(outputs(), &cancelled).unwrap();

    assert_eq!(
        runtime.workspaces.workspace_of(&first),
        Some(WorkspaceId::ONE)
    );
    assert_eq!(runtime.workspaces.floating_workspace_of(&first), None);
    assert_eq!(snapshot_slots(&runtime), slots_before);
    assert_eq!(runtime.backend.frames[&first], first_frame);
    assert_eq!(runtime.backend.frames[&sibling], sibling_frame);

    // When the sheet closes and resizable returns to true, tiling resumes normally.
    let Some(crate::NativeObservation { applications, .. }) = &mut runtime.backend.observation
    else {
        unreachable!();
    };
    let crate::AppObservation::Complete { windows, .. } = &mut applications[0] else {
        unreachable!();
    };
    windows[0].resizable = true;

    runtime.cycle(outputs(), &cancelled).unwrap();
    assert_eq!(snapshot_slots(&runtime), slots_before);
    assert_eq!(runtime.backend.frames[&first], first_frame);
    assert_eq!(runtime.backend.frames[&sibling], sibling_frame);

    runtime
        .resize_focused(ResizeDirection::Grow, outputs(), &cancelled)
        .unwrap();
    assert!(runtime.backend.frames[&first].width > first_frame.width);
}

#[test]
fn newly_opened_non_resizable_dialog_floats_without_tiling() {
    let tiled = window(10);
    let dialog = window(11);
    let dialog_frame = frame(300);
    let mut observed = observation_for_windows(
        "com.example.Test",
        &dialog,
        &[(tiled.clone(), frame(50)), (dialog.clone(), dialog_frame)],
    );
    let crate::AppObservation::Complete { windows, .. } = &mut observed.applications[0] else {
        unreachable!();
    };
    windows[1].resizable = false;
    let mut backend = FakeBackend {
        observation: Some(observed),
        ..FakeBackend::default()
    };
    backend.frames.insert(tiled.clone(), frame(50));
    backend.frames.insert(dialog.clone(), dialog_frame);
    let mut runtime = runtime(backend);
    bind(&mut runtime, WorkspaceId::ONE, &tiled);

    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    assert_eq!(
        runtime.workspaces.workspace_of(&tiled),
        Some(WorkspaceId::ONE)
    );
    assert_eq!(runtime.workspaces.workspace_of(&dialog), None);
    assert_eq!(
        runtime.workspaces.floating_workspace_of(&dialog),
        Some(WorkspaceId::ONE)
    );
    assert_eq!(runtime.backend.frames[&tiled], outputs().main().frame);
    assert_eq!(runtime.backend.frames[&dialog], dialog_frame);
    assert!(
        !runtime
            .backend
            .calls
            .iter()
            .any(|call| matches!(call, BackendCall::Apply(window, _) if window == &dialog))
    );
}

#[test]
fn a_tile_rule_tiles_a_window_that_looks_like_a_dialog_until_toggled_to_float() {
    let tiled = window(10);
    let dialog = window(11);
    let cancelled = AtomicBool::new(false);
    let mut observed = observation_for_windows(
        "com.example.Test",
        &dialog,
        &[(tiled.clone(), frame(50)), (dialog.clone(), frame(300))],
    );
    let crate::AppObservation::Complete { windows, .. } = &mut observed.applications[0] else {
        unreachable!();
    };
    windows[1].resizable = false;
    let mut backend = FakeBackend {
        focused: Some(dialog.clone()),
        frontmost: Some(dialog.app.clone()),
        observation: Some(observed),
        ..FakeBackend::default()
    };
    backend.frames.insert(tiled.clone(), frame(50));
    backend.frames.insert(dialog.clone(), frame(300));
    let config =
        Config::parse("version = 1\n[[rules]]\nbundle_id = 'com.example.Test'\naction = 'tile'")
            .unwrap();
    let mut runtime = runtime_with_config(backend, config);
    bind(&mut runtime, WorkspaceId::ONE, &tiled);

    runtime.cycle(outputs(), &cancelled).unwrap();

    assert_eq!(
        runtime.workspaces.workspace_of(&dialog),
        Some(WorkspaceId::ONE)
    );
    assert_eq!(runtime.workspaces.floating_workspace_of(&dialog), None);
    assert!(
        runtime
            .backend
            .calls
            .iter()
            .any(|call| matches!(call, BackendCall::Apply(window, _) if window == &dialog))
    );

    // Toggle Float still wins over the tile rule, and keeps winning.
    runtime.toggle_float(outputs(), &cancelled).unwrap();
    runtime.cycle(outputs(), &cancelled).unwrap();
    assert_eq!(runtime.workspaces.workspace_of(&dialog), None);
    assert_eq!(
        runtime.workspaces.floating_workspace_of(&dialog),
        Some(WorkspaceId::ONE)
    );
}

#[test]
fn manual_float_and_float_rule_still_release_tiled_windows_immediately() {
    let first = window(10);
    let sibling = window(11);
    let cancelled = AtomicBool::new(false);
    let mut backend = FakeBackend {
        focused: Some(first.clone()),
        frontmost: Some(first.app.clone()),
        observation: Some(observation_for_windows(
            "com.example.Test",
            &first,
            &[(first.clone(), frame(50)), (sibling.clone(), frame(550))],
        )),
        ..FakeBackend::default()
    };
    backend.frames.insert(first.clone(), frame(50));
    backend.frames.insert(sibling.clone(), frame(550));
    let mut runtime = runtime(backend);

    runtime.cycle(outputs(), &cancelled).unwrap();
    runtime.toggle_float(outputs(), &cancelled).unwrap();

    assert_eq!(runtime.workspaces.workspace_of(&first), None);
    assert_eq!(
        runtime.workspaces.floating_workspace_of(&first),
        Some(WorkspaceId::ONE)
    );
    assert!(runtime.workspaces.is_manual_float(&first));
    assert_eq!(runtime.backend.frames[&sibling], outputs().main().frame);

    // Subsequent cycles keep the manually floated window floating.
    runtime.cycle(outputs(), &cancelled).unwrap();
    assert_eq!(runtime.workspaces.workspace_of(&first), None);
    assert_eq!(
        runtime.workspaces.floating_workspace_of(&first),
        Some(WorkspaceId::ONE)
    );

    // A float rule also releases the remaining tiled window immediately.
    let float_config = Config::parse(
        "version = 1
[[rules]]
bundle_id = 'com.example.Test'
action = 'float'",
    )
    .unwrap();
    runtime.replace_config(float_config);
    runtime.cycle(outputs(), &cancelled).unwrap();

    assert_eq!(runtime.workspaces.workspace_of(&sibling), None);
    assert_eq!(
        runtime.workspaces.floating_workspace_of(&sibling),
        Some(WorkspaceId::ONE)
    );
}

#[test]
fn route_rule_moves_only_inserted_window_to_inactive_workspace() {
    let window = window(10);
    let normal = frame(50);
    let mut backend = FakeBackend {
        observation: Some(observation(&window, normal)),
        ..FakeBackend::default()
    };
    backend.frames.insert(window.clone(), normal);
    let config = Config::parse(
        "version = 1\n[[rules]]\nbundle_id = 'com.example.Test'\naction = 'route'\nworkspace = 2",
    )
    .unwrap();
    let mut runtime = runtime_with_config(backend, config);

    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    assert_eq!(
        runtime.workspaces.workspace_of(&window),
        Some(WorkspaceId::new(2).unwrap())
    );
    assert!(runtime.parked.contains_key(&window));
    assert!(runtime.backend.workspace_state.is_some());
    assert!(
        runtime
            .workspaces
            .engine(WorkspaceId::ONE)
            .bound_windows()
            .is_empty()
    );
}

#[test]
fn cross_display_route_survives_later_reconciliation_in_same_cycle() {
    let window = window(10);
    let normal = frame(50);
    let mut backend = FakeBackend {
        observation: Some(observation(&window, normal)),
        ..FakeBackend::default()
    };
    backend.frames.insert(window.clone(), normal);
    let config = Config::parse(
        "version = 1\n[[rules]]\nbundle_id = 'com.example.Test'\naction = 'route'\nworkspace = 5",
    )
    .unwrap();
    let mut runtime = runtime_with_config(backend, config);

    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    assert_eq!(
        runtime.workspaces.workspace_of(&window),
        Some(WorkspaceId::FIVE)
    );
    assert!(runtime.backend.frames[&window].x >= 1000);
}

#[test]
fn unmanaged_route_is_rejected_before_insertion() {
    let window = window(10);
    let normal = frame(50);
    let mut backend = FakeBackend {
        observation: Some(observation(&window, normal)),
        ..FakeBackend::default()
    };
    backend.frames.insert(window.clone(), normal);
    let config = Config::parse(
        "version = 1\n[[rules]]\nbundle_id = 'com.example.Test'\naction = 'route'\nworkspace = 6",
    )
    .unwrap();
    let mut runtime = WorkspaceRuntime::with_backend_and_config(
        ManagementScope::Main,
        backend,
        HashMap::new(),
        config,
    );

    assert!(runtime.cycle(outputs(), &AtomicBool::new(false)).is_err());
    assert_eq!(runtime.workspaces.workspace_of(&window), None);
}

#[test]
fn startup_splash_window_does_not_suppress_bundle_route_for_main_window() {
    let splash = window(640);
    let main_window = window(641);
    let splash_frame = crate::Rect {
        x: 350,
        y: 250,
        width: 300,
        height: 300,
    };
    let main_frame = crate::Rect {
        x: 50,
        y: 30,
        width: 900,
        height: 700,
    };
    let config = Config::parse(
        "version = 1\n[[rules]]\nbundle_id = 'com.example.Discord'\naction = 'route'\nworkspace = 5",
    )
    .unwrap();

    // Start focused on workspace 2 (main display); workspace 5 is active on secondary.
    let mut splash_obs = observation_for_windows(
        "com.example.Discord",
        &splash,
        &[(splash.clone(), splash_frame)],
    );
    if let crate::AppObservation::Complete { windows, .. } = &mut splash_obs.applications[0] {
        windows[0].resizable = false;
    }
    let backend = FakeBackend {
        frames: HashMap::from([(splash.clone(), splash_frame)]),
        focused: Some(splash.clone()),
        frontmost: Some(splash.app.clone()),
        observation: Some(splash_obs),
        ..FakeBackend::default()
    };
    let mut rt = runtime_with_config(backend, config);
    let cancel = AtomicBool::new(false);
    rt.switch(WorkspaceId::TWO, outputs(), &cancel).unwrap();
    rt.cycle(outputs(), &cancel).unwrap();
    assert_eq!(
        rt.workspaces.floating_workspace_of(&splash),
        Some(WorkspaceId::FIVE),
        "startup splash window follows the app's route rule"
    );

    // Main resizable window opens while the splash is still alive: it must still
    // route to workspace 5 instead of tiling on active workspace 2.
    let mut both_obs = observation_for_windows(
        "com.example.Discord",
        &main_window,
        &[
            (splash.clone(), splash_frame),
            (main_window.clone(), main_frame),
        ],
    );
    if let crate::AppObservation::Complete { windows, .. } = &mut both_obs.applications[0] {
        windows[0].resizable = false;
        windows[1].resizable = true;
    }
    rt.backend.frames.insert(main_window.clone(), main_frame);
    rt.backend.focused = Some(main_window.clone());
    rt.backend.observation = Some(both_obs);
    rt.cycle(outputs(), &cancel).unwrap();
    assert_eq!(
        rt.workspaces.workspace_of(&main_window),
        Some(WorkspaceId::FIVE),
        "main window must follow the bundle route rule even while splash is alive"
    );

    // Control: a second tiled window opened later still lands on the current display.
    let secondary = window(642);
    let mut with_second = observation_for_windows(
        "com.example.Discord",
        &secondary,
        &[
            (main_window.clone(), rt.backend.frames[&main_window]),
            (secondary.clone(), main_frame),
        ],
    );
    if let crate::AppObservation::Complete { windows, .. } = &mut with_second.applications[0] {
        windows[0].resizable = true;
        windows[1].resizable = true;
    }
    rt.backend.frames.insert(secondary.clone(), main_frame);
    rt.backend.focused = Some(secondary.clone());
    rt.backend.observation = Some(with_second);
    rt.cycle(outputs(), &cancel).unwrap();
    assert_eq!(
        rt.workspaces.workspace_of(&secondary),
        Some(WorkspaceId::TWO)
    );
}
