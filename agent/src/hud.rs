//! Non-activating Workspace HUD overlay: a capsule near the bottom (or top) of each display
//! showing its row of workspaces, with a marker on the one you are on, when you switch
//! workspaces, move a window or change a layout. Never includes window titles or steals focus.
//!
//! Each display gets its own panel from a fixed pool, at a fixed size, only ordered in and out.
//! The capsule's entrance, exit and content changes are drawn inside it (`pill.rs`), so every
//! one of them can turn around mid-flight.

mod flow;
mod grow;

mod icons;
mod menu;
#[cfg(test)]
mod motion_tests;
mod pill;
mod stage;
#[cfg(test)]
mod tests;

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use fractile::{
    WorkspaceId,
    app::{UiHudDisplay, UiHudEvent, UiHudToast},
    config::{HudPosition, HudStyle},
};
use rune::{
    Align, Cx, DisplayTarget, El, KeyPolicy, NativeDisplayId, OverlayHandle, OverlayInput,
    OverlayOptions, OverlayPlacement, OverlaySpec, ScreenEdge, Tx, row,
};

use flow::FRESH_MS;
#[cfg(test)]
use flow::{HOLD_MS, RESUME_HOLD_MS, TOAST_HOLD_MS};
pub use flow::{HudFlow, HudTick, HudTransition, LEAVE_MS};
pub use icons::{MARKER_INSET, MARKER_RADIUS};
pub use menu::HudSink;
#[cfg(target_os = "macos")]
pub use menu::LiveHudSink;
#[cfg(test)]
use menu::NoopHudSink;
pub use pill::{Icons, PILL_HEIGHT};
pub(crate) use stage::{HudPreview, SCENES, miniature, stage, triggers};

use crate::{apps, controls::Paint};

/// While a panel is up, how often it checks its display is still connected.
const DISPLAY_CHECK_MS: u64 = 250;
/// Only the first show fades the panel itself over 100 ms: nothing has been drawn to enter from yet.
const FIRST_FADE: Duration = Duration::from_millis(100);
/// Subsequent reveals use a brief fade (>= 33 ms) instead of `show()` so AppKit marks the
/// panel visible before Rune's occlusion check runs, avoiding its 250 ms retry
/// (anomalyco/rune#1714) while letting the capsule's own entry spring carry the motion.
const REVEAL_FADE: Duration = Duration::from_millis(33);
/// Room on every side of the capsule for its shadow, which the panel's edge would cut, and
/// for ten or so workspaces before the row scrolls.
const OVERLAY_SIZE: (f32, f32) = (860.0, pill::PILL_HEIGHT + 2.0 * pill::SHADOW_REACH);
// The engine's reserved strip is sized for this capsule.
const _: () = assert!(pill::PILL_HEIGHT == fractile::config::HUD_PILL_HEIGHT as f32);
/// Keeps the capsule 60 pt inside the display edge it sits on, above the bottom or below the top.
pub const OVERLAY_MARGIN: f32 = pill::CAPSULE_INSET - pill::SHADOW_REACH;

/// The edge of each display the capsule sits on.
#[must_use]
pub const fn panel_edge(position: HudPosition) -> ScreenEdge {
    match position {
        // A side's panel is placed by its rect (`HudOverlay::place`); a toast in it shows at
        // the bottom.
        HudPosition::Bottom | HudPosition::Left | HudPosition::Right => ScreenEdge::Bottom,
        HudPosition::Top => ScreenEdge::Top,
    }
}

/// A side's panel: the capsule standing on end with room round it for its shadow, as tall as
/// the row's panel is wide, or the area windows tile in when that is shorter.
const SIDE_SIZE: (f32, f32) = (OVERLAY_SIZE.1, OVERLAY_SIZE.0);
/// How far a side's capsule sits from the area windows tile in when it is not kept on screen.
const SIDE_GAP: f32 = 12.0;
/// One panel per display, up to this many displays.
const PANEL_NAMES: [&str; 4] = [
    "Fractile HUD",
    "Fractile HUD 2",
    "Fractile HUD 3",
    "Fractile HUD 4",
];

