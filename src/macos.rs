mod apply;
mod ax;
mod classify;
mod display_link;
mod displays;
mod events;
mod keys;
mod presenter;
mod spaces;
mod swipes;
mod tabs;
#[cfg(test)]
mod tests;
mod window_attributes;

use std::collections::{HashMap, HashSet};

use objc2::rc::autoreleasepool;
use objc2_app_kit::{NSApplicationActivationPolicy, NSRunningApplication, NSWorkspace};
use objc2_application_services::{AXError, AXIsProcessTrusted, AXUIElement};
use objc2_core_foundation::{CFArray, CFString};
use objc2_core_graphics::{
    CGEvent, CGEventFlags, CGEventSource, CGEventSourceStateID, CGEventType, CGMouseButton,
    CGWindowID, CGWindowListOption,
};
use objc2_foundation::ns_string;

use crate::{
    AppInstanceId, AppObservation, NativeObservation, ObservedApplication, ObservedWindow,
    PhysicalWindowId, Rect, Settings,
};

pub use apply::{
    ApplyOutcome, ApplyResult, WindowAccessError, apply, apply_managed, apply_while, focus_window,
    frame, raise_window, set_parked_position, set_position_only, warp_pointer,
};
pub use ax::{
    hover_window_at, onscreen_window_frames, onscreen_window_order, window_server_frame,
    window_server_frames, windows_at_point,
};
pub use display_link::{DisplayFrame, DisplayLink, DisplayLinkError};
pub use displays::{
    DisplayOutput, DisplayOutputs, DisplayTopology, display_ids, display_topology,
    display_under_pointer, outputs,
};
pub use events::WindowEventMonitor;
pub use keys::{KeyMonitor, key_press};
pub use presenter::{
    AppSink, AxBackend, EnhancedUi, InteractivePresenter, PresenterBackend, PresenterStats,
    WriteOutcome,
};
pub use spaces::{last_active_space_change, take_active_space_changed};
pub use swipes::SwipeMonitor;
pub use tabs::{TabDetachResult, detach_tab};

use apply::{cache_managed_window, evict_managed_app, prune_managed_app};
use ax::{
    application_element, attribute, cf_array_items, is_non_standard_window_element, is_settable,
    onscreen_windows, optional_bool_attribute, physical_window_id, set_ax_timeout,
    window_server_windows,
};
use classify::{
    DialogHint, TitleBar, TitleButton, is_dialog, is_file_panel_identifier, is_ignored,
};
use displays::main_output;
use window_attributes::{WindowAttribute, WindowAttributes};

#[derive(Debug)]
pub enum SnapshotError {
    AccessibilityPermissionRequired,
    MainThreadRequired,
    MainScreenUnavailable,
    WindowListUnavailable,
    TargetWindowUnavailable,
}

impl std::fmt::Display for SnapshotError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AccessibilityPermissionRequired => {
                formatter.write_str("Accessibility permission is required for live snapshots")
            }
            Self::MainThreadRequired => formatter.write_str("snapshot must run on the main thread"),
            Self::MainScreenUnavailable => formatter.write_str("the main display is unavailable"),
            Self::WindowListUnavailable => {
                formatter.write_str("the on-screen window list is unavailable")
            }
            Self::TargetWindowUnavailable => {
                formatter.write_str("the target window is unavailable on attached displays")
            }
        }
    }
}

pub fn snapshot(cycle: u64) -> Result<(NativeObservation, Settings), SnapshotError> {
    if !accessibility_trusted() {
        return Err(SnapshotError::AccessibilityPermissionRequired);
    }
    snapshot_in_output(cycle, main_output()?)
}

/// Fractile's own windows (Settings, Onboarding, HUD) are never managed, even when `fractile` CLI
/// observes `Fractile.app` from a second process.
pub(super) fn is_own_application(application: &NSRunningApplication) -> bool {
    application
        .bundleIdentifier()
        .is_some_and(|id| &*id == ns_string!("app.fractile"))
}

#[must_use]
pub fn accessibility_trusted() -> bool {
    // SAFETY: `AXIsProcessTrusted` queries TCC state without taking caller pointers.
    unsafe { AXIsProcessTrusted() }
}

