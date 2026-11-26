//! The HUD in the Playground: both styles over a mock desktop, with triggers that drive the
//! same flow and view as the live overlay, so their motion can be tried without switching a
//! real workspace.

use rune::{Cursor, El, Play, Playground, col, el};
use spaceman::{
    Rect, WorkspaceId,
    app::{UiHudDisplay, UiHudTile, UiHudWorkspace},
    config::{HudPosition, HudStyle},
};

use super::{FRESH_MS, HudFlow, Icons, LEAVE_MS, grow, pill};
use crate::{
    Pane, Ui, apps,
    controls::Paint,
    playground::{Scene, reset_fixture},
};

/// The workspaces the triggers step through.
const DEMO_WORKSPACES: u8 = 5;
/// Between the switches of a burst: quick presses, each landing while the last still moves.
const BURST_GAP: f64 = 0.16;
const STAGE_W: f32 = 640.0;
const STAGE_H: f32 = 380.0;

/// How a demo workspace lays out its windows.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Shape {
    /// The first window on the left, the rest stacked on the right.
    Main,
    Columns,
    Rows,
    Fullscreen,
}

#[derive(Clone, Debug)]
struct DemoWorkspace {
    shape: Shape,
    apps: Vec<&'static str>,
}

/// The stage's HUD: a flow on the Playground's logical clock, over a desktop of demo
/// workspaces the triggers switch between and rearrange.
#[derive(Clone, Debug)]
pub struct HudPreview {
    flow: HudFlow,
    icons: Icons,
    workspaces: Vec<DemoWorkspace>,
    visible: u8,
}

impl HudPreview {
    fn showing(visible: u8, workspaces: Vec<DemoWorkspace>) -> Self {
        let mut catalog = apps::Catalog::default();
        let icons = workspaces
            .iter()
            .flat_map(|workspace| workspace.apps.iter())
            .filter_map(|bundle| Some(((*bundle).to_owned(), catalog.get(bundle).icon?)))
            .collect();
        let mut preview = Self {
            flow: HudFlow::new(true),
            icons,
            workspaces,
            visible,
        };
        let row = preview.row(None);
        preview.flow.trigger(row, 0);
        // Nothing to travel from: land at rest.
        preview.flow.mark_drawn();
        preview.flow.tick(FRESH_MS);
        preview
    }

    /// The demo desktop for Settings' Customize sheet, on its first workspace.
    pub(crate) fn sample() -> Self {
        Self::showing(1, demo())
    }

    /// The row the runtime would send: every occupied workspace and the visible one.
    fn row(&self, from: Option<u8>) -> UiHudDisplay {
        UiHudDisplay {
            id: "stage".to_owned(),
            visible: WorkspaceId::new(self.visible).ok(),
            from: from.and_then(|from| WorkspaceId::new(from).ok()),
            workspaces: self
                .workspaces
                .iter()
                .zip(1..)
                .filter(|(workspace, number)| !workspace.apps.is_empty() || *number == self.visible)
                .filter_map(|(workspace, number)| hud_workspace(number, workspace))
                .collect(),
            order: Vec::new(),
            portrait: false,
        }
    }

    fn workspace(&mut self, number: u8) -> &mut DemoWorkspace {
        let index = usize::from(number.max(1) - 1).min(self.workspaces.len() - 1);
        &mut self.workspaces[index]
    }
}

fn hud_workspace(number: u8, workspace: &DemoWorkspace) -> Option<UiHudWorkspace> {
    let count = workspace.apps.len() as i32;
    let tiles = workspace
        .apps
        .iter()
        .zip(0..)
        .map(|(bundle, index)| UiHudTile {
            key: u64::from(number) * 100 + u64::try_from(index).unwrap_or(0),
            rect: tile_rect(workspace.shape, index, count),
            bundle: Some((*bundle).to_owned()),
        })
        .collect();
    Some(UiHudWorkspace {
        workspace: WorkspaceId::new(number).ok()?,
        fullscreen: workspace.shape == Shape::Fullscreen,
        tiles,
        floating: Vec::new(),
    })
}

fn tile_rect(shape: Shape, index: i32, count: i32) -> Rect {
    let (w, h) = (1500, 1100);
    let rect = |x, y, width, height| Rect {
        x,
        y,
        width,
        height,
    };
    match shape {
        Shape::Fullscreen => rect(0, 0, w, h),
        Shape::Columns => rect(index * w / count, 0, w / count, h),
        Shape::Rows => rect(0, index * h / count, w, h / count),
        Shape::Main if count == 1 => rect(0, 0, w, h),
        Shape::Main if index == 0 => rect(0, 0, 860, h),
        Shape::Main => {
            let rest = count - 1;
            rect(860, (index - 1) * h / rest, w - 860, h / rest)
        }
    }
}

