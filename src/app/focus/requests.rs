//! The runtime's side of the focus automaton: every focus Fractile requests
//! and every departure it guards goes through here, on the worker's clock.

use super::super::*;
use super::DepartureGuard;

impl<B: WorkspaceBackend> WorkspaceRuntime<B> {
    /// The focused window, trusting a focus command still in flight over the
    /// lagging report Accessibility gives until the target app activates.
    /// With `[labs] empty_workspace_focus`, the window left focused when an
    /// empty workspace was chosen on another display counts as no focus, so
    /// commands act on the chosen workspace.
    pub(in crate::app) fn current_focus(&mut self) -> Option<crate::PhysicalWindowId> {
        let reported = self.backend.focused_window();
        if self.holds_empty_choice(reported.as_ref()) {
            return None;
        }
        self.resolve_focus(reported)
    }

    /// Resolves reported against a focus command still in flight, settling it.
    pub(in crate::app) fn resolve_focus(
        &mut self,
        reported: Option<crate::PhysicalWindowId>,
    ) -> Option<crate::PhysicalWindowId> {
        let now = self.backend.now();
        self.focus.resolve(now, reported)
    }

    /// The effective focus without settling anything, for diagnostics.
    pub(in crate::app) fn effective_focus(
        &self,
        reported: Option<crate::PhysicalWindowId>,
    ) -> Option<crate::PhysicalWindowId> {
        self.focus.effective(self.backend.now(), reported)
    }

    /// Asks the app to focus target and, when it accepts, records the request
    /// so lagging reports of left (and of any request it supersedes) stand for
    /// target until it settles. A tile focused this way is where Dwindle
    /// splits next, even before an observation confirms it (#411).
    pub(in crate::app) fn request_focus(
        &mut self,
        target: &crate::PhysicalWindowId,
        left: impl IntoIterator<Item = Option<crate::PhysicalWindowId>>,
    ) -> bool {
        if !self.backend.focus_window(target) {
            return false;
        }
        let now = self.backend.now();
        self.focus.request(now, target.clone(), left);
        if let Some(workspace) = self.workspaces.workspace_of(target) {
            self.workspaces
                .engine_mut(workspace)
                .remember_insertion_focus(target);
        }
        true
    }

    /// Guards the focus left behind so focus follow does not chase it.
    pub(in crate::app) fn guard_departure(
        &mut self,
        window: Option<crate::PhysicalWindowId>,
        app: Option<AppInstanceId>,
    ) {
        self.focus.guard_departure(DepartureGuard { window, app });
    }

    /// Guards whatever Accessibility reports now (a rollback leaves it behind).
    pub(in crate::app) fn guard_reported_focus(&mut self) {
        let window = self.backend.focused_window();
        let app = self.backend.frontmost_application();
        self.guard_departure(window, app);
    }

    pub(in crate::app) fn clear_suppressed_focus(&mut self) {
        self.focus.clear_guard();
    }

    /// Remember the workspace that just took focus. The first record after a
    /// launch seeds the previous workspace from the focused display's persisted
    /// lane history, so Option+Tab works straight away as it did before.
    pub(in crate::app) fn record_focused_workspace(&mut self, workspace: WorkspaceId) {
        let workspaces = &self.workspaces;
        self.focus.record_workspace(workspace, || {
            workspaces.previous_for(workspaces.assignment(workspace))
        });
    }
}
