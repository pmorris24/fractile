//! Vsync-paced callbacks on the main thread through `CADisplayLink` (`NSScreen
//! displayLinkWithTarget:selector:`, macOS 14+), which follows the screen's refresh rate, 120 Hz
//! on ProMotion displays. A link starts paused and costs nothing until [`DisplayLink::start`]; stop
//! it as soon as the motion it drives settles.

use std::cell::RefCell;

use objc2::{
    DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, rc::Retained,
    runtime::AnyObject, sel,
};
use objc2_app_kit::NSScreen;
use objc2_foundation::{NSObject, NSObjectProtocol, NSRunLoop, NSRunLoopCommonModes};
use objc2_quartz_core::CADisplayLink;

use super::displays::screen_display_id;

/// One vsync: when the frame being shown started, and when the next one will be displayed (both
/// in host-time seconds, as `CACurrentMediaTime`). Sample animations at `target_timestamp`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DisplayFrame {
    pub timestamp: f64,
    pub target_timestamp: f64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DisplayLinkError {
    MainThreadRequired,
    ScreenUnavailable,
}

impl std::fmt::Display for DisplayLinkError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::MainThreadRequired => "a display link must be created on the main thread",
            Self::ScreenUnavailable => "no screen is available for a display link",
        })
    }
}

impl std::error::Error for DisplayLinkError {}

struct TickIvars {
    callback: RefCell<Box<dyn FnMut(DisplayFrame)>>,
}

define_class!(
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[name = "FractileDisplayLinkTarget"]
    #[ivars = TickIvars]
    struct TickTarget;

    unsafe impl NSObjectProtocol for TickTarget {}

    impl TickTarget {
        #[unsafe(method(tick:))]
        fn tick(&self, link: &CADisplayLink) {
            let frame = DisplayFrame {
                timestamp: link.timestamp(),
                target_timestamp: link.targetTimestamp(),
            };
            // A callback that re-enters its own run loop must not be called again mid-frame.
            if let Ok(mut callback) = self.ivars().callback.try_borrow_mut() {
                callback(frame);
            }
        }
    }
);

/// A main-thread vsync callback for one screen. Dropping it invalidates the link; don't drop it
/// from inside its own callback.
pub struct DisplayLink {
    link: Retained<CADisplayLink>,
    _target: Retained<TickTarget>,
}

impl DisplayLink {
    /// A paused link on the screen with Core Graphics ID `display`, or the main screen when
    /// `display` is `None` or not connected. Main thread only.
    pub fn new(
        display: Option<u32>,
        callback: impl FnMut(DisplayFrame) + 'static,
    ) -> Result<Self, DisplayLinkError> {
        let marker = MainThreadMarker::new().ok_or(DisplayLinkError::MainThreadRequired)?;
        let screen = screen(display, marker).ok_or(DisplayLinkError::ScreenUnavailable)?;
        let target = TickTarget::alloc(marker).set_ivars(TickIvars {
            callback: RefCell::new(Box::new(callback)),
        });
        // SAFETY: `NSObject`'s `-init` takes no arguments and returns an initialized `Self`.
        let target: Retained<TickTarget> = unsafe { msg_send![super(target), init] };
        let object: &AnyObject = &target;
        // SAFETY: `target` implements `tick:`, which takes the `CADisplayLink` that calls it.
        let link = unsafe { screen.displayLinkWithTarget_selector(object, sel!(tick:)) };
        link.setPaused(true);
        // SAFETY: We are on the main thread, so the main run loop is this thread's run loop, and
        // `NSRunLoopCommonModes` is a process-lifetime static string.
        unsafe { link.addToRunLoop_forMode(&NSRunLoop::mainRunLoop(), NSRunLoopCommonModes) };
        Ok(Self {
            link,
            _target: target,
        })
    }

    /// Resumes vsync callbacks.
    pub fn start(&self) {
        self.link.setPaused(false);
    }

    /// Pauses vsync callbacks; a paused link uses no CPU.
    pub fn stop(&self) {
        self.link.setPaused(true);
    }

    #[must_use]
    pub fn is_running(&self) -> bool {
        !self.link.isPaused()
    }
}

impl Drop for DisplayLink {
    fn drop(&mut self) {
        self.link.invalidate();
    }
}

fn screen(display: Option<u32>, marker: MainThreadMarker) -> Option<Retained<NSScreen>> {
    let screens = NSScreen::screens(marker);
    display
        .and_then(|display| {
            (0..screens.count())
                .map(|index| screens.objectAtIndex(index))
                .find(|screen| screen_display_id(screen) == Some(display))
        })
        .or_else(|| NSScreen::mainScreen(marker))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creating_a_link_off_the_main_thread_is_refused() {
        assert_eq!(
            DisplayLink::new(None, |_| {}).err(),
            Some(DisplayLinkError::MainThreadRequired)
        );
    }
}
