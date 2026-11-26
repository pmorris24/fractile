//! Window residency: where every managed window lives, in one place.
//!
//! A window is tiled (a slot in its workspace's engine), floating on one workspace, or in the
//! Scratchpad, and never two of these at once. Tiles live in the engines because reconciliation
//! binds them; every other residency is one [`Residency`] record here, keyed by window. A float
//! therefore cannot also be a scratchpad window, a pin cannot outlive its float, and nothing
//! has to repair a stale floating owner after the fact. [`Workspaces`] keeps tiles and records
//! disjoint: adopting a tile drops the window's record, and a record is only written for a
//! window that holds no tile.
//!
//! Pinned windows are floats shown on every workspace of their display: a pin (by hand, by a
//! `pin` rule, or both) is part of a float's record, so tiling or forgetting the float drops it.

use std::collections::BTreeMap;

use super::{PhysicalWindowId, WorkspaceId, Workspaces};
use crate::scratchpad::{Scratchpad, ScratchpadEntry};

/// Why a float floats.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FloatCause {
    /// Toggle Float, the setup tour, or a recalled float: it floats until the user toggles it
    /// back, whatever its rules say.
    Manual,
    /// A float restored from saved state, which does not say why it floated. It floats by hand
    /// until its first observation settles it: [`FloatCause::Automatic`] if a float rule
    /// matches (#160), [`FloatCause::Manual`] otherwise. Never persisted.
    Restored,
    /// A rule, a built-in default, or the window's own traits (a fixed-size panel): it tiles
    /// again once none of them holds.
    Automatic,
}

/// Who pinned a float to every workspace of its display.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct Pin {
    /// `fractile ctl pin`; persisted.
    pub(crate) by_hand: bool,
    /// A `pin` rule on the latest observation; recomputed, never persisted.
    pub(crate) by_rule: bool,
}

/// Where a window lives.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Residency {
    /// A tile on `workspace`. Derived from the engines; never stored in the table.
    Tiled { workspace: WorkspaceId },
    /// Native size above `workspace`'s tiles; parks and restores with it unless pinned.
    Floating {
        workspace: WorkspaceId,
        cause: FloatCause,
        pin: Pin,
    },
    /// In the Scratchpad: parked and owned by no workspace while hidden, floating on
    /// `shown_on` while summoned. `order` is its place in the toggle cycle.
    Scratchpad {
        order: u64,
        size: Option<(i32, i32)>,
        shown_on: Option<WorkspaceId>,
    },
}

impl Residency {
    /// The workspace a float or a summoned scratchpad window floats on.
    #[must_use]
    pub(crate) const fn floating_workspace(self) -> Option<WorkspaceId> {
        match self {
            Self::Floating { workspace, .. } => Some(workspace),
            Self::Scratchpad { shown_on, .. } => shown_on,
            Self::Tiled { .. } => None,
        }
    }
}

/// Every window that is managed but not tiled: its one [`Residency`].
#[derive(Clone, Debug, Default)]
pub(crate) struct WindowTable {
    records: BTreeMap<PhysicalWindowId, Residency>,
    next_order: u64,
}

impl WindowTable {
    fn get(&self, window: &PhysicalWindowId) -> Option<Residency> {
        self.records.get(window).copied()
    }

    fn set(&mut self, window: PhysicalWindowId, residency: Residency) {
        debug_assert!(!matches!(residency, Residency::Tiled { .. }));
        self.records.insert(window, residency);
    }

    fn remove(&mut self, window: &PhysicalWindowId) -> Option<Residency> {
        self.records.remove(window)
    }

    /// The next place at the back of the scratchpad's toggle order.
    fn back(&mut self) -> u64 {
        self.next_order += 1;
        self.next_order
    }

    pub(super) fn floats(&self) -> impl Iterator<Item = (&PhysicalWindowId, WorkspaceId)> {
        self.records
            .iter()
            .filter_map(|(window, residency)| Some((window, residency.floating_workspace()?)))
    }

    pub(super) fn hand_pinned(&self) -> impl Iterator<Item = &PhysicalWindowId> {
        self.records
            .iter()
            .filter(|(_, residency)| {
                matches!(residency, Residency::Floating { pin, .. } if pin.by_hand)
            })
            .map(|(window, _)| window)
    }
}

impl Workspaces {
    /// Where `window` lives, if Fractile manages it.
    #[must_use]
    pub(crate) fn residency(&self, window: &PhysicalWindowId) -> Option<Residency> {
        self.workspace_of(window)
            .map(|workspace| Residency::Tiled { workspace })
            .or_else(|| self.residency.get(window))
    }

    #[must_use]
    pub(crate) fn floating_workspace_of(&self, window: &PhysicalWindowId) -> Option<WorkspaceId> {
        self.residency.get(window)?.floating_workspace()
    }

    pub(crate) fn floating_entries(
        &self,
    ) -> impl Iterator<Item = (&PhysicalWindowId, WorkspaceId)> {
        self.residency.floats()
    }

