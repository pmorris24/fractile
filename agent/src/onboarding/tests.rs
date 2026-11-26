use std::sync::atomic::{AtomicUsize, Ordering};

use fractile::WorkspaceId;

use super::arrange::{Board, Target, Tile};
use super::conflicts::{Conflict, switch_on};
use super::flow::{
    Effect, Flow, Input, SetupState, Step, completion_recorded_at, launch_step_at,
    record_completion_at, record_step_at, saved_step_at,
};
use crate::{apps::window_manager_name, controls::PermissionControl, rules::Place};

const SAFARI: &str = "com.apple.Safari";
const TERMINAL: &str = "com.apple.Terminal";
const STICKIES: &str = "com.apple.Stickies";
const CALCULATOR: &str = "com.apple.calculator";

fn temp_dir(name: &str) -> std::path::PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "fractile-onboarding-test-{name}-{}-{id}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn state(accessibility: bool, tiling: bool) -> SetupState {
    SetupState {
        accessibility,
        launch_at_login: false,
        tiling,
    }
}

/// A first-run flow walked to the step, with Accessibility granted on the way.
fn flow_at(step: Step) -> Flow {
    let mut flow = Flow::new(Step::Welcome, state(true, false), false);
    while flow.step() != step {
        flow.on(Input::Continue);
    }
    flow
}

#[test]
fn fresh_untrusted_launch_opens_welcome_and_blocks_continue_until_granted() {
    let dir = temp_dir("fresh");
    assert_eq!(launch_step_at(&dir, false), Some(Step::Welcome));

    let mut flow = Flow::new(Step::Welcome, state(false, false), false);
    assert_eq!(flow.view().step, Step::Welcome);
    assert_eq!(flow.view().count, 4);
    assert!(!flow.view().can_go_back);

    assert_eq!(flow.on(Input::Continue), Effect::None);
    assert_eq!(flow.view().step, Step::Setup);
    assert_eq!(flow.view().direction, 1);
    assert_eq!(flow.view().index, 1);
    assert_eq!(flow.view().accessibility, PermissionControl::Allow);
    assert!(!flow.view().ready);

    // Continue stays blocked while Accessibility is missing.
    assert_eq!(flow.on(Input::Continue), Effect::None);
    assert_eq!(flow.view().step, Step::Setup);

    // Asking opens the Accessibility guide and enters Waiting.
    assert_eq!(
        flow.on(Input::GrantAccessibility),
        Effect::OpenAccessibility
    );
    assert_eq!(flow.view().accessibility, PermissionControl::Waiting);

    // Granting triggers AdvanceSoon so the green check settles before advancing.
    assert_eq!(
        flow.on(Input::Status(state(true, false))),
        Effect::AdvanceSoon
    );
    assert_eq!(
        flow.view().accessibility,
        PermissionControl::Allowed { animated: true }
    );
    assert!(flow.view().ready);

    // Then the plan for the open windows, whose primary button is what starts tiling.
    assert_eq!(flow.on(Input::Continue), Effect::None);
    assert_eq!(flow.view().step, Step::Arrange);
    assert_eq!(flow.view().index, 2);
    assert_eq!(flow.on(Input::Continue), Effect::Arrange);
    assert_eq!(flow.view().step, Step::TryIt);
    assert_eq!(flow.view().index, 3);

    assert_eq!(flow.on(Input::Continue), Effect::Finish);
    drop(std::fs::remove_dir_all(dir));
}

#[test]
fn existing_trusted_install_records_completion_without_opening_window() {
    let dir = temp_dir("trusted-upgrade");
    assert_eq!(launch_step_at(&dir, true), None);
    assert!(completion_recorded_at(&dir));
    // Subsequent launches even without Accessibility do not restart Welcome.
    assert_eq!(launch_step_at(&dir, false), None);
    drop(std::fs::remove_dir_all(dir));
}

#[test]
fn saved_setup_step_resumes_at_the_arrangement_after_accessibility_grant_relaunch() {
    let dir = temp_dir("resume");
    record_step_at(&dir, Step::Setup).unwrap();
    assert_eq!(saved_step_at(&dir), Some(Step::Setup));
    assert_eq!(launch_step_at(&dir, true), Some(Step::Setup));

    let flow = Flow::new(Step::Setup, state(true, false), false);
    assert_eq!(flow.step(), Step::Arrange);

    record_completion_at(&dir).unwrap();
    assert!(completion_recorded_at(&dir));
    assert_eq!(saved_step_at(&dir), None);
    drop(std::fs::remove_dir_all(dir));
}

