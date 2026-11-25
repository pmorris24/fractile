use super::super::focus::FOCUS_SETTLE;
use super::*;

/// Three tiles side by side on the main display, the first one focused.
fn three_columns() -> (WorkspaceRuntime<FakeBackend>, [crate::PhysicalWindowId; 3]) {
    let windows = [window(10), window(20), window(30)];
    let backend = FakeBackend {
        focused: Some(windows[0].clone()),
        ..FakeBackend::default()
    };
    let mut runtime = runtime(backend);
    for window in &windows {
        bind(&mut runtime, WorkspaceId::ONE, window);
    }
    (runtime, windows)
}

fn focus(runtime: &mut WorkspaceRuntime<FakeBackend>, direction: Direction) {
    runtime
        .focus_relative(direction, outputs(), &AtomicBool::new(false))
        .unwrap();
}

fn focus_calls(runtime: &WorkspaceRuntime<FakeBackend>) -> Vec<u32> {
    runtime
        .backend
        .calls
        .iter()
        .filter_map(|call| match call {
            BackendCall::Focus(window) => Some(window.window_id),
            _ => None,
        })
        .collect()
}

#[test]
fn quick_presses_step_on_while_accessibility_still_reports_the_old_window() {
    let (mut runtime, [first, second, _]) = three_columns();

    focus(&mut runtime, Direction::Right);
    // The target app hasn't activated yet; Accessibility still reports the
    // window focus left.
    runtime.backend.focused = Some(first.clone());
    focus(&mut runtime, Direction::Right);
    assert_eq!(focus_calls(&runtime), vec![20, 30]);

    // Accessibility catching up to the middle of the run is still stale.
    runtime.backend.focused = Some(second);
    focus(&mut runtime, Direction::Right);
    assert_eq!(focus_calls(&runtime), vec![20, 30]);
    focus(&mut runtime, Direction::Left);
    assert_eq!(focus_calls(&runtime), vec![20, 30, 20]);
}

#[test]
fn a_settled_or_different_focus_report_wins_over_the_pending_target() {
    let (mut runtime, [first, _, third]) = three_columns();

    focus(&mut runtime, Direction::Right);
    runtime.backend.focused = Some(first.clone());
    runtime.focus.age_request(FOCUS_SETTLE);
    focus(&mut runtime, Direction::Right);
    assert_eq!(focus_calls(&runtime), vec![20, 20]);

    // The user clicked the third tile: its report replaces the pending one.
    runtime.backend.focused = Some(third);
    focus(&mut runtime, Direction::Left);
    assert_eq!(focus_calls(&runtime), vec![20, 20, 20]);
}

#[test]
fn directional_keys_rescue_focus_from_a_floating_window() {
    let left = window(10);
    let right = window(20);
    let dialog = window(99);
    let mut backend = FakeBackend {
        focused: Some(dialog.clone()),
        ..FakeBackend::default()
    };
    backend.frames.insert(
        dialog.clone(),
        crate::Rect {
            x: 600,
            y: 200,
            width: 300,
            height: 200,
        },
    );
    let mut runtime = runtime(backend);
    bind(&mut runtime, WorkspaceId::ONE, &left);
    bind(&mut runtime, WorkspaceId::ONE, &right);
    runtime.workspaces.own_floating(dialog, WorkspaceId::ONE);

    focus(&mut runtime, Direction::Left);

    assert_eq!(runtime.backend.focused, Some(left));
}

#[test]
fn directional_keys_focus_a_tile_when_no_window_has_focus() {
    let (mut runtime, [first, ..]) = three_columns();
    runtime.backend.focused = None;

    focus(&mut runtime, Direction::Down);

    assert_eq!(runtime.backend.focused, Some(first));
}

#[test]
fn reordering_still_requires_a_tiled_window() {
    let (mut runtime, _) = three_columns();
    runtime.backend.focused = Some(window(99));

    let result = runtime.reorder_focused(Direction::Left, outputs(), &AtomicBool::new(false));

    assert!(result.is_err());
}

