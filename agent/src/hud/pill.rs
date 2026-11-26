//! The HUD capsule and how it moves: one row of workspaces with a marker on the one you are on.
//! Everything springs from where it is drawn, so a second switch mid-flight turns the motion
//! around instead of restarting it. The two styles differ only in their cells
//! (`grow.rs`, `icons.rs`); the capsule, marker, overflow and pose are shared here.
//!
//! | What changes                     | What moves                                            | Spring (duration / bounce) |
//! | -------------------------------- | ----------------------------------------------------- | -------------------------- |
//! | The HUD appears                  | fades in, scales from .96 and rises 6 pt              | .10 / 0                    |
//! | The HUD leaves                   | the same path back                                    | .105 / 0                   |
//! | Another workspace                | the marker's leading edge, then its trailing edge     | .16 / 0, .20 / 0           |
//! |                                  | Layouts: the cells grow and shrink, neighbours slide  | .12 / 0                    |
//! |                                  | Icons: the groups brighten and dim                    | .095 / 0                   |
//! | Shown again after hiding         | the marker starts on the workspace you left and       | as above                   |
//! |                                  | travels while the capsule fades in                    |                            |
//! | A workspace joins or leaves      | it fades and scales in or out in place                | settle                     |
//! | A layout changes                 | each tile springs to its new frame                    | .12 / 0                    |
//! | A window moves                   | its icon leaves one workspace for the other           | settle                     |
//! | More workspaces than fit         | the row scrolls to keep yours in view, edges fade     | .12 / 0                    |
//!
//! Under Reduce Motion nothing travels or scales; the row cross-fades instead.

use std::collections::{BTreeSet, HashMap};

use rune::{Color, El, Pixels, Spring, Transition, col, el, row, text, tok::motion::pair::SETTLE};
use spaceman::{
    WorkspaceId,
    app::{UiHudDisplay, UiHudToast, UiHudWorkspace},
    config::{HudPosition, HudStyle},
};

use super::{grow, icons};
use crate::controls::Paint;

pub const PILL_HEIGHT: f32 = 44.0;
/// The capsule inside its border, where the row is laid out.
pub const INNER: f32 = PILL_HEIGHT - 2.0;
/// How far the capsule's shadow reaches past it: the lowest layer's offset plus its blur.
pub const SHADOW_REACH: f32 = 48.0;
/// How far the capsule rests from the display edge it sits on, clear of the Dock below and
/// the menu bar above.
pub const CAPSULE_INSET: f32 = 60.0;
/// Room each side of the capsule for its shadow's widest blur.
pub const SHADOW_SIDE: f32 = 32.0;

/// The capsule answering a shortcut, crisp both ways.
const ENTER: Spring = Spring::new(0.10, 0.0);
const EXIT: Spring = Spring::new(0.105, 0.0);
const REST_SCALE: f32 = 0.96;
const DROP: f32 = 6.0;
/// The marker's leading edge, quicker so it stretches toward where it is going, and its
/// trailing edge catching up; with nowhere to go, one spring for both. A quicker lead fails the
/// motion gate's jerk check when a fast switch turns the marker around.
const MARKER_LEAD: Spring = Spring::new(0.16, 0.0);
const MARKER_TRAIL: Spring = Spring::new(0.20, 0.0);
const MARKER: Spring = Spring::new(0.115, 0.0);
/// The row's own layout: scrolling, and the capsule's width.
pub const ROW: Spring = Spring::new(0.12, 0.0);
/// The fade at an edge the row is cut at, and at an edge it fits inside.
const CLIPPED_FADE: f32 = 24.0;
const EDGE_FADE: f32 = 6.0;

/// App icons by bundle ID, looked up before the view runs.
pub type Icons = HashMap<String, Pixels>;

/// One build of a transient status toast capsule.
#[derive(Clone, Copy, Debug)]
pub struct ToastView<'a> {
    pub toast: &'a UiHudToast,
    /// Up, or on its way out.
    pub shown: bool,
    /// Reduce Motion: fades in place with no scale or drop.
    pub still: bool,
}

