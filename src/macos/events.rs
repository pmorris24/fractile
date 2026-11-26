use std::{
    cell::Cell,
    collections::{HashMap, HashSet},
    ffi::c_void,
    io::{self, PipeReader, PipeWriter, Read, Write},
    os::fd::{AsRawFd, RawFd},
    ptr::{self, NonNull},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use block2::RcBlock;
use objc2::{rc::Retained, rc::autoreleasepool, runtime::ProtocolObject};
use objc2_app_kit::{
    NSApplicationActivationPolicy, NSRunningApplication, NSWorkspace,
    NSWorkspaceActiveSpaceDidChangeNotification, NSWorkspaceDidActivateApplicationNotification,
    NSWorkspaceDidHideApplicationNotification, NSWorkspaceDidLaunchApplicationNotification,
    NSWorkspaceDidTerminateApplicationNotification, NSWorkspaceDidUnhideApplicationNotification,
};
use objc2_application_services::{AXError, AXObserver, AXUIElement};
use objc2_core_foundation::{
    CFArray, CFFileDescriptor, CFOptionFlags, CFRetained, CFRunLoop, CFRunLoopSource, CFString,
    kCFFileDescriptorReadCallBack, kCFRunLoopDefaultMode,
};
use objc2_core_graphics::CGWindowListOption;
use objc2_foundation::{NSNotification, NSNotificationName, NSObjectProtocol};

use crate::AppInstanceId;

use super::{
    app_instance_with_pid,
    ax::{application_element, attribute, cf_array_items, set_ax_timeout, window_server_windows},
    is_own_application,
};

const FALLBACK_POLL_SECONDS: f64 = 0.1;
/// How often the AX thread re-reads the running applications with no launch or
/// terminate notification. It catches processes that own windows without
/// posting `NSWorkspaceDidLaunchApplicationNotification` (trampoline launches).
const SAFETY_SWEEP_INTERVAL: Duration = Duration::from_secs(15);
/// First wait before retrying an application whose observer could not be
/// created (not yet ready after launch, or a process that never answers AX).
/// Each failure doubles it, up to `REGISTRATION_RETRY_MAX`.
const REGISTRATION_RETRY_MIN: Duration = Duration::from_millis(500);
const REGISTRATION_RETRY_MAX: Duration = Duration::from_secs(60);
/// Title changes (terminal spinners, page loads) refresh at most this often.
/// Titles only matter to `title` rules, which tolerate this much delay.
const TITLE_REFRESH_INTERVAL: Duration = Duration::from_millis(500);

pub struct WindowEventMonitor {
    invalidation: Arc<EventInvalidation>,
    waker: Arc<Waker>,
    stopped: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    /// `NSWorkspace` notification tokens, removed on drop.
    workspace_observers: Vec<Retained<ProtocolObject<dyn NSObjectProtocol>>>,
    /// When a refresh last covered title changes, to coalesce them.
    last_title_refresh: Cell<Option<Instant>>,
}

#[derive(Default)]
struct EventInvalidation {
    dirty: AtomicBool,
    window_created: AtomicBool,
    /// A window's title changed; folded into `dirty` at most every
    /// `TITLE_REFRESH_INTERVAL`.
    title_changed: AtomicBool,
    /// `NSWorkspace` reported a different active application.
    frontmost_changed: AtomicBool,
    /// The AX thread should re-read the running applications now.
    sweep_requested: AtomicBool,
}

impl EventInvalidation {
    /// An application was hidden or unhidden: its windows left or rejoined the
    /// screen, so the next tick observes at once.
    fn app_visibility_changed(&self) {
        self.dirty.store(true, Ordering::Release);
    }

    fn take_dirty(&self, last_title_refresh: &Cell<Option<Instant>>, now: Instant) -> bool {
        let dirty = self.dirty.swap(false, Ordering::AcqRel);
        let titles = !dirty
            && title_refresh_due(last_title_refresh.get(), now)
            && self.title_changed.swap(false, Ordering::AcqRel);
        if dirty {
            // This refresh reads every title, so pending title changes are covered.
            self.title_changed.store(false, Ordering::Release);
        }
        if dirty || titles {
            last_title_refresh.set(Some(now));
        }
        dirty || titles
    }
}

/// The write end of the AX thread's wake pipe, shared with the `NSWorkspace`
/// notification blocks. Closing it wakes the thread with EOF so it stops.
struct Waker {
    writer: Mutex<Option<PipeWriter>>,
}

impl Waker {
    fn wake(&self) {
        if let Ok(writer) = self.writer.lock()
            && let Some(mut writer) = writer.as_ref()
        {
            // `WouldBlock` means an unread wake byte is already queued in the pipe buffer.
            drop(writer.write(&[1]));
        }
    }

    fn close(&self) {
        if let Ok(mut writer) = self.writer.lock() {
            *writer = None;
        }
    }
}

impl WindowEventMonitor {
    #[must_use]
    pub fn new() -> Self {
        let invalidation = Arc::new(EventInvalidation {
            dirty: AtomicBool::new(true),
            sweep_requested: AtomicBool::new(true),
            ..EventInvalidation::default()
        });
        let stopped = Arc::new(AtomicBool::new(false));
        let (reader, writer) = match nonblocking_pipe() {
            Some((reader, writer)) => (Some(reader), Some(writer)),
            None => (None, None),
        };
        let waker = Arc::new(Waker {
            writer: Mutex::new(writer),
        });
        let thread_invalidation = Arc::clone(&invalidation);
        let thread_stopped = Arc::clone(&stopped);
        let thread = thread::Builder::new()
            .name("fractile-ax-events".into())
            .spawn(move || run_event_loop(reader, &thread_invalidation, &thread_stopped))
            .ok();
        let workspace_observers = observe_workspace(&invalidation, &waker);
        Self {
            invalidation,
            waker,
            stopped,
            thread,
            workspace_observers,
            last_title_refresh: Cell::new(None),
        }
    }

    /// Whether windows changed since the last call. Title-only changes count
    /// at most once per `TITLE_REFRESH_INTERVAL`; the rest stay pending.
    pub fn take_dirty(&self) -> bool {
        self.invalidation
            .take_dirty(&self.last_title_refresh, Instant::now())
    }

    pub fn take_window_created(&self) -> bool {
        self.invalidation
            .window_created
            .swap(false, Ordering::AcqRel)
    }

    /// Whether `NSWorkspace` activated another application since the last call.
    pub fn take_frontmost_changed(&self) -> bool {
        self.invalidation
            .frontmost_changed
            .swap(false, Ordering::AcqRel)
    }

    pub fn invalidate(&self) {
        self.invalidation.dirty.store(true, Ordering::Release);
    }

    /// Asks the AX thread to re-read the running applications. Launches and
    /// terminations already do this, so callers rarely need to.
    pub fn refresh_applications(&self) {
        self.invalidation
            .sweep_requested
            .store(true, Ordering::Release);
        self.waker.wake();
    }
}

impl Default for WindowEventMonitor {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for WindowEventMonitor {
    fn drop(&mut self) {
        let center = NSWorkspace::sharedWorkspace().notificationCenter();
        for observer in self.workspace_observers.drain(..) {
            // SAFETY: `observer` is a token returned by `addObserverForName:...` on this center.
            unsafe { center.removeObserver(observer.as_ref()) };
        }
        self.stopped.store(true, Ordering::Release);
        // Closing the write end wakes `CFFileDescriptor` immediately with EOF.
        self.waker.close();
        if let Some(thread) = self.thread.take() {
            drop(thread.join());
        }
    }
}

fn title_refresh_due(last: Option<Instant>, now: Instant) -> bool {
    last.is_none_or(|last| now.saturating_duration_since(last) >= TITLE_REFRESH_INTERVAL)
}

/// Subscribes to application activation, hiding, launch, and termination.
/// `NSWorkspace` posts these on the main thread; each block only sets a flag
/// and, for launch and termination, wakes the AX thread to re-read the running
/// applications.
fn observe_workspace(
    invalidation: &Arc<EventInvalidation>,
    waker: &Arc<Waker>,
) -> Vec<Retained<ProtocolObject<dyn NSObjectProtocol>>> {
    autoreleasepool(|_| {
        let center = NSWorkspace::sharedWorkspace().notificationCenter();
        let observe = |name: &NSNotificationName,
                       block: RcBlock<dyn Fn(NonNull<NSNotification>)>| {
            // SAFETY: no object filter, no queue (the block runs on the posting thread), and the
            // block only touches `Send + Sync` atomics and the wake pipe.
            unsafe {
                center.addObserverForName_object_queue_usingBlock(Some(name), None, None, &block)
            }
        };
        let activated = Arc::clone(invalidation);
        let activate = RcBlock::new(move |_: NonNull<NSNotification>| {
            activated.frontmost_changed.store(true, Ordering::Release);
        });
        let mut observers = Vec::with_capacity(6);
        // SAFETY: AppKit's notification name constants are immutable statics.
        observers.push(observe(
            unsafe { NSWorkspaceDidActivateApplicationNotification },
            activate,
        ));
        // A Space slide shows both Spaces' windows at once; the worker holds
        // layout until it settles (#334, #329).
        let space_changed = RcBlock::new(|_: NonNull<NSNotification>| {
            super::spaces::note_active_space_change();
        });
        // SAFETY: Same as above.
        observers.push(observe(
            unsafe { NSWorkspaceActiveSpaceDidChangeNotification },
            space_changed,
        ));
        // ⌘H posts no window notification (unlike ⌘M's `AXWindowMiniaturized`), so without
        // these the siblings waited for the fallback refresh before re-filling the hole.
        // SAFETY: Same as above.
        let visibility = unsafe {
            [
                NSWorkspaceDidHideApplicationNotification,
                NSWorkspaceDidUnhideApplicationNotification,
            ]
        };
        for name in visibility {
            let invalidation = Arc::clone(invalidation);
            let block = RcBlock::new(move |_: NonNull<NSNotification>| {
                invalidation.app_visibility_changed();
            });
            observers.push(observe(name, block));
        }
        // SAFETY: Same as above.
        let lifecycle = unsafe {
            [
                NSWorkspaceDidLaunchApplicationNotification,
                NSWorkspaceDidTerminateApplicationNotification,
            ]
        };
        for name in lifecycle {
            let invalidation = Arc::clone(invalidation);
            let waker = Arc::clone(waker);
            let block = RcBlock::new(move |_: NonNull<NSNotification>| {
                invalidation.sweep_requested.store(true, Ordering::Release);
                waker.wake();
            });
            observers.push(observe(name, block));
        }
        observers
    })
}

fn run_event_loop(
    reader: Option<PipeReader>,
    invalidation: &Arc<EventInvalidation>,
    stopped: &AtomicBool,
) {
    let Some(run_loop) = CFRunLoop::current() else {
        return;
    };
    let mut wake = reader.and_then(|reader| EventWake::new(reader, &run_loop));
    let mut subscriptions = Subscriptions::<EventRegistration>::default();
    let mut register =
        |app: &AppInstanceId| EventRegistration::new(app.pid, invalidation, &run_loop);
    let release = |registration: EventRegistration| {
        // SAFETY: `kCFRunLoopDefaultMode` is a static Core Foundation mode constant.
        run_loop.remove_source(Some(&registration.source), unsafe { kCFRunLoopDefaultMode });
    };
    let mut next_sweep = Instant::now();
    while !stopped.load(Ordering::Acquire) {
        let keep_running = autoreleasepool(|_| {
            if let Some(wake) = &mut wake
                && !wake.drain_and_arm()
            {
                return false;
            }
            if stopped.load(Ordering::Acquire) {
                return false;
            }
            let now = Instant::now();
            if invalidation.sweep_requested.swap(false, Ordering::AcqRel) || now >= next_sweep {
                let applications = running_application_instances();
                if subscriptions.sync(applications, &mut register, release, stopped) {
                    invalidation.dirty.store(true, Ordering::Release);
                }
                next_sweep = Instant::now() + SAFETY_SWEEP_INTERVAL;
            }
            subscriptions.retry_due(Instant::now(), &mut register, stopped);
            let deadline = subscriptions
                .next_retry()
                .map_or(next_sweep, |retry| retry.min(next_sweep));
            let mut timeout = deadline
                .saturating_duration_since(Instant::now())
                .as_secs_f64();
            if wake.is_none() {
                timeout = timeout.min(FALLBACK_POLL_SECONDS);
            }
            // SAFETY: `kCFRunLoopDefaultMode` is a static Core Foundation mode constant.
            CFRunLoop::run_in_mode(unsafe { kCFRunLoopDefaultMode }, timeout, true);
            true
        });
        if !keep_running {
            break;
        }
    }
    autoreleasepool(|_| {
        if let Some(wake) = wake {
            // SAFETY: `kCFRunLoopDefaultMode` is a static Core Foundation mode constant.
            run_loop.remove_source(Some(&wake.source), unsafe { kCFRunLoopDefaultMode });
            wake.descriptor.invalidate();
        }
        subscriptions.release_all(release);
    });
}

/// When to try an application's observer again after it failed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Retry {
    at: Instant,
    delay: Duration,
}

impl Retry {
    fn after(previous: Option<Self>, now: Instant) -> Self {
        let delay = previous.map_or(REGISTRATION_RETRY_MIN, |retry| {
            retry.delay.saturating_mul(2).min(REGISTRATION_RETRY_MAX)
        });
        Self {
            at: now + delay,
            delay,
        }
    }
}

/// Per-application observers on the AX thread. An application whose observer
/// fails is retried with doubling backoff rather than on every sweep, and is
/// forgotten once it exits.
struct Subscriptions<R> {
    known: HashSet<AppInstanceId>,
    active: HashMap<AppInstanceId, R>,
    retries: HashMap<AppInstanceId, Retry>,
}

impl<R> Default for Subscriptions<R> {
    fn default() -> Self {
        Self {
            known: HashSet::new(),
            active: HashMap::new(),
            retries: HashMap::new(),
        }
    }
}

impl<R> Subscriptions<R> {
    /// Matches observers to `applications`. Returns whether the set changed.
    fn sync(
        &mut self,
        applications: Vec<AppInstanceId>,
        register: &mut impl FnMut(&AppInstanceId) -> Option<R>,
        mut release: impl FnMut(R),
        stopped: &AtomicBool,
    ) -> bool {
        let desired: HashSet<_> = applications.into_iter().collect();
        let changed = desired != self.known;
        let exited: Vec<_> = self
            .active
            .keys()
            .filter(|app| !desired.contains(*app))
            .cloned()
            .collect();
        for app in exited {
            if let Some(registration) = self.active.remove(&app) {
                release(registration);
            }
        }
        self.retries.retain(|app, _| desired.contains(app));
        let now = Instant::now();
        for app in &desired {
            if stopped.load(Ordering::Acquire) {
                break;
            }
            if !self.active.contains_key(app) && !self.retries.contains_key(app) {
                self.attempt(app, now, register);
            }
        }
        self.known = desired;
        changed
    }

    /// Tries again every failed application whose backoff has passed.
    fn retry_due(
        &mut self,
        now: Instant,
        register: &mut impl FnMut(&AppInstanceId) -> Option<R>,
        stopped: &AtomicBool,
    ) {
        let due: Vec<_> = self
            .retries
            .iter()
            .filter(|(_, retry)| retry.at <= now)
            .map(|(app, _)| app.clone())
            .collect();
        for app in due {
            if stopped.load(Ordering::Acquire) {
                break;
            }
            self.attempt(&app, now, register);
        }
    }

    fn attempt(
        &mut self,
        app: &AppInstanceId,
        now: Instant,
        register: &mut impl FnMut(&AppInstanceId) -> Option<R>,
    ) {
        if let Some(registration) = register(app) {
            self.retries.remove(app);
            self.active.insert(app.clone(), registration);
        } else {
            let previous = self.retries.get(app).copied();
            self.retries
                .insert(app.clone(), Retry::after(previous, now));
        }
    }

    fn next_retry(&self) -> Option<Instant> {
        self.retries.values().map(|retry| retry.at).min()
    }

    fn release_all(self, release: impl FnMut(R)) {
        self.active.into_values().for_each(release);
    }
}

struct EventWake {
    reader: PipeReader,
    descriptor: CFRetained<CFFileDescriptor>,
    source: CFRetained<CFRunLoopSource>,
}

impl EventWake {
    fn new(reader: PipeReader, run_loop: &CFRunLoop) -> Option<Self> {
        // SAFETY: `reader` stays open inside `Self` until after `descriptor.invalidate()` runs
        // (`close_on_invalidate` is `false`), `wake_pipe_callback` is a valid C callback, and
        // `context` is null.
        let descriptor = unsafe {
            CFFileDescriptor::new(
                None,
                reader.as_raw_fd(),
                false,
                Some(wake_pipe_callback),
                ptr::null(),
            )
        }?;
        let source = CFFileDescriptor::new_run_loop_source(None, Some(&descriptor), 0)?;
        // SAFETY: `kCFRunLoopDefaultMode` is a static Core Foundation mode constant.
        run_loop.add_source(Some(&source), unsafe { kCFRunLoopDefaultMode });
        Some(Self {
            reader,
            descriptor,
            source,
        })
    }

    fn drain_and_arm(&mut self) -> bool {
        let mut buf = [0_u8; 64];
        loop {
            match self.reader.read(&mut buf) {
                Ok(0) => return false,
                Ok(_) => continue,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }
        self.descriptor
            .enable_call_backs(kCFFileDescriptorReadCallBack);
        true
    }
}

unsafe extern "C-unwind" fn wake_pipe_callback(
    _descriptor: *mut CFFileDescriptor,
    _callback_types: CFOptionFlags,
    _info: *mut c_void,
) {
}

fn nonblocking_pipe() -> Option<(PipeReader, PipeWriter)> {
    let (reader, writer) = io::pipe().ok()?;
    if !set_nonblocking(reader.as_raw_fd()) || !set_nonblocking(writer.as_raw_fd()) {
        return None;
    }
    Some((reader, writer))
}

fn set_nonblocking(fd: RawFd) -> bool {
    // SAFETY: `fd` is an open pipe descriptor owned by the caller.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    // SAFETY: `flags` came from `F_GETFL` on the same valid pipe descriptor.
    flags >= 0 && unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } == 0
}

struct EventRegistration {
    _observer: CFRetained<AXObserver>,
    _application: CFRetained<AXUIElement>,
    source: CFRetained<CFRunLoopSource>,
}

impl EventRegistration {
    fn new(pid: i32, invalidation: &Arc<EventInvalidation>, run_loop: &CFRunLoop) -> Option<Self> {
        let application = application_element(pid);
        let mut raw_observer = ptr::null_mut();
        // SAFETY: `AXObserverCreate` writes a +1 retained observer into `raw_observer` on success.
        let result = unsafe {
            AXObserver::create(
                pid,
                Some(window_event_callback),
                NonNull::from(&mut raw_observer),
            )
        };
        if result != AXError::Success {
            return None;
        }
        // SAFETY: `raw_observer` is non-null and +1 retained after `AXError::Success`.
        let observer = unsafe { CFRetained::from_raw(NonNull::new(raw_observer)?) };
        let refcon = Arc::as_ptr(invalidation).cast_mut().cast::<c_void>();
        // SAFETY: `invalidation` outlives all observers because `WindowEventMonitor::drop` joins
        // the event thread and removes every run-loop source before dropping `self.invalidation`.
        let window_created = unsafe {
            observer.add_notification(
                &application,
                &CFString::from_static_str("AXWindowCreated"),
                refcon,
            )
        };
        if !matches!(
            window_created,
            AXError::Success | AXError::NotificationAlreadyRegistered
        ) {
            return None;
        }
        // SAFETY: Same lifetime invariant as `AXWindowCreated` above.
        let _: AXError = unsafe {
            observer.add_notification(
                &application,
                &CFString::from_static_str("AXFocusedWindowChanged"),
                refcon,
            )
        };
        subscribe_window_notifications(&observer, &application, refcon);
        // SAFETY: `observer` is a valid live `AXObserver`.
        let source = unsafe { observer.run_loop_source() };
        // SAFETY: `kCFRunLoopDefaultMode` is a static Core Foundation mode constant.
        run_loop.add_source(Some(&source), unsafe { kCFRunLoopDefaultMode });
        Some(Self {
            _observer: observer,
            _application: application,
            source,
        })
    }
}

unsafe extern "C-unwind" fn window_event_callback(
    observer: NonNull<AXObserver>,
    element: NonNull<AXUIElement>,
    notification: NonNull<CFString>,
    refcon: *mut c_void,
) {
    // SAFETY: Core Foundation guarantees `notification` is a valid `CFString` for the callback.
    let notification = unsafe { notification.as_ref() };
    let window_created = notification == &*CFString::from_static_str("AXWindowCreated");
    let title_changed = notification == &*CFString::from_static_str("AXTitleChanged");
    // SAFETY: `refcon` was created from `Arc::as_ptr(&invalidation)` and stays valid until the
    // event thread removes all sources before `WindowEventMonitor` drops the `Arc`.
    if let Some(invalidation) = unsafe { refcon.cast::<EventInvalidation>().as_ref() } {
        if window_created {
            invalidation.window_created.store(true, Ordering::Release);
        }
        if title_changed {
            // Spinners retitle windows many times a second; `take_dirty` coalesces these.
            invalidation.title_changed.store(true, Ordering::Release);
        } else {
            invalidation.dirty.store(true, Ordering::Release);
        }
    }
    if window_created {
        subscribe_one_window(
            // SAFETY: `observer` and `element` are non-null handles supplied by HIServices.
            unsafe { observer.as_ref() },
            // SAFETY: `element` is the newly created window `AXUIElement` supplied by HIServices.
            unsafe { element.as_ref() },
            refcon,
        );
    }
}

fn subscribe_window_notifications(
    observer: &AXObserver,
    application: &AXUIElement,
    refcon: *mut c_void,
) {
    let Ok(windows) = attribute::<CFArray>(application, "AXWindows") else {
        return;
    };
    for window in cf_array_items::<AXUIElement>(&windows) {
        subscribe_one_window(observer, window, refcon);
    }
}

fn subscribe_one_window(observer: &AXObserver, window: &AXUIElement, refcon: *mut c_void) {
    set_ax_timeout(window);
    for notification in [
        "AXUIElementDestroyed",
        "AXMoved",
        "AXResized",
        "AXWindowMiniaturized",
        "AXWindowDeminiaturized",
        "AXTitleChanged",
    ] {
        // SAFETY: `observer` and `window` are live AX objects and `refcon` outlives the observer.
        let _: AXError = unsafe {
            observer.add_notification(window, &CFString::from_static_str(notification), refcon)
        };
    }
}

pub(super) fn running_application_instances() -> Vec<AppInstanceId> {
    autoreleasepool(|_| {
        let self_pid = std::process::id() as i32;
        let applications = NSWorkspace::sharedWorkspace().runningApplications();
        let mut seen_pids = HashSet::new();
        let mut instances: Vec<_> = (0..applications.count())
            .filter_map(|index| {
                let application = applications.objectAtIndex(index);
                let pid = application.processIdentifier();
                (pid > 0
                    && pid != self_pid
                    && application.activationPolicy() != NSApplicationActivationPolicy::Prohibited
                    && !is_own_application(&application)
                    && seen_pids.insert(pid))
                .then(|| app_instance_with_pid(&application, pid))
            })
            .collect();
        if let Some(windows) = window_server_windows(CGWindowListOption::OptionAll) {
            for window in windows {
                if window.pid > 0
                    && window.pid != self_pid
                    && seen_pids.insert(window.pid)
                    && let Some(application) =
                        NSRunningApplication::runningApplicationWithProcessIdentifier(window.pid)
                    && !is_own_application(&application)
                {
                    instances.push(app_instance_with_pid(&application, window.pid));
                }
            }
        }
        instances
    })
}

#[cfg(test)]
mod schedule_tests {
    use super::*;

    fn app(pid: i32) -> AppInstanceId {
        AppInstanceId {
            pid,
            launched_at: 1,
        }
    }

    #[test]
    fn failed_observers_back_off_and_cap() {
        let now = Instant::now();
        let first = Retry::after(None, now);
        assert_eq!(first.delay, REGISTRATION_RETRY_MIN);
        let mut retry = first;
        for _ in 0..20 {
            retry = Retry::after(Some(retry), now);
        }
        assert_eq!(retry.delay, REGISTRATION_RETRY_MAX);
        assert_eq!(
            Retry::after(Some(first), now).delay,
            REGISTRATION_RETRY_MIN * 2
        );
    }

    #[test]
    fn sweeps_do_not_retry_failed_observers_before_their_backoff() {
        let stopped = AtomicBool::new(false);
        let mut subscriptions = Subscriptions::<i32>::default();
        let mut attempts = Vec::new();
        let mut register = |app: &AppInstanceId| {
            attempts.push(app.pid);
            (app.pid != 2).then_some(app.pid)
        };
        assert!(subscriptions.sync(vec![app(1), app(2)], &mut register, drop, &stopped));
        // A second sweep with the same applications neither re-registers the
        // live observer nor retries the failed one early.
        assert!(!subscriptions.sync(vec![app(1), app(2)], &mut register, drop, &stopped));
        subscriptions.retry_due(Instant::now(), &mut register, &stopped);
        let retry_at = subscriptions.next_retry().expect("pid 2 waits for a retry");
        subscriptions.retry_due(retry_at, &mut register, &stopped);
        attempts.sort_unstable();
        assert_eq!(attempts, [1, 2, 2]);
        assert!(subscriptions.next_retry().expect("still failing") > retry_at);
    }

    #[test]
    fn exited_applications_release_observers_and_forget_failures() {
        let stopped = AtomicBool::new(false);
        let mut subscriptions = Subscriptions::<i32>::default();
        let mut register = |app: &AppInstanceId| (app.pid == 1).then_some(app.pid);
        subscriptions.sync(vec![app(1), app(2)], &mut register, drop, &stopped);
        let mut released = Vec::new();
        assert!(subscriptions.sync(Vec::new(), &mut register, |r| released.push(r), &stopped));
        assert_eq!(released, [1]);
        assert_eq!(subscriptions.next_retry(), None);
    }

    #[test]
    fn title_spinners_coalesce_into_one_refresh_per_interval() {
        let invalidation = EventInvalidation::default();
        let last = Cell::new(None);
        let start = Instant::now();
        invalidation.dirty.store(true, Ordering::Release);
        assert!(invalidation.take_dirty(&last, start));
        // Ten spinner frames inside one interval refresh nothing yet...
        for frame in 1..10 {
            invalidation.title_changed.store(true, Ordering::Release);
            let at = start + TITLE_REFRESH_INTERVAL * frame / 10;
            assert!(!invalidation.take_dirty(&last, at));
        }
        // ...then once, and the pending change is consumed.
        let due = start + TITLE_REFRESH_INTERVAL;
        assert!(invalidation.take_dirty(&last, due));
        assert!(!invalidation.take_dirty(&last, due + TITLE_REFRESH_INTERVAL));
        // A window event refreshes at once and covers pending titles.
        invalidation.title_changed.store(true, Ordering::Release);
        invalidation.dirty.store(true, Ordering::Release);
        let soon = due + Duration::from_millis(1);
        assert!(invalidation.take_dirty(&last, soon));
        assert!(!invalidation.take_dirty(&last, soon + TITLE_REFRESH_INTERVAL));
    }

    #[test]
    fn hiding_an_app_refreshes_on_the_next_tick() {
        let invalidation = EventInvalidation::default();
        let last = Cell::new(None);
        let now = Instant::now();
        assert!(!invalidation.take_dirty(&last, now));
        invalidation.app_visibility_changed();
        // Not held back like a title change: the next tick refreshes.
        assert!(invalidation.take_dirty(&last, now + Duration::from_millis(1)));
        assert!(!invalidation.take_dirty(&last, now + Duration::from_millis(2)));
    }

    #[test]
    fn title_changes_refresh_at_most_every_interval() {
        let now = Instant::now();
        assert!(title_refresh_due(None, now));
        assert!(!title_refresh_due(
            Some(now),
            now + TITLE_REFRESH_INTERVAL / 2
        ));
        assert!(title_refresh_due(Some(now), now + TITLE_REFRESH_INTERVAL));
    }
}