/// Two workspaces on the main display, the first one showing, and the second
/// waiting on a focus restore.
fn switch_to_second_workspace(focus_outcome: bool) -> WorkspaceRuntime<FakeBackend> {
    let shown = window(10);
    let incoming = window(20);
    let mut backend = FakeBackend {
        focused: Some(shown.clone()),
        focus_outcome: Some(focus_outcome),
        ..FakeBackend::default()
    };
    backend.frames.insert(shown.clone(), frame(50));
    backend.frames.insert(incoming.clone(), frame(500));
    let mut runtime = runtime(backend);
    let target = WorkspaceId::new(2).unwrap();
    bind(&mut runtime, WorkspaceId::ONE, &shown);
    bind(&mut runtime, target, &incoming);
    runtime.focus.restore_fullscreen_on(target);

    runtime
        .switch(target, outputs(), &AtomicBool::new(false))
        .unwrap();
    assert_eq!(focus_calls(&runtime), vec![20]);
    runtime
}

#[test]
fn a_workspace_switch_that_cannot_focus_keeps_the_focus_restore_pending() {
    let runtime = switch_to_second_workspace(false);

    assert!(
        runtime
            .focus
            .restores_fullscreen(WorkspaceId::new(2).unwrap())
    );
}

#[test]
fn a_workspace_switch_that_focuses_settles_the_focus_restore() {
    let runtime = switch_to_second_workspace(true);

    assert!(!runtime.focus.restores_any_fullscreen());
}

#[test]
fn following_focus_to_a_hidden_workspace_keeps_the_window_that_took_it() {
    // Clicking a notification activates its window on a hidden workspace; the
    // switch must not hand focus to the tile that workspace last remembered,
    // whether the window is tiled, floating, or in a Fullscreen Layout.
    for (fullscreen, floating) in [(false, false), (true, false), (false, true)] {
        let app_window = |pid, window_id| crate::PhysicalWindowId {
            app: AppInstanceId {
                pid,
                launched_at: 1,
            },
            window_id,
        };
        let current = window(10);
        let remembered = app_window(2, 20);
        let notified = app_window(3, 30);
        let target = WorkspaceId::new(2).unwrap();
        let mut backend = FakeBackend {
            focused: Some(notified.clone()),
            frontmost: Some(notified.app.clone()),
            ..FakeBackend::default()
        };
        backend.frames.insert(current.clone(), frame(50));
        let parked = [
            (remembered.clone(), frame(50)),
            (notified.clone(), frame(500)),
        ];
        for (window, normal) in &parked {
            backend
                .frames
                .insert(window.clone(), main_parked_frame(*normal));
        }
        let mut runtime = runtime_with_parked(backend, parked);
        bind(&mut runtime, WorkspaceId::ONE, &current);
        bind(&mut runtime, target, &remembered);
        if floating {
            runtime.workspaces.own_floating(notified.clone(), target);
        } else {
            bind(&mut runtime, target, &notified);
        }
        runtime.workspaces.set_deck(
            target,
            crate::workspace::DeckMode::from_fullscreen(fullscreen),
        );
        runtime.workspaces.remember_focus(target, &remembered);

        let status = runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();

        let case = format!("fullscreen {fullscreen}, floating {floating}");
        assert_eq!(status.active, vec![target, WorkspaceId::FIVE], "{case}");
        assert_eq!(runtime.backend.focused, Some(notified), "{case}");
        assert!(
            !focus_calls(&runtime).contains(&remembered.window_id),
            "{case}"
        );
    }
}

#[test]
fn a_follow_cancelled_by_the_activation_it_answers_follows_on_the_next_cycle() {
    // Accessibility reports the clicked notification's window before AppKit's
    // activation notice reaches the main thread, which then cancels the cycle
    // already switching there. The window must not stay suppressed and hidden.
    let current = window(10);
    let notified = crate::PhysicalWindowId {
        app: AppInstanceId {
            pid: 3,
            launched_at: 1,
        },
        window_id: 30,
    };
    let target = WorkspaceId::new(2).unwrap();
    let cancelled = Arc::new(AtomicBool::new(false));
    let mut backend = FakeBackend {
        focused: Some(notified.clone()),
        frontmost: Some(notified.app.clone()),
        cancel_on_position: Some(Arc::clone(&cancelled)),
        ..FakeBackend::default()
    };
    backend.frames.insert(current.clone(), frame(50));
    backend
        .frames
        .insert(notified.clone(), main_parked_frame(frame(500)));
    let mut runtime = runtime_with_parked(backend, [(notified.clone(), frame(500))]);
    bind(&mut runtime, WorkspaceId::ONE, &current);
    bind(&mut runtime, target, &notified);

    let error = runtime.cycle(outputs(), &cancelled).err().unwrap();
    assert!(error.ends_with("cancelled"), "{error}");
    // The cancelled switch rolled back: the old workspace stays up and the
    // window keeps its recovery frame.
    assert_eq!(
        runtime.workspaces.active_for(&DisplaySelector::Main),
        Some(WorkspaceId::ONE)
    );
    assert_eq!(runtime.parked.get(&notified), Some(&frame(500)));
    assert_eq!(runtime.focus.guarded_window().cloned(), None);

    let status = runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();
    assert_eq!(status.active, vec![target, WorkspaceId::FIVE]);
    assert_eq!(runtime.backend.focused, Some(notified));
}

