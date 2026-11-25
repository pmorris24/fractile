//! The Workspace Steps Lab: `switch next|prev` takes the swipe's stops on the focused display,
//! `move next|prev` sends the focused window along every enabled workspace of its display.

use super::*;
use crate::WorkspaceStep;

fn lab() -> Config {
    Config::parse("version = 1\n[labs]\nworkspace_steps = true\n").unwrap()
}

/// A tile on Workspace 1 and a float on 3, both on the main display, which shows 1.
fn stepping_runtime(config: Config, focused: bool) -> WorkspaceRuntime<FakeBackend> {
    let tiled = window(10);
    let mut backend = FakeBackend {
        focused: focused.then(|| tiled.clone()),
        observation: Some(observation(&tiled, frame(50))),
        ..FakeBackend::default()
    };
    backend.frames.insert(tiled.clone(), frame(50));
    backend.frames.insert(window(11), frame(60));
    let mut runtime = runtime_with_config(backend, config);
    bind(&mut runtime, WorkspaceId::ONE, &tiled);
    runtime
        .workspaces
        .own_floating(window(11), WorkspaceId::THREE);
    runtime
}

#[test]
fn switch_steps_through_the_occupied_workspaces_of_the_focused_display_and_stops_at_the_ends() {
    let mut runtime = stepping_runtime(lab(), false);
    let mut press = |step| {
        runtime
            .step_workspace(step, outputs(), &AtomicBool::new(false))
            .unwrap()
            .active
    };
    // Empty Workspace 2 is skipped, and the other display keeps showing 5.
    assert_eq!(
        press(WorkspaceStep::Next),
        [WorkspaceId::THREE, WorkspaceId::FIVE]
    );
    assert_eq!(
        press(WorkspaceStep::Next),
        [WorkspaceId::THREE, WorkspaceId::FIVE]
    );
    assert_eq!(
        press(WorkspaceStep::Prev),
        [WorkspaceId::ONE, WorkspaceId::FIVE]
    );
    assert_eq!(
        press(WorkspaceStep::Prev),
        [WorkspaceId::ONE, WorkspaceId::FIVE]
    );
}

#[test]
fn move_sends_the_focused_window_to_the_next_workspace_even_when_it_is_empty() {
    let mut runtime = stepping_runtime(lab(), true);
    // At the start of the display's workspaces nothing moves.
    runtime
        .move_focused_step(
            WorkspaceStep::Prev,
            false,
            outputs(),
            &AtomicBool::new(false),
        )
        .unwrap();
    assert_eq!(
        runtime.workspaces.workspace_of(&window(10)),
        Some(WorkspaceId::ONE)
    );
    runtime
        .move_focused_step(
            WorkspaceStep::Next,
            false,
            outputs(),
            &AtomicBool::new(false),
        )
        .unwrap();
    assert_eq!(
        runtime.workspaces.workspace_of(&window(10)),
        Some(WorkspaceId::TWO)
    );
    assert_eq!(
        runtime.workspaces.active_for(&DisplaySelector::Main),
        Some(WorkspaceId::ONE)
    );
}

#[test]
fn workspace_steps_do_nothing_while_the_lab_is_off() {
    let mut runtime = stepping_runtime(Config::default(), true);
    assert!(
        runtime
            .step_workspace(WorkspaceStep::Next, outputs(), &AtomicBool::new(false))
            .err()
            .is_some_and(|error| error.contains("workspace_steps"))
    );
    assert!(
        runtime
            .move_focused_step(
                WorkspaceStep::Next,
                true,
                outputs(),
                &AtomicBool::new(false),
            )
            .err()
            .is_some_and(|error| error.contains("workspace_steps"))
    );
    assert_eq!(
        runtime.workspaces.workspace_of(&window(10)),
        Some(WorkspaceId::ONE)
    );
}

/// A tile on Workspace 5 and a float on 8, both on the secondary display, which shows 5.
fn side_display_runtime(focused: bool) -> WorkspaceRuntime<FakeBackend> {
    let tiled = window(20);
    let eight = WorkspaceId::new(8).unwrap();
    let mut backend = FakeBackend {
        focused: focused.then(|| tiled.clone()),
        observation: Some(observation(&tiled, frame(1050))),
        ..FakeBackend::default()
    };
    backend.frames.insert(tiled.clone(), frame(1050));
    backend.frames.insert(window(21), frame(1100));
    let mut runtime = runtime_with_config(backend, lab());
    bind(&mut runtime, WorkspaceId::FIVE, &tiled);
    runtime.workspaces.own_floating(window(21), eight);
    runtime
}

