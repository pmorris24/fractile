//! Interactive frame presenter: moves windows every display frame during a gesture or animation
//! without ever waiting on a slow app.
//!
//! Each app is served by a lane, a thread that owns that app's Accessibility handles. A lane keeps
//! one slot per window holding the newest target position: if an app is still busy with frame N
//! when N+1 and N+2 arrive, N+1 is overwritten in place, so nothing queues up and a slow app never
//! holds back another app's windows. Writes are position-only through cached handles, with no
//! window-list walk and no read-back while the motion runs.
//!
//! `AXEnhancedUserInterface` makes some apps (Chromium, Electron) animate or delay every frame
//! write. A lane turns it off once when its app joins a motion, keeps it off for the whole motion,
//! and turns it back on 250 ms after the app's last frame, when the presenter is told to restore
//! now (pause, quit), when it is dropped, or when the lane unwinds. Apps that don't support the
//! attribute are remembered and never asked again.
//!
//! Native handles never leave their lane: callers name windows by [`PhysicalWindowId`] and
//! positions by plain points, and each lane resolves its app's windows itself.

use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc, Condvar, Mutex, MutexGuard, PoisonError,
        atomic::{AtomicU64, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use objc2_application_services::{AXError, AXUIElement};
use objc2_core_foundation::{CFArray, CFBoolean, CFRetained, Type};

use crate::{AppInstanceId, PhysicalWindowId, Rect};

use super::ax::{
    application_element, attribute, cf_array_items, physical_window_id, set_attribute,
    set_ax_timeout, set_position,
};

/// A window's top-left corner in global display coordinates.
pub type Point = (i32, i32);

/// How long an app keeps enhanced UI off after its last frame, so taps in quick succession share
/// one hold instead of toggling the attribute between them.
pub const IDLE_RESTORE: Duration = Duration::from_millis(250);

/// The most lanes (threads) a presenter runs; past this, apps share the least busy lane.
pub const MAX_LANES: usize = 16;

/// What turning an app's enhanced UI off did.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EnhancedUi {
    /// It was on and is now off; restore it when the motion ends.
    Suspended,
    /// It was already off (or couldn't be read this time); nothing to restore.
    AlreadyOff,
    /// The app doesn't have the attribute; never ask it again.
    Unsupported,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WriteOutcome {
    Written,
    Gone,
    Failed,
}

/// One app's side of the presenter. A lane creates it with [`PresenterBackend::open`] and uses and
/// drops it on that lane's thread only, so it may own native handles.
pub trait AppSink {
    fn suspend_enhanced_ui(&mut self) -> EnhancedUi;
    fn restore_enhanced_ui(&mut self);
    fn set_position(&mut self, window: &PhysicalWindowId, position: Point) -> WriteOutcome;
}

/// Opens [`AppSink`]s. [`AxBackend`] is the real one; tests substitute a recording fake.
pub trait PresenterBackend: Send + Sync + 'static {
    type Sink: AppSink;
    fn open(&self, app: &AppInstanceId) -> Self::Sink;
}

/// Running totals, for diagnostics and tests.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PresenterStats {
    /// Frames passed to [`InteractivePresenter::submit_positions`].
    pub submitted: u64,
    /// Frames replaced by a newer one before their lane wrote them.
    pub coalesced: u64,
    pub written: u64,
    pub gone: u64,
    pub failed: u64,
    /// Frames discarded because no lane thread could be started.
    pub dropped: u64,
}

#[derive(Default)]
struct Counters {
    submitted: AtomicU64,
    coalesced: AtomicU64,
    written: AtomicU64,
    gone: AtomicU64,
    failed: AtomicU64,
    dropped: AtomicU64,
}

fn bump(counter: &AtomicU64, by: u64) {
    counter.fetch_add(by, Ordering::Relaxed);
}

struct Shared<B> {
    backend: B,
    unsupported: Mutex<HashSet<AppInstanceId>>,
    idle_restore: Duration,
    counters: Counters,
}

#[derive(Default)]
struct Mailbox {
    state: Mutex<LaneState>,
    /// Wakes the lane: new frames, a new hold, a release or shutdown.
    wake: Condvar,
    /// Signalled when the lane finishes a batch or exits.
    idle: Condvar,
}

#[derive(Default)]
struct LaneState {
    /// The newest unwritten position per window.
    frames: HashMap<PhysicalWindowId, Point>,
    apps: HashMap<AppInstanceId, Hold>,
    busy: bool,
    /// Set once the lane has taken work, so a lane started for an update can't exit before the
    /// update lands.
    served: bool,
    shutdown: bool,
    exited: bool,
}