#[test]
fn an_in_progress_tour_saved_on_a_removed_step_resumes_at_the_arrangement() {
    // "rules" went in 0.3.2 and "done" in 0.3.5. Both were past the Accessibility grant with
    // tiling still held, which is where Arrange is.
    for saved in ["rules", "done"] {
        let dir = temp_dir("step-migration");
        std::fs::write(dir.join("onboarding-step"), saved).unwrap();
        assert_eq!(saved_step_at(&dir), Some(Step::Arrange), "{saved}");
        assert_eq!(launch_step_at(&dir, true), Some(Step::Arrange), "{saved}");
        drop(std::fs::remove_dir_all(dir));
    }
    // Every step the tour writes reads back as a step.
    for step in Step::TOUR {
        let dir = temp_dir("step-roundtrip");
        record_step_at(&dir, step).unwrap();
        assert_eq!(
            saved_step_at(&dir).is_some(),
            step != Step::Welcome,
            "{step:?}"
        );
        drop(std::fs::remove_dir_all(dir));
    }
}

#[test]
fn first_run_tour_can_be_dismissed_and_only_a_manual_one_is_complete() {
    let mut first_run = Flow::new(Step::Welcome, state(false, false), false);
    assert!(!first_run.view().manual);
    assert_eq!(first_run.on(Input::Dismiss), Effect::Finish);

    let manual = Flow::new(Step::Welcome, state(true, true), true);
    assert!(manual.view().manual);
}

#[test]
fn manual_tour_walks_all_steps_and_allows_dismiss() {
    let mut flow = Flow::new(Step::Welcome, state(true, true), true);
    assert_eq!(flow.on(Input::Continue), Effect::None);
    assert_eq!(flow.step(), Step::Setup);
    assert!(flow.view().ready);
    assert_eq!(flow.on(Input::Continue), Effect::None);
    assert_eq!(flow.step(), Step::Arrange);
    assert_eq!(flow.on(Input::Back), Effect::None);
    assert_eq!(flow.step(), Step::Setup);
    assert_eq!(flow.view().direction, -1);
    assert_eq!(flow.on(Input::Dismiss), Effect::Finish);
}

#[test]
fn the_permission_step_has_no_launch_at_login_and_try_it_holds_it() {
    // The flow: only Try it answers the toggle.
    for step in [Step::Welcome, Step::Setup, Step::Arrange] {
        assert_eq!(
            flow_at(step).on(Input::ToggleLaunchAtLogin),
            Effect::None,
            "{step:?}"
        );
    }
    assert_eq!(
        flow_at(Step::TryIt).on(Input::ToggleLaunchAtLogin),
        Effect::ToggleLaunchAtLogin
    );

    // And the tour around it: nothing changes on the permission step, Try it flips it.
    let login = |ui: &crate::Ui| ui.tour.as_ref().unwrap().flow.view().launch_at_login;
    let mut ui = crate::playground::blank_fixture();
    super::tour_scene(&mut ui, Step::Setup, true, false);
    assert!(!login(&ui));
    super::input(&mut ui, Input::ToggleLaunchAtLogin);
    assert!(!login(&ui));

    super::tour_scene(&mut ui, Step::TryIt, true, false);
    super::input(&mut ui, Input::ToggleLaunchAtLogin);
    assert!(login(&ui));
    super::input(&mut ui, Input::ToggleLaunchAtLogin);
    assert!(!login(&ui));
}