#[test]
fn switch_steps_on_the_side_display_that_has_focus() {
    let eight = WorkspaceId::new(8).unwrap();
    let mut runtime = side_display_runtime(true);
    let mut press = |step| {
        runtime
            .step_workspace(step, outputs(), &AtomicBool::new(false))
            .unwrap()
            .active
    };
    // The main display keeps showing 1 throughout.
    assert_eq!(press(WorkspaceStep::Next), [WorkspaceId::ONE, eight]);
    assert_eq!(press(WorkspaceStep::Next), [WorkspaceId::ONE, eight]);
    assert_eq!(
        press(WorkspaceStep::Prev),
        [WorkspaceId::ONE, WorkspaceId::FIVE]
    );
}

#[test]
fn with_no_window_focused_switch_steps_the_display_switched_to_last_and_move_refuses() {
    let eight = WorkspaceId::new(8).unwrap();
    let mut runtime = side_display_runtime(false);
    runtime.record_focused_workspace(WorkspaceId::FIVE);

    let refused = runtime.move_focused_step(
        WorkspaceStep::Next,
        false,
        outputs(),
        &AtomicBool::new(false),
    );
    assert_eq!(
        refused.err().as_deref(),
        Some("Focused window is unavailable")
    );
    assert_eq!(
        runtime.workspaces.workspace_of(&window(20)),
        Some(WorkspaceId::FIVE)
    );

    let active = runtime
        .step_workspace(WorkspaceStep::Next, outputs(), &AtomicBool::new(false))
        .unwrap()
        .active;
    assert_eq!(active, [WorkspaceId::ONE, eight]);
}

#[test]
fn move_next_with_follow_twice_ends_on_three_with_the_window() {
    let moved = window(10);
    let mut backend = FakeBackend {
        focused: Some(moved.clone()),
        observation: Some(observation(&moved, frame(50))),
        ..FakeBackend::default()
    };
    backend.frames.insert(moved.clone(), frame(50));
    let mut runtime = runtime_with_config(backend, lab());
    bind(&mut runtime, WorkspaceId::ONE, &moved);

    for _ in 0..2 {
        runtime
            .move_focused_step(
                WorkspaceStep::Next,
                true,
                outputs(),
                &AtomicBool::new(false),
            )
            .unwrap();
    }

    assert_eq!(
        runtime.workspaces.workspace_of(&moved),
        Some(WorkspaceId::THREE)
    );
    assert_eq!(
        runtime.workspaces.active_for(&DisplaySelector::Main),
        Some(WorkspaceId::THREE)
    );
    assert_eq!(runtime.backend.focused.as_ref(), Some(&moved));
}

/// Named workspaces Q and W on the main display, ordered after 1 as W, then Q.
const ORDERED: &str = r#"
version = 1
[settings]
workspace_order = [1, "W", "Q"]
[labs]
workspace_steps = true
[[workspaces]]
workspace = "Q"
monitor = "main"
[[workspaces]]
workspace = "W"
monitor = "main"
"#;

#[test]
fn switch_follows_the_workspace_order_and_named_workspaces() {
    let named = |letter| WorkspaceId::named(letter).unwrap();
    let config = Config::parse(ORDERED).unwrap();
    let mut runtime = runtime_with_config(FakeBackend::default(), config);
    for (id, workspace) in [(10, WorkspaceId::ONE), (11, named('Q')), (12, named('W'))] {
        let window = window(id);
        runtime.backend.frames.insert(window.clone(), frame(50));
        bind(&mut runtime, workspace, &window);
    }
    let mut press = |step| {
        runtime
            .step_workspace(step, outputs(), &AtomicBool::new(false))
            .unwrap()
            .active[0]
    };

    assert_eq!(press(WorkspaceStep::Next), named('W'));
    assert_eq!(press(WorkspaceStep::Next), named('Q'));
    assert_eq!(press(WorkspaceStep::Next), named('Q'));
    assert_eq!(press(WorkspaceStep::Prev), named('W'));
}
