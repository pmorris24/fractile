//! The Scratchpad Lab (`[labs] scratchpad`): an i3/sway-style stash of windows kept off-screen
//! until summoned. This module is the pure, platform-neutral side: the order a toggle cycles
//! through the scratchpad's windows and where a summoned window lands. Membership is a window's
//! [`crate::workspace::Residency`], so a scratchpad window is never also a float or a tile.
//! `app::scratchpad` performs the native writes.
//!
//! A hidden scratchpad window is parked exactly like a window on an inactive workspace (its
//! recovery frame in `parked-windows.json`), so pause, quit, restart and crashes bring it back on
//! screen. A shown one floats on the workspace it was summoned to. None of this is persisted:
//! after a restart a recovered scratchpad window is an ordinary window again.

use crate::{PhysicalWindowId, Rect, WorkspaceId};

/// What a Scratchpad shortcut or `fractile ctl scratchpad …` asks for.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScratchpadCommand {
    /// Summon the next scratchpad window onto the focused display, or hide the one shown there.
    Toggle,
    /// Send the focused window to the scratchpad, or take a shown scratchpad window out of it.
    Mark,
}

impl ScratchpadCommand {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Toggle => "toggle",
            Self::Mark => "mark",
        }
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "toggle" => Some(Self::Toggle),
            "mark" => Some(Self::Mark),
            _ => None,
        }
    }
}

/// A summoned window covers this fraction of the display's width and height until the user
/// gives it a size of its own.
const DEFAULT_SHARE: (i32, i32) = (3, 4);
/// The smallest summoned frame, so a window remembered at a sliver stays usable.
const MIN_SIZE: (i32, i32) = (320, 240);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScratchpadEntry {
    pub window: PhysicalWindowId,
    /// The size it last floated at, kept for the next summon. `None` until it has floated.
    pub size: Option<(i32, i32)>,
    /// The workspace it floats on while shown; `None` while hidden (parked off-screen).
    pub shown_on: Option<WorkspaceId>,
}

/// The scratchpad windows in toggle order, read from their residency
/// ([`crate::workspace::Residency::Scratchpad`]): a toggle summons the first one not already
/// shown on the focused workspace, and hiding a window sends it to the back, so repeated toggles
/// cycle.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Scratchpad {
    entries: Vec<ScratchpadEntry>,
}

