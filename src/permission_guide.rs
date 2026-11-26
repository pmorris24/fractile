//! Accessibility permission guide docked beside System Settings, plus focus
//! return tracking so Fractile comes back to the front as soon as trust is
//! granted.

use std::{
    ffi::{CStr, c_char},
    time::{Duration, Instant},
};

use objc2::MainThreadMarker;

unsafe extern "C" {
    fn fractile_show_accessibility_guide();
    fn fractile_close_accessibility_guide();
    fn fractile_accessibility_guide_active() -> bool;
    fn fractile_accessibility_list_name() -> *const c_char;
    fn fractile_accessibility_drag_supported() -> bool;
    fn fractile_permission_settings_state() -> i32;
    fn fractile_prompt_accessibility_trust() -> bool;
}

/// How long after a grant System Settings may take to come back to the front
/// (for example while a Touch ID or password sheet closes) before Fractile
/// stops waiting to return.
pub const RETURN_FROM_SETTINGS_WINDOW: Duration = Duration::from_secs(3);

/// Main-thread snapshot of System Settings and Fractile window focus.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SettingsFocus {
    pub settings_open: bool,
    pub settings_frontmost: bool,
    pub fractile_active: bool,
}

/// Brings Fractile back after the user grants Accessibility in System Settings,
/// so the onboarding tour's auto-advance is immediately visible. Fractile
/// returns only while System Settings is still frontmost: someone who switched
/// to another app keeps it.
#[derive(Clone, Copy, Debug, Default)]
pub struct ReturnFromSettings {
    granted_at: Option<Instant>,
}

impl ReturnFromSettings {
    pub fn note_granted(&mut self, now: Instant) {
        self.granted_at = Some(now);
    }

    #[must_use]
    pub const fn is_pending(&self) -> bool {
        self.granted_at.is_some()
    }

    /// Returns `true` when Fractile should be brought back to the front now.
    pub fn poll(&mut self, now: Instant, focus: SettingsFocus) -> bool {
        let Some(granted_at) = self.granted_at else {
            return false;
        };
        if focus.fractile_active
            || now.saturating_duration_since(granted_at) > RETURN_FROM_SETTINGS_WINDOW
        {
            self.granted_at = None;
            return false;
        }
        if focus.settings_frontmost {
            self.granted_at = None;
            return true;
        }
        false
    }
}

/// Opens System Settings' Accessibility pane and docks Fractile's floating drag
/// guide beside it (with a flying-tile handoff when clicked from Fractile).
pub fn show() {
    if MainThreadMarker::new().is_some() {
        // SAFETY: Verified on the main thread; the Objective-C helper owns its panel state.
        unsafe { fractile_show_accessibility_guide() };
    }
}

/// Dismisses the floating Accessibility guide and cancels pending cold-launch retries.
pub fn close() {
    if MainThreadMarker::new().is_some() {
        // SAFETY: Verified on the main thread; idempotent when no guide is open.
        unsafe { fractile_close_accessibility_guide() };
    }
}

/// Whether the floating Accessibility guide is open (visible or waiting for System Settings).
#[must_use]
pub fn active() -> bool {
    // SAFETY: Verified on the main thread before touching Objective-C panel state.
    MainThreadMarker::new().is_some() && unsafe { fractile_accessibility_guide_active() }
}

/// The name System Settings gives the Accessibility list: `"Accessibility"`
/// through macOS 26, `"Device Control and Data Access"` on macOS 27.0 through
/// 27.2, and `"System & App Protection Override"` on macOS 27.3 and later.
#[must_use]
pub fn list_name() -> &'static str {
    // SAFETY: Returns a static NUL-terminated ASCII string literal from Objective-C.
    let name = unsafe { CStr::from_ptr(fractile_accessibility_list_name()) };
    name.to_str().unwrap_or("Accessibility")
}

/// Whether System Settings accepts dragging Fractile's tile into its list.
///
/// macOS 27.3's System & App Protection Override rejects dropped tiles: it asks
/// for a click on the Privacy & Security notification instead, and apps are
/// added with the pane's own controls. macOS 27.0 through 27.2 only renamed the
/// list and still takes the dragged tile.
#[must_use]
pub fn drag_supported() -> bool {
    // SAFETY: Pure operating-system version check in Objective-C; no shared state.
    unsafe { fractile_accessibility_drag_supported() }
}

/// What to do in System Settings while Accessibility is still missing. The
/// first-run tour shows this, and it never tells anyone to drag on a pane that
/// refuses drops.
#[must_use]
pub fn add_instructions() -> String {
    instructions(list_name(), drag_supported())
}

