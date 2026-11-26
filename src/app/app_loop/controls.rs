use super::*;

impl MainThreadAgent {
    pub(super) fn drain_controls(&mut self) {
        while let Ok(request) = self.controls.try_recv() {
            let response = match request.command {
                ControlCommand::Status => Ok(serde_json::json!({
                    "mode": if let Some(intent) = self.exit_intent {
                        intent.mode_label()
                    } else if self.worker.is_restoring_or_pending() {
                        "restoring"
                    } else if self.automatic {
                        "automatic"
                    } else {
                        "paused"
                    },
                    "busy": self.worker.is_busy(),
                    "last_error": self.last_error.as_deref(),
                    "active_workspaces": self.active_workspaces,
                })
                .to_string()),
                ControlCommand::Settings => {
                    self.accessibility = macos::accessibility_trusted();
                    Ok(self.snapshot().to_settings_json().to_string())
                }
                ControlCommand::App(ref action) => self.queue_action(action.clone(), "queued"),
                ControlCommand::Open(ref link) => {
                    let ack = format!("opened {}", link.canonical_url());
                    self.queue_action(Action::OpenTarget(link.resolve()), &ack)
                }
                ControlCommand::Debug => self
                    .latest_debug
                    .as_ref()
                    .ok_or_else(|| "runtime debug state is not ready".into())
                    .and_then(|debug| {
                        serde_json::to_string(debug).map_err(|error| error.to_string())
                    }),
                ControlCommand::Queue(ref action) if self.automatic => self.queue_action(
                    Action::Worker(action.clone()),
                    &action.queued_acknowledgement(),
                ),
                ControlCommand::Queue(_) => Err("automatic tiling is paused".into()),
                ControlCommand::Restart if self.worker.is_busy() => {
                    Err("Fractile is busy; retry restart".into())
                }
                ControlCommand::Restart => self.queue_action(Action::Restart, "restart queued"),
                ControlCommand::Pause if self.automatic => {
                    self.queue_action(Action::ToggleAutomatic, "pause queued")
                }
                ControlCommand::Pause => Ok("already paused".into()),
                ControlCommand::Resume if self.worker.is_restoring_or_pending() => {
                    Err("windows are still being restored".into())
                }
                ControlCommand::Resume if self.automatic => Ok("already automatic".into()),
                ControlCommand::Resume => {
                    self.queue_action(Action::ToggleAutomatic, "resume queued")
                }
                ControlCommand::Quit => self.queue_action(Action::Quit, "quit queued"),
            };
            request.respond(response);
        }
    }

    fn queue_action(&self, action: Action, ack: &str) -> Result<String, String> {
        self.actions_sender
            .send(action)
            .map(|()| ack.into())
            .map_err(|_| "Fractile agent is stopping".into())
    }
}
