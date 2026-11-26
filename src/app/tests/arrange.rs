use crate::onboarding_plan::{ArrangedApp, ArrangedSlot, Arrangement};

use super::*;

const CHROME: &str = "com.google.Chrome";
const TERMINAL: &str = "com.apple.Terminal";
const STICKIES: &str = "com.apple.Stickies";
const TITLE: &str = "Quarterly numbers, do not share";

fn app_window(pid: i32, id: u32) -> crate::PhysicalWindowId {
    crate::PhysicalWindowId {
        app: AppInstanceId {
            pid,
            launched_at: 1,
        },
        window_id: id,
    }
}

/// An app as a test lists it: bundle ID, pid, and each window's ID and frame.
type OpenApp<'a> = (&'a str, i32, &'a [(u32, crate::Rect)]);

/// A desktop of several apps, every window titled, as the first-run tour finds it.
fn desktop(apps: &[OpenApp<'_>]) -> FakeBackend {
    let mut backend = FakeBackend::default();
    let mut applications = Vec::new();
    for (bundle_id, pid, windows) in apps {
        let mut observed = Vec::new();
        for (id, frame) in *windows {
            let window = app_window(*pid, *id);
            backend.frames.insert(window.clone(), *frame);
            observed.push(crate::ObservedWindow {
                id: window,
                frame: *frame,
                resizable: true,
                title: Some(TITLE.into()),
                document: None,
            });
        }
        applications.push(crate::AppObservation::Complete {
            app: crate::ObservedApplication {
                id: AppInstanceId {
                    pid: *pid,
                    launched_at: 1,
                },
                bundle_id: (*bundle_id).into(),
            },
            focused: None,
            windows: observed,
        });
    }
    backend.observation = Some(crate::NativeObservation {
        cycle: 0,
        mouse_down: false,
        applications,
    });
    backend
}

fn tiles(bundle_id: &str, workspaces: &[WorkspaceId]) -> ArrangedApp {
    ArrangedApp {
        bundle_id: bundle_id.into(),
        slots: workspaces
            .iter()
            .map(|workspace| ArrangedSlot::Tile(*workspace))
            .collect(),
    }
}

/// Four browser windows, a terminal and a sticky note, all on the main display.
fn first_run() -> (FakeBackend, Arrangement) {
    let backend = desktop(&[
        (
            CHROME,
            10,
            &[
                (1, frame(40)),
                (2, frame(80)),
                (3, frame(120)),
                (4, frame(160)),
            ],
        ),
        (TERMINAL, 11, &[(5, frame(200))]),
        (
            STICKIES,
            12,
            &[(
                6,
                crate::Rect {
                    x: 600,
                    y: 400,
                    width: 200,
                    height: 180,
                },
            )],
        ),
    ]);
    let arrangement = Arrangement {
        apps: vec![
            tiles(
                CHROME,
                &[
                    WorkspaceId::ONE,
                    WorkspaceId::ONE,
                    WorkspaceId::TWO,
                    WorkspaceId::TWO,
                ],
            ),
            tiles(TERMINAL, &[WorkspaceId::TWO]),
            ArrangedApp {
                bundle_id: STICKIES.into(),
                slots: vec![ArrangedSlot::Float],
            },
        ],
    };
    (backend, arrangement)
}

#[test]
fn an_arrangement_spreads_open_windows_over_workspaces_without_cramming_one() {
    let (backend, arrangement) = first_run();
    let mut runtime = runtime(backend);

    let status = runtime
        .arrange(&arrangement, outputs(), &AtomicBool::new(false))
        .unwrap();
    assert_eq!(status.failed, 0);

    for id in [1, 2] {
        assert_eq!(
            runtime.workspaces.workspace_of(&app_window(10, id)),
            Some(WorkspaceId::ONE)
        );
    }
    for window in [app_window(10, 3), app_window(10, 4), app_window(11, 5)] {
        assert_eq!(
            runtime.workspaces.workspace_of(&window),
            Some(WorkspaceId::TWO)
        );
        // Workspace 2 isn't on screen: its windows are parked, never laid out on workspace 1.
        assert!(runtime.parked.contains_key(&window));
        assert!(
            !runtime
                .backend
                .calls
                .iter()
                .any(|call| matches!(call, BackendCall::Apply(applied, _) if applied == &window))
        );
    }
    // Workspace 1 holds two tiles side by side on the main display.
    let left = runtime.backend.frames[&app_window(10, 1)];
    let right = runtime.backend.frames[&app_window(10, 2)];
    assert_eq!(left.width + right.width, 1000);
    assert_eq!((left.height, right.height), (800, 800));

    // The sticky note floats where it was, on the workspace it was showing on. Stickies
    // floats by default, so the default (not a hand float) keeps it floating.
    let note = app_window(12, 6);
    assert_eq!(runtime.workspaces.workspace_of(&note), None);
    assert_eq!(
        runtime.workspaces.floating_workspace_of(&note),
        Some(WorkspaceId::ONE)
    );
    assert!(!runtime.workspaces.is_manual_float(&note));
    assert_eq!(runtime.backend.frames[&note].width, 200);

    // The arrangement was for these windows once. A window opened later is adopted as usual.
    assert_eq!(runtime.arranged_target(&app_window(10, 3)), None);
}

#[test]
fn undoing_an_arrangement_puts_every_window_back_and_forgets_it() {
    let (backend, arrangement) = first_run();
    let originals = backend.frames.clone();
    let mut runtime = runtime(backend);
    runtime
        .arrange(&arrangement, outputs(), &AtomicBool::new(false))
        .unwrap();
    assert_ne!(runtime.backend.frames, originals);

    let result = runtime.execute(
        WorkerCommand::UndoArrangement(Some(outputs())),
        &AtomicBool::new(false),
    );
    assert!(matches!(result, Some(WorkerResult::Restored { failed: 0 })));

    assert_eq!(runtime.backend.frames, originals);
    assert!(runtime.parked.is_empty());
    assert!(runtime.backend.persisted.is_empty());
    assert!(
        !runtime
            .workspaces
            .floating_entries()
            .any(|(window, _)| runtime.workspaces.is_manual_float(window))
    );
    for window in originals.keys() {
        assert_eq!(runtime.workspaces.workspace_of_any(window), None);
    }
    for workspace in [WorkspaceId::ONE, WorkspaceId::TWO] {
        assert!(runtime.workspaces.windows(workspace).is_empty());
    }

    // A second undo has nothing left to put back.
    let again = runtime.execute(
        WorkerCommand::UndoArrangement(Some(outputs())),
        &AtomicBool::new(false),
    );
    assert!(matches!(again, Some(WorkerResult::Restored { failed: 0 })));
    assert_eq!(runtime.backend.frames, originals);
}

#[test]
fn arranging_again_moves_windows_a_workspace_already_owns() {
    let (backend, _) = first_run();
    let mut runtime = runtime(backend);
    // Tiling was already on: everything was adopted onto workspace 1, the note floating.
    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();
    let terminal = app_window(11, 5);
    let note = app_window(12, 6);
    assert_eq!(
        runtime.workspaces.workspace_of(&terminal),
        Some(WorkspaceId::ONE)
    );
    assert_eq!(
        runtime.workspaces.floating_workspace_of(&note),
        Some(WorkspaceId::ONE)
    );

    let arrangement = Arrangement {
        apps: vec![
            tiles(TERMINAL, &[WorkspaceId::THREE]),
            ArrangedApp {
                bundle_id: STICKIES.into(),
                slots: vec![ArrangedSlot::Float],
            },
        ],
    };
    runtime
        .arrange(&arrangement, outputs(), &AtomicBool::new(false))
        .unwrap();

    assert_eq!(
        runtime.workspaces.workspace_of(&terminal),
        Some(WorkspaceId::THREE)
    );
    assert!(runtime.parked.contains_key(&terminal));
    assert_eq!(
        runtime.workspaces.floating_workspace_of(&note),
        Some(WorkspaceId::ONE)
    );
    // An app the plan doesn't mention stays where it was.
    assert_eq!(
        runtime.workspaces.workspace_of(&app_window(10, 1)),
        Some(WorkspaceId::ONE)
    );
}

#[test]
fn an_arrangement_leaves_ignored_windows_and_unmanaged_workspaces_alone() {
    let (backend, _) = first_run();
    let config = Config::parse(&format!(
        "version = 1\n[[rules]]\nbundle_id = '{TERMINAL}'\naction = 'ignore'"
    ))
    .unwrap();
    let mut runtime = WorkspaceRuntime::with_backend_and_config(
        ManagementScope::Main,
        backend,
        HashMap::new(),
        config,
    );
    let arrangement = Arrangement {
        apps: vec![
            // Workspace 6 lives on the secondary display, which this run doesn't manage.
            tiles(CHROME, &[WorkspaceId::SIX]),
            tiles(TERMINAL, &[WorkspaceId::TWO]),
        ],
    };
    runtime
        .arrange(&arrangement, outputs(), &AtomicBool::new(false))
        .unwrap();

    assert_eq!(
        runtime.workspaces.workspace_of(&app_window(10, 1)),
        Some(WorkspaceId::ONE)
    );
    let terminal = app_window(11, 5);
    assert_eq!(runtime.workspaces.workspace_of_any(&terminal), None);
    assert_eq!(runtime.backend.frames[&terminal], frame(200));
}

#[test]
fn the_setup_inventory_carries_no_titles_and_counts_parked_windows() {
    let (backend, arrangement) = first_run();
    let mut runtime = runtime(backend);

    let fresh = runtime.inventory(&outputs());
    assert_eq!(
        fresh
            .apps
            .iter()
            .map(|app| (app.bundle_id.as_str(), app.windows.len()))
            .collect::<Vec<_>>(),
        vec![(CHROME, 4), (TERMINAL, 1), (STICKIES, 1)]
    );
    assert_eq!(fresh.lanes[0].width, 1000);
    assert_eq!(
        fresh.lanes[0].workspaces,
        (1..=4)
            .map(|number| WorkspaceId::new(number).unwrap())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        fresh.lanes.get(1).map(|lane| lane.workspaces.len()),
        Some(6)
    );
    let text = format!("{fresh:?}");
    assert!(!text.contains(TITLE) && !text.contains("title"));

    // Once windows are parked on a hidden workspace they still count as open.
    runtime
        .arrange(&arrangement, outputs(), &AtomicBool::new(false))
        .unwrap();
    let tiling = runtime.inventory(&outputs());
    let count = |bundle_id: &str| {
        tiling
            .apps
            .iter()
            .find(|app| app.bundle_id == bundle_id)
            .map_or(0, |app| app.windows.len())
    };
    assert_eq!((count(CHROME), count(TERMINAL), count(STICKIES)), (4, 1, 1));

    // Without a readable desktop the tour still gets its displays.
    runtime.backend.observation = None;
    let unreadable = runtime.inventory(&outputs());
    assert!(unreadable.apps.is_empty());
    assert_eq!(unreadable.lanes[0].workspaces.len(), 4);
}

#[test]
fn toggle_float_on_fractiles_own_window_does_nothing() {
    // The setup tour asks for the float shortcut while its own window has focus.
    let own = app_window(i32::try_from(std::process::id()).unwrap(), 1);
    let mut backend = FakeBackend {
        focused: Some(own.clone()),
        frontmost: Some(own.app.clone()),
        ..FakeBackend::default()
    };
    backend.frames.insert(own.clone(), frame(50));
    let mut runtime = runtime(backend);

    let status = runtime
        .toggle_float(outputs(), &AtomicBool::new(false))
        .unwrap();
    assert_eq!(status.failed, 0);
    assert_eq!(runtime.workspaces.workspace_of_any(&own), None);
    assert_eq!(runtime.backend.snapshot_count, 0);
}
