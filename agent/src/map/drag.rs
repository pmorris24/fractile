//! Dragging an app on the Rules map: from the press, through the slop that tells a drag from a
//! click, to the drop that rewrites its rule. Dropped on a workspace, the app's open windows go
//! there too. An open window's icon with no rule behind it drags as a rule-to-be
//! (`window_rule`): its drop writes one.

use fractile::WorkspaceId;

use super::{Layout, SLOP, TILE_H};
use crate::{
    Ui,
    rules::{Doc, Place, Rule},
};

/// Where the rule-to-be of an open window's icon numbers itself: past any real rule.
pub(super) const WINDOW_BASE: usize = usize::MAX / 2;

/// The rule an open window's icon drags as: its app sent to `workspace`, numbered by the
/// icon's `slot` among the map's so no two icons share one and none is a real rule.
pub(super) fn window_rule(bundle: &str, workspace: WorkspaceId, slot: usize) -> Rule {
    Rule {
        index: WINDOW_BASE + slot,
        bundle_id: Some(bundle.to_owned()),
        title: None,
        title_contains: None,
        place: Place::Workspace(workspace),
        all_windows: false,
    }
}

/// An app being dragged: its rule, where it started and how far the pointer has gone.
#[derive(Clone, Debug, PartialEq)]
pub struct Drag {
    pub rule: usize,
    pub(super) snapshot: Rule,
    /// The dragged item's top-left in the map, and its size.
    pub(super) origin: (f32, f32),
    pub(super) size: (f32, f32),
    /// Where in the item the press landed.
    pub(super) grab: (f32, f32),
    pub(super) delta: (f32, f32),
    pub(super) moved: bool,
    pub(super) target: Option<Place>,
}

impl Drag {
    /// Slack mid-drag over workspace 4, for stills.
    pub(super) fn fixture(doc: &Doc, width: f32) -> Option<Drag> {
        let layout = Layout::new(doc, width);
        let rule = doc
            .rules()
            .into_iter()
            .find(|rule| rule.bundle_id.as_deref() == Some("com.tinyspeck.slackmacgap"))?;
        let (origin, size) = layout.spot(&rule)?;
        let grab = (size.0 / 2.0, size.1 / 2.0);
        let to = layout.grid.tile_origin(3);
        let tile_w = layout.grid.tile_w();
        let delta = (
            to.0 + tile_w / 2.0 - origin.0 - grab.0,
            to.1 + TILE_H / 2.0 - origin.1 - grab.1,
        );
        Some(Drag {
            rule: rule.index,
            snapshot: rule,
            origin,
            size,
            grab,
            delta,
            moved: true,
            target: layout
                .grid
                .target_at(to.0 + tile_w / 2.0, to.1 + TILE_H / 2.0),
        })
    }
}

pub(super) fn drag(
    ui: &mut Ui,
    d: &rune::Drag,
    rule: &Rule,
    origin: (f32, f32),
    size: (f32, f32),
    target_at: impl Fn(f32, f32) -> Option<Place>,
) {
    drag_step(
        ui,
        d.phase,
        (d.start_x, d.start_y),
        (d.dx, d.dy),
        d.cancelled(),
        rule,
        origin,
        size,
        target_at,
    );
}

#[allow(clippy::too_many_arguments)]
pub(super) fn drag_step(
    ui: &mut Ui,
    phase: rune::Phase,
    start: (f32, f32),
    delta: (f32, f32),
    cancelled: bool,
    rule: &Rule,
    origin: (f32, f32),
    size: (f32, f32),
    target_at: impl Fn(f32, f32) -> Option<Place>,
) {
    match phase {
        rune::Phase::Down => {
            ui.menu = None;
            ui.drag = Some(Drag {
                rule: rule.index,
                snapshot: rule.clone(),
                origin,
                size,
                grab: start,
                delta: (0.0, 0.0),
                moved: false,
                target: None,
            });
        }
        rune::Phase::Move => {
            let Some(drag) = ui.drag.as_mut().filter(|drag| drag.rule == rule.index) else {
                return;
            };
            drag.delta = delta;
            drag.moved |= delta.0.abs().max(delta.1.abs()) >= SLOP;
            drag.target = target_at(
                origin.0 + drag.grab.0 + delta.0,
                origin.1 + drag.grab.1 + delta.1,
            );
        }
        rune::Phase::Up => {
            let Some(drag) = ui.drag.take().filter(|drag| drag.rule == rule.index) else {
                return;
            };
            if cancelled || !drag.moved {
                ui.flush_pending_reload();
                return;
            }
            let Some(target) = drag.target else {
                ui.flush_pending_reload();
                return;
            };
            let current = drag.snapshot.place;
            let unchanged = current == target
                || (target == Place::Float && matches!(current, Place::Ignore | Place::Pin));
            if unchanged {
                ui.flush_pending_reload();
                return;
            }
            let bundle = drag.snapshot.bundle_id.clone();
            if drag.rule >= WINDOW_BASE {
                let Some(bundle) = &bundle else {
                    ui.flush_pending_reload();
                    return;
                };
                ui.edit(|doc| doc.add_rule(bundle, None, target));
            } else {
                ui.try_edit(|doc| doc.place_matching(&drag.snapshot, target));
            }
            // The app's windows open now follow it; a title rule's do not, as it sends only
            // some of them.
            if let (Place::Workspace(workspace), Some(bundle), true) =
                (target, bundle, drag.snapshot.app_wide())
            {
                ui.move_app(&bundle, workspace);
            }
        }
    }
}