impl LaneState {
    fn quiescent(&self, now: Instant) -> bool {
        self.exited
            || (self.frames.is_empty()
                && !self.busy
                && !self.apps.values().any(|hold| hold.release_due(now)))
    }
}

/// Whether an app's enhanced UI should stay off: held for a session, or until `release_at`.
#[derive(Clone, Copy, Debug, Default)]
struct Hold {
    held: bool,
    release_at: Option<Instant>,
}

impl Hold {
    fn release_due(self, now: Instant) -> bool {
        !self.held && self.release_at.is_some_and(|at| at <= now)
    }
}

struct Lane {
    mailbox: Arc<Mailbox>,
    thread: Option<JoinHandle<()>>,
}

/// Writes interactive window positions through per-app lanes. See the module docs.
pub struct InteractivePresenter<B: PresenterBackend = AxBackend> {
    shared: Arc<Shared<B>>,
    lanes: HashMap<u64, Lane>,
    routes: HashMap<AppInstanceId, u64>,
    next_lane: u64,
    max_lanes: usize,
    session: HashSet<AppInstanceId>,
}

impl InteractivePresenter<AxBackend> {
    #[must_use]
    pub fn new() -> Self {
        Self::with_backend(AxBackend, IDLE_RESTORE, MAX_LANES)
    }
}

impl Default for InteractivePresenter<AxBackend> {
    fn default() -> Self {
        Self::new()
    }
}

impl<B: PresenterBackend> InteractivePresenter<B> {
    #[must_use]
    pub fn with_backend(backend: B, idle_restore: Duration, max_lanes: usize) -> Self {
        Self {
            shared: Arc::new(Shared {
                backend,
                unsupported: Mutex::new(HashSet::new()),
                idle_restore,
                counters: Counters::default(),
            }),
            lanes: HashMap::new(),
            routes: HashMap::new(),
            next_lane: 0,
            max_lanes: max_lanes.max(1),
            session: HashSet::new(),
        }
    }

    /// Starts (or widens) a motion over `windows`: each of their apps turns enhanced UI off now
    /// and keeps it off until [`Self::end_session`].
    pub fn begin_session(&mut self, windows: &[PhysicalWindowId]) {
        let apps: HashSet<_> = windows.iter().map(|window| window.app.clone()).collect();
        for app in apps {
            self.session.insert(app.clone());
            self.post(&app, true, |state| {
                state.apps.insert(
                    app.clone(),
                    Hold {
                        held: true,
                        release_at: None,
                    },
                );
            });
        }
    }

    /// Sets each window's next position and wakes its app's lane. A position its lane hasn't
    /// written yet is replaced. Windows outside the session are still moved, and their app's
    /// enhanced UI comes back on once their frames stop.
    pub fn submit_positions(&mut self, positions: &[(PhysicalWindowId, Point)]) {
        let shared = Arc::clone(&self.shared);
        bump(&shared.counters.submitted, positions.len() as u64);
        let release_at = Instant::now() + shared.idle_restore;
        let mut by_app = HashMap::<AppInstanceId, Vec<(PhysicalWindowId, Point)>>::new();
        for (window, position) in positions {
            by_app
                .entry(window.app.clone())
                .or_default()
                .push((window.clone(), *position));
        }
        for (app, frames) in by_app {
            let count = frames.len() as u64;
            let held = self.session.contains(&app);
            let posted = self.post(&app, true, |state| {
                let hold = state.apps.entry(app.clone()).or_default();
                if held {
                    *hold = Hold {
                        held: true,
                        release_at: None,
                    };
                } else if !hold.held {
                    hold.release_at = Some(release_at);
                }
                for (window, position) in frames {
                    if state.frames.insert(window, position).is_some() {
                        bump(&shared.counters.coalesced, 1);
                    }
                }
            });
            if !posted {
                bump(&shared.counters.dropped, count);
            }
        }
    }

    /// Ends the motion. Frames already submitted are still written; each app's enhanced UI
    /// comes back on once it has had no frames for the idle-restore delay.
    pub fn end_session(&mut self) {
        let release_at = Instant::now() + self.shared.idle_restore;
        for app in std::mem::take(&mut self.session) {
            self.post(&app, false, |state| {
                if let Some(hold) = state.apps.get_mut(&app) {
                    hold.held = false;
                    hold.release_at = Some(release_at);
                }
            });
        }
    }

