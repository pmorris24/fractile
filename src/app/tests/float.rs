use super::*;

fn two_tiled_windows() -> WorkspaceRuntime<FakeBackend> {
    let windows = [(window(10), frame(50)), (window(20), frame(500))];
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
    let mut runtime = runtime(backend);
    bind(&mut runtime, WorkspaceId::ONE, &window(10));
    bind(&mut runtime, WorkspaceId::ONE, &window(20));
    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();
    runtime.backend.calls.clear();
    runtime
}

fn applied_to(runtime: &WorkspaceRuntime<FakeBackend>, window: &crate::PhysicalWindowId) -> bool {
    runtime
        .backend
        .calls
        .iter()
        .any(|call| matches!(call, BackendCall::Apply(applied, _) if applied == window))
}

#[test]
fn toggling_a_tiled_window_floats_it_and_retiles_the_rest() {
    let mut runtime = two_tiled_windows();
    let cancelled = AtomicBool::new(false);
    let floated = window(10);
    // Where tiling had put it; floating must not move it.
    let original = runtime.backend.frames[&floated];

    let status = runtime.toggle_float(outputs(), &cancelled).unwrap();

    assert_eq!(status.failed, 0);
    assert_eq!(runtime.workspaces.workspace_of(&floated), None);
    assert_eq!(
        runtime.workspaces.floating_workspace_of(&floated),
        Some(WorkspaceId::ONE)
    );
    assert!(runtime.workspaces.is_manual_float(&floated));
    assert_eq!(
        runtime.workspaces.engine(WorkspaceId::ONE).bound_windows(),
        vec![window(20)]
    );
    // The remaining tile fills the freed space, and the floating window keeps
    // the exact frame it had so nothing jumps under the pointer.
    assert_eq!(runtime.backend.frames[&window(20)], outputs().main().frame);
    assert_eq!(runtime.backend.frames[&floated], original);
    assert!(
        !applied_to(&runtime, &floated),
        "floating must not write the window's frame"
    );
    assert!(
        runtime.backend.workspace_state.is_some(),
        "floating ownership is persisted with the workspace"
    );
}

#[test]
fn a_floating_window_survives_later_cycles_without_being_retiled() {
    let mut runtime = two_tiled_windows();
    let cancelled = AtomicBool::new(false);
    let floated = window(10);
    runtime.toggle_float(outputs(), &cancelled).unwrap();
    let original = runtime.backend.frames[&floated];
    runtime.backend.calls.clear();

    for _ in 0..4 {
        runtime.cycle(outputs(), &cancelled).unwrap();
        assert_eq!(runtime.workspaces.workspace_of(&floated), None);
        assert_eq!(
            runtime.workspaces.floating_workspace_of(&floated),
            Some(WorkspaceId::ONE)
        );
    }

    assert!(
        !applied_to(&runtime, &floated),
        "reconciliation must leave a manually floated window where the user put it"
    );
    assert_eq!(runtime.backend.frames[&floated], original);
    assert_eq!(runtime.backend.frames[&window(20)], outputs().main().frame);
}

#[test]
fn a_floating_window_parks_and_restores_with_its_workspace() {
    let mut runtime = two_tiled_windows();
    let cancelled = AtomicBool::new(false);
    let floated = window(10);
    runtime.toggle_float(outputs(), &cancelled).unwrap();
    let original = runtime.backend.frames[&floated];

    runtime
        .switch(WorkspaceId::TWO, outputs(), &cancelled)
        .unwrap();

    assert!(runtime.parked.contains_key(&floated));
    assert!(
        !outputs()
            .main()
            .frame
            .contains_center(runtime.backend.frames[&floated])
    );
    assert_eq!(
        runtime.workspaces.floating_workspace_of(&floated),
        Some(WorkspaceId::ONE)
    );

    runtime
        .switch(WorkspaceId::ONE, outputs(), &cancelled)
        .unwrap();

    assert!(!runtime.parked.contains_key(&floated));
    assert_eq!(runtime.backend.frames[&floated], original);
    assert_eq!(runtime.workspaces.workspace_of(&floated), None);
    assert_eq!(
        runtime.workspaces.floating_workspace_of(&floated),
        Some(WorkspaceId::ONE)
    );
}

