use crate::{
    Model, NativeObservation, PhysicalWindowId, Placement, Reconciliation, ReconciliationEvidence,
    Settings, Slot,
    config::LayoutMode,
    layout_tree::{LayoutTree, Minimums},
    reconcile,
};

/// Each window's known minimum `(width, height)`, learned from size refusals.
pub(crate) type WindowMinimums = std::collections::HashMap<PhysicalWindowId, (i32, i32)>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CycleTicket {
    pub cycle: u64,
    pub revision: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StaleCycle {
    pub ticket: CycleTicket,
    pub current_revision: u64,
}

#[derive(Clone, Debug, Default)]
pub struct Engine {
    model: Model,
    layout: LayoutTree,
    // Only the requested layout above is persisted. The fit is shared by
    // placement, navigation and HUD previews until its inputs change.
    fit: Option<DisplayFit>,
    insertion_focus: Option<crate::SlotId>,
    /// Slots in the order they last had focus, most recent first, kept only in
    /// memory. A native tab keeps its slot, so it keeps its place here.
    focus_order: Vec<crate::SlotId>,
    evidence: ReconciliationEvidence,
    revision: u64,
    next_cycle: u64,
    pending: Option<CycleTicket>,
}

#[derive(Clone, Debug)]
struct DisplayFit {
    output: crate::Rect,
    minimums: std::collections::HashMap<crate::SlotId, (i32, i32)>,
    tree: Option<LayoutTree>,
}

impl Engine {
    pub fn invalidate(&mut self) {
        self.pending = None;
        self.revision = self.revision.saturating_add(1);
    }

    pub fn begin_cycle(&mut self) -> CycleTicket {
        self.next_cycle = self.next_cycle.saturating_add(1);
        let ticket = CycleTicket {
            cycle: self.next_cycle,
            revision: self.revision,
        };
        self.pending = Some(ticket);
        ticket
    }

    pub fn accept(
        &mut self,
        ticket: CycleTicket,
        observation: &NativeObservation,
        settings: &Settings,
    ) -> Result<Reconciliation, StaleCycle> {
        if self.pending != Some(ticket) || observation.cycle != ticket.cycle {
            return Err(StaleCycle {
                ticket,
                current_revision: self.revision,
            });
        }
        self.pending = None;

        let mut result = reconcile(&self.model, observation, &self.evidence, settings);
        let shown = self.fit.as_ref().and_then(|fit| fit.tree.as_ref());
        if self.layout.sync_shown(
            self.layout.root_axis(),
            &result.model.workspace.slots,
            self.insertion_focus,
            settings.layout,
            settings.canvas(),
            shown,
        ) {
            self.fit = None;
            self.insertion_focus = self
                .insertion_focus
                .filter(|&id| result.model.slot(id).is_some());
        }
        result.placements = placements_for(self.display_layout(), &result.model, settings);
        self.evidence.record(observation, &result);
        self.model = result.model.clone();
        self.prune_focus_order();
        Ok(result)
    }

    #[must_use]
    pub const fn model(&self) -> &Model {
        &self.model
    }

    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    #[must_use]
    pub fn bound_windows(&self) -> Vec<PhysicalWindowId> {
        self.layout
            .slots()
            .into_iter()
            .filter_map(|id| self.model.slot(id)?.bound_window().cloned())
            .collect()
    }

    #[must_use]
    pub fn placements(&self, settings: &Settings) -> Vec<Placement> {
        placements_for(self.display_layout(), &self.model, settings)
    }

    pub(crate) fn record_placement_target(&mut self, slot: crate::SlotId, frame: crate::Rect) {
        self.evidence.previous_placements.insert(slot, frame);
    }

    pub(crate) fn toggle_axis(&mut self) {
        self.layout.toggle_root_axis();
        self.commit_layout();
    }

    #[cfg(test)]
    pub(crate) fn take_slot(&mut self, window: &PhysicalWindowId) -> Option<Slot> {
        self.take_slot_with(window, LayoutMode::Columns, crate::Rect::default())
    }

    pub(crate) fn take_slot_with(
        &mut self,
        window: &PhysicalWindowId,
        mode: LayoutMode,
        output: crate::Rect,
    ) -> Option<Slot> {
        let index = self
            .model
            .workspace
            .slots
            .iter()
            .position(|slot| slot.bound_window() == Some(window))?;
        let slot = self.model.workspace.slots.remove(index);
        self.sync_layout(None, mode, output);
        self.evidence.previous_placements.remove(&slot.id);
        self.evidence.previous_frames.remove(&slot.id);
        self.invalidate_layout();
        Some(slot)
    }

    /// Remove every slot, bound or unresolved, whose identity belongs to `app`.
    pub(crate) fn forget_app_with(
        &mut self,
        app: &crate::AppInstanceId,
        mode: LayoutMode,
        output: crate::Rect,
    ) -> bool {
        let before = self.model.workspace.slots.len();
        self.model
            .workspace
            .slots
            .retain(|slot| !slot.belongs_to(app));
        self.model.continuity.remove(app);
        if self.model.workspace.slots.len() == before {
            return false;
        }
        let slots = &self.model.workspace.slots;
        self.evidence
            .previous_placements
            .retain(|id, _| slots.iter().any(|slot| slot.id == *id));
        self.evidence
            .previous_frames
            .retain(|id, _| slots.iter().any(|slot| slot.id == *id));
        self.sync_layout(None, mode, output);
        self.invalidate_layout();
        true
    }

    pub(crate) fn insert_slot(&mut self, slot: Slot) {
        self.insert_slot_with(slot, LayoutMode::Columns, crate::Rect::default(), None);
    }

    pub(crate) fn insert_slot_with(
        &mut self,
        mut slot: Slot,
        mode: LayoutMode,
        output: crate::Rect,
        anchor: Option<crate::SlotId>,
    ) {
        if self
            .model
            .workspace
            .slots
            .iter()
            .any(|existing| existing.id == slot.id)
        {
            slot.id = crate::SlotId(self.model.next_slot_id);
        }
        self.model.next_slot_id = self.model.next_slot_id.max(slot.id.0.saturating_add(1));
        // A tile arriving here has not had focus here, whatever its ID ranked before.
        self.focus_order.retain(|&id| id != slot.id);
        self.model.workspace.slots.push(slot);
        self.sync_layout(anchor, mode, output);
        self.invalidate_layout();
    }

    pub(crate) fn swap_windows(
        &mut self,
        first: &PhysicalWindowId,
        second: &PhysicalWindowId,
    ) -> bool {
        let Some(second) = self.slot_for_window(second) else {
            return false;
        };
        self.edit_requested(first, |layout, first| layout.swap(first, second))
    }

    /// Reads a window the user resized to `current` as the split boundaries it moved.
    pub(crate) fn resize_window(
        &mut self,
        window: &PhysicalWindowId,
        current: crate::Rect,
        settings: &Settings,
        minimums: &WindowMinimums,
    ) -> bool {
        let canvas = settings.canvas();
        let current = settings.spacing.untile(current);
        self.edit_weights(window, settings, minimums, |layout, slot, floors| {
            layout.resize_within(slot, current, canvas, floors)
        })
    }

    /// The slot a newly adopted window is inserted next to.
    pub(crate) const fn insertion_focus(&self) -> Option<crate::SlotId> {
        self.insertion_focus
    }

    pub(crate) fn remember_insertion_focus(&mut self, window: &PhysicalWindowId) {
        self.insertion_focus = self.slot_for_window(window);
        if let Some(slot) = self.insertion_focus {
            self.focus_order.retain(|&id| id != slot);
            self.focus_order.insert(0, slot);
        }
        self.prune_focus_order();
    }

    /// Drops closed or moved-out tiles, so an ID reused later starts unranked.
    fn prune_focus_order(&mut self) {
        let model = &self.model;
        self.focus_order.retain(|&id| model.slot(id).is_some());
    }

    /// How recently `window`'s tile had focus: 0 for the most recent, None for
    /// a tile not focused since Fractile started (or not on this workspace).
    #[must_use]
    pub(crate) fn focus_recency(&self, window: &PhysicalWindowId) -> Option<usize> {
        let slot = self.slot_for_window(window)?;
        self.focus_order.iter().position(|&id| id == slot)
    }

    fn display_layout(&self) -> &LayoutTree {
        self.fit
            .as_ref()
            .and_then(|fit| fit.tree.as_ref())
            .unwrap_or(&self.layout)
    }

    /// Re-evaluate even without a new refusal: a removed sibling or a larger
    /// display can make the original split usable again.
    fn slot_minimums(
        &self,
        settings: &Settings,
        minimums: &std::collections::HashMap<PhysicalWindowId, (i32, i32)>,
    ) -> std::collections::HashMap<crate::SlotId, (i32, i32)> {
        if minimums.is_empty() {
            return std::collections::HashMap::new();
        }
        // Every cell gives one whole gap to the space around its window.
        let gap = settings.spacing.inner;
        self.model
            .workspace
            .slots
            .iter()
            .filter_map(|slot| {
                let (width, height) = *minimums.get(slot.bound_window()?)?;
                Some((slot.id, (width + gap, height + gap)))
            })
            .collect()
    }

    /// Compute placements for a candidate layout tree (including any temporary
    /// minimum-size fit) without mutating the committed workspace layout.
    pub(crate) fn placements_for_tree(
        &self,
        layout: &LayoutTree,
        settings: &Settings,
        minimums: &std::collections::HashMap<PhysicalWindowId, (i32, i32)>,
    ) -> Vec<Placement> {
        let slots = self.slot_minimums(settings, minimums);
        let fitted = (!slots.is_empty())
            .then(|| layout.fitted(settings.canvas(), &slots))
            .flatten();
        placements_for(fitted.as_ref().unwrap_or(layout), &self.model, settings)
    }

    /// Commit an already-edited layout tree as the workspace's requested layout.
    pub(crate) fn commit_layout_tree(&mut self, layout: LayoutTree) {
        self.layout = layout;
        self.commit_layout();
    }

    pub(crate) fn fit_minimums(
        &mut self,
        settings: &Settings,
        minimums: &std::collections::HashMap<PhysicalWindowId, (i32, i32)>,
    ) -> bool {
        let output = settings.canvas();
        let slots = self.slot_minimums(settings, minimums);
        if slots.is_empty() {
            return self.fit.take().is_some_and(|fit| fit.tree.is_some());
        }
        if self
            .fit
            .as_ref()
            .is_some_and(|fit| fit.output == output && fit.minimums == slots)
        {
            return false;
        }
        let tree = self.layout.fitted(output, &slots);
        let changed = self.fit.as_ref().and_then(|fit| fit.tree.as_ref()) != tree.as_ref();
        self.fit = Some(DisplayFit {
            output,
            minimums: slots,
            tree,
        });
        changed
    }

    pub(crate) fn resize_focused_tile(
        &mut self,
        window: &PhysicalWindowId,
        direction: crate::ResizeDirection,
        amount: i32,
        settings: &Settings,
        minimums: &WindowMinimums,
    ) -> bool {
        let canvas = settings.canvas();
        self.edit_weights(window, settings, minimums, |layout, slot, floors| {
            layout.resize_by_within(slot, direction, amount, canvas, floors)
        })
    }

    pub(crate) fn balance_focused_parent(
        &mut self,
        window: &PhysicalWindowId,
        settings: &Settings,
        minimums: &WindowMinimums,
    ) -> bool {
        let canvas = settings.canvas();
        self.edit_weights(window, settings, minimums, |layout, slot, floors| {
            layout.balance_within(slot, canvas, floors)
        })
    }

    /// Resize the focused window so its frame is `points` along `axis`: the tile's cell is the
    /// window plus the inner gap.
    pub(crate) fn resize_focused_tile_to(
        &mut self,
        window: &PhysicalWindowId,
        axis: crate::Axis,
        points: i32,
        settings: &Settings,
        minimums: &WindowMinimums,
    ) -> bool {
        let canvas = settings.canvas();
        let cell = settings.spacing.untile(crate::Rect {
            x: 0,
            y: 0,
            width: points,
            height: points,
        });
        let target = match axis {
            crate::Axis::Horizontal => cell.width,
            crate::Axis::Vertical => cell.height,
        };
        self.edit_weights(window, settings, minimums, |layout, slot, floors| {
            layout.resize_to_within(slot, axis, target, canvas, floors)
        })
    }

    /// Splits or re-orients the requested layout along `direction` only when
    /// doing so changes its split structure. A temporary minimum-width fit can
    /// make an outward directional move look perpendicular while recreating the
    /// same saved shape; returning `false` lets `reorder_focused` cross displays.
    pub(crate) fn create_implicit_split(
        &mut self,
        window: &PhysicalWindowId,
        direction: crate::Direction,
    ) -> bool {
        let Some(slot) = self.slot_for_window(window) else {
            return false;
        };
        let mut layout = self.layout.clone();
        if !layout.create_implicit_split(slot, direction) || !layout.regroups(&self.layout) {
            return false;
        }
        self.layout = layout;
        self.commit_layout();
        true
    }

    #[cfg(test)]
    pub(crate) fn move_window_relative(
        &mut self,
        moved: &PhysicalWindowId,
        target: &PhysicalWindowId,
        direction: crate::Direction,
    ) -> bool {
        let Some(target) = self.slot_for_window(target) else {
            return false;
        };
        self.edit_requested(moved, |layout, moved| {
            layout.move_relative(moved, target, direction)
        })
    }

    pub(crate) fn layout(&self) -> &LayoutTree {
        &self.layout
    }

    pub(crate) fn root_axis(&self) -> crate::Axis {
        self.display_layout().root_axis()
    }

    /// Each tile's slot, window and frame in `output`, in layout order.
    pub(crate) fn preview_placements(&self, output: crate::Rect) -> Vec<Placement> {
        placements_for(
            self.display_layout(),
            &self.model,
            &Settings::for_output(output),
        )
    }

    pub(crate) fn preview(&self, output: crate::Rect) -> Vec<crate::Rect> {
        self.display_layout()
            .placements(output)
            .into_iter()
            .map(|(_, frame)| frame)
            .collect()
    }

    pub(crate) fn restore_layout(&mut self, mut layout: LayoutTree) {
        layout.sync(layout.root_axis(), &self.model.workspace.slots);
        self.layout = layout;
        self.commit_layout();
    }

    /// Hands `from`'s slot (its id, weight and place) to `to`: the window a tab
    /// group shows once the tab `from` was is moved out of it.
    pub(crate) fn rebind_window(&mut self, from: &PhysicalWindowId, to: &PhysicalWindowId) -> bool {
        let Some(slot) = self
            .model
            .workspace
            .slots
            .iter_mut()
            .find(|slot| slot.bound_window() == Some(from))
        else {
            return false;
        };
        slot.binding = crate::BindingState::Bound(to.clone());
        self.invalidate_layout();
        true
    }

    pub(crate) fn slot_for_window(&self, window: &PhysicalWindowId) -> Option<crate::SlotId> {
        self.model
            .workspace
            .slots
            .iter()
            .find(|slot| slot.bound_window() == Some(window))
            .map(|slot| slot.id)
    }

    /// Edits the layout on display, a fit included, and saves it. Only for
    /// size edits through a fit that keeps the requested grouping (see
    /// `edit_weights`); structural edits go through `edit_requested`.
    fn edit_shown(
        &mut self,
        slot: crate::SlotId,
        op: impl FnOnce(&mut LayoutTree, crate::SlotId) -> bool,
    ) -> bool {
        let mut layout = self.display_layout().clone();
        if !op(&mut layout, slot) {
            return false;
        }
        self.layout = layout;
        self.commit_layout();
        true
    }

    /// Edits the requested layout, never the temporary fit on display, so a
    /// structural edit made while a fit stacks tiles does not save the stack.
    fn edit_requested(
        &mut self,
        window: &PhysicalWindowId,
        op: impl FnOnce(&mut LayoutTree, crate::SlotId) -> bool,
    ) -> bool {
        let Some(slot) = self.slot_for_window(window) else {
            return false;
        };
        let mut layout = self.layout.clone();
        if !op(&mut layout, slot) {
            return false;
        }
        self.layout = layout;
        self.commit_layout();
        true
    }

    /// Edits tile sizes. While the display shows an automatic grouping the
    /// requested layout lacks, the edit changes only that temporary fit:
    /// saving it would make the grouping permanent.
    ///
    /// Every tile keeps its window's known minimum size and, past the inner
    /// gap, at least `MIN_TILE_EXTENT` of its own frame (#407, #408, #409).
    fn edit_weights(
        &mut self,
        window: &PhysicalWindowId,
        settings: &Settings,
        minimums: &WindowMinimums,
        op: impl FnOnce(&mut LayoutTree, crate::SlotId, Minimums<'_>) -> bool,
    ) -> bool {
        let Some(slot) = self.slot_for_window(window) else {
            return false;
        };
        let known = self.slot_minimums(settings, minimums);
        let floors = Minimums {
            known: Some(&known),
            floor: crate::layout_tree::MIN_TILE_EXTENT + settings.spacing.inner.max(0),
        };
        let requested = &self.layout;
        let Some(tree) = self
            .fit
            .as_mut()
            .and_then(|fit| fit.tree.as_mut())
            .filter(|tree| tree.regroups(requested))
        else {
            return self.edit_shown(slot, |layout, slot| op(layout, slot, floors));
        };
        let mut edited = tree.clone();
        if !op(&mut edited, slot, floors) {
            return false;
        }
        *tree = edited;
        self.invalidate();
        true
    }

    fn invalidate_layout(&mut self) {
        self.fit = None;
        self.insertion_focus = self
            .insertion_focus
            .filter(|&id| self.model.slot(id).is_some());
        self.prune_focus_order();
        self.invalidate();
    }

    fn commit_layout(&mut self) {
        self.invalidate_layout();
    }

    /// Bring the layout's leaves in step with the model's slots after one was
    /// added or removed. The layout alone holds the split axis and weights.
    fn sync_layout(
        &mut self,
        anchor: Option<crate::SlotId>,
        mode: LayoutMode,
        output: crate::Rect,
    ) {
        let shown = self.fit.as_ref().and_then(|fit| fit.tree.as_ref());
        self.layout.sync_shown(
            self.layout.root_axis(),
            &self.model.workspace.slots,
            anchor,
            mode,
            output,
            shown,
        );
    }
}

fn placements_for(layout: &LayoutTree, model: &Model, settings: &Settings) -> Vec<Placement> {
    settings
        .spacing
        .place(settings.usable(), layout.placements(settings.canvas()))
        .into_iter()
        .filter_map(|(id, frame)| {
            let slot = model.slot(id)?;
            Some(Placement {
                slot: id,
                window: slot.bound_window().cloned(),
                frame,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::{
        AppInstanceId, AppObservation, NativeObservation, ObservedApplication, ObservedWindow,
        PhysicalWindowId, Rect,
    };

    use super::*;

    fn slot(id: u64) -> Slot {
        Slot {
            id: crate::SlotId(id),
            binding: crate::BindingState::Bound(PhysicalWindowId {
                app: AppInstanceId {
                    pid: 1,
                    launched_at: 1,
                },
                window_id: id as u32,
            }),
        }
    }

    #[test]
    fn new_window_joins_the_focused_nested_split() {
        let mut engine = Engine::default();
        for i in 1..=3 {
            engine.insert_slot(slot(i));
        }
        let windows = engine.bound_windows();
        assert!(engine.move_window_relative(&windows[0], &windows[1], crate::Direction::Up));
        engine.remember_insertion_focus(&windows[0]);
        let ticket = engine.begin_cycle();
        let columns = settings().with_layout(LayoutMode::Columns);
        engine
            .accept(ticket, &observation(ticket.cycle, 4), &columns)
            .unwrap();
        let output = crate::Rect {
            width: 1200,
            height: 800,
            ..crate::Rect::default()
        };
        let tiles = engine.layout.placements(output);
        assert_eq!(
            tiles.iter().map(|(s, _)| s.0).collect::<Vec<_>>(),
            [1, 4, 2, 3]
        );
        assert_eq!(tiles[0].1.width, 600);
        assert_eq!(tiles[1].1.width, 600);
        assert_eq!(
            tiles[3].1,
            crate::Rect {
                x: 600,
                width: 600,
                ..output
            }
        );
    }

    #[test]
    fn focus_recency_orders_tiles_and_ignores_floats() {
        let mut engine = Engine::default();
        for i in 1..=3 {
            engine.insert_slot(slot(i));
        }
        let windows = engine.bound_windows();
        assert_eq!(engine.focus_recency(&windows[1]), None);
        engine.remember_insertion_focus(&windows[2]);
        engine.remember_insertion_focus(&windows[1]);
        engine.remember_insertion_focus(&windows[0]);
        let float = PhysicalWindowId {
            app: AppInstanceId {
                pid: 9,
                launched_at: 1,
            },
            window_id: 99,
        };
        engine.remember_insertion_focus(&float);
        assert_eq!(engine.focus_recency(&windows[0]), Some(0));
        assert_eq!(engine.focus_recency(&windows[1]), Some(1));
        assert_eq!(engine.focus_recency(&windows[2]), Some(2));
        engine.remember_insertion_focus(&windows[2]);
        assert_eq!(engine.focus_recency(&windows[2]), Some(0));
        assert_eq!(engine.focus_recency(&windows[0]), Some(1));
    }

    #[test]
    fn a_reused_slot_id_starts_unranked() {
        let mut engine = Engine::default();
        for i in 1..=2 {
            engine.insert_slot(slot(i));
        }
        let windows = engine.bound_windows();
        engine.remember_insertion_focus(&windows[1]);
        engine.remember_insertion_focus(&windows[0]);
        assert_eq!(engine.focus_recency(&windows[1]), Some(1));
        assert!(engine.take_slot(&windows[1]).is_some());

        // A tile moved in from another workspace keeps its ID, here the closed tile's.
        let arriving = Slot {
            id: crate::SlotId(2),
            binding: crate::BindingState::Bound(PhysicalWindowId {
                app: AppInstanceId {
                    pid: 7,
                    launched_at: 1,
                },
                window_id: 70,
            }),
        };
        let window = arriving.bound_window().cloned().unwrap();
        engine.insert_slot(arriving);
        assert_eq!(engine.slot_for_window(&window), Some(crate::SlotId(2)));
        assert_eq!(engine.focus_recency(&window), None);
        assert_eq!(engine.focus_recency(&windows[0]), Some(0));
    }

    #[test]
    fn dwindle_splits_the_focused_tile_along_its_longer_side() {
        let mut engine = Engine::default();
        for i in 1..=3 {
            engine.insert_slot(slot(i));
        }
        let windows = engine.bound_windows();
        assert!(engine.move_window_relative(&windows[0], &windows[1], crate::Direction::Up));
        engine.remember_insertion_focus(&windows[0]);
        let output = crate::Rect {
            width: 1200,
            height: 800,
            ..crate::Rect::default()
        };
        let ticket = engine.begin_cycle();
        engine
            .accept(
                ticket,
                &observation(ticket.cycle, 4),
                &Settings::for_output(output),
            )
            .unwrap();
        // Tile 1 was 600x400 (wider than tall), so window 4 splits it horizontally.
        let tiles = engine.layout.placements(output);
        assert_eq!(
            tiles.iter().map(|(s, _)| s.0).collect::<Vec<_>>(),
            [1, 4, 2, 3]
        );
        assert_eq!(
            tiles[0].1,
            crate::Rect {
                x: 0,
                y: 0,
                width: 300,
                height: 400,
            }
        );
        assert_eq!(
            tiles[1].1,
            crate::Rect {
                x: 300,
                y: 0,
                width: 300,
                height: 400,
            }
        );
    }

    #[test]
    fn structural_layout_edits_commit_axis_and_slot_weights() {
        let mut engine = Engine::default();
        let slots = [slot(1), slot(2), slot(3)];
        let windows = slots
            .iter()
            .map(|slot| slot.bound_window().unwrap().clone())
            .collect::<Vec<_>>();
        for slot in slots {
            engine.insert_slot(slot);
        }
        let output = Rect {
            width: 1000,
            height: 800,
            ..Rect::default()
        };
        assert!(engine.resize_focused_tile(
            &windows[0],
            crate::ResizeDirection::Grow,
            50,
            &Settings::for_output(output),
            &WindowMinimums::new(),
        ));
        assert!(engine.create_implicit_split(&windows[1], crate::Direction::Up));
        assert_eq!(engine.layout().root_axis(), crate::Axis::Vertical);
        assert_eq!(engine.layout().weight(crate::SlotId(2)), Some(1));

        assert!(engine.move_window_relative(&windows[2], &windows[0], crate::Direction::Right));
        assert_eq!(engine.layout().weight(crate::SlotId(3)), Some(1));
    }

    #[test]
    fn implicit_split_returns_false_when_saved_shape_is_unchanged() {
        let output = Rect {
            width: 1000,
            height: 800,
            ..Rect::default()
        };
        for mode in [LayoutMode::Columns, LayoutMode::Dwindle] {
            let mut engine = Engine::default();
            engine.insert_slot_with(slot(1), mode, output, None);
            engine.insert_slot_with(slot(2), mode, output, Some(crate::SlotId(1)));
            let windows = engine.bound_windows();
            let before = engine.layout().clone();
            let revision = engine.revision();

            assert!(!engine.create_implicit_split(&windows[0], crate::Direction::Left));
            assert!(!engine.create_implicit_split(&windows[1], crate::Direction::Right));
            assert_eq!(engine.layout(), &before);
            assert_eq!(engine.revision(), revision);

            // Reversing the pair or re-orienting vertically changes the shape.
            assert!(engine.create_implicit_split(&windows[0], crate::Direction::Up));
            assert_eq!(engine.layout().root_axis(), crate::Axis::Vertical);
        }
    }

    fn observation(cycle: u64, window_id: u32) -> NativeObservation {
        let app = AppInstanceId {
            pid: 10,
            launched_at: 1,
        };
        let window = PhysicalWindowId {
            app: app.clone(),
            window_id,
        };
        NativeObservation {
            cycle,
            mouse_down: false,
            applications: vec![AppObservation::Complete {
                app: ObservedApplication {
                    id: app,
                    bundle_id: "com.example.Editor".into(),
                },
                focused: Some(window.clone()),
                windows: vec![ObservedWindow {
                    id: window,
                    frame: Rect {
                        width: 100,
                        height: 100,
                        ..Rect::default()
                    },
                    resizable: true,
                    title: None,
                    document: None,
                }],
            }],
        }
    }

    fn settings() -> Settings {
        Settings {
            output: Rect {
                width: 100,
                height: 100,
                ..Rect::default()
            },
            frame_tolerance: 2,
            spacing: crate::spacing::Spacing::NONE,
            layout: LayoutMode::default(),
        }
    }

    #[test]
    fn accepted_cycle_commits_model_and_evidence() {
        let mut engine = Engine::default();
        engine.invalidate();
        let ticket = engine.begin_cycle();
        let result = engine.accept(ticket, &observation(ticket.cycle, 1), &settings());

        assert_eq!(result.unwrap().model.workspace.slots.len(), 1);
        assert_eq!(engine.model().workspace.slots.len(), 1);
    }

    #[test]
    fn invalidation_rejects_an_in_flight_cycle_without_mutation() {
        let mut engine = Engine::default();
        engine.invalidate();
        let ticket = engine.begin_cycle();
        engine.invalidate();

        let error = engine
            .accept(ticket, &observation(ticket.cycle, 1), &settings())
            .unwrap_err();
        assert_eq!(error.current_revision, 2);
        assert!(engine.model().workspace.slots.is_empty());
    }

    #[test]
    fn mismatched_observation_cycle_is_rejected() {
        let mut engine = Engine::default();
        let ticket = engine.begin_cycle();

        assert!(
            engine
                .accept(ticket, &observation(ticket.cycle + 1, 1), &settings())
                .is_err()
        );
    }

    #[test]
    fn newer_cycle_supersedes_an_older_ticket_in_the_same_revision() {
        let mut engine = Engine::default();
        let older = engine.begin_cycle();
        let current = engine.begin_cycle();

        assert!(
            engine
                .accept(older, &observation(older.cycle, 1), &settings())
                .is_err()
        );
        assert!(
            engine
                .accept(current, &observation(current.cycle, 1), &settings())
                .is_ok()
        );
    }

    #[test]
    fn accepted_ticket_cannot_be_committed_twice() {
        let mut engine = Engine::default();
        let ticket = engine.begin_cycle();
        let observation = observation(ticket.cycle, 1);

        assert!(engine.accept(ticket, &observation, &settings()).is_ok());
        assert!(engine.accept(ticket, &observation, &settings()).is_err());
    }

    #[test]
    fn reordering_a_window_invalidates_pending_work() {
        let first = PhysicalWindowId {
            app: AppInstanceId {
                pid: 1,
                launched_at: 1,
            },
            window_id: 1,
        };
        let second = PhysicalWindowId {
            window_id: 2,
            ..first.clone()
        };
        let mut engine = Engine::default();
        engine.insert_slot(Slot {
            id: crate::SlotId(1),
            binding: crate::BindingState::Bound(first.clone()),
        });
        engine.insert_slot(Slot {
            id: crate::SlotId(2),
            binding: crate::BindingState::Bound(second.clone()),
        });
        let pending = engine.begin_cycle();

        assert!(engine.swap_windows(&second, &first));
        assert_eq!(engine.bound_windows(), vec![second, first]);
        assert!(
            engine
                .accept(pending, &observation(pending.cycle, 1), &settings())
                .is_err()
        );
    }

    #[test]
    fn taking_a_slot_invalidates_pending_work() {
        let window = PhysicalWindowId {
            app: AppInstanceId {
                pid: 1,
                launched_at: 1,
            },
            window_id: 1,
        };
        let mut engine = Engine::default();
        engine.insert_slot(Slot {
            id: crate::SlotId(1),
            binding: crate::BindingState::Bound(window.clone()),
        });
        let pending = engine.begin_cycle();

        assert!(engine.take_slot(&window).is_some());
        assert!(
            engine
                .accept(pending, &observation(pending.cycle, 1), &settings())
                .is_err()
        );
        assert!(engine.model().workspace.slots.is_empty());
    }

    #[test]
    fn inserting_a_slot_invalidates_pending_work() {
        let mut engine = Engine::default();
        let pending = engine.begin_cycle();
        engine.insert_slot(Slot {
            id: crate::SlotId(1),
            binding: crate::BindingState::Bound(PhysicalWindowId {
                app: AppInstanceId {
                    pid: 1,
                    launched_at: 1,
                },
                window_id: 1,
            }),
        });

        assert!(
            engine
                .accept(pending, &observation(pending.cycle, 1), &settings())
                .is_err()
        );
        assert_eq!(engine.model().workspace.slots.len(), 1);
    }
}
