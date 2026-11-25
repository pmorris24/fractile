use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::{Engine, PhysicalWindowId};

mod deck;
mod display_map;
mod id;
mod lanes;
mod residency;
mod state;
pub(crate) use deck::DeckMode;
pub(crate) use display_map::DisplayMap;
pub use id::WorkspaceId;
pub(crate) use residency::{FloatCause, Residency};
pub(crate) use state::{SavedSlot, WorkspaceState};

#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub enum DisplaySelector {
    Main,
    Secondary,
    /// The second non-main display, left to right. Falls back to `Secondary`, then `Main`.
    Tertiary,
    Preferred(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DisplayTarget {
    Next,
    Prev,
    Selector(DisplaySelector),
}

/// One step along a display's workspaces, in `settings.workspace_order`: `switch next|prev`
/// and `move next|prev` (the Workspace Steps Lab). Steps stop at either end.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkspaceStep {
    Next,
    Prev,
}

impl WorkspaceStep {
    /// Parses `next` or `prev`.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "next" => Some(Self::Next),
            "prev" => Some(Self::Prev),
            _ => None,
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Next => "next",
            Self::Prev => "prev",
        }
    }

    /// The workspace one step from `current` in `ordered`, or `None` past either end or when
    /// `current` is not listed.
    #[must_use]
    pub fn target(self, ordered: &[WorkspaceId], current: WorkspaceId) -> Option<WorkspaceId> {
        let index = ordered.iter().position(|workspace| *workspace == current)?;
        match self {
            Self::Next => ordered.get(index + 1).copied(),
            Self::Prev => index
                .checked_sub(1)
                .and_then(|index| ordered.get(index).copied()),
        }
    }
}

