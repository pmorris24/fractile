use std::{
    sync::mpsc::{self, Receiver, RecvTimeoutError, Sender},
    thread,
    time::{Duration, Instant, SystemTime},
};

use crate::{
    config::{Config, MenuBarIcon},
    login_item,
    shortcuts::Shortcuts,
    swipe::Fingers,
};

/// How long a Launch at Login reading stays fresh. Each reading is an XPC round
/// trip to `smd`; Fractile's own toggle reports its result directly, so the
/// probe only has to notice changes made in System Settings.
const LOGIN_STATUS_TTL: Duration = Duration::from_secs(30);

/// Settings the agent polls but only the user changes. Reading them means an XPC
/// round trip to `smd` and file-system calls, each of which can stall for a
/// second or more while the disk or launchd is busy, so a background thread
/// reads them and the main thread only ever receives the results.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Probe {
    /// A fresh Launch at Login reading, present only when it changed or was
    /// asked for. A cached value never overwrites what the toggle reported.
    pub(super) launch_at_login: Option<Result<login_item::Status, String>>,
    pub(super) config_stamp: Option<SystemTime>,
    /// The HUD setting read for `config_stamp`, when that config loaded.
    pub(super) hud: Option<bool>,
    /// `[shortcuts]` read for `config_stamp`, when that config loaded.
    pub(super) shortcuts: Option<Shortcuts>,
    /// The workspace swipe's finger count read for `config_stamp`, when that config loaded.
    pub(super) swipe_fingers: Option<Fingers>,
    /// `Config::menu_bar_icon` read for `config_stamp`, when that config loaded.
    pub(super) menu_bar_icon: Option<MenuBarIcon>,
    /// `[labs] focus_follows_mouse` read for `config_stamp`, when that config loaded.
    pub(super) focus_follows_mouse: Option<bool>,
    /// `settings.pause_on_builtin_display` read for `config_stamp`, when that config loaded.
    pub(super) pause_on_builtin_display: Option<bool>,
}

/// What the probe reads from a `config.toml` that loads.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct ConfigRead {
    pub(super) hud: bool,
    pub(super) shortcuts: Shortcuts,
    pub(super) swipe_fingers: Fingers,
    pub(super) menu_bar_icon: MenuBarIcon,
    pub(super) focus_follows_mouse: bool,
    pub(super) pause_on_builtin_display: bool,
}

impl ConfigRead {
    fn load() -> Option<Self> {
        Config::load().ok().map(|config| Self {
            hud: config.hud(),
            shortcuts: config.shortcuts().clone(),
            swipe_fingers: config.swipe_fingers(),
            menu_bar_icon: config.menu_bar_icon(),
            focus_follows_mouse: config.labs().focus_follows_mouse,
            pause_on_builtin_display: config.pause_on_builtin_display(),
        })
    }
}

/// The probe thread's results, and a way to ask it for a fresh login status.
pub(super) struct Prober {
    pub(super) results: Receiver<Probe>,
    refresh: Sender<()>,
}

impl Prober {
    /// Re-reads Launch at Login now instead of waiting out `LOGIN_STATUS_TTL`.
    pub(super) fn refresh_login(&self) {
        // A stopped probe thread has nothing left to refresh.
        self.refresh.send(()).unwrap_or(());
    }
}

/// What the probe thread last read.
#[derive(Default)]
struct ProbeState {
    config: Option<(Option<SystemTime>, Option<ConfigRead>)>,
    login: Option<(Instant, Result<login_item::Status, String>)>,
}

impl ProbeState {
    /// The probe to send, if anything changed. `forced` re-reads and reports
    /// the login status even when it is fresh and unchanged.
    fn next(
        &mut self,
        now: Instant,
        forced: bool,
        config_stamp: Option<SystemTime>,
        load_config: impl FnOnce() -> Option<ConfigRead>,
        login_status: impl FnOnce() -> Result<login_item::Status, String>,
    ) -> Option<Probe> {
        let (config_changed, read) = match self.config.take() {
            Some((stamp, read)) if stamp == config_stamp => (false, read),
            _ => (true, load_config()),
        };
        let hud = read.as_ref().map(|read| read.hud);
        let shortcuts = read.as_ref().map(|read| read.shortcuts.clone());
        let swipe_fingers = read.as_ref().map(|read| read.swipe_fingers);
        let menu_bar_icon = read.as_ref().map(|read| read.menu_bar_icon);
        let focus_follows_mouse = read.as_ref().map(|read| read.focus_follows_mouse);
        let pause_on_builtin_display = read.as_ref().map(|read| read.pause_on_builtin_display);
        self.config = Some((config_stamp, read));
        let login_due = forced
            || self.login.as_ref().is_none_or(|(read_at, _)| {
                now.saturating_duration_since(*read_at) >= LOGIN_STATUS_TTL
            });
        let mut launch_at_login = None;
        if login_due {
            let status = login_status();
            let changed = self.login.as_ref().is_none_or(|(_, last)| *last != status);
            if forced || changed {
                launch_at_login = Some(status.clone());
            }
            self.login = Some((now, status));
        }
        (config_changed || launch_at_login.is_some()).then_some(Probe {
            launch_at_login,
            config_stamp,
            hud,
            shortcuts,
            swipe_fingers,
            menu_bar_icon,
            focus_follows_mouse,
            pause_on_builtin_display,
        })
    }
}

