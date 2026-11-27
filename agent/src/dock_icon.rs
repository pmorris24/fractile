//! The live Dock icon: Fractile's icon over a simulated screen of window outlines in the app
//! icon's split (a half, a quarter, two eighths). Each time the focused workspace changes, the
//! windows trade places, each sliding and resizing into the next slot over [`DURATION`] seconds,
//! the focused one outlined in orange, then hold still. It shows what Fractile does at Dock size;
//! it is not the workspace's own layout.
//!
//! Each frame is an SVG with brand/fractile-icon.svg's tile, F and colours; macOS draws it.

use fractile::{WorkspaceId, app::UiAgentSnapshot};

/// A window in unit space (the icon's window area is 1 by 1): x, y, width, height.
type Unit = (f32, f32, f32, f32);

/// The slots, in the order windows move through them: the half, the quarter, then the eighths.
const SLOTS: [Unit; 4] = [
    (0.0, 0.0, 0.5, 1.0),
    (0.5, 0.0, 0.5, 0.5),
    (0.75, 0.5, 0.25, 0.5),
    (0.5, 0.5, 0.25, 0.5),
];

/// How long the windows take to rearrange, in seconds.
pub(crate) const DURATION: f64 = 0.45;
/// Between frames while they do.
pub(crate) const FRAME: f64 = 1.0 / 30.0;

/// What the Dock icon shows and whether it is moving.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct DockIcon {
    /// The focused workspace last seen; a change sets the windows moving.
    workspace: Option<WorkspaceId>,
    /// How many places the windows have moved on: window `i` sits in slot `(i + turn) % 4`.
    turn: usize,
    /// While the windows move: when they started, on Rune's clock.
    moving: Option<f64>,
    /// Nothing drawn yet.
    drawn: bool,
}

impl DockIcon {
    /// Takes in a snapshot: a newly focused workspace sets the windows moving. Returns the
    /// frame to show now, if any.
    pub(crate) fn snapshot(&mut self, snapshot: &UiAgentSnapshot, clock: f64) -> Option<String> {
        let workspace = snapshot
            .focused_workspace
            .or_else(|| snapshot.active_workspaces.first().copied());
        if !self.drawn {
            self.drawn = true;
            self.workspace = workspace;
            return Some(frame(self.turn, self.turn, 1.0));
        }
        if workspace == self.workspace {
            return None;
        }
        self.workspace = workspace;
        self.turn += 1;
        self.moving = Some(clock);
        Some(frame(self.turn - 1, self.turn, 0.0))
    }

    /// The frame for `clock` while the windows move, and whether more follow.
    pub(crate) fn tick(&mut self, clock: f64, still: bool) -> Option<(String, bool)> {
        let started = self.moving?;
        let t = if still {
            1.0
        } else {
            ((clock - started) / DURATION).clamp(0.0, 1.0)
        };
        let done = t >= 1.0;
        if done {
            self.moving = None;
        }
        Some((frame(self.turn - 1, self.turn, ease(t as f32)), !done))
    }
}

/// Ease in and out, so the windows settle.
fn ease(t: f32) -> f32 {
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
    }
}

fn lerp(a: Unit, b: Unit, t: f32) -> Unit {
    (
        a.0 + (b.0 - a.0) * t,
        a.1 + (b.1 - a.1) * t,
        a.2 + (b.2 - a.2) * t,
        a.3 + (b.3 - a.3) * t,
    )
}

/// The windows' area inside the icon's tile, the gap between windows, and their outline.
const AREA: (f32, f32, f32, f32) = (164.0, 164.0, 696.0, 696.0);
const GAP: f32 = 26.0;
const LINE: f32 = 10.0;
const FOCUS_LINE: f32 = 13.0;

