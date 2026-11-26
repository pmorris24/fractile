//! macOS Space slides: a parked window on another desktop keeps its recovery
//! frame (#328), Cmd+Tab out of a native fullscreen app lands on the target
//! workspace (#329), the outgoing desktop's tile keeps its size while the
//! slide runs (#334), and a swipe never tiles both desktops' windows
//! together once its slide settles (#431).

use super::*;

fn app_window(pid: i32, window_id: u32) -> crate::PhysicalWindowId {
    crate::PhysicalWindowId {
        app: AppInstanceId {
            pid,
            launched_at: 1,
        },
        window_id,
    }
}

/// One complete application per window; `focused` is its app's focus.
fn observe_apps(
    focused: Option<&crate::PhysicalWindowId>,
    windows: &[(crate::PhysicalWindowId, crate::Rect)],
) -> crate::NativeObservation {
    crate::NativeObservation {
        cycle: 0,
        mouse_down: false,
        applications: windows
            .iter()
            .map(|(window, frame)| crate::AppObservation::Complete {
                app: crate::ObservedApplication {
                    id: window.app.clone(),
                    bundle_id: format!("com.example.App{}", window.app.pid),
                },
                focused: (focused == Some(window)).then(|| window.clone()),
                windows: vec![crate::ObservedWindow {
                    id: window.clone(),
                    frame: *frame,
                    resizable: true,
                    title: None,
                    document: None,
                }],
            })
            .collect(),
    }
}

/// A Space change long enough ago that its slide has settled.
fn settled_space_change() -> Option<Instant> {
    Instant::now().checked_sub(Duration::from_secs(30))
}

fn main_workspace(runtime: &WorkspaceRuntime<FakeBackend>) -> Option<WorkspaceId> {
    runtime.workspaces.active_for(&DisplaySelector::Main)
}

fn applied_to(runtime: &WorkspaceRuntime<FakeBackend>, window: &crate::PhysicalWindowId) -> bool {
    runtime.backend.calls.iter().any(|call| {
        matches!(call, BackendCall::Apply(target, _) | BackendCall::SetPosition(target, _)
            if target == window)
    })
}

/// #328: Accessibility cannot reach a parked window on another macOS desktop
/// while the window server still lists it. Pause and quit keep its recovery
/// entry however long that lasts, and restore it once its desktop is shown;
/// a window that really closed still lets them finish.
#[test]
fn restore_keeps_recovery_for_a_parked_window_on_another_desktop() {
    let elsewhere = window(10);
    let closed = window(11);
    let normal = frame(50);
    let parked_at = main_parked_frame(normal);
    let mut backend = FakeBackend::default();
    // Listed by the window server, unreachable through Accessibility.
    backend.server_frames.insert(elsewhere.clone(), parked_at);
    let mut runtime = runtime_with_parked(
        backend,
        [(elsewhere.clone(), normal), (closed.clone(), frame(100))],
    );
    let two = WorkspaceId::new(2).unwrap();
    bind(&mut runtime, two, &elsewhere);
    bind(&mut runtime, two, &closed);

    assert_eq!(runtime.restore_all(), 1);
    assert_eq!(runtime.parked.get(&elsewhere), Some(&normal));
    assert!(!runtime.parked.contains_key(&closed));

    // Long past the Accessibility grace that retires unparked slots.
    if let Some(since) = runtime.inaccessible_since.get_mut(&elsewhere) {
        *since -= crate::app::parking::INACCESSIBLE_GRACE * 2;
    }
    assert_eq!(runtime.restore_all(), 1);
    assert_eq!(runtime.parked.get(&elsewhere), Some(&normal));
    assert_eq!(runtime.backend.persisted.get(&elsewhere), Some(&normal));

    // The user shows that desktop: Accessibility reaches the window again.
    runtime.backend.frames.insert(elsewhere.clone(), parked_at);
    assert_eq!(runtime.restore_all(), 0);
    assert!(runtime.parked.is_empty());
    assert_eq!(runtime.backend.frames[&elsewhere].x, normal.x);
    assert!(!runtime.inaccessible_since.contains_key(&elsewhere));
}

