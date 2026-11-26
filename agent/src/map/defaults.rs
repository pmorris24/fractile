//! The apps Fractile floats with no rule (`fractile::defaults`), as Settings › Rules shows them: a
//! quiet chip for each one that is running, or that a rule tiles, under the float box. They are
//! not rules, so they have no fill and can't be dragged. A chip's menu writes the one rule that
//! overrides the default: `action = "tile"`, or `action = "pin"` for every workspace.

use fractile::defaults::{FLOATING_BUNDLES, floats_by_default};
use rune::{El, col, row, text};
use rune_kit::{Menu as KitMenu, MenuItem, Placement};

use super::{BOX_PAD, CHIP_GAP, CHIP_H, CHIP_ICON, HEAD_GAP, MapView, SECTION_GAP};
use crate::{
    Menu, Pane, Ui, apps, controls,
    rules::{Place, Rule},
};

/// How often the running apps are read while Settings › Rules is on screen.
const REFRESH_SECONDS: f64 = 2.0;
/// The chevron that says a chip opens a menu.
const CHEVRON: f32 = 12.0;
/// The menu fits its longest choice, "Pin on all workspaces", and its check.
const MENU_W: f32 = 200.0;

/// A built-in float on the map.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct DefaultFloat {
    /// Its place in `FLOATING_BUNDLES`, which names its open menu.
    pub index: usize,
    pub bundle_id: &'static str,
    /// A `tile` rule overrides the default.
    pub tiled: bool,
}

/// The built-in floats to show: the `running` ones that float by default, and any a rule tiles
/// (so the rule can be seen and undone with its app closed). One with any other rule of its own
/// is on the map already, as that rule.
pub(super) fn listed(rules: &[Rule], running: &[String]) -> Vec<DefaultFloat> {
    FLOATING_BUNDLES
        .iter()
        .enumerate()
        .filter_map(|(index, bundle_id)| {
            let own = rules.iter().find(|rule| {
                rule.app_wide()
                    && rule
                        .bundle_id
                        .as_deref()
                        .is_some_and(|id| id.eq_ignore_ascii_case(bundle_id))
            });
            let tiled = match own.map(|rule| rule.place) {
                None => false,
                Some(Place::Tile) => true,
                Some(_) => return None,
            };
            let is_running = running.iter().any(|id| id.eq_ignore_ascii_case(bundle_id));
            (tiled || is_running).then_some(DefaultFloat {
                index,
                bundle_id,
                tiled,
            })
        })
        .collect()
}

/// Reads which built-in floats are running while Settings › Rules is on screen, every couple of
/// seconds, so opening Stickies shows its chip without reopening Settings.
pub fn poll(ui: &mut Ui, clock: f64) -> bool {
    if ui.fixture || ui.pane != Pane::Rules {
        return false;
    }
    if ui
        .default_floats_at
        .is_some_and(|read| clock - read < REFRESH_SECONDS)
    {
        return false;
    }
    ui.default_floats_at = Some(clock);
    let running: Vec<String> = apps::running()
        .into_iter()
        .map(|app| app.bundle_id)
        .filter(|bundle_id| floats_by_default(bundle_id))
        .collect();
    if running == ui.default_floats {
        return false;
    }
    ui.default_floats = running;
    ui.look_up_apps();
    true
}

/// The built-in floats a fixture has running.
pub fn fixture_default_floats() -> Vec<String> {
    [
        "com.apple.Stickies",
        "com.apple.calculator",
        "com.1password.1password",
    ]
    .map(str::to_owned)
    .into()
}

/// Calculator tiled by a rule, and Stickies' menu open.
pub(super) fn fixture_menu(ui: &mut Ui) {
    ui.set_default_float("com.apple.calculator", Some(Place::Tile));
    ui.menu = FLOATING_BUNDLES
        .iter()
        .position(|bundle_id| *bundle_id == "com.apple.Stickies")
        .map(Menu::Default);
}

impl Ui {
    /// Overrides a built-in float with a rule (`tile`, or `pin` for every workspace), or with
    /// `None` removes the rule so the app floats by default again.
    pub fn set_default_float(&mut self, bundle_id: &str, place: Option<Place>) {
        self.menu = None;
        if self.doc.app_place(bundle_id) != place {
            let bundle_id = bundle_id.to_owned();
            self.edit(move |doc| doc.set_app_place(&bundle_id, place));
        }
    }
}

impl MapView<'_> {
    /// The built-in floats' section, when any show: its label on the text rail, and the chips
    /// on the float box's first chip column, each as wide as its name.
    pub(super) fn default_floats(self, layer: &mut Vec<El>) -> Option<El> {
        if self.layout.defaults.is_empty() {
            return None;
        }
        let mut chips = row()
            .key("default-floats")
            .px(BOX_PAD)
            .gap(CHIP_GAP)
            .flex_wrap();
        for item in &self.layout.defaults {
            chips = chips.child(self.default_chip(item, layer));
        }
        Some(
            col()
                .key("defaults")
                .w(self.layout.grid.width)
                .mt(SECTION_GAP)
                .gap(HEAD_GAP)
                .child(controls::section_header(
                    self.p,
                    "Floating by default",
                    None,
                ))
                .child(chips),
        )
    }

    /// A built-in float's chip: the app in muted ink with no fill, and a chevron for its menu.
    fn default_chip(self, item: &DefaultFloat, layer: &mut Vec<El>) -> El {
        let c = &self.p.k.c;
        let app = self.ui.apps.cached(item.bundle_id);
        let name = app.map_or_else(
            || apps::fallback_name(item.bundle_id),
            |app| app.name.clone(),
        );
        let label = if item.tiled {
            format!("{name} · tiled")
        } else {
            name.clone()
        };
        let index = item.index;
        let open = self.ui.menu == Some(Menu::Default(index));
        let mut trigger = row()
            .key(format!("default-{index}"))
            .role("button")
            .label(label.clone())
            .h(CHIP_H)
            .px(6.0)
            .gap(8.0)
            .center_y()
            .radius(self.p.k.radius.md)
            .hover(c.hover)
            .press(c.pressed)
            .focusable(true)
            .focus_ring(2.0, -2.0, c.border_focus)
            .on_press(move |ui: &mut Ui| ui.menu = Some(Menu::Default(index)))
            .child(controls::icon_or_initial(self.p, app, &name, CHIP_ICON))
            .child(
                text(label)
                    .key("name")
                    .font(self.p.row_title())
                    .color(self.p.muted())
                    .truncate()
                    .shrink(),
            )
            .child(rune::icon(self.p.k.glyph.chevron_down, CHEVRON).color(c.icon_muted));
        if open {
            trigger = trigger.fill(c.hover);
        }
        let bundle_id = item.bundle_id;
        let choice = |label: &str, on: bool, place: Option<Place>| {
            MenuItem::new(label)
                .checked(on)
                .on_select(move |ui: &mut Ui| ui.set_default_float(bundle_id, place))
        };
        KitMenu::new(format!("default-{index}-menu"))
            .trigger(trigger)
            .placement(Placement::BottomStart)
            .gutter(4.0)
            .min_width(MENU_W)
            .open(open)
            .on_close(|ui: &mut Ui| ui.menu = None)
            .item(choice("Float", !item.tiled, None))
            .item(choice("Tile", item.tiled, Some(Place::Tile)))
            .item(choice("Pin on all workspaces", false, Some(Place::Pin)))
            .build(self.p.k, layer)
    }
}
