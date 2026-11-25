//! #413: When several windows reveal successively smaller minimum widths, a window that first refuses its width on the last refit pass can overlap its neighbor across subsequent retiles.
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

fn new_rig_with_status(
    layout: &str,
    gap: i32,
    side: u8,
    count: u32,
    minimum: impl Fn(u32) -> i32,
) -> (WorkspaceRuntime<FakeBackend>, usize) {
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
    let failed = command(&mut runtime, WorkerAction::Retile).failed;
    for i in 1..=count {
        assert_eq!(runtime.workspaces.workspace_of(&window(i)), Some(ws(side)));
    }
    (runtime, failed)
}

#[test]
fn research_retile_learns_minimums_revealed_on_the_last_refit() {
    let mut overlap = 0;
    for fourth_minimum in [0, 137] {
        let minimums = [650, 235, 155, fourth_minimum, 0, 0];
        let mut runtime = new_rig_with_status("columns", 0, 5, 6, |id| minimums[id as usize - 1]);
        for pass in 0..4 {
            let failures = if pass == 0 {
                runtime.1
            } else {
                command(&mut runtime.0, WorkerAction::Retile).failed
            };
            let frames = [1, 2, 3, 4, 5, 6].map(|id| runtime.0.backend.frames[&window(id)]);
            let fourth_to_fifth = frames[4].x - (frames[3].x + frames[3].width);
            println!(
                "fourth_minimum={fourth_minimum}, pass={pass}, failures={failures}, widths={:?}, gap={fourth_to_fifth}",
                frames.map(|frame| frame.width)
            );
            if fourth_minimum == 0 {
                assert_eq!(fourth_to_fifth, 0);
            } else if pass == 3 {
                overlap = -fourth_to_fifth;
            }
        }
        if fourth_minimum > 0 {
            for _ in 0..2 {
                assert_eq!(
                    command(&mut runtime.0, WorkerAction::ToggleLayout).failed,
                    0
                );
            }
            let fourth = runtime.0.backend.frames[&window(4)];
            let fifth = runtime.0.backend.frames[&window(5)];
            println!(
                "gap after toggling the root axis twice: {}",
                fifth.x - (fourth.x + fourth.width)
            );
            assert_eq!(fourth.x + fourth.width, fifth.x);
        }
    }
    assert_eq!(
        overlap, 0,
        "a fourth minimum can fit but the overlap persists"
    );
}