pub(super) struct HudOverlay {
    flow: HudFlow,
    style: HudStyle,
    /// Which edge of its display the capsule sits on.
    position: HudPosition,
    /// `settings.hud_stay`: the capsule stays up, next to the strip windows leave it.
    stay: bool,
    icons: Icons,
    names: HashMap<String, String>,
    sink: Arc<dyn HudSink>,
    /// Reduce Motion as of the last event; the panel never takes focus, so Rune doesn't
    /// re-read it for us.
    reduce_motion: bool,
    /// The panel has been shown once, so its view has a drawn state to animate from.
    primed: bool,
    /// The display the panel is on, to hide it if that display goes away.
    display: Option<u32>,
    handle: Option<OverlayHandle<HudOverlay>>,
    hovered_workspace: Option<WorkspaceId>,
    hovered_app: Option<(WorkspaceId, String)>,
    #[cfg(test)]
    virtual_clock: Option<Arc<std::sync::atomic::AtomicU64>>,
}

impl HudOverlay {
    fn new(enabled: bool, style: HudStyle, position: HudPosition, sink: Arc<dyn HudSink>) -> Self {
        Self {
            flow: HudFlow::new(enabled),
            style,
            position,
            stay: false,
            icons: Icons::new(),
            names: HashMap::new(),
            sink,
            reduce_motion: false,
            primed: false,
            display: None,
            handle: None,
            hovered_workspace: None,
            hovered_app: None,
            #[cfg(test)]
            virtual_clock: None,
        }
    }

    fn now_ms(&self) -> u64 {
        #[cfg(test)]
        if let Some(clock) = &self.virtual_clock {
            return clock.load(std::sync::atomic::Ordering::Relaxed);
        }
        fractile::journal::unix_millis()
    }

    fn schedule_wake(&self, now_ms: u64, tx: &mut Tx) {
        if let Some(remaining_ms) = self.flow.remaining_ms(now_ms) {
            let check = if self.display.is_some() {
                DISPLAY_CHECK_MS
            } else {
                u64::MAX
            };
            tx.wake_after(Duration::from_millis(remaining_ms.min(check)));
        } else if self.flow.is_up() && self.display.is_some() {
            tx.wake_after(Duration::from_millis(DISPLAY_CHECK_MS));
        }
    }

    /// Puts the panel on its edge: `OVERLAY_MARGIN` in, or, kept on screen, with the capsule
    /// `HUD_STAY_GAP` from the area windows tile in (above the Dock, below the menu bar), in
    /// the strip the engine keeps clear of them.
    fn place(&self) {
        let Some(handle) = &self.handle else {
            return;
        };
        let gap = fractile::config::HUD_STAY_GAP as f32;
        if self.position.is_side()
            && let Some((screen, frame)) = display_rects(self.display)
        {
            // Points from the display's top-left: beside the area windows tile in, past a Dock
            // on that side, and centred down it.
            let gap = if self.stay { gap } else { SIDE_GAP };
            let height = SIDE_SIZE.1.min(frame.height as f32);
            let y = (frame.y - screen.y) as f32 + (frame.height as f32 - height) / 2.0;
            let x = if self.position == HudPosition::Left {
                (frame.x - screen.x) as f32 + gap - pill::SHADOW_REACH
            } else {
                (frame.x + frame.width - screen.x) as f32
                    - gap
                    - pill::PILL_HEIGHT
                    - pill::SHADOW_REACH
            };
            handle.resize((SIDE_SIZE.0, height));
            handle.set_placement(Some(OverlayPlacement::Rect {
                x,
                y,
                width: SIDE_SIZE.0,
                height,
            }));
            return;
        }
        let margin = if self.stay && !self.position.is_side() {
            let bar = self
                .display
                .map_or(0.0, |display| bar_inset(display, self.position));
            bar + gap - pill::SHADOW_REACH
        } else {
            OVERLAY_MARGIN
        };
        handle.set_placement(None);
        handle.resize(OVERLAY_SIZE);
        handle.place(panel_edge(self.position), margin);
    }

    pub(super) fn set_hovered(&mut self, inside: bool, tx: &mut Tx) {
        let now_ms = self.now_ms();
        self.flow.set_hovered(inside, now_ms);
        if !inside && !self.flow.is_held() {
            self.hovered_app = None;
            self.hovered_workspace = None;
        }
        self.schedule_wake(now_ms, tx);
    }

