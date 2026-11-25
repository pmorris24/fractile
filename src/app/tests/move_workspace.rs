use super::*;

#[test]
fn move_workspace_to_occupied_display_promotes_source_previous_and_parks_displaced() {
    let w1 = window(10);
    let w3 = window(20);
    let w6 = window(30);
    let w3_normal = frame(100);
    let mut backend = FakeBackend {
        focused: Some(w1.clone()),
        ..FakeBackend::default()
    };
    backend.frames.insert(w1.clone(), frame(50));
    backend
        .frames
        .insert(w3.clone(), main_parked_frame(w3_normal));
    backend.frames.insert(w6.clone(), frame(1050));
    let mut runtime = runtime_with_parked(backend, [(w3.clone(), w3_normal)]);
    bind(&mut runtime, WorkspaceId::ONE, &w1);
    bind(&mut runtime, WorkspaceId::THREE, &w3);
    bind(&mut runtime, WorkspaceId::SIX, &w6);
    // Record workspace 3 as Main's previous workspace while workspace 1 stays active.
    runtime.workspaces.switch(WorkspaceId::THREE);
    runtime.workspaces.switch(WorkspaceId::ONE);
    let cancelled = AtomicBool::new(false);

    let status = runtime
        .move_workspace_to_display(DisplayTarget::Next, outputs(), &cancelled)
        .unwrap();

    assert_eq!(
        runtime.workspaces.assignment(WorkspaceId::ONE),
        &DisplaySelector::Secondary
    );
    assert_eq!(
        runtime.workspaces.active_for(&DisplaySelector::Secondary),
        Some(WorkspaceId::ONE)
    );
    assert_eq!(
        runtime.workspaces.previous_for(&DisplaySelector::Secondary),
        Some(WorkspaceId::SIX)
    );
    assert_eq!(
        runtime.workspaces.active_for(&DisplaySelector::Main),
        Some(WorkspaceId::THREE)
    );
    assert_eq!(runtime.backend.focused, Some(w1.clone()));
    assert_eq!(
        runtime.backend.frames[&w1],
        outputs().secondary().unwrap().frame
    );
    assert_eq!(runtime.backend.frames[&w3], outputs().main().frame);
    assert!(runtime.parked.contains_key(&w6));
    assert!(!runtime.parked.contains_key(&w3));
    assert_eq!(
        runtime.backend.persisted_monitors,
        vec![(WorkspaceId::ONE, DisplaySelector::Secondary)]
    );

    // Reveal-before-park ordering across both displays.
    let calls = &runtime.backend.calls;
    let pos = |p: &dyn Fn(&BackendCall) -> bool| calls.iter().position(p).unwrap();
    let reveal_source =
        pos(&|c| matches!(c, BackendCall::SetPosition(w, f) if w == &w3 && f == &w3_normal));
    let layout_source = pos(&|c| matches!(c, BackendCall::Apply(w, _) if w == &w3));
    let layout_moved = pos(&|c| matches!(c, BackendCall::Apply(w, _) if w == &w1));
    let park_displaced = pos(&|c| matches!(c, BackendCall::SetPosition(w, _) if w == &w6));
    assert!(
        reveal_source < layout_source
            && layout_source < layout_moved
            && layout_moved < park_displaced
    );

    // HUD reflects both displays, with Secondary sliding from displaced workspace 6.
    let hud = status.hud.unwrap();
    assert_eq!(hud.workspace, WorkspaceId::ONE);
    assert_eq!(hud.displays.len(), 2);
    assert_eq!(hud.displays[0].visible, Some(WorkspaceId::THREE));
    assert_eq!(hud.displays[1].visible, Some(WorkspaceId::ONE));
    assert_eq!(hud.displays[1].from, Some(WorkspaceId::SIX));

    // Option+Tab focuses the workspace focused before workspace 1 (workspace 3
    // on Main) instead of hiding the moved workspace behind displaced 6.
    runtime
        .workspace_back_and_forth(outputs(), &cancelled)
        .unwrap();
    assert_eq!(runtime.backend.focused, Some(w3.clone()));
    assert_eq!(
        runtime.workspaces.active_for(&DisplaySelector::Main),
        Some(WorkspaceId::THREE)
    );
    assert_eq!(
        runtime.workspaces.active_for(&DisplaySelector::Secondary),
        Some(WorkspaceId::ONE)
    );
    assert!(runtime.parked.contains_key(&w6));
    assert!(!runtime.parked.contains_key(&w1));

    // A second press bounces back to the moved workspace on Secondary.
    runtime
        .workspace_back_and_forth(outputs(), &cancelled)
        .unwrap();
    assert_eq!(runtime.backend.focused, Some(w1.clone()));
    assert_eq!(
        runtime.workspaces.active_for(&DisplaySelector::Secondary),
        Some(WorkspaceId::ONE)
    );
    assert!(!runtime.parked.contains_key(&w1));
}

