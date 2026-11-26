//! Pure HUD visibility, hold, hover, and deadline state machine for one display's capsule.

use std::{cell::Cell, collections::BTreeSet};

use fractile::{
    WorkspaceId,
    app::{UiHudDisplay, UiHudToast},
};

use super::pill::{HudView, ToastView};

pub const HOLD_MS: u64 = 1_150;
/// How long a transient status toast stays on screen before fading out.
pub const TOAST_HOLD_MS: u64 = 2_500;
/// Grace period before auto-dismissing once the pointer leaves the pill or a context menu closes.
pub const RESUME_HOLD_MS: u64 = 600;
/// How long the capsule's exit gets before the panel is ordered out, so the cut removes nothing
/// that shows. Reduce Motion sets it: Rune swaps the exit spring (under 1/255 by ~140 ms) for its
/// fixed 0.18 s ease-in-out fade, which still shows at about 0.14 at 125 ms.
pub const LEAVE_MS: u64 = 170;
/// A freshly shown HUD draws its marker where the switch started for one frame, then sends it
/// on without a hitch.
pub(super) const FRESH_MS: u64 = 16;
/// Waiting on that first frame, how soon to look again.
const DRAW_RETRY_MS: u64 = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HudTransition {
    /// The HUD is off.
    Ignored,
    /// It was hidden: order the panel in.
    Show,
    /// It was up or on its way out: the capsule carries on from where it is.
    Update,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HudTick {
    Idle,
    /// The fresh capsule has drawn: the marker sets off for the visible workspace.
    Arrive,
    /// The hold ran out: the capsule starts to leave.
    Leave,
    /// The exit has played: order the panel out.
    Gone,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Stage {
    Hidden,
    Shown { hide_at_ms: u64 },
    Leaving { gone_at_ms: u64 },
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum HudContent {
    Workspaces(UiHudDisplay),
    Toast(UiHudToast),
}

/// Pure HUD visibility and deadline state machine for one display's capsule.
#[derive(Clone, Debug)]
pub struct HudFlow {
    enabled: bool,
    stage: Stage,
    content: Option<HudContent>,
    /// Where a fresh show's marker starts: the workspace the switch left on this display.
    from: Option<WorkspaceId>,
    fresh: bool,
    fresh_until_ms: u64,
    /// The view has been built since the HUD was shown, so its marker has been drawn at `from`.
    drawn: Cell<bool>,
    /// Pointer is hovering over the capsule: pauses auto-dismiss until it leaves.
    hovered: bool,
    /// A per-app context menu is tracking over the capsule: pauses auto-dismiss until it closes.
    menu_open: bool,
    /// `settings.hud_stay`: the workspace capsule never leaves on its own.
    stay: bool,
    /// The last workspace row, for the capsule kept on screen to come back to after a toast.
    last_row: Option<UiHudDisplay>,
    /// Empty workspaces visited during this HUD show, kept until hidden so rapid switches across
    /// empty workspaces don't wobble the capsule width underneath the moving marker.
    visited_empty: BTreeSet<WorkspaceId>,
}

impl Default for HudFlow {
    fn default() -> Self {
        Self::new(true)
    }
}

impl HudFlow {
    #[must_use]
    pub const fn new(enabled: bool) -> Self {
        Self {
            enabled,
            stage: Stage::Hidden,
            content: None,
            from: None,
            fresh: true,
            fresh_until_ms: 0,
            drawn: Cell::new(false),
            hovered: false,
            menu_open: false,
            stay: false,
            last_row: None,
            visited_empty: BTreeSet::new(),
        }
    }

    /// What the workspace capsule draws now; the last row stays through the exit so it never blanks.
    #[must_use]
    pub fn view(&self, still: bool) -> Option<HudView<'_>> {
        let Some(HudContent::Workspaces(row)) = self.content.as_ref() else {
            return None;
        };
        Some(HudView {
            row,
            shown: matches!(self.stage, Stage::Shown { .. }),
            fresh: self.fresh,
            focus: if self.fresh {
                self.from.or(row.visible)
            } else {
                row.visible
            },
            still,
            visited_empty: &self.visited_empty,
        })
    }

    /// What a transient status toast draws now; stays through its exit so it never blanks.
    #[must_use]
    pub fn toast_view(&self, still: bool) -> Option<ToastView<'_>> {
        let Some(HudContent::Toast(toast)) = self.content.as_ref() else {
            return None;
        };
        Some(ToastView {
            toast,
            shown: matches!(self.stage, Stage::Shown { .. }),
            still,
        })
    }

    #[must_use]
    pub(super) fn current(&self) -> Option<&UiHudDisplay> {
        match self.content.as_ref() {
            Some(HudContent::Workspaces(row)) => Some(row),
            _ => None,
        }
    }

    /// The view was built: a fresh marker has been drawn where it starts.
    pub fn mark_drawn(&self) {
        self.drawn.set(true);
    }

    #[must_use]
    pub const fn is_up(&self) -> bool {
        !matches!(self.stage, Stage::Hidden)
    }

    /// Held up: hovered, its menu open, or a workspace capsule kept on screen.
    #[must_use]
    pub const fn is_held(&self) -> bool {
        self.hovered
            || self.menu_open
            || (self.stay && matches!(self.content, Some(HudContent::Workspaces(_))))
    }

    /// Keeps the workspace capsule on screen, or lets it leave on its own again.
    pub fn set_stay(&mut self, stay: bool, now_ms: u64) {
        self.stay = stay;
        if !stay && let Stage::Shown { hide_at_ms } = &mut self.stage {
            *hide_at_ms = (*hide_at_ms).max(now_ms.saturating_add(RESUME_HOLD_MS));
        }
    }

    /// With the capsule kept on screen and gone (a toast took its place), the row to show again.
    #[must_use]
    pub fn resume_row(&self) -> Option<UiHudDisplay> {
        (self.stay && self.enabled && self.stage == Stage::Hidden)
            .then(|| self.last_row.clone())
            .flatten()
    }

    /// Updates pointer hover over the capsule. Hovering while visible pauses auto-dismiss;
    /// catching a capsule mid-exit restores it cleanly; leaving resumes auto-dismiss after a
    /// short grace period.
    pub fn set_hovered(&mut self, hovered: bool, now_ms: u64) -> bool {
        if self.stage == Stage::Hidden {
            self.hovered = false;
            return false;
        }
        let was_active = self.is_held();
        self.hovered = hovered;
        let resurrected = if hovered && matches!(self.stage, Stage::Leaving { .. }) {
            self.stage = Stage::Shown {
                hide_at_ms: now_ms.saturating_add(HOLD_MS),
            };
            true
        } else {
            false
        };
        if !hovered
            && !self.is_held()
            && let Stage::Shown { hide_at_ms } = &mut self.stage
        {
            *hide_at_ms = (*hide_at_ms).max(now_ms.saturating_add(RESUME_HOLD_MS));
        }
        resurrected || was_active != self.is_held()
    }

    /// Tracks an open per-app context menu so the capsule stays visible while the menu tracks
    /// (even when the pointer moves onto the menu) and resumes auto-dismiss once it closes.
    pub fn set_menu_open(&mut self, open: bool, now_ms: u64) -> bool {
        if self.stage == Stage::Hidden {
            self.menu_open = false;
            return false;
        }
        let was_active = self.is_held();
        self.menu_open = open;
        let resurrected = if open && matches!(self.stage, Stage::Leaving { .. }) {
            self.stage = Stage::Shown {
                hide_at_ms: now_ms.saturating_add(HOLD_MS),
            };
            true
        } else {
            false
        };
        if !open
            && !self.is_held()
            && let Stage::Shown { hide_at_ms } = &mut self.stage
        {
            *hide_at_ms = (*hide_at_ms).max(now_ms.saturating_add(RESUME_HOLD_MS));
        }
        resurrected || was_active != self.is_held()
    }

    pub fn trigger(&mut self, row: UiHudDisplay, now_ms: u64) -> HudTransition {
        if !self.enabled {
            return HudTransition::Ignored;
        }
        let hidden = self.stage == Stage::Hidden;
        if hidden {
            self.fresh = true;
            self.fresh_until_ms = now_ms.saturating_add(FRESH_MS);
            self.from = row.from;
            self.drawn.set(false);
            self.hovered = false;
            self.menu_open = false;
            self.visited_empty.clear();
        } else if self.fresh {
            if self.drawn.get() {
                // First frame already drew the starting pose: leave `fresh` now so a rapid
                // second keypress springs smoothly instead of snapping with `instant_changes()`.
                self.fresh = false;
            } else {
                // Another event before the first frame drew: still starts where the first left.
                self.from = self.from.or(row.from);
            }
        }
        // The workspace this display showed left it without a switch here (it
        // moved to another display), so it keeps no slot in this row (#419).
        if let Some(left) = self.current().and_then(|current| current.visible)
            && row.visible != Some(left)
            && row.from != Some(left)
        {
            self.visited_empty.remove(&left);
        }
        if let Some(visible) = row.visible {
            let is_empty = row
                .workspaces
                .iter()
                .find(|entry| entry.workspace == visible)
                .is_none_or(|entry| entry.tiles.is_empty() && entry.floating.is_empty());
            if is_empty {
                self.visited_empty.insert(visible);
            }
        }
        self.stage = Stage::Shown {
            hide_at_ms: now_ms.saturating_add(HOLD_MS),
        };
        self.last_row = Some(row.clone());
        self.content = Some(HudContent::Workspaces(row));
        if hidden {
            HudTransition::Show
        } else {
            HudTransition::Update
        }
    }

    /// Triggers a transient status toast (~2.5 s) regardless of whether the Workspace HUD is
    /// enabled.
    pub fn trigger_toast(&mut self, toast: UiHudToast, now_ms: u64) -> HudTransition {
        let hidden = self.stage == Stage::Hidden;
        self.fresh = false;
        self.from = None;
        self.hovered = false;
        self.menu_open = false;
        self.visited_empty.clear();
        self.stage = Stage::Shown {
            hide_at_ms: now_ms.saturating_add(TOAST_HOLD_MS),
        };
        self.content = Some(HudContent::Toast(toast));
        if hidden {
            HudTransition::Show
        } else {
            HudTransition::Update
        }
    }

    /// Advances the timers to `now_ms`.
    pub fn tick(&mut self, now_ms: u64) -> HudTick {
        if self.fresh && self.is_up() && now_ms >= self.fresh_until_ms && self.drawn.get() {
            self.fresh = false;
            return HudTick::Arrive;
        }
        match self.stage {
            Stage::Shown { hide_at_ms } if !self.is_held() && now_ms >= hide_at_ms => {
                self.leave(now_ms);
                HudTick::Leave
            }
            Stage::Leaving { gone_at_ms } if now_ms >= gone_at_ms => {
                self.stage = Stage::Hidden;
                self.hovered = false;
                self.menu_open = false;
                self.visited_empty.clear();
                HudTick::Gone
            }
            _ => HudTick::Idle,
        }
    }

    /// Until the next deadline, while one is pending.
    #[must_use]
    pub fn remaining_ms(&self, now_ms: u64) -> Option<u64> {
        let arrive = self.fresh.then(|| {
            if now_ms < self.fresh_until_ms {
                self.fresh_until_ms - now_ms
            } else {
                DRAW_RETRY_MS
            }
        });
        match self.stage {
            Stage::Hidden => None,
            Stage::Shown { .. } if self.is_held() => arrive.map(|ms| ms.max(1)),
            Stage::Shown { hide_at_ms: until } | Stage::Leaving { gone_at_ms: until } => {
                let mut remaining = until.saturating_sub(now_ms);
                if let Some(arrive) = arrive {
                    remaining = remaining.min(arrive);
                }
                Some(remaining.max(1))
            }
        }
    }

    /// Updates `enabled`; returns `true` if a shown workspace HUD starts to leave now.
    /// An active status toast keeps showing until its hold expires.
    pub fn set_enabled(&mut self, enabled: bool, now_ms: u64) -> bool {
        self.enabled = enabled;
        if !enabled
            && matches!(self.stage, Stage::Shown { .. })
            && matches!(self.content, Some(HudContent::Workspaces(_)))
        {
            self.hovered = false;
            self.menu_open = false;
            self.leave(now_ms);
            return true;
        }
        false
    }

    /// Gone at once: its display went away or another event left it out.
    pub fn dismiss(&mut self) {
        self.last_row = None;
        self.stage = Stage::Hidden;
        self.hovered = false;
        self.menu_open = false;
        self.visited_empty.clear();
    }

    fn leave(&mut self, now_ms: u64) {
        self.stage = Stage::Leaving {
            gone_at_ms: now_ms.saturating_add(LEAVE_MS),
        };
    }
}