pub fn snapshot_in_outputs(
    cycle: u64,
    outputs: &DisplayOutputs,
) -> Result<NativeObservation, SnapshotError> {
    snapshot_in_output(cycle, outputs.union()).map(|(observation, _)| observation)
}

pub fn snapshot_for_window(
    cycle: u64,
    window_id: u32,
) -> Result<(NativeObservation, Settings), SnapshotError> {
    if !accessibility_trusted() {
        return Err(SnapshotError::AccessibilityPermissionRequired);
    }
    let outputs = outputs()?;
    let (observation, _) = snapshot_in_output(cycle, outputs.union())?;
    let frame = observation
        .applications
        .iter()
        .find_map(|app| match app {
            AppObservation::Complete { windows, .. } => windows
                .iter()
                .find(|window| window.id.window_id == window_id)
                .map(|window| window.frame),
            _ => None,
        })
        .ok_or(SnapshotError::TargetWindowUnavailable)?;
    let output = outputs
        .frames()
        .find(|output| output.contains_center(frame))
        .ok_or(SnapshotError::TargetWindowUnavailable)?;
    snapshot_in_output(cycle, output)
}

pub fn snapshot_in_output(
    cycle: u64,
    output: Rect,
) -> Result<(NativeObservation, Settings), SnapshotError> {
    autoreleasepool(|_| {
        if !accessibility_trusted() {
            return Err(SnapshotError::AccessibilityPermissionRequired);
        }
        let onscreen = onscreen_windows().ok_or(SnapshotError::WindowListUnavailable)?;

        let running = NSWorkspace::sharedWorkspace().runningApplications();
        let screen_locked = is_screen_locked();
        let mut observed_pids = HashSet::new();
        let mut applications: Vec<_> = (0..running.count())
            .filter_map(|index| {
                let application = running.objectAtIndex(index);
                let pid = application.processIdentifier();
                let windows = onscreen.get(&pid)?;
                observed_pids.insert(pid);
                observe_application(&application, pid, windows, output, screen_locked)
            })
            .collect();
        let mut extra_pids: Vec<_> = onscreen
            .keys()
            .copied()
            .filter(|pid| *pid > 0 && !observed_pids.contains(pid))
            .collect();
        extra_pids.sort_unstable();
        for pid in extra_pids {
            if let Some(application) =
                NSRunningApplication::runningApplicationWithProcessIdentifier(pid)
                && let Some(windows) = onscreen.get(&pid)
                && let Some(observed) =
                    observe_application(&application, pid, windows, output, screen_locked)
            {
                applications.push(observed);
            }
        }

        Ok((
            NativeObservation {
                cycle,
                mouse_down: mouse_down(),
                applications,
            },
            Settings::for_output(output),
        ))
    })
}

