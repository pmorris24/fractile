use super::*;
use crate::app::detach_tab::TAB_DETACH_RECHECK;

const BUNDLE: &str = "com.example.Test";

fn offset_frame(x: i32, y: i32) -> crate::Rect {
    crate::Rect {
        x,
        y,
        width: 400,
        height: 300,
    }
}

/// One tiled window of `com.example.Test` on workspace 1, focused.
fn tabbed_runtime(
    detach: Option<FakeTabDetach>,
    config: Config,
) -> (WorkspaceRuntime<FakeBackend>, crate::PhysicalWindowId) {
    let source = window(1);
    let mut backend = FakeBackend {
        focused: Some(source.clone()),
        frontmost: Some(source.app.clone()),
        observation: Some(observation_for(BUNDLE, &source, frame(0))),
        tab_detach: detach,
        ..FakeBackend::default()
    };
    backend.frames.insert(source.clone(), frame(0));
    let mut rt = runtime_with_config(backend, config);
    bind(&mut rt, WorkspaceId::ONE, &source);
    rt.cycle(outputs(), &AtomicBool::new(false)).unwrap();
    rt.backend.calls.clear();
    (rt, source)
}

fn move_tab(
    rt: &mut WorkspaceRuntime<FakeBackend>,
    target: WorkspaceId,
    follow: bool,
) -> Result<CycleStatus, String> {
    match rt.execute(
        WorkerCommand::Action(WorkerAction::MoveTab { target, follow }, outputs()),
        &AtomicBool::new(false),
    ) {
        Some(WorkerResult::Status { result, .. }) => result,
        _ => panic!("an action returns a status"),
    }
}

fn applied(rt: &WorkspaceRuntime<FakeBackend>, window: &crate::PhysicalWindowId) -> bool {
    rt.backend
        .calls
        .iter()
        .any(|call| matches!(call, BackendCall::Apply(applied, _) if applied == window))
}

