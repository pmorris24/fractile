use super::*;

impl MainThreadAgent {
    /// Starts automatic tiling once Accessibility and the hotkeys are available.
    ///
    /// Hotkeys may already be registered while a first-run setup tour holds
    /// tiling, so an installed set is kept rather than registered twice.
    pub(super) fn start_automatic(&mut self) {
        let trusted = macos::accessibility_trusted();
        self.accessibility = trusted;
        if !trusted {
            self.awaiting_accessibility = true;
            self.last_error = Some("Accessibility permission is required".into());
            self.ui_dirty = true;
            status_item::set_status("Paused · Accessibility required");
            return;
        }
        self.awaiting_accessibility = false;
        if self.hotkeys.is_none() {
            match HotkeyPlan::current(&self.hotkey_workspaces, &self.shortcuts)
                .and_then(|plan| Hotkeys::install(self.actions_sender.clone(), plan))
            {
                Ok(registered) => {
                    log_event!("hotkeys: registered {} shortcuts", registered.len());
                    for skipped in registered.skipped() {
                        log_event!("hotkeys: {skipped}");
                    }
                    self.hotkeys = Some(registered);
                    self.hotkey_delivery.clear();
                }
                Err(error) => {
                    let error = format!("Hotkeys unavailable: {error}");
                    self.last_error = Some(error.clone());
                    self.ui_dirty = true;
                    self.worker.request_restore(RestoreKind::Restore);
                    status_item::set_status(&error);
                    return;
                }
            }
        }
        self.listen_for_input();
        self.automatic = true;
        self.cancelled.store(false, Ordering::Release);
        self.events.invalidate();
        self.ui_dirty = true;
        status_item::set_automatic(true);
        let status = automatic_started_status(&mut self.last_error, self.scope.automatic_status());
        status_item::set_status(status);
        self.queue_arrangement();
    }

    pub(super) fn drain_actions(&mut self) {
        while let Ok(action) = self.actions.try_recv() {
            if self.exit_intent.is_some() {
                if action == Action::Terminate {
                    termination::cancel_pending();
                }
                continue;
            }
            match action {
                Action::ToggleAutomatic if self.automatic => {
                    self.awaiting_accessibility = false;
                    self.builtin_display.on_manual_pause();
                    self.stop_automatic_and_restore(RestoreKind::Restore);
                    status_item::set_status(if self.worker.is_busy() {
                        "Paused"
                    } else {
                        "Paused · worker unavailable"
                    });
                }
                Action::ToggleAutomatic if self.worker.is_restoring_or_pending() => {
                    status_item::set_status("Restoring windows...");
                }
                Action::ToggleAutomatic => {
                    // An explicit start also ends a first-run tour's hold.
                    self.startup_tiling.setup_finished();
                    self.builtin_display
                        .on_manual_resume(self.pause_on_builtin_display, macos::display_topology());
                    self.start_automatic();
                }
                Action::FinishSetup => {
                    self.startup_tiling.setup_finished();
                    if !self.automatic {
                        if self.worker.is_restoring_or_pending() {
                            // The normal gate starts tiling once restoration ends.
                            self.awaiting_accessibility = true;
                            self.ui_dirty = true;
                        } else {
                            self.start_automatic();
                        }
                    }
                    self.ui_dirty = true;
                }
                Action::SkipSetup => self.skip_setup(),
                Action::PlanSetup => self.want_inventory(),
                Action::Arrange(arrangement) => self.arrange(arrangement),
                Action::UndoArrangement => self.undo_arrangement(),
                Action::OpenSettings => {
                    if let Err(error) = (self.callbacks.open_settings)() {
                        status_item::set_status(&format!("Could not open Settings: {error}"));
                    }
                }
                Action::OpenTarget(target) => {
                    if let Some(note) = &target.diagnostic {
                        log_event!("deep link: {note}");
                    }
                    if let Err(error) = (self.callbacks.open_target)(target) {
                        status_item::set_status(&format!("Could not open Settings: {error}"));
                    }
                }
                Action::OpenSetup => {
                    if let Err(error) = (self.callbacks.open_setup)() {
                        status_item::set_status(&format!("Could not open Setup: {error}"));
                    }
                }
                Action::OpenAccessibility => {
                    if permission_guide::prompt_accessibility_trust() {
                        permission_guide::close();
                        self.accessibility_requested = false;
                        self.ui_dirty = true;
                    } else {
                        self.accessibility_requested = true;
                        self.last_accessibility_poll = Instant::now();
                        permission_guide::show();
                    }
                }
                Action::ToggleHud => {
                    let next = !self.hud;
                    match (self.callbacks.save_hud)(next) {
                        Ok(()) => {
                            self.hud = next;
                            self.config_stamp = crate::config::modified();
                            status_item::set_hud(next);
                            self.publish_ui(true);
                        }
                        Err(error) => {
                            status_item::set_status(&format!(
                                "Could not change Workspace HUD: {error}"
                            ));
                        }
                    }
                }
                Action::SetHud(enabled) => {
                    self.hud = enabled;
                    self.config_stamp = crate::config::modified();
                    status_item::set_hud(enabled);
                    self.ui_dirty = true;
                }
                Action::SetPauseOnBuiltinDisplay(enabled) => {
                    self.set_pause_on_builtin_display(enabled);
                    self.config_stamp = crate::config::modified();
                    self.ui_dirty = true;
                }
                Action::ToggleLaunchAtLogin => {
                    let result = login_item::toggle();
                    self.launch_at_login = UiLaunchAtLogin::from_result(result.clone());
                    self.ui_dirty = true;
                    match result {
                        Ok(status) => {
                            status_item::set_launch_at_login(status);
                            if status == login_item::Status::RequiresApproval {
                                status_item::set_status(
                                    "Launch at Login requires approval in System Settings",
                                );
                            }
                        }
                        Err(error) => {
                            status_item::set_status(&format!(
                                "Could not change Launch at Login: {error}"
                            ));
                        }
                    }
                }
                Action::Restart | Action::Quit | Action::Terminate => {
                    let intent = match action {
                        Action::Restart => ExitIntent::Restart,
                        Action::Terminate => ExitIntent::Terminate,
                        _ => ExitIntent::Quit,
                    };
                    permission_guide::close();
                    self.exit_intent = Some(intent);
                    self.stop_automatic_and_restore(RestoreKind::Restore);
                    if self.worker.is_busy() {
                        status_item::set_status(match intent {
                            ExitIntent::Quit => "Restoring windows...",
                            ExitIntent::Restart => "Restarting · restoring windows...",
                            ExitIntent::Terminate => "Terminating · restoring windows...",
                        });
                    } else {
                        self.cancel_unavailable_exit();
                    }
                }
                Action::Hotkey(id) => self.hotkey_delivered(id),
                Action::OptionKeyDown(key_code, modifiers) => {
                    self.option_key_down(key_code, modifiers);
                }
                Action::Worker(action) => {
                    if self.automatic
                        && crate::app::input::admits_action(&action, self.swipe_fingers, || {
                            (
                                macos::mouse_down(),
                                self.mouse_was_down,
                                self.gesture_tracker.is_active(),
                            )
                        })
                    {
                        self.queue_worker_input(QueuedInput::Action(action));
                    }
                }
            }
        }
    }
}
