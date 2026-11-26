//! Interactive HUD actions, the per-app context menu (Float or Tile, Pin on all workspaces,
//! Always open on a workspace), and the macOS secondary-click bridge for non-activating overlay
//! panels.

use fractile::WorkspaceId;
use rune::{PopupItem, PopupMenu};

use crate::rules::Place;

pub const ID_TOGGLE_FLOAT: &str = "toggle-float";
/// The `pin` action: floating, and shown on every workspace.
pub const ID_TOGGLE_PIN: &str = "toggle-pin";
/// A `route` rule to the workspace that was right-clicked.
pub const ID_TOGGLE_ROUTE: &str = "toggle-route";
/// The checked `route` rule to another workspace; picking it removes the rule.
pub const ID_CLEAR_ROUTE: &str = "clear-route";

/// Side effects triggered by an interactive HUD overlay: switching workspaces and reading or
/// editing per-app `config.toml` rules.
pub trait HudSink: Send + Sync + 'static {
    fn switch_workspace(&self, workspace: WorkspaceId);
    fn app_place(&self, bundle_id: &str) -> Option<Place>;
    fn set_app_place(&self, bundle_id: &str, place: Option<Place>) -> Result<(), String>;
}

/// Inert sink used by default in unit and motion tests that do not attach a custom recorder.
#[cfg(test)]
pub struct NoopHudSink;

#[cfg(test)]
impl HudSink for NoopHudSink {
    fn switch_workspace(&self, _workspace: WorkspaceId) {}

    fn app_place(&self, _bundle_id: &str) -> Option<Place> {
        None
    }

    fn set_app_place(&self, _bundle_id: &str, _place: Option<Place>) -> Result<(), String> {
        Ok(())
    }
}

/// Live sink wired into the running `fractile-agent`: routes workspace switches and Settings
/// opens through `UiBridge` and edits per-app rules through `rules::Doc`.
#[cfg(target_os = "macos")]
pub struct LiveHudSink {
    bridge: fractile::app::UiBridge,
    window: rune::AppHandle<crate::Ui>,
}

#[cfg(target_os = "macos")]
impl LiveHudSink {
    #[must_use]
    pub fn new(bridge: fractile::app::UiBridge, window: rune::AppHandle<crate::Ui>) -> Self {
        Self { bridge, window }
    }
}

#[cfg(target_os = "macos")]
impl HudSink for LiveHudSink {
    fn switch_workspace(&self, workspace: WorkspaceId) {
        self.bridge
            .send(fractile::app::UiAction::SwitchWorkspace(workspace));
    }

    fn app_place(&self, bundle_id: &str) -> Option<Place> {
        crate::rules::Doc::load().app_place(bundle_id)
    }

    fn set_app_place(&self, bundle_id: &str, place: Option<Place>) -> Result<(), String> {
        let mut doc = crate::rules::Doc::load();
        doc.set_app_place(bundle_id, place);
        doc.save()?;
        self.bridge.send(fractile::app::UiAction::Retile);
        self.window.update_if(|ui, _| {
            if ui.drag.is_some() {
                return false;
            }
            ui.doc = crate::rules::Doc::load();
            ui.look_up_apps();
            true
        });
        Ok(())
    }
}

/// Whether `bundle_id`'s windows float: by its own rule, else by Fractile's built-in default. A
/// `route` rule only says where windows open, so a built-in float still floats there.
fn floats(current: Option<Place>, floats_by_default: bool) -> bool {
    match current {
        Some(Place::Float | Place::Pin) => true,
        Some(Place::Tile | Place::Ignore) => false,
        Some(Place::Workspace(_)) | None => floats_by_default,
    }
}

/// Builds the native context menu for right-clicking an app icon inside a HUD slot. Two groups,
/// each editing the app's one rule in config.toml: how its windows sit (tiled, floating, or
/// pinned on every workspace), then where its new windows open. "Pin" only ever means the
/// `pin` action; a `route` rule reads "Always open … on Workspace N".
#[must_use]
pub fn build_app_menu(
    app_name: &str,
    workspace: WorkspaceId,
    current: Option<Place>,
    floats_by_default: bool,
) -> PopupMenu {
    let float_item = if floats(current, floats_by_default) {
        PopupItem::new(ID_TOGGLE_FLOAT, format!("Tile {app_name}")).symbol("rectangle.split.2x1")
    } else {
        PopupItem::new(ID_TOGGLE_FLOAT, format!("Float {app_name}"))
            .symbol("rectangle.on.rectangle")
    };
    let pin_item = if current == Some(Place::Pin) {
        PopupItem::new(ID_TOGGLE_PIN, format!("Unpin {app_name}")).symbol("pin.slash")
    } else {
        PopupItem::new(ID_TOGGLE_PIN, format!("Pin {app_name} on all workspaces")).symbol("pin")
    };
    let route_item = |id: &str, home: WorkspaceId| {
        PopupItem::new(id, format!("Always open {app_name} on Workspace {home}"))
            .symbol("arrow.down.right.square")
            .checked(current == Some(Place::Workspace(home)))
    };
    let mut menu = PopupMenu::new()
        .item(float_item)
        .item(pin_item)
        .separator()
        .item(route_item(ID_TOGGLE_ROUTE, workspace));
    // A home on another workspace stays in the menu, checked: picking it removes the rule.
    if let Some(Place::Workspace(other)) = current
        && other != workspace
    {
        menu = menu.item(route_item(ID_CLEAR_ROUTE, other));
    }
    menu
}

