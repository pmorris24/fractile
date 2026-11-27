//! Single-process macOS Rune host: runs Rune's AppKit event loop on the main
//! thread alongside the native status item, winit termination
//! gate, and a 25 ms `NSTimer` pump in `NSRunLoopCommonModes`.

#![allow(unsafe_code)]

use std::{
    cell::{Cell, RefCell},
    ptr::NonNull,
};

use block2::RcBlock;
use fractile::app::{HostCallbacks, MainThreadAgent, PreparedAgent, TICK_INTERVAL, TickOutcome};
use objc2::{
    MainThreadMarker,
    rc::{Retained, autoreleasepool},
};
use objc2_app_kit::NSApplication;
use objc2_foundation::{NSRunLoop, NSRunLoopCommonModes, NSTimer};

use crate::Ui;

struct Launch {
    prepared: PreparedAgent,
    window: rune::AppHandle<Ui>,
    setup_tour_open: bool,
}

struct PumpState {
    agent: MainThreadAgent,
    window: rune::AppHandle<Ui>,
    timer: Retained<NSTimer>,
}

thread_local! {
    static LAUNCH: RefCell<Option<Launch>> = const { RefCell::new(None) };
    static PUMP: RefCell<Option<PumpState>> = const { RefCell::new(None) };
    static STARTUP_ERROR: RefCell<Option<String>> = const { RefCell::new(None) };
    /// Settings › Shortcuts is recording a chord; the next tick holds the hotkeys off.
    static RECORDING_SHORTCUT: Cell<bool> = const { Cell::new(false) };
    /// The setup tour's Try it step is showing; each tick its window is key holds the hotkeys off.
    static PRACTICING_SHORTCUTS: Cell<bool> = const { Cell::new(false) };
    /// A `[shortcuts]` table Settings just saved, for the next tick to register.
    static SAVED_SHORTCUTS: RefCell<Option<fractile::shortcuts::Shortcuts>> =
        const { RefCell::new(None) };
}

/// Holds the global hotkeys off while Settings › Shortcuts records a chord, so pressing one
/// reaches the recorder instead of moving windows. The agent takes it on its next tick, outside
/// any Rune callback.
pub fn set_shortcut_recording(recording: bool) {
    RECORDING_SHORTCUT.with(|slot| slot.set(recording));
}

/// Holds the global hotkeys off while the setup tour's Try it step is showing and its window has
/// the keyboard, so the step's shortcuts drive its miniature desktop instead of the real
/// windows. With another window in front the shortcuts work as usual.
pub fn set_shortcut_practice(practicing: bool) {
    PRACTICING_SHORTCUTS.with(|slot| slot.set(practicing));
}

/// Whether the Settings window, which the setup tour draws in, is the key window.
fn setup_window_is_key() -> bool {
    MainThreadMarker::new().is_some_and(|mtm| {
        NSApplication::sharedApplication(mtm)
            .keyWindow()
            .is_some_and(|window| window.title().to_string() == "Fractile")
    })
}

/// Registers a `[shortcuts]` table Settings saved, without waiting for the config probe.
pub fn shortcuts_saved(shortcuts: fractile::shortcuts::Shortcuts) {
    SAVED_SHORTCUTS.with(|slot| *slot.borrow_mut() = Some(shortcuts));
}

pub fn bring_window_forward(window: &rune::AppHandle<Ui>) {
    window.set_dock_icon(true);
    if let Some(mtm) = MainThreadMarker::new() {
        NSApplication::sharedApplication(mtm).activate();
    }
    window.open();
}

/// Keeps the setup window above other apps' windows without activating it, so the Try it
/// step stays in view while its shortcuts bring real windows forward. Off puts it back.
pub fn float_setup_window(floating: bool) {
    use objc2_app_kit::{NSFloatingWindowLevel, NSNormalWindowLevel};
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let level = if floating {
        NSFloatingWindowLevel
    } else {
        NSNormalWindowLevel
    };
    for window in NSApplication::sharedApplication(mtm).windows().iter() {
        // The HUD's panels keep their own level; only the titled Settings window moves.
        let ours = window.title().to_string() == "Fractile";
        if ours && window.level() != level && (floating || window.level() == NSFloatingWindowLevel)
        {
            window.setLevel(level);
        }
    }
}

fn launch_links() -> Vec<fractile::deep_link::DeepLink> {
    std::env::args()
        .skip(1)
        .filter(|arg| arg.contains("://"))
        .filter_map(|arg| match fractile::deep_link::parse_url(&arg) {
            Ok(link) => Some(link),
            Err(error) => {
                let line = format!("fractile: ignored launch deep link ({error})\n");
                drop(std::io::Write::write_all(
                    &mut std::io::stderr().lock(),
                    line.as_bytes(),
                ));
                None
            }
        })
        .collect()
}