/// A unit window in the icon's 1024-point space, half a gap off each side that meets another.
fn place(unit: Unit) -> (f32, f32, f32, f32) {
    let side = |start: f32, length: f32| {
        let lead = if start > 0.001 { GAP / 2.0 } else { 0.0 };
        let trail = if start + length < 0.999 {
            GAP / 2.0
        } else {
            0.0
        };
        (start, lead, trail, length)
    };
    let (x, xl, xt, w) = side(unit.0, unit.2);
    let (y, yl, yt, h) = side(unit.1, unit.3);
    (
        AREA.0 + x * AREA.2 + xl,
        AREA.1 + y * AREA.3 + yl,
        (w * AREA.2 - xl - xt).max(1.0),
        (h * AREA.3 - yl - yt).max(1.0),
    )
}

/// The F's outline, in brand/fractile-mark.svg's units.
const F: &str =
    "M3 113 V58 L42 3 H117 L101 26 H54 L35 52 V113 Z M44 50 H115 L99 73 H40 L35 80 V64 Z";

/// The icon `t` of the way from turn `from` to turn `to`, as SVG: window `i` moves from slot
/// `(i + from) % 4` to slot `(i + to) % 4`, and window 0 is the focused one.
fn frame(from: usize, to: usize, t: f32) -> String {
    let mut out = String::with_capacity(6_000);
    out.push_str(concat!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="1024" viewBox="0 0 1024 1024"><defs>"##,
        r##"<linearGradient id="tile" x1="0" y1="100" x2="0" y2="924" gradientUnits="userSpaceOnUse"><stop offset="0" stop-color="#26262C"/><stop offset="1" stop-color="#0C0C0F"/></linearGradient>"##,
        r##"<linearGradient id="rim" x1="0" y1="100" x2="0" y2="512" gradientUnits="userSpaceOnUse"><stop offset="0" stop-color="#FFFFFF" stop-opacity="0.18"/><stop offset="1" stop-color="#FFFFFF" stop-opacity="0"/></linearGradient>"##,
        r##"<linearGradient id="mark" x1="0" y1="3" x2="0" y2="116" gradientUnits="userSpaceOnUse"><stop offset="0" stop-color="#FFFFFF"/><stop offset="1" stop-color="#E6E4DD"/></linearGradient>"##,
        r##"<clipPath id="clip"><rect x="100" y="100" width="824" height="824" rx="185"/></clipPath></defs>"##,
    ));
    for step in 1..=8 {
        out.push_str(&format!(
            r##"<rect x="{}" y="{}" width="{}" height="824" rx="185" fill="#000" fill-opacity="0.045"/>"##,
            100 - step,
            100 + 3 * step,
            824 + 2 * step
        ));
    }
    out.push_str(r##"<rect x="100" y="100" width="824" height="824" rx="185" fill="url(#tile)"/><g clip-path="url(#clip)">"##);
    let at = |window: usize, turn: usize| SLOTS[(window + turn) % SLOTS.len()];
    for window in 1..SLOTS.len() {
        out.push_str(&outline(
            place(lerp(at(window, from), at(window, to), t)),
            LINE,
            "#FFFFFF",
            0.34,
            0.035,
        ));
    }
    // The focused window, on top of the others as they pass.
    let focus = place(lerp(at(0, from), at(0, to), t));
    out.push_str(&outline(focus, FOCUS_LINE, "#FF6A3D", 1.0, 0.2));
    out.push_str(r##"</g><rect x="100" y="100" width="824" height="824" rx="185" fill="none" stroke="url(#rim)" stroke-width="4"/>"##);
    out.push_str(r#"<g transform="translate(252.63 266.88) scale(4.1228)">"#);
    for step in 1..=8 {
        out.push_str(&format!(
            r##"<path d="{F}" transform="translate(0 {:.2})" fill="#000" fill-opacity="0.08" stroke="#000" stroke-opacity="0.08" stroke-width="{:.1}" stroke-linejoin="round"/>"##,
            0.45 * step as f32,
            3.0 + 0.6 * step as f32
        ));
    }
    out.push_str(&format!(
        r#"<path d="{F}" fill="url(#mark)" stroke="url(#mark)" stroke-width="3" stroke-linejoin="round"/></g></svg>"#
    ));
    out
}

/// A window's outline, drawn inside its frame, with a faint fill.
fn outline(frame: (f32, f32, f32, f32), line: f32, color: &str, stroke: f32, fill: f32) -> String {
    let (x, y, w, h) = (
        frame.0 + line / 2.0,
        frame.1 + line / 2.0,
        (frame.2 - line).max(1.0),
        (frame.3 - line).max(1.0),
    );
    let radius = (frame.2 * 0.3).min(frame.3 * 0.3).min(100.5) - line / 2.0;
    format!(
        r#"<rect x="{x:.1}" y="{y:.1}" width="{w:.1}" height="{h:.1}" rx="{:.1}" fill="{color}" fill-opacity="{fill}" stroke="{color}" stroke-opacity="{stroke}" stroke-width="{line}"/>"#,
        radius.max(1.0)
    )
}

/// Sets `svg` as the Dock icon. Main thread only.
#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
pub(crate) fn show(svg: &str) {
    use objc2::{AllocAnyThread, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSData;
    let Some(marker) = MainThreadMarker::new() else {
        return;
    };
    let data = NSData::with_bytes(svg.as_bytes());
    let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) else {
        return;
    };
    // SAFETY: on the main thread, as AppKit requires; the image outlives the call.
    unsafe { NSApplication::sharedApplication(marker).setApplicationIconImage(Some(&image)) };
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn show(_svg: &str) {}

/// Reduce Motion: the windows jump to the next layout instead of moving.
#[cfg(target_os = "macos")]
pub(crate) fn still() -> bool {
    objc2_app_kit::NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion()
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn still() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use fractile::app::UiLaunchAtLogin;

    fn snapshot(focused: WorkspaceId) -> UiAgentSnapshot {
        UiAgentSnapshot {
            accessibility: true,
            automatic: true,
            hud: true,
            launch_at_login: UiLaunchAtLogin::Off,
            active_workspaces: vec![focused],
            focused_workspace: Some(focused),
            windows: Vec::new(),
            last_error: None,
        }
    }

    #[test]
    fn a_switch_moves_every_window_one_slot_on_and_then_they_hold_still() {
        let mut icon = DockIcon::default();
        // First sight: the split, still.
        assert!(icon.snapshot(&snapshot(WorkspaceId::ONE), 0.0).is_some());
        assert!(icon.tick(0.5, false).is_none());
        assert!(icon.snapshot(&snapshot(WorkspaceId::ONE), 1.0).is_none());
        // A switch sets them moving, and they land a slot on.
        assert!(icon.snapshot(&snapshot(WorkspaceId::TWO), 2.0).is_some());
        let (_, more) = icon.tick(2.0 + DURATION / 2.0, false).unwrap();
        assert!(more);
        let (last, more) = icon.tick(2.0 + DURATION, false).unwrap();
        assert!(!more);
        assert_eq!(last, frame(1, 1, 1.0));
        assert!(icon.tick(3.0, false).is_none());
        // Under Reduce Motion they land at once.
        icon.snapshot(&snapshot(WorkspaceId::THREE), 4.0);
        let (_, more) = icon.tick(4.0, true).unwrap();
        assert!(!more);
    }

    #[test]
    fn each_frame_outlines_four_windows_the_focused_one_orange_and_no_badge() {
        for turn in 0..4 {
            let drawn = frame(turn, turn, 1.0);
            assert_eq!(drawn.matches(r##"stroke="#FFFFFF""##).count(), 3);
            assert_eq!(drawn.matches(r##"stroke="#FF6A3D""##).count(), 1);
            assert!(!drawn.contains("<text") && !drawn.contains("<circle"));
        }
        // The focused window moves on with each turn.
        assert_ne!(frame(0, 0, 1.0), frame(1, 1, 1.0));
    }
}