fn instructions(list: &str, drag: bool) -> String {
    if drag {
        format!("Drag Fractile into the {list} list, or switch it on in System Settings.")
    } else {
        format!("Click the Privacy & Security notification, or add Fractile in {list}.")
    }
}

/// Registers the running binary in Accessibility and triggers macOS's prompt
/// when untrusted. Returns whether the process is already trusted.
#[must_use]
pub fn prompt_accessibility_trust() -> bool {
    // SAFETY: Objective-C helper builds an owned options dictionary and calls
    // `AXIsProcessTrustedWithOptions`.
    unsafe { fractile_prompt_accessibility_trust() }
}

/// Reads whether System Settings has an on-screen window, whether System
/// Settings is frontmost, and whether Fractile is active. Main thread only.
#[must_use]
pub fn settings_focus() -> SettingsFocus {
    if MainThreadMarker::new().is_none() {
        return SettingsFocus::default();
    }
    // SAFETY: Verified on the main thread; returns a bitmask of AppKit/CG state.
    let bits = unsafe { fractile_permission_settings_state() };
    SettingsFocus {
        settings_open: bits & 1 != 0,
        settings_frontmost: bits & 2 != 0,
        fractile_active: bits & 4 != 0,
    }
}

#[cfg(test)]
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
struct GuidePoint {
    x: f64,
    y: f64,
}

#[cfg(test)]
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
struct GuideSize {
    width: f64,
    height: f64,
}

#[cfg(test)]
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
struct GuideRect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

#[cfg(test)]
#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct GuideWindowInfo {
    pid: i32,
    layer: i32,
    alpha: f64,
    frame: GuideRect,
    is_settings_owner: bool,
    is_system_prompt_owner: bool,
    is_ignored_chrome: bool,
}

#[cfg(test)]
#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct GuideWindowAnalysis {
    has_settings_frame: bool,
    settings_frame: GuideRect,
    has_blocking_sheet_or_prompt: bool,
}

