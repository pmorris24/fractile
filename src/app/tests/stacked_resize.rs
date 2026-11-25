//! An automatic stack is temporary, so an edit made while it shows must not
//! save the stack as the requested split: a keyboard resize (#197), a swap, a
//! drag-nest, or a perpendicular move.
use super::*;

fn stacked_pair() -> (WorkspaceRuntime<FakeBackend>, [crate::PhysicalWindowId; 2]) {
    let windows = [window(10), window(11)];
    let mut backend = FakeBackend {
        focused: Some(windows[0].clone()),
        ..FakeBackend::default()
    };
    for w in &windows {
        backend.frames.insert(w.clone(), frame(0));
        backend.minimum_widths.insert(w.clone(), 600);
    }
    let mut rt = runtime(backend);
    for w in &windows {
        bind(&mut rt, WorkspaceId::ONE, w);
    }
    for _ in 0..2 {
        rt.layout_workspace(
            WorkspaceId::ONE,
            outputs().main().frame,
            &AtomicBool::new(false),
        );
    }
    (rt, windows)
}

fn side_by_side_when_wider(resize: bool) -> bool {
    let (mut rt, [a, b]) = stacked_pair();
    assert_eq!(
        rt.backend.frames[&a].x, rt.backend.frames[&b].x,
        "starts stacked"
    );
    let before = rt.workspaces.state();
    if resize {
        let stacked = (rt.backend.frames[&a], rt.backend.frames[&b]);
        rt.resize_focused(ResizeDirection::Grow, outputs(), &AtomicBool::new(false))
            .unwrap();
        assert_eq!(
            rt.workspaces.state(),
            before,
            "the stack never reaches saved state"
        );
        assert_ne!(
            (rt.backend.frames[&a], rt.backend.frames[&b]),
            stacked,
            "the resize still changes the stack shown"
        );
    }
    let wide = crate::Rect {
        width: 1400,
        ..outputs().main().frame
    };
    rt.layout_workspace(WorkspaceId::ONE, wide, &AtomicBool::new(false));
    rt.backend.frames[&a].x != rt.backend.frames[&b].x
}

#[test]
fn resizing_a_temporary_stack_keeps_the_requested_row() {
    assert!(side_by_side_when_wider(true));
}

#[test]
fn control_the_row_returns_without_a_resize() {
    assert!(side_by_side_when_wider(false));
}

#[test]
fn flipping_there_and_back_after_a_resize_keeps_the_row() {
    // Flipping the split there and back after a resize still lands on the row.
    let (mut rt, [a, b]) = stacked_pair();
    rt.resize_focused(ResizeDirection::Grow, outputs(), &AtomicBool::new(false))
        .unwrap();
    rt.toggle_layout(outputs(), &AtomicBool::new(false))
        .unwrap();
    rt.toggle_layout(outputs(), &AtomicBool::new(false))
        .unwrap();
    let wide = crate::Rect {
        width: 1400,
        ..outputs().main().frame
    };
    rt.layout_workspace(WorkspaceId::ONE, wide, &AtomicBool::new(false));
    assert_ne!(rt.backend.frames[&a].x, rt.backend.frames[&b].x);
}