    pub(crate) fn floating_windows_for_app(
        &self,
        app: &crate::AppInstanceId,
    ) -> Vec<PhysicalWindowId> {
        self.floating_entries()
            .filter(|(window, _)| &window.app == app)
            .map(|(window, _)| window.clone())
            .collect()
    }

    /// Floats `window` on `workspace` for `cause`, keeping any pin it already had there.
    /// The window must hold no tile.
    pub(crate) fn float(
        &mut self,
        window: PhysicalWindowId,
        workspace: WorkspaceId,
        cause: FloatCause,
    ) {
        debug_assert!(self.workspace_of(&window).is_none());
        let pin = match self.residency.get(&window) {
            Some(Residency::Floating { pin, .. }) => pin,
            _ => Pin::default(),
        };
        self.clear_float_focus(&window, Some(workspace));
        self.residency.set(
            window,
            Residency::Floating {
                workspace,
                cause,
                pin,
            },
        );
    }

    /// Gives a float (or a window that holds no tile) to `workspace`, keeping why it floats.
    /// A summoned scratchpad window moved to another workspace leaves the scratchpad and
    /// floats there by hand.
    pub(crate) fn own_floating(&mut self, window: PhysicalWindowId, workspace: WorkspaceId) {
        let cause = match self.residency.get(&window) {
            Some(Residency::Floating { cause, .. }) => cause,
            Some(Residency::Scratchpad {
                shown_on: Some(shown),
                ..
            }) if shown == workspace => return,
            Some(Residency::Scratchpad { .. }) => FloatCause::Manual,
            Some(Residency::Tiled { .. }) | None => FloatCause::Automatic,
        };
        self.float(window, workspace, cause);
    }

    /// Drops `window`'s float, and its pins, if it has one. A summoned scratchpad window
    /// leaves the scratchpad; a hidden one stays (no workspace owns it).
    pub(crate) fn forget_floating(&mut self, window: &PhysicalWindowId) {
        if self.floating_workspace_of(window).is_none() {
            return;
        }
        self.clear_float_focus(window, None);
        self.residency.remove(window);
    }

    /// Forgets `window` as the focused float of the workspace it floats on, unless that is
    /// `keep`. Called before its residency changes.
    fn clear_float_focus(&mut self, window: &PhysicalWindowId, keep: Option<WorkspaceId>) {
        if let Some(previous) = self.floating_workspace_of(window)
            && Some(previous) != keep
            && self.last_focused_float[previous.index()].as_ref() == Some(window)
        {
            self.last_focused_float[previous.index()] = None;
        }
    }

    /// Whether `window` floats by hand: Toggle Float, the setup tour, a restored or recalled
    /// float, or a scratchpad window. Rules and traits never tile it.
    #[must_use]
    pub(crate) fn is_manual_float(&self, window: &PhysicalWindowId) -> bool {
        matches!(
            self.residency.get(window),
            Some(
                Residency::Floating {
                    cause: FloatCause::Manual | FloatCause::Restored,
                    ..
                } | Residency::Scratchpad { .. }
            )
        )
    }

    /// Settles why a restored float floats on its first observation: a matching float rule
    /// takes it, so removing the rule later tiles it again (#160); otherwise it floats by hand.
    /// A float the user set this session stays manual whatever its rules say (#493).
    pub(crate) fn settle_restored_float(&mut self, window: &PhysicalWindowId, rule_floats: bool) {
        if let Some(Residency::Floating { cause, .. }) = self.residency.records.get_mut(window)
            && *cause == FloatCause::Restored
        {
            *cause = if rule_floats {
                FloatCause::Automatic
            } else {
                FloatCause::Manual
            };
        }
    }

    /// Hands `from`'s float to `to` (a native tab took its place). Returns whether it had one.
    pub(crate) fn hand_float_to(&mut self, from: &PhysicalWindowId, to: &PhysicalWindowId) -> bool {
        match self.residency.get(from) {
            Some(residency @ Residency::Floating { .. }) => {
                self.forget_floating(from);
                self.residency.set(to.clone(), residency);
                true
            }
            _ => false,
        }
    }

    /// Tiles a window that holds no tile on `workspace`, dropping any float or scratchpad
    /// record it had: the one transition from floating to tiled.
    pub(crate) fn adopt_tile(
        &mut self,
        window: PhysicalWindowId,
        workspace: WorkspaceId,
        mode: crate::config::LayoutMode,
        output: crate::Rect,
        anchor: Option<crate::SlotId>,
    ) {
        if self.workspace_of(&window).is_some() {
            return;
        }
        self.forget_floating(&window);
        self.residency.remove(&window);
        let engine = self.engine_mut(workspace);
        let slot = crate::Slot {
            id: crate::SlotId(engine.model().next_slot_id),
            binding: crate::BindingState::Bound(window),
        };
        engine.insert_slot_with(slot, mode, output, anchor);
    }

    /// Whether `window` shows on every workspace, by hand or by a `pin` rule.
    #[must_use]
    pub(crate) fn is_pinned(&self, window: &PhysicalWindowId) -> bool {
        matches!(self.residency.get(window), Some(Residency::Floating { pin, .. }) if pin.by_hand || pin.by_rule)
    }

