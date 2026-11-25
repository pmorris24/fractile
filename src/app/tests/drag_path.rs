//! Regression tests for non-destructive drag previews (#340) and outer
//! left/right display edge drop bands (#341).
use super::*;
use crate::app::commands::{DISPLAY_EDGE_DROP_BAND, display_edge_bands};
use crate::app::input::{GestureTracker, PointerBounds};

fn rect(x: i32, y: i32, width: i32, height: i32) -> crate::Rect {
    crate::Rect {
        x,
        y,
        width,
        height,
    }
}

// A landscape main display and a portrait display to its left.
fn displays() -> macos::DisplayOutputs {
    let display = |id: &str, frame, is_main| macos::DisplayOutput {
        id: id.into(),
        name: id.into(),
        frame,
        is_main,
    };
    macos::DisplayOutputs::new(vec![
        display("main", rect(0, 30, 2560, 1410), true),
        display("left", rect(-1440, -583, 1440, 2530), false),
    ])
    .unwrap()
}

fn setup_with_config(windows: &[u32], config: Config) -> WorkspaceRuntime<FakeBackend> {
    let mut backend = FakeBackend {
        focused: Some(window(windows[0])),
        ..FakeBackend::default()
    };
    for id in windows {
        backend.frames.insert(window(*id), frame(50));
    }
    let mut rt = runtime_with_config(backend, config);
    for id in windows {
        bind(&mut rt, WorkspaceId::ONE, &window(*id));
    }
    rt
}

fn setup(windows: &[u32]) -> WorkspaceRuntime<FakeBackend> {
    setup_with_config(windows, Config::default())
}

// Sends optional previews, then mouse-up, through the worker command the app uses.
fn drag(rt: &mut WorkspaceRuntime<FakeBackend>, id: u32, previews: &[(i32, i32)], end: (i32, i32)) {
    for (point, preview) in previews
        .iter()
        .map(|point| (*point, true))
        .chain([(end, false)])
    {
        let command =
            WorkerCommand::Gesture(FinalGesture::drop(window(id), point), preview, displays());
        let Some(WorkerResult::Status { result, .. }) =
            rt.execute(command, &AtomicBool::new(false))
        else {
            panic!("expected worker status");
        };
        result.unwrap();
    }
}

fn drag_tracked(
    rt: &mut WorkspaceRuntime<FakeBackend>,
    id: u32,
    start: (i32, i32),
    previews: &[(i32, i32)],
    end: (i32, i32),
) {
    let mut bounds = PointerBounds::point(start);
    for (point, preview) in previews
        .iter()
        .map(|point| (*point, true))
        .chain([(end, false)])
    {
        bounds.extend(point);
        let command = WorkerCommand::Gesture(
            FinalGesture::Drop {
                window: window(id),
                point,
                bounds: Some(bounds),
            },
            preview,
            displays(),
        );
        let Some(WorkerResult::Status { result, .. }) =
            rt.execute(command, &AtomicBool::new(false))
        else {
            panic!("expected worker status");
        };
        result.unwrap();
    }
}

fn layout(rt: &WorkspaceRuntime<FakeBackend>) -> Vec<(u32, i32, i32, i32, i32)> {
    let settings = rt.layout_settings(displays().main().frame);
    rt.workspaces
        .engine(WorkspaceId::ONE)
        .placements(&settings)
        .into_iter()
        .map(|p| {
            let id = p.window.unwrap().window_id;
            (id, p.frame.x, p.frame.y, p.frame.width, p.frame.height)
        })
        .collect()
}

// 10 over 20, both full width.
fn stacked() -> WorkspaceRuntime<FakeBackend> {
    stacked_with_config(Config::default())
}

fn stacked_with_config(config: Config) -> WorkspaceRuntime<FakeBackend> {
    let mut rt = setup_with_config(&[10, 20], config);
    let engine = rt.workspaces.engine_mut(WorkspaceId::ONE);
    assert!(engine.move_window_relative(&window(20), &window(10), crate::Direction::Down));
    rt.layout_workspace(
        WorkspaceId::ONE,
        displays().main().frame,
        &AtomicBool::new(false),
    );
    rt
}

