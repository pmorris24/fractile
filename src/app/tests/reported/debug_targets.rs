//! #382: ctl debug and the terminal HUD print the tile the focus command goes to.
//! Reproduction contributed by @coygeek.

use super::super::*;

fn fixture(windows: &[(u32, WorkspaceId)]) -> WorkspaceRuntime<FakeBackend> {
    let mut backend = FakeBackend::default();
    for &(id, _) in windows {
        backend.frames.insert(window(id), frame(0));
    }
    let mut rt = runtime(backend);
    for &(id, workspace) in windows {
        bind(&mut rt, workspace, &window(id));
    }
    let displays = outputs();
    let cancelled = AtomicBool::new(false);
    rt.layout_workspace(WorkspaceId::ONE, displays.main().frame, &cancelled);
    rt.layout_workspace(WorkspaceId::FIVE, displays.displays()[1].frame, &cancelled);
    rt
}

fn compare(
    rt: &mut WorkspaceRuntime<FakeBackend>,
    case: &str,
    direction: Direction,
) -> (Option<u64>, Option<u32>) {
    let displays = outputs();
    let debug = serde_json::to_value(rt.debug_state(&displays)).unwrap();
    let name = direction.to_string().to_ascii_lowercase();
    let field = debug["targets"].get(&name).unwrap();
    let reported = field["window_id"].as_u64();
    let call_start = rt.backend.calls.len();
    let Some(WorkerResult::Status { result, .. }) = rt.execute(
        WorkerCommand::Action(WorkerAction::Focus(direction), displays),
        &AtomicBool::new(false),
    ) else {
        panic!("focus did not return a status");
    };
    result.unwrap();
    let requested = rt.backend.calls[call_start..]
        .iter()
        .find_map(|call| match call {
            BackendCall::Focus(window) => Some(window.window_id),
            _ => None,
        });
    assert_eq!(
        reported.map(|id| u32::try_from(id).unwrap()),
        requested,
        "{case} {name}: debug target and focus request disagree"
    );
    (reported, requested)
}

#[test]
fn debug_targets_match_the_focus_command() {
    let three = [
        (10, WorkspaceId::ONE),
        (11, WorkspaceId::ONE),
        (12, WorkspaceId::ONE),
    ];
    let expected = |case: &str, direction: Direction| match (case, direction) {
        ("split", Direction::Left) | ("fullscreen", Direction::Left | Direction::Up) => Some(10),
        ("split", Direction::Right) | ("fullscreen", Direction::Right | Direction::Down) => {
            Some(12)
        }
        ("split", _) => None,
        ("floating", Direction::Right) => Some(12),
        ("floating", _) => Some(10),
        _ => unreachable!(),
    };
    for case in ["split", "fullscreen", "floating"] {
        for direction in [
            Direction::Left,
            Direction::Down,
            Direction::Up,
            Direction::Right,
        ] {
            let mut rt = fixture(&three);
            rt.backend.focused = Some(window(11));
            if case == "fullscreen" {
                rt.workspaces
                    .set_deck(WorkspaceId::ONE, crate::workspace::DeckMode::Monocle);
                rt.layout_workspace(
                    WorkspaceId::ONE,
                    outputs().main().frame,
                    &AtomicBool::new(false),
                );
            }
            if case == "floating" {
                let floating = window(30);
                rt.backend.frames.insert(floating.clone(), frame(0));
                rt.workspaces
                    .own_floating(floating.clone(), WorkspaceId::ONE);
                rt.backend.focused = Some(floating);
            }
            let (_, requested) = compare(&mut rt, case, direction);
            assert_eq!(requested, expected(case, direction), "{case} {direction:?}");
        }
    }

    // An empty workspace on the focused display, a tile on the display to its right.
    let mut empty = fixture(&[(12, WorkspaceId::FIVE)]);
    empty.record_focused_workspace(WorkspaceId::ONE);
    assert_eq!(
        compare(&mut empty, "empty", Direction::Right),
        (Some(12), Some(12))
    );

    // A second press while Accessibility still reports the window the first left.
    let mut quick = fixture(&three);
    quick.backend.focused = Some(window(10));
    assert_eq!(
        compare(&mut quick, "first", Direction::Right),
        (Some(11), Some(11))
    );
    quick.backend.focused = Some(window(10));
    assert_eq!(
        compare(&mut quick, "pending", Direction::Right),
        (Some(12), Some(12))
    );
}
