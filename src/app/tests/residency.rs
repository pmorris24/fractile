//! Window residency regressions (#414, #415, #416): a window's workspace and the display it
//! shows on come from one residency, so marking, detaching and recalling never strand it.

use super::*;

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

fn monitors() -> macos::DisplayOutputs {
    let rows = [
        ("center", CENTER, true),
        ("left", LEFT, false),
        ("right", RIGHT, false),
    ];
    macos::DisplayOutputs::with_bounds(
        rows.iter()
            .map(|(id, frame, is_main)| macos::DisplayOutput {
                id: (*id).into(),
                name: (*id).into(),
                frame: *frame,
                is_main: *is_main,
            })
            .collect(),
        rows.into_iter()
            .map(|(_, frame, _)| crate::Rect {
                y: frame.y - 30,
                height: frame.height + 30,
                ..frame
            })
            .collect(),
    )
    .unwrap()
}

fn ws(number: u8) -> WorkspaceId {
    WorkspaceId::new(number).unwrap()
}

fn id(number: u32) -> crate::PhysicalWindowId {
    crate::PhysicalWindowId {
        app: AppInstanceId {
            pid: number as i32,
            launched_at: 1,
        },
        window_id: number,
    }
}

fn inset(output: crate::Rect) -> crate::Rect {
    crate::Rect {
        x: output.x + 100,
        y: output.y + 100,
        width: 400,
        height: 300,
    }
}

fn open(
    rt: &mut WorkspaceRuntime<FakeBackend>,
    window: &crate::PhysicalWindowId,
    frame: crate::Rect,
) {
    rt.backend.frames.insert(window.clone(), frame);
    rt.backend
        .observation
        .as_mut()
        .unwrap()
        .applications
        .extend(
            observation_for(
                &format!("com.example.Fixture{}", window.app.pid),
                window,
                frame,
            )
            .applications,
        );
}

fn act(rt: &mut WorkspaceRuntime<FakeBackend>, action: WorkerAction) {
    let Some(WorkerResult::Status { result, .. }) = rt.execute(
        WorkerCommand::Action(action, monitors()),
        &AtomicBool::new(false),
    ) else {
        panic!("expected command status");
    };
    assert_eq!(result.unwrap().failed, 0);
}

fn fixture(rules: &str, windows: &[(u32, crate::Rect)]) -> WorkspaceRuntime<FakeBackend> {
    let config = Config::parse(&format!(
        "version = 1\n[settings]\nlayout = 'columns'\nmouse_follows_focus = 'display'\n[labs]\nscratchpad = true\n[[workspaces]]\nworkspace = 8\nmonitor = 'right'\n[[workspaces]]\nworkspace = 9\nmonitor = 'right'\n{rules}"
    ))
    .unwrap();
    let mut rt = runtime_with_config(
        FakeBackend {
            observation: Some(crate::NativeObservation {
                cycle: 0,
                mouse_down: false,
                applications: Vec::new(),
            }),
            ..FakeBackend::default()
        },
        config,
    );
    for (number, output) in windows {
        open(&mut rt, &id(*number), inset(*output));
    }
    act(&mut rt, WorkerAction::Retile);
    rt
}

fn focus(rt: &mut WorkspaceRuntime<FakeBackend>, window: &crate::PhysicalWindowId) {
    rt.backend.focused = Some(window.clone());
    rt.backend.frontmost = Some(window.app.clone());
}

