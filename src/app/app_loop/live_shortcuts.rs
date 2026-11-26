//! `[shortcuts]` applied while Fractile runs: a changed table re-registers the global hotkeys at
//! once, and Settings › Shortcuts holds them off while it records a chord.

use crate::shortcuts::Shortcuts;

use super::*;

/// The longest Settings may hold the hotkeys off for one recording. A recorder left listening
/// (a window that never reported losing focus) must not cost the user every shortcut.
const RECORDING_LIMIT: Duration = Duration::from_secs(60);

/// Whether Settings › Shortcuts is holding the hotkeys off for a recording, and any
/// `[shortcuts]` table saved while the hold is active.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) enum HotkeyHold {
    #[default]
    None,
    /// Hotkeys were unregistered for recording; `MainThreadAgent::shortcuts` keeps the last
    /// working table so a saved replacement that fails to plan or register rolls back to it.
    Held { pending: Option<Shortcuts> },
}

impl HotkeyHold {
    const fn is_held(&self) -> bool {
        matches!(self, Self::Held { .. })
    }
}

/// What a `[shortcuts]` change asks of the registered hotkeys.
#[derive(Debug, Eq, PartialEq)]
pub(super) enum ShortcutChange {
    /// The same hotkeys, perhaps spelled differently: nothing to do.
    Same,
    /// Keep the table for the next registration; none is installed or held now.
    Store,
    /// Hotkeys are suspended while Settings records; try this table when recording ends.
    Defer,
    /// Register the new table in place of the installed hotkeys.
    Reinstall,
}

pub(super) fn shortcut_change(
    active: &Shortcuts,
    next: &Shortcuts,
    installed: bool,
    held: bool,
) -> ShortcutChange {
    if held {
        ShortcutChange::Defer
    } else if !active.differs_from(next) {
        ShortcutChange::Same
    } else if installed {
        ShortcutChange::Reinstall
    } else {
        ShortcutChange::Store
    }
}

/// Result of attempting to activate a replacement shortcut table over the active one.
#[derive(Debug, Eq, PartialEq)]
enum InstallOutcome {
    /// The replacement table is now active and installed.
    Applied,
    /// The replacement could not be applied; the previous working table is active and `String`
    /// explains why.
    Reverted(String),
    /// Neither the replacement nor the previous table could be installed.
    Unavailable {
        error: String,
        not_applied: Option<String>,
    },
}

/// Applies `next` over `active`, rolling back to `active` if planning or registering `next`
/// fails.
///
/// `hotkeys` is `Some` during a live reload, or `None` when resuming after a recording hold (in
/// which case a planning failure must also re-register `active`).
fn apply_with_rollback<H, P>(
    active: &mut Shortcuts,
    hotkeys: &mut Option<H>,
    next: Shortcuts,
    mut plan: impl FnMut(&Shortcuts) -> Result<P, String>,
    mut install: impl FnMut(P) -> Result<H, String>,
    mut release_before_install: impl FnMut(&mut Option<H>),
) -> InstallOutcome {
    let planned = match plan(&next) {
        Ok(planned) => planned,
        Err(error) => {
            if hotkeys.is_some() {
                return InstallOutcome::Reverted(error);
            }
            return restore_active(active, hotkeys, error, plan, install);
        }
    };
    // Carbon cannot register the new chords until the old ones are released.
    release_before_install(hotkeys);
    match install(planned) {
        Ok(installed) => {
            *active = next;
            *hotkeys = Some(installed);
            InstallOutcome::Applied
        }
        Err(error) => restore_active(active, hotkeys, error, plan, install),
    }
}

fn restore_active<H, P>(
    active: &Shortcuts,
    hotkeys: &mut Option<H>,
    apply_error: String,
    mut plan: impl FnMut(&Shortcuts) -> Result<P, String>,
    mut install: impl FnMut(P) -> Result<H, String>,
) -> InstallOutcome {
    match plan(active).and_then(&mut install) {
        Ok(installed) => {
            *hotkeys = Some(installed);
            InstallOutcome::Reverted(apply_error)
        }
        Err(previous_error) => InstallOutcome::Unavailable {
            error: previous_error,
            not_applied: Some(apply_error),
        },
    }
}

