use super::*;

#[test]
fn ghostty_native_tab_replacement_preserves_workspace_and_fullscreen_layout() {
    let previous = window(10);
    let replacement = window(11);
    let normal = frame(50);
    let mut backend = FakeBackend {
        observation: Some(observation_for("com.mitchellh.ghostty", &previous, normal)),
        focused: Some(previous.clone()),
        ..FakeBackend::default()
    };
    backend.frames.insert(previous.clone(), normal);
    let config = Config::parse(
        "version = 1\n[[rules]]\nbundle_id = 'com.mitchellh.ghostty'\naction = 'route'\nworkspace = 2",
    )
    .unwrap();
    let mut runtime = runtime_with_config(backend, config);
    bind(&mut runtime, WorkspaceId::ONE, &previous);
    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();
    runtime
        .toggle_fullscreen_layout(outputs(), &AtomicBool::new(false))
        .unwrap();
    let accepted = runtime.backend.frames[&previous];
    runtime.backend.observation = Some(observation_for(
        "com.mitchellh.ghostty",
        &previous,
        accepted,
    ));
    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();
    runtime.backend.frames.remove(&previous);
    runtime.backend.frames.insert(replacement.clone(), accepted);
    runtime.backend.observation = Some(observation_for(
        "com.mitchellh.ghostty",
        &replacement,
        accepted,
    ));

    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    assert_eq!(
        runtime.workspaces.workspace_of(&replacement),
        Some(WorkspaceId::ONE)
    );
    assert!(runtime.workspaces.deck(WorkspaceId::ONE).is_monocle());
    assert_eq!(
        runtime.workspaces.preferred_focus(WorkspaceId::ONE),
        Some(replacement.clone())
    );
    assert_eq!(runtime.backend.frames[&replacement], outputs().main().frame);
    assert!(!runtime.parked.contains_key(&replacement));
}

#[test]
fn fullscreen_layout_toggles_without_changing_tile_order() {
    let first = window(10);
    let second = window(20);
    let mut backend = FakeBackend {
        focused: Some(second.clone()),
        ..FakeBackend::default()
    };
    backend.frames.insert(first.clone(), frame(50));
    backend.frames.insert(second.clone(), frame(500));
    let mut runtime = runtime(backend);
    bind(&mut runtime, WorkspaceId::ONE, &first);
    bind(&mut runtime, WorkspaceId::ONE, &second);
    let outputs = outputs();

    let status = runtime
        .toggle_fullscreen_layout(outputs.clone(), &AtomicBool::new(false))
        .unwrap();

    assert_eq!(runtime.backend.frames[&first], outputs.main().frame);
    assert_eq!(runtime.backend.frames[&second], outputs.main().frame);
    assert!(runtime.workspaces.deck(WorkspaceId::ONE).is_monocle());
    assert!(matches!(status.layouts[0].mode, UiHudLayout::Fullscreen));
    assert_eq!(
        runtime.workspaces.engine(WorkspaceId::ONE).bound_windows(),
        vec![first.clone(), second.clone()]
    );

    let status = runtime
        .toggle_fullscreen_layout(outputs, &AtomicBool::new(false))
        .unwrap();

    assert_eq!(runtime.backend.frames[&first].width, 500);
    assert_eq!(runtime.backend.frames[&second].x, 500);
    assert!(!runtime.workspaces.deck(WorkspaceId::ONE).is_monocle());
    assert!(matches!(status.layouts[0].mode, UiHudLayout::Split(_)));
    assert_eq!(
        runtime.workspaces.engine(WorkspaceId::ONE).bound_windows(),
        vec![first, second]
    );
}