/// #334: activating a window on another desktop slides it in while the
/// outgoing desktop's tile is still on screen. Accessibility already reports
/// the arriving window, but laying both out together would shrink the
/// outgoing tile mid-slide. Layout waits for the slide to settle.
#[test]
fn a_space_slide_holds_layout_until_it_settles() {
    let outgoing = app_window(1, 10);
    let arriving = app_window(2, 20);
    let mut backend = FakeBackend {
        focused: Some(outgoing.clone()),
        frontmost: Some(outgoing.app.clone()),
        observation: Some(observe_apps(
            Some(&outgoing),
            &[(outgoing.clone(), frame(50))],
        )),
        ..FakeBackend::default()
    };
    backend.frames.insert(outgoing.clone(), frame(50));
    let mut runtime = runtime(backend);
    let cancelled = AtomicBool::new(false);
    runtime.cycle(outputs(), &cancelled).unwrap();
    assert_eq!(
        runtime.workspaces.workspace_of(&outgoing),
        Some(WorkspaceId::ONE)
    );
    let tiled = runtime.backend.frames[&outgoing];

    // The slide begins: the arriving window is reported alongside it.
    runtime.backend.space_changed_at = Some(Instant::now());
    runtime.backend.focused = Some(arriving.clone());
    runtime.backend.frontmost = Some(arriving.app.clone());
    runtime.backend.observation = Some(observe_apps(
        Some(&arriving),
        &[(outgoing.clone(), tiled), (arriving.clone(), frame(300))],
    ));
    runtime.backend.frames.insert(arriving.clone(), frame(300));
    runtime.backend.calls.clear();
    let held = runtime.cycle(outputs(), &cancelled).unwrap();

    assert!(held.close_recheck.is_some(), "no retile after the slide");
    assert!(
        !applied_to(&runtime, &outgoing),
        "outgoing tile resized mid-slide"
    );
    assert_eq!(runtime.backend.frames[&outgoing], tiled);
    assert_eq!(runtime.workspaces.workspace_of(&arriving), None);

    // Once it settles, the arriving window is adopted and tiled as before.
    runtime.backend.space_changed_at = settled_space_change();
    runtime.cycle(outputs(), &cancelled).unwrap();
    assert_eq!(
        runtime.workspaces.workspace_of(&arriving),
        Some(WorkspaceId::ONE)
    );
    assert!(applied_to(&runtime, &arriving));
}

/// A tiled Terminal on workspace 1 and TextEdit hidden on workspace 2.
fn fullscreen_cmd_tab_runtime() -> (
    WorkspaceRuntime<FakeBackend>,
    crate::PhysicalWindowId,
    crate::PhysicalWindowId,
) {
    let terminal = app_window(1, 10);
    let text_edit = app_window(2, 20);
    let normal = frame(100);
    let mut backend = FakeBackend {
        focused: Some(terminal.clone()),
        frontmost: Some(terminal.app.clone()),
        observation: Some(observe_apps(
            Some(&terminal),
            &[(terminal.clone(), frame(50)), (text_edit.clone(), normal)],
        )),
        ..FakeBackend::default()
    };
    backend.frames.insert(terminal.clone(), frame(50));
    backend
        .frames
        .insert(text_edit.clone(), main_parked_frame(normal));
    let mut runtime = runtime_with_parked(backend, [(text_edit.clone(), normal)]);
    bind(&mut runtime, WorkspaceId::ONE, &terminal);
    bind(&mut runtime, WorkspaceId::new(2).unwrap(), &text_edit);
    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();
    (runtime, terminal, text_edit)
}

/// #329: Cmd+Tab from a native fullscreen app to a window on a hidden
/// workspace slides macOS back to the desktop. Following focus mid-slide
/// could not lay the workspace out, rolled back, and heal then re-homed the
/// window onto the workspace on screen. Focus follow now waits for the slide.
#[test]
fn cmd_tab_out_of_native_fullscreen_lands_on_the_target_workspace() {
    let (mut runtime, _, text_edit) = fullscreen_cmd_tab_runtime();
    let two = WorkspaceId::new(2).unwrap();
    let cancelled = AtomicBool::new(false);

    runtime.backend.space_changed_at = Some(Instant::now());
    // Mid-slide, writes to the incoming desktop's windows fail.
    runtime.backend.position_outcome = Some(macos::ApplyOutcome::Unavailable);
    runtime.backend.focused = Some(text_edit.clone());
    runtime.backend.frontmost = Some(text_edit.app.clone());
    runtime.cycle(outputs(), &cancelled).unwrap();
    assert_eq!(main_workspace(&runtime), Some(WorkspaceId::ONE));
    assert_eq!(runtime.workspaces.workspace_of(&text_edit), Some(two));
    assert!(runtime.parked.contains_key(&text_edit));

    runtime.backend.space_changed_at = settled_space_change();
    runtime.backend.position_outcome = None;
    runtime.cycle(outputs(), &cancelled).unwrap();
    assert_eq!(main_workspace(&runtime), Some(two));
    assert_eq!(runtime.workspaces.workspace_of(&text_edit), Some(two));
    assert!(!runtime.parked.contains_key(&text_edit));
}