impl MainThreadAgent {
    /// Applies a `[shortcuts]` table read from `config.toml`, on a Settings save or a hand edit.
    /// A table that can't be registered leaves the previous hotkeys working and says why.
    pub fn apply_shortcuts(&mut self, shortcuts: Shortcuts) {
        let installed = self.hotkeys.is_some();
        let held = self.hotkey_hold.is_held();
        match shortcut_change(&self.shortcuts, &shortcuts, installed, held) {
            ShortcutChange::Same => {
                if self.is_shortcuts_not_applied_error() {
                    self.last_error = None;
                    self.ui_dirty = true;
                }
            }
            ShortcutChange::Store => {
                log_event!("hotkeys: shortcuts changed; they register when tiling starts");
                self.shortcuts = shortcuts;
                if self.is_shortcuts_not_applied_error() {
                    self.last_error = None;
                    self.ui_dirty = true;
                }
            }
            ShortcutChange::Defer => {
                let differs = self.shortcuts.differs_from(&shortcuts);
                if let HotkeyHold::Held { pending } = &mut self.hotkey_hold {
                    *pending = differs.then_some(shortcuts);
                }
                if !differs && self.is_shortcuts_not_applied_error() {
                    self.last_error = None;
                    self.ui_dirty = true;
                }
            }
            ShortcutChange::Reinstall => {
                self.reinstall_changed_shortcuts(shortcuts);
            }
        }
    }

    pub(super) fn is_shortcuts_not_applied_error(&self) -> bool {
        self.last_error
            .as_deref()
            .is_some_and(|error| error.starts_with(NOT_APPLIED))
    }

    /// Drops any active recording hold when automatic tiling stops, storing a pending table so
    /// the next tiling start uses the latest saved shortcuts.
    pub(super) fn release_hotkey_hold_on_stop(&mut self) {
        if let HotkeyHold::Held {
            pending: Some(next),
        } = std::mem::take(&mut self.hotkey_hold)
        {
            self.shortcuts = next;
            if self.is_shortcuts_not_applied_error() {
                self.last_error = None;
                self.ui_dirty = true;
            }
        }
    }

    fn reinstall_changed_shortcuts(&mut self, next: Shortcuts) {
        let workspaces = &self.hotkey_workspaces;
        let sender = &self.actions_sender;
        let delivery = &mut self.hotkey_delivery;
        let outcome = apply_with_rollback(
            &mut self.shortcuts,
            &mut self.hotkeys,
            next,
            |shortcuts| HotkeyPlan::current(workspaces, shortcuts),
            |plan| {
                let hotkeys = Hotkeys::install(sender.clone(), plan)?;
                log_event!("hotkeys: reloaded {} shortcuts", hotkeys.len());
                for skipped in hotkeys.skipped() {
                    log_event!("hotkeys: {skipped}");
                }
                Ok(hotkeys)
            },
            |hotkeys| {
                *hotkeys = None;
                delivery.clear();
            },
        );
        match outcome {
            InstallOutcome::Applied => {
                if self.is_shortcuts_not_applied_error() {
                    self.last_error = None;
                }
                self.ui_dirty = true;
            }
            InstallOutcome::Reverted(error) => {
                self.shortcuts_not_applied(&error);
            }
            InstallOutcome::Unavailable { error, not_applied } => {
                log_event!("hotkeys: could not restore the previous shortcuts: {error}");
                let message = not_applied.as_deref().unwrap_or(&error);
                self.shortcuts_not_applied(message);
                self.stop_automatic_and_restore(RestoreKind::Restore);
                status_item::set_status(&format!("Hotkeys unavailable: {error}"));
            }
        }
    }

    fn shortcuts_not_applied(&mut self, error: &str) {
        let message = format!("{NOT_APPLIED}: {error}");
        log_event!("hotkeys: {message}");
        self.last_error = Some(message);
        self.ui_dirty = true;
    }

