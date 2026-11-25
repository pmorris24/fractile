use super::*;

#[test]
fn focus_navigation_follows_linear_tile_order() {
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

    runtime
        .focus_relative(Direction::Left, outputs(), &AtomicBool::new(false))
        .unwrap();

    assert_eq!(runtime.backend.focused, Some(first.clone()));
    assert!(matches!(
        runtime.backend.calls.last(),
        Some(BackendCall::Focus(window)) if window == &first
    ));

    runtime
        .focus_relative(Direction::Right, outputs(), &AtomicBool::new(false))
        .unwrap();

    assert_eq!(runtime.backend.focused, Some(second));
}

#[test]
fn workspace_focus_cycle_wraps_through_tile_order_in_every_layout() {
    let first = window(10);
    let second = window(20);
    let third = window(30);
    let mut backend = FakeBackend {
        focused: Some(second.clone()),
        ..FakeBackend::default()
    };
    for (window, initial) in [(&first, 50), (&second, 500), (&third, 750)] {
        backend.frames.insert(window.clone(), frame(initial));
    }
    let mut runtime = runtime(backend);
    for window in [&first, &second, &third] {
        bind(&mut runtime, WorkspaceId::ONE, window);
    }
    let outputs = outputs();
    let cancelled = AtomicBool::new(false);

    runtime.cycle_focus(outputs.clone(), &cancelled).unwrap();
    assert_eq!(runtime.backend.focused, Some(third.clone()));
    assert_eq!(runtime.workspaces.preferred_focus(WorkspaceId::ONE), None);
    runtime
        .toggle_fullscreen_layout(outputs.clone(), &cancelled)
        .unwrap();

    runtime.cycle_focus(outputs.clone(), &cancelled).unwrap();
    assert_eq!(runtime.backend.focused, Some(first.clone()));
    runtime.cycle_focus(outputs, &cancelled).unwrap();
    assert_eq!(runtime.backend.focused, Some(second));
}

#[test]
fn layout_toggle_exits_fullscreen_and_applies_the_next_split_axis() {
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
    let outputs = outputs();
    let cancelled = AtomicBool::new(false);
    runtime
        .toggle_fullscreen_layout(outputs.clone(), &cancelled)
        .unwrap();

    runtime.toggle_layout(outputs, &cancelled).unwrap();

    assert!(!runtime.workspaces.deck(WorkspaceId::ONE).is_monocle());
    assert_eq!(
        runtime.workspaces.engine(WorkspaceId::ONE).root_axis(),
        crate::Axis::Vertical
    );
    assert_eq!(runtime.backend.frames[&first].height, 400);
    assert_eq!(runtime.backend.frames[&second].y, 400);
}

#[test]
fn focus_navigation_crosses_to_the_adjacent_display() {
    let main = window(10);
    let secondary = window(20);
    let mut backend = FakeBackend {
        focused: Some(main.clone()),
        ..FakeBackend::default()
    };
    backend.frames.insert(main.clone(), frame(50));
    backend.frames.insert(secondary.clone(), frame(1050));
    let mut runtime = runtime(backend);
    bind(&mut runtime, WorkspaceId::ONE, &main);
    bind(&mut runtime, WorkspaceId::FIVE, &secondary);

    runtime
        .focus_relative(Direction::Right, outputs(), &AtomicBool::new(false))
        .unwrap();

    assert_eq!(runtime.backend.focused, Some(secondary));

    runtime.backend.focused = Some(main.clone());
    runtime
        .focus_relative(Direction::Up, outputs(), &AtomicBool::new(false))
        .unwrap();

    assert_eq!(runtime.backend.focused, Some(main));
}

#[test]
fn layout_toggle_repartitions_and_persists_the_focused_workspace() {
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

    runtime
        .toggle_layout(outputs(), &AtomicBool::new(false))
        .unwrap();

    assert_eq!(
        runtime
            .workspaces
            .engine(WorkspaceId::ONE)
            .layout()
            .root_axis(),
        crate::Axis::Vertical
    );
    assert_eq!(runtime.backend.frames[&first].height, 400);
    assert_eq!(runtime.backend.frames[&second].y, 400);
    assert!(runtime.backend.workspace_state.is_some());
}

