use std::thread;

use crate::{AppInstanceId, DisplaySelector, PhysicalWindowId, Rect};

use super::{
    DisplayOutput, DisplayOutputs, any_button_down,
    apply::{group_placements_by_app, position_matches},
    ax::{
        StackedWindow, WindowServerWindow, hover_target, should_ignore_missing_window_id,
        windows_reaching_point,
    },
    classify::{
        DialogHint, TitleBar, TitleButton, is_dialog, is_file_panel_identifier, is_ignored,
    },
    events::running_application_instances,
    frontmost_application, is_process_incarnation_running, process_start_time,
};

/// Title bars measured on macOS 27.2 with `AXFullScreenButton`, `AXZoomButton`
/// and `AXCloseButton`.
fn bar(fullscreen: TitleButton, zoom: TitleButton, close: bool) -> TitleBar {
    TitleBar {
        fullscreen,
        zoom,
        close,
    }
}

#[test]
fn primary_windows_without_a_working_fullscreen_button_still_tile() {
    use TitleButton::{Absent, Disabled, Enabled};
    let none = DialogHint::default();
    // FaceTime (16:10 landscape), Phone, News, Podcasts, Games, Shortcuts and
    // System Settings: no fullscreen button, an enabled zoom button.
    assert!(!is_dialog(false, false, bar(Absent, Enabled, true), none));
    // FaceTime (2:3 portrait), or any window while it shows a sheet: both
    // buttons disabled, but the fullscreen button is there.
    assert!(!is_dialog(
        false,
        false,
        bar(Disabled, Disabled, true),
        none
    ));
    // An ordinary document window.
    assert!(!is_dialog(false, false, bar(Enabled, Enabled, true), none));
    // Borderless regular-app windows (Ghostty without decorations).
    assert!(!is_dialog(false, false, bar(Absent, Absent, false), none));
    // A primary window that happens to expose a default Return button still tiles.
    let default_only = DialogHint {
        default_button: true,
        ..none
    };
    assert!(!is_dialog(
        false,
        false,
        bar(Enabled, Enabled, true),
        default_only
    ));
    assert!(!is_dialog(
        false,
        false,
        bar(Absent, Enabled, true),
        default_only
    ));
}

#[test]
fn settings_windows_modals_and_accessory_panels_still_float() {
    use TitleButton::{Absent, Disabled, Enabled};
    let none = DialogHint::default();
    // TextEdit Settings and Calculator: no fullscreen button, zoom disabled.
    assert!(is_dialog(false, false, bar(Absent, Disabled, true), none));
    // A closable window with neither button.
    assert!(is_dialog(false, false, bar(Absent, Absent, true), none));
    // Modal windows always float.
    assert!(is_dialog(false, true, bar(Enabled, Enabled, true), none));
    // Accessory apps' windows tile only with an enabled fullscreen button; an
    // enabled zoom button is not enough.
    assert!(is_dialog(true, false, bar(Absent, Enabled, true), none));
    assert!(is_dialog(true, false, bar(Disabled, Disabled, true), none));
    assert!(!is_dialog(true, false, bar(Enabled, Enabled, true), none));
    // Their buttonless windows are ignored outright.
    assert!(is_ignored(true, bar(Absent, Absent, false)));
    assert!(!is_ignored(true, bar(Absent, Absent, true)));
    assert!(!is_ignored(false, bar(Absent, Absent, false)));
}

#[test]
fn open_save_panels_and_button_bar_dialogs_float() {
    use TitleButton::{Absent, Enabled};
    let none = DialogHint::default();

    assert!(is_file_panel_identifier("open-panel"));
    assert!(is_file_panel_identifier("save-panel"));
    assert!(is_file_panel_identifier("OPEN-PANEL"));
    assert!(is_file_panel_identifier("NSOpenPanel"));
    assert!(is_file_panel_identifier("NSSavePanel"));
    assert!(!is_file_panel_identifier(""));
    assert!(!is_file_panel_identifier("main-window"));

    let buttonless = bar(Absent, Absent, false);
    let primary = bar(Enabled, Enabled, true);

    // Standalone NSOpenPanel / NSSavePanel (no traffic lights, non-modal):
    // floats on AXIdentifier, AXCancelButton, or buttonless AXDefaultButton.
    assert!(is_dialog(
        false,
        false,
        buttonless,
        DialogHint {
            file_panel: true,
            ..none
        }
    ));
    assert!(is_dialog(
        false,
        false,
        buttonless,
        DialogHint {
            cancel_button: true,
            ..none
        }
    ));
    assert!(is_dialog(
        false,
        false,
        buttonless,
        DialogHint {
            default_button: true,
            ..none
        }
    ));
    // Even if a file panel or cancel-button dialog exposes traffic lights, it floats.
    assert!(is_dialog(
        false,
        false,
        primary,
        DialogHint {
            file_panel: true,
            ..none
        }
    ));
    assert!(is_dialog(
        false,
        false,
        primary,
        DialogHint {
            cancel_button: true,
            ..none
        }
    ));
    assert!(is_dialog(
        true,
        false,
        primary,
        DialogHint {
            file_panel: true,
            ..none
        }
    ));
}

