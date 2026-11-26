//! Hears the Try it step's shortcuts wherever the keyboard is.
//!
//! A listen-only key tap, up only while the Try it step is. It never stops a press. It matches
//! the step's chords by key code and modifiers and reads nothing else, and each one it
//! hears plays on the step's miniature desktop. While the setup window has the keyboard the
//! global hotkeys are held off (`host::set_shortcut_practice`), so the press moves the
//! miniature and no real window; with another window in front the real shortcut runs as usual
//! and the miniature follows along.

use fractile::macos::KeyMonitor;

use super::demo::{self, Move, Presses};
use crate::Ui;

pub struct Listener {
    _monitor: KeyMonitor,
    hears: [bool; Move::ALL.len()],
}

impl Listener {
    /// Whether the tap hears `mv`'s chord, so the window's own key events leave it alone.
    pub const fn hears(&self, mv: Move) -> bool {
        self.hears[mv.index()]
    }
}

/// Starts hearing `presses` for `window`, or `None` without Accessibility.
pub fn listen(window: rune::AppHandle<Ui>, presses: Presses) -> Option<Listener> {
    KeyMonitor::install(move |key_code, modifiers| {
        let Some(mv) = demo::heard(&presses, key_code, modifiers) else {
            return;
        };
        window.update_if(move |ui, tx| super::pressed(ui, mv, tx));
    })
    .ok()
    .map(|monitor| Listener {
        _monitor: monitor,
        hears: presses.map(demo::hearable),
    })
}