    pub(super) fn set_hovered_workspace(&mut self, workspace: WorkspaceId, inside: bool) {
        if inside {
            self.hovered_workspace = Some(workspace);
        } else if self.hovered_workspace == Some(workspace) {
            self.hovered_workspace = None;
        }
    }

    pub(super) fn set_hovered_app(&mut self, workspace: WorkspaceId, bundle: &str, inside: bool) {
        if inside {
            self.hovered_app = Some((workspace, bundle.to_owned()));
        } else if self
            .hovered_app
            .as_ref()
            .is_some_and(|(ws, id)| *ws == workspace && id == bundle)
        {
            self.hovered_app = None;
        }
    }

    pub(super) fn switch_workspace(&mut self, workspace: WorkspaceId, tx: &mut Tx) {
        self.sink.switch_workspace(workspace);
        self.schedule_wake(self.now_ms(), tx);
    }

    pub(super) fn open_app_menu(
        &mut self,
        workspace: WorkspaceId,
        bundle: &str,
        at: (f32, f32),
        tx: &mut Tx,
    ) {
        let now_ms = self.now_ms();
        let app_name = self
            .names
            .get(bundle)
            .cloned()
            .unwrap_or_else(|| apps::fallback_name(bundle));
        let current = self.sink.app_place(bundle);
        let floats_by_default = fractile::defaults::floats_by_default(bundle);
        let popup = menu::build_app_menu(&app_name, workspace, current, floats_by_default);
        self.flow.set_menu_open(true, now_ms);
        let bundle_id = bundle.to_owned();
        tx.popup_menu(popup, at, move |overlay: &mut HudOverlay, picked, tx| {
            let close_ms = overlay.now_ms();
            if let Some(picked) = picked
                && let Some(new_place) =
                    menu::resolve_app_menu_pick(picked, workspace, current, floats_by_default)
            {
                drop(overlay.sink.set_app_place(&bundle_id, new_place));
            }
            overlay.flow.set_menu_open(false, close_ms);
            overlay.schedule_wake(close_ms, tx);
        });
    }

    pub(super) fn secondary_click_at(&mut self, x: f32, y: f32, tx: &mut Tx) {
        if !self.flow.is_up() {
            return;
        }
        let target = self.hovered_app.clone().or_else(|| {
            let workspace = self.hovered_workspace?;
            let row = self.flow.current()?;
            let entry = row
                .workspaces
                .iter()
                .find(|entry| entry.workspace == workspace)?;
            let apps = icons::apps(entry);
            (apps.len() == 1).then(|| (workspace, apps[0].to_owned()))
        });
        if let Some((workspace, bundle)) = target {
            self.open_app_menu(workspace, &bundle, (x, y), tx);
        }
    }
}

struct Panel {
    handle: OverlayHandle<HudOverlay>,
    /// The display the panel is assigned to, by its `UiHudDisplay::id`.
    display: Option<String>,
}

struct Panels {
    panels: Vec<Panel>,
    apps: apps::Catalog,
}

#[derive(Clone)]
pub struct HudDriver {
    panels: Arc<Mutex<Panels>>,
}

pub fn install<S: 'static>(
    app: &mut rune::App<S>,
    enabled: bool,
    style: HudStyle,
    position: HudPosition,
    sink: Arc<dyn HudSink>,
) -> HudDriver {
    let panels: Vec<Panel> = PANEL_NAMES
        .iter()
        .map(|&name| {
            let spec = OverlaySpec {
                name,
                size: OVERLAY_SIZE,
                edge: panel_edge(position),
                margin: OVERLAY_MARGIN,
                align: Align::Center,
                inset: 0.0,
                interactive: false,
                view: overlay_view,
            }
            .options(OverlayOptions {
                input: OverlayInput::Regions,
                keys: KeyPolicy::None,
                prewarm: false,
                ..Default::default()
            })
            // Showing must not settle the view: the capsule enters from the pose it left in.
            .animate_reveal()
            .poll(overlay_poll);
            let handle = app.overlay(
                spec,
                HudOverlay::new(enabled, style, position, Arc::clone(&sink)),
            );
            let inner = handle.clone();
            handle.update(move |overlay, _| {
                overlay.handle = Some(inner);
            });
            Panel {
                handle,
                display: None,
            }
        })
        .collect();
    menu::install_secondary_click_monitor(
        panels.iter().map(|panel| panel.handle.clone()).collect(),
    );
    HudDriver {
        panels: Arc::new(Mutex::new(Panels {
            panels,
            apps: apps::Catalog::default(),
        })),
    }
}

