//! #394: A newly observed window with its center in the physical Dock strip of the center display gets no workspace, even when Fractile observes its frame and workspace 1 is visible.
//! Reproduction contributed by @coygeek.

use super::super::*;

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

#[test]
fn new_window_in_internal_dock_strip() {
    let moved = window(10);
    let mut displays = trio().displays().to_vec();
    displays[0].frame.height -= 100;
    let outputs = macos::DisplayOutputs::with_bounds(displays, trio().screens()).unwrap();
    let mut found = vec![];
    for resizable in [true, false] {
        for y in [1180, 1320] {
            let frame = rect(300, y, 160, 120);
            assert!(outputs.screens()[0].contains_center(frame));
            assert!(outputs.union().contains_center(frame));
            let mut observation = observation(&moved, frame);
            if let crate::AppObservation::Complete { windows, .. } =
                &mut observation.applications[0]
            {
                windows[0].resizable = resizable;
            }
            let backend = FakeBackend {
                observation: Some(observation),
                frames: [(moved.clone(), frame)].into(),
                focused: Some(moved.clone()),
                frontmost: Some(moved.app.clone()),
                ..FakeBackend::default()
            };
            let mut rt = runtime(backend);
            rt.cycle(outputs.clone(), &AtomicBool::new(false)).unwrap();
            let owner = if resizable {
                rt.workspaces.workspace_of(&moved)
            } else {
                rt.workspaces.floating_workspace_of(&moved)
            };
            println!("resizable={resizable} y={y} owner={owner:?}");
            found.push(owner);
        }
    }
    assert_eq!(found, vec![Some(WorkspaceId::ONE); 4]);
}