/// Issue #76: Option+Tab right after Option+Shift+Tab must focus the workspace
/// focused before the one that moved, not the empty workspace the move
/// displaced (which would hide the workspace just moved).
#[test]
fn option_tab_after_moving_the_focused_workspace_never_reveals_the_displaced_one() {
    let b = window(10);
    let c = window(20);
    let c_normal = frame(100);
    let mut backend = FakeBackend {
        focused: Some(b.clone()),
        ..FakeBackend::default()
    };
    backend.frames.insert(b.clone(), frame(50));
    backend
        .frames
        .insert(c.clone(), main_parked_frame(c_normal));
    let mut runtime = runtime_with_parked(backend, [(c.clone(), c_normal)]);
    bind(&mut runtime, WorkspaceId::TWO, &b);
    bind(&mut runtime, WorkspaceId::THREE, &c);
    let nine = WorkspaceId::new(9).unwrap();
    let cancelled = AtomicBool::new(false);

    // Main shows workspace 2 (B); Secondary shows empty workspace 9.
    runtime.switch(nine, outputs(), &cancelled).unwrap();
    runtime
        .switch(WorkspaceId::TWO, outputs(), &cancelled)
        .unwrap();
    assert_eq!(runtime.backend.focused, Some(b.clone()));

    // Main moves B -> C.
    runtime
        .switch(WorkspaceId::THREE, outputs(), &cancelled)
        .unwrap();
    assert_eq!(runtime.backend.focused, Some(c.clone()));

    // Option+Shift+Tab moves C to Secondary. C keeps focus, so the focus
    // history still says B came before it.
    runtime
        .move_workspace_to_display(DisplayTarget::Next, outputs(), &cancelled)
        .unwrap();
    assert_eq!(
        runtime.workspaces.active_for(&DisplaySelector::Main),
        Some(WorkspaceId::TWO)
    );
    assert_eq!(
        runtime.workspaces.active_for(&DisplaySelector::Secondary),
        Some(WorkspaceId::THREE)
    );
    assert_eq!(runtime.backend.focused, Some(c.clone()));
    assert_eq!(runtime.focus.history().previous, Some(WorkspaceId::TWO));

    // Option+Tab focuses B on Main and leaves C on screen: empty workspace 9
    // never appears and nothing is parked.
    runtime
        .workspace_back_and_forth(outputs(), &cancelled)
        .unwrap();
    assert_eq!(runtime.backend.focused, Some(b.clone()));
    assert_eq!(
        runtime.workspaces.active_for(&DisplaySelector::Main),
        Some(WorkspaceId::TWO)
    );
    assert_eq!(
        runtime.workspaces.active_for(&DisplaySelector::Secondary),
        Some(WorkspaceId::THREE)
    );
    assert!(!runtime.parked.contains_key(&c));
    assert!(
        outputs()
            .secondary()
            .unwrap()
            .frame
            .contains_center(runtime.backend.frames[&c]),
        "moved workspace stayed visible on Secondary"
    );
    assert_eq!(
        runtime.workspaces.previous_for(&DisplaySelector::Secondary),
        Some(nine),
        "the displaced workspace is only the lane's previous, not Option+Tab's"
    );

    // A second Option+Tab bounces straight back to C.
    runtime
        .workspace_back_and_forth(outputs(), &cancelled)
        .unwrap();
    assert_eq!(runtime.backend.focused, Some(c.clone()));
    assert_eq!(
        runtime.workspaces.active_for(&DisplaySelector::Main),
        Some(WorkspaceId::TWO)
    );
    assert_eq!(
        runtime.workspaces.active_for(&DisplaySelector::Secondary),
        Some(WorkspaceId::THREE)
    );
    assert!(!runtime.parked.contains_key(&b));
}