/// Resolves a picked menu item ID into the new `Place` to write with `Doc::set_app_place`
/// (`Some(None)` removes the app's rule). Returns `None` if the pick ID is unrecognized.
#[must_use]
pub fn resolve_app_menu_pick(
    picked: &str,
    workspace: WorkspaceId,
    current: Option<Place>,
    floats_by_default: bool,
) -> Option<Option<Place>> {
    // Floating, the way this app floats with the fewest rules: a built-in float needs none.
    let floating = (!floats_by_default).then_some(Place::Float);
    match picked {
        ID_TOGGLE_FLOAT if floats(current, floats_by_default) => {
            // Tiling a built-in float takes a `tile` rule; any other app tiles with no rule.
            Some(floats_by_default.then_some(Place::Tile))
        }
        ID_TOGGLE_FLOAT => Some(floating),
        // Unpinning leaves the windows floating on the workspace they are on.
        ID_TOGGLE_PIN if current == Some(Place::Pin) => Some(floating),
        ID_TOGGLE_PIN => Some(Some(Place::Pin)),
        ID_TOGGLE_ROUTE if current == Some(Place::Workspace(workspace)) => Some(None),
        ID_TOGGLE_ROUTE => Some(Some(Place::Workspace(workspace))),
        ID_CLEAR_ROUTE => Some(None),
        _ => None,
    }
}

/// Ensures right-clicks (and Control+clicks) on any HUD overlay panel reach `HudOverlay`.
///
/// Rune's macOS overlay host forwards left clicks through winit but drops `MouseButton::Right`.
/// Installing a process-local AppKit monitor scoped strictly to our HUD panels' window numbers
/// keeps normal windows (like Settings) untouched while opening the native `NSMenu` right at the
/// pointer without activating Fractile.
#[cfg(target_os = "macos")]
pub(super) fn install_secondary_click_monitor(
    handles: Vec<rune::OverlayHandle<super::HudOverlay>>,
) {
    macos::install(handles);
}

#[cfg(not(target_os = "macos"))]
pub(super) fn install_secondary_click_monitor(
    _handles: Vec<rune::OverlayHandle<super::HudOverlay>>,
) {
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod macos {
    use std::{cell::RefCell, ptr::NonNull};

    use block2::RcBlock;
    use objc2::{MainThreadMarker, rc::Retained, runtime::AnyObject};
    use objc2_app_kit::{NSEvent, NSEventMask, NSEventModifierFlags, NSEventType};
    use rune::OverlayHandle;

    use super::super::{HudOverlay, OVERLAY_SIZE};

    struct Monitor {
        token: Retained<AnyObject>,
    }

    impl Drop for Monitor {
        fn drop(&mut self) {
            // SAFETY: `token` was returned by `addLocalMonitorForEventsMatchingMask:handler:`
            // on the main thread and is removed once on drop.
            unsafe {
                NSEvent::removeMonitor(&self.token);
            }
        }
    }

    thread_local! {
        static MONITOR: RefCell<Option<Monitor>> = const { RefCell::new(None) };
    }

    pub(super) fn install(handles: Vec<OverlayHandle<HudOverlay>>) {
        if MainThreadMarker::new().is_none() {
            return;
        }
        MONITOR.with(|slot| {
            let mut slot = slot.borrow_mut();
            let block = RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
                // SAFETY: AppKit invokes the local monitor block on the main thread with a valid
                // non-null `NSEvent` reference for the duration of the callback.
                let ev = unsafe { event.as_ref() };
                let kind = ev.r#type();
                let is_secondary = kind == NSEventType::RightMouseDown
                    || (kind == NSEventType::LeftMouseDown
                        && ev.modifierFlags().contains(NSEventModifierFlags::Control));
                if !is_secondary {
                    return event.as_ptr();
                }
                let Ok(window_number) = u32::try_from(ev.windowNumber()) else {
                    return event.as_ptr();
                };
                let Some(handle) = handles
                    .iter()
                    .find(|handle| handle.window_number() == Some(window_number))
                else {
                    return event.as_ptr();
                };
                let loc = ev.locationInWindow();
                let height = MainThreadMarker::new()
                    .and_then(|mtm| ev.window(mtm))
                    .map_or(f64::from(OVERLAY_SIZE.1), |window| {
                        window.frame().size.height
                    });
                let x = loc.x as f32;
                let y = (height - loc.y) as f32;
                handle.update(move |overlay, tx| {
                    overlay.secondary_click_at(x, y, tx);
                });
                std::ptr::null_mut()
            });
            let mask = NSEventMask::RightMouseDown | NSEventMask::LeftMouseDown;
            let token = {
                // SAFETY: Main thread verified above; `block` is retained by AppKit until removed
                // in `Monitor::drop`.
                unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(mask, &block) }
            };
            if let Some(token) = token {
                *slot = Some(Monitor { token });
            }
        });
    }
}