// 10 on the left, 20 over 30 on the right.
fn one_beside_two() -> WorkspaceRuntime<FakeBackend> {
    let mut rt = setup(&[10, 20, 30]);
    let engine = rt.workspaces.engine_mut(WorkspaceId::ONE);
    assert!(engine.move_window_relative(&window(30), &window(20), crate::Direction::Down));
    assert_eq!(
        layout(&rt),
        [
            (10, 0, 30, 1280, 1410),
            (20, 1280, 30, 1280, 705),
            (30, 1280, 735, 1280, 705)
        ]
    );
    rt
}

const SIDE_BY_SIDE: [(u32, i32, i32, i32, i32); 2] =
    [(20, 0, 30, 1280, 1410), (10, 1280, 30, 1280, 1410)];

// --- #340: non-destructive drag previews ---

#[test]
fn control_drop_on_left_edge_without_previews() {
    let mut rt = stacked();
    drag(&mut rt, 20, &[], (100, 380));
    assert_eq!(layout(&rt), SIDE_BY_SIDE);
}

#[test]
fn drop_on_left_edge_after_a_preview_over_the_middle() {
    let mut rt = stacked();
    drag(&mut rt, 20, &[(1280, 380)], (100, 380));
    assert_eq!(layout(&rt), SIDE_BY_SIDE);
}

#[test]
fn dense_drag_through_the_middle_to_the_left_edge() {
    let mut rt = stacked();
    let up = (0..18).map(|step| (1280, 735 - step * 20));
    let left = (0..30).map(|step| (1280 - step * 40, 380));
    let path: Vec<_> = up.chain(left).collect();
    drag(&mut rt, 20, &path, (100, 380));
    assert_eq!(layout(&rt), SIDE_BY_SIDE);
}

#[test]
fn drop_on_right_edge_after_a_preview_over_the_middle() {
    let mut rt = stacked();
    drag(&mut rt, 20, &[(1280, 380)], (2460, 380));
    assert_eq!(
        layout(&rt),
        [(10, 0, 30, 1280, 1410), (20, 1280, 30, 1280, 1410)]
    );
}

#[test]
fn control_drop_on_other_display_without_previews() {
    let mut rt = one_beside_two();
    drag(&mut rt, 30, &[], (-700, 1300));
    assert_eq!(
        rt.workspaces.workspace_of(&window(30)),
        Some(WorkspaceId::FIVE)
    );
    assert_eq!(
        layout(&rt),
        [(10, 0, 30, 1280, 1410), (20, 1280, 30, 1280, 1410)]
    );
}

#[test]
fn control_slow_drag_to_other_display() {
    let mut rt = one_beside_two();
    let path: Vec<_> = (0..38).map(|step| (1900 - step * 50, 735)).collect();
    drag(&mut rt, 30, &path, (-700, 1300));
    assert_eq!(
        rt.workspaces.workspace_of(&window(30)),
        Some(WorkspaceId::FIVE)
    );
    assert_eq!(
        layout(&rt),
        [(10, 0, 30, 1280, 1410), (20, 1280, 30, 1280, 1410)]
    );
}

#[test]
fn control_entering_through_the_left_quarter() {
    let mut rt = stacked();
    let path: Vec<_> = (0..8).map(|step| (300, 700 - step * 40)).collect();
    drag(&mut rt, 20, &path, (100, 380));
    assert_eq!(layout(&rt), SIDE_BY_SIDE);
}

#[test]
fn drop_on_other_display_after_a_preview_over_a_sibling() {
    let mut rt = one_beside_two();
    drag(&mut rt, 30, &[(640, 735)], (-700, 1300));
    assert_eq!(
        rt.workspaces.workspace_of(&window(30)),
        Some(WorkspaceId::FIVE)
    );
    assert_eq!(
        layout(&rt),
        [(10, 0, 30, 1280, 1410), (20, 1280, 30, 1280, 1410)]
    );
}

#[test]
fn control_layout_toggle_restores_side_by_side() {
    let mut rt = stacked();
    let command = WorkerCommand::Action(WorkerAction::ToggleLayout, displays());
    rt.execute(command, &AtomicBool::new(false));
    assert_eq!(
        layout(&rt),
        [(10, 0, 30, 1280, 1410), (20, 1280, 30, 1280, 1410)]
    );
}