/// When launched as an app bundle, stderr goes nowhere. Redirect it to a
/// size-capped, owner-only log file so runtime diagnostics survive. Terminal
/// launches keep stderr on the terminal.
pub(crate) fn redirect_stderr_to_log() {
    use std::{
        io::IsTerminal,
        os::unix::fs::{OpenOptionsExt, PermissionsExt},
    };
    if std::io::stderr().is_terminal() {
        return;
    }
    let Some(path) = crate::config::application_support_path("agent.log") else {
        return;
    };
    if let Some(parent) = path.parent() {
        drop(std::fs::create_dir_all(parent));
    }
    if std::fs::metadata(&path).is_ok_and(|metadata| metadata.len() > 5 * 1024 * 1024) {
        // Keep one previous generation: the history before a restart is
        // usually what a bug report needs.
        let previous = path.with_extension("log.prev");
        drop(std::fs::rename(&path, &previous));
        drop(std::fs::set_permissions(
            &previous,
            std::fs::Permissions::from_mode(0o600),
        ));
    }
    let Ok(file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(&path)
    else {
        return;
    };
    drop(std::fs::set_permissions(
        &path,
        std::fs::Permissions::from_mode(0o600),
    ));
    redirect_stderr_to(&file);
}

fn redirect_stderr_to(file: &std::fs::File) {
    let fd = std::os::unix::io::AsRawFd::as_raw_fd(file);
    // SAFETY: dup2 atomically replaces descriptor 2 with a duplicate of a
    // descriptor we own; the original File may close independently afterwards.
    let _: i32 = unsafe { libc::dup2(fd, libc::STDERR_FILENO) };
}

#[must_use]
pub fn focused_window() -> Option<PhysicalWindowId> {
    let id = frontmost_application()?;
    let element = application_element(id.pid);
    let focused = attribute::<AXUIElement>(&element, "AXFocusedWindow").ok()?;
    physical_window_id(&id, &focused)
}

#[must_use]
pub fn frontmost_application() -> Option<AppInstanceId> {
    autoreleasepool(|_| {
        let application = NSWorkspace::sharedWorkspace().frontmostApplication()?;
        if is_lock_screen(&application) {
            return None;
        }
        let pid = resolve_application_pid(&application)?;
        Some(app_instance_with_pid(&application, pid))
    })
}

const LOCK_SCREEN_BUNDLE_ID: &str = "com.apple.loginwindow";

fn is_lock_screen(application: &NSRunningApplication) -> bool {
    application
        .bundleIdentifier()
        .is_some_and(|bundle| &*bundle == ns_string!("com.apple.loginwindow"))
}

fn is_screen_locked() -> bool {
    autoreleasepool(|_| {
        NSWorkspace::sharedWorkspace()
            .frontmostApplication()
            .is_some_and(|app| is_lock_screen(&app))
    })
}

#[must_use]
pub fn mouse_down() -> bool {
    CGEventSource::button_state(CGEventSourceStateID::HIDSystemState, CGMouseButton::Left)
}

/// Whether any mouse button is held: left, right or middle. A pointer warp
/// under a held button would yank a drag or a context menu away (#422).
#[must_use]
pub fn any_mouse_button_down() -> bool {
    any_button_down(|button| {
        CGEventSource::button_state(CGEventSourceStateID::HIDSystemState, button)
    })
}

fn any_button_down(pressed: impl Fn(CGMouseButton) -> bool) -> bool {
    [
        CGMouseButton::Left,
        CGMouseButton::Right,
        CGMouseButton::Center,
    ]
    .into_iter()
    .any(pressed)
}

/// How long ago the user last typed, clicked, or changed modifiers, and
/// whether Command is held. Tells Command-Tab apart from an app activating
/// itself in focus-follow logs, without recording which keys were pressed.
#[derive(Clone, Copy, Debug)]
pub struct RecentInput {
    pub key_down: f64,
    pub mouse_down: f64,
    pub modifiers_changed: f64,
    pub command_held: bool,
}

impl std::fmt::Display for RecentInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "key {:.2}s ago, click {:.2}s ago, modifiers {:.2}s ago, command {}",
            self.key_down,
            self.mouse_down,
            self.modifiers_changed,
            if self.command_held { "held" } else { "up" }
        )
    }
}

#[must_use]
pub fn recent_input() -> RecentInput {
    let state = CGEventSourceStateID::HIDSystemState;
    let since = |event| CGEventSource::seconds_since_last_event_type(state, event);
    RecentInput {
        key_down: since(CGEventType::KeyDown),
        mouse_down: since(CGEventType::LeftMouseDown).min(since(CGEventType::RightMouseDown)),
        modifiers_changed: since(CGEventType::FlagsChanged),
        command_held: CGEventSource::flags_state(state).contains(CGEventFlags::MaskCommand),
    }
}

#[must_use]
pub fn mouse_position() -> Option<(i32, i32)> {
    let event = CGEvent::new(None)?;
    let point = CGEvent::location(Some(&event));
    Some((point.x.round() as i32, point.y.round() as i32))
}

pub(super) fn running_application(
    id: &AppInstanceId,
) -> Option<objc2::rc::Retained<NSRunningApplication>> {
    autoreleasepool(|_| {
        NSRunningApplication::runningApplicationWithProcessIdentifier(id.pid)
            .filter(|application| launch_time(application, id.pid) == id.launched_at)
    })
}

pub(super) fn process_incarnation_matches(id: &AppInstanceId) -> bool {
    autoreleasepool(|_| running_application(id).is_some())
}

