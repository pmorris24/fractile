//! Drawing the arrangement panel, and dragging a workspace card to another screen. Apps in the
//! cards drag with the map's own app drag (`super::super::drag`), so they drop on any workspace
//! card, here or in the screens' sections below, or on the float box.

use rune::{Cursor, El, Tx, el, text};
use spaceman::WorkspaceId;

use super::{CARD_GAP, Frame, INSET, MENU_BAR, Monitor, NAME_H, PAD_BOTTOM, Panel};
use crate::{
    Ui,
    map::{HEAD_GAP, HEAD_H, MapView, SLOP, drag},
    panes::workspaces as screens,
    rules::{Place, Rule},
};

/// The panel's top in the map: under the map's heading.
pub(in crate::map) const TOP: f32 = HEAD_H + HEAD_GAP;
/// Inside a card: its number's line, and its app icons' size and spacing.
const NUMBER_H: f32 = 18.0;
const CARD_PAD: f32 = 5.0;
const CARD_ICON: f32 = 16.0;
const CARD_ICON_GAP: f32 = 3.0;
const HINT: &str = "Drag a workspace to another screen, or an app onto a workspace";

/// A workspace card being dragged to another screen.
#[derive(Clone, Debug, PartialEq)]
pub struct ScreenDrag {
    pub(in crate::map) workspace: WorkspaceId,
    /// The card's top-left in the map, and its side.
    origin: (f32, f32),
    side: f32,
    grab: (f32, f32),
    delta: (f32, f32),
    moved: bool,
    /// The screen under the pointer, by its index in the panel.
    pub(in crate::map) target: Option<usize>,
}

#[allow(clippy::too_many_arguments)]
pub(in crate::map) fn screen_drag_step(
    ui: &mut Ui,
    phase: rune::Phase,
    start: (f32, f32),
    delta: (f32, f32),
    cancelled: bool,
    workspace: WorkspaceId,
    origin: (f32, f32),
    side: f32,
    panel: &Panel,
) {
    match phase {
        rune::Phase::Down => {
            ui.menu = None;
            ui.screen_drag = Some(ScreenDrag {
                workspace,
                origin,
                side,
                grab: start,
                delta: (0.0, 0.0),
                moved: false,
                target: None,
            });
        }
        rune::Phase::Move => {
            let Some(drag) = ui
                .screen_drag
                .as_mut()
                .filter(|drag| drag.workspace == workspace)
            else {
                return;
            };
            drag.delta = delta;
            drag.moved |= delta.0.abs().max(delta.1.abs()) >= SLOP;
            drag.target = panel.monitor_at(
                origin.0 + drag.grab.0 + delta.0,
                origin.1 + drag.grab.1 + delta.1 - TOP,
            );
        }
        rune::Phase::Up => {
            let Some(drag) = ui
                .screen_drag
                .take()
                .filter(|drag| drag.workspace == workspace)
            else {
                return;
            };
            let to = drag
                .target
                .filter(|_| drag.moved && !cancelled)
                .filter(|to| panel.monitor_of(workspace) != Some(*to))
                .and_then(|to| panel.monitors.get(to));
            if let Some(monitor) = to {
                let selector = screens::override_for(workspace, &monitor.selector);
                ui.edit(|doc| doc.set_monitor(workspace, selector.as_deref()));
            }
            ui.flush_pending_reload();
        }
    }
}

/// Workspace 2's card held over the first screen that is not Main, for stills.
pub(in crate::map) fn fixture_drag(ui: &Ui) -> Option<ScreenDrag> {
    let layout = crate::map::Layout::with_running(&ui.doc, ui.card_w, &ui.displays, &[], &[]);
    let panel = &layout.grid.panel;
    let (_, (x, y, side, _)) = panel
        .monitors
        .iter()
        .flat_map(|m| &m.cards)
        .find(|(ws, _)| *ws == WorkspaceId::TWO)
        .copied()?;
    let to = panel.monitors.iter().position(|m| !m.is_main)?;
    let (tx, ty, tw, th) = panel.monitors[to].frame;
    Some(ScreenDrag {
        workspace: WorkspaceId::TWO,
        origin: (x, y + TOP),
        side,
        grab: (side / 2.0, side / 2.0),
        delta: (tx + tw / 2.0 - x - side, ty + th / 2.0 - y - side),
        moved: true,
        target: Some(to),
    })
}