    /// Holds the hotkeys off for this tick, or lets them back. `recording`: Settings › Shortcuts
    /// is recording a chord, so it must reach the recorder instead of firing. `practicing`: the
    /// setup tour's Try it step has the keyboard, so its shortcuts drive the tour's miniature
    /// desktop instead of the real windows; the host says so only while the setup window is the
    /// key window, so that hold needs no time limit. The hotkeys register again, with any table
    /// saved meanwhile, the moment neither holds.
    pub fn hold_hotkeys(&mut self, recording: bool, practicing: bool) {
        let recording_for = recording.then(|| {
            self.recording_since
                .get_or_insert_with(Instant::now)
                .elapsed()
        });
        if !recording {
            self.recording_since = None;
        }
        if let Some(reason) = hold_reason(recording_for, practicing) {
            if self.hotkeys.take().is_some() {
                log_event!("hotkeys: paused while {reason}");
                self.hotkey_delivery.clear();
                self.hotkey_hold = HotkeyHold::Held { pending: None };
            }
            return;
        }
        let HotkeyHold::Held { pending } = std::mem::take(&mut self.hotkey_hold) else {
            return;
        };
        if self.hotkeys.is_some() {
            return;
        }
        log_event!("hotkeys: resumed");
        if let Some(next) = pending {
            self.reinstall_changed_shortcuts(next);
            return;
        }
        match HotkeyPlan::current(&self.hotkey_workspaces, &self.shortcuts) {
            Ok(plan) => self.reinstall_hotkeys(plan),
            Err(error) => {
                let error = format!("Hotkeys unavailable: {error}");
                log_event!("{error}");
                self.last_error = Some(error.clone());
                self.stop_automatic_and_restore(RestoreKind::Restore);
                status_item::set_status(&error);
            }
        }
    }
}

/// Why the hotkeys are held off, if they are: a recording within its limit, or the setup tour's
/// Try it step.
fn hold_reason(recording_for: Option<Duration>, practicing: bool) -> Option<&'static str> {
    if recording_for.is_some_and(|elapsed| elapsed < RECORDING_LIMIT) {
        Some("Settings records a shortcut")
    } else if practicing {
        Some("the setup tour tries its shortcuts")
    } else {
        None
    }
}

