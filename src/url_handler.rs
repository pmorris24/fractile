//! Main-thread AppKit `application:openURLs:` handler attached to winit's
//! `NSApplicationDelegate`, complementing Rune's `kAEGetURL` handler.
//!
//! Incoming `fractile://` URLs are strictly validated by [`crate::deep_link::parse_url`]
//! before entering the action queue. Deep links only open Settings and highlight
//! a target; they never mutate configuration or run window-tiling commands.

use std::{
    cell::{Cell, RefCell},
    sync::mpsc::Sender,
};

use objc2::{
    MainThreadMarker, ffi, msg_send,
    rc::autoreleasepool,
    runtime::{AnyClass, AnyObject, Sel},
    sel,
};
use objc2_app_kit::NSApplication;
use objc2_foundation::{NSArray, NSString};

use crate::{app::Action, deep_link};

thread_local! {
    static SENDER: RefCell<Option<Sender<Action>>> = const { RefCell::new(None) };
    static HOOKED: Cell<*const AnyClass> = const { Cell::new(std::ptr::null()) };
}

pub(crate) fn install(actions: Sender<Action>) -> Result<(), String> {
    let marker = MainThreadMarker::new().ok_or("URL handler requires the main thread")?;
    let app = NSApplication::sharedApplication(marker);
    // SAFETY: `delegate` returns AppKit's unretained pointer to winit's installed delegate.
    let delegate: *mut AnyObject = unsafe { msg_send![&app, delegate] };
    // SAFETY: Checked for null; the event loop owns the delegate on the main thread.
    let delegate = unsafe { delegate.as_ref() }.ok_or("application delegate is unavailable")?;
    let class = delegate.class();
    if !HOOKED.with(|slot| std::ptr::eq(slot.get(), class)) {
        let selector = sel!(application:openURLs:);
        // SAFETY: `respondsToSelector:` queries the valid Objective-C object without side effects.
        let exists: bool = unsafe { msg_send![delegate, respondsToSelector: selector] };
        if exists {
            return Err("application delegate already implements application:openURLs:".into());
        }
        // SAFETY: Objective-C's `IMP` slot uses `unsafe extern "C-unwind" fn()` as an erased
        // function pointer; AppKit dispatches `application:openURLs:` with
        // `(id, SEL, NSApplication*, NSArray<NSURL*>*) -> void`.
        let implementation: unsafe extern "C-unwind" fn() = unsafe {
            std::mem::transmute::<
                unsafe extern "C-unwind" fn(
                    *mut AnyObject,
                    Sel,
                    *mut NSApplication,
                    *mut NSArray<AnyObject>,
                ),
                unsafe extern "C-unwind" fn(),
            >(application_open_urls)
        };
        // SAFETY: `class` is a valid Objective-C class, `selector` is not yet implemented on it,
        // and `"v@:@@"` matches `(void)(id, SEL, id, id)` on 64-bit macOS.
        let added = unsafe {
            ffi::class_addMethod(
                class as *const AnyClass as *mut AnyClass,
                selector,
                implementation,
                c"v@:@@".as_ptr(),
            )
        };
        if !added.as_bool() {
            return Err("could not install application URL handler".into());
        }
        HOOKED.with(|slot| slot.set(class));
        // Refresh AppKit's optional-selector cache, retaining the exact winit delegate.
        // SAFETY: Re-assigns the currently installed delegate on the main thread.
        unsafe {
            let _: () = msg_send![&app, setDelegate: delegate];
        }
    }
    SENDER.with(|slot| *slot.borrow_mut() = Some(actions));
    Ok(())
}

unsafe extern "C-unwind" fn application_open_urls(
    _delegate: *mut AnyObject,
    _selector: Sel,
    _application: *mut NSApplication,
    urls: *mut NSArray<AnyObject>,
) {
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        autoreleasepool(|_| {
            // SAFETY: AppKit passes a valid NSArray<NSURL*> or nil on the main thread.
            let Some(urls) = (unsafe { urls.as_ref() }) else {
                return;
            };
            let count = urls.count();
            for index in 0..count {
                let item = urls.objectAtIndex(index);
                // SAFETY: Each element is an NSURL; `absoluteString` returns an autoreleased
                // NSString* or nil, scoped to this autoreleasepool.
                let raw: *mut NSString = unsafe { msg_send![&item, absoluteString] };
                // SAFETY: Checked for null via `as_ref()`; valid for the duration of this
                // autoreleasepool on the main thread.
                let Some(raw) = (unsafe { raw.as_ref() }) else {
                    continue;
                };
                let link = raw.to_string();
                dispatch_url(&link);
            }
        });
    }));
    if caught.is_err() {
        drop(std::io::Write::write_all(
            &mut std::io::stderr().lock(),
            b"fractile: application:openURLs: handler panicked\n",
        ));
    }
}

fn dispatch_url(raw: &str) {
    match deep_link::parse_url(raw) {
        Ok(link) => {
            let target = link.resolve();
            SENDER.with(|slot| {
                if let Some(actions) = slot.borrow().as_ref() {
                    drop(actions.send(Action::OpenTarget(target)));
                }
            });
        }
        Err(error) => {
            let line = format!("fractile: ignored deep link ({error})\n");
            drop(std::io::Write::write_all(
                &mut std::io::stderr().lock(),
                line.as_bytes(),
            ));
        }
    }
}