#[test]
fn try_it_never_advances_by_itself() {
    let mut flow = flow_at(Step::TryIt);
    // The grant that moves the permission step on does nothing here, however it arrives.
    for next in [state(false, true), state(true, true), state(true, false)] {
        assert_eq!(flow.on(Input::Status(next)), Effect::None);
        assert_eq!(flow.step(), Step::TryIt);
    }
    // Try it goes back only through Undo & restore.
    assert!(!flow.view().can_go_back);
    assert_eq!(flow.on(Input::Back), Effect::None);
    assert_eq!(flow.step(), Step::TryIt);

    // Making every move leaves the tour on Try it: Done is the only way on.
    let mut ui = crate::playground::blank_fixture();
    super::tour_scene(&mut ui, Step::TryIt, true, false);
    for mv in super::ALL_LESSONS {
        let pressed = super::press(&mut ui, mv, 0.0);
        assert!(pressed.is_some_and(|pressed| pressed.learned), "{mv:?}");
    }
    let tour = ui.tour.as_ref().unwrap();
    assert!(tour.play.demo.complete());
    assert_eq!(tour.flow.step(), Step::TryIt);
    // Nor do the beats that let the keys up, the tick go and the HUD leave.
    assert!(ui.tour.as_mut().unwrap().play.settle(10.0));
    assert_eq!(ui.tour.as_ref().unwrap().flow.step(), Step::TryIt);
    super::input(&mut ui, Input::Continue);
    assert!(ui.tour.is_none());
}

#[test]
fn skip_leaves_the_windows_alone_and_undo_goes_back_to_the_plan() {
    // Before tiling has started, Skip ends the tour: nothing moved, nothing to try.
    let mut first_run = flow_at(Step::Arrange);
    assert_eq!(first_run.on(Input::Skip), Effect::Finish);
    // Re-run from Settings with tiling on, there are still shortcuts to try.
    let mut manual = Flow::new(Step::Welcome, state(true, true), true);
    manual.on(Input::Continue);
    manual.on(Input::Continue);
    assert_eq!(manual.on(Input::Skip), Effect::None);
    assert_eq!(manual.step(), Step::TryIt);
    // Skip and Undo mean nothing on the other steps.
    assert_eq!(flow_at(Step::Setup).on(Input::Skip), Effect::None);
    assert_eq!(flow_at(Step::Arrange).on(Input::Undo), Effect::None);

    let mut flow = flow_at(Step::TryIt);
    assert_eq!(flow.on(Input::Undo), Effect::Undo);
    assert_eq!(flow.step(), Step::Arrange);
    assert_eq!(flow.view().direction, -1);
}

#[test]
fn recognizes_conflicting_window_managers_by_bundle_or_daemon_name() {
    assert_eq!(
        window_manager_name(Some("bobko.aerospace"), None),
        Some("AeroSpace")
    );
    assert_eq!(
        window_manager_name(Some("com.knollsoft.Rectangle"), None),
        Some("Rectangle")
    );
    assert_eq!(window_manager_name(None, Some("yabai")), Some("yabai"));
    assert_eq!(
        window_manager_name(Some("com.apple.Safari"), Some("Safari")),
        None
    );
}

#[test]
fn a_conflict_shows_only_when_its_switch_is_on() {
    // Stage Manager is off until someone turns it on; macOS 15's edge tiling is on until
    // someone turns it off.
    assert!(!switch_on(None, false));
    assert!(switch_on(Some(true), false));
    assert!(switch_on(None, true));
    assert!(!switch_on(Some(false), true));

    assert_eq!(Conflict::StageManager.label(), "Stage Manager is on");
    assert_eq!(Conflict::EdgeTiling.label(), "macOS edge tiling is on");
    assert_eq!(Conflict::EdgeTiling.action(), "Turn off");
    let other = Conflict::WindowManager("AeroSpace".into());
    assert_eq!(other.label(), "AeroSpace is running");
    assert_eq!(other.action(), "Quit");

    // A row goes when its button worked; a scene has nothing to switch, so it always does.
    let mut ui = crate::playground::blank_fixture();
    super::tour_scene(&mut ui, Step::Setup, false, false);
    assert!(ui.tour.as_ref().unwrap().conflicts.is_empty());
    ui.tour.as_mut().unwrap().conflicts = vec![Conflict::StageManager, Conflict::EdgeTiling];
    super::resolve(&mut ui, 0);
    assert_eq!(
        ui.tour.as_ref().unwrap().conflicts,
        vec![Conflict::EdgeTiling]
    );
    super::resolve(&mut ui, 5);
    assert_eq!(ui.tour.as_ref().unwrap().conflicts.len(), 1);
}