/// #329: when the slide's notification arrives only while the follow is
/// already restoring the window, the failed follow is retried once the slide
/// settles instead of staying suppressed (which left the window to heal).
#[test]
fn a_focus_follow_a_space_slide_interrupts_is_retried() {
    let (mut runtime, _, text_edit) = fullscreen_cmd_tab_runtime();
    let two = WorkspaceId::new(2).unwrap();
    let cancelled = AtomicBool::new(false);

    runtime.backend.space_change_on_position = true;
    runtime.backend.position_outcome = Some(macos::ApplyOutcome::Unavailable);
    runtime.backend.focused = Some(text_edit.clone());
    runtime.backend.frontmost = Some(text_edit.app.clone());
    let held = runtime.cycle(outputs(), &cancelled).unwrap();
    assert!(held.close_recheck.is_some(), "no retile after the slide");
    assert_eq!(runtime.workspaces.workspace_of(&text_edit), Some(two));
    assert!(runtime.parked.contains_key(&text_edit));
    assert_eq!(runtime.focus.guarded_window().cloned(), None);

    runtime.backend.space_changed_at = settled_space_change();
    runtime.backend.position_outcome = None;
    runtime.cycle(outputs(), &cancelled).unwrap();
    assert_eq!(main_workspace(&runtime), Some(two));
    assert_eq!(runtime.workspaces.workspace_of(&text_edit), Some(two));
}

/// Control for #329: with no Space change, a follow that fails keeps focus
/// suppressed, so it cannot loop.
#[test]
fn a_failed_focus_follow_without_a_space_change_stays_suppressed() {
    let (mut runtime, _, text_edit) = fullscreen_cmd_tab_runtime();
    let cancelled = AtomicBool::new(false);

    runtime.backend.position_outcome = Some(macos::ApplyOutcome::Unavailable);
    runtime.backend.focused = Some(text_edit.clone());
    runtime.backend.frontmost = Some(text_edit.app.clone());
    assert!(runtime.cycle(outputs(), &cancelled).is_err());
    assert_eq!(runtime.focus.guarded_window().cloned(), Some(text_edit));
}

/// #431: swiping between two macOS desktops with one tile each. The arriving
/// window was retired when its desktop was left, so after the slide it is not
/// a tile yet; the outgoing window's app has no window on this desktop and the
/// window server lists that window off screen. It still kept its slot until
/// the arriving window had been adopted beside it, so both were tiled at half
/// width for about a second.
#[test]
fn a_desktop_swipe_does_not_tile_both_desktops_windows_together() {
    let outgoing = app_window(1, 10);
    let arriving = app_window(2, 20);
    let mut backend = FakeBackend {
        focused: Some(outgoing.clone()),
        frontmost: Some(outgoing.app.clone()),
        observation: Some(observe_apps(
            Some(&outgoing),
            &[(outgoing.clone(), frame(50))],
        )),
        ..FakeBackend::default()
    };
    backend.frames.insert(outgoing.clone(), frame(50));
    let mut runtime = runtime(backend);
    let cancelled = AtomicBool::new(false);
    runtime.cycle(outputs(), &cancelled).unwrap();
    let full = runtime.backend.frames[&outgoing];

    // The slide has settled on the other desktop: only its window is
    // reported, and the outgoing one is ordered out but still listed.
    runtime.backend.space_changed_at = settled_space_change();
    runtime.backend.focused = Some(arriving.clone());
    runtime.backend.frontmost = Some(arriving.app.clone());
    runtime.backend.observation = Some(observe_apps(Some(&arriving), &[(arriving.clone(), full)]));
    runtime.backend.frames.insert(arriving.clone(), full);
    runtime.backend.ordered_out.insert(outgoing.window_id);
    runtime
        .backend
        .apps_confirming_no_windows
        .insert(outgoing.app.clone());
    runtime.cycle(outputs(), &cancelled).unwrap();

    assert_eq!(runtime.workspaces.workspace_of(&outgoing), None);
    assert_eq!(
        runtime.workspaces.workspace_of(&arriving),
        Some(WorkspaceId::ONE)
    );
    assert_eq!(runtime.backend.frames[&arriving].width, full.width);
    assert_eq!(runtime.backend.frames[&outgoing], full);
}

