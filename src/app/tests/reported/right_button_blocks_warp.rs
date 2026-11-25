//! #422: a focus command never warps the pointer while any mouse button is held.
//! Reported by @coygeek.

use super::super::*;

#[test]
fn no_pointer_warp_while_any_mouse_button_is_held() {
    let config =
        Config::parse("version = 1\n[settings]\nmouse_follows_focus = 'display'\n").unwrap();
    for held in [None, Some("left"), Some("right")] {
        let (main_tile, side_tile) = (window(10), window(60));
        let backend = FakeBackend {
            frames: HashMap::from([
                (main_tile.clone(), frame(50)),
                (side_tile.clone(), frame(1050)),
            ]),
            focused: Some(main_tile.clone()),
            frontmost: Some(main_tile.app.clone()),
            pointer: Some((200, 200)),
            mouse_down: held == Some("left"),
            other_button_down: held == Some("right"),
            ..FakeBackend::default()
        };
        let mut rt = runtime_with_config(backend, config.clone());
        bind(&mut rt, WorkspaceId::ONE, &main_tile);
        bind(&mut rt, WorkspaceId::FIVE, &side_tile);
        let Some(WorkerResult::Status { result, .. }) = rt.execute(
            WorkerCommand::Action(WorkerAction::Focus(Direction::Right), outputs()),
            &AtomicBool::new(false),
        ) else {
            panic!("missing worker status");
        };
        result.unwrap();
        assert_eq!(rt.backend.focused, Some(side_tile), "{held:?}");
        let warped = rt
            .backend
            .calls
            .iter()
            .any(|call| matches!(call, BackendCall::WarpPointer(_)));
        assert_eq!(warped, held.is_none(), "{held:?}");
    }
}
