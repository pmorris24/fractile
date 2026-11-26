use fractile::{
    WorkspaceId,
    journal::{Anomaly, AnomalyKind, AnomalyState, JournalEvent, JournalLine, PlacementSource},
};

use super::{DoctorReport, Tone};

#[test]
fn summarizes_empty_journal_without_warnings() {
    let report = DoctorReport::from_lines(&[], 100_000, 0);
    assert_eq!(report.total_events, 0);
    assert_eq!(report.anomalies, 0);
    assert_eq!(report.restore_failures, 0);
    assert!(report.rows.is_empty());
}

#[test]
fn aggregates_recalls_restores_wrong_placements_and_open_anomalies() {
    let now = 500_000;
    let lines = vec![
        JournalLine {
            t: 100_000,
            event: JournalEvent::Start {
                version: "0.1.6".into(),
                pid: 99,
                restored_state: true,
                slots: 4,
                parked: 2,
                remembered_apps: 5,
            },
        },
        JournalLine {
            t: 120_000,
            event: JournalEvent::Recall {
                bundle_id: "com.apple.Safari".into(),
                window: 10,
                workspace: WorkspaceId::ONE,
                floating: false,
            },
        },
        JournalLine {
            t: 150_000,
            event: JournalEvent::WrongPlacement {
                bundle_id: "com.apple.Terminal".into(),
                window: 12,
                source: PlacementSource::Active,
                placed: WorkspaceId::ONE,
                moved_to: WorkspaceId::THREE,
                after_ms: 4_000,
            },
        },
        JournalLine {
            t: 200_000,
            event: JournalEvent::Anomaly {
                state: AnomalyState::Begin,
                anomaly: Anomaly {
                    kind: AnomalyKind::Misplaced,
                    window: 12,
                    bundle_id: "com.apple.Terminal".into(),
                    workspace: Some(WorkspaceId::THREE),
                    frame: None,
                    expected: None,
                },
                duration_ms: None,
            },
        },
        JournalLine {
            t: 300_000,
            event: JournalEvent::Restore {
                reason: "pause".into(),
                requested: 2,
                failed: 0,
            },
        },
        JournalLine {
            t: 499_500,
            event: JournalEvent::MarkedWrong,
        },
    ];

    let report = DoctorReport::from_lines(&lines, now, 1);
    assert_eq!(report.total_events, 6);
    assert_eq!(report.skipped_lines, 1);
    assert_eq!(report.starts, 1);
    assert_eq!(report.recalled_windows, 1);
    assert_eq!(report.wrong_placements, 1);
    assert_eq!(report.anomalies, 1);
    assert_eq!(report.open_anomalies, 1);
    assert_eq!(report.top_anomaly, Some((AnomalyKind::Misplaced, 1)));
    assert_eq!(report.restores, 1);
    assert_eq!(report.restore_failures, 0);
    assert_eq!(report.last_snapshot.as_deref(), Some("just now"));
    assert_eq!(report.rows[0].title, "Snapshot saved");
    assert_eq!(report.rows[0].age, "just now");
    assert_eq!(report.rows[0].tone, Tone::Normal);
    assert_eq!(report.rows[2].title, "Misplaced: Terminal on Workspace 3");
}

#[test]
fn workspace_switch_diffs_stay_out_of_recent_activity() {
    let switch = |t| JournalLine {
        t,
        event: JournalEvent::Diff {
            active: Some(vec![WorkspaceId::TWO]),
            displays: None,
            changes: Vec::new(),
        },
    };
    let mut lines = vec![JournalLine {
        t: 1_000,
        event: JournalEvent::Stop,
    }];
    lines.extend((0..40).map(|i| switch(2_000 + i)));

    let report = DoctorReport::from_lines(&lines, 10_000, 0);
    assert_eq!(report.total_events, 41);
    assert_eq!(report.rows.len(), 1);
    assert_eq!(report.rows[0].title, "Agent stopped cleanly");
}
