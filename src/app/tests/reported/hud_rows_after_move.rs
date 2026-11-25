//! #419: the worker's HUD rows list a moved empty workspace on its new display only.
//! Reproduction contributed by @coygeek.

use super::super::*;

#[test]
fn native_hud_move_worker_rows_follow_the_empty_workspace() {
    let outputs = three_outputs();
    let config =
        Config::parse("version = 1\n[[workspaces]]\nworkspace = 8\nmonitor = 'third-id'\n")
            .unwrap();
    let mut rt = runtime_with_config(FakeBackend::default(), config);
    let cancelled = AtomicBool::new(false);
    let execute = |rt: &mut WorkspaceRuntime<FakeBackend>, action| {
        let Some(WorkerResult::Status { result, .. }) =
            rt.execute(WorkerCommand::Action(action, outputs.clone()), &cancelled)
        else {
            panic!("missing command status");
        };
        result.unwrap().hud.unwrap()
    };
    let ws = WorkspaceId::TWO;
    let before = execute(&mut rt, WorkerAction::SwitchWorkspace(ws));
    assert_eq!(before.display_of(ws).unwrap().id, "main-id");
    assert_eq!(
        before
            .displays
            .iter()
            .map(|row| row.visible)
            .collect::<Vec<_>>(),
        [Some(ws), Some(WorkspaceId::SIX), WorkspaceId::new(8).ok()]
    );
    let after = execute(
        &mut rt,
        WorkerAction::MoveWorkspaceToDisplay(DisplayTarget::Next),
    );
    assert_eq!(after.display_of(ws).unwrap().id, "secondary-id");
    assert_eq!(
        after
            .displays
            .iter()
            .map(|row| row.visible)
            .collect::<Vec<_>>(),
        [Some(WorkspaceId::ONE), Some(ws), WorkspaceId::new(8).ok()]
    );
    assert!(
        !after.displays[0]
            .workspaces
            .iter()
            .any(|entry| entry.workspace == ws)
    );
}