fn tiles_on(board: &Board, workspace: WorkspaceId) -> Vec<&str> {
    board
        .plan
        .workspaces
        .iter()
        .find(|planned| planned.workspace == workspace)
        .map(|planned| planned.tiles.iter().map(String::as_str).collect())
        .unwrap_or_default()
}

/// Drags tile `index` of `from` to a board point, as the pointer would.
fn drag_to(board: &mut Board, from: WorkspaceId, index: usize, to: (f32, f32)) {
    let geometry = board.geometry();
    let (card, planned) = geometry
        .cards
        .iter()
        .zip(&board.plan.workspaces)
        .find(|(card, _)| card.workspace == from)
        .unwrap();
    let frame = geometry.tiles(card, planned.tiles.len())[index];
    let tile = Tile {
        workspace: from,
        index,
        bundle_id: planned.tiles[index].clone(),
        frame,
    };
    let start = (frame[2] / 2.0, frame[3] / 2.0);
    let delta = (to.0 - frame[0] - start.0, to.1 - frame[1] - start.1);
    board.drag_step(
        rune::Phase::Down,
        start,
        (0.0, 0.0),
        false,
        &tile,
        &geometry,
    );
    board.drag_step(rune::Phase::Move, start, delta, false, &tile, &geometry);
    board.drag_step(rune::Phase::Up, start, delta, false, &tile, &geometry);
}

fn card_centre(board: &Board, workspace: WorkspaceId) -> (f32, f32) {
    let geometry = board.geometry();
    let card = geometry
        .cards
        .iter()
        .find(|card| card.workspace == workspace)
        .unwrap();
    (card.x + card.w / 2.0, card.y + 40.0)
}

#[test]
fn the_board_moves_a_tile_by_click_or_drag_and_floats_by_drop() {
    let mut board = Board::fixture_laptop();
    // Four browser windows are two and two, never four on one workspace.
    assert_eq!(tiles_on(&board, WorkspaceId::ONE), [SAFARI, SAFARI]);
    assert_eq!(tiles_on(&board, WorkspaceId::TWO), [TERMINAL]);
    assert_eq!(board.plan.floating, [CALCULATOR, STICKIES]);
    assert!(!board.nothing_to_arrange());

    // A click sends a tile to the next workspace.
    assert!(board.click(WorkspaceId::ONE, 1));
    assert_eq!(tiles_on(&board, WorkspaceId::ONE), [SAFARI]);
    assert_eq!(tiles_on(&board, WorkspaceId::TWO), [TERMINAL, SAFARI]);

    // A drag drops it on the card under the pointer.
    let three = card_centre(&board, WorkspaceId::THREE);
    let before = tiles_on(&board, WorkspaceId::THREE).len();
    drag_to(&mut board, WorkspaceId::TWO, 0, three);
    assert_eq!(tiles_on(&board, WorkspaceId::TWO), [SAFARI]);
    assert_eq!(tiles_on(&board, WorkspaceId::THREE).len(), before + 1);
    assert_eq!(tiles_on(&board, WorkspaceId::THREE).last(), Some(&TERMINAL));
    assert!(board.drag.is_none());

    // Dropped between cards, nothing moves.
    let geometry = board.geometry();
    assert_eq!(geometry.target_at(-20.0, 30.0), None);
    drag_to(&mut board, WorkspaceId::TWO, 0, (-20.0, 30.0));
    assert_eq!(tiles_on(&board, WorkspaceId::TWO), [SAFARI]);

    // Dropped on the Floating line, the app floats; pressing its name tiles it again.
    let strip = (320.0, geometry.strip_y + 10.0);
    assert_eq!(geometry.target_at(strip.0, strip.1), Some(Target::Float));
    drag_to(&mut board, WorkspaceId::THREE, before, strip);
    assert!(board.plan.floating.iter().any(|app| app == TERMINAL));
    assert!(!tiles_on(&board, WorkspaceId::THREE).contains(&TERMINAL));
    board.set_floating(TERMINAL, false);
    assert!(!board.plan.floating.iter().any(|app| app == TERMINAL));
    assert_eq!(tiles_on(&board, WorkspaceId::TWO).last(), Some(&TERMINAL));

    // A cancelled drag puts nothing anywhere.
    let geometry = board.geometry();
    let card = &geometry.cards[0];
    let tile = Tile {
        workspace: card.workspace,
        index: 0,
        bundle_id: SAFARI.into(),
        frame: geometry.tiles(card, 1)[0],
    };
    board.drag_step(
        rune::Phase::Down,
        (4.0, 4.0),
        (0.0, 0.0),
        false,
        &tile,
        &geometry,
    );
    board.drag_step(
        rune::Phase::Move,
        (4.0, 4.0),
        (300.0, 0.0),
        false,
        &tile,
        &geometry,
    );
    assert!(board.drag.is_some());
    board.drag_step(
        rune::Phase::Up,
        (4.0, 4.0),
        (300.0, 0.0),
        true,
        &tile,
        &geometry,
    );
    assert_eq!(tiles_on(&board, WorkspaceId::ONE), [SAFARI]);
}

