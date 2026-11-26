//! Reopening an app whose windows lived on several workspaces: every window parked at quit is
//! rescued, and the arrangement is recalled however long the app was closed.

use crate::window_memory::{RememberedWindow, WindowMemory};

use super::*;

const SAFARI: &str = "com.apple.Safari";
const STICKIES: &str = "com.apple.Stickies";

fn workspace(value: u8) -> WorkspaceId {
    WorkspaceId::new(value).unwrap()
}

fn reopened(pid: i32, window_id: u32) -> crate::PhysicalWindowId {
    crate::PhysicalWindowId {
        app: AppInstanceId {
            pid,
            launched_at: 9,
        },
        window_id,
    }
}

fn remembered(bundle_id: &str, placements: &[(u8, crate::Rect, bool)]) -> WindowMemory {
    let mut memory = WindowMemory::default();
    assert!(
        memory.record(
            bundle_id,
            placements
                .iter()
                .map(|(value, frame, floating)| RememberedWindow {
                    workspace: workspace(*value),
                    frame: *frame,
                    floating: *floating,
                    document: None,
                    title: None,
                })
                .collect(),
            1,
        )
    );
    memory
}

/// A runtime long past its startup recall window, with no recent quit on record: the app was
/// closed minutes ago, not relaunched by an update.
fn reopened_minutes_later(
    bundle_id: &str,
    memory: WindowMemory,
    windows: &[(crate::PhysicalWindowId, crate::Rect)],
) -> WorkspaceRuntime<FakeBackend> {
    let mut runtime = runtime(FakeBackend {
        frames: windows.iter().cloned().collect(),
        observation: Some(observation_for_windows(bundle_id, &windows[0].0, windows)),
        ..FakeBackend::default()
    });
    runtime.restore_window_memory(memory);
    runtime.memory.started_at = Some(runtime.backend.now() - crate::app::memory::RECALL_WINDOW);
    runtime
}

fn recalls(runtime: &WorkspaceRuntime<FakeBackend>) -> Vec<(u32, WorkspaceId)> {
    runtime
        .backend
        .journal
        .iter()
        .filter_map(|event| match event {
            crate::journal::JournalEvent::Recall {
                window, workspace, ..
            } => Some((*window, *workspace)),
            _ => None,
        })
        .collect()
}

#[test]
fn every_window_reopened_at_a_parking_spot_is_rescued_and_recalled() {
    // Safari quit with windows on workspaces 1, 2 and 3 while 2 and 3 were parked; macOS
    // reopens those two at the parking spot, and Safari is not even frontmost.
    let tile = outputs().main().frame;
    let parked = main_parked_frame(tile);
    let (first, second, third) = (reopened(7, 30), reopened(7, 31), reopened(7, 32));
    let mut runtime = reopened_minutes_later(
        SAFARI,
        remembered(
            SAFARI,
            &[(1, tile, false), (2, tile, false), (3, tile, false)],
        ),
        &[
            (first.clone(), tile),
            (second.clone(), parked),
            (third.clone(), parked),
        ],
    );

    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    assert_eq!(
        runtime.workspaces.workspace_of(&first),
        Some(WorkspaceId::ONE)
    );
    assert_eq!(runtime.workspaces.workspace_of(&second), Some(workspace(2)));
    assert_eq!(runtime.workspaces.workspace_of(&third), Some(workspace(3)));
    assert!(!runtime.parked.contains_key(&first));
    assert!(runtime.parked.contains_key(&second));
    assert!(runtime.parked.contains_key(&third));
    assert_eq!(
        recalls(&runtime),
        vec![
            (30, WorkspaceId::ONE),
            (31, workspace(2)),
            (32, workspace(3))
        ]
    );
}

#[test]
fn a_window_parked_by_fractile_is_never_rescued() {
    let tile = outputs().main().frame;
    let owned = window(10);
    let mut runtime = runtime(FakeBackend {
        frames: HashMap::from([(owned.clone(), tile)]),
        observation: Some(observation(&owned, tile)),
        ..FakeBackend::default()
    });
    let cancelled = AtomicBool::new(false);
    runtime.cycle(outputs(), &cancelled).unwrap();
    runtime
        .switch(WorkspaceId::TWO, outputs(), &cancelled)
        .unwrap();
    let hidden = runtime.backend.frames[&owned];
    runtime.backend.calls.clear();

    runtime.cycle(outputs(), &cancelled).unwrap();

    assert!(runtime.parked.contains_key(&owned));
    assert_eq!(runtime.backend.frames[&owned], hidden);
    assert!(
        runtime.backend.calls.is_empty(),
        "{:?}",
        runtime.backend.calls
    );
}

