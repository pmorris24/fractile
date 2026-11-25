//! #391: focus next after a cross-display drop starts from the dropped tile.
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
fn pending_focus_cross_drop() {
    let main = WorkspaceId::ONE;
    let right = WorkspaceId::new(8).unwrap();
    let mut results = vec![];
    for layout in ["columns", "dwindle"] {
        for (pending, expired) in [(false, false), (true, false), (true, true)] {
            let mut rt = setup(
                layout,
                &[
                    (10, main, rect(0, 30, 1280, 1410)),
                    (11, main, rect(1280, 30, 1280, 1410)),
                    (90, right, rect(2560, -539, 1440, 2530)),
                ],
            );
            action(&mut rt, WorkerAction::Retile);
            if pending {
                rt.backend.mouse_down = true;
                action(&mut rt, WorkerAction::Focus(Direction::Right));
                assert_eq!(rt.backend.focused, Some(window(11)));
                rt.backend.mouse_down = false;
            }
            let start = rt.backend.frames[&window(10)];
            drag(
                &mut rt,
                (start.x + start.width / 2, start.y + 12),
                (3300, 150),
            );
            assert_eq!(rt.backend.focused, Some(window(10)));
            assert_eq!(rt.workspaces.workspace_of(&window(10)), Some(right));
            assert_eq!(rt.backend.pointer, Some((3300, 150)));
            if expired {
                rt.focus.age_request(crate::app::focus::FOCUS_SETTLE);
            }
            action(&mut rt, WorkerAction::CycleFocus);
            println!(
                "{layout} pending={pending} expired={expired} next={:?}",
                rt.backend.focused.as_ref().map(|w| w.window_id)
            );
            results.push(rt.backend.focused.as_ref().map(|w| w.window_id));
        }
    }
    assert_eq!(results, vec![Some(90); 6]);
}