    /// Ends any motion, writes what's pending and restores every app's enhanced UI without the
    /// idle delay, waiting up to `timeout`. For pause, quit and handing windows back to the
    /// layout engine. Returns whether every lane finished in time.
    pub fn restore_now(&mut self, timeout: Duration) -> bool {
        self.session.clear();
        let now = Instant::now();
        for lane in self.lanes.values() {
            let mut state = lock(&lane.mailbox.state);
            for hold in state.apps.values_mut() {
                *hold = Hold {
                    held: false,
                    release_at: Some(now),
                };
            }
            drop(state);
            lane.mailbox.wake.notify_one();
        }
        self.wait_idle(timeout)
    }

    /// Waits up to `timeout` until every submitted frame is written and every due restore is
    /// done. Returns whether that happened in time.
    pub fn wait_idle(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        for lane in self.lanes.values() {
            let mut state = lock(&lane.mailbox.state);
            while !state.quiescent(Instant::now()) {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    return false;
                }
                state = lane
                    .mailbox
                    .idle
                    .wait_timeout(state, remaining)
                    .unwrap_or_else(PoisonError::into_inner)
                    .0;
            }
        }
        true
    }

    #[must_use]
    pub fn stats(&self) -> PresenterStats {
        let counters = &self.shared.counters;
        let read = |counter: &AtomicU64| counter.load(Ordering::Relaxed);
        PresenterStats {
            submitted: read(&counters.submitted),
            coalesced: read(&counters.coalesced),
            written: read(&counters.written),
            gone: read(&counters.gone),
            failed: read(&counters.failed),
            dropped: read(&counters.dropped),
        }
    }

    /// Lanes still running. Lanes exit once their apps are restored and idle.
    pub fn active_lanes(&mut self) -> usize {
        let exited: Vec<_> = self
            .lanes
            .iter()
            .filter(|(_, lane)| lock(&lane.mailbox.state).exited)
            .map(|(id, _)| *id)
            .collect();
        for id in exited {
            self.retire(id);
        }
        self.lanes.len()
    }

    /// Applies `update` to `app`'s lane under its lock and wakes it, starting a lane when
    /// `spawn` allows. Returns whether a lane took the update.
    fn post(
        &mut self,
        app: &AppInstanceId,
        spawn: bool,
        update: impl FnOnce(&mut LaneState),
    ) -> bool {
        let mut update = Some(update);
        for _ in 0..3 {
            let Some(id) = self.route(app, spawn) else {
                return false;
            };
            let Some(mailbox) = self.lanes.get(&id).map(|lane| Arc::clone(&lane.mailbox)) else {
                self.routes.remove(app);
                continue;
            };
            let mut state = lock(&mailbox.state);
            if state.exited {
                drop(state);
                self.retire(id);
                if !spawn {
                    return false;
                }
                continue;
            }
            if let Some(update) = update.take() {
                update(&mut state);
            }
            drop(state);
            mailbox.wake.notify_one();
            return true;
        }
        false
    }

    fn route(&mut self, app: &AppInstanceId, spawn: bool) -> Option<u64> {
        if let Some(id) = self.routes.get(app) {
            return Some(*id);
        }
        if !spawn {
            return None;
        }
        let id = if self.lanes.len() < self.max_lanes {
            self.spawn_lane().or_else(|| self.least_busy_lane())
        } else {
            self.least_busy_lane()
        }?;
        self.routes.insert(app.clone(), id);
        Some(id)
    }

    fn least_busy_lane(&self) -> Option<u64> {
        let mut load: HashMap<u64, usize> = self.lanes.keys().map(|id| (*id, 0)).collect();
        for id in self.routes.values() {
            if let Some(count) = load.get_mut(id) {
                *count += 1;
            }
        }
        load.into_iter()
            .min_by_key(|(id, count)| (*count, *id))
            .map(|(id, _)| id)
    }

    fn spawn_lane(&mut self) -> Option<u64> {
        let id = self.next_lane;
        let mailbox = Arc::new(Mailbox::default());
        let shared = Arc::clone(&self.shared);
        let lane_mailbox = Arc::clone(&mailbox);
        let thread = thread::Builder::new()
            .name(format!("fractile-presenter-{id}"))
            .spawn(move || run_lane(&shared, &lane_mailbox))
            .ok()?;
        self.next_lane += 1;
        self.lanes.insert(
            id,
            Lane {
                mailbox,
                thread: Some(thread),
            },
        );
        Some(id)
    }

    fn retire(&mut self, id: u64) {
        if let Some(mut lane) = self.lanes.remove(&id)
            && let Some(thread) = lane.thread.take()
        {
            drop(thread.join());
        }
        self.routes.retain(|_, lane| *lane != id);
    }
}