#[test]
fn a_single_window_cold_launch_still_opens_on_the_active_workspace() {
    let tile = outputs().main().frame;
    let only = reopened(7, 30);
    let mut runtime = reopened_minutes_later(
        SAFARI,
        remembered(SAFARI, &[(2, tile, false), (3, tile, false)]),
        &[(only.clone(), tile)],
    );

    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    assert_eq!(
        runtime.workspaces.workspace_of(&only),
        Some(WorkspaceId::ONE)
    );
    assert!(recalls(&runtime).is_empty());
}

#[test]
fn reopened_notes_find_their_workspaces_by_size() {
    let sized = |x, width, height| crate::Rect {
        x,
        y: 100,
        width,
        height,
    };
    // Three notes of different sizes on workspaces 1, 2 and 3, reopened elsewhere and in a
    // different creation order.
    let memory = remembered(
        STICKIES,
        &[
            (1, sized(50, 220, 180), true),
            (2, sized(60, 300, 240), true),
            (3, sized(70, 180, 400), true),
        ],
    );
    let (tall, small, wide) = (reopened(8, 40), reopened(8, 41), reopened(8, 42));
    let mut runtime = reopened_minutes_later(
        STICKIES,
        memory,
        &[
            (tall.clone(), sized(100, 180, 400)),
            (small.clone(), sized(400, 220, 180)),
            (wide.clone(), sized(650, 300, 240)),
        ],
    );

    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    assert_eq!(
        runtime.workspaces.floating_workspace_of(&small),
        Some(WorkspaceId::ONE)
    );
    assert_eq!(
        runtime.workspaces.floating_workspace_of(&wide),
        Some(workspace(2))
    );
    assert_eq!(
        runtime.workspaces.floating_workspace_of(&tall),
        Some(workspace(3))
    );
    // A recalled float joins its workspace this cycle and parks with it on the next.
    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();
    assert!(!runtime.parked.contains_key(&small));
    assert!(runtime.parked.contains_key(&wide));
    assert!(runtime.parked.contains_key(&tall));
    let persisted = serde_json::to_string(&runtime.memory.memory).unwrap();
    assert!(!persisted.contains("title"), "{persisted}");
    assert!(!persisted.contains("document"), "{persisted}");
}

#[test]
fn same_size_reopened_windows_match_by_document_and_fuzzy_title_in_reverse_order() {
    let tile = outputs().main().frame;
    let mut memory = WindowMemory::default();
    assert!(memory.record(
        SAFARI,
        vec![
            RememberedWindow {
                workspace: workspace(2),
                frame: tile,
                floating: false,
                document: Some("file:///Users/example/code/fractile".into()),
                title: Some("fractile — Cargo.toml".into()),
            },
            RememberedWindow {
                workspace: workspace(3),
                frame: tile,
                floating: false,
                document: None,
                title: Some("Inbox (3) - Gmail".into()),
            },
        ],
        1,
    ));
    let (gmail, editor) = (reopened(9, 50), reopened(9, 51));
    let mut observation = observation_for_windows(
        SAFARI,
        &gmail,
        &[(gmail.clone(), tile), (editor.clone(), tile)],
    );
    if let crate::AppObservation::Complete { windows, .. } = &mut observation.applications[0] {
        windows[0].title = Some("Inbox (4) - Gmail".into());
        windows[1].document = Some("file:///Users/example/code/fractile".into());
        windows[1].title = Some("fractile — src/window_memory.rs".into());
    }
    let mut runtime = runtime(FakeBackend {
        frames: HashMap::from([(gmail.clone(), tile), (editor.clone(), tile)]),
        observation: Some(observation),
        ..FakeBackend::default()
    });
    runtime.restore_window_memory(memory);
    runtime.memory.started_at = Some(runtime.backend.now() - crate::app::memory::RECALL_WINDOW);

    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    assert_eq!(runtime.workspaces.workspace_of(&gmail), Some(workspace(3)));
    assert_eq!(runtime.workspaces.workspace_of(&editor), Some(workspace(2)));
    assert_eq!(
        recalls(&runtime),
        vec![(50, workspace(3)), (51, workspace(2))]
    );
    // Journal and doctor output must never leak remembered titles or documents.
    for event in &runtime.backend.journal {
        let json = serde_json::to_string(event).unwrap();
        assert!(!json.contains("Gmail"), "{json}");
        assert!(!json.contains("fractile —"), "{json}");
        assert!(!json.contains("file:///Users/example"), "{json}");
    }
}

fn position_writes(runtime: &WorkspaceRuntime<FakeBackend>, id: u32) -> usize {
    runtime
        .backend
        .calls
        .iter()
        .filter(
            |call| matches!(call, BackendCall::SetPosition(window, _) if window.window_id == id),
        )
        .count()
}