#[test]
fn preview_crossing_to_another_display_or_cancelling_restores_siblings() {
    let mut rt = one_beside_two();
    let before = layout(&rt);

    // Preview over A's middle slides A into C's slot on screen, without mutating layout.
    let swap_preview =
        WorkerCommand::Gesture(FinalGesture::drop(window(30), (640, 735)), true, displays());
    rt.execute(swap_preview, &AtomicBool::new(false));
    assert_eq!(rt.backend.frames[&window(10)], rect(1280, 735, 1280, 705));
    assert_eq!(layout(&rt), before);

    // Preview moving onto the left display immediately restores A to its pre-drag frame.
    let leave_preview = WorkerCommand::Gesture(
        FinalGesture::drop(window(30), (-700, 735)),
        true,
        displays(),
    );
    rt.execute(leave_preview, &AtomicBool::new(false));
    assert_eq!(rt.backend.frames[&window(10)], rect(0, 30, 1280, 1410));
    assert_eq!(layout(&rt), before);

    // Cancelling the drag (another command arrives without mouse-up) leaves no ghost swap.
    let swap_again =
        WorkerCommand::Gesture(FinalGesture::drop(window(30), (640, 735)), true, displays());
    rt.execute(swap_again, &AtomicBool::new(false));
    assert_eq!(rt.backend.frames[&window(10)], rect(1280, 735, 1280, 705));

    rt.execute(
        WorkerCommand::Action(WorkerAction::CycleFocus, displays()),
        &AtomicBool::new(false),
    );
    assert_eq!(rt.backend.frames[&window(10)], rect(0, 30, 1280, 1410));
    assert_eq!(layout(&rt), before);
}

// --- #341: outer left/right display edge drop bands ---

#[test]
fn full_left_and_right_edge_bands_dock_either_row_into_columns() {
    let output = displays().main().frame;
    for dragged_id in [10_u32, 20_u32] {
        let other_id = if dragged_id == 10 { 20 } else { 10 };
        for (edge_x, expected) in [
            (
                output.x + 1,
                [
                    (dragged_id, 0, 30, 1280, 1410),
                    (other_id, 1280, 30, 1280, 1410),
                ],
            ),
            (
                output.x + output.width - 1,
                [
                    (other_id, 0, 30, 1280, 1410),
                    (dragged_id, 1280, 30, 1280, 1410),
                ],
            ),
        ] {
            let mut y = output.y;
            while y < output.y + output.height {
                let mut rt = stacked();
                drag_tracked(&mut rt, dragged_id, (1280, 45), &[], (edge_x, y));
                assert_eq!(
                    layout(&rt),
                    expected,
                    "dragged {dragged_id} to ({edge_x}, {y})"
                );
                y += 5;
            }
        }
    }
}

#[test]
fn edge_band_with_three_tiles_keeps_remaining_nesting_and_relative_weights() {
    let mut rt = one_beside_two();
    let settings = rt.layout_settings(displays().main().frame);
    // Give 20 and 30 unequal vertical weights (305 vs 1105 -> ~22% vs 78%) before docking 10
    // on the right edge, and also test docking 30 on the left edge.
    assert!(rt.workspaces.engine_mut(WorkspaceId::ONE).resize_window(
        &window(20),
        rect(1280, 30, 1280, 405),
        &settings,
        &std::collections::HashMap::new(),
    ));
    let before_20 = rt
        .workspaces
        .engine(WorkspaceId::ONE)
        .placements(&settings)
        .into_iter()
        .find(|p| p.window == Some(window(20)))
        .unwrap()
        .frame;
    assert_eq!(before_20.height, 405);

    // Dock 10 on the right edge -> 10 gets full-height right half, 20/30 keep vertical nesting
    // and their 405:1005 height ratio in the left half.
    drag_tracked(&mut rt, 10, (640, 45), &[], (2550, 100));
    assert_eq!(
        layout(&rt),
        [
            (20, 0, 30, 1280, 405),
            (30, 0, 435, 1280, 1005),
            (10, 1280, 30, 1280, 1410),
        ]
    );

    // Dock 30 on the left edge -> 30 gets full-height left half (1280), 20 | 10 keep their
    // horizontal arrangement in the right half.
    drag_tracked(&mut rt, 30, (640, 800), &[], (10, 100));
    assert_eq!(
        layout(&rt),
        [
            (30, 0, 30, 1280, 1410),
            (20, 1280, 30, 640, 1410),
            (10, 1920, 30, 640, 1410),
        ]
    );
}

