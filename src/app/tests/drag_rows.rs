//! Top and bottom display edge drop bands on portrait displays, behind
//! `[labs] portrait_edge_rows` (#366).
use super::*;
use crate::app::commands::{DISPLAY_EDGE_DROP_BAND, display_row_bands};
use crate::app::input::PointerBounds;

fn rect(x: i32, y: i32, width: i32, height: i32) -> crate::Rect {
    crate::Rect {
        x,
        y,
        width,
        height,
    }
}

// A portrait main display with a 25-point menu bar, and a landscape display above it.
fn displays() -> macos::DisplayOutputs {
    let display = |id: &str, frame, is_main| macos::DisplayOutput {
        id: id.into(),
        name: id.into(),
        frame,
        is_main,
    };
    macos::DisplayOutputs::with_bounds(
        vec![
            display("main", rect(0, 25, 1440, 2535), true),
            display("above", rect(-560, -1440, 2560, 1440), false),
        ],
        vec![rect(0, 0, 1440, 2560), rect(-560, -1440, 2560, 1440)],
    )
    .unwrap()
}

fn labs_on() -> Config {
    Config::parse("version = 1\n[labs]\nportrait_edge_rows = true\n").unwrap()
}

// `ids` side by side as columns on workspace 1 of the portrait display.
fn columns(ids: &[u32], config: Config) -> WorkspaceRuntime<FakeBackend> {
    let mut backend = FakeBackend {
        focused: Some(window(ids[0])),
        ..FakeBackend::default()
    };
    for id in ids {
        backend.frames.insert(window(*id), frame(50));
    }
    let mut rt = runtime_with_config(backend, config);
    for id in ids {
        bind(&mut rt, WorkspaceId::ONE, &window(*id));
    }
    let engine = rt.workspaces.engine_mut(WorkspaceId::ONE);
    for pair in ids.windows(2) {
        assert!(engine.move_window_relative(
            &window(pair[1]),
            &window(pair[0]),
            crate::Direction::Right
        ));
    }
    rt.layout_workspace(
        WorkspaceId::ONE,
        displays().main().frame,
        &AtomicBool::new(false),
    );
    rt
}

fn drag(
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

const COLUMNS: [(u32, i32, i32, i32, i32); 2] = [(10, 0, 25, 720, 2535), (20, 720, 25, 720, 2535)];

// `upper` in a full-width top row over `lower`.
fn rows(upper: u32, lower: u32) -> [(u32, i32, i32, i32, i32); 2] {
    [(upper, 0, 25, 1440, 1267), (lower, 0, 1292, 1440, 1268)]
}

#[test]
fn either_column_dropped_at_the_top_or_bottom_edge_becomes_that_row() {
    for dragged in [10_u32, 20] {
        let other = 30 - dragged;
        let start = if dragged == 10 {
            (360, 1200)
        } else {
            (1080, 1200)
        };
        // Every tenth point across the width between the side bands: in the menu bar, in the
        // top band, and in the bottom band.
        let mut x = DISPLAY_EDGE_DROP_BAND;
        while x < 1440 - DISPLAY_EDGE_DROP_BAND {
            for (y, expected) in [
                (5, rows(dragged, other)),
                (28, rows(dragged, other)),
                (2557, rows(other, dragged)),
            ] {
                let mut rt = columns(&[10, 20], labs_on());
                assert_eq!(layout(&rt), COLUMNS);
                drag(&mut rt, dragged, start, &[], (x, y));
                assert_eq!(layout(&rt), expected, "dragged {dragged} to ({x}, {y})");
            }
            x += 10;
        }
    }
}

#[test]
fn rows_need_the_lab_and_a_portrait_display() {
    // Lab off: the top band is just the dragged window's own tile.
    let mut rt = columns(&[10, 20], Config::default());
    drag(&mut rt, 10, (360, 1200), &[], (360, 28));
    assert_eq!(layout(&rt), COLUMNS);

    // Lab on, but a landscape or square tiled area has no row bands.
    let landscape = rect(0, 25, 2560, 1415);
    assert!(display_row_bands(landscape, landscape, rect(0, 0, 2560, 1440)).is_none());
    let square = rect(0, 0, 1000, 1000);
    assert!(display_row_bands(square, square, square).is_none());
}

#[test]
fn a_nudge_that_never_leaves_the_top_band_changes_nothing() {
    let mut rt = columns(&[10, 20], labs_on());
    drag(&mut rt, 10, (360, 35), &[(380, 40)], (400, 45));
    assert_eq!(layout(&rt), COLUMNS);
}

#[test]
fn entering_and_leaving_the_band_before_release_keeps_the_columns() {
    let mut rt = columns(&[10, 20], labs_on());
    let previews = [(360, 30), (360, 1200)];
    drag(&mut rt, 10, (360, 1200), &previews, (360, 1210));
    assert_eq!(layout(&rt), COLUMNS);
}

#[test]
fn the_side_band_wins_in_a_corner() {
    let mut rt = columns(&[10, 20], labs_on());
    // 20 dropped in the top-left corner docks into the left column, not the top row.
    drag(&mut rt, 20, (1080, 1200), &[], (10, 30));
    assert_eq!(
        layout(&rt),
        [(20, 0, 25, 720, 2535), (10, 720, 25, 720, 2535)]
    );
}

#[test]
fn an_unarmed_side_band_still_keeps_its_corner() {
    let mut rt = columns(&[10, 20], labs_on());
    // Grabbed inside the left band and moved up into the top-left corner without leaving the
    // left band: the side band is not armed yet, and the corner is not a top row either.
    drag(&mut rt, 10, (20, 1200), &[(20, 600)], (20, 30));
    assert_eq!(layout(&rt), COLUMNS);
}

#[test]
fn crossing_onto_the_display_above_still_moves_the_window_there() {
    let mut rt = columns(&[10, 20], labs_on());
    drag(&mut rt, 10, (360, 1200), &[(360, 10)], (360, -100));
    assert_eq!(
        rt.workspaces.workspace_of(&window(10)),
        Some(WorkspaceId::FIVE)
    );
    assert_eq!(layout(&rt), [(20, 0, 25, 1440, 2535)]);
}

#[test]
fn three_tiles_keep_their_nesting_below_a_top_row() {
    // 10 | 20 | 30 as columns; dock 30 on top, 10 | 20 keep their columns beneath.
    let mut rt = columns(&[10, 20, 30], labs_on());
    drag(&mut rt, 30, (1200, 1200), &[], (720, 30));
    assert_eq!(
        layout(&rt),
        [
            (30, 0, 25, 1440, 1267),
            (10, 0, 1292, 720, 1268),
            (20, 720, 1292, 720, 1268)
        ]
    );
}