fn swap_retains_requested_row(pointer: bool) {
    let (mut runtime, windows) = stacked_pair();
    let [a, b] = &windows;
    let command = if pointer {
        let target = runtime.backend.frames[b];
        WorkerCommand::Gesture(
            FinalGesture::drop(
                a.clone(),
                (target.x + target.width / 2, target.y + target.height / 2),
            ),
            false,
            outputs(),
        )
    } else {
        WorkerCommand::Action(WorkerAction::Reorder(Direction::Down), outputs())
    };
    let Some(WorkerResult::Status { result, .. }) =
        runtime.execute(command, &AtomicBool::new(false))
    else {
        panic!("missing status");
    };
    assert_eq!(result.unwrap().failed, 0);
    assert_eq!(
        runtime.workspaces.engine(WorkspaceId::ONE).bound_windows(),
        [b.clone(), a.clone()]
    );
    assert_eq!(
        runtime
            .workspaces
            .engine(WorkspaceId::ONE)
            .layout()
            .root_axis(),
        crate::Axis::Horizontal
    );

    // On a wide display the reversed row returns, including after persistence round-trip.
    let wide = crate::Rect {
        width: 1400,
        ..outputs().main().frame
    };
    runtime.layout_workspace(WorkspaceId::ONE, wide, &AtomicBool::new(false));
    assert_eq!(runtime.backend.frames[b].x, 0);
    assert!(runtime.backend.frames[a].x > runtime.backend.frames[b].x);

    let state = runtime.backend.workspace_state.clone().unwrap();
    let backend = FakeBackend {
        frames: runtime.backend.frames.clone(),
        focused: Some(a.clone()),
        ..FakeBackend::default()
    };
    let mut restored = WorkspaceRuntime::with_backend_config_state(
        ManagementScope::Both,
        backend,
        HashMap::new(),
        Config::default(),
        Some(state),
    );
    restored.layout_workspace(WorkspaceId::ONE, wide, &AtomicBool::new(false));
    assert_eq!(restored.backend.frames[b].x, 0);
    assert!(restored.backend.frames[a].x > restored.backend.frames[b].x);
}

#[test]
fn keyboard_swap_of_temporarily_stacked_tiles_keeps_requested_row() {
    swap_retains_requested_row(false);
}

#[test]
fn pointer_swap_of_temporarily_stacked_tiles_keeps_requested_row() {
    swap_retains_requested_row(true);
}

/// `| A | B | C |` (windows 10, 20 and 30) on the main display with these
/// minimum widths, laid out until the display shows the fit.
fn three_with_minimums(
    minimums: [i32; 3],
) -> (WorkspaceRuntime<FakeBackend>, [crate::PhysicalWindowId; 3]) {
    let windows = [window(10), window(20), window(30)];
    let mut backend = FakeBackend {
        focused: Some(windows[0].clone()),
        ..FakeBackend::default()
    };
    for (w, minimum) in windows.iter().zip(minimums) {
        backend.frames.insert(w.clone(), frame(0));
        backend.minimum_widths.insert(w.clone(), minimum);
    }
    let mut runtime = runtime(backend);
    for w in &windows {
        bind(&mut runtime, WorkspaceId::ONE, w);
    }
    for _ in 0..2 {
        runtime.layout_workspace(
            WorkspaceId::ONE,
            outputs().main().frame,
            &AtomicBool::new(false),
        );
    }
    runtime.backend.workspace_state = None;
    (runtime, windows)
}

fn drop_on(
    runtime: &mut WorkspaceRuntime<FakeBackend>,
    window: &crate::PhysicalWindowId,
    point: (i32, i32),
    preview: bool,
) {
    let command = WorkerCommand::Gesture(
        FinalGesture::drop(window.clone(), point),
        preview,
        outputs(),
    );
    let Some(WorkerResult::Status { result, .. }) =
        runtime.execute(command, &AtomicBool::new(false))
    else {
        panic!("missing status");
    };
    assert_eq!(result.unwrap().failed, 0);
}

fn requested_axis(runtime: &WorkspaceRuntime<FakeBackend>) -> crate::Axis {
    runtime
        .workspaces
        .engine(WorkspaceId::ONE)
        .layout()
        .root_axis()
}

const WIDE: crate::Rect = crate::Rect {
    x: 0,
    y: 0,
    width: 1400,
    height: 800,
};

/// Where the requested row fits, `| B | A/C |`: B in its own half, A above C.
fn b_beside_a_over_c(
    runtime: &WorkspaceRuntime<FakeBackend>,
    [a, b, c]: &[crate::PhysicalWindowId; 3],
) -> bool {
    let frames = &runtime.backend.frames;
    frames[b] == crate::Rect { width: 700, ..WIDE }
        && frames[a].x == 700
        && frames[c].x == 700
        && frames[a].y < frames[c].y
}