#[test]
fn resizing_a_tile_previews_its_sibling_and_saves_weights_on_release() {
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

    runtime
        .resize_window(
            first.clone(),
            crate::Rect {
                x: 0,
                y: 0,
                width: 600,
                height: 800,
            },
            true,
            outputs(),
            &AtomicBool::new(false),
        )
        .unwrap();

    let weights = |runtime: &WorkspaceRuntime<FakeBackend>| {
        let engine = runtime.workspaces.engine(WorkspaceId::ONE);
        let slots = &engine.model().workspace.slots;
        (
            engine.layout().weight(slots[0].id),
            engine.layout().weight(slots[1].id),
        )
    };
    // The preview moves the sibling; the split itself waits for mouse-up.
    assert_eq!(weights(&runtime), (Some(1), Some(1)));
    assert_eq!(runtime.backend.frames[&second].x, 600);
    assert_eq!(runtime.backend.frames[&first], frame(50));
    assert!(runtime.backend.workspace_state.is_none());

    runtime
        .resize_window(
            first,
            crate::Rect {
                x: 0,
                y: 0,
                width: 600,
                height: 800,
            },
            false,
            outputs(),
            &AtomicBool::new(false),
        )
        .unwrap();

    assert_eq!(weights(&runtime), (Some(600), Some(400)));
    assert!(runtime.backend.workspace_state.is_some());
}

#[test]
fn keyboard_resize_applies_and_persists_the_focused_layout() {
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

    runtime
        .resize_focused(ResizeDirection::Grow, outputs(), &AtomicBool::new(false))
        .unwrap();

    assert_eq!(runtime.backend.frames[&first].width, 550);
    assert_eq!(runtime.backend.frames[&second].x, 550);
    assert!(runtime.backend.workspace_state.is_some());
}

#[test]
fn balance_applies_and_persists_the_focused_parent_split() {
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
    runtime
        .resize_focused(ResizeDirection::Grow, outputs(), &AtomicBool::new(false))
        .unwrap();
    runtime.backend.workspace_state = None;

    runtime
        .balance_focused(outputs(), &AtomicBool::new(false))
        .unwrap();

    assert_eq!(runtime.backend.frames[&first].width, 500);
    assert_eq!(runtime.backend.frames[&second].x, 500);
    assert!(runtime.backend.workspace_state.is_some());
}

#[test]
fn resize_to_applies_and_persists_the_focused_window_width() {
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

    runtime
        .resize_focused_to(
            crate::Axis::Horizontal,
            700,
            outputs(),
            &AtomicBool::new(false),
        )
        .unwrap();

    assert_eq!(runtime.backend.frames[&second].width, 700);
    assert_eq!(runtime.backend.frames[&second].x, 300);
    assert_eq!(runtime.backend.frames[&first].width, 300);
    assert!(runtime.backend.workspace_state.is_some());

    // The lone-axis case is a no-op, like `resize grow` with no sibling.
    runtime.backend.workspace_state = None;
    runtime
        .resize_focused_to(
            crate::Axis::Vertical,
            300,
            outputs(),
            &AtomicBool::new(false),
        )
        .unwrap();
    assert_eq!(runtime.backend.frames[&second].width, 700);
    assert!(runtime.backend.workspace_state.is_none());
}

#[test]
fn resize_to_requires_leaving_fullscreen_layout() {
    let (first, second) = (window(10), window(20));
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
    runtime
        .toggle_fullscreen_layout(outputs(), &cancelled)
        .unwrap();

    let Err(resize) =
        runtime.resize_focused_to(crate::Axis::Horizontal, 600, outputs(), &cancelled)
    else {
        panic!("resize width must not apply in Fullscreen Layout");
    };
    assert!(resize.contains("Exit Fullscreen Layout"), "{resize}");
}

#[test]
fn reorder_moves_to_the_adjacent_display_at_a_tile_boundary() {
    let moving = window(10);
    let secondary = window(20);
    let mut backend = FakeBackend {
        focused: Some(moving.clone()),
        ..FakeBackend::default()
    };
    backend.frames.insert(moving.clone(), frame(50));
    backend.frames.insert(secondary.clone(), frame(1050));
    let mut runtime = runtime(backend);
    bind(&mut runtime, WorkspaceId::ONE, &moving);
    bind(&mut runtime, WorkspaceId::FIVE, &secondary);

    runtime
        .reorder_focused(Direction::Right, outputs(), &AtomicBool::new(false))
        .unwrap();

    assert_eq!(
        runtime.workspaces.workspace_of(&moving),
        Some(WorkspaceId::FIVE)
    );
    assert_eq!(runtime.backend.focused, Some(moving));
}