#[test]
fn existing_left_or_right_column_dropped_in_its_own_edge_band_becomes_half_width() {
    let mut rt = setup(&[10, 20]);
    let settings = rt.layout_settings(displays().main().frame);
    // Resize 10 narrow (700px) and 20 wide (1860px).
    assert!(rt.workspaces.engine_mut(WorkspaceId::ONE).resize_window(
        &window(10),
        rect(0, 30, 700, 1410),
        &settings,
        &std::collections::HashMap::new(),
    ));
    assert_eq!(
        layout(&rt),
        [(10, 0, 30, 700, 1410), (20, 700, 30, 1860, 1410)]
    );

    // Dropping 10 in the left edge band resets it to half width (1280).
    drag_tracked(&mut rt, 10, (350, 45), &[], (10, 45));
    assert_eq!(
        layout(&rt),
        [(10, 0, 30, 1280, 1410), (20, 1280, 30, 1280, 1410)]
    );

    // Resize 20 narrow (600px) on the right, then drop 20 in the right edge band -> 1280.
    assert!(rt.workspaces.engine_mut(WorkspaceId::ONE).resize_window(
        &window(20),
        rect(1960, 30, 600, 1410),
        &settings,
        &std::collections::HashMap::new(),
    ));
    assert_eq!(
        layout(&rt),
        [(10, 0, 30, 1960, 1410), (20, 1960, 30, 600, 1410)]
    );
    drag_tracked(&mut rt, 20, (2200, 45), &[], (2550, 45));
    assert_eq!(
        layout(&rt),
        [(10, 0, 30, 1280, 1410), (20, 1280, 30, 1280, 1410)]
    );
}

#[test]
fn edge_band_counts_only_after_pointer_has_been_outside_it() {
    let mut rt = stacked();
    let before = layout(&rt);

    // Grabbed inside the left edge band and nudged inside it -> does NOT snap.
    drag_tracked(&mut rt, 10, (10, 45), &[(16, 45)], (20, 45));
    assert_eq!(layout(&rt), before);

    // Same via GestureTracker: nudging inside the band stays unarmed; leaving the band
    // while the worker is busy and returning arms it.
    let start_frame = rect(0, 30, 2560, 705);
    let nudged = rect(10, 30, 2560, 705);
    let mut tracker = GestureTracker::default();
    tracker.begin_at(vec![(window(10), start_frame)], Some((10, 45)));
    tracker.observe_point((20, 45));
    let nudge_gesture = tracker.finish(|_| Some(nudged), Some((20, 45))).unwrap();
    rt.execute(
        WorkerCommand::Gesture(nudge_gesture, false, displays()),
        &AtomicBool::new(false),
    );
    assert_eq!(layout(&rt), before);

    // Now grab at (10, 45), leave the left band to (300, 45), and return to (20, 45) -> snaps!
    let mut tracker = GestureTracker::default();
    tracker.begin_at(vec![(window(10), start_frame)], Some((10, 45)));
    tracker.observe_point((DISPLAY_EDGE_DROP_BAND + 40, 45));
    tracker.observe_point((20, 45));
    let armed_gesture = tracker.finish(|_| Some(nudged), Some((20, 45))).unwrap();
    rt.execute(
        WorkerCommand::Gesture(armed_gesture, false, displays()),
        &AtomicBool::new(false),
    );
    assert_eq!(
        layout(&rt),
        [(10, 0, 30, 1280, 1410), (20, 1280, 30, 1280, 1410)]
    );
}

#[test]
fn entering_and_leaving_an_edge_band_before_release_restores_original_layout() {
    let mut rt = stacked();
    let before = layout(&rt);

    // Preview enters left edge band -> sibling 20 slides to the right half.
    let mut bounds = PointerBounds::point((1280, 45));
    bounds.extend((10, 45));
    rt.execute(
        WorkerCommand::Gesture(
            FinalGesture::Drop {
                window: window(10),
                point: (10, 45),
                bounds: Some(bounds),
            },
            true,
            displays(),
        ),
        &AtomicBool::new(false),
    );
    assert_eq!(rt.backend.frames[&window(20)], rect(1280, 30, 1280, 1410));
    assert_eq!(layout(&rt), before);

    // Leaving the band back over 10's own tile undoes the preview; releasing keeps stacked rows.
    bounds.extend((1280, 100));
    rt.execute(
        WorkerCommand::Gesture(
            FinalGesture::Drop {
                window: window(10),
                point: (1280, 100),
                bounds: Some(bounds),
            },
            true,
            displays(),
        ),
        &AtomicBool::new(false),
    );
    assert_eq!(rt.backend.frames[&window(20)], rect(0, 735, 2560, 705));

    rt.execute(
        WorkerCommand::Gesture(
            FinalGesture::Drop {
                window: window(10),
                point: (1280, 100),
                bounds: Some(bounds),
            },
            false,
            displays(),
        ),
        &AtomicBool::new(false),
    );
    assert_eq!(layout(&rt), before);
}

