//! The open windows in a workspace's tile: a small screen in the shape of the display the
//! workspace shows on, with a box per window and its app's icon, so an app with three windows
//! shows three. The engine sends them with every snapshot (`UiAgentSnapshot::windows`).

use rune::{El, el};
use spaceman::{
    WorkspaceId,
    app::{
        HUD_PORTRAIT_PREVIEW_RECT, HUD_PREVIEW_RECT, UiHudTile, UiHudWorkspace, UiWorkspaceWindows,
    },
};

use crate::{
    Ui, apps,
    controls::{self, Paint},
};

/// Around and between the windows.
const GAP: f32 = 1.5;
/// The smallest a window box gets, however many share the screen.
const MIN_SIDE: f32 = 6.0;
/// The largest icon in a window box, and the smallest worth drawing.
const ICON_MAX: f32 = 16.0;
const ICON_MIN: f32 = 8.0;
/// A floating window's icon, along the screen's top edge.
const FLOAT_ICON: f32 = 10.0;
const FLOATS_SHOWN: usize = 3;

/// `workspace`'s open windows, as the engine last reported them.
pub(super) fn windows(ui: &Ui, workspace: WorkspaceId) -> Option<&UiWorkspaceWindows> {
    ui.agent
        .as_ref()?
        .windows
        .iter()
        .find(|entry| entry.windows.workspace == workspace)
}

/// The screen's size inside a `width` × `height` box, in the display's own shape.
fn screen_size(portrait: bool, width: f32, height: f32) -> (f32, f32) {
    let rect = if portrait {
        HUD_PORTRAIT_PREVIEW_RECT
    } else {
        HUD_PREVIEW_RECT
    };
    let aspect = rect.width as f32 / rect.height as f32;
    if width / height > aspect {
        (height * aspect, height)
    } else {
        (width, width / aspect)
    }
}

fn app_name(ui: &Ui, bundle: &str) -> String {
    ui.apps
        .cached(bundle)
        .map_or_else(|| apps::fallback_name(bundle), |app| app.name.clone())
}

/// What a screen reader says for the preview: each window's app, in tile order.
fn label(ui: &Ui, windows: &UiHudWorkspace) -> String {
    let names: Vec<String> = windows
        .tiles
        .iter()
        .filter_map(|tile| tile.bundle.as_deref())
        .chain(windows.floating.iter().map(String::as_str))
        .map(|bundle| app_name(ui, bundle))
        .collect();
    if names.is_empty() {
        "No open windows".to_owned()
    } else {
        format!("Open now: {}", names.join(", "))
    }
}

/// The preview of `entry`, fitted inside a `width` × `height` box.
pub(super) fn preview(
    ui: &Ui,
    p: Paint,
    entry: &UiWorkspaceWindows,
    (width, height): (f32, f32),
) -> El {
    let c = &p.k.c;
    let rect = if entry.portrait {
        HUD_PORTRAIT_PREVIEW_RECT
    } else {
        HUD_PREVIEW_RECT
    };
    let (screen_w, screen_h) = screen_size(entry.portrait, width, height);
    let label = label(ui, &entry.windows);
    let mut screen = el()
        .key("live")
        .role("img")
        .label(label.clone())
        .help(label)
        .size(screen_w, screen_h)
        .radius(p.k.radius.sm)
        .fill(c.bg_deep)
        .border(p.hair, c.border_weak);
    for tile in &entry.windows.tiles {
        screen = screen.child(window(ui, p, tile, rect, (screen_w, screen_h)));
    }
    for (slot, bundle) in entry.windows.floating.iter().take(FLOATS_SHOWN).enumerate() {
        let x = screen_w - GAP - (slot + 1) as f32 * (FLOAT_ICON + 2.0);
        screen = screen.child(
            controls::icon_or_initial(p, ui.apps.cached(bundle), &app_name(ui, bundle), FLOAT_ICON)
                .key(format!("float-{slot}"))
                .absolute(x, GAP + 1.0),
        );
    }
    screen
}

fn window(
    ui: &Ui,
    p: Paint,
    tile: &UiHudTile,
    rect: spaceman::Rect,
    (screen_w, screen_h): (f32, f32),
) -> El {
    let c = &p.k.c;
    let (x, y, w, h) = crate::hud::scale_tile_in(
        tile.rect,
        rect,
        GAP,
        screen_w - 2.0 * GAP,
        screen_h - 2.0 * GAP,
        GAP,
        MIN_SIDE,
    );
    let mut window = el()
        .key(format!("window{}", tile.key))
        .absolute(x, y)
        .size(w, h)
        .radius(2.0)
        .fill(c.layer_03)
        .border(p.hair, c.border_base)
        .center();
    let icon = (w.min(h) - 4.0).min(ICON_MAX);
    if icon >= ICON_MIN
        && let Some(bundle) = &tile.bundle
    {
        window = window.child(controls::icon_or_initial(
            p,
            ui.apps.cached(bundle),
            &app_name(ui, bundle),
            icon,
        ));
    }
    window
}

/// Two Safari windows beside Slack on workspace 1, and one Notes window on 5, for the scenes.
pub(crate) fn fixture() -> Vec<UiWorkspaceWindows> {
    let tile = |key, x, width, bundle: &str| UiHudTile {
        key,
        rect: spaceman::Rect {
            x,
            y: 0,
            width,
            height: HUD_PREVIEW_RECT.height,
        },
        bundle: Some(bundle.to_owned()),
    };
    let half = HUD_PREVIEW_RECT.width / 2;
    let quarter = HUD_PREVIEW_RECT.width / 4;
    vec![
        UiWorkspaceWindows {
            windows: UiHudWorkspace {
                workspace: WorkspaceId::ONE,
                fullscreen: false,
                tiles: vec![
                    tile(1, 0, half, "com.apple.Safari"),
                    tile(2, half, quarter, "com.apple.Safari"),
                    tile(3, half + quarter, quarter, "com.tinyspeck.slackmacgap"),
                ],
                floating: vec![],
            },
            portrait: false,
        },
        UiWorkspaceWindows {
            windows: UiHudWorkspace {
                workspace: WorkspaceId::FIVE,
                fullscreen: false,
                tiles: vec![tile(4, 0, HUD_PREVIEW_RECT.width, "com.apple.Notes")],
                floating: vec!["com.apple.Calculator".to_owned()],
            },
            portrait: false,
        },
    ]
}
