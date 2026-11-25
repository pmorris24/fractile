//! #405: a workspace's floats return with it when its display reconnects.
//! Reproduction contributed by @coygeek.

use super::super::*;

fn command(
    runtime: &mut WorkspaceRuntime<FakeBackend>,
    action: WorkerAction,
    displays: &macos::DisplayOutputs,
) {
    let Some(WorkerResult::Status { result, .. }) = runtime.execute(
        WorkerCommand::Action(action, displays.clone()),
        &AtomicBool::new(false),
    ) else {
        panic!("missing worker status");
    };
    assert_eq!(result.unwrap().failed, 0);
}

#[test]
fn float_returns_to_its_reconnected_display() {
    let all = three_outputs();
    let outputs = all.displays();
    let side = outputs[1].frame;
    let reduced = macos::DisplayOutputs::new(vec![outputs[0].clone(), outputs[2].clone()]).unwrap();
    let config = Config::parse(
        "version = 1
[[workspaces]]
workspace = 5
monitor = 'secondary-id'
[[workspaces]]
workspace = 9
monitor = 'third-id'",
    )
    .unwrap();
    let five = WorkspaceId::FIVE;
    let floating = window(60);
    let tile = window(61);
    let first = frame(side.x + 100);
    let second = frame(side.x + 500);

    for unplug in [false, true] {
        let backend = FakeBackend {
            frames: HashMap::from([(floating.clone(), first), (tile.clone(), second)]),
            observation: Some(observation_for_windows(
                "com.example.Test",
                &floating,
                &[(floating.clone(), first), (tile.clone(), second)],
            )),
            focused: Some(floating.clone()),
            frontmost: Some(floating.app.clone()),
            ..FakeBackend::default()
        };
        let mut runtime = runtime_with_config(backend, config.clone());
        command(&mut runtime, WorkerAction::Retile, &all);
        command(&mut runtime, WorkerAction::ToggleFloat, &all);
        runtime.backend.frames.insert(floating.clone(), first);
        assert_eq!(
            runtime.workspaces.floating_workspace_of(&floating),
            Some(five)
        );
        assert_eq!(runtime.workspaces.workspace_of(&tile), Some(five));

        if unplug {
            runtime.backend.frames.insert(floating.clone(), frame(100));
            runtime.backend.frames.insert(tile.clone(), frame(500));
            command(&mut runtime, WorkerAction::Retile, &reduced);
            assert!(runtime.is_visible(five, &reduced));
        }

        for _ in 0..3 {
            command(&mut runtime, WorkerAction::Retile, &all);
        }
        let actual = runtime.backend.frames[&floating];
        let tile_frame = runtime.backend.frames[&tile];
        println!("unplug={unplug}, tile={tile_frame:?}, float={actual:?}");
        assert!(runtime.is_visible(five, &all));
        assert_eq!(
            runtime.workspaces.floating_workspace_of(&floating),
            Some(five)
        );
        assert_eq!(runtime.workspaces.workspace_of(&tile), Some(five));
        assert!(side.contains_center(tile_frame));
        assert!(side.contains_center(actual));
    }
}