#[test]
fn toggling_a_floating_window_tiles_it_again() {
    let mut runtime = two_tiled_windows();
    let cancelled = AtomicBool::new(false);
    let tiled = window(10);
    runtime.toggle_float(outputs(), &cancelled).unwrap();
    runtime.backend.calls.clear();

    runtime.toggle_float(outputs(), &cancelled).unwrap();

    assert_eq!(
        runtime.workspaces.workspace_of(&tiled),
        Some(WorkspaceId::ONE)
    );
    assert_eq!(runtime.workspaces.floating_workspace_of(&tiled), None);
    assert!(!runtime.workspaces.is_manual_float(&tiled));

    let bound = runtime.workspaces.engine(WorkspaceId::ONE).bound_windows();
    assert_eq!(bound.len(), 2);
    assert!(bound.contains(&tiled));
    assert!(bound.contains(&window(20)));

    let first = runtime.backend.frames[&tiled];
    let second = runtime.backend.frames[&window(20)];
    let main = outputs().main().frame;
    let split_side_by_side = first.height == main.height
        && second.height == main.height
        && first.width + second.width == main.width;
    let split_stacked = first.width == main.width
        && second.width == main.width
        && first.height + second.height == main.height;
    assert!(
        split_side_by_side || split_stacked,
        "both tiles must tile the display: {first:?} {second:?}"
    );

    // A cycle keeps it tiled: the floating override is gone for good.
    runtime.cycle(outputs(), &cancelled).unwrap();
    assert_eq!(
        runtime.workspaces.workspace_of(&tiled),
        Some(WorkspaceId::ONE)
    );
    assert_eq!(runtime.workspaces.floating_workspace_of(&tiled), None);
}

#[test]
fn a_float_rule_window_stays_floating_and_says_so() {
    let floating = window(10);
    let normal = frame(50);
    let backend = FakeBackend {
        frames: HashMap::from([(floating.clone(), normal)]),
        focused: Some(floating.clone()),
        frontmost: Some(floating.app.clone()),
        observation: Some(observation(&floating, normal)),
        ..FakeBackend::default()
    };
    let config = Config::parse(
        "version = 1
[[rules]]
bundle_id = 'com.example.Test'
action = 'float'",
    )
    .unwrap();
    let mut runtime = runtime_with_config(backend, config);
    let cancelled = AtomicBool::new(false);

    runtime.cycle(outputs(), &cancelled).unwrap();
    assert_eq!(
        runtime.workspaces.floating_workspace_of(&floating),
        Some(WorkspaceId::ONE)
    );

    let error = runtime
        .toggle_float(outputs(), &cancelled)
        .err()
        .expect("a float rule keeps the window floating");

    assert!(error.contains("rule"), "{error}");
    assert_eq!(runtime.workspaces.workspace_of(&floating), None);
    assert_eq!(
        runtime.workspaces.floating_workspace_of(&floating),
        Some(WorkspaceId::ONE)
    );
    assert!(
        !runtime.workspaces.is_manual_float(&floating),
        "a rule-floated window is not a manual float"
    );
}

#[test]
fn an_unmanaged_focused_window_is_rejected_clearly() {
    let managed = window(10);
    let backend = FakeBackend {
        frames: HashMap::from([(managed.clone(), frame(50))]),
        focused: Some(window(99)),
        frontmost: Some(window(99).app),
        observation: Some(observation(&managed, frame(50))),
        ..FakeBackend::default()
    };
    let mut runtime = runtime(backend);
    bind(&mut runtime, WorkspaceId::ONE, &managed);

    let error = runtime
        .toggle_float(outputs(), &AtomicBool::new(false))
        .err()
        .expect("an unmanaged window cannot be toggled");

    assert_eq!(error, "Focused window is not managed by Fractile");
}

/// A tile and a non-resizable dialog on workspace 1, the dialog focused.
fn tile_and_focused_dialog() -> WorkspaceRuntime<FakeBackend> {
    let (tile, dialog) = (window(10), window(20));
    let frames = [(tile.clone(), frame(50)), (dialog.clone(), frame(500))];
    let mut observed = observation_for_windows("com.example.Test", &dialog, &frames);
    if let crate::AppObservation::Complete { windows, .. } = &mut observed.applications[0] {
        for observed in windows.iter_mut().filter(|observed| observed.id == dialog) {
            observed.resizable = false;
        }
    }
    let backend = FakeBackend {
        frames: frames.into_iter().collect(),
        focused: Some(dialog.clone()),
        frontmost: Some(dialog.app.clone()),
        observation: Some(observed),
        ..FakeBackend::default()
    };
    let mut runtime = runtime(backend);
    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();
    assert_eq!(
        runtime.workspaces.workspace_of(&tile),
        Some(WorkspaceId::ONE)
    );
    assert_eq!(
        runtime.workspaces.floating_workspace_of(&dialog),
        Some(WorkspaceId::ONE)
    );
    runtime
}

