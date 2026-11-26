//! #498: A new window matching a `route` rule stayed unowned when its center was in the display's
//! menu bar strip, while the same window a little lower routed to its hidden workspace.
//! Reported by @coygeek.

use super::super::*;

fn rect(x: i32, y: i32, width: i32, height: i32) -> crate::Rect {
    crate::Rect {
        x,
        y,
        width,
        height,
    }
}

fn display() -> macos::DisplayOutputs {
    macos::DisplayOutputs::with_bounds(
        vec![
            macos::DisplayOutput {
                id: "main-id".into(),
                name: "Main".into(),
                frame: rect(0, 30, 1000, 770),
                is_main: true,
            },
            macos::DisplayOutput {
                id: "secondary-id".into(),
                name: "Secondary".into(),
                frame: rect(1000, 0, 1000, 800),
                is_main: false,
            },
        ],
        vec![rect(0, 0, 1000, 800), rect(1000, 0, 1000, 800)],
    )
    .unwrap()
}

const RULE: &str =
    "version = 1\n[[rules]]\nbundle_id = 'com.example.Test'\naction = 'route'\nworkspace = 2";

#[test]
fn route_rule_owns_a_new_window_centered_in_the_menu_strip() {
    let target = WorkspaceId::new(2).unwrap();
    // Centers 8 points above and below the usable top.
    for y in [-10, 6] {
        let created = window(10);
        let frame = rect(200, y, 420, 64);
        let outputs = display();
        assert!(outputs.screens()[0].contains_center(frame));
        let backend = FakeBackend {
            observation: Some(observation(&created, frame)),
            frames: [(created.clone(), frame)].into(),
            ..FakeBackend::default()
        };
        let config = Config::parse(RULE).unwrap();
        let mut runtime = runtime_with_config(backend, config);

        runtime.cycle(outputs, &AtomicBool::new(false)).unwrap();

        assert_eq!(
            runtime.workspaces.workspace_of(&created),
            Some(target),
            "y={y}"
        );
        assert!(runtime.parked.contains_key(&created), "y={y}");
        assert!(
            runtime
                .workspaces
                .engine(WorkspaceId::ONE)
                .bound_windows()
                .is_empty(),
            "y={y}"
        );
    }
}
