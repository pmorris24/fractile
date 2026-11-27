//! `settings.workspace_order`: `listed_workspaces` (the HUD row, the menu bar list and
//! three-finger swipes) gives the configured workspaces first, then the rest in default order.

use super::*;

fn named(letter: char) -> WorkspaceId {
    WorkspaceId::named(letter).unwrap()
}

fn config(order: &str) -> Config {
    Config::parse(&format!(
        r#"
version = 1
[settings]
{order}
[[workspaces]]
workspace = "Q"
monitor = "main"
[[workspaces]]
workspace = "W"
monitor = "main"
[[workspaces]]
workspace = "E"
monitor = "main"
"#
    ))
    .unwrap()
}

/// Workspaces 1, Q, W and E on the main display, each holding one window; 1 is visible.
fn keyboard_row(order: &str) -> WorkspaceRuntime<FakeBackend> {
    let mut runtime = runtime_with_config(FakeBackend::default(), config(order));
    for (id, workspace) in [
        (10, WorkspaceId::ONE),
        (11, named('Q')),
        (12, named('W')),
        (13, named('E')),
    ] {
        let window = window(id);
        runtime.backend.frames.insert(window.clone(), frame(50));
        bind(&mut runtime, workspace, &window);
    }
    runtime
}

fn listed(runtime: &WorkspaceRuntime<FakeBackend>) -> Vec<WorkspaceId> {
    runtime.listed_workspaces("main-id", Some(WorkspaceId::ONE), &outputs())
}

#[test]
fn listed_workspaces_come_first_in_the_configured_order_and_the_rest_follow() {
    // P isn't configured and 4 is empty: listing them enables and shows neither.
    let runtime = keyboard_row(r#"workspace_order = ["P", "Q", 4, "W", "E"]"#);

    assert!(!runtime.workspaces.is_enabled(named('P')));
    assert_eq!(
        listed(&runtime),
        [named('Q'), named('W'), named('E'), WorkspaceId::ONE]
    );
}

#[test]
fn the_hud_row_follows_the_configured_order() {
    let mut runtime = keyboard_row(r#"workspace_order = [1, "Q", "W", "E"]"#);

    let hud = runtime
        .switch(named('W'), outputs(), &AtomicBool::new(false))
        .unwrap()
        .hud
        .unwrap();
    let main = hud
        .displays
        .iter()
        .find(|display| display.id == "main-id")
        .unwrap();

    assert_eq!(main.visible, Some(named('W')));
    assert!(
        main.order
            .starts_with(&[WorkspaceId::ONE, named('Q'), named('W'), named('E')])
    );
    assert_eq!(
        main.workspaces
            .iter()
            .map(|entry| entry.workspace)
            .collect::<Vec<_>>(),
        // The configured order first, then the display's other workspaces, empty ones too.
        [
            WorkspaceId::ONE,
            named('Q'),
            named('W'),
            named('E'),
            WorkspaceId::TWO,
            WorkspaceId::THREE,
            WorkspaceId::FOUR,
        ]
    );
}

#[test]
fn swipes_step_through_workspaces_in_the_configured_order() {
    let mut runtime = keyboard_row(r#"workspace_order = [1, "Q", "W", "E"]"#);
    let mut swipe = |swipe| {
        runtime
            .swipe(
                swipe,
                Fingers::Three,
                (500, 400),
                outputs(),
                &AtomicBool::new(false),
            )
            .unwrap()
            .active[0]
    };

    assert_eq!(swipe(Swipe::Left), named('Q'));
    assert_eq!(swipe(Swipe::Left), named('W'));
    assert_eq!(swipe(Swipe::Right), named('Q'));
}

#[test]
fn a_config_reload_applies_and_clears_the_order() {
    let mut runtime = keyboard_row("");

    runtime.replace_config(config(r#"workspace_order = ["W", "Q", "E"]"#));
    assert_eq!(
        listed(&runtime),
        [named('W'), named('Q'), named('E'), WorkspaceId::ONE]
    );

    runtime.replace_config(config(""));
    assert_eq!(
        listed(&runtime),
        [WorkspaceId::ONE, named('E'), named('Q'), named('W')]
    );
}

#[test]
fn trackpad_swipe_off_stops_swipes_only_with_its_lab_on_and_reloads_live() {
    let off = "trackpad_swipe = false";
    let lab = "[labs]\nswipe_switch = true";
    let swipe = |runtime: &mut WorkspaceRuntime<FakeBackend>, swipe| {
        runtime
            .swipe(
                swipe,
                Fingers::Three,
                (500, 400),
                outputs(),
                &AtomicBool::new(false),
            )
            .unwrap()
            .active[0]
    };

    // With the Lab off the setting is ignored: swipes switch exactly as before.
    let mut runtime = keyboard_row(off);
    assert_eq!(swipe(&mut runtime, Swipe::Left), named('E'));
    assert_eq!(swipe(&mut runtime, Swipe::Right), WorkspaceId::ONE);

    // With it on, the swipe leaves the workspace alone in either direction.
    runtime.replace_config(config(&format!("{off}\n{lab}")));
    assert_eq!(swipe(&mut runtime, Swipe::Left), WorkspaceId::ONE);
    assert_eq!(swipe(&mut runtime, Swipe::Right), WorkspaceId::ONE);

    // Back on (or unset) after a reload, swipes step through workspaces again.
    runtime.replace_config(config(lab));
    assert_eq!(swipe(&mut runtime, Swipe::Left), named('E'));
}

#[test]
fn four_finger_swipe_lab_moves_the_swipe_to_four_fingers_and_reloads_live() {
    let lab = "[labs]\nfour_finger_swipe = true";
    let swipe = |runtime: &mut WorkspaceRuntime<FakeBackend>, fingers| {
        runtime
            .swipe(
                Swipe::Left,
                fingers,
                (500, 400),
                outputs(),
                &AtomicBool::new(false),
            )
            .unwrap()
            .active[0]
    };

    // With the Lab off, four-finger swipes stay with macOS.
    let mut runtime = keyboard_row("");
    assert_eq!(swipe(&mut runtime, Fingers::Four), WorkspaceId::ONE);

    // With it on, four fingers step and three are left to macOS.
    runtime.replace_config(config(lab));
    assert_eq!(swipe(&mut runtime, Fingers::Three), WorkspaceId::ONE);
    assert_eq!(swipe(&mut runtime, Fingers::Four), named('E'));

    // Off again after a reload, three fingers step once more.
    runtime.replace_config(config(""));
    assert_eq!(swipe(&mut runtime, Fingers::Three), named('Q'));
}
