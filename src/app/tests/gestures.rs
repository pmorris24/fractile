use super::*;

#[test]
fn mouse_deferred_layout_does_not_roll_back_a_switch() {
    let outgoing = window(10);
    let incoming = window(11);
    let mut backend = FakeBackend {
        observation: Some(observation(&outgoing, frame(50))),
        managed_outcome: Some(macos::ApplyOutcome::Deferred),
        ..FakeBackend::default()
    };
    backend.frames.insert(outgoing.clone(), frame(50));
    backend.frames.insert(incoming.clone(), frame(60));
    let mut runtime = runtime(backend);
    bind(&mut runtime, WorkspaceId::ONE, &outgoing);
    let target = WorkspaceId::new(2).unwrap();
    bind(&mut runtime, target, &incoming);

    let result = runtime.switch(target, outputs(), &AtomicBool::new(false));

    assert!(result.is_ok());
    // The outgoing window stays parked; layout converges on a later cycle.
    assert!(runtime.parked.contains_key(&outgoing));
}

#[test]
fn dragging_to_a_tile_edge_previews_a_nested_insertion() {
    let moving = window(10);
    let target = window(20);
    let third = window(30);
    let mut backend = FakeBackend {
        focused: Some(moving.clone()),
        ..FakeBackend::default()
    };
    backend.frames.insert(moving.clone(), frame(50));
    backend.frames.insert(target.clone(), frame(400));
    backend.frames.insert(third.clone(), frame(750));
    let mut runtime = runtime(backend);
    bind(&mut runtime, WorkspaceId::ONE, &moving);
    bind(&mut runtime, WorkspaceId::ONE, &target);
    bind(&mut runtime, WorkspaceId::ONE, &third);

    let before = runtime.workspaces.engine(WorkspaceId::ONE).layout().clone();

    runtime
        .drop_window(
            moving.clone(),
            (500, 10),
            true,
            outputs(),
            &AtomicBool::new(false),
        )
        .unwrap();

    // Preview moves the siblings on screen without mutating the committed layout (#340).
    assert_eq!(
        runtime.workspaces.engine(WorkspaceId::ONE).layout(),
        &before
    );
    assert_eq!(
        runtime.backend.frames[&target],
        crate::Rect {
            x: 0,
            y: 400,
            width: 500,
            height: 400
        }
    );
    assert_eq!(
        runtime.backend.frames[&third],
        crate::Rect {
            x: 500,
            y: 0,
            width: 500,
            height: 800
        }
    );
    assert!(
        !runtime
            .backend
            .calls
            .iter()
            .any(|call| matches!(call, BackendCall::Apply(window, _) if window == &moving))
    );

    // Mouse-up at the same point commits the nested insertion.
    runtime
        .drop_window(
            moving.clone(),
            (500, 10),
            false,
            outputs(),
            &AtomicBool::new(false),
        )
        .unwrap();

    let placements = runtime
        .workspaces
        .engine(WorkspaceId::ONE)
        .placements(&Settings::for_output(outputs().main().frame));
    assert_eq!(
        placements[0].frame,
        crate::Rect {
            x: 0,
            y: 0,
            width: 500,
            height: 400
        }
    );
    assert_eq!(
        placements[1].frame,
        crate::Rect {
            x: 0,
            y: 400,
            width: 500,
            height: 400
        }
    );
    assert_eq!(
        placements[2].frame,
        crate::Rect {
            x: 500,
            y: 0,
            width: 500,
            height: 800
        }
    );
}

