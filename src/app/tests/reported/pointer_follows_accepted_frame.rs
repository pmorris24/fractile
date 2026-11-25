//! #418: the pointer follows the frame a constrained window accepted, not its tile.
//! Reproduction contributed by @coygeek.

use super::super::*;

#[test]
fn native_pointer_follow_uses_the_focused_windows_accepted_frame() {
    let left = crate::Rect {
        x: -1440,
        y: -583,
        width: 1440,
        height: 2530,
    };
    let main = crate::Rect {
        x: 0,
        y: 30,
        width: 2560,
        height: 1410,
    };
    let right = crate::Rect {
        x: 2560,
        y: -539,
        width: 1440,
        height: 2530,
    };
    let outputs = macos::DisplayOutputs::new(
        [
            ("main-id", main, true),
            ("left-id", left, false),
            ("right-id", right, false),
        ]
        .into_iter()
        .map(|(id, frame, is_main)| macos::DisplayOutput {
            id: id.into(),
            name: id.into(),
            frame,
            is_main,
        })
        .collect(),
    )
    .unwrap();
    let right_ws = WorkspaceId::new(9).unwrap();
    let mut failures = Vec::new();

    for layout in ["columns", "dwindle"] {
        for gap in [0, 20] {
            for (mode, width, pointer) in [
                ("display", 0, (1280, 500)),
                ("display", 1800, (1280, 500)),
                ("window", 0, (100, 500)),
                ("window", 1800, (100, 500)),
            ] {
                let config = Config::parse(&format!(
                    "version = 1\n[settings]\nlayout = '{layout}'\nmouse_follows_focus = '{mode}'\n                     [settings.gaps]\nouter = {gap}\n                     [[workspaces]]\nworkspace = 9\nmonitor = 'right-id'\n"
                )).unwrap();
                let origin = window(10);
                let target = window(20);
                let far = window(30);
                let backend = FakeBackend {
                    frames: [
                        (origin.clone(), main),
                        (target.clone(), left),
                        (far.clone(), right),
                    ]
                    .into_iter()
                    .collect(),
                    focused: Some(origin.clone()),
                    minimum_widths: [(target.clone(), width)].into_iter().collect(),
                    pointer: Some(pointer),
                    ..FakeBackend::default()
                };
                let mut rt = runtime_with_config(backend, config);
                bind(&mut rt, WorkspaceId::ONE, &origin);
                bind(&mut rt, WorkspaceId::FIVE, &target);
                bind(&mut rt, right_ws, &far);
                let cancelled = AtomicBool::new(false);
                for (workspace, frame) in [
                    (WorkspaceId::ONE, main),
                    (WorkspaceId::FIVE, left),
                    (right_ws, right),
                ] {
                    rt.layout_workspace(workspace, frame, &cancelled);
                }
                assert_eq!(rt.visible_workspaces(&outputs).len(), 3);
                let actual = rt.backend.frames[&target];
                let Some(WorkerResult::Status { result, .. }) = rt.execute(
                    WorkerCommand::Action(WorkerAction::Focus(Direction::Left), outputs.clone()),
                    &cancelled,
                ) else {
                    panic!("missing command status");
                };
                assert_eq!(result.unwrap().failed, 0);
                assert_eq!(rt.backend.focused, Some(target));
                let expected = if mode == "window" && actual.contains(pointer.0, pointer.1) {
                    pointer
                } else {
                    (actual.x + actual.width / 2, actual.y + actual.height / 2)
                };
                let observed = rt.backend.pointer.unwrap();
                println!(
                    "{layout} gap={gap} {mode} minimum={width}: frame={actual:?} got={observed:?} expected={expected:?}"
                );
                if observed != expected {
                    failures.push((layout, gap, mode, width));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{failures:?}");
}