#[test]
fn a_follow_that_fails_while_an_activation_cancels_stays_suppressed() {
    // Only a cancellation retries; a switch that genuinely failed keeps the
    // window suppressed even if an activation also raised the cancel flag.
    let current = window(10);
    let notified = crate::PhysicalWindowId {
        app: AppInstanceId {
            pid: 3,
            launched_at: 1,
        },
        window_id: 30,
    };
    let target = WorkspaceId::new(2).unwrap();
    let cancelled = Arc::new(AtomicBool::new(false));
    let mut backend = FakeBackend {
        focused: Some(notified.clone()),
        frontmost: Some(notified.app.clone()),
        cancel_on_position: Some(Arc::clone(&cancelled)),
        position_outcome: Some(macos::ApplyOutcome::Unavailable),
        ..FakeBackend::default()
    };
    backend.frames.insert(current.clone(), frame(50));
    backend
        .frames
        .insert(notified.clone(), main_parked_frame(frame(500)));
    let mut runtime = runtime_with_parked(backend, [(notified.clone(), frame(500))]);
    bind(&mut runtime, WorkspaceId::ONE, &current);
    bind(&mut runtime, target, &notified);

    let error = runtime.cycle(outputs(), &cancelled).err().unwrap();

    assert!(!error.ends_with("cancelled"), "{error}");
    assert!(cancelled.load(Ordering::Acquire));
    assert_eq!(runtime.focus.guarded_window().cloned(), Some(notified));
}

fn config_with_mouse_follows_focus(mode: &str) -> Config {
    Config::parse(&format!(
        "version = 1\n[settings]\nmouse_follows_focus = {mode:?}\n"
    ))
    .unwrap()
}

fn warp_calls(runtime: &WorkspaceRuntime<FakeBackend>) -> Vec<(i32, i32)> {
    runtime
        .backend
        .calls
        .iter()
        .filter_map(|call| match call {
            BackendCall::WarpPointer(point) => Some(*point),
            _ => None,
        })
        .collect()
}

#[test]
fn mouse_follows_focus_off_never_warps() {
    let main_win = window(10);
    let sec_win = window(20);
    let mut backend = FakeBackend {
        focused: Some(main_win.clone()),
        pointer: Some((100, 100)),
        ..FakeBackend::default()
    };
    backend.frames.insert(main_win.clone(), frame(50));
    backend.frames.insert(sec_win.clone(), frame(1050));
    let mut runtime = WorkspaceRuntime::with_backend_and_config(
        ManagementScope::Both,
        backend,
        HashMap::new(),
        config_with_mouse_follows_focus("off"),
    );
    bind(&mut runtime, WorkspaceId::ONE, &main_win);
    bind(&mut runtime, WorkspaceId::SIX, &sec_win);

    let cancelled = AtomicBool::new(false);
    focus(&mut runtime, Direction::Right);
    runtime
        .switch(WorkspaceId::SIX, outputs(), &cancelled)
        .unwrap();
    runtime
        .switch(WorkspaceId::ONE, outputs(), &cancelled)
        .unwrap();
    runtime
        .workspace_back_and_forth(outputs(), &cancelled)
        .unwrap();
    runtime
        .move_workspace_to_display(DisplayTarget::Next, outputs(), &cancelled)
        .unwrap();
    runtime
        .move_focused(WorkspaceId::ONE, outputs(), &cancelled)
        .unwrap();

    assert!(warp_calls(&runtime).is_empty());
    assert_eq!(runtime.backend.pointer, Some((100, 100)));
}