/// A different layout and set of apps on each demo workspace; the fourth is empty.
fn demo() -> Vec<DemoWorkspace> {
    let workspace = |shape, apps: &[&'static str]| DemoWorkspace {
        shape,
        apps: apps.to_vec(),
    };
    vec![
        workspace(
            Shape::Main,
            &["com.apple.Terminal", "com.apple.Safari", "com.apple.Notes"],
        ),
        workspace(Shape::Columns, &["com.apple.MobileSMS", "com.apple.mail"]),
        workspace(Shape::Fullscreen, &["com.apple.finder", "com.apple.Music"]),
        workspace(Shape::Columns, &[]),
        workspace(Shape::Columns, &["com.apple.iCal"]),
    ]
}

/// Full-window Playground stage: the visible workspace's windows, with the default Icons capsule
/// and the Layouts capsule under it, both on the same flow.
pub fn stage(preview: &HudPreview, p: Paint, reduce_motion: bool) -> El {
    const PAD: f32 = 16.0;
    let inner_w = STAGE_W - 2.0 * p.hair - PAD * 2.0;
    let inner_h = STAGE_H - 2.0 * p.hair - PAD * 2.0;
    preview.flow.mark_drawn();
    let view = preview.flow.view(reduce_motion);
    // The windows keep to their own layer, so one fading out never draws over the capsules.
    let mut windows = el()
        .key("stage-windows")
        .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0));
    let visible = view.as_ref().and_then(|view| {
        let visible = view.row.visible?;
        view.row
            .workspaces
            .iter()
            .find(|entry| entry.workspace == visible)
    });
    if let Some(entry) = visible {
        let tiles = if entry.fullscreen {
            &entry.tiles[..entry.tiles.len().min(1)]
        } else {
            &entry.tiles[..]
        };
        for tile in tiles {
            let frame = grow::scale_tile(tile.rect, PAD, inner_w, inner_h, 4.0, 12.0);
            windows = windows.child(stage_window(tile.key, frame, p));
        }
    }
    let capsule =
        |key: &str, style| {
            el().key(key).w_full().children(view.as_ref().map(|view| {
                pill::slot(view, HudPosition::Bottom, style, &preview.icons, STAGE_W, p)
            }))
        };
    let mock = col()
        .key("desktop-mock")
        .w(STAGE_W)
        .h(STAGE_H)
        .radius(p.k.radius.xl)
        .fill(p.k.c.bg_base)
        .border(p.hair, p.k.c.border_weak)
        .pb(24.0)
        .gap(14.0)
        .center_y()
        .child(windows)
        .child(el().key("screen-spacer").grow(1.0))
        .child(capsule("icons", HudStyle::Icons))
        .child(capsule("layouts", HudStyle::Layouts));

    col()
        .key("hud-stage")
        .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
        .fill(p.k.c.bg_deep)
        .padding(32.0)
        .center()
        .child(mock)
}

/// From the miniature's edge to the capsule resting on it.
const MINIATURE_EDGE: f32 = 16.0;
/// Around and between the miniature's windows: half on the display, half on each window.
const MINIATURE_GAP: f32 = 2.0;