#[test]
fn removing_the_last_window_exits_fullscreen_layout() {
    let window = window(10);
    let mut backend = FakeBackend {
        focused: Some(window.clone()),
        observation: Some(observation(&window, frame(50))),
        ..FakeBackend::default()
    };
    backend.frames.insert(window.clone(), frame(50));
    let mut runtime = runtime(backend);
    let outputs = outputs();
    let cancelled = AtomicBool::new(false);
    runtime.cycle(outputs.clone(), &cancelled).unwrap();
    runtime
        .toggle_fullscreen_layout(outputs.clone(), &cancelled)
        .unwrap();
    runtime.focus.restore_fullscreen_on(WorkspaceId::ONE);

    runtime.backend.frames.remove(&window);
    runtime.backend.observation = Some(crate::NativeObservation {
        cycle: 0,
        mouse_down: false,
        applications: Vec::new(),
    });
    let status = runtime.cycle(outputs, &cancelled).unwrap();

    assert!(!runtime.workspaces.deck(WorkspaceId::ONE).is_monocle());
    assert!(!runtime.focus.restores_any_fullscreen());
    assert!(matches!(status.layouts[0].mode, UiHudLayout::Split(_)));
}

#[test]
fn fullscreen_layout_remembers_native_same_app_focus_changes() {
    let first = window(10);
    let second = window(20);
    let output = outputs().main().frame;
    let mut backend = FakeBackend {
        focused: Some(first.clone()),
        frontmost: Some(first.app.clone()),
        observation: Some(observation_for_windows(
            "com.example.Test",
            &first,
            &[(first.clone(), frame(50)), (second.clone(), frame(500))],
        )),
        ..FakeBackend::default()
    };
    backend.frames.insert(first.clone(), frame(50));
    backend.frames.insert(second.clone(), frame(500));
    let mut runtime = runtime(backend);
    bind(&mut runtime, WorkspaceId::ONE, &first);
    bind(&mut runtime, WorkspaceId::ONE, &second);
    let outputs = outputs();
    let cancelled = AtomicBool::new(false);
    runtime
        .toggle_fullscreen_layout(outputs.clone(), &cancelled)
        .unwrap();

    runtime.backend.focused = Some(second.clone());
    runtime.backend.observation = Some(observation_for_windows(
        "com.example.Test",
        &second,
        &[(first.clone(), output), (second.clone(), output)],
    ));
    runtime.cycle(outputs, &cancelled).unwrap();

    assert_eq!(
        runtime.workspaces.preferred_focus(WorkspaceId::ONE),
        Some(second)
    );
}

#[test]
fn fullscreen_layout_activates_persisted_cross_app_focus_after_restart() {
    let first = window(10);
    let focused = crate::PhysicalWindowId {
        app: AppInstanceId {
            pid: 2,
            launched_at: 2,
        },
        window_id: 20,
    };
    let output = outputs().main().frame;
    let mut persisted = Workspaces::default();
    for (id, window) in [(10, &first), (20, &focused)] {
        persisted.engine_mut(WorkspaceId::ONE).insert_slot(Slot {
            id: SlotId(id),
            binding: BindingState::Bound(window.clone()),
        });
    }
    persisted.remember_focus(WorkspaceId::ONE, &focused);
    persisted.set_deck(WorkspaceId::ONE, crate::workspace::DeckMode::Monocle);
    let mut backend = FakeBackend {
        focused: Some(first.clone()),
        frontmost: Some(first.app.clone()),
        observation: Some(crate::NativeObservation {
            cycle: 0,
            mouse_down: false,
            applications: vec![
                crate::AppObservation::Complete {
                    app: crate::ObservedApplication {
                        id: first.app.clone(),
                        bundle_id: "com.example.First".into(),
                    },
                    focused: Some(first.clone()),
                    windows: vec![crate::ObservedWindow {
                        id: first.clone(),
                        frame: output,
                        resizable: true,
                        title: None,
                        document: None,
                    }],
                },
                crate::AppObservation::Complete {
                    app: crate::ObservedApplication {
                        id: focused.app.clone(),
                        bundle_id: "com.example.Second".into(),
                    },
                    focused: Some(focused.clone()),
                    windows: vec![crate::ObservedWindow {
                        id: focused.clone(),
                        frame: output,
                        resizable: true,
                        title: None,
                        document: None,
                    }],
                },
            ],
        }),
        ..FakeBackend::default()
    };
    backend.frames.insert(first, output);
    backend.frames.insert(focused.clone(), output);
    let mut runtime = WorkspaceRuntime::with_backend_config_state(
        ManagementScope::Both,
        backend,
        HashMap::new(),
        Config::default(),
        Some(persisted.state()),
    );

    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    assert!(
        runtime
            .backend
            .calls
            .contains(&BackendCall::Focus(focused.clone()))
    );
    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();
    assert_eq!(
        runtime.workspaces.preferred_focus(WorkspaceId::ONE),
        Some(focused)
    );
}