/// Dropping a tile near a stacked tile's edge nests it in the requested
/// layout: `| A | B | C |` shown as `| A | B/C |`, A dragged to C's top edge
/// saves `| B | A/C |`, not the temporary stack with A inside it. The preview
/// shows it without mutating or persisting the layout, and mouse-up commits it.
#[test]
fn drag_nest_into_a_temporary_stack_keeps_the_requested_layout() {
    let (mut runtime, [a, b, c]) = three_with_minimums([400, 250, 450]);
    let target = runtime.backend.frames[&c];
    assert_eq!(
        runtime.backend.frames[&b].x, target.x,
        "B and C start stacked"
    );
    let point = (target.x + target.width / 2, target.y + 10);

    drop_on(&mut runtime, &a, point, true);
    assert!(
        runtime.backend.workspace_state.is_none(),
        "a preview never persists"
    );
    assert_eq!(requested_axis(&runtime), crate::Axis::Horizontal);

    drop_on(&mut runtime, &a, point, false);
    assert_eq!(
        requested_axis(&runtime),
        crate::Axis::Horizontal,
        "the requested row survives"
    );
    let engine = runtime.workspaces.engine(WorkspaceId::ONE);
    assert!(
        engine
            .model()
            .workspace
            .slots
            .iter()
            .all(|slot| engine.layout().weight(slot.id) == Some(1)),
        "the fit's weights are not saved"
    );

    // Where the row fits, B has its own half and A sits above C, also after
    // a persistence round-trip.
    let windows = [a.clone(), b, c];
    runtime.layout_workspace(WorkspaceId::ONE, WIDE, &AtomicBool::new(false));
    assert!(
        b_beside_a_over_c(&runtime, &windows),
        "{:?}",
        runtime.backend.frames
    );

    let backend = FakeBackend {
        frames: runtime.backend.frames.clone(),
        focused: Some(a.clone()),
        ..FakeBackend::default()
    };
    let mut restored = WorkspaceRuntime::with_backend_config_state(
        ManagementScope::Both,
        backend,
        HashMap::new(),
        Config::default(),
        Some(runtime.backend.workspace_state.clone().unwrap()),
    );
    restored.layout_workspace(WorkspaceId::ONE, WIDE, &AtomicBool::new(false));
    assert!(
        b_beside_a_over_c(&restored, &windows),
        "{:?}",
        restored.backend.frames
    );
}

/// The same drop with no preview first edits the requested layout on the
/// drop itself.
#[test]
fn drag_nest_without_a_preview_keeps_the_requested_layout() {
    let (mut runtime, windows) = three_with_minimums([400, 250, 450]);
    let [a, _, c] = &windows;
    let target = runtime.backend.frames[c];
    let requested = runtime.workspaces.engine(WorkspaceId::ONE).layout().clone();

    drop_on(
        &mut runtime,
        a,
        (target.x + target.width / 2, target.y + 10),
        false,
    );

    assert_ne!(
        runtime.workspaces.engine(WorkspaceId::ONE).layout(),
        &requested,
        "the drop itself nests A"
    );
    assert_eq!(requested_axis(&runtime), crate::Axis::Horizontal);
    assert!(runtime.backend.workspace_state.is_some());
    runtime.layout_workspace(WorkspaceId::ONE, WIDE, &AtomicBool::new(false));
    assert!(
        b_beside_a_over_c(&runtime, &windows),
        "{:?}",
        runtime.backend.frames
    );
}

/// Previews evaluate against the gesture-start layout (#340), so holding A on
/// B's top edge in `| A | B/C |` keeps targeting B across repeated previews
/// and commits `| A/B | C |`, matching a drop there with no previews.
#[test]
fn drag_nest_on_the_other_stacked_tile_commits_against_gesture_start() {
    let (mut runtime, windows) = three_with_minimums([400, 250, 450]);
    let [a, b, c] = &windows;
    let target = runtime.backend.frames[b];
    let point = (target.x + target.width / 2, target.y + 10);

    drop_on(&mut runtime, a, point, true);
    drop_on(&mut runtime, a, point, true);
    assert!(runtime.backend.workspace_state.is_none());
    drop_on(&mut runtime, a, point, false);

    assert_eq!(requested_axis(&runtime), crate::Axis::Horizontal);
    runtime.layout_workspace(WorkspaceId::ONE, WIDE, &AtomicBool::new(false));
    let frames = &runtime.backend.frames;
    assert_eq!(
        frames[c],
        crate::Rect {
            x: 700,
            width: 700,
            ..WIDE
        }
    );
    assert_eq!(frames[a].x, 0);
    assert_eq!(frames[b].x, 0);
    assert!(frames[a].y < frames[b].y, "{frames:?}");
}

