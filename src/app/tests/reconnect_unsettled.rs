//! #203: a tile whose display disconnects before its layout settles returns
//! to its tile when the display reconnects. Spaceman and macOS moved it, not
//! its app, so that position is no refusal of the tile.
use super::*;
use std::time::Duration;

#[derive(Clone, Copy, Default)]
struct Case {
    focus_secondary: bool,
    settle: bool,
    switch_away: bool,
}

/// Age every timer the runtime started by `by`, instead of sleeping.
fn age(rt: &mut WorkspaceRuntime<FakeBackend>, by: Duration) {
    let back = |at: &mut Instant| *at = at.checked_sub(by).unwrap_or(*at);
    for applied in &mut rt.applied {
        applied.values_mut().for_each(|frame| back(&mut frame.at));
    }
    rt.backend.advance(by);
}

fn tile_frame_after_reconnect(case: Case) -> crate::Rect {
    let window_of = |pid, window_id| crate::PhysicalWindowId {
        app: AppInstanceId {
            pid,
            launched_at: 1,
        },
        window_id,
    };
    let (one, five) = (window_of(1, 10), window_of(5, 50));
    let focused = if case.focus_secondary { &five } else { &one };
    let alone = macos::DisplayOutputs::new(vec![outputs().displays()[0].clone()]).unwrap();
    let mut backend = FakeBackend {
        focused: Some(focused.clone()),
        frontmost: Some(focused.app.clone()),
        ..FakeBackend::default()
    };
    backend.frames.insert(one.clone(), frame(100));
    backend.frames.insert(five.clone(), frame(1100));
    let mut rt = runtime(backend);
    let cancelled = AtomicBool::new(false);
    let retile = |rt: &mut WorkspaceRuntime<FakeBackend>, displays: macos::DisplayOutputs| {
        let mut all = observation_for_windows(
            "com.example.One",
            &one,
            &[(one.clone(), rt.backend.frames[&one])],
        );
        all.applications.extend(
            observation_for_windows(
                "com.example.Five",
                &five,
                &[(five.clone(), rt.backend.frames[&five])],
            )
            .applications,
        );
        rt.backend.observation = Some(all);
        rt.execute(
            WorkerCommand::Action(WorkerAction::Retile, displays),
            &cancelled,
        );
    };
    for _ in 0..3 {
        retile(&mut rt, outputs());
    }
    if case.settle {
        age(&mut rt, Duration::from_millis(600));
        retile(&mut rt, outputs());
    }
    // macOS moves the window onto main when Secondary disconnects.
    rt.backend.frames.insert(five.clone(), frame(200));
    for _ in 0..3 {
        retile(&mut rt, alone.clone());
    }
    age(&mut rt, Duration::from_millis(1300));
    for _ in 0..6 {
        retile(&mut rt, outputs());
    }
    if case.switch_away {
        for n in [7, 5] {
            let switch = WorkerAction::SwitchWorkspace(WorkspaceId::new(n).unwrap());
            rt.execute(WorkerCommand::Action(switch, outputs()), &cancelled);
        }
    }
    assert_eq!(rt.workspaces.workspace_of(&five), Some(WorkspaceId::FIVE));
    assert!(rt.is_visible(WorkspaceId::FIVE, &outputs()));
    rt.backend.frames[&five]
}

fn secondary_tile() -> crate::Rect {
    outputs().displays()[1].frame
}

#[test]
fn a_tile_returns_to_its_reconnected_display() {
    assert_eq!(
        tile_frame_after_reconnect(Case::default()),
        secondary_tile()
    );
}

#[test]
fn control_a_settled_layout_returns() {
    let case = Case {
        settle: true,
        ..Case::default()
    };
    assert_eq!(tile_frame_after_reconnect(case), secondary_tile());
}

#[test]
fn control_a_focused_tile_returns() {
    let case = Case {
        focus_secondary: true,
        ..Case::default()
    };
    assert_eq!(tile_frame_after_reconnect(case), secondary_tile());
}

#[test]
fn a_display_change_disarms_readback_but_keeps_a_refusing_app_frozen() {
    let huddle = window(11);
    let mut rt = runtime(FakeBackend::default());
    let target = outputs().main().frame;
    let limit = super::super::cycle::APPLY_ATTEMPT_LIMIT;
    let one = WorkspaceId::ONE.index();
    rt.note_outputs(&outputs());
    rt.applied[one].insert(
        huddle.clone(),
        super::super::cycle::AppliedFrame {
            target,
            at: rt.backend.now(),
        },
    );
    rt.constrained[one].insert(huddle.clone(), (target, frame(350)));
    rt.apply_attempts[one].insert(huddle.clone(), (target, limit));
    rt.note_outputs(&outputs());
    assert!(
        rt.applied[one].contains_key(&huddle),
        "unchanged displays keep readback armed"
    );
    let alone = macos::DisplayOutputs::new(vec![outputs().displays()[0].clone()]).unwrap();
    rt.note_outputs(&alone);
    assert!(rt.applied[one].is_empty());
    assert_eq!(
        rt.constrained[one].get(&huddle),
        Some(&(target, frame(350)))
    );
    assert_eq!(rt.apply_attempts[one].get(&huddle), Some(&(target, limit)));
}

#[test]
fn switching_away_and_back_returns_it() {
    let case = Case {
        switch_away: true,
        ..Case::default()
    };
    assert_eq!(tile_frame_after_reconnect(case), secondary_tile());
}