impl<B: PresenterBackend> Drop for InteractivePresenter<B> {
    /// Stops every lane; each restores the enhanced UI it turned off before exiting.
    fn drop(&mut self) {
        for lane in self.lanes.values() {
            lock(&lane.mailbox.state).shutdown = true;
            lane.mailbox.wake.notify_one();
        }
        for lane in self.lanes.values_mut() {
            if let Some(thread) = lane.thread.take() {
                drop(thread.join());
            }
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// An open sink plus what this lane did to its app's enhanced UI. Dropping it restores enhanced
/// UI if this lane turned it off, so release, shutdown and unwinding all put it back.
struct SinkSlot<S: AppSink> {
    sink: S,
    suspended: Option<EnhancedUi>,
}

impl<S: AppSink> Drop for SinkSlot<S> {
    fn drop(&mut self) {
        if self.suspended == Some(EnhancedUi::Suspended) {
            self.sink.restore_enhanced_ui();
        }
    }
}

/// Marks the lane exited however its thread ends.
struct ExitGuard<'a>(&'a Mailbox);

impl Drop for ExitGuard<'_> {
    fn drop(&mut self) {
        let mut state = lock(&self.0.state);
        state.exited = true;
        state.busy = false;
        drop(state);
        self.0.idle.notify_all();
    }
}

#[derive(Default)]
struct Batch {
    suspend: Vec<AppInstanceId>,
    frames: Vec<(PhysicalWindowId, Point)>,
    release: Vec<AppInstanceId>,
}

fn run_lane<B: PresenterBackend>(shared: &Shared<B>, mailbox: &Mailbox) {
    let _exit = ExitGuard(mailbox);
    // Declared after the guard so slots restore before the lane reports itself exited.
    let mut slots = HashMap::<AppInstanceId, SinkSlot<B::Sink>>::new();
    while let Some(batch) = next_batch(shared, mailbox, &slots) {
        for app in batch.suspend {
            let slot = open_slot(shared, &mut slots, &app);
            if slot.suspended.is_none() {
                let outcome = slot.sink.suspend_enhanced_ui();
                slot.suspended = Some(outcome);
                if outcome == EnhancedUi::Unsupported {
                    lock(&shared.unsupported).insert(app);
                }
            }
        }
        for (window, position) in batch.frames {
            let slot = open_slot(shared, &mut slots, &window.app);
            let counter = match slot.sink.set_position(&window, position) {
                WriteOutcome::Written => &shared.counters.written,
                WriteOutcome::Gone => &shared.counters.gone,
                WriteOutcome::Failed => &shared.counters.failed,
            };
            bump(counter, 1);
        }
        for app in batch.release {
            slots.remove(&app);
        }
    }
}

fn open_slot<'a, B: PresenterBackend>(
    shared: &Shared<B>,
    slots: &'a mut HashMap<AppInstanceId, SinkSlot<B::Sink>>,
    app: &AppInstanceId,
) -> &'a mut SinkSlot<B::Sink> {
    slots.entry(app.clone()).or_insert_with(|| SinkSlot {
        sink: shared.backend.open(app),
        suspended: None,
    })
}

