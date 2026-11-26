//! Auto-pause when only the built-in display is connected, and auto-resume when an external
//! display reconnects (`settings.pause_on_builtin_display`).

use std::time::{Duration, Instant};

use crate::macos::{self, DisplayTopology};

use super::*;

pub(super) const TOAST_PAUSED: &str = "Tiling paused — built-in display";
pub(super) const TOAST_RESUMED: &str = "Tiling resumed — external display";
pub(super) const PAUSED_STATUS: &str = "Paused · built-in display";
/// How often the main loop checks connected display topology when the feature is active.
const DISPLAY_TOPOLOGY_INTERVAL: Duration = Duration::from_millis(500);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum BuiltinDisplayAction {
    None,
    AutoPause,
    AutoResume,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct BuiltinDisplayPolicy {
    /// True only when Fractile itself paused tiling because only a built-in display was attached.
    pub(super) auto_paused_for_builtin: bool,
    /// Built-in-only topology where the user explicitly resumed tiling; suppresses immediate
    /// re-pausing until the display topology actually changes.
    manual_resume_topology: Option<DisplayTopology>,
}

impl BuiltinDisplayPolicy {
    /// Decides whether the observed `topology` should auto-pause or auto-resume tiling.
    #[must_use]
    pub(super) fn evaluate(
        &mut self,
        enabled: bool,
        automatic: bool,
        trusted: bool,
        restoring: bool,
        topology: DisplayTopology,
    ) -> BuiltinDisplayAction {
        if self
            .manual_resume_topology
            .is_some_and(|saved| saved != topology)
        {
            self.manual_resume_topology = None;
        }
        if !enabled {
            self.auto_paused_for_builtin = false;
            return BuiltinDisplayAction::None;
        }
        if automatic && topology.is_builtin_only() {
            if self.manual_resume_topology == Some(topology) {
                return BuiltinDisplayAction::None;
            }
            self.auto_paused_for_builtin = true;
            return BuiltinDisplayAction::AutoPause;
        }
        if !automatic && self.auto_paused_for_builtin && topology.has_external() {
            if !trusted || restoring {
                return BuiltinDisplayAction::None;
            }
            self.auto_paused_for_builtin = false;
            self.manual_resume_topology = None;
            return BuiltinDisplayAction::AutoResume;
        }
        BuiltinDisplayAction::None
    }

    /// Startup gate: when `enabled` and only a built-in display is connected at launch, start
    /// already auto-paused so windows are not briefly tiled and immediately restored.
    #[must_use]
    pub(super) fn should_start_auto_paused(
        &mut self,
        enabled: bool,
        topology: Option<DisplayTopology>,
    ) -> bool {
        if enabled && topology.is_some_and(DisplayTopology::is_builtin_only) {
            self.auto_paused_for_builtin = true;
            true
        } else {
            false
        }
    }

    pub(super) fn mark_auto_paused(&mut self) {
        self.auto_paused_for_builtin = true;
    }

    pub(super) fn clear_auto_pause(&mut self) {
        self.auto_paused_for_builtin = false;
    }

    /// User manually paused tiling: clear auto-pause so reconnecting displays never resumes it.
    pub(super) fn on_manual_pause(&mut self) {
        self.auto_paused_for_builtin = false;
        self.manual_resume_topology = None;
    }

    /// User manually resumed tiling: clear auto-pause and suppress immediate re-pausing while
    /// the current built-in-only display topology stays unchanged.
    pub(super) fn on_manual_resume(&mut self, enabled: bool, topology: Option<DisplayTopology>) {
        self.auto_paused_for_builtin = false;
        self.manual_resume_topology = if enabled {
            topology.filter(|t| t.is_builtin_only())
        } else {
            None
        };
    }

    /// Updates state when `settings.pause_on_builtin_display` changes at runtime.
    pub(super) fn on_setting_changed(&mut self, enabled: bool) {
        self.manual_resume_topology = None;
        if !enabled {
            self.auto_paused_for_builtin = false;
        }
    }
}

impl MainThreadAgent {
    pub(super) fn set_pause_on_builtin_display(&mut self, enabled: bool) {
        if self.pause_on_builtin_display == enabled {
            return;
        }
        self.pause_on_builtin_display = enabled;
        self.builtin_display.on_setting_changed(enabled);
        self.display_topology_dirty = true;
    }

    pub(super) fn tick_builtin_display(&mut self) {
        if self.exit_intent.is_some()
            || self.startup_tiling.held()
            || (!self.pause_on_builtin_display && !self.builtin_display.auto_paused_for_builtin)
        {
            return;
        }
        let due = self.display_topology_dirty
            || self.last_display_topology_check.elapsed() >= DISPLAY_TOPOLOGY_INTERVAL;
        if !due {
            return;
        }
        self.display_topology_dirty = false;
        self.last_display_topology_check = Instant::now();
        let Some(topology) = macos::display_topology() else {
            return;
        };
        let trusted = macos::accessibility_trusted();
        let action = self.builtin_display.evaluate(
            self.pause_on_builtin_display,
            self.automatic,
            trusted,
            self.worker.is_restoring_or_pending(),
            topology,
        );
        match action {
            BuiltinDisplayAction::None => {}
            BuiltinDisplayAction::AutoPause => {
                self.awaiting_accessibility = false;
                self.stop_automatic_and_restore(RestoreKind::Restore);
                self.builtin_display.mark_auto_paused();
                log_event!("auto-paused tiling: built-in display only");
                status_item::set_status(if self.worker.is_busy() {
                    PAUSED_STATUS
                } else {
                    "Paused · worker unavailable"
                });
                self.bridge.publish_toast(UiHudToast::paused(TOAST_PAUSED));
            }
            BuiltinDisplayAction::AutoResume => {
                self.start_automatic();
                if self.automatic {
                    log_event!("auto-resumed tiling: external display connected");
                    self.bridge
                        .publish_toast(UiHudToast::resumed(TOAST_RESUMED));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BUILTIN_ONLY: DisplayTopology = DisplayTopology {
        builtin: 1,
        external: 0,
    };
    const DOCKED: DisplayTopology = DisplayTopology {
        builtin: 1,
        external: 1,
    };
    const CLAMSHELL: DisplayTopology = DisplayTopology {
        builtin: 0,
        external: 1,
    };
    const STUDIO_SINGLE: DisplayTopology = DisplayTopology {
        builtin: 0,
        external: 1,
    };
    const STUDIO_DUAL: DisplayTopology = DisplayTopology {
        builtin: 0,
        external: 2,
    };

    #[test]
    fn disconnect_external_auto_pauses_and_reconnect_auto_resumes() {
        let mut policy = BuiltinDisplayPolicy::default();

        // Docked laptop stays active.
        assert_eq!(
            policy.evaluate(true, true, true, false, DOCKED),
            BuiltinDisplayAction::None
        );
        assert!(!policy.auto_paused_for_builtin);

        // Unplugging external display triggers auto-pause.
        assert_eq!(
            policy.evaluate(true, true, true, false, BUILTIN_ONLY),
            BuiltinDisplayAction::AutoPause
        );
        assert!(policy.auto_paused_for_builtin);

        // Staying on built-in display while paused does nothing.
        assert_eq!(
            policy.evaluate(true, false, true, false, BUILTIN_ONLY),
            BuiltinDisplayAction::None
        );
        assert!(policy.auto_paused_for_builtin);

        // Reconnecting external while still restoring waits until restoration completes.
        assert_eq!(
            policy.evaluate(true, false, true, true, DOCKED),
            BuiltinDisplayAction::None
        );
        assert!(policy.auto_paused_for_builtin);

        // Reconnecting external without Accessibility permission does not resume.
        assert_eq!(
            policy.evaluate(true, false, false, false, DOCKED),
            BuiltinDisplayAction::None
        );
        assert!(policy.auto_paused_for_builtin);

        // Once Accessibility is trusted and restoration is done, reconnect auto-resumes.
        assert_eq!(
            policy.evaluate(true, false, true, false, DOCKED),
            BuiltinDisplayAction::AutoResume
        );
        assert!(!policy.auto_paused_for_builtin);
    }

    #[test]
    fn clamshell_reconnect_auto_resumes_too() {
        let mut policy = BuiltinDisplayPolicy::default();
        assert_eq!(
            policy.evaluate(true, true, true, false, BUILTIN_ONLY),
            BuiltinDisplayAction::AutoPause
        );
        assert_eq!(
            policy.evaluate(true, false, true, false, CLAMSHELL),
            BuiltinDisplayAction::AutoResume
        );
        assert!(!policy.auto_paused_for_builtin);
    }

    #[test]
    fn manual_pause_on_external_is_never_overridden_on_reconnect() {
        let mut policy = BuiltinDisplayPolicy::default();

        // User manually pauses while docked.
        policy.on_manual_pause();
        assert!(!policy.auto_paused_for_builtin);

        // Unplugging while manually paused does not mark auto-paused.
        assert_eq!(
            policy.evaluate(true, false, true, false, BUILTIN_ONLY),
            BuiltinDisplayAction::None
        );
        assert!(!policy.auto_paused_for_builtin);

        // Reconnecting external display NEVER auto-resumes a manual pause.
        assert_eq!(
            policy.evaluate(true, false, true, false, DOCKED),
            BuiltinDisplayAction::None
        );
        assert!(!policy.auto_paused_for_builtin);
    }

    #[test]
    fn manual_resume_on_builtin_stays_active_until_display_topology_changes() {
        let mut policy = BuiltinDisplayPolicy::default();

        // Auto-paused after unplugging external monitor.
        assert_eq!(
            policy.evaluate(true, true, true, false, BUILTIN_ONLY),
            BuiltinDisplayAction::AutoPause
        );
        assert!(policy.auto_paused_for_builtin);

        // User manually resumes while still on the built-in display.
        policy.on_manual_resume(true, Some(BUILTIN_ONLY));
        assert!(!policy.auto_paused_for_builtin);

        // Subsequent ticks on the same built-in display must NOT re-pause the user.
        assert_eq!(
            policy.evaluate(true, true, true, false, BUILTIN_ONLY),
            BuiltinDisplayAction::None
        );
        assert!(!policy.auto_paused_for_builtin);

        // Connecting an external display changes topology and clears the suppression.
        assert_eq!(
            policy.evaluate(true, true, true, false, DOCKED),
            BuiltinDisplayAction::None
        );

        // Disconnecting the external display again auto-pauses cleanly.
        assert_eq!(
            policy.evaluate(true, true, true, false, BUILTIN_ONLY),
            BuiltinDisplayAction::AutoPause
        );
        assert!(policy.auto_paused_for_builtin);
    }

    #[test]
    fn desktop_mac_with_zero_builtin_displays_never_auto_pauses() {
        let mut policy = BuiltinDisplayPolicy::default();

        for topology in [STUDIO_DUAL, STUDIO_SINGLE, DisplayTopology::default()] {
            assert_eq!(
                policy.evaluate(true, true, true, false, topology),
                BuiltinDisplayAction::None
            );
            assert!(!policy.auto_paused_for_builtin);
        }
        assert!(!policy.should_start_auto_paused(true, Some(STUDIO_SINGLE)));
        assert!(!policy.should_start_auto_paused(true, Some(STUDIO_DUAL)));
    }

    #[test]
    fn disabled_setting_never_auto_pauses_and_clears_auto_pause_state() {
        let mut policy = BuiltinDisplayPolicy::default();

        assert_eq!(
            policy.evaluate(false, true, true, false, BUILTIN_ONLY),
            BuiltinDisplayAction::None
        );
        assert!(!policy.should_start_auto_paused(false, Some(BUILTIN_ONLY)));

        // Auto-pause, then disable the setting: reconnecting must not auto-resume.
        assert_eq!(
            policy.evaluate(true, true, true, false, BUILTIN_ONLY),
            BuiltinDisplayAction::AutoPause
        );
        policy.on_setting_changed(false);
        assert!(!policy.auto_paused_for_builtin);
        assert_eq!(
            policy.evaluate(false, false, true, false, DOCKED),
            BuiltinDisplayAction::None
        );
    }

    #[test]
    fn startup_auto_pause_sets_flag_only_when_enabled_and_builtin_only() {
        let mut policy = BuiltinDisplayPolicy::default();
        assert!(!policy.should_start_auto_paused(true, Some(DOCKED)));
        assert!(!policy.auto_paused_for_builtin);

        assert!(policy.should_start_auto_paused(true, Some(BUILTIN_ONLY)));
        assert!(policy.auto_paused_for_builtin);

        // Plugging in an external display later resumes cleanly.
        assert_eq!(
            policy.evaluate(true, false, true, false, DOCKED),
            BuiltinDisplayAction::AutoResume
        );
        assert!(!policy.auto_paused_for_builtin);
    }
}