#[test]
fn dragging_a_tiled_window_over_its_sibling_live_previews_the_swap() {
    let first = window(10);
    let second = window(20);
    let mut backend = FakeBackend {
        focused: Some(first.clone()),
        ..FakeBackend::default()
    };
    backend.frames.insert(first.clone(), frame(50));
    backend.frames.insert(second.clone(), frame(500));
    let mut runtime = runtime(backend);
    bind(&mut runtime, WorkspaceId::ONE, &first);
    bind(&mut runtime, WorkspaceId::ONE, &second);

    runtime
        .drop_window(
            first.clone(),
            (750, 400),
            true,
            outputs(),
            &AtomicBool::new(false),
        )
        .unwrap();

    // Preview slides the sibling into the first slot without mutating the committed order (#340).
    assert_eq!(
        runtime.workspaces.engine(WorkspaceId::ONE).bound_windows(),
        vec![first.clone(), second.clone()]
    );
    assert!(runtime.backend.calls.contains(&BackendCall::Apply(
        second.clone(),
        crate::Rect {
            x: 0,
            y: 0,
            width: 500,
            height: 800,
        }
    )));
    assert!(
        !runtime
            .backend
            .calls
            .iter()
            .any(|call| matches!(call, BackendCall::Apply(window, _) if window == &first))
    );

    // Releasing over the sibling commits the swap.
    runtime
        .drop_window(
            first.clone(),
            (750, 400),
            false,
            outputs(),
            &AtomicBool::new(false),
        )
        .unwrap();
    assert_eq!(
        runtime.workspaces.engine(WorkspaceId::ONE).bound_windows(),
        vec![second, first]
    );
}

#[test]
fn dropping_a_tiled_window_on_another_display_moves_it() {
    let moving = window(10);
    let target = window(20);
    let mut backend = FakeBackend {
        focused: Some(moving.clone()),
        ..FakeBackend::default()
    };
    backend.frames.insert(moving.clone(), frame(50));
    backend.frames.insert(target.clone(), frame(1050));
    let mut runtime = runtime(backend);
    bind(&mut runtime, WorkspaceId::ONE, &moving);
    bind(&mut runtime, WorkspaceId::FIVE, &target);

    runtime
        .drop_window(
            moving.clone(),
            (1500, 400),
            false,
            outputs(),
            &AtomicBool::new(false),
        )
        .unwrap();

    assert_eq!(
        runtime.workspaces.workspace_of(&moving),
        Some(WorkspaceId::FIVE)
    );
}