/// Two macOS desktops with one full-width window each (#431): `left` is tiled
/// on workspace 1 and `other` waits on the second desktop, at the frame it
/// had there.
fn two_desktops() -> (
    WorkspaceRuntime<FakeBackend>,
    crate::PhysicalWindowId,
    crate::PhysicalWindowId,
) {
    let left = app_window(1, 10);
    let other = app_window(2, 20);
    let mut backend = FakeBackend {
        focused: Some(left.clone()),
        frontmost: Some(left.app.clone()),
        observation: Some(observe_apps(Some(&left), &[(left.clone(), frame(50))])),
        ..FakeBackend::default()
    };
    backend.frames.insert(left.clone(), frame(50));
    let mut runtime = runtime(backend);
    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();
    let full = runtime.backend.frames[&left];
    // The other desktop's window was laid out full width when it was last shown.
    runtime.backend.frames.insert(other.clone(), full);
    runtime.backend.ordered_out.insert(other.window_id);
    (runtime, left, other)
}

/// A trackpad swipe from the desktop showing `leaving` to the one showing
/// `arriving`, past the slide's hold: macOS orders `leaving` out (the window
/// server still lists it) and Accessibility lists only the arriving desktop.
fn swipe(
    runtime: &mut WorkspaceRuntime<FakeBackend>,
    leaving: &crate::PhysicalWindowId,
    arriving: &crate::PhysicalWindowId,
) {
    let backend = &mut runtime.backend;
    backend.space_changed_at = Some(backend.now());
    backend.ordered_out.insert(leaving.window_id);
    backend.ordered_out.remove(&arriving.window_id);
    let at = backend.frames[arriving];
    backend.observation = Some(observe_apps(Some(arriving), &[(arriving.clone(), at)]));
    backend.focused = Some(arriving.clone());
    backend.frontmost = Some(arriving.app.clone());
    backend.advance(SPACE_TRANSITION_SETTLE + Duration::from_millis(5));
    backend.calls.clear();
}

/// Every frame Fractile wrote to `window`.
fn writes_to(
    runtime: &WorkspaceRuntime<FakeBackend>,
    window: &crate::PhysicalWindowId,
) -> Vec<crate::Rect> {
    runtime
        .backend
        .calls
        .iter()
        .filter_map(|call| match call {
            BackendCall::Apply(target, frame) if target == window => Some(*frame),
            _ => None,
        })
        .collect()
}

/// #431: right after a swipe settles, the desktop the slide left keeps no
/// slot beside the windows that arrived, so neither window is laid out at
/// half width, in either direction.
#[test]
fn swiping_between_desktops_never_tiles_both_desktops_together() {
    let (mut runtime, left, other) = two_desktops();
    let full = runtime.backend.frames[&left];
    let cancelled = AtomicBool::new(false);

    for (leaving, arriving) in [(&left, &other), (&other, &left), (&left, &other)] {
        swipe(&mut runtime, leaving, arriving);
        runtime.cycle(outputs(), &cancelled).unwrap();

        assert_eq!(
            runtime.workspaces.workspace_of(leaving),
            None,
            "{leaving:?} kept its slot"
        );
        assert_eq!(
            runtime.workspaces.workspace_of(arriving),
            Some(WorkspaceId::ONE)
        );
        assert_eq!(runtime.backend.frames[arriving], full);
        assert!(
            writes_to(&runtime, arriving)
                .iter()
                .all(|frame| *frame == full),
            "{arriving:?} laid out beside the other desktop's window: {:?}",
            writes_to(&runtime, arriving)
        );
        assert!(writes_to(&runtime, leaving).is_empty());
    }
}

/// #431 control: an app whose Accessibility snapshot timed out during the
/// swipe keeps its slot, as any unavailable app does.
#[test]
fn a_swipe_keeps_the_slot_of_an_app_whose_snapshot_timed_out() {
    let (mut runtime, left, other) = two_desktops();
    swipe(&mut runtime, &left, &other);
    runtime
        .backend
        .observation
        .as_mut()
        .unwrap()
        .applications
        .push(crate::AppObservation::Unavailable {
            app: left.app.clone(),
            error: "AXWindows timed out".into(),
        });
    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();
    assert_eq!(
        runtime.workspaces.workspace_of(&left),
        Some(WorkspaceId::ONE)
    );
}

/// #431 control: with no Space change, a bound window the window server lists
/// but orders out (a window its app closed and keeps alive) still settles
/// before its slot is given up.
#[test]
fn an_ordered_out_window_without_a_swipe_still_settles() {
    let (mut runtime, left, other) = two_desktops();
    swipe(&mut runtime, &left, &other);
    runtime.backend.space_changed_at = None;
    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();
    assert_eq!(
        runtime.workspaces.workspace_of(&left),
        Some(WorkspaceId::ONE)
    );
}