#[test]
fn processes_without_a_launch_date_are_identified_by_their_kernel_start_time() {
    let now = crate::config::now().as_millis() as u64;
    let started = process_start_time(std::process::id() as i32).unwrap();
    assert!(started <= now && now - started < 3_600_000);
    assert_eq!(process_start_time(std::process::id() as i32), Some(started));
    assert_eq!(process_start_time(0), None);
}

#[test]
fn pointer_hit_testing_uses_front_to_back_geometry_without_ax_focus() {
    let front = WindowServerWindow {
        pid: 1,
        id: 10,
        frame: Rect {
            x: -1000,
            y: -200,
            width: 400,
            height: 300,
        },
    };
    let behind = WindowServerWindow {
        pid: 2,
        id: 20,
        frame: Rect {
            width: 800,
            ..front.frame
        },
    };
    let windows = [front, behind];
    let ids = |point| {
        windows_reaching_point(&windows, point)
            .map(|window| window.id)
            .collect::<Vec<_>>()
    };
    assert_eq!(ids((-900, -100)), [10, 20]);
    assert_eq!(ids((-500, -100)), [20]);
    assert!(ids((100, 100)).is_empty());
}

#[test]
fn pointer_hit_testing_includes_windows_whose_resize_border_reaches_a_gap() {
    let left = WindowServerWindow {
        pid: 1,
        id: 10,
        frame: Rect {
            x: 0,
            y: 0,
            width: 500,
            height: 800,
        },
    };
    // An 8-point gap: the pointer at x 504 is over neither window.
    let right = WindowServerWindow {
        pid: 2,
        id: 20,
        frame: Rect {
            x: 508,
            ..left.frame
        },
    };
    let windows = [right, left];
    let ids = |point| {
        windows_reaching_point(&windows, point)
            .map(|window| window.id)
            .collect::<Vec<_>>()
    };
    assert_eq!(ids((504, 400)), [20, 10]);
    // Just past the left window's edge, inside the right one.
    assert_eq!(ids((509, 400)), [20, 10]);
    assert_eq!(ids((300, 400)), [10]);
    assert!(ids((540, 900)).is_empty());
}

#[test]
fn position_only_validation_ignores_window_size() {
    let target = Rect {
        x: 100,
        y: 200,
        width: 800,
        height: 600,
    };
    let differently_sized = Rect {
        width: 320,
        height: 240,
        ..target
    };

    assert!(position_matches(differently_sized, target));
}

#[test]
fn managed_layout_reads_each_app_window_list_once() {
    let app = AppInstanceId {
        pid: 1,
        launched_at: 1,
    };
    let windows: Vec<_> = (1..=256)
        .map(|window_id| PhysicalWindowId {
            app: app.clone(),
            window_id,
        })
        .collect();
    let placements: Vec<_> = windows
        .iter()
        .map(|window| (window, Rect::default()))
        .collect();

    let groups = group_placements_by_app(&placements);

    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].0, app);
    assert_eq!(groups[0].1.len(), 256);
}

#[test]
fn arbitrary_display_topology_resolves_preferences_and_fallbacks() {
    let outputs = DisplayOutputs::new(vec![
        DisplayOutput {
            id: "main-id".into(),
            name: "Built-in".into(),
            frame: Rect::default(),
            is_main: true,
        },
        DisplayOutput {
            id: "second-id".into(),
            name: "Studio Display".into(),
            frame: Rect {
                x: 100,
                ..Rect::default()
            },
            is_main: false,
        },
        DisplayOutput {
            id: "third-id".into(),
            name: "Projector".into(),
            frame: Rect {
                x: 200,
                ..Rect::default()
            },
            is_main: false,
        },
    ])
    .unwrap();

    assert_eq!(outputs.displays().len(), 3);
    assert_eq!(
        outputs
            .resolve(&DisplaySelector::Preferred("third-id".into()))
            .id,
        "third-id"
    );
    assert_eq!(
        outputs
            .resolve(&DisplaySelector::Preferred("missing".into()))
            .id,
        "main-id"
    );
}

fn current_phys_footprint() -> u64 {
    // SAFETY: `rusage_info_v4` is a plain C struct of integer counters valid when zero-initialized.
    let mut info = unsafe { std::mem::zeroed::<libc::rusage_info_v4>() };
    // SAFETY: `info` is writable stack storage matching `RUSAGE_INFO_V4`.
    let rc = unsafe {
        libc::proc_pid_rusage(
            libc::getpid(),
            libc::RUSAGE_INFO_V4,
            (&raw mut info).cast::<libc::rusage_info_t>(),
        )
    };
    assert_eq!(rc, 0, "proc_pid_rusage failed");
    info.ri_phys_footprint
}

