//! #424: deleting an unused named workspace stops it being a switch target.
//! Reproduction contributed by @coygeek.

use super::super::*;

fn execute(
    rt: &mut WorkspaceRuntime<FakeBackend>,
    action: WorkerAction,
    outputs: &macos::DisplayOutputs,
) -> Result<serde_json::Value, String> {
    let Some(WorkerResult::Status { result, debug }) = rt.execute(
        WorkerCommand::Action(action, outputs.clone()),
        &AtomicBool::new(false),
    ) else {
        panic!("missing worker status");
    };
    assert_eq!(result?.failed, 0);
    Ok(serde_json::to_value(debug).unwrap())
}

fn portrait_displays() -> macos::DisplayOutputs {
    let mut displays = three_outputs().displays().to_vec();
    for (display, (x, y, width, height)) in displays.iter_mut().zip([
        (0, 30, 2560, 1410),
        (-1440, -583, 1440, 2530),
        (2560, -539, 1440, 2530),
    ]) {
        display.frame = crate::Rect {
            x,
            y,
            width,
            height,
        };
    }
    let bounds = displays
        .iter()
        .map(|d| crate::Rect {
            y: d.frame.y - 30,
            height: d.frame.height + 30,
            ..d.frame
        })
        .collect();
    macos::DisplayOutputs::with_bounds(displays, bounds).unwrap()
}

fn configured_rig(
    config: Config,
    displays: &macos::DisplayOutputs,
    tiles: &[(u8, u32, usize)],
) -> WorkspaceRuntime<FakeBackend> {
    let mut applications = Vec::new();
    let mut backend = FakeBackend::default();
    for &(_, id, display) in tiles {
        let window = window(id);
        let output = displays.displays()[display].frame;
        let frame = crate::Rect {
            x: output.x + 50,
            y: output.y + 50,
            width: 400,
            height: 300,
        };
        backend.frames.insert(window.clone(), frame);
    }
    if !tiles.is_empty() {
        let app = crate::ObservedApplication {
            id: window(tiles[0].1).app,
            bundle_id: "com.example.Test".into(),
        };
        applications.push(crate::AppObservation::Complete {
            app,
            focused: Some(window(tiles[0].1)),
            windows: tiles
                .iter()
                .map(|&(_, id, _)| crate::ObservedWindow {
                    id: window(id),
                    frame: backend.frames[&window(id)],
                    title: None,
                    document: None,
                    resizable: true,
                })
                .collect(),
        });
        backend.focused = Some(window(tiles[0].1));
        backend.frontmost = Some(window(tiles[0].1).app);
    }
    backend.observation = Some(crate::NativeObservation {
        cycle: 0,
        applications,
        mouse_down: false,
    });
    let mut rt = runtime_with_config(backend, config);
    for &(n, id, _) in tiles {
        bind(&mut rt, WorkspaceId::new(n).unwrap(), &window(id));
    }
    execute(&mut rt, WorkerAction::Retile, displays).unwrap();
    rt
}

fn original_config() -> Config {
    Config::parse(
        "version = 1\n[[workspaces]]\nworkspace = 8\nmonitor = 'third-id'\n[[workspaces]]\nworkspace = 'Z'\nmonitor = 'third-id'\n",
    )
    .unwrap()
}

#[test]
fn config_research_removed_named() {
    let displays = portrait_displays();
    let removed =
        Config::parse("version = 1\n[[workspaces]]\nworkspace = 8\nmonitor = 'third-id'\n")
            .unwrap();
    let original = original_config();
    let tiles = [(1, 10, 0), (5, 60, 1), (8, 90, 2)];
    let z = WorkspaceId::named('Z').unwrap();
    let mut rt = configured_rig(original, &displays, &tiles);
    assert!(!rt.workspaces.state().named_workspaces_in_use().contains(&z));
    rt.replace_config(removed.clone());
    execute(&mut rt, WorkerAction::Retile, &displays).unwrap();
    let error = execute(&mut rt, WorkerAction::SwitchWorkspace(z), &displays).unwrap_err();
    assert!(error.contains("Workspace Z is not configured"), "{error}");
    let status = execute(&mut rt, WorkerAction::Retile, &displays).unwrap();
    assert_eq!(status["active_workspaces"], serde_json::json!([1, 5, 8]));
    assert!(!rt.parked.contains_key(&window(10)));
    let mut fresh = configured_rig(removed, &displays, &tiles);
    let error = execute(&mut fresh, WorkerAction::SwitchWorkspace(z), &displays).unwrap_err();
    assert!(error.contains("Workspace Z is not configured"));

    // Declared again, Z is a switch target again.
    rt.replace_config(original_config());
    execute(&mut rt, WorkerAction::SwitchWorkspace(z), &displays).unwrap();
}