#[must_use]
pub fn is_process_incarnation_running(id: &AppInstanceId) -> bool {
    let running = process_incarnation_matches(id);
    if !running {
        evict_managed_app(id);
    }
    running
}

/// The running application instance owning each of `window_ids`, per the window server.
/// Windows it no longer lists, or whose process is not a running application, are left out.
#[must_use]
pub fn window_owners(window_ids: &HashSet<u32>) -> HashMap<u32, AppInstanceId> {
    autoreleasepool(|_| {
        let Some(windows) = window_server_windows(CGWindowListOption::OptionAll) else {
            return HashMap::new();
        };
        windows
            .into_iter()
            .filter(|window| window.pid > 0 && window_ids.contains(&window.id))
            .filter_map(|window| {
                let application =
                    NSRunningApplication::runningApplicationWithProcessIdentifier(window.pid)?;
                Some((window.id, app_instance_with_pid(&application, window.pid)))
            })
            .collect()
    })
}

pub(super) fn app_instance_with_pid(application: &NSRunningApplication, pid: i32) -> AppInstanceId {
    AppInstanceId {
        pid,
        launched_at: launch_time(application, pid),
    }
}

/// Trampoline-launched bundles (such as Xcode's `DeviceHub.app`, whose
/// `CFBundleExecutable` is `DevicesTrampoline`) can register an `LSASN` whose
/// `NSRunningApplication.processIdentifier()` reports `-1` even while the real
/// process owns windows in the window server. Match against window-server
/// owner PIDs to recover the real process identifier.
fn resolve_application_pid(application: &NSRunningApplication) -> Option<i32> {
    let pid = application.processIdentifier();
    if pid > 0 {
        return Some(pid);
    }
    let windows = window_server_windows(CGWindowListOption::OptionAll)?;
    let mut checked = HashSet::new();
    for window in windows {
        if window.pid <= 0 || !checked.insert(window.pid) {
            continue;
        }
        if NSRunningApplication::runningApplicationWithProcessIdentifier(window.pid)
            .is_some_and(|candidate| *candidate == *application)
        {
            return Some(window.pid);
        }
    }
    None
}

/// Processes started outside LaunchServices (a bare executable run from a
/// terminal) have no `launchDate`; their kernel start time identifies the
/// process incarnation instead.
pub(super) fn launch_time(application: &NSRunningApplication, pid: i32) -> u64 {
    application.launchDate().map_or_else(
        || process_start_time(pid).unwrap_or(0),
        |date| (date.timeIntervalSince1970() * 1000.0).round() as u64,
    )
}

pub(super) fn process_start_time(pid: i32) -> Option<u64> {
    if pid <= 0 {
        return None;
    }
    let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::zeroed();
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as i32;
    // SAFETY: `info` is a writable buffer of exactly `size` bytes, and
    // `PROC_PIDTBSDINFO` writes at most one `proc_bsdinfo` into it.
    let written = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            info.as_mut_ptr().cast(),
            size,
        )
    };
    if written != size {
        return None;
    }
    // SAFETY: the kernel filled the whole struct.
    let info = unsafe { info.assume_init() };
    let millis = info.pbi_start_tvsec * 1000 + info.pbi_start_tvusec / 1000;
    (millis > 0).then_some(millis)
}

/// A fresh, unfiltered empty AXWindows list can confirm that an ordered-out
/// surface is stale. A hidden or unresponsive app provides no such evidence.
pub(crate) fn confirms_no_windows(id: &AppInstanceId) -> bool {
    let Some(application) = running_application(id) else {
        return false;
    };
    if application.isHidden() {
        return false;
    }
    let element = application_element(id.pid);
    // SAFETY: This fresh application element is valid. Bound this optional
    // request so an unresponsive app cannot hold up neighboring frame writes.
    if unsafe { element.set_messaging_timeout(0.05) } != AXError::Success {
        return false;
    }
    attribute::<CFArray>(&element, "AXWindows").is_ok_and(|windows| windows.count() == 0)
}

/// The user set the window aside: its app is hidden (⌘H) or the window is
/// minimized (⌘M). Its ordered-out surface then proves it left the layout,
/// unlike a snapshot that merely omitted it.
pub(crate) fn confirms_set_aside(window: &PhysicalWindowId) -> bool {
    running_application(&window.app).is_some_and(|application| {
        application.isHidden() || apply::cached_window_is_minimized(window)
    })
}