impl MapView<'_> {
    /// The panel: every screen where it sits, its workspace cards, and the hint under them.
    pub(in crate::map) fn arrangement(self) -> El {
        let c = &self.p.k.c;
        let panel = &self.layout.grid.panel;
        let mut out = el()
            .key("arrangement")
            .role("group")
            .label("Display arrangement")
            .size(panel.width, panel.height)
            .radius(self.p.k.radius.lg)
            .fill(c.layer_01)
            .border(self.p.hair, c.border_weak);
        for (index, monitor) in panel.monitors.iter().enumerate() {
            out = out.child(self.monitor(index, monitor));
        }
        out.child(
            text(HINT)
                .key("hint")
                .font(self.p.meta())
                .color(self.p.faint())
                .absolute(INSET + CARD_GAP, panel.height - PAD_BOTTOM / 2.0 - 7.0),
        )
    }

    fn monitor(self, index: usize, monitor: &Monitor) -> El {
        let c = &self.p.k.c;
        let (x, y, w, h) = monitor.frame;
        let target = self
            .ui
            .screen_drag
            .as_ref()
            .filter(|drag| drag.moved)
            .is_some_and(|drag| drag.target == Some(index));
        let on_view = monitor.cards.iter().any(|(ws, _)| self.active.contains(ws));
        let border = if target {
            (2.0, c.border_focus)
        } else if on_view {
            (1.5, c.border_strong)
        } else {
            (self.p.hair, c.border_base)
        };
        let mut out = el()
            .key(format!("arrange-{}", monitor.key))
            .role("group")
            .label(format!("{}: {}", monitor.title, monitor.caption))
            .help(format!("{} · {}", monitor.title, monitor.caption))
            .size(w, h)
            .radius(self.p.k.radius.md)
            .fill(if target { c.layer_03 } else { c.layer_02 })
            .border(border.0, border.1)
            .absolute(x, y);
        let mut name_top = INSET;
        if monitor.is_main {
            out = out.child(
                el().key("menu-bar")
                    .size(w, MENU_BAR)
                    .fill(c.layer_03)
                    .absolute(0.0, 0.0),
            );
            name_top += MENU_BAR;
        }
        if w > 2.0 * INSET && h > name_top + NAME_H {
            out = out.child(
                text(monitor.title.clone())
                    .key("name")
                    .font(self.p.meta())
                    .color(self.p.muted())
                    .truncate()
                    .w(w - 2.0 * INSET)
                    .absolute(INSET, name_top),
            );
        }
        if monitor.cards.is_empty() && h > name_top + 2.0 * NAME_H {
            out = out.child(
                text("Drop a workspace here")
                    .key("empty")
                    .font(self.p.meta())
                    .color(self.p.faint())
                    .truncate()
                    .w(w - 2.0 * INSET)
                    .absolute(INSET, name_top + NAME_H + 4.0),
            );
        }
        for (workspace, frame) in &monitor.cards {
            let local = (frame.0 - x, frame.1 - y);
            out = out.child(self.card(*workspace, *frame).absolute(local.0, local.1));
        }
        out
    }

    /// A workspace's card: its number and the apps whose new windows open there. Dragged by
    /// its body it moves the workspace to another screen; its apps drag on their own.
    fn card(self, workspace: WorkspaceId, (fx, fy, side, _): Frame) -> El {
        let c = &self.p.k.c;
        let origin = (fx, fy + TOP);
        let dragging = self
            .ui
            .screen_drag
            .as_ref()
            .is_some_and(|drag| drag.workspace == workspace && drag.moved);
        let app_target = self.target == Some(Place::Workspace(workspace));
        let active = self.active.contains(&workspace);
        let border = if app_target {
            (2.0, c.border_focus)
        } else if active {
            (1.5, c.border_strong)
        } else {
            (self.p.hair, c.border_weak)
        };
        let panel = self.layout.grid.panel.clone();
        let card = self
            .card_face(workspace, side, active)
            .key(format!("card-{workspace}"))
            .role("button")
            .label(format!("Workspace {workspace}: drag to another screen"))
            .help("Drag to another screen")
            .fill(if app_target { c.layer_03 } else { c.layer_01 })
            .border(border.0, border.1)
            .cursor(Cursor::Grab)
            .opacity(if dragging { 0.3 } else { 1.0 })
            .on_drag(move |ui: &mut Ui, d: &mut rune::Drag, _: &mut Tx| {
                screen_drag_step(
                    ui,
                    d.phase,
                    (d.start_x, d.start_y),
                    (d.dx, d.dy),
                    d.cancelled(),
                    workspace,
                    origin,
                    side,
                    &panel,
                );
            });
        self.card_apps(card, workspace, side, Some(origin))
    }

    /// The apps whose new windows open on `workspace`, in a card `side` wide. With the card's
    /// `origin` in the map they drag; without one (the card following the pointer) they are
    /// only drawn.
    fn card_apps(
        self,
        mut card: El,
        workspace: WorkspaceId,
        side: f32,
        origin: Option<(f32, f32)>,
    ) -> El {
        let c = &self.p.k.c;
        let rules = self
            .layout
            .grid
            .tiles
            .iter()
            .position(|tile| *tile == workspace)
            .map_or(&[][..], |index| self.layout.routed[index].as_slice());
        let per_row = (((side - 2.0 * CARD_PAD + CARD_ICON_GAP) / (CARD_ICON + CARD_ICON_GAP))
            .floor() as usize)
            .max(1);
        let rows = (((side - NUMBER_H - CARD_PAD + CARD_ICON_GAP) / (CARD_ICON + CARD_ICON_GAP))
            .floor() as usize)
            .max(1);
        let room = per_row * rows;
        for (slot, rule) in rules.iter().enumerate() {
            let (col, row) = (slot % per_row, slot / per_row);
            let at = (
                CARD_PAD + col as f32 * (CARD_ICON + CARD_ICON_GAP),
                NUMBER_H + row as f32 * (CARD_ICON + CARD_ICON_GAP),
            );
            if rules.len() > room && slot == room - 1 {
                card = card.child(
                    el().key("more")
                        .size(CARD_ICON, CARD_ICON)
                        .radius(self.p.k.radius.sm)
                        .fill(c.layer_03)
                        .center()
                        .child(
                            text(format!("+{}", rules.len() - slot))
                                .font(self.p.tabular(self.p.tag()))
                                .color(self.p.muted()),
                        )
                        .absolute(at.0, at.1),
                );
                break;
            }
            let icon = match origin {
                Some(origin) => self.card_icon(rule, (origin.0 + at.0, origin.1 + at.1)),
                None => self.app_icon(rule, CARD_ICON),
            };
            card = card.child(icon.absolute(at.0, at.1));
        }
        card
    }

    /// A card's shape and number, shared by the card and the one following the pointer.
    fn card_face(self, workspace: WorkspaceId, side: f32, active: bool) -> El {
        el().size(side, side).radius(self.p.k.radius.md).child(
            text(workspace.to_string())
                .key("number")
                .font(self.p.tabular(self.p.tag()))
                .color(if active {
                    self.p.text()
                } else {
                    self.p.muted()
                })
                .absolute(CARD_PAD + 1.0, 3.0),
        )
    }

    /// An app in a card: dragged like any app on the map. Its menu lives on the app's icon in
    /// the sections below, so one rule never opens two.
    fn card_icon(self, rule: &Rule, origin: (f32, f32)) -> El {
        let grid = self.layout.grid.clone();
        let drag_rule = rule.clone();
        let dragging = self
            .ui
            .drag
            .as_ref()
            .is_some_and(|drag| drag.rule == rule.index && drag.moved);
        let size = (CARD_ICON, CARD_ICON);
        self.app_icon(rule, CARD_ICON)
            .key(format!("card-rule-{}", rule.index))
            .role("button")
            .label(self.rule_name(rule))
            .help(self.rule_help(rule))
            .cursor(Cursor::Grab)
            .opacity(if dragging { 0.3 } else { 1.0 })
            .on_drag(move |ui: &mut Ui, d: &mut rune::Drag, _: &mut Tx| {
                drag::drag(ui, d, &drag_rule, origin, size, |x, y| grid.target_at(x, y));
            })
    }

    /// The dragged workspace card, following the pointer.
    pub(in crate::map) fn card_ghost(self, drag: &ScreenDrag) -> Option<El> {
        if !drag.moved {
            return None;
        }
        let c = &self.p.k.c;
        let mut ghost = self
            .card_face(drag.workspace, drag.side, true)
            .key("card-ghost")
            .fill(c.layer_02)
            .border(1.5, c.border_focus)
            .cursor(Cursor::Grabbing)
            .absolute(drag.origin.0 + drag.delta.0, drag.origin.1 + drag.delta.1);
        ghost = self.card_apps(ghost, drag.workspace, drag.side, None);
        for shadow in &self.p.k.elevation.floating {
            ghost = ghost.box_shadow(*shadow);
        }
        Some(ghost)
    }
}