#[test]
fn move_workspace_to_empty_display_and_three_display_wrap_around() {
    let w1 = window(10);
    let mut backend = FakeBackend {
        focused: Some(w1.clone()),
        ..FakeBackend::default()
    };
    backend.frames.insert(w1.clone(), frame(50));
    let mut runtime = runtime(backend);
    bind(&mut runtime, WorkspaceId::ONE, &w1);
    let cancelled = AtomicBool::new(false);
    let three = three_outputs();

    // Main -> Secondary (empty workspace 6 displaced).
    runtime
        .move_workspace_to_display(DisplayTarget::Next, three.clone(), &cancelled)
        .unwrap();
    assert_eq!(
        runtime.workspaces.assignment(WorkspaceId::ONE),
        &DisplaySelector::Secondary
    );
    assert_eq!(runtime.backend.frames[&w1], three.displays()[1].frame);

    // Secondary -> Third (empty workspace 8 displaced; the third display is `tertiary`).
    runtime
        .move_workspace_to_display(DisplayTarget::Next, three.clone(), &cancelled)
        .unwrap();
    assert_eq!(
        runtime.workspaces.assignment(WorkspaceId::ONE),
        &DisplaySelector::Tertiary
    );
    assert_eq!(runtime.backend.frames[&w1], three.displays()[2].frame);

    // Third -> wraps around to Main.
    runtime
        .move_workspace_to_display(DisplayTarget::Next, three.clone(), &cancelled)
        .unwrap();
    assert_eq!(
        runtime.workspaces.assignment(WorkspaceId::ONE),
        &DisplaySelector::Main
    );
    assert_eq!(runtime.backend.frames[&w1], three.displays()[0].frame);

    // Prev wraps Main -> Third; explicit selector moves Third -> Secondary; same display is no-op.
    runtime
        .move_workspace_to_display(DisplayTarget::Prev, three.clone(), &cancelled)
        .unwrap();
    assert_eq!(
        runtime.workspaces.assignment(WorkspaceId::ONE),
        &DisplaySelector::Tertiary
    );
    runtime
        .move_workspace_to_display(
            DisplayTarget::Selector(DisplaySelector::Secondary),
            three.clone(),
            &cancelled,
        )
        .unwrap();
    assert_eq!(
        runtime.workspaces.assignment(WorkspaceId::ONE),
        &DisplaySelector::Secondary
    );
    let count_before = runtime.backend.persisted_monitors.len();
    runtime
        .move_workspace_to_display(
            DisplayTarget::Selector(DisplaySelector::Secondary),
            three,
            &cancelled,
        )
        .unwrap();
    assert_eq!(runtime.backend.persisted_monitors.len(), count_before);
}

#[test]
fn move_workspace_single_display_is_a_noop() {
    let w1 = window(10);
    let mut backend = FakeBackend {
        focused: Some(w1.clone()),
        ..FakeBackend::default()
    };
    backend.frames.insert(w1.clone(), frame(50));
    let mut runtime = runtime(backend);
    bind(&mut runtime, WorkspaceId::ONE, &w1);
    let single = macos::DisplayOutputs::new(vec![outputs().main().clone()]).unwrap();

    let error = runtime
        .move_workspace_to_display(DisplayTarget::Next, single, &AtomicBool::new(false))
        .err()
        .unwrap();

    assert_eq!(error, "Only one display is connected");
    assert!(runtime.backend.persisted_monitors.is_empty());
    assert_eq!(
        runtime.workspaces.assignment(WorkspaceId::ONE),
        &DisplaySelector::Main
    );
}

#[test]
fn move_workspace_preserves_fullscreen_layout_and_focused_tile() {
    let first = window(10);
    let second = window(20);
    let mut backend = FakeBackend {
        focused: Some(second.clone()),
        ..FakeBackend::default()
    };
    backend.frames.insert(first.clone(), frame(50));
    backend.frames.insert(second.clone(), frame(50));
    let mut runtime = runtime(backend);
    bind(&mut runtime, WorkspaceId::ONE, &first);
    bind(&mut runtime, WorkspaceId::ONE, &second);
    runtime
        .workspaces
        .set_deck(WorkspaceId::ONE, crate::workspace::DeckMode::Monocle);

    runtime
        .move_workspace_to_display(DisplayTarget::Next, outputs(), &AtomicBool::new(false))
        .unwrap();

    let secondary_frame = outputs().secondary().unwrap().frame;
    assert!(runtime.workspaces.deck(WorkspaceId::ONE).is_monocle());
    assert_eq!(runtime.backend.frames[&first], secondary_frame);
    assert_eq!(runtime.backend.frames[&second], secondary_frame);
    assert_eq!(runtime.backend.focused, Some(second));
}