fn observe_application(
    application: &NSRunningApplication,
    pid: i32,
    onscreen: &HashMap<CGWindowID, i64>,
    output: Rect,
    screen_locked: bool,
) -> Option<AppObservation> {
    if pid <= 0
        || pid == std::process::id() as i32
        || is_own_application(application)
        || application.isHidden()
    {
        return None;
    }

    let id = app_instance_with_pid(application, pid);
    let accessory = application.activationPolicy() == NSApplicationActivationPolicy::Accessory;
    let bundle_id = application
        .bundleIdentifier()
        .map_or_else(|| "unknown".into(), |value| value.to_string());
    if bundle_id == LOCK_SCREEN_BUNDLE_ID {
        return None;
    }
    if screen_locked {
        return Some(AppObservation::Unavailable {
            app: id,
            error: "screen is locked".into(),
        });
    }
    let app_element = application_element(pid);

    let windows = match attribute::<CFArray>(&app_element, "AXWindows") {
        Ok(windows) => windows,
        Err(error) => {
            return Some(AppObservation::Unavailable {
                app: id,
                error: format!("AXWindows: {error:?}"),
            });
        }
    };

    let focused_element = match attribute::<AXUIElement>(&app_element, "AXFocusedWindow") {
        Ok(window) => Some(window),
        Err(AXError::NoValue | AXError::AttributeUnsupported) => None,
        Err(error) => {
            return Some(AppObservation::Unavailable {
                app: id,
                error: format!("AXFocusedWindow: {error:?}"),
            });
        }
    };
    let focused_id = focused_element
        .as_ref()
        .and_then(|window| physical_window_id(&id, window));
    let focused = focused_id
        .clone()
        .filter(|window| onscreen.contains_key(&window.window_id));
    let mut observed = Vec::new();
    let mut current_windows = HashSet::new();
    let elements = focused_element
        .as_deref()
        .into_iter()
        .map(|window| (window, focused_id.clone()))
        .chain(cf_array_items::<AXUIElement>(&windows).map(|window| (window, None)));
    for (window, known_id) in elements {
        set_ax_timeout(window);
        let Some(window_id) = known_id.or_else(|| physical_window_id(&id, window)) else {
            if is_non_standard_window_element(window) {
                continue;
            }
            return Some(AppObservation::Unavailable {
                app: id,
                error: "physical window ID unavailable".into(),
            });
        };
        if !current_windows.insert(window_id.clone()) {
            continue;
        }
        match observe_window(window_id, accessory, window, onscreen, output) {
            Ok(WindowObservation::Observed(observation)) => {
                cache_managed_window(&observation.id, window);
                observed.push(observation);
            }
            Ok(WindowObservation::Ignored) => {}
            Err(error) => return Some(AppObservation::Unavailable { app: id, error }),
        }
    }
    prune_managed_app(&id, &current_windows);

    if id.launched_at == 0 && !observed.is_empty() {
        return Some(AppObservation::Unavailable {
            app: id,
            error: "application launch date is unavailable".into(),
        });
    }

    Some(AppObservation::Complete {
        app: ObservedApplication { id, bundle_id },
        focused,
        windows: observed,
    })
}

enum WindowObservation {
    Observed(ObservedWindow),
    Ignored,
}

const MIN_MANAGEABLE_WINDOW_DIMENSION: i32 = 64;

