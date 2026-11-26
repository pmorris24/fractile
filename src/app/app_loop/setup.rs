//! The setup tour's requests on the main loop: its inventory of open windows, the arrangement
//! that starts tiling, and its undo.

use crate::onboarding_plan::Arrangement;

use super::*;

/// Whether automatic tiling may start without the user asking for it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::app) enum TilingStart {
    /// Start now: Accessibility is granted and nothing is holding tiling.
    Now,
    /// Hold: the first-run setup tour is still open.
    AfterSetup,
}

/// Automatic tiling's first start, held while a first-run setup tour is open.
///
/// The tour grants Accessibility while the user is still reading it, and the
/// grant is noticed on the next tick. Starting tiling then rearranges every
/// window behind the setup window, so the tour finishes first.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::app) struct TilingStartup {
    setup_tour_open: bool,
}

impl TilingStartup {
    pub(in crate::app) const fn new(setup_tour_open: bool) -> Self {
        Self { setup_tour_open }
    }

    /// Accessibility became available, or the loop noticed it now.
    pub(in crate::app) fn accessibility_granted(&mut self) -> TilingStart {
        if self.setup_tour_open {
            TilingStart::AfterSetup
        } else {
            TilingStart::Now
        }
    }

    /// Whether a first-run tour still holds tiling.
    pub(in crate::app) const fn held(&self) -> bool {
        self.setup_tour_open
    }

    /// The first-run tour finished, was dismissed, or the user asked for tiling
    /// directly. A manual re-open of the tour never holds tiling, so this is only
    /// ever called for a first-run tour or an explicit start.
    pub(in crate::app) fn setup_finished(&mut self) -> TilingStart {
        self.setup_tour_open = false;
        TilingStart::Now
    }
}

/// What the setup tour has asked for and the worker has not taken yet.
#[derive(Default)]
pub(super) struct SetupRequests {
    inventory: bool,
    /// Applied as the first input once automatic tiling is on.
    arrangement: Option<Arrangement>,
}

impl MainThreadAgent {
    pub(super) fn want_inventory(&mut self) {
        self.setup.inventory = true;
    }

    /// Asks the worker for the inventory once it is idle and may read windows.
    pub(super) fn send_wanted_inventory(&mut self) {
        if self.setup.inventory
            && self.accessibility
            && self.exit_intent.is_none()
            && !self.worker.is_busy()
            && !self.worker.is_restoring_or_pending()
            && self.worker.send_with_outputs(WorkerCommand::Inventory)
        {
            self.setup.inventory = false;
        }
    }

    /// Ends a first-run tour's hold without starting tiling: windows stay where they are
    /// until tiling is turned on from the menu bar or Settings, or Fractile next launches.
    pub(super) fn skip_setup(&mut self) {
        self.startup_tiling.setup_finished();
        self.setup.arrangement = None;
        if !self.automatic {
            self.awaiting_accessibility = false;
            status_item::set_status(self.paused_status());
        }
        self.ui_dirty = true;
    }

    /// Starts tiling, as finishing the tour does, with `arrangement` applied first.
    pub(super) fn arrange(&mut self, arrangement: Arrangement) {
        self.setup.arrangement = Some(arrangement);
        self.startup_tiling.setup_finished();
        if self.automatic {
            self.queue_arrangement();
        } else if self.worker.is_restoring_or_pending() {
            // The normal gate starts tiling once restoration ends.
            self.awaiting_accessibility = true;
        } else {
            self.start_automatic();
        }
        self.ui_dirty = true;
    }

    /// Queues a waiting arrangement ahead of the first retile. Called whenever tiling starts.
    pub(super) fn queue_arrangement(&mut self) {
        if let Some(arrangement) = self.setup.arrangement.take() {
            self.queue_worker_input(QueuedInput::Action(WorkerAction::Arrange(arrangement)));
        }
    }

    /// Pauses tiling and has the restore put back the frames the arrangement changed.
    pub(super) fn undo_arrangement(&mut self) {
        self.setup.arrangement = None;
        self.awaiting_accessibility = false;
        self.stop_automatic_and_restore(RestoreKind::Undo);
        status_item::set_status(if self.worker.is_busy() {
            "Paused"
        } else {
            "Paused · worker unavailable"
        });
    }
}
