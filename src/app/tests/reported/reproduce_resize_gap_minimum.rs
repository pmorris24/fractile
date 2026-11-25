//! #407: With inner gaps configured, an absolute keyboard resize can leave either the focused tile or its sibling below the documented 50-point minimum.
//! Reproduction contributed by @coygeek.

use super::super::*;

const CENTER: crate::Rect = crate::Rect {
    x: 0,
    y: 30,
    width: 2560,
    height: 1410,
};
const LEFT: crate::Rect = crate::Rect {
    x: -1440,
    y: -583,
    width: 1440,
    height: 2530,
};
const RIGHT: crate::Rect = crate::Rect {
    x: 2560,
    y: -539,
    width: 1440,
    height: 2530,
};

fn displays() -> macos::DisplayOutputs {
    macos::DisplayOutputs::new(
        [("center", CENTER), ("left", LEFT), ("right", RIGHT)]
            .into_iter()
            .map(|(id, frame)| macos::DisplayOutput {
                id: id.into(),
                name: id.into(),
                frame,
                is_main: id == "center",
            })
            .collect(),
    )
    .unwrap()
}

fn ws(n: u8) -> WorkspaceId {
    WorkspaceId::new(n).unwrap()
}

fn command<B: WorkspaceBackend>(rt: &mut WorkspaceRuntime<B>, action: WorkerAction) -> CycleStatus {
    let Some(WorkerResult::Status { result, .. }) = rt.execute(
        WorkerCommand::Action(action, displays()),
        &AtomicBool::new(false),
    ) else {
        panic!("missing status")
    };
    result.unwrap()
}

fn new_rig(
    layout: &str,
    gap: i32,
    side: u8,
    count: u32,
    minimum: impl Fn(u32) -> i32,
) -> WorkspaceRuntime<FakeBackend> {
    let output = match side {
        5 => LEFT,
        8 => RIGHT,
        _ => CENTER,
    };
    let initial = crate::Rect {
        x: output.x + 100,
        y: output.y + 100,
        width: 800,
        height: 400,
    };
    let windows: Vec<_> = (1..=count).map(|i| (window(i), initial)).collect();
    let backend = FakeBackend {
        observation: Some(observation_for_windows(
            "com.example.Test",
            &window(1),
            &windows,
        )),
        focused: Some(window(1)),
        frames: windows.iter().cloned().collect(),
        minimum_widths: windows
            .iter()
            .filter_map(|(w, _)| {
                let minimum = minimum(w.window_id);
                (minimum > 0).then(|| (w.clone(), minimum))
            })
            .collect(),
        ..FakeBackend::default()
    };
    let config = Config::parse(&format!(
        "version = 1\n[settings]\nlayout = '{layout}'\n[settings.gaps]\ninner = {gap}\n[[workspaces]]\nworkspace = 5\nmonitor = 'left'\n[[workspaces]]\nworkspace = 8\nmonitor = 'right'\n"
    )).unwrap();
    let mut runtime = runtime_with_config(backend, config);
    assert_eq!(command(&mut runtime, WorkerAction::Retile).failed, 0);
    for i in 1..=count {
        assert_eq!(runtime.workspaces.workspace_of(&window(i)), Some(ws(side)));
    }
    runtime
}

#[test]
fn research_resize_with_gap_clamps_to_50_point_window_frames() {
    let mut controls = Vec::new();
    for gap in [0, 10, 48] {
        for points in [1, 750, 16384] {
            let mut runtime = new_rig("columns", gap, 5, 2, |_| 0);
            let status = command(
                &mut runtime,
                WorkerAction::ResizeTo(crate::Axis::Horizontal, points),
            );
            let actual = (
                runtime.backend.frames[&window(1)].width,
                runtime.backend.frames[&window(2)].width,
            );
            println!(
                "gap={gap}, request={points}, applied={actual:?}, failed={}",
                status.failed
            );
            assert_eq!(status.failed, 0);
            controls.push((gap, points, actual));
        }
    }
    for (gap, points, (focused, sibling)) in controls {
        assert_eq!(
            focused,
            points.clamp(50, LEFT.width - gap - 50),
            "gap={gap}, request={points}"
        );
        assert_eq!(
            sibling,
            LEFT.width - gap - focused,
            "gap={gap}, request={points}"
        );
    }
}
