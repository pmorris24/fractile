//! Floating-window regressions: rule origin across a restart (#160) and within a session
//! (#493), focus next from and through floats (#178), and a float omitted from one snapshot
//! (#179).
use super::*;

fn act(runtime: &mut WorkspaceRuntime<FakeBackend>, action: WorkerAction) -> Result<(), String> {
    match runtime.execute(
        WorkerCommand::Action(action, outputs()),
        &AtomicBool::new(false),
    ) {
        Some(WorkerResult::Status { result, .. }) => result.map(|_| ()),
        _ => panic!("expected worker status"),
    }
}

fn two_windows(focused: u32) -> FakeBackend {
    let windows = [(window(10), frame(50)), (window(20), frame(500))];
    FakeBackend {
        frames: windows.iter().cloned().collect(),
        focused: Some(window(focused)),
        frontmost: Some(window(focused).app),
        observation: Some(observation_for_windows(
            "com.example.Test",
            &window(focused),
            &windows,
        )),
        ..FakeBackend::default()
    }
}

/// Window 10 is titled `Notes`, so a title rule can float it alone.
fn titled_notes(mut backend: FakeBackend) -> FakeBackend {
    let Some(crate::NativeObservation { applications, .. }) = backend.observation.as_mut() else {
        unreachable!()
    };
    let crate::AppObservation::Complete { windows, .. } = &mut applications[0] else {
        unreachable!()
    };
    windows[0].title = Some("Notes".into());
    backend
}

fn float_rule() -> Config {
    Config::parse("version = 1\n[[rules]]\ntitle = 'Notes'\naction = 'float'").unwrap()
}

fn restart(
    runtime: WorkspaceRuntime<FakeBackend>,
    config: Config,
) -> WorkspaceRuntime<FakeBackend> {
    let state = runtime.workspaces.state();
    WorkspaceRuntime::with_backend_config_state(
        ManagementScope::Both,
        runtime.backend,
        runtime.parked.recovery_frames(),
        config,
        Some(state),
    )
}

/// #160: a window floated only by a rule tiles again once the rule is
/// removed, even when Fractile restarted while it floated.
#[test]
fn removing_a_float_rule_tiles_its_window_after_a_restart() {
    let mut runtime = runtime_with_config(titled_notes(two_windows(20)), float_rule());
    act(&mut runtime, WorkerAction::Retile).unwrap();
    assert_eq!(
        runtime.workspaces.floating_workspace_of(&window(10)),
        Some(WorkspaceId::ONE)
    );

    let mut runtime = restart(runtime, float_rule());
    act(&mut runtime, WorkerAction::Retile).unwrap();
    assert_eq!(
        runtime.workspaces.floating_workspace_of(&window(10)),
        Some(WorkspaceId::ONE)
    );

    runtime.replace_config(Config::default());
    act(&mut runtime, WorkerAction::Retile).unwrap();
    assert_eq!(runtime.workspaces.floating_workspace_of(&window(10)), None);
    assert_eq!(
        runtime.workspaces.engine(WorkspaceId::ONE).bound_windows(),
        vec![window(20), window(10)]
    );
}

/// #160 control: a window the user floated keeps floating across a restart.
#[test]
fn a_manual_float_keeps_floating_after_a_restart() {
    let mut runtime = runtime(titled_notes(two_windows(10)));
    act(&mut runtime, WorkerAction::Retile).unwrap();
    act(&mut runtime, WorkerAction::ToggleFloat).unwrap();

    let mut runtime = restart(runtime, Config::default());
    act(&mut runtime, WorkerAction::Retile).unwrap();
    assert_eq!(
        runtime.workspaces.floating_workspace_of(&window(10)),
        Some(WorkspaceId::ONE)
    );
    assert_eq!(
        runtime.workspaces.engine(WorkspaceId::ONE).bound_windows(),
        vec![window(20)]
    );
}

/// #493: a float rule added and removed while a window floats by hand leaves it floating by
/// hand; only Toggle Float tiles it again.
#[test]
fn a_temporary_float_rule_keeps_a_manual_float() {
    let mut runtime = runtime(titled_notes(two_windows(10)));
    act(&mut runtime, WorkerAction::Retile).unwrap();
    act(&mut runtime, WorkerAction::ToggleFloat).unwrap();

    runtime.replace_config(float_rule());
    act(&mut runtime, WorkerAction::Retile).unwrap();
    runtime.replace_config(Config::default());
    act(&mut runtime, WorkerAction::Retile).unwrap();
    assert_eq!(
        runtime.workspaces.floating_workspace_of(&window(10)),
        Some(WorkspaceId::ONE)
    );
    assert_eq!(
        runtime.workspaces.engine(WorkspaceId::ONE).bound_windows(),
        vec![window(20)]
    );

    act(&mut runtime, WorkerAction::ToggleFloat).unwrap();
    assert_eq!(runtime.workspaces.floating_workspace_of(&window(10)), None);
}