/// One build of the capsule: what it shows and how it got here.
#[derive(Clone, Copy, Debug)]
pub struct HudView<'a> {
    pub row: &'a UiHudDisplay,
    /// Up, or on its way out.
    pub shown: bool,
    /// Just shown: everything lands where it is drawn, with the marker on `focus`.
    pub fresh: bool,
    /// Where the marker sits: the visible workspace, or while fresh the one a switch left.
    pub focus: Option<WorkspaceId>,
    /// Reduce Motion: it fades where it rests, with no scale or drop to land in one frame.
    pub still: bool,
    /// Empty workspaces visited during this HUD show, kept in the row until it hides so rapid
    /// flicks across empty workspaces never wobble the capsule width underneath the marker.
    pub visited_empty: &'a BTreeSet<WorkspaceId>,
}

impl HudView<'_> {
    /// The row's workspaces, plus any empty workspace visited during this show and the one a
    /// fresh marker starts on, so the marker glides smoothly without the capsule wobbling.
    #[must_use]
    pub fn entries(&self) -> Vec<UiHudWorkspace> {
        // Where `workspace` sits in the row's listing order; an empty `order` is 1-10, then A-Z.
        let rank = |workspace: WorkspaceId| {
            let position = self
                .row
                .order
                .iter()
                .position(|&listed| listed == workspace);
            (position.unwrap_or(usize::MAX), workspace)
        };
        let mut entries = self.row.workspaces.clone();
        for &workspace in self.visited_empty.iter().chain(self.focus.iter()) {
            if !entries.iter().any(|entry| entry.workspace == workspace) {
                let at = entries.partition_point(|entry| rank(entry.workspace) < rank(workspace));
                entries.insert(
                    at,
                    UiHudWorkspace {
                        workspace,
                        fullscreen: false,
                        tiles: Vec::new(),
                        floating: Vec::new(),
                    },
                );
            }
        }
        entries
    }
}

/// A style's row: cells laid out from `x = 0` (from `y = 0` in a side's column), and where the
/// marker goes.
pub struct Track {
    /// Its length along the row: its width, or its height in a column.
    pub width: f32,
    /// The marker's left edge, top and size, when the focus is in the row.
    pub marker: Option<[f32; 4]>,
    /// Corner radius of the marker (concentric pill for Icons, rounded rect for Layouts).
    pub marker_radius: f32,
    pub cells: Vec<El>,
}

/// The widest the row gets before it scrolls, for a panel `panel_width` wide.
#[must_use]
pub fn max_row_width(panel_width: f32) -> f32 {
    panel_width - 2.0 * SHADOW_SIDE - 2.0
}

/// Where the capsule starts when it is about to appear, as an `(x, y)` offset: below the bottom
/// edge it rises from, above the top edge it drops from, or out past the side it slides in from.
/// Shown (or still) it rests where it is drawn.
#[must_use]
pub const fn enter_offset(position: HudPosition, shown: bool, still: bool) -> (f32, f32) {
    if shown || still {
        return (0.0, 0.0);
    }
    match position {
        HudPosition::Bottom => (0.0, DROP),
        HudPosition::Top => (0.0, -DROP),
        HudPosition::Left => (-DROP, 0.0),
        HudPosition::Right => (DROP, 0.0),
    }
}

/// Non-activating transient status toast pill (pause / resume feedback).
pub fn toast_slot(view: &ToastView<'_>, position: HudPosition, p: Paint) -> El {
    let dark = Paint::new(true, 1.0 / p.hair);
    let (spring, opacity) = if view.shown {
        (ENTER, 1.0)
    } else {
        (EXIT, 0.0)
    };
    let scale = if view.shown || view.still {
        1.0
    } else {
        REST_SCALE
    };
    // A toast is a line of text: on a side it shows along the bottom.
    let position = if position.is_side() {
        HudPosition::Bottom
    } else {
        position
    };
    let (_, drop) = enter_offset(position, view.shown, view.still);
    let indicator = if view.toast.paused {
        let bar = |key: &'static str| {
            el().key(key)
                .size(2.5, 10.0)
                .radius(1.0)
                .fill(rgba(0xffffff, 0.55))
        };
        row()
            .key("hud-toast-pause")
            .gap(2.5)
            .center_y()
            .child(bar("hud-toast-pause-l"))
            .child(bar("hud-toast-pause-r"))
    } else {
        el().key("hud-toast-active")
            .size(7.0, 7.0)
            .radius(3.5)
            .fill(dark.k.c.success.fg)
    };
    let mut capsule = row()
        .key("hud-toast-pill")
        .h(PILL_HEIGHT)
        .px(16.0)
        .gap(10.0)
        .center_y()
        .radius(PILL_HEIGHT / 2.0)
        .fill(srgb(0x161618))
        .border(p.hair, srgb(0x303036))
        .child(indicator)
        .child(
            text(&view.toast.message)
                .key("hud-toast-message")
                .font(rune::TextStyle::new(13.0, 500.0))
                .color(rgba(0xffffff, 0.92)),
        );
    for shadow in &dark.k.elevation.overlay {
        capsule = capsule.box_shadow(*shadow);
    }
    row()
        .key("hud-toast-slot")
        .w_full()
        .h(PILL_HEIGHT)
        .center()
        .opacity(opacity)
        .opacity_motion(spring)
        .scale(scale)
        .scale_motion(spring)
        .offset(0.0, drop)
        .offset_motion(spring)
        .child(capsule)
}