#[test]
fn mouse_follows_focus_display_warps_across_displays_and_skips_same_display() {
    let main_left = window(10);
    let main_right = window(11);
    let sec_left = window(20);
    let sec_right = window(21);
    let backend = FakeBackend {
        focused: Some(main_left.clone()),
        pointer: Some((100, 100)),
        ..FakeBackend::default()
    };
    let mut runtime = WorkspaceRuntime::with_backend_and_config(
        ManagementScope::Both,
        backend,
        HashMap::new(),
        config_with_mouse_follows_focus("display"),
    );
    bind(&mut runtime, WorkspaceId::ONE, &main_left);
    bind(&mut runtime, WorkspaceId::ONE, &main_right);
    bind(&mut runtime, WorkspaceId::FIVE, &sec_left);
    bind(&mut runtime, WorkspaceId::FIVE, &sec_right);

    let cancelled = AtomicBool::new(false);
    // Same display: pointer is already on main (0..1000) -> no warp.
    focus(&mut runtime, Direction::Right);
    assert_eq!(runtime.backend.focused, Some(main_right));
    assert!(warp_calls(&runtime).is_empty());

    // Cross display edge to secondary's left tile (1000..1500, 0..800 -> center (1250, 400)).
    focus(&mut runtime, Direction::Right);
    assert_eq!(runtime.backend.focused, Some(sec_left));
    assert_eq!(warp_calls(&runtime), vec![(1250, 400)]);

    // Moving within secondary display now sees pointer on secondary -> no warp.
    focus(&mut runtime, Direction::Right);
    assert_eq!(runtime.backend.focused, Some(sec_right));
    assert_eq!(warp_calls(&runtime), vec![(1250, 400)]);

    // Explicit switch back to workspace 1 (main) warps to main's focused tile center.
    runtime.record_focused_workspace(WorkspaceId::FIVE);
    runtime
        .execute(
            WorkerCommand::Action(WorkerAction::SwitchWorkspace(WorkspaceId::ONE), outputs()),
            &cancelled,
        )
        .unwrap();
    assert_eq!(warp_calls(&runtime), vec![(1250, 400), (250, 400)]);

    // Option+Tab back to visible workspace 5 on secondary warps back to secondary.
    runtime
        .execute(
            WorkerCommand::Action(WorkerAction::WorkspaceBackAndForth, outputs()),
            &cancelled,
        )
        .unwrap();
    assert_eq!(
        warp_calls(&runtime),
        vec![(1250, 400), (250, 400), (1250, 400)]
    );
}

#[test]
fn mouse_follows_focus_display_centers_on_empty_workspace() {
    let main_win = window(10);
    let backend = FakeBackend {
        focused: Some(main_win.clone()),
        pointer: Some((100, 100)),
        ..FakeBackend::default()
    };
    let mut runtime = WorkspaceRuntime::with_backend_and_config(
        ManagementScope::Both,
        backend,
        HashMap::new(),
        config_with_mouse_follows_focus("display"),
    );
    bind(&mut runtime, WorkspaceId::ONE, &main_win);

    let cancelled = AtomicBool::new(false);
    // Secondary workspace 6 is empty -> warp to secondary display center (1500, 400).
    runtime
        .switch(WorkspaceId::SIX, outputs(), &cancelled)
        .unwrap();
    assert_eq!(warp_calls(&runtime), vec![(1500, 400)]);

    // Switching to another empty workspace on secondary while pointer is already there -> no warp.
    runtime
        .switch(WorkspaceId::new(7).unwrap(), outputs(), &cancelled)
        .unwrap();
    assert_eq!(warp_calls(&runtime), vec![(1500, 400)]);
}