#[test]
fn the_board_lays_out_one_display_two_displays_and_one_window() {
    // One display: its cards share a line and fit the step.
    let laptop = Board::fixture_laptop();
    let geometry = laptop.geometry();
    assert_eq!(geometry.cards.len(), 5);
    assert_eq!(geometry.screens.len(), 1);
    assert!(
        geometry
            .cards
            .iter()
            .all(|card| card.y == geometry.cards[0].y)
    );
    let last = geometry.cards.last().unwrap();
    assert!(geometry.cards[0].x >= 0.0 && last.x + last.w <= super::arrange::WIDTH);
    // Tiles stay inside their card and never overlap.
    let frames = geometry.tiles(&geometry.cards[0], 3);
    for (index, [x, y, w, h]) in frames.iter().copied().enumerate() {
        let card = &geometry.cards[0];
        assert!(x >= card.x && x + w <= card.x + card.w, "tile {index}");
        assert!(y > card.y && w > 0.0 && h > 0.0, "tile {index}");
    }
    assert!(frames[0][0] + frames[0][2] <= frames[1][0]);
    assert!(frames[1][1] + frames[1][3] <= frames[2][1]);

    // Two displays, then three: each is its own box where it sits on the desk, holding its
    // cards, and no two boxes overlap.
    let dual = Board::fixture_dual();
    for board in [&dual, &Board::fixture_triple()] {
        let geometry = board.geometry();
        let screens = &geometry.screens;
        assert_eq!(screens.len(), board.inventory().unwrap().lanes.len());
        for (card, planned) in geometry.cards.iter().zip(&board.plan.workspaces) {
            assert_eq!(card.workspace, planned.workspace);
            let screen = screens.iter().find(|s| s.screen == planned.screen).unwrap();
            assert!(card.x >= screen.x && card.x + card.w <= screen.x + screen.w);
            assert!(card.y >= screen.y && card.y + card.mini_h <= screen.y + screen.h);
        }
        for (index, a) in screens.iter().enumerate() {
            for b in &screens[index + 1..] {
                let apart =
                    a.x + a.w <= b.x || b.x + b.w <= a.x || a.y + a.h <= b.y || b.y + b.h <= a.y;
                assert!(apart, "{} overlaps {}", a.name, b.name);
            }
            assert!(a.x >= 0.0 && a.x + a.w <= super::arrange::WIDTH);
        }
    }
    // The laptop of the three sits left of the main display, as on the desk.
    let triple = Board::fixture_triple().geometry();
    let x_of = |name: &str| triple.screens.iter().find(|s| s.name == name).unwrap().x;
    assert!(x_of("Built-in Retina Display") < x_of("Studio Display"));
    assert!(x_of("Studio Display") < x_of("LG UltraFine"));
    assert_eq!(tiles_on(&dual, WorkspaceId::ONE), [SAFARI, SAFARI]);
    assert_eq!(tiles_on(&dual, WorkspaceId::FIVE).len(), 2);

    // One window: the starter line shows, and only once the windows were read.
    assert!(Board::fixture_single().nothing_to_arrange());
    assert!(!Board::default().nothing_to_arrange());
}