#[test]
fn failed_fullscreen_focus_restore_does_not_fight_later_focus() {
    let first = window(10);
    let persisted_focus = window(20);
    let output = outputs().main().frame;
    let mut persisted = Workspaces::default();
    for (id, window) in [(10, &first), (20, &persisted_focus)] {
        persisted.engine_mut(WorkspaceId::ONE).insert_slot(Slot {
            id: SlotId(id),
            binding: BindingState::Bound(window.clone()),
        });
    }
    persisted.remember_focus(WorkspaceId::ONE, &persisted_focus);
    persisted.set_deck(WorkspaceId::ONE, crate::workspace::DeckMode::Monocle);
    let mut backend = FakeBackend {
        focused: Some(first.clone()),
        frontmost: Some(first.app.clone()),
        focus_outcome: Some(false),
        observation: Some(observation_for_windows(
            "com.example.Test",
            &first,
            &[(first.clone(), output), (persisted_focus.clone(), output)],
        )),
        ..FakeBackend::default()
    };
    backend.frames.insert(first.clone(), output);
    backend.frames.insert(persisted_focus.clone(), output);
    let mut runtime = WorkspaceRuntime::with_backend_config_state(
        ManagementScope::Both,
        backend,
        HashMap::new(),
        Config::default(),
        Some(persisted.state()),
    );

    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();
    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    assert_eq!(
        runtime
            .backend
            .calls
            .iter()
            .filter(|call| **call == BackendCall::Focus(persisted_focus.clone()))
            .count(),
        1
    );
    assert_eq!(
        runtime.workspaces.preferred_focus(WorkspaceId::ONE),
        Some(first)
    );
}

