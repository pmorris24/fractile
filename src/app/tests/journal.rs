use crate::journal::{AnomalyKind, JournalEvent, WindowRole};

use super::*;

fn anomaly_kinds(runtime: &mut WorkspaceRuntime<FakeBackend>) -> Vec<(AnomalyKind, u32)> {
    let outputs = outputs();
    let visible = runtime.visible_workspaces(&outputs);
    let server = runtime.backend.window_server_frames().unwrap();
    let windows = runtime.arranged_windows(&server);
    let mut kinds: Vec<_> = runtime
        .anomalies(&outputs, &visible, &windows, &server)
        .into_iter()
        .map(|anomaly| (anomaly.kind, anomaly.window))
        .collect();
    kinds.sort();
    kinds
}

#[test]
fn a_hidden_workspace_window_left_on_screen_is_a_leak() {
    let leaked = window(10);
    let mut backend = FakeBackend {
        observation: Some(observation(&leaked, frame(50))),
        ..FakeBackend::default()
    };
    backend.frames.insert(leaked.clone(), frame(50));
    let mut runtime = runtime(backend);
    bind(&mut runtime, WorkspaceId::new(2).unwrap(), &leaked);

    assert_eq!(anomaly_kinds(&mut runtime), vec![(AnomalyKind::Leaked, 10)]);

    // Once parked off screen, the model and the screen agree again.
    runtime
        .switch(
            WorkspaceId::new(2).unwrap(),
            outputs(),
            &AtomicBool::new(false),
        )
        .unwrap();
    runtime
        .switch(WorkspaceId::ONE, outputs(), &AtomicBool::new(false))
        .unwrap();
    assert!(runtime.parked.contains_key(&leaked));
    assert_eq!(anomaly_kinds(&mut runtime), Vec::new());
}

#[test]
fn owned_windows_the_window_server_lost_and_unadopted_windows_are_reported() {
    let ghost = window(10);
    let stray = window(11);
    let mut backend = FakeBackend {
        observation: Some(observation(&stray, frame(500))),
        ..FakeBackend::default()
    };
    backend.frames.insert(stray.clone(), frame(500));
    let mut runtime = runtime(backend);
    bind(&mut runtime, WorkspaceId::ONE, &ghost);
    // The stray window has been observed but not reconciled yet.
    runtime.journal.observation = runtime.backend.observation.clone();

    assert_eq!(
        anomaly_kinds(&mut runtime),
        vec![(AnomalyKind::Unadopted, 11), (AnomalyKind::Missing, 10)]
    );
}

#[test]
fn journal_writes_a_snapshot_then_only_diffs() {
    let tiled = window(10);
    let mut backend = FakeBackend {
        observation: Some(observation(&tiled, frame(50))),
        ..FakeBackend::default()
    };
    backend.frames.insert(tiled.clone(), frame(50));
    let mut runtime = runtime(backend);
    let cancelled = AtomicBool::new(false);
    runtime.cycle(outputs(), &cancelled).unwrap();
    runtime.check_arrangement(&outputs());
    let Some(JournalEvent::Snapshot { arrangement }) = runtime.backend.journal.last() else {
        panic!("expected a snapshot, got {:?}", runtime.backend.journal);
    };
    assert_eq!(
        arrangement.active,
        vec![WorkspaceId::ONE, WorkspaceId::FIVE]
    );
    assert_eq!(arrangement.windows.len(), 1);
    assert_eq!(arrangement.windows[0].owner, Some(WorkspaceId::ONE));
    assert_eq!(arrangement.windows[0].role, WindowRole::Tiled);
    assert_eq!(arrangement.windows[0].bundle_id, "com.example.Test");

    // Unchanged arrangements write nothing, even after the throttle.
    let written = runtime.backend.journal.len();
    runtime.journal.last_check = None;
    runtime.check_arrangement(&outputs());
    assert_eq!(runtime.backend.journal.len(), written);

    runtime
        .switch(WorkspaceId::new(2).unwrap(), outputs(), &cancelled)
        .unwrap();
    runtime.journal.last_check = None;
    runtime.check_arrangement(&outputs());
    let Some(JournalEvent::Diff {
        active, changes, ..
    }) = runtime.backend.journal.last()
    else {
        panic!("expected a diff, got {:?}", runtime.backend.journal.last());
    };
    assert_eq!(
        active.as_deref(),
        Some([WorkspaceId::new(2).unwrap(), WorkspaceId::FIVE].as_slice())
    );
    assert_eq!(changes.len(), 1);
    assert!(changes[0].after.as_ref().is_some_and(|after| after.parked));
}