#[cfg(test)]
unsafe extern "C" {
    fn fractile_permission_guide_origin(
        settings: GuideRect,
        visible: GuideRect,
        panel: GuideSize,
        protected_mode: bool,
    ) -> GuidePoint;
    fn fractile_permission_guide_analyze_windows(
        windows: *const GuideWindowInfo,
        count: usize,
        self_pid: i32,
        modal_layer: i32,
    ) -> GuideWindowAnalysis;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guide_origin(
        settings: GuideRect,
        visible: GuideRect,
        panel: GuideSize,
        protected_mode: bool,
    ) -> GuidePoint {
        // SAFETY: Pure value-type geometry calculation in `native/permission_guide.m`.
        unsafe { fractile_permission_guide_origin(settings, visible, panel, protected_mode) }
    }

    fn analyze_windows(windows: &[GuideWindowInfo], self_pid: i32) -> GuideWindowAnalysis {
        // SAFETY: Passes a valid slice pointer and length to the pure C analyzer.
        unsafe {
            fractile_permission_guide_analyze_windows(windows.as_ptr(), windows.len(), self_pid, 8)
        }
    }

    fn rect(x: f64, y: f64, width: f64, height: f64) -> GuideRect {
        GuideRect {
            x,
            y,
            width,
            height,
        }
    }

    fn settings_window(frame: GuideRect, layer: i32) -> GuideWindowInfo {
        GuideWindowInfo {
            pid: 500,
            layer,
            alpha: 1.0,
            frame,
            is_settings_owner: true,
            is_system_prompt_owner: false,
            is_ignored_chrome: false,
        }
    }

    #[test]
    fn return_from_settings_brings_fractile_back_only_from_system_settings() {
        let t0 = Instant::now();
        let in_settings = SettingsFocus {
            settings_open: true,
            settings_frontmost: true,
            fractile_active: false,
        };
        let sheet_closing = SettingsFocus {
            settings_open: true,
            settings_frontmost: false,
            fractile_active: false,
        };
        let back_in_fractile = SettingsFocus {
            settings_open: true,
            settings_frontmost: false,
            fractile_active: true,
        };

        let mut returning = ReturnFromSettings::default();
        assert!(!returning.poll(t0, in_settings));

        // Immediate grant while System Settings is frontmost returns once.
        returning.note_granted(t0);
        assert!(returning.poll(t0, in_settings));
        assert!(!returning.is_pending());
        assert!(!returning.poll(t0 + Duration::from_millis(500), in_settings));

        // Waiting for a Touch ID / password sheet to dismiss still returns within the window.
        returning.note_granted(t0);
        assert!(!returning.poll(t0, sheet_closing));
        assert!(returning.is_pending());
        assert!(returning.poll(t0 + Duration::from_secs(1), in_settings));
        assert!(!returning.is_pending());

        // Switching to another app past the window cancels instead of stealing focus later.
        returning.note_granted(t0);
        assert!(!returning.poll(t0 + Duration::from_secs(1), sheet_closing));
        assert!(!returning.poll(t0 + Duration::from_secs(4), in_settings));
        assert!(!returning.is_pending());

        // Already active in Fractile cancels cleanly.
        returning.note_granted(t0);
        assert!(!returning.poll(t0, back_in_fractile));
        assert!(!returning.is_pending());
    }

    #[test]
    fn accessibility_list_name_is_recognized() {
        let name = list_name();
        assert!(
            matches!(
                name,
                "Accessibility"
                    | "Device Control and Data Access"
                    | "System & App Protection Override"
            ),
            "unexpected Accessibility list name: {name}"
        );
        // Only macOS 27.3+ stopped accepting the dragged tile.
        assert_eq!(drag_supported(), name != "System & App Protection Override");
    }

    #[test]
    fn instructions_follow_each_macos_tier() {
        // macOS 14 through 26.
        let legacy = instructions("Accessibility", true);
        assert!(legacy.contains("Drag Fractile into the Accessibility list"));

        // macOS 27.0 through 27.2: renamed, still draggable.
        let renamed = instructions("Device Control and Data Access", true);
        assert!(renamed.contains("Drag Fractile into the Device Control and Data Access list"));

        // macOS 27.3: no drag, so the copy points at the notification instead.
        let protected = instructions("System & App Protection Override", false);
        assert!(!protected.contains("Drag"), "misleading copy: {protected}");
        assert!(protected.contains("Privacy & Security notification"));
        assert!(protected.contains("System & App Protection Override"));
    }

    #[test]
    fn the_running_system_gets_matching_copy() {
        let copy = add_instructions();
        assert!(copy.contains(list_name()));
        assert_eq!(copy.contains("Drag"), drag_supported());
    }

    #[test]
    fn protected_guide_prefers_above_then_below_then_top_never_bottom() {
        let visible = rect(0.0, 0.0, 1440.0, 875.0);
        let panel = GuideSize {
            width: 658.0,
            height: 52.0,
        };

        // Room above and below: prefers above System Settings (+12 pt).
        let centered = rect(360.0, 140.0, 720.0, 600.0);
        let above = guide_origin(centered, visible, panel, true);
        assert_eq!(above.y, 140.0 + 600.0 + 12.0);
        assert_eq!(above.x, 360.0 + 360.0 - 658.0 / 2.0);

        // Settings near the top of the screen: no room above, falls back below (-12 pt).
        let near_top = rect(360.0, 250.0, 720.0, 600.0);
        let below = guide_origin(near_top, visible, panel, true);
        assert_eq!(below.y, 250.0 - 52.0 - 12.0);

        // Tall / full-height Settings on a laptop display: neither above nor below fits.
        // Must sit near the top of visible (never NSMinY(visible) + 8 over bottom buttons).
        let laptop_visible = rect(0.0, 0.0, 1280.0, 775.0);
        let tall_settings = rect(200.0, 15.0, 880.0, 745.0);
        let fallback = guide_origin(tall_settings, laptop_visible, panel, true);
        assert_eq!(fallback.y, 775.0 - 52.0 - 12.0);
        assert!(
            fallback.y >= tall_settings.y + 120.0,
            "fallback ({}) overlapped bottom 120 pt of Settings",
            fallback.y
        );

        // Secondary display with negative Y coordinates behaves identically.
        let lower_display = rect(0.0, -900.0, 1440.0, 875.0);
        let tall_on_lower = rect(360.0, -885.0, 720.0, 845.0);
        let lower_fallback = guide_origin(tall_on_lower, lower_display, panel, true);
        assert_eq!(lower_fallback.y, -25.0 - 52.0 - 12.0);
        assert!(lower_fallback.y >= tall_on_lower.y + 120.0);
    }

    #[test]
    fn protected_guide_never_overlaps_settings_bottom_band_on_standard_displays() {
        let visible = rect(0.0, 0.0, 1280.0, 720.0);
        let panel = GuideSize {
            width: 658.0,
            height: 52.0,
        };
        for settings_height in [400.0, 550.0, 650.0, 700.0, 720.0] {
            for offset_y in [0.0, 10.0, 40.0, 80.0, 140.0, 260.0] {
                if offset_y + settings_height > visible.height + 40.0 {
                    continue;
                }
                let settings = rect(240.0, offset_y, 760.0, settings_height);
                let origin = guide_origin(settings, visible, panel, true);
                let panel_top = origin.y + panel.height;
                let danger_bottom = settings.y;
                let danger_top = settings.y + 120.0;
                let overlaps_bottom = origin.y < danger_top && panel_top > danger_bottom;
                assert!(
                    !overlaps_bottom,
                    "panel [{}, {}] overlapped Settings bottom band [{}, {}] for settings={settings:?}",
                    origin.y, panel_top, danger_bottom, danger_top
                );
            }
        }
    }

    #[test]
    fn legacy_drag_guide_stays_clamped_inside_visible_frame() {
        let visible = rect(0.0, 0.0, 1440.0, 875.0);
        let settings = rect(360.0, 140.0, 720.0, 600.0);
        let panel = GuideSize {
            width: 480.0,
            height: 116.0,
        };
        let origin = guide_origin(settings, visible, panel, false);
        assert_eq!(origin.x, 360.0 + 720.0 - 480.0 - 16.0);
        assert_eq!(origin.y, 140.0 + 16.0);
    }

    #[test]
    fn window_analysis_hides_guide_for_settings_sheets_and_security_prompts() {
        let main_frame = rect(300.0, 100.0, 720.0, 620.0);
        let sheet_frame = rect(430.0, 200.0, 460.0, 360.0);
        let main = settings_window(main_frame, 0);

        // Main System Settings window alone: show the guide.
        let alone = analyze_windows(&[main], 999);
        assert!(alone.has_settings_frame);
        assert!(!alone.has_blocking_sheet_or_prompt);

        // Layer-0 attached sheet ("Choose Apps" / "Override System Protections"): hide.
        let layer0_sheet = settings_window(sheet_frame, 0);
        let with_layer0_sheet = analyze_windows(&[layer0_sheet, main], 999);
        assert!(with_layer0_sheet.has_settings_frame);
        assert!(with_layer0_sheet.has_blocking_sheet_or_prompt);

        // Modal-layer sheet owned by System Settings: hide.
        let modal_sheet = settings_window(sheet_frame, 8);
        let with_modal_sheet = analyze_windows(&[modal_sheet, main], 999);
        assert!(with_modal_sheet.has_blocking_sheet_or_prompt);

        // Tiny or transparent Settings helper windows must not trigger false positives.
        let tiny_helper = settings_window(rect(310.0, 110.0, 24.0, 24.0), 0);
        let invisible_helper = GuideWindowInfo {
            alpha: 0.0,
            ..settings_window(sheet_frame, 0)
        };
        let with_helpers = analyze_windows(&[tiny_helper, invisible_helper, main], 999);
        assert!(!with_helpers.has_blocking_sheet_or_prompt);

        // System security / TCC prompt (SecurityAgent, CoreServicesUIAgent, etc.): hide.
        let security_prompt = GuideWindowInfo {
            pid: 700,
            layer: 8,
            alpha: 1.0,
            frame: sheet_frame,
            is_settings_owner: false,
            is_system_prompt_owner: true,
            is_ignored_chrome: false,
        };
        let with_prompt = analyze_windows(&[security_prompt, main], 999);
        assert!(with_prompt.has_blocking_sheet_or_prompt);

        // Unknown macOS 27.3 XPC sheet window rendered in front of Settings overlapping it: hide.
        let xpc_sheet = GuideWindowInfo {
            pid: 801,
            layer: 0,
            alpha: 1.0,
            frame: sheet_frame,
            is_settings_owner: false,
            is_system_prompt_owner: false,
            is_ignored_chrome: false,
        };
        let with_xpc_sheet = analyze_windows(&[xpc_sheet, main], 999);
        assert!(with_xpc_sheet.has_blocking_sheet_or_prompt);

        // Regular app behind System Settings, Fractile's own panel, and another display's window
        // must not hide the guide.
        let own_panel = GuideWindowInfo {
            pid: 999,
            layer: 3,
            alpha: 1.0,
            frame: rect(300.0, 40.0, 658.0, 52.0),
            is_settings_owner: false,
            is_system_prompt_owner: false,
            is_ignored_chrome: false,
        };
        let other_monitor = GuideWindowInfo {
            pid: 802,
            layer: 0,
            alpha: 1.0,
            frame: rect(2000.0, 100.0, 900.0, 700.0),
            is_settings_owner: false,
            is_system_prompt_owner: false,
            is_ignored_chrome: false,
        };
        let app_behind = GuideWindowInfo {
            pid: 803,
            layer: 0,
            alpha: 1.0,
            frame: main_frame,
            is_settings_owner: false,
            is_system_prompt_owner: false,
            is_ignored_chrome: false,
        };
        let benign = analyze_windows(&[own_panel, other_monitor, main, app_behind], 999);
        assert!(benign.has_settings_frame);
        assert!(!benign.has_blocking_sheet_or_prompt);
    }
}