pub fn run() -> Result<(), String> {
    let links = launch_links();
    let (prepared, bridge) = match fractile::app::prepare() {
        Ok(ready) => ready,
        Err(error) => {
            // If another Fractile agent already owns control.sock, hand off any
            // launch deep links to the running instance and exit cleanly.
            if !links.is_empty() {
                let mut forwarded = false;
                for link in &links {
                    if fractile::control::send(&["open".into(), link.canonical_url()]).is_ok() {
                        forwarded = true;
                    }
                }
                if forwarded {
                    return Ok(());
                }
            }
            return Err(error);
        }
    };
    let initial_target = links.last().map(fractile::deep_link::DeepLink::resolve);
    let accessibility = fractile::macos::accessibility_trusted();
    let initial_tour = crate::onboarding::launch_step(accessibility);
    let open_at_launch = initial_tour.is_some() || !accessibility || initial_target.is_some();
    let mut app = crate::live_app(bridge, initial_tour, accessibility, initial_target)
        .args(Vec::new())
        .accessory();
    app = if open_at_launch {
        app.keep_running()
    } else {
        app.windowless()
    };
    let window = app.handle();
    // Show in Dock (on unless config.toml turns it off) keeps the live icon there from launch.
    let show_in_dock =
        fractile::config::Config::load().map_or(true, |config| config.show_in_dock());
    if open_at_launch || show_in_dock {
        window.set_dock_icon(true);
    }
    LAUNCH.with(|slot| {
        *slot.borrow_mut() = Some(Launch {
            prepared,
            window: window.clone(),
            setup_tour_open: initial_tour.is_some(),
        });
    });
    window.update_if(|_, _| {
        start_main_thread();
        false
    });
    let run_result = app.try_run();
    if let Some(state) = PUMP.with(|slot| slot.borrow_mut().take()) {
        state.timer.invalidate();
    }
    if let Some(error) = STARTUP_ERROR.with(|slot| slot.borrow_mut().take()) {
        return Err(error);
    }
    run_result
}

fn start_main_thread() {
    let Some(Launch {
        prepared,
        window,
        setup_tour_open,
    }) = LAUNCH.with(|slot| slot.borrow_mut().take())
    else {
        return;
    };
    let settings_window = window.clone();
    let target_window = window.clone();
    let setup_window = window.clone();
    let callbacks = HostCallbacks {
        open_settings: Box::new(move || {
            bring_window_forward(&settings_window);
            Ok(())
        }),
        open_target: Box::new(move |target| {
            target_window.update_if(move |ui, _| {
                ui.open_target(target);
                true
            });
            bring_window_forward(&target_window);
            Ok(())
        }),
        open_setup: Box::new(move || {
            setup_window.update_if(|ui, _| {
                ui.start_tour();
                true
            });
            bring_window_forward(&setup_window);
            Ok(())
        }),
        save_hud: Box::new(|enabled| {
            let mut doc = crate::rules::Doc::load();
            doc.set_hud(enabled);
            doc.save()
        }),
    };
    let agent = match prepared.start_main_thread(callbacks, setup_tour_open) {
        Ok(agent) => agent,
        Err(error) => {
            STARTUP_ERROR.with(|slot| *slot.borrow_mut() = Some(error));
            fractile::app::allow_immediate_termination();
            window.quit();
            return;
        }
    };
    let block = RcBlock::new(|_: NonNull<NSTimer>| {
        autoreleasepool(|_| tick_pump());
    });
    // SAFETY: Scheduled on the main thread's run loop; the block touches only main-thread state.
    let timer = unsafe {
        NSTimer::scheduledTimerWithTimeInterval_repeats_block(
            TICK_INTERVAL.as_secs_f64(),
            true,
            &block,
        )
    };
    // Keep pumping while the status-bar menu tracks pointer input.
    // SAFETY: `mainRunLoop` and `NSRunLoopCommonModes` are valid on the main thread.
    unsafe {
        NSRunLoop::mainRunLoop().addTimer_forMode(&timer, NSRunLoopCommonModes);
    }
    PUMP.with(|slot| {
        *slot.borrow_mut() = Some(PumpState {
            agent,
            window,
            timer,
        });
    });
}

fn tick_pump() {
    let recording = RECORDING_SHORTCUT.with(Cell::get);
    let practicing = PRACTICING_SHORTCUTS.with(Cell::get) && setup_window_is_key();
    let saved = SAVED_SHORTCUTS.with(|slot| slot.borrow_mut().take());
    let outcome = PUMP.with(|slot| {
        slot.borrow_mut().as_mut().map(|state| {
            // A table saved by the recorder registers before the recording's hold ends, so
            // the hotkeys come back once, with the new chord.
            if let Some(shortcuts) = saved {
                state.agent.apply_shortcuts(shortcuts);
            }
            state.agent.hold_hotkeys(recording, practicing);
            state.agent.tick()
        })
    });
    match outcome {
        Some(TickOutcome::Continue) | None => {}
        Some(TickOutcome::Quit) => {
            if let Some(state) = PUMP.with(|slot| slot.borrow_mut().take()) {
                state.timer.invalidate();
                state.window.quit();
            }
        }
        Some(TickOutcome::TerminateAppKit) => {
            if let Some(state) = PUMP.with(|slot| slot.borrow_mut().take()) {
                state.timer.invalidate();
                drop(state);
            }
            fractile::app::finish_appkit_termination();
        }
    }
}