impl DisplaySelector {
    #[must_use]
    pub fn parse(value: &str) -> Self {
        match value {
            "main" => Self::Main,
            "secondary" => Self::Secondary,
            "tertiary" => Self::Tertiary,
            value => Self::Preferred(value.to_owned()),
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Main => "main",
            Self::Secondary => "secondary",
            Self::Tertiary => "tertiary",
            Self::Preferred(value) => value.as_str(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Workspaces {
    engines: [Engine; WorkspaceId::COUNT],
    assignments: [DisplaySelector; WorkspaceId::COUNT],
    /// What each display lane and each physical display shows.
    displays: DisplayMap,
    /// Every managed window that holds no tile: floats (pinned or not) and the Scratchpad.
    residency: residency::WindowTable,
    enabled: HashSet<WorkspaceId>,
    last_focused: [Option<crate::SlotId>; WorkspaceId::COUNT],
    last_focused_float: [Option<PhysicalWindowId>; WorkspaceId::COUNT],
    decks: [DeckMode; WorkspaceId::COUNT],
}

impl Default for Workspaces {
    fn default() -> Self {
        Self {
            engines: std::array::from_fn(|_| Engine::default()),
            assignments: std::array::from_fn(|index| {
                Self::default_assignment(WorkspaceId::from_index(index))
            }),
            displays: DisplayMap::new([
                (DisplaySelector::Main, WorkspaceId::ONE),
                (DisplaySelector::Secondary, WorkspaceId::SIX),
                (DisplaySelector::Tertiary, WorkspaceId::EIGHT),
            ]),
            residency: residency::WindowTable::default(),
            enabled: WorkspaceId::numbered().collect(),
            last_focused: std::array::from_fn(|_| None),
            last_focused_float: std::array::from_fn(|_| None),
            decks: [DeckMode::Split; WorkspaceId::COUNT],
        }
    }
}

impl Workspaces {
    #[must_use]
    pub(crate) fn state(&self) -> WorkspaceState {
        // An emptied workspace still keeps a split direction chosen for it (#402).
        let default_layout = crate::layout_tree::LayoutTree::default();
        let workspace_count = self
            .engines
            .iter()
            .rposition(|engine| {
                !engine.model().workspace.slots.is_empty() || *engine.layout() != default_layout
            })
            .map_or(WorkspaceId::NUMBERED_COUNT, |index| index + 1)
            .max(WorkspaceId::NUMBERED_COUNT);
        WorkspaceState {
            active: sorted(self.displays.lanes().map(|(lane, ws)| (lane.clone(), ws))),
            previous: sorted(
                self.displays
                    .previous_lanes()
                    .map(|(lane, ws)| (lane.clone(), ws)),
            ),
            fronts: self.displays.fronts_in_order(),
            axes: Vec::new(),
            layouts: self
                .engines
                .iter()
                .take(workspace_count)
                .map(|engine| engine.layout().clone())
                .collect(),
            slots: self
                .engines
                .iter()
                .take(workspace_count)
                .map(|engine| {
                    let slots = engine.model().workspace.slots.iter();
                    slots
                        .map(|slot| SavedSlot::new(slot, engine.layout()))
                        .collect()
                })
                .collect(),
            floating: sorted(
                self.floating_entries()
                    .map(|(window, workspace)| (window.clone(), workspace)),
            ),
            pinned: sorted(self.residency.hand_pinned().cloned()),
            last_focused: trimmed(&self.last_focused, Option::is_some),
            fullscreen_layout: trimmed(&self.decks.map(DeckMode::is_monocle), |enabled| *enabled),
            maximized: Vec::new(),
        }
    }

    pub(crate) fn restore(&mut self, state: WorkspaceState) {
        self.enabled.extend(state.named_workspaces_in_use());
        let WorkspaceState {
            active,
            previous,
            fronts,
            axes,
            layouts,
            slots,
            floating,
            pinned,
            last_focused,
            fullscreen_layout,
            maximized,
        } = state;
        for (index, saved) in slots.into_iter().take(WorkspaceId::COUNT).enumerate() {
            let workspace = WorkspaceId::from_index(index);
            for saved in &saved {
                let mut slot = saved.slot();
                if let crate::BindingState::Unresolved { since, .. } = &mut slot.binding {
                    // Reconciliation cycles are process-local, so restart the
                    // bounded settling window when restoring persisted state.
                    *since = 0;
                }
                self.engine_mut(workspace).insert_slot(slot);
            }
            let layout = layouts.get(index).cloned().unwrap_or_else(|| {
                crate::layout_tree::LayoutTree::weighted_flat(
                    axes.get(index).copied().unwrap_or_default(),
                    saved.iter().map(|slot| (slot.id, slot.weight)),
                )
            });
            self.engine_mut(workspace).restore_layout(layout);
        }
        for (selector, workspace) in active {
            if self.assignment(workspace) == &selector {
                self.displays.set_lane(selector, workspace);
            }
        }
        // A lane's previous workspace is what its display showed before, which
        // another selector naming the same display may assign (#401).
        for (selector, workspace) in previous {
            if self.is_enabled(workspace) && self.active_for(&selector) != Some(workspace) {
                self.displays.set_previous(selector, workspace);
            }
        }
        self.displays.restore_fronts(fronts);
        // The saved state does not say why a window floated, so every restored float floats
        // by hand until its first observation settles whether a float rule holds it.
        for (window, workspace) in floating {
            if self.workspace_of(&window).is_none() {
                self.float(window, workspace, FloatCause::Restored);
            }
        }
        for window in pinned {
            let _: bool = self.pin(window);
        }
        for index in 0..WorkspaceId::COUNT {
            let workspace = WorkspaceId::from_index(index);
            let valid_slot = |slots: &[Option<crate::SlotId>]| {
                slots
                    .get(index)
                    .copied()
                    .flatten()
                    .filter(|slot| self.has_slot(workspace, *slot))
            };
            let legacy_maximized = valid_slot(&maximized);
            self.last_focused[index] = legacy_maximized.or_else(|| valid_slot(&last_focused));
            let monocle = fullscreen_layout.get(index).copied().unwrap_or(false)
                || legacy_maximized.is_some();
            self.set_deck(workspace, DeckMode::from_fullscreen(monocle));
        }
    }

    #[must_use]
    pub fn default_assignment(workspace: WorkspaceId) -> DisplaySelector {
        match workspace.number() {
            Some(number) if number >= 8 => DisplaySelector::Tertiary,
            Some(number) if number >= 6 => DisplaySelector::Secondary,
            _ => DisplaySelector::Main,
        }
    }

    #[must_use]
    pub fn assignment(&self, workspace: WorkspaceId) -> &DisplaySelector {
        &self.assignments[workspace.index()]
    }

    pub fn enable(&mut self, workspace: WorkspaceId) {
        self.enabled.insert(workspace);
    }

    /// Disables every named workspace keep rejects that nothing refers to any
    /// more: no window, no display history, no Fullscreen Layout (#424).
    pub(crate) fn retain_named(&mut self, keep: impl Fn(WorkspaceId) -> bool) {
        let in_use = self.state().named_workspaces_in_use();
        self.enabled.retain(|workspace| {
            workspace.letter().is_none() || keep(*workspace) || in_use.contains(workspace)
        });
    }

    #[must_use]
    pub fn is_enabled(&self, workspace: WorkspaceId) -> bool {
        self.enabled.contains(&workspace)
    }

    #[must_use]
    pub fn engine(&self, workspace: WorkspaceId) -> &Engine {
        &self.engines[workspace.index()]
    }

    pub fn engine_mut(&mut self, workspace: WorkspaceId) -> &mut Engine {
        &mut self.engines[workspace.index()]
    }

    pub fn move_window(
        &mut self,
        window: &PhysicalWindowId,
        target: WorkspaceId,
    ) -> Option<WorkspaceId> {
        self.move_window_with(
            window,
            target,
            crate::config::LayoutMode::Columns,
            crate::Rect::default(),
            crate::Rect::default(),
        )
    }

    pub(crate) fn move_window_with(
        &mut self,
        window: &PhysicalWindowId,
        target: WorkspaceId,
        mode: crate::config::LayoutMode,
        source_output: crate::Rect,
        target_output: crate::Rect,
    ) -> Option<WorkspaceId> {
        let source = self.workspace_of(window)?;
        let source_index = source.index();
        let target_index = target.index();
        if source_index == target_index {
            return None;
        }
        let anchor = match mode {
            crate::config::LayoutMode::Dwindle => self.last_focused[target_index],
            crate::config::LayoutMode::Columns => None,
        };
        let (source_engine, target_engine) = if source_index < target_index {
            let (before, after) = self.engines.split_at_mut(target_index);
            (&mut before[source_index], &mut after[0])
        } else {
            let (before, after) = self.engines.split_at_mut(source_index);
            (&mut after[0], &mut before[target_index])
        };
        let slot = source_engine.take_slot_with(window, mode, source_output)?;
        target_engine.insert_slot_with(slot, mode, target_output, anchor);
        self.prune_intents(source);
        Some(source)
    }

    #[must_use]
    pub fn workspace_of(&self, window: &PhysicalWindowId) -> Option<WorkspaceId> {
        self.engines
            .iter()
            .position(|engine| {
                engine
                    .model()
                    .workspace
                    .slots
                    .iter()
                    .any(|slot| slot.identity_window() == window)
            })
            .map(WorkspaceId::from_index)
    }

    #[must_use]
    pub(crate) fn workspace_of_any(&self, window: &PhysicalWindowId) -> Option<WorkspaceId> {
        self.workspace_of(window)
            .or_else(|| self.floating_workspace_of(window))
    }

    /// The workspace's tiles in stable tile order, then its floats in a stable
    /// (app instance, window id) order so callers can walk them predictably.
    pub(crate) fn windows(&self, workspace: WorkspaceId) -> Vec<PhysicalWindowId> {
        let mut floats: Vec<_> = self
            .floating_entries()
            .filter(|(_, owner)| *owner == workspace)
            .map(|(window, _)| window.clone())
            .collect();
        floats.sort();
        let mut windows = self.engine(workspace).bound_windows();
        windows.extend(floats);
        windows
    }

    pub(crate) fn app_instances(
        &self,
        workspace: WorkspaceId,
    ) -> impl Iterator<Item = &crate::AppInstanceId> {
        self.engine(workspace)
            .model()
            .workspace
            .slots
            .iter()
            .map(|slot| &slot.identity_window().app)
    }

    pub(crate) fn app_owns_tiled_windows(&self, app: &crate::AppInstanceId) -> bool {
        WorkspaceId::all().any(|workspace| self.app_instances(workspace).any(|owner| owner == app))
    }

    pub(crate) fn app_owns_windows(&self, app: &crate::AppInstanceId) -> bool {
        self.app_owns_tiled_windows(app)
            || self
                .floating_entries()
                .any(|(window, _)| &window.app == app)
    }

    pub(crate) fn forget_app_with(
        &mut self,
        workspace: WorkspaceId,
        app: &crate::AppInstanceId,
        mode: crate::config::LayoutMode,
        output: crate::Rect,
    ) -> bool {
        let removed = self
            .engine_mut(workspace)
            .forget_app_with(app, mode, output);
        if removed {
            self.prune_intents(workspace);
        }
        removed
    }

    /// The one enabled workspace holding the app's windows, tiled or floating;
    /// none when they are spread over several. A pinned window shows on every
    /// workspace, so its home workspace does not count (#465).
    #[must_use]
    pub fn workspace_for_app(&self, app: &crate::AppInstanceId) -> Option<WorkspaceId> {
        let mut matches = WorkspaceId::all().filter(|workspace| {
            self.is_enabled(*workspace)
                && (self
                    .engine(*workspace)
                    .model()
                    .workspace
                    .slots
                    .iter()
                    .any(|slot| slot.belongs_to(app))
                    || self.floating_entries().any(|(window, owner)| {
                        owner == *workspace && &window.app == app && !self.is_pinned(window)
                    }))
        });
        let workspace = matches.next()?;
        matches.next().is_none().then_some(workspace)
    }

    /// Candidate evidence is reserved, not owned: only reconciliation may bind
    /// it. More than one reserving workspace means adoption must be deferred.
    pub(crate) fn replacement_workspaces<'a>(
        &'a self,
        window: &'a PhysicalWindowId,
    ) -> impl Iterator<Item = WorkspaceId> + 'a {
        WorkspaceId::all().filter(|workspace| {
            self.engine(*workspace)
                .model()
                .workspace
                .slots
                .iter()
                .any(|slot| {
                    matches!(&slot.binding, crate::BindingState::Unresolved { candidates, .. }
                    if candidates.contains(window))
                })
        })
    }

    pub fn forget_window(&mut self, workspace: WorkspaceId, window: &PhysicalWindowId) {
        self.forget_window_with(
            workspace,
            window,
            crate::config::LayoutMode::Columns,
            crate::Rect::default(),
        );
    }

    pub(crate) fn forget_window_with(
        &mut self,
        workspace: WorkspaceId,
        window: &PhysicalWindowId,
        mode: crate::config::LayoutMode,
        output: crate::Rect,
    ) {
        self.engine_mut(workspace)
            .take_slot_with(window, mode, output);
        self.prune_intents(workspace);
    }

    /// Remember `window` as the workspace's focused tile or float. Focusing a
    /// float records it without overwriting the remembered tile (#138, #240).
    pub(crate) fn remember_focus(&mut self, workspace: WorkspaceId, window: &PhysicalWindowId) {
        if let Some(slot) = self.engine(workspace).slot_for_window(window) {
            self.last_focused[workspace.index()] = Some(slot);
            self.last_focused_float[workspace.index()] = None;
        } else if self.floating_workspace_of(window) == Some(workspace) {
            self.last_focused_float[workspace.index()] = Some(window.clone());
        }
    }

    /// The workspace's most recently focused floating window, if it still
    /// belongs to `workspace`.
    pub(crate) fn preferred_float_focus(&self, workspace: WorkspaceId) -> Option<PhysicalWindowId> {
        self.last_focused_float[workspace.index()]
            .as_ref()
            .filter(|window| self.floating_workspace_of(window) == Some(workspace))
            .cloned()
    }

    pub(crate) fn preferred_focus(&self, workspace: WorkspaceId) -> Option<PhysicalWindowId> {
        let slot = self.last_focused[workspace.index()]?;
        self.engine(workspace)
            .model()
            .slot(slot)?
            .bound_window()
            .cloned()
    }

    /// How the workspace shows its tiles.
    pub(crate) fn deck(&self, workspace: WorkspaceId) -> DeckMode {
        self.decks[workspace.index()]
    }

    /// An empty workspace always shows its (nonexistent) tiles split, so the
    /// next window to arrive starts in the ordinary layout.
    pub(crate) fn set_deck(&mut self, workspace: WorkspaceId, deck: DeckMode) {
        let empty = self.engine(workspace).model().workspace.slots.is_empty();
        self.decks[workspace.index()] = if empty { DeckMode::Split } else { deck };
    }

    pub(crate) fn prune_intents(&mut self, workspace: WorkspaceId) {
        let index = workspace.index();
        self.last_focused[index] =
            self.last_focused[index].filter(|slot| self.has_slot(workspace, *slot));
        self.set_deck(workspace, self.decks[index]);
    }

    fn has_slot(&self, workspace: WorkspaceId, slot: crate::SlotId) -> bool {
        self.engine(workspace).model().slot(slot).is_some()
    }

    pub(crate) fn inactive_windows_except(
        &self,
        visible: &HashSet<WorkspaceId>,
    ) -> Vec<(WorkspaceId, PhysicalWindowId)> {
        WorkspaceId::all()
            .filter(|workspace| self.is_enabled(*workspace) && !visible.contains(workspace))
            .flat_map(|workspace| {
                self.windows(workspace)
                    .into_iter()
                    .map(move |window| (workspace, window))
            })
            .collect()
    }
}

fn sorted<T: Ord>(items: impl IntoIterator<Item = T>) -> Vec<T> {
    let mut items: Vec<_> = items.into_iter().collect();
    items.sort();
    items
}

fn trimmed<T: Clone>(items: &[T; WorkspaceId::COUNT], keep: impl Fn(&T) -> bool) -> Vec<T> {
    let length = items.iter().rposition(keep).map_or(0, |index| index + 1);
    items[..length].to_vec()
}

#[cfg(test)]
mod tests;