#[test]
fn move_workspace_persisted_assignment_survives_config_reload() {
    let dir = std::env::temp_dir().join(format!(
        "spaceman-move-ws-test-{}-{}",
        std::process::id(),
        crate::config::now().as_nanos()
    ));
    let config_path = dir.join("config.toml");
    let w1 = window(10);
    let mut backend = FakeBackend {
        focused: Some(w1.clone()),
        config_path: Some(config_path.clone()),
        ..FakeBackend::default()
    };
    backend.frames.insert(w1.clone(), frame(50));
    let mut runtime = runtime(backend);
    bind(&mut runtime, WorkspaceId::ONE, &w1);
    let cancelled = AtomicBool::new(false);

    runtime
        .move_workspace_to_display(DisplayTarget::Next, outputs(), &cancelled)
        .unwrap();
    runtime.replace_config(crate::config::load_path(&config_path).unwrap());
    assert_eq!(
        runtime.workspaces.assignment(WorkspaceId::ONE),
        &DisplaySelector::Secondary
    );
    assert_eq!(
        runtime.workspaces.active_for(&DisplaySelector::Secondary),
        Some(WorkspaceId::ONE)
    );

    // Moving back to Main clears the override and survives reload cleanly.
    runtime
        .move_workspace_to_display(DisplayTarget::Next, outputs(), &cancelled)
        .unwrap();
    runtime.replace_config(crate::config::load_path(&config_path).unwrap());
    assert_eq!(
        runtime.workspaces.assignment(WorkspaceId::ONE),
        &DisplaySelector::Main
    );
    assert!(
        !std::fs::read_to_string(&config_path)
            .unwrap()
            .contains("[[workspaces]]")
    );
    drop(std::fs::remove_dir_all(dir));
}

#[test]
fn move_workspace_replaces_unplugged_assignment_and_rolls_back_on_error() {
    // Part 1: unplugged preferred display showing on main moves to secondary cleanly.
    let w1 = window(10);
    let w9 = window(20);
    let w1_normal = frame(50);
    let config =
        Config::parse("version = 1\n[[workspaces]]\nworkspace = 9\nmonitor = \"unplugged-id\"\n")
            .unwrap();
    let mut backend = FakeBackend {
        focused: Some(w9.clone()),
        ..FakeBackend::default()
    };
    backend
        .frames
        .insert(w1.clone(), main_parked_frame(w1_normal));
    backend.frames.insert(w9.clone(), frame(100));
    let mut parked = HashMap::new();
    parked.insert(w1.clone(), w1_normal);
    let mut runtime =
        WorkspaceRuntime::with_backend_and_config(ManagementScope::Both, backend, parked, config);
    let nine = WorkspaceId::new(9).unwrap();
    bind(&mut runtime, WorkspaceId::ONE, &w1);
    bind(&mut runtime, nine, &w9);
    runtime
        .workspaces
        .bring_forward("main-id", DisplaySelector::Preferred("unplugged-id".into()));
    let cancelled = AtomicBool::new(false);

    runtime
        .move_workspace_to_display(DisplayTarget::Next, outputs(), &cancelled)
        .unwrap();
    assert_eq!(
        runtime.workspaces.assignment(nine),
        &DisplaySelector::Secondary
    );
    assert_eq!(
        runtime.workspaces.active_for(&DisplaySelector::Secondary),
        Some(nine)
    );
    assert_eq!(runtime.backend.frames[&w1], outputs().main().frame);

    // Part 2: config save failure rolls back both displays completely.
    let w6 = window(30);
    let w2 = window(40);
    let w2_normal = frame(80);
    let mut backend = FakeBackend {
        focused: Some(w1.clone()),
        fail_persist_config: true,
        ..FakeBackend::default()
    };
    backend.frames.insert(w1.clone(), outputs().main().frame);
    backend
        .frames
        .insert(w2.clone(), main_parked_frame(w2_normal));
    backend
        .frames
        .insert(w6.clone(), outputs().secondary().unwrap().frame);
    let mut runtime = runtime_with_parked(backend, [(w2.clone(), w2_normal)]);
    bind(&mut runtime, WorkspaceId::ONE, &w1);
    bind(&mut runtime, WorkspaceId::TWO, &w2);
    bind(&mut runtime, WorkspaceId::SIX, &w6);

    assert!(
        runtime
            .move_workspace_to_display(DisplayTarget::Next, outputs(), &cancelled)
            .is_err()
    );
    assert_eq!(
        runtime.workspaces.assignment(WorkspaceId::ONE),
        &DisplaySelector::Main
    );
    assert_eq!(
        runtime.workspaces.active_for(&DisplaySelector::Main),
        Some(WorkspaceId::ONE)
    );
    assert_eq!(
        runtime.workspaces.active_for(&DisplaySelector::Secondary),
        Some(WorkspaceId::SIX)
    );
    assert_eq!(runtime.backend.frames[&w1], outputs().main().frame);
    assert_eq!(
        runtime.backend.frames[&w6],
        outputs().secondary().unwrap().frame
    );
    assert!(!runtime.parked.contains_key(&w6));
    assert_eq!(runtime.parked.get(&w2), Some(&w2_normal));
}