fn tiles_and_focused_float() -> WorkspaceRuntime<FakeBackend> {
    let windows = [
        (window(10), frame(50)),
        (window(20), frame(500)),
        (window(30), frame(750)),
    ];
    let mut runtime = runtime(FakeBackend {
        frames: windows.iter().cloned().collect(),
        focused: Some(window(30)),
        frontmost: Some(window(30).app),
        observation: Some(observation_for_windows(
            "com.example.Test",
            &window(30),
            &windows,
        )),
        ..FakeBackend::default()
    });
    act(&mut runtime, WorkerAction::Retile).unwrap();
    act(&mut runtime, WorkerAction::ToggleFloat).unwrap();
    assert_eq!(
        runtime.workspaces.floating_workspace_of(&window(30)),
        Some(WorkspaceId::ONE)
    );
    assert_eq!(
        runtime.workspaces.engine(WorkspaceId::ONE).bound_windows(),
        vec![window(10), window(20)]
    );
    runtime
}

/// #178: focus next from a workspace-owned float wraps onto its first tile.
#[test]
fn focus_next_from_an_owned_float_focuses_a_tile() {
    let mut runtime = tiles_and_focused_float();
    act(&mut runtime, WorkerAction::CycleFocus).unwrap();
    assert_eq!(runtime.backend.focused, Some(window(10)));
    assert_eq!(
        runtime.workspaces.floating_workspace_of(&window(30)),
        Some(WorkspaceId::ONE)
    );
}

/// #178 control: focus next from a tile still walks the tile order.
#[test]
fn focus_next_from_a_tile_still_cycles_the_tiles() {
    let mut runtime = tiles_and_focused_float();
    runtime.backend.focused = Some(window(10));
    act(&mut runtime, WorkerAction::CycleFocus).unwrap();
    assert_eq!(runtime.backend.focused, Some(window(20)));
}

/// Focus next walks the tiles, then the workspace's floats, then wraps.
#[test]
fn focus_next_cycles_tiles_then_floats() {
    let mut runtime = tiles_and_focused_float();
    runtime.backend.focused = Some(window(20));
    act(&mut runtime, WorkerAction::CycleFocus).unwrap();
    assert_eq!(runtime.backend.focused, Some(window(30)));
    act(&mut runtime, WorkerAction::CycleFocus).unwrap();
    assert_eq!(runtime.backend.focused, Some(window(10)));
}

/// One tile plus a float still has somewhere to go.
#[test]
fn focus_next_reaches_a_float_beside_a_single_tile() {
    let mut runtime = floated_second_window();
    act(&mut runtime, WorkerAction::CycleFocus).unwrap();
    assert_eq!(runtime.backend.focused, Some(window(20)));
    act(&mut runtime, WorkerAction::CycleFocus).unwrap();
    assert_eq!(runtime.backend.focused, Some(window(10)));
}

/// A workspace holding only floats cycles among them.
#[test]
fn focus_next_cycles_a_floats_only_workspace() {
    let mut runtime = floated_second_window();
    act(&mut runtime, WorkerAction::ToggleFloat).unwrap();
    assert!(
        runtime
            .workspaces
            .engine(WorkspaceId::ONE)
            .bound_windows()
            .is_empty()
    );
    act(&mut runtime, WorkerAction::CycleFocus).unwrap();
    assert_eq!(runtime.backend.focused, Some(window(20)));
    act(&mut runtime, WorkerAction::CycleFocus).unwrap();
    assert_eq!(runtime.backend.focused, Some(window(10)));
}

/// Focus prev walks the same ring backward: floats, then tiles in reverse.
#[test]
fn focus_prev_walks_the_ring_backward() {
    let mut runtime = tiles_and_focused_float();
    runtime.backend.focused = Some(window(10));
    act(&mut runtime, WorkerAction::CycleFocusPrevious).unwrap();
    assert_eq!(runtime.backend.focused, Some(window(30)));
    act(&mut runtime, WorkerAction::CycleFocusPrevious).unwrap();
    assert_eq!(runtime.backend.focused, Some(window(20)));
    act(&mut runtime, WorkerAction::CycleFocusPrevious).unwrap();
    assert_eq!(runtime.backend.focused, Some(window(10)));
}