/// Passive capsule used by Playground previews (no `HudOverlay` event handlers).
pub fn slot(
    view: &HudView,
    position: HudPosition,
    style: HudStyle,
    icons: &Icons,
    panel_width: f32,
    p: Paint,
) -> El {
    render_slot(view, position, style, icons, panel_width, p, false)
}

/// Interactive capsule used by the live HUD overlay and its 120 Hz tests: marks the capsule as a
/// hit region, holds on hover, switches workspaces on click, and opens the per-app rule menu
/// on secondary click.
pub fn interactive_slot(
    view: &HudView,
    position: HudPosition,
    style: HudStyle,
    icons: &Icons,
    panel_width: f32,
    p: Paint,
) -> El {
    render_slot(view, position, style, icons, panel_width, p, true)
}

fn render_slot(
    view: &HudView,
    position: HudPosition,
    style: HudStyle,
    icons: &Icons,
    panel_width: f32,
    p: Paint,
    interactive: bool,
) -> El {
    let (spring, opacity) = if view.shown {
        (ENTER, 1.0)
    } else {
        (EXIT, 0.0)
    };
    let scale = if view.shown || view.still {
        1.0
    } else {
        REST_SCALE
    };
    let (dx, dy) = enter_offset(position, view.shown, view.still);
    let entries = view.entries();
    let side = position.is_side();
    let track = match style {
        // A side's column draws Icons: Layouts' screens are wider than the column.
        HudStyle::Layouts if !side => grow::track_with_mode(
            &entries,
            view.focus,
            view.row.portrait,
            icons,
            p,
            interactive,
        ),
        HudStyle::Layouts | HudStyle::Icons => {
            icons::track_with_mode(&entries, view.focus, icons, p, interactive, side)
        }
    };
    let slot = if side {
        col().key("hud-slot").h_full().w(PILL_HEIGHT)
    } else {
        row().key("hud-slot").w_full().h(PILL_HEIGHT)
    };
    slot.center()
        .opacity(opacity)
        .opacity_motion(spring)
        .scale(scale)
        .scale_motion(spring)
        .offset(dx, dy)
        .offset_motion(spring)
        .child(capsule(
            view,
            track,
            max_row_width(panel_width),
            p,
            interactive,
            side,
        ))
}