#[test]
fn mouse_follows_focus_display_warps_on_move_workspace_and_move_focused() {
    let main_win = window(10);
    let mut backend = FakeBackend {
        focused: Some(main_win.clone()),
        pointer: Some((100, 100)),
        ..FakeBackend::default()
    };
    backend.frames.insert(main_win.clone(), frame(50));
    let mut runtime = WorkspaceRuntime::with_backend_and_config(
        ManagementScope::Both,
        backend,
        HashMap::new(),
        config_with_mouse_follows_focus("display"),
    );
    bind(&mut runtime, WorkspaceId::ONE, &main_win);

    let cancelled = AtomicBool::new(false);
    // Move workspace 1 from main to secondary -> warps to moved tile center (1500, 400).
    runtime
        .move_workspace_to_display(DisplayTarget::Next, outputs(), &cancelled)
        .unwrap();
    assert_eq!(warp_calls(&runtime), vec![(1500, 400)]);

    // Now move the focused window onto workspace 2 (active on main after move) -> warps to main.
    runtime
        .move_focused(WorkspaceId::TWO, outputs(), &cancelled)
        .unwrap();
    assert_eq!(warp_calls(&runtime), vec![(1500, 400), (500, 400)]);

    // Moving onto a hidden workspace (workspace 7 on secondary) does NOT move focus or warp.
    runtime
        .move_focused(WorkspaceId::new(7).unwrap(), outputs(), &cancelled)
        .unwrap();
    assert_eq!(warp_calls(&runtime), vec![(1500, 400), (500, 400)]);
}

#[test]
fn mouse_follows_focus_window_warps_within_display_and_skips_when_already_inside() {
    let left = window(10);
    let right = window(20);
    let backend = FakeBackend {
        focused: Some(left.clone()),
        pointer: Some((100, 100)),
        ..FakeBackend::default()
    };
    let mut runtime = WorkspaceRuntime::with_backend_and_config(
        ManagementScope::Both,
        backend,
        HashMap::new(),
        config_with_mouse_follows_focus("window"),
    );
    bind(&mut runtime, WorkspaceId::ONE, &left);
    bind(&mut runtime, WorkspaceId::ONE, &right);

    let cancelled = AtomicBool::new(false);
    // Tile 1 is [0..500), tile 2 is [500..1000). Pointer at (100, 100) is outside tile 2 -> warp.
    focus(&mut runtime, Direction::Right);
    assert_eq!(warp_calls(&runtime), vec![(750, 400)]);

    // CycleFocus back to tile 1 -> warps to (250, 400).
    runtime.cycle_focus(outputs(), &cancelled).unwrap();
    assert_eq!(warp_calls(&runtime), vec![(750, 400), (250, 400)]);

    // Pointer already inside tile 2 -> focusing tile 2 does NOT warp.
    runtime.backend.pointer = Some((600, 300));
    focus(&mut runtime, Direction::Right);
    assert_eq!(warp_calls(&runtime), vec![(750, 400), (250, 400)]);
}

#[test]
fn mouse_follows_focus_never_warps_on_passive_cycle_focus_follow_or_mouse_down() {
    let current = window(10);
    let hidden_win = crate::PhysicalWindowId {
        app: AppInstanceId {
            pid: 2,
            launched_at: 1,
        },
        window_id: 20,
    };
    let target = WorkspaceId::new(7).unwrap();
    let mut backend = FakeBackend {
        focused: Some(hidden_win.clone()),
        frontmost: Some(hidden_win.app.clone()),
        pointer: Some((100, 100)),
        ..FakeBackend::default()
    };
    backend.frames.insert(current.clone(), frame(50));
    backend
        .frames
        .insert(hidden_win.clone(), main_parked_frame(frame(1100)));
    let mut runtime = WorkspaceRuntime::with_backend_and_config(
        ManagementScope::Both,
        backend,
        HashMap::from([(hidden_win.clone(), frame(1100))]),
        config_with_mouse_follows_focus("window"),
    );
    bind(&mut runtime, WorkspaceId::ONE, &current);
    bind(&mut runtime, target, &hidden_win);

    let cancelled = AtomicBool::new(false);
    // Passive cycle() triggering focus-follow onto hidden workspace 7 on secondary NEVER warps!
    runtime.cycle(outputs(), &cancelled).unwrap();
    assert!(warp_calls(&runtime).is_empty());

    // Same via WorkerAction::Retile.
    let other_hidden = crate::PhysicalWindowId {
        app: AppInstanceId {
            pid: 3,
            launched_at: 1,
        },
        window_id: 30,
    };
    runtime
        .backend
        .frames
        .insert(other_hidden.clone(), main_parked_frame(frame(1200)));
    runtime
        .parked
        .insert(other_hidden.clone(), frame(1200), None);
    bind(&mut runtime, WorkspaceId::new(8).unwrap(), &other_hidden);
    runtime.backend.focused = Some(other_hidden.clone());
    runtime.backend.frontmost = Some(other_hidden.app);
    runtime
        .execute(
            WorkerCommand::Action(WorkerAction::Retile, outputs()),
            &cancelled,
        )
        .unwrap();
    assert!(warp_calls(&runtime).is_empty());

    // Never warps while mouse_down is true, even on explicit keyboard actions!
    runtime.backend.mouse_down = true;
    runtime
        .execute(
            WorkerCommand::Action(WorkerAction::SwitchWorkspace(WorkspaceId::ONE), outputs()),
            &cancelled,
        )
        .unwrap();
    assert!(warp_calls(&runtime).is_empty());
}