#[test]
fn worker_thread_queries_drain_objective_c_autorelease_pool() {
    let (baseline, after) = thread::spawn(|| {
        for _ in 0..10 {
            let apps = running_application_instances();
            drop(frontmost_application());
            if let Some(first) = apps.first() {
                std::hint::black_box(is_process_incarnation_running(first));
            }
        }
        let baseline = current_phys_footprint();
        for _ in 0..400 {
            let apps = running_application_instances();
            drop(frontmost_application());
            for app in &apps {
                std::hint::black_box(is_process_incarnation_running(app));
            }
        }
        let after = current_phys_footprint();
        (baseline, after)
    })
    .join()
    .expect("worker thread panicked");

    let growth = after.saturating_sub(baseline);
    assert!(
        growth < 2 * 1024 * 1024,
        "worker thread leaked {growth} bytes of phys_footprint across 400 cycles (baseline={baseline}, after={after})"
    );
}

#[test]
fn window_event_monitor_wakes_parked_run_loop_on_refresh_and_drop() {
    let monitor = super::WindowEventMonitor::new();
    assert!(monitor.take_dirty());
    // Let the AX event thread park inside `CFRunLoop::run_in_mode` (24h timeout).
    thread::sleep(std::time::Duration::from_millis(25));
    monitor.refresh_applications();
    thread::sleep(std::time::Duration::from_millis(10));
    let stop_started = std::time::Instant::now();
    drop(monitor);
    assert!(
        stop_started.elapsed() < std::time::Duration::from_millis(500),
        "parked event monitor took {:?} to wake and stop",
        stop_started.elapsed()
    );
}

#[test]
fn missing_window_id_skips_non_standard_elements_like_finders_desktop() {
    // Finder reports the desktop as an AXScrollArea / AXDesktop entry without a CGWindowID.
    assert!(should_ignore_missing_window_id(
        Some("AXScrollArea"),
        Some("AXDesktop"),
    ));
    assert!(should_ignore_missing_window_id(Some("AXScrollArea"), None));
    assert!(should_ignore_missing_window_id(
        Some("AXWindow"),
        Some("AXDesktop"),
    ));
    // Standard windows or unreadable elements stay conservative (Unavailable).
    assert!(!should_ignore_missing_window_id(
        Some("AXWindow"),
        Some("AXStandardWindow"),
    ));
    assert!(!should_ignore_missing_window_id(Some("AXWindow"), None));
    assert!(!should_ignore_missing_window_id(None, None));
}

#[test]
fn any_held_mouse_button_counts_as_down() {
    use objc2_core_graphics::CGMouseButton;
    // #422: the right (or middle) button held blocks a pointer warp like the left.
    assert!(!any_button_down(|_| false));
    for held in [
        CGMouseButton::Left,
        CGMouseButton::Right,
        CGMouseButton::Center,
    ] {
        assert!(any_button_down(|button| button == held));
    }
}

#[test]
fn hover_focuses_only_an_ordinary_window_in_front_of_the_pointer() {
    let tile = StackedWindow {
        pid: 2,
        id: 20,
        layer: 0,
        frame: Rect {
            x: 0,
            y: 30,
            width: 800,
            height: 800,
        },
    };
    let menu_bar = StackedWindow {
        pid: 3,
        id: 30,
        layer: 24,
        frame: Rect {
            x: 0,
            y: 0,
            width: 1600,
            height: 30,
        },
    };
    let hud = StackedWindow {
        pid: 1,
        id: 10,
        layer: 0,
        frame: Rect {
            x: 300,
            y: 300,
            width: 200,
            height: 100,
        },
    };
    let own = 1;
    assert_eq!(
        hover_target(&[menu_bar, hud, tile], (100, 100), own),
        Some(20)
    );
    // The menu bar, and Fractile's own HUD or Settings, are not hovers.
    assert_eq!(hover_target(&[menu_bar, hud, tile], (100, 10), own), None);
    assert_eq!(hover_target(&[menu_bar, hud, tile], (350, 350), own), None);
    // Nothing under the pointer.
    assert_eq!(hover_target(&[menu_bar, tile], (1200, 400), own), None);
    // An open menu anywhere holds focus where it is.
    let menu = StackedWindow {
        pid: 3,
        id: 40,
        layer: 101,
        frame: Rect {
            x: 900,
            y: 30,
            width: 200,
            height: 300,
        },
    };
    assert_eq!(hover_target(&[menu, menu_bar, tile], (100, 100), own), None);
}
