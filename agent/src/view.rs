//! The window: Hex's settings shell. A sidebar of panes under the traffic lights, with the
//! version at its foot, beside a raised panel holding the pane.

use rune::{Anim, Cx, El, ScrollSpec, Tx, col, el, icon, row, text};

use crate::{
    Pane, Ui,
    controls::{self, METRICS, Paint, WINDOW},
    hud, onboarding, panes,
};

/// The traffic lights sit over the sidebar, so the navigation starts below them.
const SIDEBAR_TOP: f32 = 52.0;
/// OpenCode Desktop's settings navigation: compact rows with a 4 pt gap.
const NAV_ROW_GAP: f32 = 4.0;
/// The sidebar's inset, and a navigation item's inside it: icons and the version stamp share
/// the second.
const SIDEBAR_PX: f32 = 8.0;
/// How far a pane body's content dissolves as it scrolls under the header (top) and off the
/// panel's foot (bottom), as Hex's list-and-detail panes do. Resting content sits clear of both.
const SCROLL_FADE: (f32, f32) = (18.0, 28.0);
/// The gap between the header and a pane's first row at rest.
const REST_TOP: f32 = 4.0;

pub fn view(ui: &Ui, cx: &mut Cx<Ui>) -> El {
    let p = Paint::new(cx.system_dark(), cx.env().scale);
    // Menus mount above the page (rune-kit overlays).
    let mut layer = Vec::new();
    let body = ui.pane.render(ui, p, &mut layer);
    let panel = col()
        .key("panel")
        .grow(1.0)
        .shrink()
        .mt(WINDOW.panel_inset)
        .mr(WINDOW.panel_inset)
        .mb(WINDOW.panel_inset)
        .radius(p.k.radius.lg)
        .fill(p.k.c.bg_base)
        .border(p.hair, p.k.c.border_weak)
        .padding(p.hair)
        .clip_soft(0.0)
        .child(pane_header(p, ui.pane.title(), content_max(ui.pane)))
        .children(error_banner(ui, p))
        .child(body);
    let window = row()
        .key("window")
        .role("window")
        .fill(p.k.c.bg_deep)
        .hover_motion_within(Anim::INSTANT, Anim::INSTANT)
        .child(sidebar(ui, p))
        .child(panel);
    panes::sheet(ui, p, cx.is_reduced_motion(), &mut layer);
    if let Some(tour) = onboarding::overlay(ui, p, cx.is_reduced_motion()) {
        layer.push(tour);
    }
    if let Some(hud_preview) = &ui.hud_preview {
        layer.push(hud::stage(hud_preview, p, cx.is_reduced_motion()));
    }
    rune_kit::root(window, layer)
}

fn sidebar(ui: &Ui, p: Paint) -> El {
    let mut nav = col().key("nav").role("list").focus_group().focus_follows();
    for pane in Pane::listed(ui.doc.labs()) {
        nav = nav.child(nav_slot(p, pane, ui.pane == pane));
    }
    col()
        .window_drag()
        .key("sidebar")
        .role("navigation")
        .w(METRICS.sidebar_w)
        .h_full()
        .pt(SIDEBAR_TOP - NAV_ROW_GAP / 2.0)
        .pb(16.0)
        .px(SIDEBAR_PX)
        .child(nav)
        .child(el().key("spacer").grow(1.0))
        .child(
            crate::brand::lockup(p, 15.0)
                .pl(SIDEBAR_PX)
                .pb(6.0)
                .a11y_hidden(),
        )
        .child(
            text(format!("Version {}", fractile::build_version()))
                .key("version")
                .font(p.tabular(p.meta()))
                .color(p.faint())
                .pl(SIDEBAR_PX),
        )
}

/// Slots tile the item column, each reaching halfway into the gaps around its item, so a press
/// anywhere in the column selects the nearest item.
fn nav_slot(p: Paint, pane: Pane, on: bool) -> El {
    col()
        .key(format!("{}-slot", pane.key()))
        .py(NAV_ROW_GAP / 2.0)
        .hover_group()
        .on_drag(move |ui: &mut Ui, d: &mut rune::Drag, _: &mut Tx| {
            if d.phase == rune::Phase::Down {
                ui.select_pane(pane);
            }
        })
        .on_press(move |ui: &mut Ui| ui.select_pane(pane))
        .child(nav_item(p, pane, on))
}