/// #120: reselecting the active split workspace keeps focus on the tile of
/// that workspace that already has it, rather than its remembered tile.
#[test]
fn reselecting_the_active_split_workspace_keeps_the_focused_tile() {
    let first = window(10);
    let second = window(20);
    let mut backend = FakeBackend {
        focused: Some(first.clone()),
        ..FakeBackend::default()
    };
    backend.frames.insert(first.clone(), frame(50));
    backend.frames.insert(second.clone(), frame(500));
    let mut runtime = runtime(backend);
    bind(&mut runtime, WorkspaceId::ONE, &first);
    bind(&mut runtime, WorkspaceId::ONE, &second);
    let cancelled = AtomicBool::new(false);
    let two = WorkspaceId::new(2).unwrap();

    runtime.switch(two, outputs(), &cancelled).unwrap();
    runtime
        .switch(WorkspaceId::ONE, outputs(), &cancelled)
        .unwrap();
    assert_eq!(
        runtime.workspaces.preferred_focus(WorkspaceId::ONE),
        Some(first.clone())
    );
    runtime.backend.focused = Some(second.clone());

    runtime
        .switch(WorkspaceId::ONE, outputs(), &cancelled)
        .unwrap();
    assert_eq!(runtime.backend.focused, Some(second.clone()));

    // Entering from another workspace still lands on the remembered tile.
    runtime.switch(two, outputs(), &cancelled).unwrap();
    runtime.backend.focused = None;
    runtime.workspaces.remember_focus(WorkspaceId::ONE, &first);
    runtime
        .switch(WorkspaceId::ONE, outputs(), &cancelled)
        .unwrap();
    assert_eq!(runtime.backend.focused, Some(first));
}

/// #161: pointer-initiated workspace selection (menu bar or HUD click) switches
/// and focuses the target workspace without warping the pointer away, whereas
/// an equivalent keyboard/CLI switch still warps under mouse_follows_focus.
#[test]
fn pointer_workspace_selection_preserves_pointer_while_keyboard_switch_warps() {
    let main_win = window(10);
    let sec_six = window(20);
    let sec_seven = window(21);
    let seven = WorkspaceId::new(7).unwrap();
    let cancelled = AtomicBool::new(false);

    for mode in ["display", "window"] {
        let mut backend = FakeBackend {
            focused: Some(main_win.clone()),
            pointer: Some((200, 12)),
            ..FakeBackend::default()
        };
        backend.frames.insert(main_win.clone(), frame(50));
        backend.frames.insert(sec_six.clone(), frame(1050));
        backend
            .frames
            .insert(sec_seven.clone(), main_parked_frame(frame(1050)));
        let mut runtime = WorkspaceRuntime::with_backend_and_config(
            ManagementScope::Both,
            backend,
            HashMap::from([(sec_seven.clone(), frame(1050))]),
            config_with_mouse_follows_focus(mode),
        );
        bind(&mut runtime, WorkspaceId::ONE, &main_win);
        bind(&mut runtime, WorkspaceId::SIX, &sec_six);
        bind(&mut runtime, seven, &sec_seven);

        // Menu-bar / HUD click to hidden secondary workspace 7 switches and
        // focuses it, leaving the pointer where the click happened.
        runtime
            .execute(
                WorkerCommand::Action(WorkerAction::SelectWorkspace(seven), outputs()),
                &cancelled,
            )
            .unwrap();
        assert_eq!(runtime.backend.focused, Some(sec_seven.clone()), "{mode}");
        assert!(warp_calls(&runtime).is_empty(), "{mode}");
        assert_eq!(runtime.backend.pointer, Some((200, 12)), "{mode}");

        // Clicking an already-visible cross-display workspace also preserves pointer.
        runtime
            .execute(
                WorkerCommand::Action(WorkerAction::SelectWorkspace(seven), outputs()),
                &cancelled,
            )
            .unwrap();
        assert!(warp_calls(&runtime).is_empty(), "{mode}");
        assert_eq!(runtime.backend.pointer, Some((200, 12)), "{mode}");

        // Control: keyboard/CLI switch to workspace 6 on secondary DOES warp.
        runtime
            .execute(
                WorkerCommand::Action(WorkerAction::SwitchWorkspace(WorkspaceId::SIX), outputs()),
                &cancelled,
            )
            .unwrap();
        assert_eq!(runtime.backend.focused, Some(sec_six.clone()), "{mode}");
        assert_eq!(warp_calls(&runtime), vec![(1500, 400)], "{mode}");
    }
}