#[test]
fn perpendicular_reorder_reorients_a_two_window_workspace_before_crossing_displays() {
    let top = window(10);
    let bottom = window(20);
    let other_display = window(30);
    let mut backend = FakeBackend {
        focused: Some(bottom.clone()),
        ..FakeBackend::default()
    };
    backend.frames.insert(top.clone(), frame(50));
    backend.frames.insert(bottom.clone(), frame(500));
    backend.frames.insert(other_display.clone(), frame(1050));
    let mut runtime = runtime(backend);
    bind(&mut runtime, WorkspaceId::ONE, &top);
    bind(&mut runtime, WorkspaceId::ONE, &bottom);
    bind(&mut runtime, WorkspaceId::SIX, &other_display);
    runtime
        .workspaces
        .engine_mut(WorkspaceId::ONE)
        .toggle_axis();

    runtime
        .reorder_focused(Direction::Right, outputs(), &AtomicBool::new(false))
        .unwrap();

    assert_eq!(
        runtime
            .workspaces
            .engine(WorkspaceId::ONE)
            .layout()
            .root_axis(),
        crate::Axis::Horizontal
    );
    assert_eq!(
        runtime.workspaces.workspace_of(&bottom),
        Some(WorkspaceId::ONE)
    );
    assert_eq!(
        runtime.workspaces.engine(WorkspaceId::ONE).bound_windows(),
        vec![top, bottom]
    );
}

#[test]
fn perpendicular_reorder_creates_a_mixed_split_for_three_windows() {
    let first = window(10);
    let moving = window(20);
    let third = window(30);
    let mut backend = FakeBackend {
        focused: Some(moving.clone()),
        ..FakeBackend::default()
    };
    backend.frames.insert(first.clone(), frame(50));
    backend.frames.insert(moving.clone(), frame(400));
    backend.frames.insert(third.clone(), frame(750));
    let mut runtime = runtime(backend);
    bind(&mut runtime, WorkspaceId::ONE, &first);
    bind(&mut runtime, WorkspaceId::ONE, &moving);
    bind(&mut runtime, WorkspaceId::ONE, &third);

    runtime
        .reorder_focused(Direction::Up, outputs(), &AtomicBool::new(false))
        .unwrap();

    let placements = runtime
        .workspaces
        .engine(WorkspaceId::ONE)
        .placements(&Settings::for_output(outputs().main().frame));
    assert_eq!(placements[0].window.as_ref(), Some(&moving));
    assert_eq!(
        placements[0].frame,
        crate::Rect {
            x: 0,
            y: 0,
            width: 1000,
            height: 400
        }
    );
    assert_eq!(
        placements[1].frame,
        crate::Rect {
            x: 0,
            y: 400,
            width: 500,
            height: 400
        }
    );
    assert_eq!(
        placements[2].frame,
        crate::Rect {
            x: 500,
            y: 400,
            width: 500,
            height: 400
        }
    );
    assert!(runtime.backend.workspace_state.is_some());
}

/// #119: an empty workspace on the adjacent display still takes the tile.
#[test]
fn reorder_moves_to_an_empty_workspace_on_the_adjacent_display() {
    let moving = window(10);
    let mut backend = FakeBackend {
        focused: Some(moving.clone()),
        ..FakeBackend::default()
    };
    backend.frames.insert(moving.clone(), frame(50));
    let mut runtime = runtime(backend);
    bind(&mut runtime, WorkspaceId::ONE, &moving);
    assert!(runtime.workspaces.windows(WorkspaceId::FIVE).is_empty());

    runtime
        .reorder_focused(Direction::Right, outputs(), &AtomicBool::new(false))
        .unwrap();

    assert_eq!(
        runtime.workspaces.workspace_of(&moving),
        Some(WorkspaceId::FIVE)
    );
    assert!(runtime.is_visible(WorkspaceId::FIVE, &outputs()));
    assert_eq!(runtime.backend.focused, Some(moving.clone()));

    // Nothing lies further right: the key does nothing.
    runtime
        .reorder_focused(Direction::Right, outputs(), &AtomicBool::new(false))
        .unwrap();
    assert_eq!(
        runtime.workspaces.workspace_of(&moving),
        Some(WorkspaceId::FIVE)
    );
}