/// Waits for and takes the lane's next work, or returns `None` when the lane should exit:
/// on shutdown, or once it has served its apps and holds none.
fn next_batch<B: PresenterBackend>(
    shared: &Shared<B>,
    mailbox: &Mailbox,
    slots: &HashMap<AppInstanceId, SinkSlot<B::Sink>>,
) -> Option<Batch> {
    let mut state = lock(&mailbox.state);
    state.busy = false;
    mailbox.idle.notify_all();
    loop {
        if state.shutdown {
            return None;
        }
        let now = Instant::now();
        let mut batch = Batch {
            frames: state.frames.drain().collect(),
            ..Batch::default()
        };
        // Apps that are held or about to move need enhanced UI off first, even when their release
        // is also due in this batch: they move, then restore.
        let unsupported = lock(&shared.unsupported);
        let mut needs_suspend: HashSet<&AppInstanceId> =
            batch.frames.iter().map(|(window, _)| &window.app).collect();
        needs_suspend.extend(
            state
                .apps
                .iter()
                .filter(|(_, hold)| hold.held)
                .map(|(app, _)| app),
        );
        batch.suspend = needs_suspend
            .into_iter()
            .filter(|app| !unsupported.contains(*app))
            .filter(|app| slots.get(*app).is_none_or(|slot| slot.suspended.is_none()))
            .cloned()
            .collect();
        drop(unsupported);
        for (app, hold) in &mut state.apps {
            if hold.release_due(now) {
                hold.release_at = None;
                batch.release.push(app.clone());
            }
        }
        state
            .apps
            .retain(|_, hold| hold.held || hold.release_at.is_some());
        if !(batch.suspend.is_empty() && batch.frames.is_empty() && batch.release.is_empty()) {
            state.busy = true;
            state.served = true;
            return Some(batch);
        }
        if state.apps.is_empty() && state.served {
            // Decided under the same lock a poster checks, so no update can land after this.
            state.exited = true;
            return None;
        }
        let next_release = state.apps.values().filter_map(|hold| hold.release_at).min();
        state = match next_release {
            Some(at) => {
                mailbox
                    .wake
                    .wait_timeout(state, at.saturating_duration_since(now))
                    .unwrap_or_else(PoisonError::into_inner)
                    .0
            }
            None => mailbox
                .wake
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner),
        };
    }
}

const ENHANCED_UI: &str = "AXEnhancedUserInterface";

/// The Accessibility backend: position-only `AXPosition` writes through handles the lane
/// resolves once per hold.
#[derive(Clone, Copy, Debug, Default)]
pub struct AxBackend;

impl PresenterBackend for AxBackend {
    type Sink = AxAppSink;

    fn open(&self, app: &AppInstanceId) -> AxAppSink {
        AxAppSink {
            app: app.clone(),
            application: application_element(app.pid),
            windows: HashMap::new(),
            enumerated: false,
        }
    }
}

/// One app's Accessibility handles, owned by its lane.
pub struct AxAppSink {
    app: AppInstanceId,
    application: CFRetained<AXUIElement>,
    windows: HashMap<u32, CFRetained<AXUIElement>>,
    /// The app's window list is read at most once per hold, the first time a window is unknown.
    enumerated: bool,
}

impl AxAppSink {
    fn window(&mut self, id: &PhysicalWindowId) -> Option<&AXUIElement> {
        if !self.windows.contains_key(&id.window_id) && !self.enumerated {
            self.enumerated = true;
            if let Ok(windows) = attribute::<CFArray>(&self.application, "AXWindows") {
                for window in cf_array_items::<AXUIElement>(&windows) {
                    if let Some(found) = physical_window_id(&self.app, window) {
                        set_ax_timeout(window);
                        self.windows.insert(found.window_id, window.retain());
                    }
                }
            }
        }
        self.windows.get(&id.window_id).map(|window| &**window)
    }
}

impl AppSink for AxAppSink {
    fn suspend_enhanced_ui(&mut self) -> EnhancedUi {
        match attribute::<CFBoolean>(&self.application, ENHANCED_UI) {
            Ok(value) if value.value() => {
                if set_attribute(&self.application, ENHANCED_UI, CFBoolean::new(false))
                    == AXError::Success
                {
                    EnhancedUi::Suspended
                } else {
                    EnhancedUi::AlreadyOff
                }
            }
            Err(AXError::AttributeUnsupported) => EnhancedUi::Unsupported,
            _ => EnhancedUi::AlreadyOff,
        }
    }

    fn restore_enhanced_ui(&mut self) {
        let _: AXError = set_attribute(&self.application, ENHANCED_UI, CFBoolean::new(true));
    }

    fn set_position(&mut self, window: &PhysicalWindowId, (x, y): Point) -> WriteOutcome {
        if window.app != self.app {
            return WriteOutcome::Gone;
        }
        let Some(element) = self.window(window) else {
            return WriteOutcome::Gone;
        };
        match set_position(
            element,
            Rect {
                x,
                y,
                ..Rect::default()
            },
        ) {
            AXError::Success => WriteOutcome::Written,
            AXError::InvalidUIElement => {
                self.windows.remove(&window.window_id);
                WriteOutcome::Gone
            }
            _ => WriteOutcome::Failed,
        }
    }
}

#[cfg(test)]
mod tests;