#[test]
fn arrange_and_start_saves_no_rules_for_utilities_that_float_by_default() {
    let mut ui = crate::playground::blank_fixture();
    super::tour_scene(&mut ui, Step::Arrange, true, false);
    let before = ui.doc.rules();
    assert!(!ui.agent.as_ref().unwrap().automatic);

    super::input(&mut ui, Input::Continue);

    let tour = ui.tour.as_ref().unwrap();
    assert_eq!(tour.flow.step(), Step::TryIt);
    assert!(ui.agent.as_ref().unwrap().automatic, "tiling started");
    let added: Vec<_> = ui
        .doc
        .rules()
        .into_iter()
        .filter(|rule| !before.iter().any(|old| old.bundle_id == rule.bundle_id))
        .collect();
    // Stickies and Calculator float by default, so nothing needed a rule, and no browser,
    // terminal, mail or notes app got a home.
    assert!(added.is_empty(), "{added:?}");

    // Arranging again writes nothing new.
    let rules = ui.doc.rules().len();
    super::input(&mut ui, Input::Undo);
    let tour = ui.tour.as_ref().unwrap();
    assert_eq!(tour.flow.step(), Step::Arrange);
    assert!(!ui.agent.as_ref().unwrap().automatic, "paused and restored");
    super::input(&mut ui, Input::Continue);
    assert_eq!(ui.doc.rules().len(), rules);

    // Undo once more, then Skip: the tour ends with the windows where they were.
    super::input(&mut ui, Input::Undo);
    super::input(&mut ui, Input::Skip);
    assert!(ui.tour.is_none());
    assert!(!ui.agent.as_ref().unwrap().automatic);
}

#[test]
fn the_worker_inventory_becomes_the_plan_only_while_a_tour_is_open() {
    use fractile::onboarding_plan::{Inventory, Lane, OpenApp, OpenWindow};
    let inventory = || Inventory {
        apps: vec![OpenApp {
            bundle_id: TERMINAL.into(),
            category: None,
            windows: vec![
                OpenWindow {
                    resizable: true,
                    width: 900,
                    height: 700,
                };
                3
            ],
        }],
        lanes: vec![Lane {
            width: 1512,
            workspaces: vec![WorkspaceId::ONE, WorkspaceId::TWO, WorkspaceId::THREE],
            ..Lane::default()
        }],
    };
    let mut ui = crate::playground::blank_fixture();
    assert!(!super::inventory(&mut ui, inventory()));

    super::tour_scene(&mut ui, Step::Arrange, true, false);
    assert!(super::inventory(&mut ui, inventory()));
    let board = &ui.tour.as_ref().unwrap().board;
    // Code starts on workspace 2; three terminals are two there and one on an empty one.
    assert_eq!(tiles_on(board, WorkspaceId::TWO), [TERMINAL, TERMINAL]);
    assert_eq!(tiles_on(board, WorkspaceId::ONE), [TERMINAL]);
    assert!(ui.apps.cached(TERMINAL).is_some(), "its icon was looked up");
}

#[test]
fn every_tour_scene_draws() {
    let p = crate::controls::Paint::new(true, 2.0);
    for (name, apply) in super::SCENES {
        let mut ui = crate::playground::blank_fixture();
        apply(&mut ui);
        assert!(super::overlay(&ui, p, false).is_some(), "{name}");
        let light = crate::controls::Paint::new(false, 1.0);
        assert!(super::overlay(&ui, light, false).is_some(), "{name}");
        // Under Reduce Motion too.
        assert!(super::overlay(&ui, p, true).is_some(), "{name}");
    }
}

#[test]
fn tiling_a_built_in_float_on_the_board_saves_a_tile_rule() {
    let mut ui = crate::playground::blank_fixture();
    super::tour_scene(&mut ui, Step::Arrange, true, false);
    let before = ui.doc.rules();
    ui.tour
        .as_mut()
        .unwrap()
        .board
        .set_floating(STICKIES, false);

    super::input(&mut ui, Input::Continue);

    let added: Vec<_> = ui
        .doc
        .rules()
        .into_iter()
        .filter(|rule| !before.iter().any(|old| old.bundle_id == rule.bundle_id))
        .map(|rule| (rule.bundle_id, rule.place))
        .collect();
    assert_eq!(added, [(Some(STICKIES.to_owned()), Place::Tile)]);
}
