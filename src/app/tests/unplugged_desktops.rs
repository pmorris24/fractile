//! Unplugging a display keeps each of its workspaces a desktop of its own: switching to one on
//! the screen that is left shows its windows in the order they were tiled, without the other
//! workspace's, and plugging the display back in returns it there.
use super::*;
use std::time::Duration;

/// Age every timer the runtime started by `by`, instead of sleeping.
fn age(rt: &mut WorkspaceRuntime<FakeBackend>, by: Duration) {
    let back = |at: &mut Instant| *at = at.checked_sub(by).unwrap_or(*at);
    for applied in &mut rt.applied {
        applied.values_mut().for_each(|frame| back(&mut frame.at));
    }
    rt.backend.advance(by);
}

fn on(display: &macos::DisplayOutputs, index: usize, rect: crate::Rect) -> bool {
    let screen = display.displays()[index].frame;
    rect.x >= screen.x && rect.x + rect.width <= screen.x + screen.width
}

#[test]
fn an_unplugged_displays_workspace_stays_its_own_desktop_and_returns() {
    let window_of = |pid, window_id| crate::PhysicalWindowId {
        app: AppInstanceId {
            pid,
            launched_at: 1,
        },
        window_id,
    };
    let one = window_of(1, 10);
    let (left, right) = (window_of(5, 50), window_of(5, 51));
    let alone = macos::DisplayOutputs::new(vec![outputs().displays()[0].clone()]).unwrap();
    let mut backend = FakeBackend {
        focused: Some(one.clone()),
        frontmost: Some(one.app.clone()),
        ..FakeBackend::default()
    };
    backend.frames.insert(one.clone(), frame(100));
    backend.frames.insert(left.clone(), frame(1100));
    backend.frames.insert(right.clone(), frame(1500));
    let mut rt = runtime(backend);
    let cancelled = AtomicBool::new(false);
    let run = |rt: &mut WorkspaceRuntime<FakeBackend>,
               action: WorkerAction,
               displays: &macos::DisplayOutputs| {
        let mut all = observation_for_windows(
            "com.example.One",
            &one,
            &[(one.clone(), rt.backend.frames[&one])],
        );
        all.applications.extend(
            observation_for_windows(
                "com.example.Five",
                &left,
                &[
                    (left.clone(), rt.backend.frames[&left]),
                    (right.clone(), rt.backend.frames[&right]),
                ],
            )
            .applications,
        );
        rt.backend.observation = Some(all);
        rt.execute(WorkerCommand::Action(action, displays.clone()), &cancelled);
    };
    for _ in 0..3 {
        run(&mut rt, WorkerAction::Retile, &outputs());
    }
    assert_eq!(rt.workspaces.workspace_of(&left), Some(WorkspaceId::FIVE));
    assert_eq!(rt.workspaces.workspace_of(&right), Some(WorkspaceId::FIVE));
    assert_eq!(rt.workspaces.workspace_of(&one), Some(WorkspaceId::ONE));
    let order = |rt: &WorkspaceRuntime<FakeBackend>| {
        rt.workspaces
            .engine(WorkspaceId::FIVE)
            .model()
            .workspace
            .slots
            .iter()
            .map(|slot| slot.bound_window().cloned())
            .collect::<Vec<_>>()
    };
    let before = order(&rt);
    assert!(rt.backend.frames[&left].x < rt.backend.frames[&right].x);

    // Unplug Secondary; macOS drops its windows onto Main.
    rt.backend.frames.insert(left.clone(), frame(150));
    rt.backend.frames.insert(right.clone(), frame(250));
    for _ in 0..3 {
        run(&mut rt, WorkerAction::Retile, &alone);
    }
    assert_eq!(rt.workspaces.workspace_of(&left), Some(WorkspaceId::FIVE));
    assert_eq!(rt.workspaces.workspace_of(&one), Some(WorkspaceId::ONE));
    assert!(rt.is_visible(WorkspaceId::ONE, &alone));
    assert!(!rt.is_visible(WorkspaceId::FIVE, &alone));

    // Workspace 5 is a desktop of its own on Main, tiled as it was.
    run(
        &mut rt,
        WorkerAction::SelectWorkspace(WorkspaceId::FIVE),
        &alone,
    );
    for _ in 0..2 {
        run(&mut rt, WorkerAction::Retile, &alone);
    }
    assert!(rt.is_visible(WorkspaceId::FIVE, &alone));
    assert!(!rt.is_visible(WorkspaceId::ONE, &alone));
    assert_eq!(order(&rt), before);
    let (l, r) = (rt.backend.frames[&left], rt.backend.frames[&right]);
    assert!(on(&alone, 0, l) && on(&alone, 0, r), "{l:?} {r:?}");
    assert!(l.x < r.x, "{l:?} {r:?}");

    // Plug Secondary back in: 5 returns there, and Main shows 1 again.
    run(
        &mut rt,
        WorkerAction::SelectWorkspace(WorkspaceId::ONE),
        &alone,
    );
    age(&mut rt, Duration::from_millis(1300));
    for _ in 0..6 {
        run(&mut rt, WorkerAction::Retile, &outputs());
    }
    assert!(rt.is_visible(WorkspaceId::ONE, &outputs()));
    assert!(rt.is_visible(WorkspaceId::FIVE, &outputs()));
    assert_eq!(order(&rt), before);
    let (l, r) = (rt.backend.frames[&left], rt.backend.frames[&right]);
    assert!(on(&outputs(), 1, l) && on(&outputs(), 1, r), "{l:?} {r:?}");
    assert!(l.x < r.x, "{l:?} {r:?}");
    assert!(on(&outputs(), 0, rt.backend.frames[&one]));
}