fn focus_calls(rt: &WorkspaceRuntime<FakeBackend>) -> Vec<crate::PhysicalWindowId> {
    rt.backend
        .calls
        .iter()
        .filter_map(|call| match call {
            BackendCall::Focus(window) => Some(window.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn detached_tab_goes_straight_to_a_hidden_workspace_and_focus_stays_on_the_source() {
    let tab = window(2);
    let (mut rt, source) = tabbed_runtime(
        Some(FakeTabDetach::NewWindow(
            tab.clone(),
            offset_frame(100, 100),
        )),
        Config::default(),
    );

    let status = move_tab(&mut rt, WorkspaceId::TWO, false).unwrap();

    assert!(
        rt.backend
            .calls
            .contains(&BackendCall::DetachTab(source.clone()))
    );
    assert_eq!(rt.workspaces.workspace_of(&tab), Some(WorkspaceId::TWO));
    assert!(
        rt.parked.contains_key(&tab),
        "the tab parks with workspace 2"
    );
    assert!(!applied(&rt, &tab), "the tab never tiles on workspace 1");
    assert_eq!(rt.workspaces.workspace_of(&source), Some(WorkspaceId::ONE));
    assert_eq!(
        rt.workspaces.windows(WorkspaceId::ONE),
        vec![source.clone()]
    );
    assert_eq!(focus_calls(&rt), vec![source.clone()]);
    assert_eq!(rt.backend.focused.as_ref(), Some(&source));
    assert_eq!(rt.focus.guarded_window(), Some(&tab));
    assert_eq!(
        rt.workspaces.active_for(&DisplaySelector::Main),
        Some(WorkspaceId::ONE)
    );
    assert!(rt.tab_detach.is_none(), "nothing left pending");
    assert!(status.hud.is_some());

    // Focus settles on the source: later cycles stay on workspace 1.
    rt.cycle(outputs(), &AtomicBool::new(false)).unwrap();
    assert_eq!(
        rt.workspaces.active_for(&DisplaySelector::Main),
        Some(WorkspaceId::ONE)
    );
    assert_eq!(rt.workspaces.workspace_of(&tab), Some(WorkspaceId::TWO));
}

#[test]
fn detached_tab_ignores_the_apps_route_rule() {
    let tab = window(2);
    let config = Config::parse(&format!(
        "version = 1\n[[rules]]\nbundle_id = '{BUNDLE}'\naction = 'route'\nworkspace = 3"
    ))
    .unwrap();
    let (mut rt, source) = tabbed_runtime(
        Some(FakeTabDetach::NewWindow(
            tab.clone(),
            offset_frame(100, 100),
        )),
        config,
    );

    move_tab(&mut rt, WorkspaceId::TWO, false).unwrap();

    assert_eq!(rt.workspaces.workspace_of(&tab), Some(WorkspaceId::TWO));
    assert!(rt.workspaces.windows(WorkspaceId::THREE).is_empty());
    assert_eq!(rt.workspaces.workspace_of(&source), Some(WorkspaceId::ONE));
}

#[test]
fn follow_switches_to_the_target_and_focuses_the_detached_tab() {
    let tab = window(2);
    let (mut rt, source) = tabbed_runtime(
        Some(FakeTabDetach::NewWindow(
            tab.clone(),
            offset_frame(100, 100),
        )),
        Config::default(),
    );

    move_tab(&mut rt, WorkspaceId::TWO, true).unwrap();

    assert_eq!(
        rt.workspaces.active_for(&DisplaySelector::Main),
        Some(WorkspaceId::TWO)
    );
    assert_eq!(rt.workspaces.workspace_of(&tab), Some(WorkspaceId::TWO));
    assert!(!rt.parked.contains_key(&tab));
    assert_eq!(rt.backend.focused.as_ref(), Some(&tab));
    assert!(rt.parked.contains_key(&source), "workspace 1 is hidden now");
    assert!(rt.tab_detach.is_none());
}

#[test]
fn detached_tab_tiles_on_a_target_visible_on_another_display() {
    let tab = window(2);
    let (mut rt, source) = tabbed_runtime(
        Some(FakeTabDetach::NewWindow(
            tab.clone(),
            offset_frame(100, 100),
        )),
        Config::default(),
    );

    move_tab(&mut rt, WorkspaceId::FIVE, false).unwrap();

    assert_eq!(rt.workspaces.workspace_of(&tab), Some(WorkspaceId::FIVE));
    assert!(!rt.parked.contains_key(&tab));
    let tab_frame = rt.backend.frames[&tab];
    assert!(
        tab_frame.x >= 1000,
        "tiled on the secondary display: {tab_frame:?}"
    );
    assert_eq!(focus_calls(&rt), vec![source.clone()]);
    assert_eq!(rt.focus.guarded_window().cloned(), None);
}

#[test]
fn appkit_tab_group_keeps_its_slot_for_the_next_tab_and_sends_the_moved_tab() {
    let next = window(2);
    let (mut rt, source) = tabbed_runtime(
        Some(FakeTabDetach::TabGroup {
            next: next.clone(),
            moved_to: offset_frame(300, 300),
        }),
        Config::default(),
    );
    let slot = rt
        .workspaces
        .engine(WorkspaceId::ONE)
        .slot_for_window(&source);

    move_tab(&mut rt, WorkspaceId::TWO, false).unwrap();

    assert_eq!(
        rt.workspaces
            .engine(WorkspaceId::ONE)
            .slot_for_window(&next),
        slot,
        "the group's next tab keeps the source slot"
    );
    assert_eq!(rt.workspaces.windows(WorkspaceId::ONE), vec![next.clone()]);
    assert_eq!(rt.workspaces.workspace_of(&source), Some(WorkspaceId::TWO));
    assert!(rt.parked.contains_key(&source));
    assert_eq!(focus_calls(&rt), vec![next.clone()]);
    assert!(rt.tab_detach.is_none());
}

#[test]
fn single_tab_window_moves_whole_like_move() {
    let (mut rt, source) = tabbed_runtime(None, Config::default());

    move_tab(&mut rt, WorkspaceId::TWO, false).unwrap();

    assert!(
        rt.backend
            .calls
            .contains(&BackendCall::DetachTab(source.clone()))
    );
    assert_eq!(rt.workspaces.workspace_of(&source), Some(WorkspaceId::TWO));
    assert!(rt.parked.contains_key(&source));
    assert!(rt.workspaces.windows(WorkspaceId::ONE).is_empty());
    assert!(rt.tab_detach.is_none());
}

#[test]
fn single_tab_window_with_follow_moves_and_switches() {
    let (mut rt, source) = tabbed_runtime(None, Config::default());

    move_tab(&mut rt, WorkspaceId::TWO, true).unwrap();

    assert_eq!(rt.workspaces.workspace_of(&source), Some(WorkspaceId::TWO));
    assert_eq!(
        rt.workspaces.active_for(&DisplaySelector::Main),
        Some(WorkspaceId::TWO)
    );
    assert!(!rt.parked.contains_key(&source));
}

#[test]
fn a_late_window_is_claimed_by_a_prompt_re_observation() {
    let tab = window(2);
    let (mut rt, source) = tabbed_runtime(Some(FakeTabDetach::Silent), Config::default());

    let status = move_tab(&mut rt, WorkspaceId::TWO, false).unwrap();

    assert!(rt.tab_detach.is_some(), "waiting for the window");
    let recheck = status.close_recheck.expect("a prompt re-observation");
    assert!(recheck <= rt.backend.now() + TAB_DETACH_RECHECK);

    rt.backend.open_window(&tab, offset_frame(100, 100));
    rt.backend.focused = Some(tab.clone());
    rt.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    assert_eq!(rt.workspaces.workspace_of(&tab), Some(WorkspaceId::TWO));
    assert!(rt.parked.contains_key(&tab));
    assert_eq!(rt.backend.focused.as_ref(), Some(&source));
    assert!(rt.tab_detach.is_none());
}

#[test]
fn an_expired_detach_drops_cleanly_and_later_windows_open_normally() {
    let late = window(2);
    let (mut rt, source) = tabbed_runtime(Some(FakeTabDetach::Silent), Config::default());

    move_tab(&mut rt, WorkspaceId::TWO, false).unwrap();
    rt.tab_detach.as_mut().unwrap().expires_at = rt.backend.now();
    let status = rt.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    assert!(rt.tab_detach.is_none(), "dropped at expiry");
    assert_eq!(status.close_recheck, None);
    assert_eq!(rt.workspaces.workspace_of(&source), Some(WorkspaceId::ONE));

    rt.backend.open_window(&late, offset_frame(100, 100));
    rt.cycle(outputs(), &AtomicBool::new(false)).unwrap();
    assert_eq!(rt.workspaces.workspace_of(&late), Some(WorkspaceId::ONE));
    assert!(rt.workspaces.windows(WorkspaceId::TWO).is_empty());
}

#[test]
fn a_failed_press_reports_an_error_and_arms_nothing() {
    let (mut rt, source) = tabbed_runtime(Some(FakeTabDetach::Failed), Config::default());

    let Err(error) = move_tab(&mut rt, WorkspaceId::TWO, false) else {
        panic!("a failed press is an error");
    };

    assert!(error.contains("Move Tab to New Window failed"), "{error}");
    assert!(rt.tab_detach.is_none());
    assert_eq!(rt.workspaces.workspace_of(&source), Some(WorkspaceId::ONE));
}

#[test]
fn other_apps_new_windows_are_not_claimed() {
    let tab = window(2);
    let other = crate::PhysicalWindowId {
        app: AppInstanceId {
            pid: 9,
            launched_at: 1,
        },
        window_id: 90,
    };
    let (mut rt, source) = tabbed_runtime(Some(FakeTabDetach::Silent), Config::default());
    move_tab(&mut rt, WorkspaceId::TWO, false).unwrap();
    if let Some(observation) = rt.backend.observation.as_mut() {
        observation
            .applications
            .push(crate::AppObservation::Complete {
                app: crate::ObservedApplication {
                    id: other.app.clone(),
                    bundle_id: "com.example.Other".into(),
                },
                focused: None,
                windows: Vec::new(),
            });
    }
    rt.backend.open_window(&other, offset_frame(500, 100));
    rt.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    assert_eq!(rt.workspaces.workspace_of(&other), Some(WorkspaceId::ONE));
    assert!(rt.tab_detach.is_some(), "still waiting for its own window");

    // Chromium focuses the tab's new window.
    rt.backend.open_window(&tab, offset_frame(100, 100));
    rt.backend.focused = Some(tab.clone());
    rt.cycle(outputs(), &AtomicBool::new(false)).unwrap();
    assert_eq!(rt.workspaces.workspace_of(&tab), Some(WorkspaceId::TWO));
    assert_eq!(rt.workspaces.workspace_of(&source), Some(WorkspaceId::ONE));
}
