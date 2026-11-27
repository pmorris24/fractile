//! The live Dock icon: Fractile's icon redrawn from the focused workspace. Its windows are that
//! workspace's real tile layout, the first tile lit orange as in the app icon, with the
//! workspace's number on a paper disc at the bottom right (clear of the top right, where macOS
//! puts its red badges). It changes only when the workspace or its layout does.
//!
//! The drawing is an SVG with brand/fractile-icon.svg's tile, F and colours; macOS draws it.

use fractile::{
    WorkspaceId,
    app::{HUD_PORTRAIT_PREVIEW_RECT, HUD_PREVIEW_RECT, UiAgentSnapshot},
};

/// A window, in the icon's 1024-point space: x, y, width, height.
type Frame = (f32, f32, f32, f32);

/// What the icon shows: redrawn only when this changes.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Face {
    pub workspace: Option<WorkspaceId>,
    pub windows: Vec<Frame>,
}

/// The windows' area inside the icon's tile, and the gap between them.
const AREA: Frame = (164.0, 164.0, 696.0, 696.0);
const GAP: f32 = 26.0;

/// The icon's own split, for a workspace with nothing tiled: a half, a quarter, two eighths.
fn resting() -> Vec<Frame> {
    vec![
        (164.0, 164.0, 335.0, 696.0),
        (525.0, 164.0, 335.0, 335.0),
        (525.0, 525.0, 154.5, 335.0),
        (705.5, 525.0, 154.5, 335.0),
    ]
}

/// The face for `snapshot`'s focused workspace: its tiles fitted into the icon's window area.
pub(crate) fn face(snapshot: &UiAgentSnapshot) -> Face {
    let workspace = snapshot
        .focused_workspace
        .or_else(|| snapshot.active_workspaces.first().copied());
    let entry = workspace.and_then(|workspace| {
        snapshot
            .windows
            .iter()
            .find(|entry| entry.windows.workspace == workspace)
    });
    let windows = match entry {
        Some(entry) if entry.windows.fullscreen && !entry.windows.tiles.is_empty() => vec![AREA],
        Some(entry) if !entry.windows.tiles.is_empty() => {
            let preview = if entry.portrait {
                HUD_PORTRAIT_PREVIEW_RECT
            } else {
                HUD_PREVIEW_RECT
            };
            let (px, py) = (preview.x as f32, preview.y as f32);
            let (pw, ph) = (preview.width.max(1) as f32, preview.height.max(1) as f32);
            entry
                .windows
                .tiles
                .iter()
                .map(|tile| {
                    let r = tile.rect;
                    let x = AREA.0 + (r.x as f32 - px) / pw * AREA.2;
                    let y = AREA.1 + (r.y as f32 - py) / ph * AREA.3;
                    let w = r.width as f32 / pw * AREA.2;
                    let h = r.height as f32 / ph * AREA.3;
                    // Half the gap off each side that meets another window.
                    let inset = |start: f32, length: f32, origin: f32, span: f32| {
                        let lead = if start - origin > 1.0 { GAP / 2.0 } else { 0.0 };
                        let trail = if origin + span - (start + length) > 1.0 {
                            GAP / 2.0
                        } else {
                            0.0
                        };
                        (start + lead, (length - lead - trail).max(1.0))
                    };
                    let (x, w) = inset(x, w, AREA.0, AREA.2);
                    let (y, h) = inset(y, h, AREA.1, AREA.3);
                    (x, y, w, h)
                })
                .collect()
        }
        _ => resting(),
    };
    Face { workspace, windows }
}

/// The F's outline, in brand/fractile-mark.svg's units.
const F: &str =
    "M3 113 V58 L42 3 H117 L101 26 H54 L35 52 V113 Z M44 50 H115 L99 73 H40 L35 80 V64 Z";

fn round(rect: Frame) -> f32 {
    (rect.2 * 0.3).min(rect.3 * 0.3).min(100.5)
}