    /// Whether `window` was pinned by hand, as opposed to by a `pin` rule.
    #[must_use]
    pub(crate) fn is_pinned_by_hand(&self, window: &PhysicalWindowId) -> bool {
        matches!(self.residency.get(window), Some(Residency::Floating { pin, .. }) if pin.by_hand)
    }

    /// Pin a floating window by hand. A summoned scratchpad window leaves the scratchpad and
    /// stays where it is. Returns whether it is pinned (a tiled or unowned window cannot be).
    pub(crate) fn pin(&mut self, window: PhysicalWindowId) -> bool {
        let Some(workspace) = self.floating_workspace_of(&window) else {
            return false;
        };
        if matches!(
            self.residency.get(&window),
            Some(Residency::Scratchpad { .. })
        ) {
            self.float(window.clone(), workspace, FloatCause::Manual);
        }
        self.edit_pin(&window, |pin| pin.by_hand = true)
    }

    /// Drop a hand pin. Returns whether one was dropped.
    pub(crate) fn unpin(&mut self, window: &PhysicalWindowId) -> bool {
        let pinned = self.is_pinned_by_hand(window);
        self.edit_pin(window, |pin| pin.by_hand = false);
        pinned
    }

    /// Record whether a `pin` rule matches `window` on this observation.
    pub(crate) fn set_rule_pinned(&mut self, window: &PhysicalWindowId, pinned: bool) {
        self.edit_pin(window, |pin| pin.by_rule = pinned);
    }

    fn edit_pin(&mut self, window: &PhysicalWindowId, edit: impl FnOnce(&mut Pin)) -> bool {
        match self.residency.records.get_mut(window) {
            Some(Residency::Floating { pin, .. }) => {
                edit(pin);
                true
            }
            _ => false,
        }
    }

    /// The Scratchpad, in toggle order.
    #[must_use]
    pub(crate) fn scratchpad(&self) -> Scratchpad {
        let mut entries: Vec<_> = self
            .residency
            .records
            .iter()
            .filter_map(|(window, residency)| match *residency {
                Residency::Scratchpad {
                    order,
                    size,
                    shown_on,
                } => Some((
                    order,
                    ScratchpadEntry {
                        window: window.clone(),
                        size,
                        shown_on,
                    },
                )),
                _ => None,
            })
            .collect();
        entries.sort_by_key(|(order, _)| *order);
        Scratchpad::new(entries.into_iter().map(|(_, entry)| entry).collect())
    }

    /// Hides a window that holds no tile in the scratchpad, at the back of the toggle order.
    /// Returns false if it was already there.
    pub(crate) fn stash(&mut self, window: PhysicalWindowId, size: Option<(i32, i32)>) -> bool {
        if matches!(
            self.residency.get(&window),
            Some(Residency::Scratchpad { .. })
        ) {
            return false;
        }
        self.forget_floating(&window);
        let order = self.residency.back();
        self.residency.set(
            window,
            Residency::Scratchpad {
                order,
                size,
                shown_on: None,
            },
        );
        true
    }

    /// A scratchpad window was summoned onto `workspace`.
    pub(crate) fn show_scratchpad(&mut self, window: &PhysicalWindowId, workspace: WorkspaceId) {
        if let Some(Residency::Scratchpad { shown_on, .. }) = self.residency.records.get_mut(window)
        {
            *shown_on = Some(workspace);
        }
    }

    /// Hides a scratchpad window and sends it to the back of the order, so the next toggle
    /// summons the window after it. `size` replaces the remembered size when known.
    pub(crate) fn hide_scratchpad(&mut self, window: &PhysicalWindowId, size: Option<(i32, i32)>) {
        let Some(Residency::Scratchpad { size: kept, .. }) = self.residency.get(window) else {
            return;
        };
        self.clear_float_focus(window, None);
        let order = self.residency.back();
        self.residency.set(
            window.clone(),
            Residency::Scratchpad {
                order,
                size: size.or(kept),
                shown_on: None,
            },
        );
    }

    /// Takes `window` out of the scratchpad: a hidden one is owned by no workspace, a shown one
    /// floats by hand where it is shown. Returns whether it was there.
    pub(crate) fn leave_scratchpad(&mut self, window: &PhysicalWindowId) -> bool {
        let Some(Residency::Scratchpad { shown_on, .. }) = self.residency.get(window) else {
            return false;
        };
        match shown_on {
            Some(workspace) => self.float(window.clone(), workspace, FloatCause::Manual),
            None => drop(self.residency.remove(window)),
        }
        true
    }

    /// Empties the scratchpad, returning the windows it held.
    pub(crate) fn clear_scratchpad(&mut self) -> Vec<PhysicalWindowId> {
        let windows: Vec<_> = self
            .scratchpad()
            .entries()
            .iter()
            .map(|entry| entry.window.clone())
            .collect();
        for window in &windows {
            self.leave_scratchpad(window);
        }
        windows
    }
}