/// #135: Fullscreen Layout and the axis toggle act on the workspace that owns
/// a focused floating window, not only on a focused tile's workspace.
#[test]
fn workspace_layout_commands_accept_a_focused_floating_window() {
    let cancelled = AtomicBool::new(false);
    let dialog = window(20);
    let mut runtime = tile_and_focused_dialog();
    let dialog_frame = runtime.backend.frames[&dialog];

    runtime
        .toggle_fullscreen_layout(outputs(), &cancelled)
        .unwrap();
    assert!(runtime.workspaces.deck(WorkspaceId::ONE).is_monocle());
    assert_ne!(
        runtime.workspaces.preferred_focus(WorkspaceId::ONE),
        Some(dialog.clone()),
        "a float is never the Fullscreen Layout tile"
    );
    assert_eq!(runtime.backend.frames[&dialog], dialog_frame);

    runtime
        .toggle_fullscreen_layout(outputs(), &cancelled)
        .unwrap();
    assert!(!runtime.workspaces.deck(WorkspaceId::ONE).is_monocle());

    runtime
        .toggle_fullscreen_layout(outputs(), &cancelled)
        .unwrap();
    let axis = runtime.workspaces.engine(WorkspaceId::ONE).root_axis();
    runtime.toggle_layout(outputs(), &cancelled).unwrap();
    assert!(!runtime.workspaces.deck(WorkspaceId::ONE).is_monocle());
    assert_ne!(
        runtime.workspaces.engine(WorkspaceId::ONE).root_axis(),
        axis
    );
    assert_eq!(runtime.backend.frames[&dialog], dialog_frame);
}

/// #138 & #240: leaving a workspace while one of its floats has focus
/// remembers that float for returning AND preserves the remembered tile as a
/// fallback once the float closes, moves, or tiles.
#[test]
fn leaving_with_a_float_focused_refocuses_it_and_keeps_the_remembered_tile() {
    let (first, remembered, floating) = (window(10), window(20), window(30));
    let mut backend = FakeBackend::default();
    backend.frames.insert(first.clone(), frame(50));
    backend.frames.insert(remembered.clone(), frame(500));
    backend.frames.insert(floating.clone(), frame(100));
    let mut runtime = runtime(backend);
    bind(&mut runtime, WorkspaceId::ONE, &first);
    bind(&mut runtime, WorkspaceId::ONE, &remembered);
    runtime
        .workspaces
        .own_floating(floating.clone(), WorkspaceId::ONE);
    runtime
        .workspaces
        .remember_focus(WorkspaceId::ONE, &remembered);
    let cancelled = AtomicBool::new(false);
    let two = WorkspaceId::new(2).unwrap();

    runtime.backend.focused = Some(floating.clone());
    runtime.switch(two, outputs(), &cancelled).unwrap();
    assert_eq!(
        runtime.workspaces.preferred_focus(WorkspaceId::ONE),
        Some(remembered.clone())
    );
    assert_eq!(
        runtime.workspaces.preferred_float_focus(WorkspaceId::ONE),
        Some(floating.clone())
    );

    // Coming back refocuses the floating window that had focus when leaving (#240).
    runtime.backend.focused = None;
    runtime
        .switch(WorkspaceId::ONE, outputs(), &cancelled)
        .unwrap();
    assert_eq!(runtime.backend.focused, Some(floating.clone()));

    // Once that float leaves the workspace, coming back falls back to the
    // remembered tile rather than the first tile (#138).
    runtime.backend.focused = Some(floating.clone());
    runtime.switch(two, outputs(), &cancelled).unwrap();
    runtime.workspaces.forget_floating(&floating);
    runtime.backend.focused = None;
    runtime
        .switch(WorkspaceId::ONE, outputs(), &cancelled)
        .unwrap();
    assert_eq!(runtime.backend.focused, Some(remembered));
}

#[test]
fn floats_only_workspace_refocuses_the_float_that_had_focus_before_leaving() {
    let (first, second) = (window(10), window(20));
    let mut backend = FakeBackend::default();
    backend.frames.insert(first.clone(), frame(50));
    backend.frames.insert(second.clone(), frame(300));
    let mut runtime = runtime(backend);
    runtime
        .workspaces
        .own_floating(first.clone(), WorkspaceId::ONE);
    runtime
        .workspaces
        .own_floating(second.clone(), WorkspaceId::ONE);
    let cancelled = AtomicBool::new(false);
    let two = WorkspaceId::new(2).unwrap();

    runtime.backend.focused = Some(second.clone());
    runtime.switch(two, outputs(), &cancelled).unwrap();
    runtime.backend.focused = None;
    runtime
        .switch(WorkspaceId::ONE, outputs(), &cancelled)
        .unwrap();
    assert_eq!(runtime.backend.focused, Some(second));
}
