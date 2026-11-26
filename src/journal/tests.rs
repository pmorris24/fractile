use super::*;

fn window(id: u32, owner: Option<u8>, x: i32) -> ArrangedWindow {
    ArrangedWindow {
        window: id,
        pid: 7,
        bundle_id: "com.example.App".into(),
        frame: Some(Rect {
            x,
            y: 0,
            width: 100,
            height: 100,
        }),
        owner: owner.map(|owner| WorkspaceId::new(owner).unwrap()),
        role: if owner.is_some() {
            WindowRole::Tiled
        } else {
            WindowRole::Unowned
        },
        parked: false,
    }
}

fn anomaly(kind: AnomalyKind, id: u32) -> Anomaly {
    Anomaly {
        kind,
        window: id,
        bundle_id: "com.example.App".into(),
        workspace: None,
        frame: None,
        expected: None,
    }
}

#[test]
fn diff_reports_added_removed_and_material_changes_only() {
    let before = Arrangement {
        displays: vec![],
        active: vec![WorkspaceId::ONE],
        windows: vec![
            window(1, Some(1), 0),
            window(2, Some(1), 0),
            window(3, None, 0),
        ],
    };
    let after = Arrangement {
        displays: vec![],
        active: vec![WorkspaceId::ONE],
        windows: vec![
            window(1, Some(1), 3),
            window(2, Some(2), 0),
            window(4, None, 0),
        ],
    };
    let Some(JournalEvent::Diff {
        active, changes, ..
    }) = diff(&before, &after)
    else {
        panic!("expected a diff");
    };
    assert_eq!(active, None);
    let ids: Vec<_> = changes
        .iter()
        .map(|change| {
            (
                change.window,
                change.before.is_some(),
                change.after.is_some(),
            )
        })
        .collect();
    assert_eq!(
        ids,
        vec![(2, true, true), (3, true, false), (4, false, true)]
    );
    assert_eq!(diff(&after, &after), None);
}

#[test]
fn anomalies_are_debounced_and_report_their_duration() {
    let mut tracker = AnomalyTracker::default();
    let start = Instant::now();
    let leak = || vec![anomaly(AnomalyKind::Leaked, 9)];
    assert!(tracker.update(start, leak()).is_empty());
    // A transient that clears before settling is never journaled.
    let mut with_transient = leak();
    with_transient.push(anomaly(AnomalyKind::Missing, 3));
    assert!(
        tracker
            .update(start + Duration::from_secs(1), with_transient)
            .is_empty()
    );
    assert!(
        tracker
            .update(start + Duration::from_secs(2), leak())
            .is_empty()
    );
    let began = tracker.update(start + Duration::from_secs(4), leak());
    assert!(matches!(
        began.as_slice(),
        [JournalEvent::Anomaly {
            state: AnomalyState::Begin,
            ..
        }]
    ));
    assert!(
        tracker
            .update(start + Duration::from_secs(8), leak())
            .is_empty()
    );
    let ended = tracker.update(start + Duration::from_secs(10), Vec::new());
    assert!(matches!(
        ended.as_slice(),
        [JournalEvent::Anomaly {
            state: AnomalyState::End,
            duration_ms: Some(10_000),
            ..
        }]
    ));
}

#[test]
fn still_open_anomaly_keeps_the_time_before_it_was_journaled() {
    let mut tracker = AnomalyTracker::default();
    let start = Instant::now();
    let base = 100_000;
    let misplaced = || vec![anomaly(AnomalyKind::Misplaced, 10)];
    let mut lines = Vec::new();
    for second in 0..=3 {
        lines.extend(
            tracker
                .update(start + Duration::from_secs(second), misplaced())
                .into_iter()
                .map(|event| JournalLine {
                    t: base + second * 1_000,
                    event,
                }),
        );
    }
    assert_eq!(lines.len(), 1);

    // Four seconds in, the episode is still open and must already report its full length.
    let now = base + 4_000;
    let open = Summary::new(&lines, 0, now);
    let stats = &open.anomalies[&AnomalyKind::Misplaced];
    assert_eq!((stats.count, stats.open), (1, 1));
    assert_eq!(stats.longest_ms, 4_000);
    assert!(
        summarize(&lines, 0, now, 0).contains("longest      4s"),
        "{}",
        summarize(&lines, 0, now, 0)
    );

    // Ending it at that same instant gives the identical longest duration.
    lines.extend(
        tracker
            .update(start + Duration::from_secs(4), Vec::new())
            .into_iter()
            .map(|event| JournalLine { t: now, event }),
    );
    let closed = Summary::new(&lines, 0, now);
    let stats = &closed.anomalies[&AnomalyKind::Misplaced];
    assert_eq!((stats.count, stats.open), (1, 0));
    assert_eq!(stats.longest_ms, 4_000);
}