#[test]
fn releasing_resize_or_drop_after_workspace_switch_keeps_outgoing_workspace_parked() {
    use crate::app::input::{GestureTracker, InputQueue, QueuedInput};

    let main_only = macos::DisplayOutputs::new(vec![outputs().main().clone()]).unwrap();
    let left = crate::Rect {
        width: 500,
        height: 800,
        ..crate::Rect::default()
    };
    let right = crate::Rect { x: 500, ..left };
    let incoming = [(window(30), left), (window(40), right)];
    let parked_incoming = incoming
        .clone()
        .map(|(w, original)| (w, crate::Rect { x: 999, ..original }));
    let make_runtime = || {
        let backend = FakeBackend {
            frames: [(window(10), left), (window(20), right)]
                .into_iter()
                .chain(parked_incoming.clone())
                .collect(),
            focused: Some(window(20)),
            persisted: incoming.iter().cloned().collect(),
            ..FakeBackend::default()
        };
        let mut rt = runtime_with_parked(backend, incoming.clone());
        bind(&mut rt, WorkspaceId::ONE, &window(10));
        bind(&mut rt, WorkspaceId::ONE, &window(20));
        bind(&mut rt, WorkspaceId::TWO, &window(30));
        bind(&mut rt, WorkspaceId::TWO, &window(40));
        rt
    };
    let widths = |rt: &WorkspaceRuntime<FakeBackend>, ws: WorkspaceId| -> Vec<i32> {
        rt.workspaces
            .engine(ws)
            .placements(&Settings::for_output(main_only.main().frame))
            .into_iter()
            .map(|placement| placement.frame.width)
            .collect()
    };
    let dispatch = |rt: &mut WorkspaceRuntime<FakeBackend>, input: QueuedInput| -> CycleStatus {
        let command = match input {
            QueuedInput::Action(action) => WorkerCommand::Action(action, main_only.clone()),
            QueuedInput::Gesture(gesture) => {
                WorkerCommand::Gesture(gesture, false, main_only.clone())
            }
        };
        let Some(WorkerResult::Status { result, .. }) =
            rt.execute(command, &AtomicBool::new(false))
        else {
            panic!("expected worker status");
        };
        result.unwrap()
    };

    // 1. Resize released after switching away must leave outgoing workspace parked and intact.
    let mut rt = make_runtime();
    let mut queue = InputQueue::default();
    let mut tracker = GestureTracker::default();
    tracker.begin(vec![(window(20), right)]);
    rt.backend.mouse_down = true;
    rt.backend.frames.insert(
        window(20),
        crate::Rect {
            x: 550,
            width: 450,
            ..right
        },
    );
    queue
        .push(QueuedInput::Action(WorkerAction::SwitchWorkspace(
            WorkspaceId::TWO,
        )))
        .unwrap();
    rt.backend.managed_outcome = Some(macos::ApplyOutcome::Deferred);
    let status = dispatch(&mut rt, queue.pop().unwrap());
    rt.backend.managed_outcome = None;
    assert_eq!(status.active, vec![WorkspaceId::TWO]);

    let parked_left = rt.backend.window_server_frame(&window(10)).unwrap();
    let parked_right = rt.backend.window_server_frame(&window(20)).unwrap();
    let saved_before = rt.backend.workspace_state.clone().unwrap();

    rt.backend.mouse_down = false;
    let release = tracker.finish(|_| Some(parked_right), None).unwrap();
    assert_eq!(release, FinalGesture::Resize(window(20), parked_right));
    let before_calls = rt.backend.calls.len();
    let status = dispatch(&mut rt, QueuedInput::Gesture(release));
    assert_eq!(status.active, vec![WorkspaceId::TWO]);
    assert!(
        rt.backend.calls[before_calls..].iter().all(
            |call| !matches!(call, BackendCall::Apply(w, _) if w == &window(10) || w == &window(20))
        ),
        "mouse-up wrote frames to hidden workspace tiles"
    );
    assert_eq!(
        rt.backend.window_server_frame(&window(10)),
        Some(parked_left)
    );
    assert_eq!(
        rt.backend.window_server_frame(&window(20)),
        Some(parked_right)
    );
    assert!(rt.parked.contains_key(&window(10)));
    assert!(rt.parked.contains_key(&window(20)));
    assert!(rt.backend.persisted.contains_key(&window(10)));
    assert!(rt.backend.persisted.contains_key(&window(20)));
    assert_eq!(widths(&rt, WorkspaceId::ONE), [500, 500]);
    assert_eq!(rt.backend.workspace_state.as_ref().unwrap(), &saved_before);

    // 2. Drop released after switching away is also ignored without disturbing hidden workspace.
    let before_drop = rt.backend.calls.len();
    dispatch(
        &mut rt,
        QueuedInput::Gesture(FinalGesture::drop(window(20), (250, 400))),
    );
    assert!(rt.backend.calls[before_drop..].iter().all(
        |call| !matches!(call, BackendCall::Apply(w, _) if w == &window(10) || w == &window(20))
    ));
    assert_eq!(
        rt.workspaces.workspace_of(&window(20)),
        Some(WorkspaceId::ONE)
    );
    assert!(rt.parked.contains_key(&window(10)));
    assert!(rt.parked.contains_key(&window(20)));

    // 3. Control: resizing on the visible workspace still updates the split normally.
    let mut rt = make_runtime();
    let resized = crate::Rect {
        x: 550,
        width: 450,
        ..right
    };
    dispatch(
        &mut rt,
        QueuedInput::Gesture(FinalGesture::Resize(window(20), resized)),
    );
    assert_eq!(widths(&rt, WorkspaceId::ONE), [550, 450]);
}