/// The icon for `face`, as SVG.
pub(crate) fn svg(face: &Face) -> String {
    let mut out = String::with_capacity(6_000);
    out.push_str(concat!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="1024" viewBox="0 0 1024 1024"><defs>"##,
        r##"<linearGradient id="tile" x1="0" y1="100" x2="0" y2="924" gradientUnits="userSpaceOnUse"><stop offset="0" stop-color="#26262C"/><stop offset="1" stop-color="#0C0C0F"/></linearGradient>"##,
        r##"<linearGradient id="rim" x1="0" y1="100" x2="0" y2="512" gradientUnits="userSpaceOnUse"><stop offset="0" stop-color="#FFFFFF" stop-opacity="0.18"/><stop offset="1" stop-color="#FFFFFF" stop-opacity="0"/></linearGradient>"##,
        r##"<linearGradient id="focus" x1="0" y1="164" x2="0" y2="860" gradientUnits="userSpaceOnUse"><stop offset="0" stop-color="#FF7A48"/><stop offset="1" stop-color="#F2471C"/></linearGradient>"##,
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
    for (index, window) in face.windows.iter().enumerate() {
        let fill = if index == 0 {
            r##"fill="url(#focus)""##
        } else {
            r##"fill="#FFFFFF" fill-opacity="0.07""##
        };
        out.push_str(&format!(
            r#"<rect x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" rx="{:.1}" {fill}/>"#,
            window.0,
            window.1,
            window.2,
            window.3,
            round(*window)
        ));
    }
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
        r#"<path d="{F}" fill="url(#mark)" stroke="url(#mark)" stroke-width="3" stroke-linejoin="round"/></g>"#
    ));
    if let Some(workspace) = face.workspace {
        let label = workspace.to_string();
        let size = if label.chars().count() > 1 { 92 } else { 112 };
        out.push_str(concat!(
            r##"<circle cx="812" cy="812" r="112" fill="#000" fill-opacity="0.35"/>"##,
            r##"<circle cx="812" cy="806" r="104" fill="#F4F2EC"/>"##,
        ));
        out.push_str(&format!(
            r##"<text x="812" y="{}" text-anchor="middle" font-family="-apple-system, Helvetica Neue, Helvetica" font-weight="700" font-size="{size}" fill="#111111">{}</text>"##,
            806 + size * 36 / 100,
            escape(&label)
        ));
    }
    out.push_str("</svg>");
    out
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Sets `face` as the Dock icon. Main thread only.
#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
pub(crate) fn show(face: &Face) {
    use objc2::{AllocAnyThread, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSData;
    let Some(marker) = MainThreadMarker::new() else {
        return;
    };
    let data = NSData::with_bytes(svg(face).as_bytes());
    let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) else {
        return;
    };
    // SAFETY: on the main thread, as AppKit requires; the image outlives the call.
    unsafe { NSApplication::sharedApplication(marker).setApplicationIconImage(Some(&image)) };
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn show(_face: &Face) {}

#[cfg(test)]
mod tests {
    use super::*;
    use fractile::{
        Rect,
        app::{UiHudTile, UiHudWorkspace, UiLaunchAtLogin, UiWorkspaceWindows},
    };

    fn snapshot(focused: WorkspaceId, tiles: Vec<Rect>) -> UiAgentSnapshot {
        UiAgentSnapshot {
            accessibility: true,
            automatic: true,
            hud: true,
            launch_at_login: UiLaunchAtLogin::Off,
            active_workspaces: vec![WorkspaceId::ONE, focused],
            focused_workspace: Some(focused),
            windows: vec![UiWorkspaceWindows {
                windows: UiHudWorkspace {
                    workspace: focused,
                    fullscreen: false,
                    tiles: tiles
                        .into_iter()
                        .enumerate()
                        .map(|(key, rect)| UiHudTile {
                            key: key as u64,
                            rect,
                            bundle: None,
                        })
                        .collect(),
                    floating: Vec::new(),
                },
                portrait: false,
            }],
            last_error: None,
        }
    }

    #[test]
    fn the_icon_follows_the_focused_workspace_and_its_layout() {
        let p = HUD_PREVIEW_RECT;
        let half = |left: bool| Rect {
            x: if left { p.x } else { p.x + p.width / 2 },
            y: p.y,
            width: p.width / 2,
            height: p.height,
        };
        let two = face(&snapshot(WorkspaceId::FIVE, vec![half(true), half(false)]));
        assert_eq!(two.workspace, Some(WorkspaceId::FIVE));
        assert_eq!(two.windows.len(), 2);
        // Side by side, filling the area, a gap between them.
        let (a, b) = (two.windows[0], two.windows[1]);
        assert!((a.0 - AREA.0).abs() < 0.5 && (b.0 + b.2 - (AREA.0 + AREA.2)).abs() < 0.5);
        assert!((b.0 - (a.0 + a.2) - GAP).abs() < 0.5);
        assert!((a.3 - AREA.3).abs() < 0.5);
        // Nothing tiled: the icon's own split.
        let empty = face(&snapshot(WorkspaceId::TWO, Vec::new()));
        assert_eq!(empty.windows, resting());
        // The SVG carries the first window lit and the number.
        let drawn = svg(&two);
        assert_eq!(drawn.matches("url(#focus)").count(), 1);
        assert!(drawn.contains(">5</text>"));
        assert!(drawn.starts_with("<svg") && drawn.ends_with("</svg>"));
    }
}
