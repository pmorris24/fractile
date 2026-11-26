//! The saved form of every workspace (`workspace-state.json`). In memory a
//! layout tree alone holds each workspace's split axis and tile weights; saving
//! projects them back out per slot so files written by earlier builds still load.
use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use super::{DisplaySelector, WorkspaceId};

/// A slot as saved: its tile's weight in its parent split rides along, so a
/// file with no layout tree (written before trees were saved) still restores
/// each tile's share.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct SavedSlot {
    pub(crate) id: crate::SlotId,
    pub(crate) weight: u32,
    pub(crate) binding: crate::BindingState,
}

impl SavedSlot {
    pub(crate) fn new(slot: &crate::Slot, layout: &crate::layout_tree::LayoutTree) -> Self {
        Self {
            id: slot.id,
            weight: layout.weight(slot.id).unwrap_or(1),
            binding: slot.binding.clone(),
        }
    }

    pub(crate) fn slot(&self) -> crate::Slot {
        crate::Slot {
            id: self.id,
            binding: self.binding.clone(),
        }
    }

    fn identity_window(&self) -> &crate::PhysicalWindowId {
        match &self.binding {
            crate::BindingState::Bound(window) => window,
            crate::BindingState::Unresolved { previous, .. } => previous,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct WorkspaceState {
    pub(super) active: Vec<(DisplaySelector, WorkspaceId)>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) previous: Vec<(DisplaySelector, WorkspaceId)>,
    /// Per physical display ID, the lane it showed last, oldest first: which of
    /// two selectors naming one display is in front, across restarts (#386).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) fronts: Vec<(String, DisplaySelector)>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) axes: Vec<crate::Axis>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) layouts: Vec<crate::layout_tree::LayoutTree>,
    pub(super) slots: Vec<Vec<SavedSlot>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) floating: Vec<(crate::PhysicalWindowId, WorkspaceId)>,
    /// Floating windows pinned by hand (`fractile ctl pin`) to show on every workspace.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) pinned: Vec<crate::PhysicalWindowId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) last_focused: Vec<Option<crate::SlotId>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) fullscreen_layout: Vec<bool>,
    /// Legacy per-window maximize intent. A populated entry migrates the
    /// workspace into fullscreen layout mode.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) maximized: Vec<Option<crate::SlotId>>,
}

impl WorkspaceState {
    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.slots.len() > WorkspaceId::COUNT
            || self.layouts.len() > WorkspaceId::COUNT
            || self.axes.len() > WorkspaceId::COUNT
            || self.last_focused.len() > WorkspaceId::COUNT
            || self.fullscreen_layout.len() > WorkspaceId::COUNT
            || self.maximized.len() > WorkspaceId::COUNT
        {
            return Err("workspace state contains more than 36 workspaces".into());
        }
        for (index, slots) in self.slots.iter().enumerate() {
            let next_slot_id = slots
                .iter()
                .map(|slot| slot.id.0)
                .max()
                .unwrap_or(0)
                .checked_add(1)
                .ok_or("workspace slot ID cannot be incremented")?;
            if let Some(slot) = slots.iter().find(|slot| slot.weight == 0) {
                return Err(format!(
                    "workspace {}: slot {} has zero weight",
                    index + 1,
                    slot.id.0
                ));
            }
            let slots = slots.iter().map(SavedSlot::slot).collect::<Vec<_>>();
            crate::Model {
                workspace: crate::model::Workspace {
                    slots: slots.clone(),
                },
                continuity: HashMap::new(),
                next_slot_id,
            }
            .validate()
            .map_err(|error| format!("workspace {}: {error}", index + 1))?;
            if let Some(layout) = self.layouts.get(index) {
                layout
                    .validate(&slots)
                    .map_err(|error| format!("workspace {}: {error}", index + 1))?;
            }
        }
        if self.layouts.len() > self.slots.len() {
            return Err("workspace layouts have no matching slot state".into());
        }
        for (name, intents) in [
            ("last-focused", &self.last_focused),
            ("maximized", &self.maximized),
        ] {
            for (index, slot) in intents.iter().enumerate() {
                if let Some(slot) = slot
                    && !self
                        .slots
                        .get(index)
                        .is_some_and(|slots| slots.iter().any(|candidate| candidate.id == *slot))
                {
                    return Err(format!(
                        "workspace {} {name} slot {} does not exist",
                        index + 1,
                        slot.0
                    ));
                }
            }
        }
        let mut tiled = HashSet::new();
        for window in self.slots.iter().flatten().map(SavedSlot::identity_window) {
            if !tiled.insert(window) {
                return Err("tiled window is owned by multiple workspaces".into());
            }
        }
        let mut floating = HashSet::new();
        for (window, _) in &self.floating {
            if tiled.contains(window) {
                return Err("window is both tiled and floating".into());
            }
            if !floating.insert(window) {
                return Err("floating window is owned by multiple workspaces".into());
            }
        }
        let mut pinned = HashSet::new();
        for window in &self.pinned {
            if !floating.contains(window) {
                return Err("pinned window is not floating".into());
            }
            if !pinned.insert(window) {
                return Err("pinned window is listed more than once".into());
            }
        }
        Ok(())
    }

    pub(crate) fn named_workspaces_in_use(&self) -> HashSet<WorkspaceId> {
        let mut used = self
            .active
            .iter()
            .chain(&self.previous)
            .map(|(_, workspace)| *workspace)
            .chain(self.floating.iter().map(|(_, workspace)| *workspace))
            .filter(|workspace| workspace.letter().is_some())
            .collect::<HashSet<_>>();
        for (index, slots) in self.slots.iter().enumerate() {
            let workspace = WorkspaceId::from_index(index);
            if workspace.letter().is_some() && !slots.is_empty() {
                used.insert(workspace);
            }
        }
        used
    }
}