/// The Customize sheet's miniature: a display `size` big showing the demo workspace's windows
/// and the real capsule at full size, in `style`, resting `MINIATURE_EDGE` from the edge
/// `position` names. It is also the control: pressing its top or bottom half moves the capsule
/// there, on the row's own spring.
pub fn miniature(
    preview: &HudPreview,
    position: HudPosition,
    style: HudStyle,
    (width, height): (f32, f32),
    p: Paint,
    reduce_motion: bool,
) -> El {
    preview.flow.mark_drawn();
    let view = preview.flow.view(reduce_motion);
    let mut display = el()
        .key("hud-miniature")
        .role("group")
        .label("Workspace HUD preview")
        .size(width, height)
        .radius(p.k.radius.lg)
        .clip_rounded(p.k.radius.lg)
        .fill(p.k.c.bg_deep)
        .border(p.hair, p.k.c.border_weak);
    let visible = view.as_ref().and_then(|view| {
        let visible = view.row.visible?;
        view.row
            .workspaces
            .iter()
            .find(|entry| entry.workspace == visible)
    });
    for tile in visible.map_or(&[][..], |entry| &entry.tiles[..]) {
        let (x, y, w, h) = grow::scale_tile(
            tile.rect,
            MINIATURE_GAP,
            width - 2.0 * MINIATURE_GAP,
            height - 2.0 * MINIATURE_GAP,
            MINIATURE_GAP,
            12.0,
        );
        // Windows sit 4 pt inside the display's 8 pt corner, so theirs is 4.
        display = display.child(
            el().key(format!("window{}", tile.key))
                .absolute(x, y)
                .size(w, h)
                .radius(p.k.radius.sm)
                .fill(p.k.c.layer_01)
                .border(p.hair, p.k.c.border_weak)
                .a11y_hidden(),
        );
    }
    let half = |key: &str, label: &str, top: bool, to: HudPosition| {
        el().key(key)
            .role("button")
            .label(label)
            .pin(
                Some(0.0),
                top.then_some(0.0),
                Some(0.0),
                (!top).then_some(0.0),
            )
            .h(height / 2.0)
            .cursor(Cursor::Pointer)
            .on_press(move |ui: &mut Ui| ui.set_hud_position(to))
    };
    // Where the capsule rests: its top, or on a side its left edge, standing on end.
    let (x, y, w, h, length) = match position {
        HudPosition::Bottom => (
            0.0,
            height - MINIATURE_EDGE - pill::PILL_HEIGHT,
            width,
            pill::PILL_HEIGHT,
            width,
        ),
        HudPosition::Top => (0.0, MINIATURE_EDGE, width, pill::PILL_HEIGHT, width),
        HudPosition::Left => (MINIATURE_EDGE, 0.0, pill::PILL_HEIGHT, height, height),
        HudPosition::Right => (
            width - MINIATURE_EDGE - pill::PILL_HEIGHT,
            0.0,
            pill::PILL_HEIGHT,
            height,
            height,
        ),
    };
    display
        .child(half("show-top", "Show at the top", true, HudPosition::Top))
        .child(half(
            "show-bottom",
            "Show at the bottom",
            false,
            HudPosition::Bottom,
        ))
        .child(
            el().key("capsule")
                .absolute(x, y)
                .w(w)
                .h(h)
                .motion(pill::ROW)
                .children(
                    view.as_ref()
                        .map(|view| pill::slot(view, position, style, &preview.icons, length, p)),
                ),
        )
}

fn stage_window(key: u64, (x, y, w, h): (f32, f32, f32, f32), p: Paint) -> El {
    el().key(format!("stage-tile{key}"))
        .absolute(x, y)
        .w(w)
        .h(h)
        .radius(p.k.radius.md)
        .fill(p.k.c.layer_01)
        .border(p.hair, p.k.c.border_weak)
}

/// The motion triggers of the workspace-switch scene.
pub fn triggers(playground: Playground<Ui>) -> Playground<Ui> {
    const SCENE: &str = "HUD/hud-switch";
    playground
        .trigger(SCENE, 'r', "Switch 1, 2, 3 quickly", |ui, play| {
            burst(ui, play, &[1, 2, 3]);
        })
        .trigger(SCENE, 'n', "Next workspace", |ui, play| step(ui, play, 1))
        .trigger(SCENE, 'p', "Previous workspace", |ui, play| {
            step(ui, play, DEMO_WORKSPACES - 1);
        })
        .trigger(SCENE, 'b', "Back and forth", |ui, play| {
            let here = visible(ui);
            let there = if here == 1 { 3 } else { 1 };
            burst(ui, play, &[there, here]);
        })
        .trigger(
            SCENE,
            'm',
            "Move a window to the next workspace",
            move_window,
        )
        .trigger(SCENE, 'l', "Change this workspace's layout", relayout)
        .trigger(
            SCENE,
            'h',
            "Hide, then show the next workspace",
            hide_then_show,
        )
}

fn millis(seconds: f64) -> u64 {
    (seconds * 1_000.0).round() as u64
}

fn visible(ui: &Ui) -> u8 {
    ui.hud_preview.as_ref().map_or(1, |preview| preview.visible)
}