#[test]
fn a_window_at_a_parking_spot_that_cannot_move_backs_off_and_is_rescued_later() {
    // Each failed write can wait out the Accessibility timeout, so retrying it on every cycle
    // makes every retile slow (#481). A relaunching app may refuse the first writes, so the
    // window is still rescued once it accepts, with its frame unchanged.
    let tile = outputs().main().frame;
    let stuck = reopened(7, 30);
    let parked = main_parked_frame(tile);
    let other = window(10);
    let mut runtime = runtime(FakeBackend {
        frames: HashMap::from([(stuck.clone(), parked), (other.clone(), tile)]),
        observation: Some(observation(&other, tile)),
        refuses_position: Some(|_| true),
        ..FakeBackend::default()
    });
    let cancelled = AtomicBool::new(false);

    runtime.cycle(outputs(), &cancelled).unwrap();
    runtime.cycle(outputs(), &cancelled).unwrap();
    runtime.cycle(outputs(), &cancelled).unwrap();
    assert_eq!(position_writes(&runtime, 30), 1);

    // After the first delay it is tried once more, still refused; the next wait is longer.
    runtime
        .backend
        .advance(crate::app::stale_parked::RESCUE_RETRY_FIRST);
    runtime.cycle(outputs(), &cancelled).unwrap();
    assert_eq!(position_writes(&runtime, 30), 2);
    runtime
        .backend
        .advance(crate::app::stale_parked::RESCUE_RETRY_FIRST);
    runtime.cycle(outputs(), &cancelled).unwrap();
    assert_eq!(position_writes(&runtime, 30), 2);

    // The app now accepts the move: the next retry rescues the window on screen.
    runtime.backend.refuses_position = None;
    runtime
        .backend
        .advance(crate::app::stale_parked::RESCUE_RETRY_CAP);
    runtime.cycle(outputs(), &cancelled).unwrap();
    assert_eq!(position_writes(&runtime, 30), 3);
    let rescued = runtime.backend.frames[&stuck];
    assert!(tile.contains_center(rescued), "{rescued:?}");
}

#[test]
fn a_window_that_moves_after_a_failed_rescue_is_tried_again_at_once() {
    let tile = outputs().main().frame;
    let stuck = reopened(7, 30);
    let other = window(10);
    let mut runtime = runtime(FakeBackend {
        frames: HashMap::from([
            (stuck.clone(), main_parked_frame(tile)),
            (other.clone(), tile),
        ]),
        observation: Some(observation(&other, tile)),
        refuses_position: Some(|_| true),
        ..FakeBackend::default()
    });
    let cancelled = AtomicBool::new(false);
    runtime.cycle(outputs(), &cancelled).unwrap();
    assert_eq!(position_writes(&runtime, 30), 1);

    // Still at the spot, but another size.
    let resized = main_parked_frame(crate::Rect {
        width: tile.width / 2,
        ..tile
    });
    runtime.backend.frames.insert(stuck, resized);
    runtime.cycle(outputs(), &cancelled).unwrap();
    assert_eq!(position_writes(&runtime, 30), 2);
}

#[test]
fn a_window_one_pixel_off_its_parking_spot_is_still_rescued() {
    let tile = outputs().main().frame;
    let parked = main_parked_frame(tile);
    // One pixel toward the display, so two columns of it show.
    let toward = if parked.x < tile.x { 1 } else { -1 };
    let off = crate::Rect {
        x: parked.x + toward,
        ..parked
    };
    let stray = reopened(7, 30);
    let other = window(10);
    let mut runtime = runtime(FakeBackend {
        frames: HashMap::from([(stray.clone(), off), (other.clone(), tile)]),
        observation: Some(observation(&other, tile)),
        ..FakeBackend::default()
    });

    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    assert_eq!(position_writes(&runtime, 30), 1);
}

#[test]
fn one_pixel_and_empty_windows_at_a_parking_spot_are_never_rescued() {
    let dot = crate::Rect {
        width: 1,
        height: 1,
        ..crate::Rect::default()
    };
    let empty = crate::Rect {
        width: 0,
        height: 0,
        ..crate::Rect::default()
    };
    let tile = outputs().main().frame;
    let other = window(10);
    let mut runtime = runtime(FakeBackend {
        frames: HashMap::from([
            (reopened(7, 30), main_parked_frame(dot)),
            (reopened(7, 31), main_parked_frame(empty)),
            (other.clone(), tile),
        ]),
        observation: Some(observation(&other, tile)),
        ..FakeBackend::default()
    });

    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    assert_eq!(position_writes(&runtime, 30), 0);
    assert_eq!(position_writes(&runtime, 31), 0);
}