impl HudDriver {
    /// Call on the main thread, where displays, app icons and Reduce Motion are read.
    pub fn trigger(&self, event: UiHudEvent) {
        let reduce_motion = reduce_motion();
        let connected = display_ids();
        let Ok(mut panels) = self.panels.lock() else {
            return;
        };
        let (icons, names) = panels.metadata_for(&event);
        let mut rows: Vec<(UiHudDisplay, DisplayTarget, Option<u32>)> = event
            .displays
            .iter()
            .filter_map(|row| {
                let id = *connected.get(&row.id)?;
                Some((row.clone(), native(id), Some(id)))
            })
            .take(PANEL_NAMES.len())
            .collect();
        if rows.is_empty()
            && let Some(row) = event.display_of(event.workspace)
        {
            // Displays couldn't be matched up: the acting display's row on the main one.
            rows.push((row.clone(), DisplayTarget::Primary, None));
        }
        panels.assign(&rows);
        for (row, target, display) in rows {
            let Some(panel) = panels
                .panels
                .iter()
                .find(|panel| panel.display.as_deref() == Some(row.id.as_str()))
            else {
                continue;
            };
            let icons = icons.clone();
            let names = names.clone();
            panel.handle.update_if(move |overlay, tx| {
                let moved = overlay.display != display || display.is_none();
                if moved {
                    if let Some(handle) = &overlay.handle {
                        handle.set_display(target);
                    }
                    overlay.display = display;
                }
                // A side's panel may have shown a toast along the bottom since: place it again.
                if moved || overlay.position.is_side() {
                    overlay.place();
                }
                let now_ms = overlay.now_ms();
                let handle = overlay.handle.clone();
                match overlay.flow.trigger(row, now_ms) {
                    HudTransition::Ignored => return false,
                    HudTransition::Show if overlay.primed => {
                        if let Some(handle) = handle {
                            handle.fade_in(REVEAL_FADE);
                        }
                    }
                    HudTransition::Show => {
                        overlay.primed = true;
                        if let Some(handle) = handle {
                            handle.fade_in(FIRST_FADE);
                        }
                    }
                    HudTransition::Update => {}
                }
                overlay.icons.extend(icons);
                overlay.names.extend(names);
                overlay.reduce_motion = reduce_motion;
                overlay.schedule_wake(now_ms, tx);
                true
            });
        }
    }

    /// Shows a brief non-activating status toast (~2.5 s) on the primary display, even when
    /// the Workspace HUD is turned off.
    pub fn trigger_toast(&self, toast: UiHudToast) {
        let reduce_motion = reduce_motion();
        let Ok(mut panels) = self.panels.lock() else {
            return;
        };
        for panel in panels.panels.iter_mut().skip(1) {
            panel.display = None;
            panel.handle.update_if(|overlay, _| {
                overlay.flow.dismiss();
                if let Some(handle) = &overlay.handle {
                    handle.hide();
                }
                false
            });
        }
        let Some(panel) = panels.panels.first_mut() else {
            return;
        };
        panel.display = None;
        panel.handle.update_if(move |overlay, tx| {
            if let Some(handle) = &overlay.handle {
                handle.set_display(DisplayTarget::Primary);
                if overlay.position.is_side() {
                    // A toast is a line of text: along the bottom, not down a side.
                    handle.set_placement(None);
                    handle.resize(OVERLAY_SIZE);
                    handle.place(ScreenEdge::Bottom, OVERLAY_MARGIN);
                }
            }
            overlay.display = None;
            let now_ms = overlay.now_ms();
            let handle = overlay.handle.clone();
            match overlay.flow.trigger_toast(toast, now_ms) {
                HudTransition::Ignored => return false,
                HudTransition::Show if overlay.primed => {
                    if let Some(handle) = handle {
                        handle.fade_in(REVEAL_FADE);
                    }
                }
                HudTransition::Show => {
                    overlay.primed = true;
                    if let Some(handle) = handle {
                        handle.fade_in(FIRST_FADE);
                    }
                }
                HudTransition::Update => {}
            }
            overlay.reduce_motion = reduce_motion;
            overlay.schedule_wake(now_ms, tx);
            true
        });
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.each(move |overlay, tx| {
            let left = overlay
                .flow
                .set_enabled(enabled, fractile::journal::unix_millis());
            if left {
                tx.wake_after(Duration::from_millis(LEAVE_MS));
            }
            left
        });
    }