/// The capsule round the row; `side` stands it on end, a column down a side of the display,
/// where `max_width` is how tall it may grow.
fn capsule(
    view: &HudView,
    track: Track,
    max_width: f32,
    p: Paint,
    interactive: bool,
    side: bool,
) -> El {
    let dark = Paint::new(true, 1.0 / p.hair);
    let width = track.width.min(max_width);
    let overflow = track.width - width;
    // Keep the marker's workspace in the middle of what shows, as far as the row reaches.
    let scroll = track.marker.map_or(0.0, |[x, y, w, h]| {
        let (at, length) = if side { (y, h) } else { (x, w) };
        (at + length / 2.0 - width / 2.0).clamp(0.0, overflow.max(0.0))
    });
    // Along the row is x, across it y; a column swaps them.
    let along = |el: El, length: f32, across: f32| {
        if side {
            el.w(across).h(length)
        } else {
            el.w(length).h(across)
        }
    };
    let fade = |clipped: bool| if clipped { CLIPPED_FADE } else { EDGE_FADE };
    let mut content = el()
        .key(match (view.still, view.focus) {
            // Under Reduce Motion the whole row cross-fades instead of anything moving.
            (true, Some(focus)) => format!("hud-row-{focus}"),
            _ => "hud-row".to_owned(),
        })
        .absolute(
            if side { 0.0 } else { -scroll },
            if side { -scroll } else { 0.0 },
        )
        .motion(ROW)
        .transition(Transition::opacity().springs(SETTLE));
    content = along(content, track.width, INNER);
    if let Some([x, y, w, h]) = track.marker {
        let marker = el()
            .key(match view.still {
                true => format!(
                    "hud-marker-{}",
                    view.focus.map(|f| f.to_string()).unwrap_or_default()
                ),
                false => "hud-marker".to_owned(),
            })
            .absolute(x, y)
            .w(w)
            .h(h)
            .radius(track.marker_radius)
            .fill(rgba(0xffffff, 0.10))
            .border(p.hair, rgba(0xffffff, 0.16))
            .motion(MARKER)
            .lead(MARKER_LEAD, MARKER_TRAIL)
            .transition(Transition::opacity().springs(SETTLE));
        content = content.child(marker);
    }
    let (lead, trail) = (fade(scroll > 0.5), fade(scroll < overflow - 0.5));
    let viewport = along(el().key("hud-view"), width, INNER)
        .motion(ROW)
        .clip_fade(
            if side { 0.0 } else { lead },
            if side { lead } else { 0.0 },
            if side { 0.0 } else { trail },
            if side { trail } else { 0.0 },
        )
        .child(content.children(track.cells));
    let mut capsule = along(row().key("hud-pill"), width + 2.0, PILL_HEIGHT)
        .center()
        .radius(PILL_HEIGHT / 2.0)
        .fill(srgb(0x161618))
        .border(p.hair, srgb(0x303036))
        .motion(ROW)
        .child(viewport);
    if interactive {
        capsule = capsule
            .hit()
            .on_hover_change(|overlay: &mut super::HudOverlay, inside, tx| {
                overlay.set_hovered(inside, tx);
            });
    }
    if view.fresh {
        // Just shown: the row lands as drawn, inside the capsule's entrance.
        capsule = capsule.instant_changes();
    }
    for shadow in &dark.k.elevation.overlay {
        capsule = capsule.box_shadow(*shadow);
    }
    capsule
}

/// How a workspace joins or leaves the row: in place, with a fade and a little scale.
pub fn presence() -> Transition {
    Transition::scale(0.9).springs(SETTLE)
}

/// An app's icon at `size`, or its initial in a rounded square when it has none here.
pub fn app_icon(
    icons: &Icons,
    workspace: WorkspaceId,
    bundle: &str,
    size: f32,
    interactive: bool,
) -> El {
    let mut base = el()
        .key(format!("app-{bundle}"))
        .size(size, size)
        .transition(presence());
    base = match icons.get(bundle) {
        Some(pixels) => base.pixels(pixels),
        None => {
            let initial = crate::apps::fallback_name(bundle)
                .chars()
                .next()
                .map(String::from)
                .unwrap_or_default();
            base.radius(size * 0.25)
                .fill(rgba(0xffffff, 0.16))
                .center()
                .child(
                    text(initial)
                        .font(rune::TextStyle::new((size * 0.6).max(6.0), 600.0))
                        .color(rgba(0xffffff, 0.7)),
                )
        }
    };
    if interactive {
        let hover_bundle = bundle.to_owned();
        let menu_bundle = bundle.to_owned();
        base = base
            .on_hover_change(move |overlay: &mut super::HudOverlay, inside, _| {
                overlay.set_hovered_app(workspace, &hover_bundle, inside);
            })
            .on_secondary_press(move |overlay: &mut super::HudOverlay, press, tx| {
                overlay.open_app_menu(workspace, &menu_bundle, (press.x, press.y), tx);
            });
    }
    base
}

pub fn srgb(hex: u32) -> Color {
    rgba(hex, 1.0)
}

pub fn rgba(hex: u32, alpha: f32) -> Color {
    let channel = |shift: u32| rune::color::srgb_decode(((hex >> shift) & 0xff) as f32 / 255.0);
    Color {
        r: channel(16),
        g: channel(8),
        b: channel(0),
        a: alpha,
    }
}
