//! #399: Option+Tab after rapid cross-display focus returns to the previous workspace.
//! Reproduction contributed by @coygeek.

use super::super::*;

fn ws(n: u8) -> WorkspaceId {
    WorkspaceId::new(n).unwrap()
}

fn win(n: u32) -> crate::PhysicalWindowId {
    let mut window = window(n);
    window.app.pid = n as i32;
    window
}

fn rect(x: i32, y: i32, width: i32, height: i32) -> crate::Rect {
    crate::Rect {
        x,
        y,
        width,
        height,
    }
}

fn trio(frames: [crate::Rect; 3]) -> macos::DisplayOutputs {
    let mut displays = three_outputs().displays().to_vec();
    for (display, frame) in displays.iter_mut().zip(frames) {
        display.frame = frame;
    }
    let bounds = frames.map(|frame| rect(frame.x, frame.y - 30, frame.width, frame.height + 30));
    macos::DisplayOutputs::with_bounds(displays, bounds.to_vec()).unwrap()
}

fn side_by_side() -> macos::DisplayOutputs {
    trio([
        rect(0, 30, 2560, 1410),
        rect(-1440, -583, 1440, 2530),
        rect(2560, -539, 1440, 2530),
    ])
}

fn execute(
    rt: &mut WorkspaceRuntime<FakeBackend>,
    displays: &macos::DisplayOutputs,
    action: WorkerAction,
) -> CycleStatus {
    let Some(WorkerResult::Status { result, .. }) = rt.execute(
        WorkerCommand::Action(action, displays.clone()),
        &AtomicBool::new(false),
    ) else {
        panic!("missing worker status");
    };
    let status = result.unwrap();
    assert_eq!(status.failed, 0);
    status
}

fn report(rt: &mut WorkspaceRuntime<FakeBackend>, id: u32) {
    rt.backend.focused = Some(win(id));
    rt.backend.frontmost = Some(win(id).app);
}

fn setup(
    displays: &macos::DisplayOutputs,
    settings: &str,
    windows: &[(u32, u8)],
    focus: u32,
) -> WorkspaceRuntime<FakeBackend> {
    let config = Config::parse(&format!(
        "version = 1\n[settings]\nlayout = 'columns'\n{settings}\n[[workspaces]]\nworkspace = 8\nmonitor = 'third-id'\n"
    ))
    .unwrap();
    let mut rt = runtime_with_config(FakeBackend::default(), config);
    let mut applications = Vec::new();
    for &(id, workspace) in windows {
        let window = win(id);
        let output = match workspace {
            6 => displays.displays()[1].frame,
            8 => displays.displays()[2].frame,
            _ => displays.main().frame,
        };
        let frame = rect(output.x + 20, output.y + 20, 400, 300);
        rt.backend.frames.insert(window.clone(), frame);
        bind(&mut rt, ws(workspace), &window);
        applications
            .extend(observation_for(&format!("com.example.Test{id}"), &window, frame).applications);
    }
    rt.backend.observation = Some(crate::NativeObservation {
        cycle: 0,
        mouse_down: false,
        applications,
    });
    report(&mut rt, focus);
    execute(&mut rt, displays, WorkerAction::Retile);
    rt
}

#[test]
fn back_and_forth_stale_report() {
    let displays = side_by_side();
    let mut result = Vec::new();
    for case in ["settled", "intermediate", "refresh"] {
        let mut rt = setup(&displays, "", &[(10, 1), (60, 6), (90, 8)], 60);
        execute(&mut rt, &displays, WorkerAction::Focus(Direction::Right));
        assert_eq!(rt.backend.focused, Some(win(10)));
        report(&mut rt, 60);
        execute(&mut rt, &displays, WorkerAction::Focus(Direction::Right));
        assert_eq!(rt.backend.focused, Some(win(90)));
        if case == "intermediate" {
            report(&mut rt, 10);
        } else if case == "refresh" {
            report(&mut rt, 60);
            execute(&mut rt, &displays, WorkerAction::Retile);
            report(&mut rt, 90);
        }
        assert!(rt.focus.in_flight(rt.backend.now()).is_some());
        execute(&mut rt, &displays, WorkerAction::WorkspaceBackAndForth);
        let focused = rt.backend.focused.as_ref().map(|window| window.window_id);
        println!("{case} focus after back-and-forth={focused:?}");
        result.push(focused);
    }
    assert_eq!(result, [Some(10), Some(10), Some(10)]);
}