    pub fn set_style(&self, style: HudStyle) {
        self.each(move |overlay, _| {
            let changed = overlay.style != style;
            overlay.style = style;
            changed
        });
    }

    /// Moves every panel to the other screen edge; the capsule's entry follows it in `pill`.
    pub fn set_position(&self, position: HudPosition) {
        self.each(move |overlay, _| {
            let changed = overlay.position != position;
            overlay.position = position;
            if changed {
                overlay.place();
            }
            changed
        });
    }

    /// Keeps the capsule on screen (`settings.hud_stay`), or lets it leave on its own again.
    pub fn set_stay(&self, stay: bool) {
        self.each(move |overlay, tx| {
            if overlay.stay == stay {
                return false;
            }
            overlay.stay = stay;
            let now_ms = overlay.now_ms();
            overlay.flow.set_stay(stay, now_ms);
            overlay.place();
            overlay.schedule_wake(now_ms, tx);
            true
        });
    }

    fn each(&self, f: impl Fn(&mut HudOverlay, &mut Tx) -> bool + Clone + Send + 'static) {
        let Ok(panels) = self.panels.lock() else {
            return;
        };
        for panel in &panels.panels {
            let f = f.clone();
            panel.handle.update_if(move |overlay, tx| f(overlay, tx));
        }
    }
}

impl Panels {
    /// Every app in `event`: its cached icon (when installed) and display name, looked up once.
    fn metadata_for(&mut self, event: &UiHudEvent) -> (Icons, HashMap<String, String>) {
        let mut icons = Icons::new();
        let mut names = HashMap::new();
        for workspace in event.displays.iter().flat_map(|row| &row.workspaces) {
            let tiles = workspace
                .tiles
                .iter()
                .filter_map(|tile| tile.bundle.as_ref());
            for bundle in tiles.chain(&workspace.floating) {
                if !names.contains_key(bundle) {
                    let app = self.apps.get(bundle);
                    names.insert(bundle.clone(), app.name);
                    if let Some(pixels) = app.icon {
                        icons.insert(bundle.clone(), pixels);
                    }
                }
            }
        }
        (icons, names)
    }

    /// Gives each display in `rows` a panel, and hides the panels of displays it leaves out.
    fn assign(&mut self, rows: &[(UiHudDisplay, DisplayTarget, Option<u32>)]) {
        let current: Vec<Option<String>> = self
            .panels
            .iter()
            .map(|panel| panel.display.clone())
            .collect();
        let wanted: Vec<&str> = rows.iter().map(|(row, _, _)| row.id.as_str()).collect();
        for (panel, display) in self.panels.iter_mut().zip(assign_panels(&current, &wanted)) {
            if panel.display.is_some() && panel.display != display {
                panel.handle.update_if(|overlay, _| {
                    overlay.flow.dismiss();
                    if let Some(handle) = &overlay.handle {
                        handle.hide();
                    }
                    false
                });
            }
            panel.display = display;
        }
    }
}

/// Which display each panel shows: every display it already showed and `wanted` still lists
/// stays put, the rest are freed, and new displays take free panels in order. Displays past
/// the pool get none.
#[must_use]
pub fn assign_panels(current: &[Option<String>], wanted: &[&str]) -> Vec<Option<String>> {
    let mut panels: Vec<Option<String>> = current
        .iter()
        .map(|display| display.clone().filter(|id| wanted.contains(&id.as_str())))
        .collect();
    for id in wanted {
        if panels.iter().any(|display| display.as_deref() == Some(*id)) {
            continue;
        }
        if let Some(free) = panels.iter_mut().find(|display| display.is_none()) {
            *free = Some((*id).to_owned());
        }
    }
    panels
}

const fn native(id: u32) -> DisplayTarget {
    DisplayTarget::Native(NativeDisplayId::MacOS(id))
}

#[cfg(target_os = "macos")]
fn display_ids() -> std::collections::HashMap<String, u32> {
    fractile::macos::display_ids()
}