/// Shows the row as it is now; `from` is the workspace a switch left.
fn show(ui: &mut Ui, play: &mut Play<'_, Ui>, from: Option<u8>) {
    let now_ms = millis(play.now());
    let Some(preview) = &mut ui.hud_preview else {
        return;
    };
    let fresh = !preview.flow.is_up();
    let row = preview.row(from);
    preview.flow.trigger(row, now_ms);
    if fresh {
        // As the overlay's poll does once the panel has drawn.
        play.after((FRESH_MS + 10) as f64 / 1_000.0, |ui, play| {
            if let Some(preview) = &mut ui.hud_preview {
                preview.flow.tick(millis(play.now()));
            }
        });
    }
}

fn switch(ui: &mut Ui, play: &mut Play<'_, Ui>, to: u8) {
    let Some(preview) = &mut ui.hud_preview else {
        return;
    };
    let from = std::mem::replace(&mut preview.visible, to);
    show(ui, play, (from != to).then_some(from));
}

fn step(ui: &mut Ui, play: &mut Play<'_, Ui>, by: u8) {
    let here = visible(ui);
    play.clear();
    switch(ui, play, (here - 1 + by) % DEMO_WORKSPACES + 1);
}

fn burst(ui: &mut Ui, play: &mut Play<'_, Ui>, workspaces: &[u8]) {
    play.clear();
    let Some((&first, rest)) = workspaces.split_first() else {
        return;
    };
    switch(ui, play, first);
    for (index, &workspace) in rest.iter().enumerate() {
        play.after(BURST_GAP * (index + 1) as f64, move |ui, play| {
            switch(ui, play, workspace);
        });
    }
}

/// Moves this workspace's last window to the next workspace; the marker stays here.
fn move_window(ui: &mut Ui, play: &mut Play<'_, Ui>) {
    let Some(preview) = &mut ui.hud_preview else {
        return;
    };
    let here = preview.visible;
    let Some(app) = preview.workspace(here).apps.pop() else {
        return;
    };
    preview.workspace(here % DEMO_WORKSPACES + 1).apps.push(app);
    play.clear();
    show(ui, play, None);
}

/// Steps this workspace through its layouts: columns, rows, Fullscreen Layout, and back.
fn relayout(ui: &mut Ui, play: &mut Play<'_, Ui>) {
    let Some(preview) = &mut ui.hud_preview else {
        return;
    };
    let here = preview.visible;
    let workspace = preview.workspace(here);
    workspace.shape = match workspace.shape {
        Shape::Main | Shape::Fullscreen => Shape::Columns,
        Shape::Columns => Shape::Rows,
        Shape::Rows => Shape::Fullscreen,
    };
    play.clear();
    show(ui, play, None);
}

/// Lets the HUD leave, then switches to the next workspace: it comes back with the marker on
/// the workspace it left, and the marker travels as the capsule fades in.
fn hide_then_show(ui: &mut Ui, play: &mut Play<'_, Ui>) {
    let now_ms = millis(play.now());
    let Some(preview) = &mut ui.hud_preview else {
        return;
    };
    preview.flow.set_enabled(false, now_ms);
    preview.flow.set_enabled(true, now_ms);
    play.clear();
    let leave = LEAVE_MS as f64 / 1_000.0;
    play.after(leave, |ui, play| {
        if let Some(preview) = &mut ui.hud_preview {
            preview.flow.tick(millis(play.now()));
        }
    });
    play.after(leave + 0.4, |ui, play| {
        let here = visible(ui);
        switch(ui, play, here % DEMO_WORKSPACES + 1);
    });
}

fn hud_scene(ui: &mut Ui, visible: u8, workspaces: Vec<DemoWorkspace>) {
    reset_fixture(ui, Pane::Rules);
    ui.hud_preview = Some(HudPreview::showing(visible, workspaces));
}

/// The demo workspaces and five more, so the row overflows the capsule.
fn many() -> Vec<DemoWorkspace> {
    let extra = ["com.apple.Preview", "com.apple.TextEdit", "com.apple.Maps"];
    let mut workspaces = demo();
    workspaces.extend((0..5).map(|index| {
        let mut apps = extra.to_vec();
        apps.rotate_left(index % 3);
        DemoWorkspace {
            shape: Shape::Columns,
            apps,
        }
    }));
    workspaces
}

pub(crate) const SCENES: &[Scene] = &[
    ("hud-switch", |ui| hud_scene(ui, 1, demo())),
    ("hud-empty", |ui| hud_scene(ui, 4, demo())),
    ("hud-fullscreen", |ui| hud_scene(ui, 3, demo())),
    ("hud-many", |ui| hud_scene(ui, 7, many())),
];