fn observe_window(
    id: PhysicalWindowId,
    accessory: bool,
    window: &AXUIElement,
    onscreen: &HashMap<CGWindowID, i64>,
    output: Rect,
) -> Result<WindowObservation, String> {
    let Some(&layer) = onscreen.get(&id.window_id) else {
        return Ok(WindowObservation::Ignored);
    };
    // Every normal macOS window lives on layer 0 (`kCGNormalWindowLevel`).
    // Always-on-top overlays (e.g. Slack Huddle, PiP, floating panels on
    // layer != 0) must be ignored even when they expose a close or fullscreen
    // button, otherwise tiling fights their floating bounds.
    if layer != 0 {
        return Ok(WindowObservation::Ignored);
    }
    // One round trip for every attribute below; each keeps the result (and
    // error) its own `AXUIElementCopyAttributeValue` would have given.
    let attributes = WindowAttributes::read(window);
    let role = attributes
        .get::<CFString>(WindowAttribute::Role)
        .map_err(|error| format!("AXRole: {error:?}"))?;
    if *role != *CFString::from_static_str("AXWindow") {
        return Ok(WindowObservation::Ignored);
    }
    let subrole = match attributes.get::<CFString>(WindowAttribute::Subrole) {
        Ok(subrole) => subrole,
        Err(AXError::NoValue | AXError::AttributeUnsupported) => {
            return Ok(WindowObservation::Ignored);
        }
        Err(error) => return Err(format!("AXSubrole: {error:?}")),
    };
    if *subrole != *CFString::from_static_str("AXStandardWindow") {
        return Ok(WindowObservation::Ignored);
    }

    if attributes
        .optional_bool(WindowAttribute::Minimized)
        .map_err(|error| format!("AXMinimized: {error:?}"))?
        || attributes
            .optional_bool(WindowAttribute::FullScreen)
            .map_err(|error| format!("AXFullScreen: {error:?}"))?
        || !is_settable(window, "AXPosition")
            .map_err(|error| format!("AXPosition settable: {error:?}"))?
    {
        return Ok(WindowObservation::Ignored);
    }

    let frame = attributes.frame().ok_or("AX frame was unavailable")?;
    if frame.width < MIN_MANAGEABLE_WINDOW_DIMENSION
        || frame.height < MIN_MANAGEABLE_WINDOW_DIMENSION
        || !output.contains_center(frame)
    {
        return Ok(WindowObservation::Ignored);
    }
    let close = attributes
        .get::<AXUIElement>(WindowAttribute::CloseButton)
        .is_ok();
    let mut bar = TitleBar {
        fullscreen: title_button(&attributes, WindowAttribute::FullScreenButton),
        zoom: TitleButton::Absent,
        close,
    };
    if is_ignored(accessory, bar) {
        return Ok(WindowObservation::Ignored);
    }
    let modal = attributes
        .optional_bool(WindowAttribute::Modal)
        .unwrap_or(false);
    let hint = DialogHint {
        file_panel: attributes
            .get::<CFString>(WindowAttribute::Identifier)
            .is_ok_and(|id| is_file_panel_identifier(&id.to_string())),
        cancel_button: attributes
            .get::<AXUIElement>(WindowAttribute::CancelButton)
            .is_ok(),
        default_button: attributes
            .get::<AXUIElement>(WindowAttribute::DefaultButton)
            .is_ok(),
    };
    // The zoom button's state costs its own round trip, and only decides a
    // regular application's closable (or default-button) window without a
    // fullscreen button.
    if !accessory
        && !modal
        && !hint.file_panel
        && !hint.cancel_button
        && bar.fullscreen == TitleButton::Absent
        && (close || hint.default_button)
    {
        bar.zoom = title_button(&attributes, WindowAttribute::ZoomButton);
    }
    let dialog = is_dialog(accessory, modal, bar, hint);
    let resizable = !dialog
        && is_settable(window, "AXSize").map_err(|error| format!("AXSize settable: {error:?}"))?;
    let title = match attributes.get::<CFString>(WindowAttribute::Title) {
        Ok(title) => Some(std::sync::Arc::from(title.to_string())),
        Err(_) => None,
    };
    let document = attributes
        .get::<CFString>(WindowAttribute::Document)
        .ok()
        .and_then(|document| crate::window_memory::normalize_document(&document.to_string()))
        .map(std::sync::Arc::from);
    Ok(WindowObservation::Observed(ObservedWindow {
        id,
        frame,
        resizable,
        title,
        document,
    }))
}

fn title_button(attributes: &WindowAttributes<'_>, button: WindowAttribute) -> TitleButton {
    match attributes.get::<AXUIElement>(button) {
        Ok(button) => {
            set_ax_timeout(&button);
            if optional_bool_attribute(&button, "AXEnabled").unwrap_or(false) {
                TitleButton::Enabled
            } else {
                TitleButton::Disabled
            }
        }
        Err(_) => TitleButton::Absent,
    }
}