/// A fit that only reweights the row: a drag-nest keeps the requested
/// weights rather than saving the fit's.
#[test]
fn drag_nest_through_a_reweighting_fit_keeps_the_requested_weights() {
    let (mut runtime, [a, b, c]) = three_with_minimums([500, 200, 200]);
    let frames = &runtime.backend.frames;
    assert!(
        frames[&a].x < frames[&b].x && frames[&b].x < frames[&c].x,
        "still a row"
    );
    assert!(frames[&a].width >= 500, "the fit widened A");
    let target = frames[&c];

    drop_on(
        &mut runtime,
        &a,
        (target.x + target.width / 2, target.y + 10),
        false,
    );

    let engine = runtime.workspaces.engine(WorkspaceId::ONE);
    assert_eq!(engine.bound_windows(), [b.clone(), a.clone(), c.clone()]);
    assert!(
        engine
            .model()
            .workspace
            .slots
            .iter()
            .all(|slot| engine.layout().weight(slot.id) == Some(1)),
        "{:?}",
        engine.model().workspace.slots
    );
}

/// A perpendicular move with no tile that way splits the requested row:
/// `| A | B | C |` shown as `| A | B/C |`, A moved up, saves A above `B | C`.
#[test]
fn perpendicular_move_through_a_temporary_stack_keeps_the_requested_row() {
    let (mut runtime, [a, b, c]) = three_with_minimums([400, 250, 450]);
    assert_eq!(runtime.backend.frames[&b].x, runtime.backend.frames[&c].x);

    let Some(WorkerResult::Status { result, .. }) = runtime.execute(
        WorkerCommand::Action(WorkerAction::Reorder(Direction::Up), outputs()),
        &AtomicBool::new(false),
    ) else {
        panic!("missing status");
    };
    assert_eq!(result.unwrap().failed, 0);
    assert!(runtime.backend.workspace_state.is_some());

    runtime.layout_workspace(WorkspaceId::ONE, WIDE, &AtomicBool::new(false));
    let frames = &runtime.backend.frames;
    assert_eq!(
        frames[&a],
        crate::Rect {
            height: 400,
            ..WIDE
        }
    );
    assert_eq!(frames[&b].y, 400);
    assert_eq!(frames[&c].y, 400);
    assert!(frames[&b].x < frames[&c].x, "{frames:?}");
}