#[test]
fn cycle_records_a_click_on_another_display_for_option_tab() {
    let first = window(10);
    let clicked = window(11);
    let secondary = window(20);
    let mut backend = FakeBackend {
        focused: Some(first.clone()),
        observation: Some(observation_for_windows(
            "com.example.Test",
            &first,
            &[
                (first.clone(), frame(50)),
                (clicked.clone(), frame(500)),
                (secondary.clone(), frame(1050)),
            ],
        )),
        ..FakeBackend::default()
    };
    backend.frames.insert(first.clone(), frame(50));
    backend.frames.insert(clicked.clone(), frame(500));
    backend.frames.insert(secondary.clone(), frame(1050));
    let mut runtime = runtime(backend);
    bind(&mut runtime, WorkspaceId::ONE, &first);
    bind(&mut runtime, WorkspaceId::ONE, &clicked);
    bind(&mut runtime, WorkspaceId::SIX, &secondary);
    let cancelled = AtomicBool::new(false);

    runtime
        .switch(WorkspaceId::SIX, outputs(), &cancelled)
        .unwrap();
    // Click workspace 1's other window on Main. No switch happens; the cycle
    // only watches the focus move.
    runtime.backend.focused = Some(clicked.clone());
    runtime.cycle(outputs(), &cancelled).unwrap();
    assert_eq!(runtime.focus.history().current, Some(WorkspaceId::ONE));
    assert_eq!(runtime.focus.history().previous, Some(WorkspaceId::SIX));

    runtime
        .workspace_back_and_forth(outputs(), &cancelled)
        .unwrap();
    assert_eq!(runtime.backend.focused, Some(secondary.clone()));
    assert_eq!(
        runtime.workspaces.active_for(&DisplaySelector::Main),
        Some(WorkspaceId::ONE)
    );
    assert_eq!(
        runtime.workspaces.active_for(&DisplaySelector::Secondary),
        Some(WorkspaceId::SIX)
    );

    runtime
        .workspace_back_and_forth(outputs(), &cancelled)
        .unwrap();
    assert_eq!(runtime.backend.focused, Some(first));
}

#[test]
fn workspace_back_and_forth_switches_a_hidden_workspace_on_another_display() {
    let main = window(10);
    let six = window(20);
    let mut backend = FakeBackend {
        focused: Some(main.clone()),
        ..FakeBackend::default()
    };
    backend.frames.insert(main.clone(), frame(50));
    backend.frames.insert(six.clone(), frame(1050));
    let mut runtime = runtime(backend);
    bind(&mut runtime, WorkspaceId::ONE, &main);
    bind(&mut runtime, WorkspaceId::SIX, &six);
    let cancelled = AtomicBool::new(false);

    runtime
        .switch(WorkspaceId::SIX, outputs(), &cancelled)
        .unwrap();
    runtime
        .switch(WorkspaceId::ONE, outputs(), &cancelled)
        .unwrap();
    // Secondary leaves workspace 6 without focus moving (a display change or a
    // lane change recorded elsewhere); workspace 6 is now hidden.
    runtime
        .workspaces
        .move_to_display(WorkspaceId::THREE, DisplaySelector::Secondary, None);
    assert_eq!(runtime.focus.history().previous, Some(WorkspaceId::SIX));

    runtime
        .workspace_back_and_forth(outputs(), &cancelled)
        .unwrap();

    assert_eq!(
        runtime.workspaces.active_for(&DisplaySelector::Secondary),
        Some(WorkspaceId::SIX)
    );
    assert_eq!(runtime.backend.focused, Some(six));
}