/// #414: Mark on a shown scratchpad window tiles it where it is shown, whatever its route says.
#[test]
fn scratchpad_mark_keeps_current_workspace() {
    use crate::scratchpad::ScratchpadCommand::{Mark, Toggle};
    for routed in [false, true] {
        let rules = if routed {
            "[[rules]]\nbundle_id = 'com.example.Fixture91'\naction = 'route'\nworkspace = 8\n"
        } else {
            ""
        };
        let mut rt = fixture(rules, &[(61, LEFT), (91, RIGHT)]);
        let marked = id(91);
        focus(&mut rt, &marked);
        act(&mut rt, WorkerAction::Scratchpad(Mark));
        focus(&mut rt, &id(61));
        act(&mut rt, WorkerAction::Scratchpad(Toggle));
        assert_eq!(rt.workspaces.floating_workspace_of(&marked), Some(ws(6)));
        assert_eq!(rt.backend.focused.as_ref(), Some(&marked));
        act(&mut rt, WorkerAction::Scratchpad(Mark));
        assert!(!rt.workspaces.scratchpad().contains(&marked));
        assert_eq!(
            rt.workspaces.workspace_of(&marked),
            Some(ws(6)),
            "route={routed}"
        );
        assert!(
            LEFT.contains_center(rt.backend.frames[&marked]),
            "route={routed}"
        );
    }
}

/// #415: a detached floating tab lands on the display of the visible workspace it was sent to.
#[test]
fn detached_floating_tab_reaches_visible_display() {
    for floating in [false, true] {
        let rules = if floating {
            "[[rules]]\nbundle_id = 'com.example.Fixture11'\naction = 'float'\n"
        } else {
            ""
        };
        let mut rt = fixture(rules, &[(11, CENTER), (61, LEFT), (91, RIGHT)]);
        let source = id(11);
        let tab = crate::PhysicalWindowId {
            app: source.app.clone(),
            window_id: 100,
        };
        focus(&mut rt, &source);
        rt.backend.tab_detach = Some(FakeTabDetach::NewWindow(tab.clone(), inset(CENTER)));
        act(
            &mut rt,
            WorkerAction::MoveTab {
                target: ws(8),
                follow: false,
            },
        );
        assert_eq!(rt.workspaces.workspace_of_any(&tab), Some(ws(8)));
        assert_eq!(
            rt.workspaces.floating_workspace_of(&tab),
            floating.then_some(ws(8))
        );
        assert_eq!(rt.backend.focused.as_ref(), Some(&source));
        let immediate = rt.backend.frames[&tab];
        act(&mut rt, WorkerAction::Retile);
        let settled = rt.backend.frames[&tab];
        assert!(
            RIGHT.contains_center(immediate) && RIGHT.contains_center(settled),
            "floating={floating} immediate={immediate:?} settled={settled:?}"
        );
    }
}

/// #416: a recalled float reopens on its remembered workspace's display.
#[test]
fn recalled_float_returns_to_remembered_display() {
    use crate::window_memory::{RememberedWindow, WindowMemory};
    for floating in [false, true] {
        let mut rt = fixture("", &[(61, LEFT), (91, RIGHT)]);
        let reopened = id(51);
        let mut memory = WindowMemory::default();
        assert!(memory.record(
            "com.example.Fixture51",
            vec![RememberedWindow {
                workspace: ws(8),
                frame: inset(RIGHT),
                floating,
                document: None,
                title: None,
            }],
            1
        ));
        rt.restore_window_memory(memory);
        open(&mut rt, &reopened, inset(CENTER));
        act(&mut rt, WorkerAction::Retile);
        act(&mut rt, WorkerAction::Retile);
        assert_eq!(rt.workspaces.workspace_of_any(&reopened), Some(ws(8)));
        assert_eq!(
            rt.workspaces.floating_workspace_of(&reopened),
            floating.then_some(ws(8))
        );
        let initial = rt.backend.frames[&reopened];
        act(&mut rt, WorkerAction::SwitchWorkspace(ws(9)));
        assert!(rt.parked.contains_key(&reopened));
        act(&mut rt, WorkerAction::SwitchWorkspace(ws(8)));
        assert!(!rt.parked.contains_key(&reopened));
        let restored = rt.backend.frames[&reopened];
        assert!(
            RIGHT.contains_center(initial) && RIGHT.contains_center(restored),
            "floating={floating} initial={initial:?} restored={restored:?}"
        );
    }
}