pub(super) const NOT_APPLIED: &str = "Shortcuts not applied";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        hotkeys::{CONTROL_KEY, OPTION_KEY},
        keyboard_layout::LayoutSnapshot,
    };

    fn parse(bindings: &str) -> Shortcuts {
        crate::config::Config::parse(&format!("version = 1\n[shortcuts.bindings]\n{bindings}"))
            .unwrap()
            .shortcuts()
            .clone()
    }

    fn parse_custom(defaults: bool, bindings: &str) -> Shortcuts {
        crate::config::Config::parse(&format!(
            "version = 1\n[shortcuts]\ndefaults = {defaults}\n[shortcuts.bindings]\n{bindings}"
        ))
        .unwrap()
        .shortcuts()
        .clone()
    }

    #[test]
    fn a_recording_holds_for_its_limit_and_the_tour_for_as_long_as_it_practices() {
        assert_eq!(hold_reason(None, false), None);
        assert!(hold_reason(Some(Duration::ZERO), false).is_some());
        // A recorder left listening gives the shortcuts back.
        assert_eq!(hold_reason(Some(RECORDING_LIMIT), false), None);
        // The tour's hold has no limit of its own, with or without a recording beside it.
        assert!(hold_reason(None, true).is_some());
        assert!(hold_reason(Some(RECORDING_LIMIT), true).is_some());
    }

    #[test]
    fn a_changed_table_reinstalls_only_registered_hotkeys() {
        let active = parse(r#""alt+h" = "none""#);
        let rebound = parse(r#""alt+h" = "focus right""#);
        assert_eq!(
            shortcut_change(&active, &rebound, true, false),
            ShortcutChange::Reinstall
        );
        assert_eq!(
            shortcut_change(&active, &rebound, false, false),
            ShortcutChange::Store
        );
        assert_eq!(
            shortcut_change(&active, &rebound, false, true),
            ShortcutChange::Defer
        );
    }

    #[test]
    fn a_respelled_or_untouched_table_is_no_change() {
        let active = parse(r#""alt+h" = "none""#);
        let respelled = parse(r#""Option + H" = "none""#);
        assert_eq!(
            shortcut_change(&active, &respelled, true, false),
            ShortcutChange::Same
        );
        assert_eq!(
            shortcut_change(&Shortcuts::default(), &Shortcuts::default(), true, false),
            ShortcutChange::Same
        );
    }

    #[test]
    fn recording_an_unplannable_chord_restores_the_previous_keymap_instead_of_discarding_it() {
        let layout = LayoutSnapshot::for_test(
            vec![
                (0x18, '=', false, false),
                (0x18, '+', true, false),
                (0x45, '+', false, false),
                (0x0b, 'b', false, false),
                (0x09, 'v', false, false),
            ],
            vec![],
        );
        let original = parse_custom(false, "'alt++' = 'none'\n'ctrl+alt+b' = 'balance'\n");
        let unusable = parse_custom(false, "'alt++' = 'none'\n'alt+shift+=' = 'balance'\n");
        let replacement = parse_custom(false, "'alt++' = 'none'\n'ctrl+alt+v' = 'balance'\n");

        // Start with the original keymap installed, then suspend hotkeys for recording.
        let mut active = original.clone();
        let initial_plan = HotkeyPlan::from_layout(&[], &active, &layout).unwrap();
        assert!(initial_plan.has_registration(
            0x0b,
            CONTROL_KEY | OPTION_KEY,
            &WorkerAction::Balance
        ));
        let mut hotkeys: Option<HotkeyPlan> = None; // held while Settings records
        assert_eq!(
            shortcut_change(&active, &unusable, hotkeys.is_some(), true),
            ShortcutChange::Defer
        );

        // Recording ends and tries the unusable table: planning fails, so the previous
        // working keymap (Control+Option+B) is restored and active stays intact.
        let outcome = apply_with_rollback(
            &mut active,
            &mut hotkeys,
            unusable,
            |shortcuts| HotkeyPlan::from_layout(&[], shortcuts, &layout),
            Ok,
            |slot| *slot = None,
        );
        match outcome {
            InstallOutcome::Reverted(error) => {
                assert!(
                    error.contains("another shortcut already uses that key"),
                    "{error}"
                );
            }
            other => panic!("expected Reverted, got {other:?}"),
        }
        assert_eq!(active, original);
        let restored = hotkeys
            .as_ref()
            .expect("previous working hotkeys must be restored");
        assert!(restored.has_registration(0x0b, CONTROL_KEY | OPTION_KEY, &WorkerAction::Balance));

        // Control: recording the unused Control+Option+V chord while held replaces Control+Option+B.
        hotkeys = None; // held again for the next recording
        let control = apply_with_rollback(
            &mut active,
            &mut hotkeys,
            replacement.clone(),
            |shortcuts| HotkeyPlan::from_layout(&[], shortcuts, &layout),
            Ok,
            |slot| *slot = None,
        );
        assert_eq!(control, InstallOutcome::Applied);
        assert_eq!(active, replacement);
        let applied = hotkeys.as_ref().expect("replacement hotkeys installed");
        assert!(applied.has_registration(0x09, CONTROL_KEY | OPTION_KEY, &WorkerAction::Balance));
        assert!(!applied.has_registration(0x0b, CONTROL_KEY | OPTION_KEY, &WorkerAction::Balance));
    }

    #[test]
    fn failed_registration_after_recording_hold_also_restores_previous_keymap() {
        let layout = LayoutSnapshot::for_test(
            vec![(0x0b, 'b', false, false), (0x09, 'v', false, false)],
            vec![],
        );
        let original = parse_custom(false, "'ctrl+alt+b' = 'balance'\n");
        let blocked = parse_custom(false, "'ctrl+alt+v' = 'balance'\n");

        let mut active = original.clone();
        let mut hotkeys: Option<HotkeyPlan> = None; // held during recording
        let mut calls = 0usize;
        let outcome = apply_with_rollback(
            &mut active,
            &mut hotkeys,
            blocked,
            |shortcuts| HotkeyPlan::from_layout(&[], shortcuts, &layout),
            |plan| {
                calls += 1;
                if calls == 1 {
                    Err("skipped balance layout: hotkey is unavailable (-9878)".into())
                } else {
                    Ok(plan)
                }
            },
            |slot| *slot = None,
        );
        assert_eq!(
            outcome,
            InstallOutcome::Reverted(
                "skipped balance layout: hotkey is unavailable (-9878)".into()
            )
        );
        assert_eq!(active, original);
        assert!(hotkeys.as_ref().is_some_and(|plan| plan.has_registration(
            0x0b,
            CONTROL_KEY | OPTION_KEY,
            &WorkerAction::Balance
        )));
    }
}
