//! The setup tour's arrangement on the worker: the title-free inventory its plan is made from,
//! the one-shot move of the open windows onto their workspaces, and its undo.
//!
//! An arrangement is not a rule. Windows no workspace owns yet are routed once, through the
//! same path a `route` rule takes, as the next cycle adopts them; windows already owned move
//! as a hand move does. Nothing here is written to `config.toml`.

use crate::onboarding_plan::{ArrangedSlot, Arrangement, Inventory, Lane, OpenWindow};

use super::*;

#[derive(Default)]
pub(super) struct ArrangeState {
    /// Where the arrangement sends windows no workspace owns yet, until the cycle that adopts
    /// them has run.
    targets: HashMap<crate::PhysicalWindowId, WorkspaceId>,
    /// Windows no workspace owns yet that the arrangement floats by hand once adopted.
    floats: HashSet<crate::PhysicalWindowId>,
    /// Each window's frame before the last arrangement, for Undo.
    undo: HashMap<crate::PhysicalWindowId, crate::Rect>,
}

impl<B: WorkspaceBackend> WorkspaceRuntime<B> {
    /// The workspace an arrangement under way gives `window`.
    pub(super) fn arranged_target(&self, window: &crate::PhysicalWindowId) -> Option<WorkspaceId> {
        self.arranging
            .targets
            .get(window)
            .copied()
            .filter(|target| self.arrangeable(*target))
    }

    /// Whether an arrangement under way floats `window` (owned by no workspace yet) by hand.
    pub(super) fn arranged_float(&self, window: &crate::PhysicalWindowId) -> bool {
        self.arranging.floats.contains(window)
    }

    fn arrangeable(&self, workspace: WorkspaceId) -> bool {
        self.workspaces.is_enabled(workspace)
            && self
                .scope
                .manages_selector(self.workspaces.assignment(workspace))
    }

    /// The open windows by app, with every display that has numbered workspaces: the main one
    /// first, then the others left to right, each with its name, frame and workspaces. Only
    /// bundle IDs, sizes and resizability leave here; an unreadable desktop is an empty
    /// inventory.
    pub(super) fn inventory(&mut self, outputs: &macos::DisplayOutputs) -> Inventory {
        let lane = |display: &macos::DisplayOutput| Lane {
            name: display.name.clone(),
            frame: display.frame,
            width: display.frame.width,
            workspaces: self
                .config
                .ordered_workspaces()
                .filter(|workspace| {
                    workspace.number().is_some()
                        && self.arrangeable(*workspace)
                        && outputs.resolve(self.workspaces.assignment(*workspace)).id == display.id
                })
                .collect(),
        };
        let main = outputs.main();
        let mut lanes = vec![lane(main)];
        lanes.extend(
            outputs
                .ordered()
                .into_iter()
                .filter(|display| display.id != main.id)
                .map(lane)
                .filter(|lane| !lane.workspaces.is_empty()),
        );
        let mut observation = match self.backend.snapshot(0, outputs) {
            Ok(observation) => observation,
            Err(error) => {
                log_event!("setup inventory unavailable: {error}");
                return Inventory {
                    apps: Vec::new(),
                    lanes,
                };
            }
        };
        let mut observed = HashSet::new();
        for application in &mut observation.applications {
            if let crate::AppObservation::Complete { app, windows, .. } = application {
                windows.retain(|window| !self.config.patch(&app.bundle_id, window).is_ignored());
                observed.extend(windows.iter().map(|window| window.id.clone()));
            }
        }
        let mut inventory = Inventory::from_observation(&observation, lanes);
        // Windows parked on hidden workspaces are open too.
        let mut parked: Vec<_> = self
            .parked
            .iter()
            .filter(|(window, _)| !observed.contains(*window))
            .map(|(window, frame)| (window.clone(), *frame))
            .collect();
        parked.sort_by(|left, right| left.0.cmp(&right.0));
        for (window, frame) in parked {
            let resizable = self.workspaces.workspace_of(&window).is_some();
            if let Some(bundle_id) = self.bundle_id(&window.app).map(str::to_owned) {
                inventory.add_window(
                    &bundle_id,
                    OpenWindow {
                        resizable,
                        width: frame.width,
                        height: frame.height,
                    },
                );
            }
        }
        inventory
    }