/// Sends a [`Probe`] whenever one changes, checking every `interval`. The thread
/// ends when the [`Prober`] is dropped.
pub(super) fn spawn(interval: Duration) -> Prober {
    let (sender, results) = mpsc::channel();
    let (refresh, refresh_requests) = mpsc::channel();
    let spawned = thread::Builder::new()
        .name("fractile-probe".into())
        .spawn(move || {
            let mut state = ProbeState::default();
            let mut forced = false;
            loop {
                let probe = state.next(
                    Instant::now(),
                    forced,
                    crate::config::modified(),
                    ConfigRead::load,
                    login_item::status,
                );
                if let Some(probe) = probe
                    && sender.send(probe).is_err()
                {
                    return;
                }
                forced = match refresh_requests.recv_timeout(interval) {
                    Ok(()) => {
                        while refresh_requests.try_recv().is_ok() {}
                        true
                    }
                    Err(RecvTimeoutError::Timeout) => false,
                    Err(RecvTimeoutError::Disconnected) => return,
                };
            }
        });
    if let Err(error) = spawned {
        log_event!("settings probe unavailable: {error}");
    }
    Prober { results, refresh }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    fn next(
        state: &mut ProbeState,
        now: Instant,
        forced: bool,
        reads: &Cell<u32>,
        status: login_item::Status,
    ) -> Option<Probe> {
        state.next(
            now,
            forced,
            None,
            || Some(read(true)),
            || {
                reads.set(reads.get() + 1);
                Ok(status)
            },
        )
    }

    #[test]
    fn login_status_is_read_once_per_ttl() {
        let reads = Cell::new(0);
        let mut state = ProbeState::default();
        let start = Instant::now();
        let first = next(
            &mut state,
            start,
            false,
            &reads,
            login_item::Status::Disabled,
        );
        assert_eq!(
            first.and_then(|probe| probe.launch_at_login),
            Some(Ok(login_item::Status::Disabled))
        );
        for second in 1..30 {
            let at = start + Duration::from_secs(second);
            let probe = next(&mut state, at, false, &reads, login_item::Status::Disabled);
            assert_eq!(probe, None);
        }
        assert_eq!(reads.get(), 1);
        let at = start + LOGIN_STATUS_TTL;
        let probe = next(&mut state, at, false, &reads, login_item::Status::Enabled);
        assert_eq!(reads.get(), 2);
        assert_eq!(
            probe.and_then(|probe| probe.launch_at_login),
            Some(Ok(login_item::Status::Enabled))
        );
    }

    #[test]
    fn forced_refresh_reads_and_reports_even_when_fresh() {
        let reads = Cell::new(0);
        let mut state = ProbeState::default();
        let start = Instant::now();
        let status = login_item::Status::RequiresApproval;
        next(&mut state, start, false, &reads, status);
        let probe = next(&mut state, start, true, &reads, status);
        assert_eq!(reads.get(), 2);
        assert_eq!(
            probe.and_then(|probe| probe.launch_at_login),
            Some(Ok(status))
        );
    }

    #[test]
    fn config_changes_never_carry_a_cached_login_status() {
        let reads = Cell::new(0);
        let mut state = ProbeState::default();
        let start = Instant::now();
        next(
            &mut state,
            start,
            false,
            &reads,
            login_item::Status::Disabled,
        );
        let stamp = Some(SystemTime::UNIX_EPOCH);
        let probe = state.next(
            start,
            false,
            stamp,
            || Some(read(false)),
            || unreachable!("login status is still fresh"),
        );
        let probe = probe.expect("config changed");
        assert_eq!(probe.launch_at_login, None);
        assert_eq!(probe.hud, Some(false));
        assert_eq!(probe.shortcuts, Some(Shortcuts::default()));
        assert_eq!(probe.swipe_fingers, Some(Fingers::Three));
        assert_eq!(probe.menu_bar_icon, Some(MenuBarIcon::Full));
        assert_eq!(probe.focus_follows_mouse, Some(false));
        assert_eq!(probe.pause_on_builtin_display, Some(false));
    }

    fn read(hud: bool) -> ConfigRead {
        ConfigRead {
            hud,
            shortcuts: Shortcuts::default(),
            swipe_fingers: Fingers::Three,
            menu_bar_icon: MenuBarIcon::Full,
            focus_follows_mouse: false,
            pause_on_builtin_display: false,
        }
    }
}