/// #170: Display mouse-follow mode treats a display's menu-bar and Dock strips
/// as part of that physical display, both with synthetic visible-only frames
/// and with full physical bounds.
#[test]
fn mouse_follows_focus_display_preserves_pointer_in_menu_bar_and_dock_strips() {
    let visible = crate::Rect {
        x: 0,
        y: 25,
        width: 1000,
        height: 750,
    };
    let displays = macos::DisplayOutputs::new(vec![
        macos::DisplayOutput {
            id: "main".into(),
            name: "Main".into(),
            frame: visible,
            is_main: true,
        },
        macos::DisplayOutput {
            id: "second".into(),
            name: "Second".into(),
            frame: crate::Rect { x: 1000, ..visible },
            is_main: false,
        },
    ])
    .unwrap();
    let run_switch = |mode: &str, pointer: (i32, i32), mouse_down: bool| {
        let outgoing = window(10);
        let incoming = window(20);
        let mut backend = FakeBackend {
            focused: Some(outgoing.clone()),
            pointer: Some(pointer),
            mouse_down,
            ..FakeBackend::default()
        };
        backend.frames.insert(outgoing.clone(), visible);
        backend.frames.insert(incoming.clone(), visible);
        let mut runtime = WorkspaceRuntime::with_backend_and_config(
            ManagementScope::Both,
            backend,
            HashMap::new(),
            config_with_mouse_follows_focus(mode),
        );
        bind(&mut runtime, WorkspaceId::ONE, &outgoing);
        bind(&mut runtime, WorkspaceId::TWO, &incoming);
        let result = runtime
            .execute(
                WorkerCommand::Action(
                    WorkerAction::SwitchWorkspace(WorkspaceId::TWO),
                    displays.clone(),
                ),
                &AtomicBool::new(false),
            )
            .unwrap();
        match result {
            WorkerResult::Status { result, .. } => {
                result.unwrap();
            }
            _ => panic!("unexpected worker result"),
        }
        assert_eq!(runtime.backend.focused, Some(incoming));
        (warp_calls(&runtime), runtime.backend.pointer)
    };

    // Already on main display (visible area, menu bar, or Dock): preserve pointer.
    assert_eq!(
        run_switch("display", (500, 300), false),
        (vec![], Some((500, 300)))
    );
    assert_eq!(
        run_switch("display", (500, 10), false),
        (vec![], Some((500, 10)))
    );
    assert_eq!(
        run_switch("display", (500, 790), false),
        (vec![], Some((500, 790)))
    );
    // Other display (visible area or its menu bar): warp to main's incoming window.
    assert_eq!(
        run_switch("display", (1500, 300), false),
        (vec![(500, 400)], Some((500, 400)))
    );
    assert_eq!(
        run_switch("display", (1500, 10), false),
        (vec![(500, 400)], Some((500, 400)))
    );
    // Window mode still warps from the menu bar into the focused window.
    assert_eq!(
        run_switch("window", (500, 10), false),
        (vec![(500, 400)], Some((500, 400)))
    );
    // Mouse button held still suppresses warping.
    assert_eq!(
        run_switch("display", (1500, 300), true),
        (vec![], Some((1500, 300)))
    );
}