    /// Moves the open windows onto the workspaces `arrangement` gives their apps. Each app's
    /// resizable windows take its slots in window order; windows past the last slot follow it.
    pub(super) fn arrange(
        &mut self,
        arrangement: &Arrangement,
        outputs: macos::DisplayOutputs,
        cancelled: &AtomicBool,
    ) -> Result<CycleStatus, String> {
        let observation = self
            .backend
            .snapshot(0, &outputs)
            .map_err(|error| format!("Observation error: {error}"))?;
        self.arranging = ArrangeState::default();
        let mut taken: HashMap<&str, usize> = HashMap::new();
        let mut moves = Vec::new();
        let mut floats = Vec::new();
        for application in &observation.applications {
            let crate::AppObservation::Complete { app, windows, .. } = application else {
                continue;
            };
            let mut windows: Vec<_> = windows
                .iter()
                .filter(|window| !self.config.patch(&app.bundle_id, window).is_ignored())
                .collect();
            windows.sort_by(|left, right| left.id.cmp(&right.id));
            for window in &windows {
                self.arranging.undo.insert(window.id.clone(), window.frame);
            }
            let Some(arranged) = arrangement
                .apps
                .iter()
                .find(|arranged| arranged.bundle_id == app.bundle_id)
                .filter(|arranged| !arranged.slots.is_empty())
            else {
                continue;
            };
            for window in windows.into_iter().filter(|window| window.resizable) {
                let index = taken.entry(arranged.bundle_id.as_str()).or_default();
                let slot = arranged.slots[(*index).min(arranged.slots.len() - 1)];
                *index += 1;
                let owner = self.workspaces.workspace_of_any(&window.id);
                match slot {
                    ArrangedSlot::Tile(target) if !self.arrangeable(target) => {}
                    ArrangedSlot::Tile(target) => match owner {
                        Some(owner) if owner != target => moves.push((window.id.clone(), target)),
                        Some(_) => {}
                        None => {
                            self.arranging.targets.insert(window.id.clone(), target);
                        }
                    },
                    ArrangedSlot::Float => match self.workspaces.workspace_of(&window.id) {
                        Some(owner) => floats.push((window.id.clone(), owner)),
                        None => {
                            self.arranging.floats.insert(window.id.clone());
                        }
                    },
                }
            }
        }
        log_event!(
            "arrange: {} window(s) routed, {} moved, {} floated",
            self.arranging.targets.len(),
            moves.len(),
            floats.len()
        );
        for (window, owner) in floats {
            let canvas = self
                .layout_settings(output_for(&self.workspaces, owner, &outputs))
                .canvas();
            self.forget_window_on(owner, &window, canvas);
            self.workspaces
                .float(window, owner, crate::workspace::FloatCause::Manual);
        }
        let mut failed = 0;
        for (window, target) in moves {
            match self.move_window(window, target, outputs.clone(), cancelled) {
                Ok(status) => failed += status.failed,
                Err(error) => {
                    log_event!("arrange: a window stayed where it was: {error}");
                    failed += 1;
                }
            }
        }
        let cycled = self.cycle(outputs.clone(), cancelled);
        self.arranging.targets.clear();
        self.arranging.floats.clear();
        let mut status = cycled?;
        // A window routed to a workspace on screen joins its tiles now, not a cycle later.
        for resolved in self.visible_workspaces(&outputs) {
            if self.scope.manages_selector(&resolved.selector) {
                failed += self
                    .layout_workspace(resolved.workspace, resolved.output, cancelled)
                    .failed;
            }
        }
        self.persist_workspace_state_if_changed()?;
        status.failed += failed;
        Ok(status)
    }

    /// Puts every window the last arrangement saw back at the frame it had, owned by no
    /// workspace, after restoring everything parked: the desktop as it was before Arrange.
    pub(super) fn undo_arrangement(
        &mut self,
        outputs: Option<&macos::DisplayOutputs>,
        cancelled: &AtomicBool,
    ) -> WorkerResult {
        if let Some(outputs) = outputs {
            self.note_outputs(outputs);
        }
        let originals = std::mem::take(&mut self.arranging).undo;
        // A parked window comes back straight to where it was, not to its tile.
        for (window, original) in &originals {
            if self.parked.contains_key(window) {
                let _: bool = self.record_recovery(window, *original);
            }
        }
        let requested = self.parked.len();
        self.clear_scratchpad();
        let failed = self.restore_all();
        self.journal_restore("undo arrangement", requested, failed);
        let mut windows: Vec<_> = originals.into_iter().collect();
        windows.sort_by(|left, right| left.0.cmp(&right.0));
        let output = outputs.map_or_else(crate::Rect::default, macos::DisplayOutputs::union);
        let placements: Vec<_> = windows
            .iter()
            .filter(|(window, _)| !self.parked.contains_key(window))
            .enumerate()
            .map(|(index, (window, original))| crate::Placement {
                slot: crate::SlotId(index as u64),
                window: Some(window.clone()),
                frame: *original,
            })
            .collect();
        let results =
            self.backend
                .apply_managed(&placements, &Settings::for_output(output), cancelled);
        let kept = results
            .iter()
            .filter(|result| {
                !matches!(
                    result.outcome,
                    macos::ApplyOutcome::Applied | macos::ApplyOutcome::Gone
                )
            })
            .count();
        if kept > 0 {
            log_event!("undo arrangement: {kept} window(s) kept their tiled size");
        }
        for (window, _) in &windows {
            if let Some(owner) = self.workspaces.workspace_of_any(window) {
                self.forget_window(owner, window);
            }
        }
        self.flush_workspace_state();
        WorkerResult::Restored { failed }
    }
}