fn nav_item(p: Paint, pane: Pane, on: bool) -> El {
    let c = &p.k.c;
    let mut item = row()
        .key(pane.key())
        .role("button")
        .label(pane.title())
        .h(METRICS.row_h_compact)
        .px(SIDEBAR_PX)
        .gap(8.0)
        .center_y()
        .radius(p.k.radius.sm)
        .press(c.pressed)
        .on_press(move |ui: &mut Ui| ui.select_pane(pane))
        .child(
            el().key("hover")
                .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
                .radius(p.k.radius.sm)
                .fill(c.hover)
                .group_opacity(0.0, 1.0),
        )
        .child(icon(pane.icon(), METRICS.icon).key("icon").color(if on {
            c.icon_base
        } else {
            c.icon_muted
        }))
        .child(
            text(pane.title())
                .key("label")
                .font(p.row_title())
                .color(if on { c.text_base } else { c.text_muted })
                .truncate(),
        );
    if on {
        item = item.fill(c.layer_02);
    }
    item.focusable(true)
        .focus_selected(on)
        .focus_ring(2.0, -2.0, c.border_focus)
}

/// The panel's content column: as wide as the panel until the cards reach `content_max`, then
/// centred. The header, the error banner and the pane body all sit in one, so the title stays on
/// the cards' text rail at every window size.
fn column(key: &str, max: f32) -> El {
    col()
        .key(key)
        .w_full()
        .max_w(max + 2.0 * METRICS.card_edge())
        .self_center()
}

/// How wide `pane`'s cards get: `content_max`, except Screens, whose arrangement draws every
/// display to scale and uses all the room a bigger window gives it.
pub fn content_max(pane: Pane) -> f32 {
    match pane {
        Pane::Rules => SCREENS_MAX,
        _ => METRICS.content_max,
    }
}

/// The widest Screens gets: room for a desk of three displays at a size worth reading.
const SCREENS_MAX: f32 = 1600.0;

/// The panel's title bar, which also moves the window. The title sits on the text rail.
fn pane_header(p: Paint, title: &str, max: f32) -> El {
    col()
        .window_drag()
        .key("pane-header")
        .h(METRICS.header_h)
        .child(
            column("rail", max).h_full().child(
                row()
                    .key("title-row")
                    .h_full()
                    .px(METRICS.gutter)
                    .center_y()
                    .child(
                        text(title)
                            .key("pane-title")
                            .role("heading")
                            .level(1)
                            .font(p.title())
                            .color(p.text())
                            .shrink()
                            .truncate(),
                    ),
            ),
        )
}

/// A save that failed, a config file Fractile can't read, or an agent error such as
/// unavailable hotkeys, which used to be visible only through the control socket.
fn error_banner(ui: &Ui, p: Paint) -> Option<El> {
    let agent_error = ui
        .agent
        .as_ref()
        .and_then(|agent| agent.last_error.as_deref());
    let copy = ui
        .error
        .as_deref()
        .or(ui.doc.error.as_deref())
        .or(agent_error)?;
    // A card-shaped notice on the card edge, its copy on the text rail.
    Some(
        column("error-rail", content_max(ui.pane))
            .px(METRICS.card_edge())
            .pb(8.0)
            .child(
                row()
                    .key("error")
                    .px(METRICS.row_px)
                    .py(8.0)
                    .gap(8.0)
                    .center_y()
                    .radius(p.k.radius.lg)
                    .fill(p.k.c.danger.bg)
                    .child(
                        text(copy)
                            .key("copy")
                            .font(p.description())
                            .color(p.text())
                            .wrap()
                            .grow(1.0)
                            .shrink(),
                    )
                    .child(controls::status_dot(p.k.c.danger.fg)),
            ),
    )
}

/// A vertically scrolling pane body on the page's rails: cards at the card edge, capped at
/// `content_max` and centred past it. Its content fades out under the header and at the
/// panel's foot instead of clipping on a straight line. The body reaches up under the header by
/// the fade's extra height, and its inset matches the fade, so resting content keeps its place
/// and only content scrolled into either edge dissolves.
pub fn scroll_body(key: &str, content: El) -> El {
    scroll_body_max(key, content, METRICS.content_max)
}

/// `scroll_body` whose cards get up to `max` wide.
pub fn scroll_body_max(key: &str, content: El, max: f32) -> El {
    let (top, bottom) = SCROLL_FADE;
    col()
        .key(key)
        .grow(1.0)
        .shrink()
        .min_h(0.0)
        .mt(REST_TOP - top)
        .clip_fade(0.0, top, 0.0, bottom)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .child(
            column("inset", max)
                .px(METRICS.card_edge())
                .pt(top)
                .pb(bottom)
                // The Rules map lays itself out from the cards' width.
                .on_layout(|ui: &mut Ui, [_, _, width, _]| {
                    ui.card_w = width - 2.0 * METRICS.card_edge();
                })
                .child(content),
        )
}