/// #175: swiping with the pointer in a secondary display's menu-bar or Dock
/// strip switches the secondary display instead of falling back to main.
#[test]
fn swipe_in_secondary_menu_bar_or_dock_targets_secondary_display() {
    let rect = |x, y, width, height| crate::Rect {
        x,
        y,
        width,
        height,
    };
    let swipe_state = |point: (i32, i32), height: i32| -> Vec<WorkspaceId> {
        let displays = macos::DisplayOutputs::new(vec![
            macos::DisplayOutput {
                id: "main-id".into(),
                name: "Main".into(),
                frame: rect(0, 25, 1000, height),
                is_main: true,
            },
            macos::DisplayOutput {
                id: "secondary-id".into(),
                name: "Secondary".into(),
                frame: rect(1000, 25, 1000, height),
                is_main: false,
            },
        ])
        .unwrap();
        let mut rt = runtime(FakeBackend::default());
        for (id, workspace, x) in [(10, 1, 50), (11, 2, 450), (12, 5, 1050), (13, 7, 1450)] {
            let win = window(id);
            rt.backend.frames.insert(win.clone(), rect(x, 50, 400, 300));
            bind(&mut rt, WorkspaceId::new(workspace).unwrap(), &win);
        }
        let mut tracker = crate::swipe::SwipeTracker::default();
        assert_eq!(tracker.frame(0.0, &[(0.6, 0.5); 3]), None);
        let (swipe, fingers) = tracker.frame(0.1, &[(0.3, 0.5); 3]).unwrap();
        let result = rt.execute(
            WorkerCommand::Action(WorkerAction::Swipe(swipe, fingers, point), displays.clone()),
            &AtomicBool::new(false),
        );
        match result.unwrap() {
            WorkerResult::Status { result, .. } => {
                assert_eq!(result.unwrap().failed, 0);
            }
            _ => panic!("unexpected worker result"),
        }
        rt.visible_workspaces(&displays)
            .iter()
            .map(|entry| entry.workspace)
            .collect()
    };

    let seven = WorkspaceId::new(7).unwrap();
    // Secondary visible area, menu bar, and Dock all advance secondary 5 -> 7.
    assert_eq!(swipe_state((1500, 400), 775), [WorkspaceId::ONE, seven]);
    assert_eq!(swipe_state((1500, 10), 775), [WorkspaceId::ONE, seven]);
    assert_eq!(swipe_state((1500, 790), 750), [WorkspaceId::ONE, seven]);
    // Main menu bar advances main 1 -> 2 and leaves secondary on 5.
    assert_eq!(
        swipe_state((500, 10), 775),
        [WorkspaceId::TWO, WorkspaceId::FIVE]
    );
}

#[test]
fn focus_next_and_prev_follow_pointer_onto_floating_windows() {
    for backward in [false, true] {
        let windows = [
            (window(10), frame(50)),
            (window(20), frame(500)),
            (window(30), frame(700)),
        ];
        let backend = FakeBackend {
            frames: windows.iter().cloned().collect(),
            focused: Some(window(30)),
            frontmost: Some(window(30).app),
            pointer: Some((100, 100)),
            observation: Some(observation_for_windows(
                "com.example.Test",
                &window(30),
                &windows,
            )),
            ..FakeBackend::default()
        };
        let mut rt = WorkspaceRuntime::with_backend_and_config(
            ManagementScope::Both,
            backend,
            HashMap::new(),
            config_with_mouse_follows_focus("window"),
        );
        let run = |rt: &mut WorkspaceRuntime<FakeBackend>, action: WorkerAction| {
            let Some(WorkerResult::Status { result, .. }) = rt.execute(
                WorkerCommand::Action(action, outputs()),
                &AtomicBool::new(false),
            ) else {
                panic!("expected worker status");
            };
            result.unwrap();
        };
        run(&mut rt, WorkerAction::Retile);
        run(&mut rt, WorkerAction::ToggleFloat);
        rt.backend.calls.clear();
        rt.backend.focused = Some(window(if backward { 10 } else { 20 }));

        let target = rt.backend.frames[&window(30)];
        let expected = (target.x + target.width / 2, target.y + target.height / 2);
        run(
            &mut rt,
            if backward {
                WorkerAction::CycleFocusPrevious
            } else {
                WorkerAction::CycleFocus
            },
        );
        assert_eq!(rt.backend.focused, Some(window(30)));
        assert_eq!(warp_calls(&rt), vec![expected]);
    }
}