/// #373: moving the first tile left or the last tile right in a temporarily
/// stacked horizontal pair would recreate the same saved split, so it crosses
/// to the adjacent display's active workspace (empty or occupied) in both
/// Columns and default Dwindle layouts.
#[test]
fn outward_move_from_temporarily_stacked_pair_crosses_displays() {
    let cancelled = AtomicBool::new(false);
    let main = outputs().main().frame;
    for mode in [
        crate::config::LayoutMode::Columns,
        crate::config::LayoutMode::Dwindle,
    ] {
        for (direction, display_x, focus_index) in
            [(Direction::Left, -1000, 0), (Direction::Right, 1000, 1)]
        {
            let mut displays = outputs().displays().to_vec();
            displays[1].frame.x = display_x;
            let displays = macos::DisplayOutputs::new(displays).unwrap();
            for occupied in [false, true] {
                let pair = [window(10), window(11)];
                let focused = &pair[focus_index];
                let mut backend = FakeBackend {
                    focused: Some(focused.clone()),
                    ..FakeBackend::default()
                };
                for tile in &pair {
                    backend.frames.insert(tile.clone(), frame(0));
                    backend.minimum_widths.insert(tile.clone(), 600);
                }
                if occupied {
                    backend.frames.insert(window(20), frame(display_x + 50));
                }
                let config = match mode {
                    crate::config::LayoutMode::Dwindle => Config::default(),
                    crate::config::LayoutMode::Columns => {
                        Config::parse("version = 1\n[settings]\nlayout = \"columns\"\n").unwrap()
                    }
                };
                let mut rt = runtime_with_config(backend, config);
                for (index, tile) in pair.iter().enumerate() {
                    let anchor = (index > 0).then_some(SlotId(u64::from(pair[0].window_id)));
                    rt.workspaces.engine_mut(WorkspaceId::ONE).insert_slot_with(
                        Slot {
                            id: SlotId(u64::from(tile.window_id)),
                            binding: BindingState::Bound(tile.clone()),
                        },
                        mode,
                        main,
                        anchor,
                    );
                }
                if occupied {
                    bind(&mut rt, WorkspaceId::FIVE, &window(20));
                    rt.layout_workspace(
                        WorkspaceId::FIVE,
                        displays.displays()[1].frame,
                        &cancelled,
                    );
                }
                for _ in 0..2 {
                    rt.layout_workspace(WorkspaceId::ONE, main, &cancelled);
                }
                assert_eq!(
                    rt.workspaces.engine(WorkspaceId::ONE).layout().root_axis(),
                    crate::Axis::Horizontal
                );
                assert_eq!(
                    rt.backend.frames[&pair[0]].x, rt.backend.frames[&pair[1]].x,
                    "pair starts temporarily stacked"
                );
                assert!(rt.backend.frames[&pair[0]].y < rt.backend.frames[&pair[1]].y);

                let Some(WorkerResult::Status { result, .. }) = rt.execute(
                    WorkerCommand::Action(WorkerAction::Reorder(direction), displays.clone()),
                    &cancelled,
                ) else {
                    panic!("missing command status");
                };
                assert_eq!(result.unwrap().failed, 0);
                assert_eq!(
                    rt.workspaces.workspace_of(focused),
                    Some(WorkspaceId::FIVE),
                    "{mode:?} {direction:?} occupied={occupied} should cross displays on first press"
                );
                assert_eq!(rt.backend.focused, Some(focused.clone()));
                let remaining = &pair[1 - focus_index];
                assert_eq!(
                    rt.workspaces.workspace_of(remaining),
                    Some(WorkspaceId::ONE)
                );
                assert_eq!(rt.backend.frames[remaining], main);
            }
        }
    }
}

/// #373 control: moving A up in `A | B | C` temporarily fitted as `A | (B / C)`
/// changes the saved tree to `A / (B | C)`, so it stays on the source workspace
/// even when another display sits above it.
#[test]
fn perpendicular_move_through_stack_stays_local_when_saved_tree_changes_even_with_adjacent_display()
{
    let cancelled = AtomicBool::new(false);
    let (mut runtime, [a, b, c]) = three_with_minimums([400, 250, 450]);
    assert_eq!(runtime.backend.frames[&b].x, runtime.backend.frames[&c].x);
    let mut displays = outputs().displays().to_vec();
    displays[1].frame.x = 0;
    displays[1].frame.y = -800;
    let displays = macos::DisplayOutputs::new(displays).unwrap();

    let Some(WorkerResult::Status { result, .. }) = runtime.execute(
        WorkerCommand::Action(WorkerAction::Reorder(Direction::Up), displays),
        &cancelled,
    ) else {
        panic!("missing status");
    };
    assert_eq!(result.unwrap().failed, 0);
    assert_eq!(runtime.workspaces.workspace_of(&a), Some(WorkspaceId::ONE));
    assert!(runtime.backend.workspace_state.is_some());

    runtime.layout_workspace(WorkspaceId::ONE, WIDE, &cancelled);
    let frames = &runtime.backend.frames;
    assert_eq!(
        frames[&a],
        crate::Rect {
            height: 400,
            ..WIDE
        }
    );
    assert_eq!(frames[&b].y, 400);
    assert_eq!(frames[&c].y, 400);
    assert!(frames[&b].x < frames[&c].x, "{frames:?}");
}