#[cfg(not(target_os = "macos"))]
fn display_ids() -> std::collections::HashMap<String, u32> {
    std::collections::HashMap::new()
}

/// How far the area windows tile in sits from `display`'s `position` edge: the Dock's height
/// below, the menu bar's above; 0 when the display can't be read.
#[cfg(target_os = "macos")]
fn bar_inset(display: u32, position: HudPosition) -> f32 {
    let Ok(outputs) = fractile::macos::outputs() else {
        return 0.0;
    };
    let ids = display_ids();
    let Some(index) = outputs
        .displays()
        .iter()
        .position(|output| ids.get(&output.id) == Some(&display))
    else {
        return 0.0;
    };
    let (frame, screen) = (outputs.displays()[index].frame, outputs.screens()[index]);
    let inset = match position {
        HudPosition::Bottom => (screen.y + screen.height) - (frame.y + frame.height),
        HudPosition::Top => frame.y - screen.y,
        HudPosition::Left => frame.x - screen.x,
        HudPosition::Right => (screen.x + screen.width) - (frame.x + frame.width),
    };
    inset.max(0) as f32
}

/// `display`'s whole screen and the area windows tile in (Dock and menu bar left out), in
/// points, y down; the main display's when `display` is `None`.
#[cfg(target_os = "macos")]
fn display_rects(display: Option<u32>) -> Option<(fractile::Rect, fractile::Rect)> {
    let outputs = fractile::macos::outputs().ok()?;
    let ids = display_ids();
    let index = outputs.displays().iter().position(|output| match display {
        Some(display) => ids.get(&output.id) == Some(&display),
        None => output.is_main,
    })?;
    Some((outputs.screens()[index], outputs.displays()[index].frame))
}

#[cfg(not(target_os = "macos"))]
fn display_rects(_display: Option<u32>) -> Option<(fractile::Rect, fractile::Rect)> {
    None
}

#[cfg(not(target_os = "macos"))]
fn bar_inset(_display: u32, _position: HudPosition) -> f32 {
    0.0
}

#[cfg(target_os = "macos")]
fn reduce_motion() -> bool {
    objc2_app_kit::NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion()
}

#[cfg(not(target_os = "macos"))]
fn reduce_motion() -> bool {
    false
}

fn overlay_poll(overlay: &mut HudOverlay, _clock: f64, tx: &mut Tx) -> bool {
    let now_ms = fractile::journal::unix_millis();
    if overlay.flow.is_up()
        && let Some(display) = overlay.display
        && !display_ids().values().any(|id| *id == display)
    {
        // Its display went away: Rune would move the panel to the main display, over that
        // display's own capsule.
        overlay.flow.dismiss();
        if let Some(handle) = &overlay.handle {
            handle.hide();
        }
        return false;
    }
    let tick = overlay.flow.tick(now_ms);
    if tick == HudTick::Gone {
        overlay.hovered_app = None;
        overlay.hovered_workspace = None;
        if let Some(handle) = &overlay.handle {
            handle.hide();
        }
        // A toast took the place of a capsule kept on screen: the capsule comes back.
        if let Some(row) = overlay.flow.resume_row()
            && overlay.flow.trigger(row, now_ms) == HudTransition::Show
            && let Some(handle) = &overlay.handle
        {
            overlay.place();
            handle.fade_in(REVEAL_FADE);
            overlay.schedule_wake(now_ms, tx);
            return true;
        }
    }
    overlay.schedule_wake(now_ms, tx);
    matches!(tick, HudTick::Arrive | HudTick::Leave)
}

fn overlay_view(overlay: &HudOverlay, cx: &mut Cx<HudOverlay>) -> El {
    if overlay.reduce_motion {
        cx.reduced_motion(true);
    }
    let p = Paint::new(true, cx.env().scale);
    let still = cx.is_reduced_motion();
    overlay.flow.mark_drawn();
    let content = overlay
        .flow
        .view(still)
        .map(|view| {
            pill::interactive_slot(
                &view,
                overlay.position,
                overlay.style,
                &overlay.icons,
                OVERLAY_SIZE.0,
                p,
            )
        })
        .or_else(|| {
            overlay
                .flow
                .toast_view(still)
                .map(|view| pill::toast_slot(&view, overlay.position, p))
        });
    row()
        .key("hud-root")
        .w_full()
        .h_full()
        .center()
        .children(content)
}