#[test]
fn journal_rotates_and_reads_both_generations() {
    let directory = std::env::temp_dir().join(format!(
        "fractile-journal-test-{}-{}",
        std::process::id(),
        unix_millis()
    ));
    let path = directory.join(JOURNAL_FILE);
    let rotated = directory.join(ROTATED_JOURNAL_FILE);
    let line = |t| JournalLine {
        t,
        event: JournalEvent::Stop,
    };
    append(&path, &rotated, &line(1)).unwrap();
    // Simulate an oversized live journal.
    let file = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap();
    file.set_len(MAX_JOURNAL_BYTES + 1).unwrap();
    append(&path, &rotated, &line(2)).unwrap();
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let (lines, skipped) = read(&[&rotated, &path]).unwrap();
    assert_eq!(
        lines.iter().map(|line| line.t).collect::<Vec<_>>(),
        vec![1, 2]
    );
    // The padding bytes form one unreadable line.
    assert_eq!(skipped, 1);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn summary_reconstructs_the_arrangement_and_counts_anomalies() {
    let base = 1_000_000;
    let lines = vec![
        JournalLine {
            t: base,
            event: JournalEvent::Start {
                version: "0.1.2".into(),
                pid: 1,
                restored_state: true,
                slots: 2,
                parked: 0,
                remembered_apps: 1,
            },
        },
        JournalLine {
            t: base + 10,
            event: JournalEvent::Snapshot {
                arrangement: Arrangement {
                    displays: vec![],
                    active: vec![WorkspaceId::ONE],
                    windows: vec![window(1, Some(1), 0)],
                },
            },
        },
        JournalLine {
            t: base + 20,
            event: diff(
                &Arrangement {
                    displays: vec![],
                    active: vec![WorkspaceId::ONE],
                    windows: vec![window(1, Some(1), 0)],
                },
                &Arrangement {
                    displays: vec![],
                    active: vec![WorkspaceId::ONE],
                    windows: vec![window(1, Some(1), 0), window(2, Some(3), 0)],
                },
            )
            .unwrap(),
        },
        JournalLine {
            t: base + 30,
            event: JournalEvent::Anomaly {
                state: AnomalyState::Begin,
                anomaly: anomaly(AnomalyKind::Leaked, 2),
                duration_ms: None,
            },
        },
        JournalLine {
            t: base + 5_030,
            event: JournalEvent::Anomaly {
                state: AnomalyState::End,
                anomaly: anomaly(AnomalyKind::Leaked, 2),
                duration_ms: Some(5_000),
            },
        },
        JournalLine {
            t: base + 6_000,
            event: JournalEvent::WrongPlacement {
                bundle_id: "com.example.App".into(),
                window: 2,
                source: PlacementSource::Active,
                placed: WorkspaceId::ONE,
                moved_to: WorkspaceId::new(3).unwrap(),
                after_ms: 4_000,
            },
        },
        JournalLine {
            t: base + 7_000,
            event: JournalEvent::MarkedWrong,
        },
    ];
    let summary = summarize(&lines, 0, base + 65_000, 0);
    assert!(
        summary.contains("Wrong placements (moved by hand within 120s of being placed): 1"),
        "{summary}"
    );
    assert!(
        summary.contains("1x opened on the active workspace: com.example.App (1)"),
        "{summary}"
    );
    assert!(
        summary.contains("Marked wrong by hand (fractile ctl wrong): 58s ago"),
        "{summary}"
    );
    assert!(summary.contains("Agent starts: 1"), "{summary}");
    assert!(summary.contains("leaked"), "{summary}");
    assert!(summary.contains("1x"), "{summary}");
    assert!(summary.contains("  1: App"), "{summary}");
    assert!(summary.contains("  3: App"), "{summary}");
    assert_eq!(
        summarize(&[], base + 100_000, base + 200_000, 0),
        "No journal events in this period."
    );
    let quiet = summarize(&lines, base + 100_000, base + 200_000, 0);
    assert!(
        quiet.starts_with("No journal events in this period."),
        "{quiet}"
    );
    assert!(
        quiet.contains("Last known arrangement: active 1"),
        "{quiet}"
    );
    assert!(quiet.contains("  1: App"), "{quiet}");
    assert!(quiet.contains("  3: App"), "{quiet}");
}

#[test]
fn lookback_with_no_events_keeps_last_known_arrangement() {
    let minute = 60_000;
    let now = 10_000_000;
    let display = Rect {
        x: 0,
        y: 0,
        width: 1200,
        height: 800,
    };
    let lines = vec![JournalLine {
        t: now - 90 * minute,
        event: JournalEvent::Snapshot {
            arrangement: Arrangement {
                displays: vec![display],
                active: vec![WorkspaceId::new(2).unwrap()],
                windows: vec![],
            },
        },
    }];

    let one_hour = summarize(&lines, now - 60 * minute, now, 0);
    assert!(
        one_hour.starts_with("No journal events in this period."),
        "{one_hour}"
    );
    assert!(
        one_hour.contains("Last known arrangement: active 2"),
        "{one_hour}"
    );

    let two_hours = summarize(&lines, now - 120 * minute, now, 0);
    assert!(
        two_hours.starts_with("1 events from 1h 30m ago to 1h 30m ago"),
        "{two_hours}"
    );
    assert!(
        two_hours.contains("Last known arrangement: active 2"),
        "{two_hours}"
    );
}

#[test]
fn anomaly_durations_are_independent_of_other_windows_event_order() {
    let minute = 60_000;
    let now = 10_000_000;
    let since = now - 60 * minute;
    let event = |window, offset_ms, state, duration_ms| JournalLine {
        t: now - offset_ms,
        event: JournalEvent::Anomaly {
            state,
            anomaly: anomaly(AnomalyKind::Misplaced, window),
            duration_ms,
        },
    };
    let build = |begin_b_offset_ms: u64| {
        let mut rows = vec![
            event(1, 62 * minute, AnomalyState::Begin, None),
            event(1, 59 * minute, AnomalyState::End, Some(183_000)),
            event(2, begin_b_offset_ms, AnomalyState::Begin, None),
            event(
                2,
                begin_b_offset_ms - minute,
                AnomalyState::End,
                Some(63_000),
            ),
        ];
        rows.sort_by_key(|line| line.t);
        rows
    };

    // Window 2 begins either before (59.5m ago) or after (58m ago) window 1's end (59m ago).
    let early = build(59 * minute + 30_000);
    let late = build(58 * minute);

    let early_summary = Summary::new(&early, since, now);
    let late_summary = Summary::new(&late, since, now);
    let early_kind = &early_summary.anomalies[&AnomalyKind::Misplaced];
    let late_kind = &late_summary.anomalies[&AnomalyKind::Misplaced];
    assert_eq!(early_kind.count, 1);
    assert_eq!(late_kind.count, 1);
    assert_eq!(early_kind.total_ms, 63_000);
    assert_eq!(late_kind.total_ms, 63_000);
    assert_eq!(early_kind.longest_ms, 63_000);
    assert_eq!(late_kind.longest_ms, 63_000);

    let early_report = summarize(&early, since, now, 0);
    let late_report = summarize(&late, since, now, 0);
    assert!(
        early_report.contains("misplaced         1x  total      1m  longest      1m"),
        "{early_report}"
    );
    assert!(
        late_report.contains("misplaced         1x  total      1m  longest      1m"),
        "{late_report}"
    );

    // All-time report includes both completed episodes.
    let all_time = Summary::new(&early, 0, now);
    let all_kind = &all_time.anomalies[&AnomalyKind::Misplaced];
    assert_eq!(all_kind.count, 2);
    assert_eq!(all_kind.total_ms, 246_000);
    assert_eq!(all_kind.longest_ms, 183_000);
}

#[test]
fn lookback_reconstructs_arrangement_from_snapshot_before_cutoff() {
    let minute = 60_000;
    let now = 10_000_000;
    let since = now - 60 * minute;
    let display = Rect {
        x: 0,
        y: 0,
        width: 1200,
        height: 800,
    };
    let initial = Arrangement {
        displays: vec![display],
        active: vec![WorkspaceId::ONE],
        windows: vec![window(1, Some(1), 0)],
    };
    let before_cutoff = Arrangement {
        displays: vec![display],
        active: vec![WorkspaceId::ONE],
        windows: vec![window(1, Some(1), 0), window(2, Some(3), 0)],
    };
    let after_cutoff = Arrangement {
        displays: vec![display],
        active: vec![WorkspaceId::new(2).unwrap()],
        windows: vec![window(1, Some(1), 0), window(2, Some(3), 0)],
    };
    let lines = vec![
        JournalLine {
            t: now - 75 * minute,
            event: JournalEvent::Snapshot {
                arrangement: initial.clone(),
            },
        },
        JournalLine {
            t: now - 70 * minute,
            event: diff(&initial, &before_cutoff).unwrap(),
        },
        JournalLine {
            t: now - 50 * minute,
            event: diff(&before_cutoff, &after_cutoff).unwrap(),
        },
    ];

    let lookback = summarize(&lines, since, now, 0);
    assert!(
        lookback.starts_with("1 events from 50m ago to 50m ago"),
        "{lookback}"
    );
    assert!(
        lookback.contains("Last known arrangement: active 2"),
        "{lookback}"
    );
    assert!(lookback.contains("  1: App"), "{lookback}");
    assert!(lookback.contains("  3: App"), "{lookback}");

    let all_time = summarize(&lines, 0, now, 0);
    assert!(
        all_time.starts_with("3 events from 1h 15m ago to 50m ago"),
        "{all_time}"
    );
    assert!(
        all_time.contains("Last known arrangement: active 2"),
        "{all_time}"
    );
}

#[test]
fn every_anomaly_kind_has_prose_meaning_distinct_from_its_label() {
    for kind in [
        AnomalyKind::Leaked,
        AnomalyKind::Stranded,
        AnomalyKind::ParkedVisible,
        AnomalyKind::Unadopted,
        AnomalyKind::ConstrainedOverlap,
        AnomalyKind::Misplaced,
        AnomalyKind::Missing,
        AnomalyKind::ParkingExempt,
    ] {
        assert_ne!(
            kind.meaning(),
            kind.label(),
            "anomaly {:?} meaning should explain the condition rather than repeating its label",
            kind
        );
        assert!(kind.meaning().contains(' '), "expected prose for {kind:?}");
    }
}
