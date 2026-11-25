//! #392: A tile can remain physically on another display after a drag while Fullscreen Layout still owns it on the original workspace.
//! Reproduction contributed by @coygeek.

use super::super::*;
use crate::app::input::GestureTracker;

fn rect(x: i32, y: i32, width: i32, height: i32) -> crate::Rect {
    crate::Rect {
        x,
        y,
        width,
        height,
    }
}

fn trio() -> macos::DisplayOutputs {
    let records = [
        ("center", rect(0, 30, 2560, 1410), true),
        ("left", rect(-1440, -583, 1440, 2530), false),
        ("right", rect(2560, -539, 1440, 2530), false),
    ];
    macos::DisplayOutputs::with_bounds(
        records
            .iter()
            .map(|&(id, frame, is_main)| macos::DisplayOutput {
                id: id.into(),
                name: id.into(),
                frame,
                is_main,
            })
            .collect(),
        records
            .iter()
            .map(|&(_, frame, _)| rect(frame.x, frame.y - 30, frame.width, frame.height + 30))
            .collect(),
    )
    .unwrap()
}

fn setup(layout: &str, tiles: &[(u32, WorkspaceId, crate::Rect)]) -> WorkspaceRuntime<FakeBackend> {
    let moved = window(10);
    let windows: Vec<_> = tiles
        .iter()
        .map(|&(id, _, frame)| (window(id), frame))
        .collect();
    let backend = FakeBackend {
        frames: windows.iter().cloned().collect(),
        observation: Some(observation_for_windows(
            "com.example.Test",
            &moved,
            &windows,
        )),
        focused: Some(moved.clone()),
        frontmost: Some(moved.app),
        ..FakeBackend::default()
    };
    let config = Config::parse(&format!("version = 1\n[settings]\nlayout = '{layout}'\nmouse_follows_focus = 'display'\n[[workspaces]]\nworkspace = 8\nmonitor = 'right'\n")).unwrap();
    let mut rt = runtime_with_config(backend, config);
    for &(id, ws, _) in tiles {
        bind(&mut rt, ws, &window(id));
    }
    rt
}

fn run(rt: &mut WorkspaceRuntime<FakeBackend>, command: WorkerCommand) {
    let Some(WorkerResult::Status { result, .. }) = rt.execute(command, &AtomicBool::new(false))
    else {
        panic!("expected worker status");
    };
    assert_eq!(result.unwrap().failed, 0);
}

fn action(rt: &mut WorkspaceRuntime<FakeBackend>, action: WorkerAction) {
    run(rt, WorkerCommand::Action(action, trio()));
}

fn drag(rt: &mut WorkspaceRuntime<FakeBackend>, from: (i32, i32), to: (i32, i32)) {
    let moved = window(10);
    let start = rt.backend.frames[&moved];
    let final_frame = rect(
        start.x + to.0 - from.0,
        start.y + to.1 - from.1,
        start.width,
        start.height,
    );
    let mut tracker = GestureTracker::default();
    tracker.begin_at(vec![(moved.clone(), start)], Some(from));
    rt.backend.frames.insert(moved, final_frame);
    rt.backend.pointer = Some(to);
    let final_gesture = tracker.finish(|_| Some(final_frame), Some(to)).unwrap();
    run(rt, WorkerCommand::Gesture(final_gesture, false, trio()));
}

#[test]
fn fullscreen_tile_cross_drop() {
    let main = WorkspaceId::ONE;
    let right = WorkspaceId::new(8).unwrap();
    let mut found = vec![];
    for fullscreen in [false, true] {
        let mut rt = setup(
            "columns",
            &[
                (10, main, rect(0, 30, 2560, 1410)),
                (90, right, rect(2560, -539, 1440, 2530)),
            ],
        );
        action(&mut rt, WorkerAction::Retile);
        if fullscreen {
            action(&mut rt, WorkerAction::ToggleFullscreenLayout);
            assert!(rt.workspaces.deck(main).is_monocle());
        }
        let start = rt.backend.frames[&window(10)];
        drag(&mut rt, (1700, 42), (3200, 42));
        for _ in 0..3 {
            action(&mut rt, WorkerAction::Retile);
        }
        let expected = if fullscreen { main } else { right };
        assert_eq!(rt.workspaces.workspace_of(&window(10)), Some(expected));
        let frame = rt.backend.frames[&window(10)];
        println!(
            "fullscreen={fullscreen} frame={frame:?} constrained={:?}",
            rt.constrained[expected.index()].get(&window(10))
        );
        found.push(if fullscreen {
            frame == start
        } else {
            trio().displays()[2].frame.contains_center(frame)
        });
    }
    assert_eq!(found, vec![true; 2]);
}

#[test]
fn external_move_is_not_an_apply_bounce() {
    let main = WorkspaceId::ONE;
    let right = WorkspaceId::new(8).unwrap();
    let mut found = vec![];
    for recent_write in [false, true] {
        let moved = window(10);
        let start = rect(0, 30, 2560, 1410);
        let end = rect(1500, 30, 2560, 1410);
        let mut rt = setup(
            "columns",
            &[
                (
                    10,
                    main,
                    if recent_write {
                        rect(100, 100, 900, 700)
                    } else {
                        start
                    },
                ),
                (90, right, rect(2560, -539, 1440, 2530)),
            ],
        );
        action(&mut rt, WorkerAction::Retile);
        assert_eq!(rt.backend.frames[&moved], start);
        assert_eq!(rt.applied[main.index()].contains_key(&moved), recent_write);
        rt.backend.frames.insert(moved.clone(), end);
        assert!(trio().displays()[2].frame.contains_center(end));
        for _ in 0..4 {
            action(&mut rt, WorkerAction::Retile);
        }
        let frame = rt.backend.frames[&moved];
        assert_eq!(rt.workspaces.workspace_of(&moved), Some(main));
        println!(
            "recent_write={recent_write} frame={frame:?} constrained={:?}",
            rt.constrained[main.index()].get(&moved)
        );
        found.push(frame == start);
    }
    assert_eq!(found, vec![true; 2]);
}
