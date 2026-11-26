use super::*;

/// Shortest gap between two re-registrations caused by undelivered presses.
const HOTKEY_HEAL_COOLDOWN: Duration = Duration::from_secs(5);

impl MainThreadAgent {
    pub(super) fn hotkey_delivered(&mut self, id: u32) {
        let Some((action, description)) = self.hotkey_binding(id) else {
            return;
        };
        log_event!("hotkey: {description}");
        self.hotkey_delivery.delivered(id, Instant::now());
        if self.automatic {
            self.queue_worker_input(QueuedInput::Action(action));
        }
    }

    pub(super) fn option_key_down(&mut self, key_code: u32, modifiers: u32) {
        if let Some(id) = self
            .hotkeys
            .as_ref()
            .and_then(|hotkeys| hotkeys.id_for(key_code, modifiers))
        {
            self.hotkey_delivery.pressed(id, Instant::now());
        }
    }

    /// A shortcut the keyboard tap saw but Carbon never delivered means macOS
    /// dropped the registrations: register them again and run the missed press.
    pub(super) fn heal_missed_hotkey(&mut self) {
        let Some(id) = self.hotkey_delivery.missed(Instant::now()) else {
            return;
        };
        let Some((action, description)) = self.hotkey_binding(id) else {
            return;
        };
        if self
            .last_hotkey_heal
            .is_some_and(|healed| healed.elapsed() < HOTKEY_HEAL_COOLDOWN)
        {
            log_event!("hotkey: {description} pressed but macOS did not deliver it");
        } else {
            log_event!(
                "hotkey: {description} pressed but macOS did not deliver it; registering shortcuts again"
            );
            self.last_hotkey_heal = Some(Instant::now());
            match HotkeyPlan::current(&self.hotkey_workspaces, &self.shortcuts) {
                Ok(plan) => self.reinstall_hotkeys(plan),
                Err(error) => log_event!("hotkeys: could not plan shortcuts: {error}"),
            }
        }
        if self.automatic {
            self.queue_worker_input(QueuedInput::Action(action));
        }
    }

    pub(super) fn reinstall_hotkeys(&mut self, plan: HotkeyPlan) {
        // Carbon cannot register the replacement chords until the old ones are released.
        self.hotkeys = None;
        self.hotkey_delivery.clear();
        match Hotkeys::install(self.actions_sender.clone(), plan) {
            Ok(hotkeys) => {
                log_event!("hotkeys: registered {} shortcuts", hotkeys.len());
                for skipped in hotkeys.skipped() {
                    log_event!("hotkeys: {skipped}");
                }
                self.hotkeys = Some(hotkeys);
                clear_hotkey_reinstall_error(&mut self.last_error);
                self.ui_dirty = true;
            }
            Err(error) => {
                let error = format!("Hotkeys unavailable: {error}");
                log_event!("{error}");
                self.last_error = Some(error.clone());
                self.stop_automatic_and_restore(RestoreKind::Restore);
                status_item::set_status(&error);
            }
        }
    }

    fn hotkey_binding(&self, id: u32) -> Option<(WorkerAction, String)> {
        let (action, description) = self.hotkeys.as_ref()?.binding(id)?;
        Some((action.clone(), description.to_owned()))
    }
}

/// Clears `last_error` after re-registering the active shortcut table,
/// preserving a retained diagnostic that re-registration does not resolve
/// (`Shortcuts not applied:`).
pub(super) fn clear_hotkey_reinstall_error(last_error: &mut Option<String>) {
    if !is_shortcuts_not_applied_error(last_error.as_deref()) {
        *last_error = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hotkey_reinstall_preserves_unapplied_shortcut_errors() {
        let shortcut_reason = format!("{}: conflict", live_shortcuts::NOT_APPLIED);
        let mut last_error = Some(shortcut_reason.clone());
        clear_hotkey_reinstall_error(&mut last_error);
        assert_eq!(last_error.as_deref(), Some(shortcut_reason.as_str()));

        let mut hotkey_error = Some("Hotkeys unavailable: could not register".to_owned());
        clear_hotkey_reinstall_error(&mut hotkey_error);
        assert_eq!(hotkey_error, None);
    }
}