/// #431 control: once the slide is long past, the old settle applies again.
#[test]
fn a_long_settled_swipe_no_longer_releases_ordered_out_windows() {
    let (mut runtime, left, other) = two_desktops();
    swipe(&mut runtime, &left, &other);
    runtime.backend.space_changed_at = settled_space_change();
    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();
    assert_eq!(
        runtime.workspaces.workspace_of(&left),
        Some(WorkspaceId::ONE)
    );
}

/// #431, the order recorded on dev-132: both desktops' windows were reported
/// mid-swipe, before macOS posted the Space change, and tiled together. The
/// first cycle after the slide's hold releases the desktop it left at once
/// instead of after the omission settles.
#[test]
fn the_first_cycle_after_a_swipe_releases_a_window_adopted_mid_swipe() {
    let (mut runtime, left, other) = two_desktops();
    let full = runtime.backend.frames[&left];
    let cancelled = AtomicBool::new(false);
    runtime.backend.ordered_out.remove(&other.window_id);
    runtime.backend.observation = Some(observe_apps(
        Some(&left),
        &[(left.clone(), full), (other.clone(), full)],
    ));
    runtime.cycle(outputs(), &cancelled).unwrap();
    assert_eq!(
        runtime.workspaces.workspace_of(&other),
        Some(WorkspaceId::ONE)
    );
    assert!(runtime.backend.frames[&other].width < full.width);

    swipe(&mut runtime, &left, &other);
    runtime.cycle(outputs(), &cancelled).unwrap();
    assert_eq!(runtime.workspaces.workspace_of(&left), None);
    assert_eq!(runtime.backend.frames[&other], full);
}

/// A browser with `shown` tiled and focused on workspace 1 and `other`
/// parked on workspace 2. Leaving a video's native fullscreen in `shown`
/// slides back to the desktop, and macOS moves `other` back to its last
/// on-screen frame, which is its recovery frame (#489).
fn fullscreen_exit_runtime(
    after_slide: bool,
) -> (
    WorkspaceRuntime<FakeBackend>,
    crate::PhysicalWindowId,
    crate::PhysicalWindowId,
) {
    let shown = window(10);
    let other = window(11);
    let normal = frame(400);
    let mut backend = FakeBackend {
        observation: Some(observation_for_windows(
            "com.example.Browser",
            &shown,
            &[(shown.clone(), frame(50)), (other.clone(), normal)],
        )),
        focused: Some(shown.clone()),
        frontmost: Some(shown.app.clone()),
        ..FakeBackend::default()
    };
    backend.frames.insert(shown.clone(), frame(50));
    backend.frames.insert(other.clone(), normal);
    backend.server_frames.insert(other.clone(), normal);
    let mut runtime = runtime(backend);
    bind(&mut runtime, WorkspaceId::ONE, &shown);
    bind(&mut runtime, WorkspaceId::new(2).unwrap(), &other);
    runtime.parked.insert(other.clone(), normal, None);
    if after_slide {
        // The slide's hold ended moments ago.
        runtime.backend.space_changed_at = Some(runtime.backend.now());
        runtime
            .backend
            .advance(SPACE_TRANSITION_SETTLE + Duration::from_millis(800));
    }
    (runtime, shown, other)
}

/// #489: the other browser window stays on its workspace, keeps its recovery
/// frame, and is parked again instead of joining the workspace on screen.
#[test]
fn leaving_native_fullscreen_re_parks_the_apps_windows_from_other_workspaces() {
    let (mut runtime, _, other) = fullscreen_exit_runtime(true);
    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    assert_eq!(
        runtime.workspaces.workspace_of(&other),
        Some(WorkspaceId::new(2).unwrap())
    );
    assert_eq!(runtime.parked.get(&other), Some(&frame(400)));
    assert!(
        !outputs()
            .union()
            .contains_center(runtime.backend.frames[&other])
    );
}

/// #489 control: with no Space change, a parked window its frontmost app
/// brings back on screen was summoned and is still re-homed.
#[test]
fn a_frontmost_apps_window_back_on_screen_without_a_slide_is_still_re_homed() {
    let (mut runtime, _, other) = fullscreen_exit_runtime(false);
    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    assert_eq!(
        runtime.workspaces.workspace_of(&other),
        Some(WorkspaceId::ONE)
    );
    assert!(!runtime.parked.contains_key(&other));
}
