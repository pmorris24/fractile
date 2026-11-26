//! Drawing the arrangement panel, and dragging a workspace card to another screen. A card holds
//! its number, Add, and the apps whose new windows open there. Its apps drag with the map's own app drag (`crate::map::drag`) and
//! take the same right-click menu, so they drop on any workspace card or on the float box.

use rune::{Cursor, El, Tx, el, text};
use spaceman::WorkspaceId;

use super::{
    CARD_PAD, Frame, INSET, MENU_BAR, Monitor, NAME_H, PAD_BOTTOM, Panel, RULE_ICON, rule_offset,
    rule_room,
};
use crate::{
    Ui,
    map::{HEAD_GAP, HEAD_H, MapView, SLOP},
    panes::workspaces as screens,
    rules::Place,
};

/// The panel's top in the map: under the map's heading.
pub(in crate::map) const TOP: f32 = HEAD_H + HEAD_GAP;
const HINT: &str = "Drag a workspace to another screen, or an app onto a workspace";

/// A workspace card being dragged to another screen.
#[derive(Clone, Debug, PartialEq)]
pub struct ScreenDrag {
    pub(in crate::map) workspace: WorkspaceId,
    /// The card's top-left in the map, and its size.
    origin: (f32, f32),
    size: (f32, f32),
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
    size: (f32, f32),
    panel: &Panel,
) {
    match phase {
        rune::Phase::Down => {
            ui.menu = None;
            ui.screen_drag = Some(ScreenDrag {
                workspace,
                origin,
                size,
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
    let (x, y, w, h) = panel.card(WorkspaceId::TWO)?;
    let to = panel.monitors.iter().position(|m| !m.is_main)?;
    let (tx, ty, tw, th) = panel.monitors[to].frame;
    Some(ScreenDrag {
        workspace: WorkspaceId::TWO,
        origin: (x, y + TOP),
        size: (w, h),
        grab: (w / 2.0, h / 2.0),
        delta: (tx + tw / 2.0 - x - w, ty + th / 2.0 - y - h),
        moved: true,
        target: Some(to),
    })
}

impl MapView<'_> {
    /// The panel: every screen where it sits, its workspace cards, and the hint under them.
    pub(in crate::map) fn arrangement(self, layer: &mut Vec<El>) -> El {
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
            out = out.child(self.monitor(index, monitor, layer));
        }
        out.child(
            text(HINT)
                .key("hint")
                .font(self.p.meta())
                .color(self.p.faint())
                .absolute(INSET + CARD_PAD, panel.height - PAD_BOTTOM / 2.0 - 7.0),
        )
    }

    fn monitor(self, index: usize, monitor: &Monitor, layer: &mut Vec<El>) -> El {
        let c = &self.p.k.c;
        let (x, y, w, h) = monitor.frame;
        let target = self
            .ui
            .screen_drag
            .as_ref()
            .filter(|drag| drag.moved)
            .is_some_and(|drag| drag.target == Some(index));
        let border = if target {
            (2.0, c.border_focus)
        } else {
            (self.p.hair, c.border_base)
        };
        let mut out = el()
            .key(format!("arrange-{}", monitor.key))
            .role("group")
            .label(format!("{}: {}", monitor.title, monitor.caption))
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
                    .help(format!("{} · {}", monitor.title, monitor.caption))
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
            out = out.child(
                self.card(*workspace, *frame, layer)
                    .absolute(local.0, local.1),
            );
        }
        out
    }

    /// A workspace's card. Dragged by its body it moves the workspace to another screen; its
    /// apps drag on their own and Add adds one.
    fn card(self, workspace: WorkspaceId, (fx, fy, w, h): Frame, layer: &mut Vec<El>) -> El {
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
            .card_face((w, h))
            .key(format!("card-{workspace}"))
            .role("group")
            .label(format!("Workspace {workspace}"))
            .help("Drag to another screen")
            .fill(if app_target { c.layer_03 } else { c.layer_01 })
            .border(border.0, border.1)
            .cursor(Cursor::Grab)
            .hover_group()
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
                    (w, h),
                    &panel,
                );
            })
            .child(
                self.screen_menu(workspace, active, layer)
                    .absolute(1.0, 1.0),
            )
            .child(
                self.add_button(Place::Workspace(workspace), layer)
                    .absolute(w - 24.0, 2.0),
            );
        self.card_apps(card, workspace, (w, h), Some(layer))
    }

    /// The apps whose new windows open on `workspace`, in rows filling the card under its
    /// number, with a count of the rest when they outgrow it. With the menu `layer` they drag
    /// and take their menu; without it (the card following the pointer) they are only drawn.
    fn card_apps(
        self,
        mut card: El,
        workspace: WorkspaceId,
        size: (f32, f32),
        mut layer: Option<&mut Vec<El>>,
    ) -> El {
        let c = &self.p.k.c;
        let rules = self.layout.rules_for(workspace);
        let room = rule_room(size);
        for (slot, rule) in rules.iter().enumerate() {
            let at = rule_offset(slot, size.0);
            if rules.len() > room && slot == room - 1 {
                card = card.child(
                    el().key("more")
                        .size(RULE_ICON, RULE_ICON)
                        .radius(self.p.k.radius.sm)
                        .fill(c.layer_03)
                        .center()
                        .help(
                            rules[slot..]
                                .iter()
                                .map(|rule| self.rule_label(rule))
                                .collect::<Vec<_>>()
                                .join(", "),
                        )
                        .child(
                            text(format!("+{}", rules.len() - slot))
                                .font(self.p.tabular(self.p.tag()))
                                .color(self.p.muted()),
                        )
                        .absolute(at.0, at.1),
                );
                break;
            }
            let icon = self.app_icon(rule, RULE_ICON);
            let icon = match layer.as_deref_mut() {
                Some(layer) => self.draggable(rule, icon, layer),
                None => icon,
            };
            card = card.child(icon.absolute(at.0, at.1));
        }
        card
    }

    /// A card's shape, shared by the card and the one following the pointer.
    fn card_face(self, (w, h): (f32, f32)) -> El {
        el().size(w, h).radius(self.p.k.radius.md)
    }

    /// The dragged workspace card, following the pointer.
    pub(in crate::map) fn card_ghost(self, drag: &ScreenDrag) -> Option<El> {
        if !drag.moved {
            return None;
        }
        let c = &self.p.k.c;
        let mut ghost = self
            .card_face(drag.size)
            .key("card-ghost")
            .fill(c.layer_02)
            .border(1.5, c.border_focus)
            .cursor(Cursor::Grabbing)
            .child(
                text(drag.workspace.to_string())
                    .font(self.p.tabular(self.p.strong()))
                    .color(self.p.text())
                    .absolute(CARD_PAD + 2.0, 4.0),
            )
            .absolute(drag.origin.0 + drag.delta.0, drag.origin.1 + drag.delta.1);
        ghost = self.card_apps(ghost, drag.workspace, drag.size, None);
        for shadow in &self.p.k.elevation.floating {
            ghost = ghost.box_shadow(*shadow);
        }
        Some(ghost)
    }
}