impl Scratchpad {
    pub(crate) const fn new(entries: Vec<ScratchpadEntry>) -> Self {
        Self { entries }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[must_use]
    pub fn contains(&self, window: &PhysicalWindowId) -> bool {
        self.entry(window).is_some()
    }

    #[must_use]
    pub fn entry(&self, window: &PhysicalWindowId) -> Option<ScratchpadEntry> {
        self.entries
            .iter()
            .find(|entry| &entry.window == window)
            .cloned()
    }

    #[must_use]
    pub fn entries(&self) -> &[ScratchpadEntry] {
        &self.entries
    }

    /// The windows shown on `workspace`, in order.
    #[must_use]
    pub fn shown_on(&self, workspace: WorkspaceId) -> Vec<PhysicalWindowId> {
        self.entries
            .iter()
            .filter(|entry| entry.shown_on == Some(workspace))
            .map(|entry| entry.window.clone())
            .collect()
    }

    /// The hidden windows, in order.
    #[must_use]
    pub fn hidden(&self) -> Vec<PhysicalWindowId> {
        self.entries
            .iter()
            .filter(|entry| entry.shown_on.is_none())
            .map(|entry| entry.window.clone())
            .collect()
    }

    /// The window a toggle on `workspace` summons: the first one not already shown there.
    #[must_use]
    pub fn next_for(&self, workspace: WorkspaceId) -> Option<PhysicalWindowId> {
        self.entries
            .iter()
            .find(|entry| entry.shown_on != Some(workspace))
            .map(|entry| entry.window.clone())
    }
}

/// Where a summoned window lands on `area` (a display's frame): centred, at its remembered
/// size or three quarters of the display, and never larger than the display.
#[must_use]
pub fn summon_frame(area: Rect, size: Option<(i32, i32)>) -> Rect {
    let (width, height) = size.unwrap_or((
        area.width * DEFAULT_SHARE.0 / DEFAULT_SHARE.1,
        area.height * DEFAULT_SHARE.0 / DEFAULT_SHARE.1,
    ));
    let width = width.max(MIN_SIZE.0).min(area.width);
    let height = height.max(MIN_SIZE.1).min(area.height);
    Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + (area.height - height) / 2,
        width,
        height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AppInstanceId;

    fn window(id: u32) -> PhysicalWindowId {
        PhysicalWindowId {
            app: AppInstanceId {
                pid: 1,
                launched_at: 1,
            },
            window_id: id,
        }
    }

    fn area() -> Rect {
        Rect {
            x: 1000,
            y: 25,
            width: 1600,
            height: 1000,
        }
    }

    #[test]
    fn summoned_windows_centre_at_three_quarters_by_default() {
        assert_eq!(
            summon_frame(area(), None),
            Rect {
                x: 1200,
                y: 150,
                width: 1200,
                height: 750,
            }
        );
    }

    #[test]
    fn summoned_windows_keep_their_size_clamped_to_the_display() {
        assert_eq!(
            summon_frame(area(), Some((800, 600))),
            Rect {
                x: 1400,
                y: 225,
                width: 800,
                height: 600,
            }
        );
        assert_eq!(summon_frame(area(), Some((5000, 5000))), area());
        let tiny = summon_frame(area(), Some((10, 10)));
        assert_eq!((tiny.width, tiny.height), MIN_SIZE);
        let small_display = Rect {
            x: 0,
            y: 0,
            width: 200,
            height: 100,
        };
        assert_eq!(summon_frame(small_display, Some((10, 10))), small_display);
    }

    #[test]
    fn toggles_cycle_through_windows_in_order() {
        let mut workspaces = crate::workspace::Workspaces::default();
        let one = WorkspaceId::ONE;
        assert_eq!(workspaces.scratchpad().next_for(one), None);
        assert!(workspaces.stash(window(1), None));
        assert!(workspaces.stash(window(2), Some((500, 400))));
        assert!(!workspaces.stash(window(1), None));
        assert_eq!(workspaces.scratchpad().len(), 2);

        assert_eq!(workspaces.scratchpad().next_for(one), Some(window(1)));
        workspaces.show_scratchpad(&window(1), one);
        assert_eq!(workspaces.scratchpad().shown_on(one), vec![window(1)]);
        assert_eq!(workspaces.floating_workspace_of(&window(1)), Some(one));
        assert_eq!(workspaces.scratchpad().hidden(), vec![window(2)]);
        // A window shown on another workspace can be summoned here.
        assert_eq!(
            workspaces.scratchpad().next_for(WorkspaceId::TWO),
            Some(window(1))
        );

        workspaces.hide_scratchpad(&window(1), Some((640, 480)));
        assert_eq!(workspaces.floating_workspace_of(&window(1)), None);
        let scratchpad = workspaces.scratchpad();
        assert_eq!(scratchpad.next_for(one), Some(window(2)));
        assert_eq!(scratchpad.entry(&window(1)).unwrap().size, Some((640, 480)));
        workspaces.show_scratchpad(&window(2), one);
        workspaces.hide_scratchpad(&window(2), None);
        let scratchpad = workspaces.scratchpad();
        assert_eq!(scratchpad.entry(&window(2)).unwrap().size, Some((500, 400)));
        assert_eq!(scratchpad.next_for(one), Some(window(1)));

        assert!(workspaces.leave_scratchpad(&window(1)));
        assert!(!workspaces.scratchpad().contains(&window(1)));
        assert_eq!(workspaces.clear_scratchpad().len(), 1);
        assert!(workspaces.scratchpad().is_empty());
    }

    #[test]
    fn a_window_has_one_residency() {
        use crate::workspace::{FloatCause, Residency};
        let mut workspaces = crate::workspace::Workspaces::default();
        let one = WorkspaceId::ONE;
        workspaces.float(window(1), one, FloatCause::Manual);
        assert!(workspaces.pin(window(1)));
        // Stashing a float drops the float and its pin.
        assert!(workspaces.stash(window(1), None));
        assert!(!workspaces.is_pinned(&window(1)));
        assert_eq!(workspaces.floating_workspace_of(&window(1)), None);
        // Moving a summoned window to another workspace takes it out of the scratchpad.
        workspaces.show_scratchpad(&window(1), one);
        workspaces.own_floating(window(1), WorkspaceId::TWO);
        assert!(!workspaces.scratchpad().contains(&window(1)));
        assert!(workspaces.is_manual_float(&window(1)));
        // Tiling a float is one transition: no float is left behind.
        workspaces.adopt_tile(
            window(1),
            WorkspaceId::TWO,
            crate::config::LayoutMode::Columns,
            Rect::default(),
            None,
        );
        assert_eq!(
            workspaces.residency(&window(1)),
            Some(Residency::Tiled {
                workspace: WorkspaceId::TWO
            })
        );
        assert_eq!(workspaces.floating_workspace_of(&window(1)), None);
    }

    #[test]
    fn commands_round_trip_their_ctl_names() {
        for command in [ScratchpadCommand::Toggle, ScratchpadCommand::Mark] {
            assert_eq!(ScratchpadCommand::parse(command.as_str()), Some(command));
        }
        assert_eq!(ScratchpadCommand::parse("show"), None);
    }
}