#[test]
fn journal_lines_never_contain_window_titles_or_documents() {
    let tiled = window(10);
    let mut observed = observation(&tiled, frame(50));
    if let crate::AppObservation::Complete { windows, .. } = &mut observed.applications[0] {
        windows[0].title = Some("Private document".into());
        windows[0].document = Some("file:///Users/example/private/secret.rs".into());
    }
    let mut backend = FakeBackend {
        observation: Some(observed),
        ..FakeBackend::default()
    };
    backend.frames.insert(tiled.clone(), frame(50));
    let mut runtime = runtime(backend);
    runtime.journal_start(false);
    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();
    runtime.check_arrangement(&outputs());
    let mut lines = Vec::new();
    for event in &runtime.backend.journal {
        let json = serde_json::to_string(event).unwrap();
        assert!(!json.contains("Private document"), "{json}");
        assert!(!json.contains("title"), "{json}");
        assert!(!json.contains("secret.rs"), "{json}");
        assert!(!json.contains("document"), "{json}");
        lines.push(crate::journal::JournalLine {
            t: 1_000,
            event: event.clone(),
        });
    }
    let doctor = crate::journal::summarize(&lines, 0, 2_000, 0);
    assert!(!doctor.contains("Private document"), "{doctor}");
    assert!(!doctor.contains("secret.rs"), "{doctor}");
}

#[test]
fn constrained_tile_only_reports_overlap_with_a_visible_tiled_neighbor() {
    let first = window(10);
    let second = window(11);
    let mut runtime = runtime(FakeBackend::default());
    for id in [&first, &second] {
        bind(&mut runtime, WorkspaceId::ONE, id);
    }
    let left = crate::Rect {
        width: 530,
        height: 800,
        ..crate::Rect::default()
    };
    runtime.backend.frames.insert(first.clone(), left);
    let right = crate::Rect {
        x: 500,
        width: 500,
        ..left
    };
    runtime.backend.frames.insert(second.clone(), right);
    runtime.constrained[WorkspaceId::ONE.index()]
        .insert(first.clone(), (crate::Rect { width: 500, ..left }, left));
    assert!(
        anomaly_kinds(&mut runtime).contains(&(AnomalyKind::ConstrainedOverlap, first.window_id))
    );
    runtime
        .backend
        .frames
        .insert(first.clone(), crate::Rect { width: 490, ..left });
    assert!(
        !anomaly_kinds(&mut runtime)
            .iter()
            .any(|(kind, _)| *kind == AnomalyKind::ConstrainedOverlap)
    );
}

#[test]
fn visible_overflowing_window_whose_center_falls_in_display_gap_is_not_stranded() {
    let wide = window(10);
    let mut runtime = runtime(FakeBackend::default());
    bind(&mut runtime, WorkspaceId::ONE, &wide);
    let staggered = macos::DisplayOutputs::new(vec![
        macos::DisplayOutput {
            id: "main".into(),
            name: "Main".into(),
            frame: crate::Rect {
                x: 0,
                y: 0,
                width: 1000,
                height: 800,
            },
            is_main: true,
        },
        macos::DisplayOutput {
            id: "secondary".into(),
            name: "Secondary".into(),
            frame: crate::Rect {
                x: 1000,
                y: 900,
                width: 1000,
                height: 800,
            },
            is_main: false,
        },
    ])
    .unwrap();
    runtime.backend.frames.insert(
        wide.clone(),
        crate::Rect {
            x: 0,
            y: 0,
            width: 2200,
            height: 800,
        },
    );
    let visible = runtime.visible_workspaces(&staggered);
    let server = runtime.backend.window_server_frames().unwrap();
    let windows = runtime.arranged_windows(&server);
    let anomalies = runtime.anomalies(&staggered, &visible, &windows, &server);
    assert!(
        !anomalies.iter().any(
            |anomaly| anomaly.kind == AnomalyKind::Stranded && anomaly.window == wide.window_id
        ),
        "visible overflowing window should not be reported as stranded: {anomalies:?}"
    );
}