/// Next then prev returns to the starting window, floats included.
#[test]
fn focus_next_then_prev_returns_to_the_start() {
    let mut runtime = tiles_and_focused_float();
    act(&mut runtime, WorkerAction::CycleFocus).unwrap();
    act(&mut runtime, WorkerAction::CycleFocusPrevious).unwrap();
    assert_eq!(runtime.backend.focused, Some(window(30)));
}

fn floated_second_window() -> WorkspaceRuntime<FakeBackend> {
    let mut runtime = runtime(two_windows(10));
    act(&mut runtime, WorkerAction::Retile).unwrap();
    runtime.backend.focused = Some(window(20));
    act(&mut runtime, WorkerAction::ToggleFloat).unwrap();
    runtime.backend.focused = Some(window(10));
    runtime
}

fn omit_float(runtime: &mut WorkspaceRuntime<FakeBackend>) {
    runtime.backend.observation = Some(observation_for_windows(
        "com.example.Test",
        &window(10),
        &[(window(10), frame(50))],
    ));
    act(runtime, WorkerAction::Retile).unwrap();
}

/// #179: one Complete snapshot that omits an on-screen float keeps its
/// workspace, so it still parks when that workspace is left.
#[test]
fn a_float_omitted_from_one_snapshot_keeps_its_workspace() {
    let mut runtime = floated_second_window();
    let original = runtime.backend.frames[&window(20)];
    omit_float(&mut runtime);
    assert_eq!(
        runtime.workspaces.floating_workspace_of(&window(20)),
        Some(WorkspaceId::ONE)
    );
    assert_eq!(runtime.backend.frames[&window(20)], original);

    act(
        &mut runtime,
        WorkerAction::SwitchWorkspace(WorkspaceId::TWO),
    )
    .unwrap();
    assert!(runtime.parked.contains_key(&window(20)));
    assert!(
        !outputs()
            .main()
            .frame
            .contains_center(runtime.backend.frames[&window(20)])
    );

    runtime.backend.observation = Some(observation_for_windows(
        "com.example.Test",
        &window(20),
        &[(window(10), frame(50)), (window(20), frame(500))],
    ));
    act(&mut runtime, WorkerAction::Retile).unwrap();
    assert_eq!(
        runtime.workspaces.floating_workspace_of(&window(20)),
        Some(WorkspaceId::ONE)
    );
}

/// #179 bound: a float the window server no longer shows, or one omitted past
/// the settle, is forgotten as before.
#[test]
fn an_omitted_float_off_screen_or_past_the_settle_is_forgotten() {
    let mut runtime = floated_second_window();
    runtime.backend.ordered_out.insert(20);
    omit_float(&mut runtime);
    assert_eq!(runtime.workspaces.floating_workspace_of(&window(20)), None);

    let mut runtime = floated_second_window();
    omit_float(&mut runtime);
    runtime.omitted_windows.insert(
        window(20),
        cycle::Omission::new(Instant::now() - OMISSION_SETTLE * 2),
    );
    act(&mut runtime, WorkerAction::Retile).unwrap();
    assert_eq!(runtime.workspaces.floating_workspace_of(&window(20)), None);
}

#[test]
fn guarded_omitted_tile_schedules_prompt_close_recheck_until_window_leaves_screen() {
    let kept = window(10);
    let closing = window(20);
    let mut runtime = runtime(two_windows(10));
    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();
    assert_eq!(
        runtime
            .workspaces
            .engine(WorkspaceId::ONE)
            .model()
            .workspace
            .slots
            .len(),
        2
    );

    // Closing window disappears from AX immediately while WindowServer still
    // animates it on screen.
    runtime.backend.observation = Some(observation_for_windows(
        "com.example.Test",
        &kept,
        &[(kept.clone(), frame(50))],
    ));
    let before = Instant::now();
    let status = runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();
    assert_eq!(
        runtime
            .workspaces
            .engine(WorkspaceId::ONE)
            .model()
            .workspace
            .slots
            .len(),
        2,
        "slot stays guarded while closing window is still on screen"
    );
    let recheck = status
        .close_recheck
        .expect("guard_omitted_windows must schedule a prompt close_recheck");
    assert!(
        recheck <= before + Duration::from_millis(200),
        "close_recheck should fire promptly (~75ms), not wait for the 1s fallback"
    );

    // Once the close animation finishes and WindowServer drops the window, the
    // next cycle retires the slot and clears close_recheck.
    runtime.backend.frames.remove(&closing);
    let settled = runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();
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
    assert_eq!(settled.close_recheck, None);
}