#[test]
fn switching_away_and_back_restores_fullscreen_layout_focus() {
    let first = window(10);
    let focused = window(20);
    let incoming = window(30);
    let outputs = outputs();
    let displays = outputs.frames().collect::<Vec<_>>();
    let incoming_normal = frame(100);
    let incoming_parked =
        crate::geometry::parked_frame(outputs.main().frame, &displays, incoming_normal);
    let mut backend = FakeBackend {
        focused: Some(first.clone()),
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

    runtime
        .toggle_fullscreen_layout(outputs.clone(), &cancelled)
        .unwrap();
    runtime.backend.focused = Some(focused.clone());
    runtime.switch(target, outputs.clone(), &cancelled).unwrap();
    runtime
        .switch(WorkspaceId::ONE, outputs.clone(), &cancelled)
        .unwrap();

    assert!(runtime.workspaces.deck(WorkspaceId::ONE).is_monocle());
    assert_eq!(runtime.backend.frames[&first], outputs.main().frame);
    assert_eq!(runtime.backend.frames[&focused], outputs.main().frame);
    assert_eq!(runtime.backend.focused, Some(focused));
}

#[test]
fn selecting_an_active_fullscreen_workspace_preserves_its_current_focus() {
    let first = window(10);
    let focused = window(20);
    let outputs = outputs();
    let mut backend = FakeBackend {
        focused: Some(first.clone()),
        ..FakeBackend::default()
    };
    backend.frames.insert(first.clone(), frame(50));
    backend.frames.insert(focused.clone(), frame(500));
    let mut runtime = runtime(backend);
    bind(&mut runtime, WorkspaceId::ONE, &first);
    bind(&mut runtime, WorkspaceId::ONE, &focused);
    let cancelled = AtomicBool::new(false);
    runtime
        .toggle_fullscreen_layout(outputs.clone(), &cancelled)
        .unwrap();
    runtime.backend.focused = Some(focused.clone());

    runtime
        .switch(WorkspaceId::ONE, outputs, &cancelled)
        .unwrap();

    assert_eq!(runtime.backend.focused, Some(focused));
}

#[test]
fn leaving_a_fullscreen_workspace_parks_the_front_window_last() {
    // Every Fullscreen Layout tile shares one frame. Parking the front window
    // first would uncover the stacked windows beneath it mid-switch.
    let back = window(10);
    let middle = window(20);
    let front = window(30);
    let outputs = outputs();
    let mut backend = FakeBackend {
        focused: Some(front.clone()),
        // Front-most first, as the window server reports it.
        stacking: Some(vec![30, 999, 20, 10]),
        ..FakeBackend::default()
    };
    for window in [&back, &middle, &front] {
        backend.frames.insert(window.clone(), frame(50));
    }
    let mut runtime = runtime(backend);
    for window in [&front, &back, &middle] {
        bind(&mut runtime, WorkspaceId::ONE, window);
    }
    let cancelled = AtomicBool::new(false);
    runtime
        .toggle_fullscreen_layout(outputs.clone(), &cancelled)
        .unwrap();
    runtime.backend.calls.clear();

    runtime
        .switch(WorkspaceId::new(2).unwrap(), outputs, &cancelled)
        .unwrap();

    let parked: Vec<_> = runtime
        .backend
        .calls
        .iter()
        .filter_map(|call| match call {
            BackendCall::SetPosition(window, _) => Some(window.window_id),
            _ => None,
        })
        .collect();
    assert_eq!(parked, vec![10, 20, 30]);
}

#[test]
fn directional_focus_steps_through_a_fullscreen_stack_then_crosses_displays() {
    let first = window(10);
    let second = window(20);
    let third = window(30);
    let neighbor = window(40);
    let outputs = outputs();
    let mut backend = FakeBackend {
        focused: Some(second.clone()),
        ..FakeBackend::default()
    };
    for window in [&first, &second, &third] {
        backend.frames.insert(window.clone(), frame(50));
    }
    backend.frames.insert(neighbor.clone(), frame(1200));
    let mut runtime = runtime(backend);
    for window in [&first, &second, &third] {
        bind(&mut runtime, WorkspaceId::ONE, window);
    }
    bind(&mut runtime, WorkspaceId::FIVE, &neighbor);
    let cancelled = AtomicBool::new(false);
    runtime
        .toggle_fullscreen_layout(outputs.clone(), &cancelled)
        .unwrap();
    let focus = |runtime: &mut WorkspaceRuntime<FakeBackend>, direction| {
        runtime
            .focus_relative(direction, outputs.clone(), &cancelled)
            .unwrap();
        runtime.backend.focused.clone().unwrap().window_id
    };

    assert_eq!(focus(&mut runtime, Direction::Left), 10);
    // The start of the stack has no previous tile and nothing lies left.
    assert_eq!(focus(&mut runtime, Direction::Up), 10);
    assert_eq!(focus(&mut runtime, Direction::Right), 20);
    assert_eq!(focus(&mut runtime, Direction::Down), 30);
    // The end of the stack continues to the adjacent display.
    assert_eq!(focus(&mut runtime, Direction::Right), 40);
    assert!(runtime.workspaces.deck(WorkspaceId::ONE).is_monocle());
}

#[test]
fn opening_a_new_tiled_window_keeps_fullscreen_layout_and_emits_hud() {
    let first = window(10);
    let second = window(20);
    let outputs = outputs();
    let cancelled = AtomicBool::new(false);
    let mut backend = FakeBackend {
        focused: Some(first.clone()),
        observation: Some(observation(&first, frame(50))),
        ..FakeBackend::default()
    };
    backend.frames.insert(first.clone(), frame(50));
    let mut runtime = runtime(backend);
    runtime.cycle(outputs.clone(), &cancelled).unwrap();
    runtime
        .toggle_fullscreen_layout(outputs.clone(), &cancelled)
        .unwrap();
    assert!(runtime.workspaces.deck(WorkspaceId::ONE).is_monocle());
    assert_eq!(runtime.backend.frames[&first], outputs.main().frame);

    runtime.backend.frames.insert(second.clone(), frame(400));
    runtime.backend.focused = Some(second.clone());
    runtime.backend.observation = Some(observation_for_windows(
        "com.example.Test",
        &second,
        &[
            (first.clone(), outputs.main().frame),
            (second.clone(), frame(400)),
        ],
    ));
    let status = runtime.cycle(outputs.clone(), &cancelled).unwrap();

    assert!(runtime.workspaces.deck(WorkspaceId::ONE).is_monocle());
    assert!(matches!(status.layouts[0].mode, UiHudLayout::Fullscreen));
    assert_eq!(runtime.backend.frames[&first], outputs.main().frame);
    assert_eq!(runtime.backend.frames[&second], outputs.main().frame);
    assert_eq!(
        runtime.workspaces.preferred_focus(WorkspaceId::ONE),
        Some(second.clone())
    );
    let hud = status.hud.expect("joining Fullscreen Layout emits HUD cue");
    assert_eq!(hud.workspace, WorkspaceId::ONE);
    assert_eq!(hud.kind, UiHudKind::Fullscreen { enabled: true });
    let ws_hud = hud.displays[0]
        .workspaces
        .iter()
        .find(|entry| entry.workspace == WorkspaceId::ONE)
        .unwrap();
    assert!(ws_hud.fullscreen);
    assert_eq!(ws_hud.tiles.len(), 2);

    // Steady-state cycles do not spam the HUD.
    runtime.backend.observation = Some(observation_for_windows(
        "com.example.Test",
        &second,
        &[
            (first, outputs.main().frame),
            (second.clone(), outputs.main().frame),
        ],
    ));
    assert_eq!(runtime.cycle(outputs, &cancelled).unwrap().hud, None);
}

#[test]
fn opening_a_floating_window_preserves_fullscreen_layout() {
    let tiled = window(10);
    let dialog = window(20);
    let outputs = outputs();
    let cancelled = AtomicBool::new(false);
    let mut backend = FakeBackend {
        focused: Some(tiled.clone()),
        observation: Some(observation(&tiled, frame(50))),
        ..FakeBackend::default()
    };
    backend.frames.insert(tiled.clone(), frame(50));
    let mut runtime = runtime(backend);
    runtime.cycle(outputs.clone(), &cancelled).unwrap();
    runtime
        .toggle_fullscreen_layout(outputs.clone(), &cancelled)
        .unwrap();
    assert!(runtime.workspaces.deck(WorkspaceId::ONE).is_monocle());

    let mut observed = observation_for_windows(
        "com.example.Test",
        &dialog,
        &[
            (tiled.clone(), outputs.main().frame),
            (dialog.clone(), frame(300)),
        ],
    );
    if let crate::AppObservation::Complete { windows, .. } = &mut observed.applications[0] {
        windows
            .iter_mut()
            .find(|window| window.id == dialog)
            .unwrap()
            .resizable = false;
    }
    runtime.backend.frames.insert(dialog.clone(), frame(300));
    runtime.backend.observation = Some(observed);
    runtime.cycle(outputs.clone(), &cancelled).unwrap();

    assert!(runtime.workspaces.deck(WorkspaceId::ONE).is_monocle());
    assert_eq!(runtime.backend.frames[&tiled], outputs.main().frame);
    assert_eq!(runtime.backend.frames[&dialog], frame(300));
    assert_eq!(
        runtime.workspaces.floating_workspace_of(&dialog),
        Some(WorkspaceId::ONE)
    );
}

/// #153: a parked tiled window that its frontmost app brings back onto a
/// visible Fullscreen Layout workspace re-homes into the stack without exiting
/// Fullscreen Layout, and cues the Workspace HUD.
#[test]
fn reappearing_parked_tile_keeps_fullscreen_layout_and_emits_hud() {
    let first = window(10);
    let second = window(20);
    let returning = window(30);
    let visible = [(first.clone(), frame(40)), (second.clone(), frame(500))];
    let mut backend = FakeBackend {
        focused: Some(first.clone()),
        frontmost: Some(first.app.clone()),
        observation: Some(observation_for_windows(
            "com.example.Test",
            &first,
            &visible,
        )),
        frames: visible.into_iter().collect(),
        ..FakeBackend::default()
    };
    backend
        .frames
        .insert(returning.clone(), main_parked_frame(frame(100)));
    let mut rt = runtime_with_parked(backend, [(returning.clone(), frame(100))]);
    bind(&mut rt, WorkspaceId::ONE, &first);
    bind(&mut rt, WorkspaceId::ONE, &second);
    bind(&mut rt, WorkspaceId::new(2).unwrap(), &returning);
    let cancelled = AtomicBool::new(false);
    rt.cycle(outputs(), &cancelled).unwrap();
    rt.toggle_fullscreen_layout(outputs(), &cancelled).unwrap();
    assert!(rt.workspaces.deck(WorkspaceId::ONE).is_monocle());
    assert_eq!(rt.backend.frames[&first], rt.backend.frames[&second]);

    rt.backend.frames.insert(returning.clone(), frame(100));
    let returned = [
        (first.clone(), frame(40)),
        (second.clone(), frame(500)),
        (returning.clone(), frame(100)),
    ];
    rt.backend.observation = Some(observation_for_windows(
        "com.example.Test",
        &first,
        &returned,
    ));
    let status = rt.cycle(outputs(), &cancelled).unwrap();

    assert_eq!(
        rt.workspaces.workspace_of(&returning),
        Some(WorkspaceId::ONE)
    );
    assert!(!rt.parked.contains_key(&returning));
    assert_eq!(
        rt.workspaces.engine(WorkspaceId::ONE).bound_windows().len(),
        3
    );
    assert!(rt.workspaces.deck(WorkspaceId::ONE).is_monocle());
    for id in [&first, &second, &returning] {
        assert_eq!(rt.backend.frames[id], outputs().main().frame);
    }
    let hud = status.hud.expect("reappearing tile cues Fullscreen HUD");
    assert_eq!(hud.workspace, WorkspaceId::ONE);
    assert_eq!(hud.kind, UiHudKind::Fullscreen { enabled: true });
    assert_eq!(rt.cycle(outputs(), &cancelled).unwrap().hud, None);
}

/// #153 control: a parked floating window that reappears on a Fullscreen
/// Layout workspace re-homes without exiting Fullscreen Layout.
#[test]
fn reappearing_parked_floating_window_preserves_fullscreen_layout() {
    let first = window(10);
    let second = window(20);
    let floating = window(30);
    let visible = [(first.clone(), frame(40)), (second.clone(), frame(500))];
    let mut backend = FakeBackend {
        focused: Some(first.clone()),
        frontmost: Some(first.app.clone()),
        observation: Some(observation_for_windows(
            "com.example.Test",
            &first,
            &visible,
        )),
        frames: visible.into_iter().collect(),
        ..FakeBackend::default()
    };
    backend
        .frames
        .insert(floating.clone(), main_parked_frame(frame(100)));
    let mut rt = runtime_with_parked(backend, [(floating.clone(), frame(100))]);
    bind(&mut rt, WorkspaceId::ONE, &first);
    bind(&mut rt, WorkspaceId::ONE, &second);
    rt.workspaces
        .own_floating(floating.clone(), WorkspaceId::new(2).unwrap());
    let cancelled = AtomicBool::new(false);
    rt.cycle(outputs(), &cancelled).unwrap();
    rt.toggle_fullscreen_layout(outputs(), &cancelled).unwrap();
    assert!(rt.workspaces.deck(WorkspaceId::ONE).is_monocle());

    rt.backend.frames.insert(floating.clone(), frame(100));
    let mut returned = observation_for_windows(
        "com.example.Test",
        &first,
        &[
            (first.clone(), frame(40)),
            (second.clone(), frame(500)),
            (floating.clone(), frame(100)),
        ],
    );
    if let crate::AppObservation::Complete { windows, .. } = &mut returned.applications[0] {
        windows
            .iter_mut()
            .find(|observed| observed.id == floating)
            .unwrap()
            .resizable = false;
    }
    rt.backend.observation = Some(returned);
    rt.cycle(outputs(), &cancelled).unwrap();

    assert_eq!(
        rt.workspaces.floating_workspace_of(&floating),
        Some(WorkspaceId::ONE)
    );
    assert!(!rt.parked.contains_key(&floating));
    assert!(rt.workspaces.deck(WorkspaceId::ONE).is_monocle());
    assert_eq!(rt.backend.frames[&first], outputs().main().frame);
    assert_eq!(rt.backend.frames[&second], outputs().main().frame);
}

#[test]
fn routing_moving_and_unfloating_preserve_fullscreen_layout() {
    let existing = window(10);
    let routed = crate::PhysicalWindowId {
        app: AppInstanceId {
            pid: 2,
            launched_at: 2,
        },
        window_id: 20,
    };
    let moved = crate::PhysicalWindowId {
        app: AppInstanceId {
            pid: 3,
            launched_at: 3,
        },
        window_id: 30,
    };
    let unfloated = crate::PhysicalWindowId {
        app: AppInstanceId {
            pid: 4,
            launched_at: 4,
        },
        window_id: 40,
    };
    let outputs = outputs();
    let secondary_frame = outputs.displays()[1].frame;
    let cancelled = AtomicBool::new(false);
    // Route `com.example.Routed` directly onto workspace 5 (visible on secondary).
    let config = Config::parse(
        "version = 1\n[[rules]]\nbundle_id = 'com.example.Routed'\naction = 'route'\nworkspace = 5",
    )
    .unwrap();
    let mut backend = FakeBackend {
        focused: Some(existing.clone()),
        frontmost: Some(existing.app.clone()),
        ..FakeBackend::default()
    };
    backend.frames.insert(existing.clone(), secondary_frame);
    backend.frames.insert(routed.clone(), frame(50));
    backend.frames.insert(moved.clone(), frame(100));
    backend.frames.insert(unfloated.clone(), secondary_frame);
    let mut rt = runtime_with_config(backend, config);
    bind(&mut rt, WorkspaceId::FIVE, &existing);
    bind(&mut rt, WorkspaceId::ONE, &moved);
    rt.workspaces.float(
        unfloated.clone(),
        WorkspaceId::FIVE,
        crate::workspace::FloatCause::Manual,
    );
    rt.workspaces
        .set_deck(WorkspaceId::FIVE, crate::workspace::DeckMode::Monocle);
    rt.workspaces.remember_focus(WorkspaceId::FIVE, &existing);

    // 1. Routing a new window from display 1 onto visible Fullscreen Layout workspace 5
    // keeps Monocle, preserves the front tile, and emits the Fullscreen HUD cue.
    rt.backend.observation = Some(crate::NativeObservation {
        cycle: 0,
        mouse_down: false,
        applications: vec![
            crate::AppObservation::Complete {
                app: crate::ObservedApplication {
                    id: existing.app.clone(),
                    bundle_id: "com.example.Existing".into(),
                },
                focused: Some(existing.clone()),
                windows: vec![crate::ObservedWindow {
                    id: existing.clone(),
                    frame: secondary_frame,
                    resizable: true,
                    title: None,
                    document: None,
                }],
            },
            crate::AppObservation::Complete {
                app: crate::ObservedApplication {
                    id: routed.app.clone(),
                    bundle_id: "com.example.Routed".into(),
                },
                focused: None,
                windows: vec![crate::ObservedWindow {
                    id: routed.clone(),
                    frame: frame(50),
                    resizable: true,
                    title: None,
                    document: None,
                }],
            },
            crate::AppObservation::Complete {
                app: crate::ObservedApplication {
                    id: moved.app.clone(),
                    bundle_id: "com.example.Moved".into(),
                },
                focused: None,
                windows: vec![crate::ObservedWindow {
                    id: moved.clone(),
                    frame: frame(100),
                    resizable: true,
                    title: None,
                    document: None,
                }],
            },
            crate::AppObservation::Complete {
                app: crate::ObservedApplication {
                    id: unfloated.app.clone(),
                    bundle_id: "com.example.Unfloated".into(),
                },
                focused: None,
                windows: vec![crate::ObservedWindow {
                    id: unfloated.clone(),
                    frame: secondary_frame,
                    resizable: true,
                    title: None,
                    document: None,
                }],
            },
        ],
    });
    let status = rt.cycle(outputs.clone(), &cancelled).unwrap();
    assert!(rt.workspaces.deck(WorkspaceId::FIVE).is_monocle());
    assert_eq!(rt.workspaces.workspace_of(&routed), Some(WorkspaceId::FIVE));
    assert_eq!(rt.backend.frames[&routed], secondary_frame);
    assert_eq!(
        rt.workspaces.preferred_focus(WorkspaceId::FIVE),
        Some(existing.clone())
    );
    let hud = status
        .hud
        .expect("routing to visible Fullscreen workspace cues HUD");
    assert_eq!(hud.workspace, WorkspaceId::FIVE);
    assert_eq!(hud.kind, UiHudKind::Fullscreen { enabled: true });

    // 2. Moving a tiled window onto Fullscreen Layout workspace 5 keeps Monocle.
    rt.backend.focused = Some(moved.clone());
    rt.backend.frontmost = Some(moved.app.clone());
    let status = rt
        .move_focused(WorkspaceId::FIVE, outputs.clone(), &cancelled)
        .unwrap();
    assert!(rt.workspaces.deck(WorkspaceId::FIVE).is_monocle());
    assert_eq!(rt.workspaces.workspace_of(&moved), Some(WorkspaceId::FIVE));
    assert_eq!(rt.backend.frames[&moved], secondary_frame);
    assert_eq!(status.hud.unwrap().kind, UiHudKind::Moved);

    // 3. Unfloating a window onto Fullscreen Layout workspace 5 keeps Monocle
    // and cues the Fullscreen HUD.
    rt.backend.focused = Some(unfloated.clone());
    rt.backend.frontmost = Some(unfloated.app.clone());
    let status = rt.toggle_float(outputs, &cancelled).unwrap();
    assert!(rt.workspaces.deck(WorkspaceId::FIVE).is_monocle());
    assert_eq!(
        rt.workspaces.workspace_of(&unfloated),
        Some(WorkspaceId::FIVE)
    );
    assert_eq!(rt.backend.frames[&unfloated], secondary_frame);
    assert_eq!(
        rt.workspaces.preferred_focus(WorkspaceId::FIVE),
        Some(unfloated)
    );
    let hud = status
        .hud
        .expect("unfloating into Fullscreen workspace cues HUD");
    assert_eq!(hud.workspace, WorkspaceId::FIVE);
    assert_eq!(hud.kind, UiHudKind::Fullscreen { enabled: true });
}

#[test]
fn routing_onto_a_hidden_fullscreen_workspace_preserves_mode_without_hud() {
    let existing = window(10);
    let routed = crate::PhysicalWindowId {
        app: AppInstanceId {
            pid: 2,
            launched_at: 2,
        },
        window_id: 20,
    };
    let two = WorkspaceId::new(2).unwrap();
    let config = Config::parse(
        "version = 1\n[[rules]]\nbundle_id = 'com.example.Routed'\naction = 'route'\nworkspace = 2",
    )
    .unwrap();
    let mut backend = FakeBackend {
        focused: Some(routed.clone()),
        frontmost: Some(routed.app.clone()),
        observation: Some(observation_for("com.example.Routed", &routed, frame(50))),
        ..FakeBackend::default()
    };
    backend
        .frames
        .insert(existing.clone(), main_parked_frame(frame(100)));
    backend.frames.insert(routed.clone(), frame(50));
    let mut rt = runtime_with_config(backend, config);
    bind(&mut rt, two, &existing);
    rt.parked
        .insert(existing.clone(), frame(100), Some("main-id".into()));
    rt.workspaces
        .set_deck(two, crate::workspace::DeckMode::Monocle);

    let status = rt.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    assert!(rt.workspaces.deck(two).is_monocle());
    assert_eq!(rt.workspaces.workspace_of(&routed), Some(two));
    assert_eq!(status.hud, None);
}