#[test]
fn edge_band_includes_outer_gaps_and_stops_at_adjacent_display_boundary() {
    let config = Config::parse(
        "version = 1
[settings.gaps]
outer = 24
",
    )
    .unwrap();
    let mut rt = stacked_with_config(config.clone());
    let output = displays().main().frame;
    let usable = rt.layout_settings(output).usable();
    let (left_band, right_band) = display_edge_bands(output, usable);
    assert_eq!(left_band.x, output.x);
    assert_eq!(left_band.width, 24 + DISPLAY_EDGE_DROP_BAND);
    assert_eq!(right_band.x + right_band.width, output.x + output.width);
    assert_eq!(right_band.width, 24 + DISPLAY_EDGE_DROP_BAND);

    // Dropping inside the outer left gap (x = 4, still inside main display) snaps here.
    drag_tracked(&mut rt, 10, (1280, 100), &[], (4, 100));
    let docked = layout(&rt);
    assert_eq!(docked[0].0, 10);
    assert_eq!(docked[1].0, 20);
    assert_eq!(docked[0].3, docked[1].3);
    assert_eq!(docked[0].4, usable.height);

    // Crossing onto the adjacent left display (x = -1) moves the window there instead.
    let mut rt = stacked_with_config(config);
    drag_tracked(&mut rt, 10, (1280, 100), &[(4, 100)], (-1, 100));
    assert_eq!(
        rt.workspaces.workspace_of(&window(10)),
        Some(WorkspaceId::FIVE)
    );
}

#[test]
fn edge_band_leaves_lone_tile_and_fullscreen_layout_unchanged() {
    // 1. Lone tile is unchanged.
    let mut rt = setup(&[10]);
    let before = layout(&rt);
    drag_tracked(&mut rt, 10, (1280, 45), &[], (10, 45));
    assert_eq!(layout(&rt), before);

    // 2. Fullscreen Layout still ignores drags, including edge bands.
    let mut rt = stacked();
    let before = layout(&rt);
    rt.workspaces
        .set_deck(WorkspaceId::ONE, crate::workspace::DeckMode::Monocle);
    drag_tracked(&mut rt, 10, (1280, 45), &[], (10, 45));
    assert!(rt.workspaces.deck(WorkspaceId::ONE).is_monocle());
    rt.workspaces
        .set_deck(WorkspaceId::ONE, crate::workspace::DeckMode::Split);
    assert_eq!(layout(&rt), before);
}

#[test]
fn edge_band_drop_while_temporarily_stacked_edits_saved_layout() {
    let windows = [window(10), window(20), window(30)];
    let mut backend = FakeBackend {
        focused: Some(windows[0].clone()),
        ..FakeBackend::default()
    };
    for (w, minimum) in windows.iter().zip([400, 250, 450]) {
        backend.frames.insert(w.clone(), frame(0));
        backend.minimum_widths.insert(w.clone(), minimum);
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
    assert_eq!(
        rt.backend.frames[&windows[1]].x, rt.backend.frames[&windows[2]].x,
        "B and C start temporarily stacked"
    );

    // Drop C in the left edge band of the 1000x800 display.
    let mut bounds = PointerBounds::point((750, 600));
    bounds.extend((10, 600));
    rt.execute(
        WorkerCommand::Gesture(
            FinalGesture::Drop {
                window: windows[2].clone(),
                point: (10, 600),
                bounds: Some(bounds),
            },
            false,
            outputs(),
        ),
        &AtomicBool::new(false),
    );

    // On a wide display where the saved layout fits, C fills the left half and A | B share the right.
    let wide = rect(0, 0, 1600, 800);
    rt.layout_workspace(WorkspaceId::ONE, wide, &AtomicBool::new(false));
    assert_eq!(rt.backend.frames[&windows[2]], rect(0, 0, 800, 800));
    assert_eq!(rt.backend.frames[&windows[0]], rect(800, 0, 400, 800));
    assert_eq!(rt.backend.frames[&windows[1]], rect(1200, 0, 400, 800));
}
