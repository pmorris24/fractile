//! Rules as a map of screens: a card per display holding the workspaces that show on it, a tile
//! per workspace showing the apps whose new windows open there,
//! the apps that float below, and under those the running apps Spaceman floats with no rule.
//! Rules are optional: without one a window opens on the workspace in view and can move to any
//! other. Dragging an app to another tile, or to the float box, rewrites its rule. An app can
//! have several rules, each for the windows a title filter picks out (`filter`). Everything sits
//! at computed positions, so a drag knows where the pointer is from the dragged icon's own spot.
//! The map is as wide as the pane's cards (`Ui::card_w`): its tiles and chips share that width
//! out, and its labels sit on the page's text rail.

mod arrangement;
mod defaults;
mod drag;
mod filter;
mod live;
mod screen_menu;
#[cfg(test)]
mod tests;

use rune::{Cursor, El, PointerPress, Tx, el, row, text};
use rune_kit::{Menu as KitMenu, MenuItem, Placement};
use spaceman::{WorkspaceId, config::LabFeature};

pub use arrangement::ScreenDrag;
pub use defaults::{fixture_default_floats, poll as poll_default_floats};
pub use drag::Drag;
pub use filter::{FilterDraft, sheet as filter_sheet};
pub(crate) use live::fixture as live_fixture;

use crate::{
    Menu, Pane, Ui,
    apps::{self, App},
    controls::{self, METRICS, Paint},
    panes::workspaces::{self as screens, Choice, ConnectedDisplay},
    playground::{Scene, reset_fixture},
    rules::{Doc, Place, Rule},
    view::scroll_body,
};
use defaults::DefaultFloat;

const COLS: usize = 5;
const TILE_H: f32 = 100.0;
/// A tile on a screen with open windows: taller, for their preview along its bottom.
const LIVE_TILE_H: f32 = 160.0;
const LIVE_TOP: f32 = 96.0;
const LIVE_H: f32 = LIVE_TILE_H - LIVE_TOP - 10.0;
const GAP: f32 = 8.0;
/// A section label's line and the gap to what it names, as over a card.
const HEAD_H: f32 = 16.0;
const HEAD_GAP: f32 = METRICS.label_gap;
/// The first screen's heading, under the map's own.
const GRID_TOP: f32 = HEAD_H + SECTION_GAP;
const SECTION_GAP: f32 = METRICS.section_gap;
/// Inside a tile: where its icons start, their size and spacing, and how many fit.
const TILE_PAD: f32 = 12.0;
const ICONS_TOP: f32 = 38.0;
const ICON: f32 = 24.0;
const ICON_GAP: f32 = 6.0;
const ICONS_PER_ROW: usize = 3;
const ICONS_SHOWN: usize = 6;
/// The float box and its chips.
const BOX_PAD: f32 = 8.0;
const CHIP_H: f32 = 32.0;
const CHIP_GAP: f32 = 8.0;
const CHIP_ICON: f32 = 20.0;
const CHIPS_PER_ROW: usize = 4;
/// Pointer travel before a press on an app becomes a drag.
const SLOP: f32 = 3.0;
/// The dot a title rule's icon wears.
const TITLE_DOT: f32 = 8.0;
/// Beside the map's first label: what a workspace is. A rule is a default, never a fence.
const INTRO: &str = "Each screen shows one of its workspaces at a time";

pub(crate) const SCENES: &[Scene] = &[
    ("rules", fixture),
    ("rules-drag", |ui| {
        fixture(ui);
        ui.drag = Drag::fixture(&ui.doc, ui.card_w);
    }),
    ("rules-add", |ui| {
        fixture(ui);
        ui.open_add_menu(Place::Workspace(WorkspaceId::THREE));
    }),
    // A second Safari rule, for the windows a title filter picks out.
    ("rules-filter", |ui| {
        fixture(ui);
        filter::fixture_sheet(ui, "Docs");
    }),
    // The same sheet before anything is typed: Safari's every-window rule is taken.
    ("rules-filter-taken", |ui| {
        fixture(ui);
        ui.edit(|doc| doc.add_rule(filter::SAFARI, None, Place::Workspace(WorkspaceId::FOUR)));
        filter::fixture_sheet(ui, "");
    }),
    // A built-in float's choices, beside one a rule tiles.
    ("rules-default", |ui| {
        fixture(ui);
        defaults::fixture_menu(ui);
    }),
    // Three screens, each with its own card of workspaces.
    ("rules-three-screens", |ui| {
        fixture(ui);
        ui.displays = crate::panes::workspaces::fixture_three();
    }),
    // Screens of different sizes at different heights: a laptop on the left, a portrait
    // monitor on the right, as the Arrange view draws them.
    ("rules-arrangement", |ui| {
        fixture(ui);
        ui.displays = arrangement::fixture_desk();
    }),
    // Workspace 2's card mid-drag from Main to the portrait screen.
    ("rules-arrangement-drag", |ui| {
        fixture(ui);
        ui.displays = arrangement::fixture_desk();
        ui.screen_drag = arrangement::fixture_drag(ui);
    }),
    // A workspace number's menu: the screens it can show on.
    ("rules-screen-menu", |ui| {
        fixture(ui);
        ui.displays = crate::panes::workspaces::fixture_three();
        ui.menu = Some(Menu::Screen(WorkspaceId::FIVE));
    }),
];

/// The fixture's rules, and Safari twice: its Work windows on 1 and its Claude windows on 2.
fn fixture(ui: &mut Ui) {
    reset_fixture(ui, Pane::Rules);
    ui.edit(|doc| {
        doc.add_rule(
            filter::SAFARI,
            Some("Work"),
            Place::Workspace(WorkspaceId::ONE),
        );
        doc.add_rule(
            filter::SAFARI,
            Some("Claude"),
            Place::Workspace(WorkspaceId::TWO),
        );
    });
}

/// Where everything in the map goes, from the rules.
struct Layout {
    grid: DropGrid,
    /// Every display a workspace can show on, as a tile's screen menu offers them.
    choices: Vec<Choice>,
    /// Rules by tile, in file order.
    routed: Vec<Vec<Rule>>,
    floating: Vec<Rule>,
    /// The built-in floats shown under the float box.
    defaults: Vec<DefaultFloat>,
}

impl Layout {
    /// The map for a pane whose cards are `width` wide, with no display read and no built-in
    /// float running.
    fn new(doc: &Doc, width: f32) -> Layout {
        Layout::with_running(doc, width, &[], &[], &[])
    }

    /// The map for the connected `displays`, with a chip for each of the `running` apps
    /// Spaceman floats by default. A screen holding one of the `live` workspaces, those with
    /// open windows, gets the taller tiles that preview them.
    fn with_running(
        doc: &Doc,
        width: f32,
        displays: &[ConnectedDisplay],
        live: &[WorkspaceId],
        running: &[String],
    ) -> Layout {
        let rules = doc.rules();
        let defaults = defaults::listed(&rules, running);
        let choices = screens::choices(displays);
        let sections = screens::sections(
            doc,
            &choices,
            doc.workspaces_with_referenced(WorkspaceId::numbered()),
        );
        let panel = arrangement::Panel::new(&sections, &choices, displays, width);
        let tiles: Vec<WorkspaceId> = sections
            .iter()
            .flat_map(|section| section.workspaces.iter().copied())
            .collect();
        let screens: Vec<Screen> = sections
            .into_iter()
            .map(|section| Screen {
                tile_h: if section.workspaces.iter().any(|ws| live.contains(ws)) {
                    LIVE_TILE_H
                } else {
                    TILE_H
                },
                key: section.key,
                title: section.title,
                caption: section.caption,
                len: section.workspaces.len(),
            })
            .collect();
        let mut routed = vec![Vec::new(); tiles.len()];
        let mut floating = Vec::new();
        for rule in rules {
            match rule.place {
                Place::Workspace(workspace) => {
                    if let Some(index) = tiles.iter().position(|tile| *tile == workspace) {
                        routed[index].push(rule);
                    }
                }
                Place::Float | Place::Pin | Place::Ignore => floating.push(rule),
                // Tiling is the default, so a tile rule has no place on the map, except the
                // one that tiles a built-in float: that shows on its default chip.
                // `spaceman rules` and config.toml edit the others.
                Place::Tile => {}
            }
        }
        Layout {
            grid: DropGrid {
                width,
                panel,
                tiles,
                screens,
                floating_len: floating.len(),
            },
            choices,
            routed,
            floating,
            defaults,
        }
    }

    /// The `slot`th icon in a tile, from the tile's top-left.
    fn icon_offset(slot: usize) -> (f32, f32) {
        let (col, row) = (slot % ICONS_PER_ROW, slot / ICONS_PER_ROW);
        (
            TILE_PAD + col as f32 * (ICON + ICON_GAP),
            ICONS_TOP + row as f32 * (ICON + ICON_GAP),
        )
    }

    /// A rule's item: its top-left in the map and its size.
    fn spot(&self, rule: &Rule) -> Option<((f32, f32), (f32, f32))> {
        if let Some(slot) = self.floating.iter().position(|r| r.index == rule.index) {
            return Some((self.grid.chip_origin(slot), (self.grid.chip_w(), CHIP_H)));
        }
        self.routed.iter().enumerate().find_map(|(tile, rules)| {
            let slot = rules.iter().position(|r| r.index == rule.index)?;
            let (x, y) = self.grid.tile_origin(tile);
            let (dx, dy) = Self::icon_offset(slot);
            Some(((x + dx, y + dy), (ICON, ICON)))
        })
    }
}

/// One screen's card in the map: its heading, and how many of the map's tiles, in order, it holds.
#[derive(Clone, Debug)]
struct Screen {
    key: String,
    title: String,
    caption: String,
    len: usize,
    /// Its tiles' height: taller when one of its workspaces has open windows to preview.
    tile_h: f32,
}

/// The map's geometry: the screens, their tiles, and how many chips the float box holds. Drag
/// handlers keep a copy to hit-test the pointer.
#[derive(Clone, Debug)]
struct DropGrid {
    /// The map's width: the pane's card width.
    width: f32,
    /// The screens as they are arranged, under the map's heading; empty with one screen.
    panel: arrangement::Panel,
    /// Every screen's tiles, screen after screen.
    tiles: Vec<WorkspaceId>,
    screens: Vec<Screen>,
    floating_len: usize,
}

impl DropGrid {
    /// The first screen's heading: under the map's own, and the arrangement panel if any.
    fn grid_top(&self) -> f32 {
        if self.panel.is_empty() {
            GRID_TOP
        } else {
            GRID_TOP + HEAD_GAP + self.panel.height
        }
    }

    /// A workspace tile's width: the map's, shared between the columns.
    fn tile_w(&self) -> f32 {
        (self.width - (COLS - 1) as f32 * GAP) / COLS as f32
    }

    /// A float chip's width: the box's inside, shared between a row of chips.
    fn chip_w(&self) -> f32 {
        (self.width - 2.0 * BOX_PAD - (CHIPS_PER_ROW - 1) as f32 * CHIP_GAP) / CHIPS_PER_ROW as f32
    }

    /// A screen's rows of tiles; a screen with none keeps one row for its empty note.
    fn screen_rows(screen: &Screen) -> usize {
        screen.len.div_ceil(COLS).max(1)
    }

    fn rows_h(screen: &Screen) -> f32 {
        let rows = Self::screen_rows(screen) as f32;
        rows * screen.tile_h + (rows - 1.0).max(0.0) * GAP
    }

    /// Each screen's heading top, then the first tile index it holds.
    fn screen_tops(&self) -> Vec<(f32, usize)> {
        let mut top = self.grid_top();
        let mut start = 0;
        self.screens
            .iter()
            .map(|screen| {
                let here = (top, start);
                top += HEAD_H + HEAD_GAP + Self::rows_h(screen) + SECTION_GAP;
                start += screen.len;
                here
            })
            .collect()
    }

    /// The bottom of the last screen's tiles.
    fn grid_bottom(&self) -> f32 {
        self.screen_tops()
            .last()
            .zip(self.screens.last())
            .map_or(self.grid_top(), |((top, _), screen)| {
                top + HEAD_H + HEAD_GAP + Self::rows_h(screen)
            })
    }

    fn float_head_top(&self) -> f32 {
        self.grid_bottom() + SECTION_GAP
    }

    fn box_top(&self) -> f32 {
        self.float_head_top() + HEAD_H + HEAD_GAP
    }

    /// The float box holds its rules and the Add chip.
    fn box_h(&self) -> f32 {
        let rows = (self.floating_len + 1).div_ceil(CHIPS_PER_ROW) as f32;
        2.0 * BOX_PAD + rows * CHIP_H + (rows - 1.0) * CHIP_GAP
    }

    fn height(&self) -> f32 {
        self.box_top() + self.box_h()
    }

    fn tile_origin(&self, index: usize) -> (f32, f32) {
        let ((top, start), tile_h) = self
            .screen_tops()
            .into_iter()
            .zip(&self.screens)
            .find(|((_, start), screen)| (*start..start + screen.len).contains(&index))
            .map_or(((self.grid_top(), 0), TILE_H), |(at, screen)| {
                (at, screen.tile_h)
            });
        let local = index - start;
        let (col, row) = (local % COLS, local / COLS);
        (
            col as f32 * (self.tile_w() + GAP),
            top + HEAD_H + HEAD_GAP + row as f32 * (tile_h + GAP),
        )
    }

    /// The height of the tile at `index`.
    fn tile_h(&self, index: usize) -> f32 {
        let mut start = 0;
        for screen in &self.screens {
            if (start..start + screen.len).contains(&index) {
                return screen.tile_h;
            }
            start += screen.len;
        }
        TILE_H
    }

    fn chip_origin(&self, slot: usize) -> (f32, f32) {
        let (col, row) = (slot % CHIPS_PER_ROW, slot / CHIPS_PER_ROW);
        (
            BOX_PAD + col as f32 * (self.chip_w() + CHIP_GAP),
            self.box_top() + BOX_PAD + row as f32 * (CHIP_H + CHIP_GAP),
        )
    }

    /// The place under a point in the map.
    fn target_at(&self, x: f32, y: f32) -> Option<Place> {
        if !(0.0..=self.width).contains(&x) {
            return None;
        }
        if let Some(workspace) = self.panel.card_at(x, y - arrangement::TOP) {
            return Some(Place::Workspace(workspace));
        }
        for ((top, start), screen) in self.screen_tops().into_iter().zip(&self.screens) {
            let tiles_top = top + HEAD_H + HEAD_GAP;
            let rows = Self::screen_rows(screen);
            if !(tiles_top - GAP..=tiles_top + Self::rows_h(screen) + GAP).contains(&y) {
                continue;
            }
            let col = ((x + GAP / 2.0) / (self.tile_w() + GAP))
                .floor()
                .clamp(0.0, (COLS - 1) as f32);
            let row = ((y - tiles_top + GAP / 2.0) / (screen.tile_h + GAP))
                .floor()
                .clamp(0.0, (rows - 1) as f32);
            let local = row as usize * COLS + col as usize;
            return (local < screen.len)
                .then(|| self.tiles.get(start + local))
                .flatten()
                .map(|tile| Place::Workspace(*tile));
        }
        let top = self.float_head_top();
        ((0.0..=self.width).contains(&x)
            && (top..=self.box_top() + self.box_h() + GAP).contains(&y))
        .then_some(Place::Float)
    }
}

pub fn pane(ui: &Ui, p: Paint, layer: &mut Vec<El>) -> El {
    let live: Vec<WorkspaceId> = ui
        .agent
        .iter()
        .flat_map(|agent| &agent.windows)
        .map(|entry| entry.windows.workspace)
        .collect();
    let layout = Layout::with_running(&ui.doc, ui.card_w, &ui.displays, &live, &ui.default_floats);
    let width = layout.grid.width;
    let view = MapView {
        ui,
        p,
        layout: &layout,
        active: ui
            .agent
            .as_ref()
            .map_or(&[][..], |agent| agent.active_workspaces.as_slice()),
        target: ui
            .drag
            .as_ref()
            .filter(|drag| drag.moved)
            .and_then(|drag| drag.target),
    };
    // The labels sit on the text rail, as over a card; the tiles and the float box take the
    // card's edges.
    let heading = |key: &str, title: &str, accessory: Option<El>, top: f32| {
        el().key(key)
            .absolute(0.0, top)
            .w(width)
            .child(controls::section_header(p, title, accessory))
    };
    let mut map = el()
        .key("map")
        .w(width)
        .h(layout.grid.height())
        .child(heading(
            "where",
            "Where new windows open",
            Some(controls::caption(p, INTRO)),
            0.0,
        ));
    if !layout.grid.panel.is_empty() {
        map = map.child(view.arrangement().absolute(0.0, arrangement::TOP));
    }
    for ((top, _), screen) in layout
        .grid
        .screen_tops()
        .into_iter()
        .zip(&layout.grid.screens)
    {
        map = map.child(heading(
            &screen.key,
            &screen.title,
            Some(controls::caption(p, &screen.caption)),
            top,
        ));
        if screen.len == 0 {
            map = map.child(
                text("No workspaces. Use a workspace number's menu to move one here.")
                    .key(format!("{}-empty", screen.key))
                    .font(p.row_title())
                    .color(p.faint())
                    .absolute(0.0, top + HEAD_H + HEAD_GAP + 12.0),
            );
        }
    }
    for (index, workspace) in layout.grid.tiles.iter().copied().enumerate() {
        let (x, y) = layout.grid.tile_origin(index);
        map = map.child(view.tile(index, workspace, layer).absolute(x, y));
    }
    map = map
        .child(heading(
            "float-heading",
            "Floating",
            None,
            layout.grid.float_head_top(),
        ))
        .child(view.float_box(layer));
    if let Some(drag) = ui.drag.as_ref().filter(|drag| drag.moved) {
        map = map.children(view.ghost(drag));
    }
    if let Some(drag) = &ui.screen_drag {
        map = map.children(view.card_ghost(drag));
    }
    // Drag positions are in map coordinates, so scrolling the body doesn't move drop targets.
    // The built-in floats flow under the map: they are no drop target and take no rule.
    let rules = rune::col()
        .key("rules")
        .child(map)
        .children(view.default_floats(layer));
    scroll_body("rules-body", rules)
}

#[derive(Clone, Copy)]
struct MapView<'a> {
    ui: &'a Ui,
    p: Paint,
    layout: &'a Layout,
    active: &'a [WorkspaceId],
    target: Option<Place>,
}

impl MapView<'_> {
    fn tile(self, index: usize, workspace: WorkspaceId, layer: &mut Vec<El>) -> El {
        let c = &self.p.k.c;
        let rules = &self.layout.routed[index];
        let active = self.active.contains(&workspace);
        let target = self.target == Some(Place::Workspace(workspace));
        let border = if target {
            (1.5, c.border_focus)
        } else if active {
            (1.5, c.border_strong)
        } else {
            (self.p.hair, c.border_weak)
        };
        let mut tile = el()
            .key(format!("tile-{workspace}"))
            .role("group")
            .label(format!("Workspace {workspace}"))
            .size(self.layout.grid.tile_w(), self.layout.grid.tile_h(index))
            .radius(self.p.k.radius.lg)
            .fill(if target { c.layer_02 } else { c.layer_01 })
            .border(border.0, border.1)
            .hover_group()
            .child(
                self.screen_menu(workspace, active, layer)
                    .absolute(TILE_PAD - 6.0, 6.0),
            )
            .child(
                self.add_button(Place::Workspace(workspace), layer)
                    .absolute(self.layout.grid.tile_w() - 30.0, 8.0),
            );
        if let Some(entry) = live::windows(self.ui, workspace) {
            let size = (self.layout.grid.tile_w() - 2.0 * TILE_PAD, LIVE_H);
            tile = tile
                .child(live::preview(self.ui, self.p, entry, size).absolute(TILE_PAD, LIVE_TOP));
        }
        let overflow = rules.len() > ICONS_SHOWN;
        for (slot, rule) in rules.iter().enumerate() {
            let (x, y) = Layout::icon_offset(slot);
            if overflow && slot == ICONS_SHOWN - 1 {
                tile = tile.child(
                    el().key("more")
                        .size(ICON, ICON)
                        .radius(self.p.k.radius.md)
                        .fill(c.layer_03)
                        .center()
                        .child(
                            text(format!("+{}", rules.len() - slot))
                                .font(self.p.tabular(self.p.tag()))
                                .color(self.p.muted()),
                        )
                        .absolute(x, y),
                );
                break;
            }
            let item = self.draggable(rule, self.app_icon(rule, ICON), layer);
            tile = tile.child(item.absolute(x, y));
        }
        tile
    }

    /// The float box: a chip per floating app, then Add.
    fn float_box(self, layer: &mut Vec<El>) -> El {
        let c = &self.p.k.c;
        let top = self.layout.grid.box_top();
        let target = self.target == Some(Place::Float);
        let mut area = el()
            .key("float-box")
            .w(self.layout.grid.width)
            .h(self.layout.grid.box_h())
            .radius(self.p.k.radius.lg)
            .fill(if target { c.layer_02 } else { c.layer_01 })
            .border(
                if target { 1.5 } else { self.p.hair },
                if target {
                    c.border_focus
                } else {
                    c.border_weak
                },
            )
            .absolute(0.0, top);
        for (slot, rule) in self.layout.floating.iter().enumerate() {
            let (x, y) = self.layout.grid.chip_origin(slot);
            let chip = self.chip(rule);
            area = area.child(self.draggable(rule, chip, layer).absolute(x, y - top));
        }
        let (x, y) = self.layout.grid.chip_origin(self.layout.floating.len());
        area.child(self.add_chip(layer).absolute(x, y - top))
    }

    fn chip(self, rule: &Rule) -> El {
        let c = &self.p.k.c;
        let name = self.rule_label(rule);
        row()
            .size(self.layout.grid.chip_w(), CHIP_H)
            .px(6.0)
            .gap(8.0)
            .center_y()
            .radius(self.p.k.radius.md)
            .fill(c.layer_02)
            .child(self.app_icon(rule, CHIP_ICON))
            .child(
                text(match rule.place {
                    Place::Ignore => format!("{name} · ignored"),
                    Place::Pin => format!("{name} · pinned"),
                    _ => name,
                })
                .key("name")
                .font(self.p.row_title())
                .color(self.p.text())
                .truncate()
                .shrink(),
            )
    }

    fn add_chip(self, layer: &mut Vec<El>) -> El {
        let c = &self.p.k.c;
        let trigger = row()
            .key("add-float")
            .role("button")
            .label("Add app")
            .size(self.layout.grid.chip_w(), CHIP_H)
            .px(8.0)
            .gap(6.0)
            .center_y()
            .radius(self.p.k.radius.md)
            .border(self.p.hair, c.border_base)
            .hover(c.hover)
            .press(c.pressed)
            .focusable(true)
            .focus_ring(2.0, -2.0, c.border_focus)
            .on_press(|ui: &mut Ui| ui.open_add_menu(Place::Float))
            .child(rune::icon(&rune_theme_opencode::icons::PLUS, 14.0).color(c.icon_muted))
            .child(
                text("Add app")
                    .font(self.p.row_title())
                    .color(self.p.muted()),
            );
        self.add_menu("add-float-menu", trigger, Place::Float, layer)
    }

    /// A tile's Add, shown while the pointer is over the tile or its menu is open.
    fn add_button(self, place: Place, layer: &mut Vec<El>) -> El {
        let c = &self.p.k.c;
        let open = self.ui.menu == Some(Menu::Add(place));
        let trigger = el()
            .key("add")
            .role("button")
            .label("Add an app here")
            .size(22.0, 22.0)
            .center()
            .radius(self.p.k.radius.sm)
            .hover(c.hover)
            .press(c.pressed)
            .on_press(move |ui: &mut Ui| ui.open_add_menu(place))
            .child(rune::icon(&rune_theme_opencode::icons::PLUS, 14.0).color(c.icon_muted));
        let trigger = if open || self.ui.drag.is_some() {
            trigger.opacity(if open { 1.0 } else { 0.0 })
        } else {
            trigger.group_opacity(0.0, 1.0)
        };
        let key = match place {
            Place::Workspace(workspace) => format!("add-{workspace}-menu"),
            Place::Float | Place::Pin | Place::Tile | Place::Ignore => "add-menu".into(),
        };
        self.add_menu(&key, trigger, place, layer)
    }

    fn add_menu(self, key: &str, trigger: El, place: Place, layer: &mut Vec<El>) -> El {
        let open = self.ui.menu == Some(Menu::Add(place));
        let mut menu = KitMenu::new(key)
            .trigger(trigger)
            .placement(Placement::BottomStart)
            .gutter(4.0)
            .min_width(220.0)
            .open(open)
            .on_close(|ui: &mut Ui| ui.menu = None);
        if open {
            menu = menu.group_label("Running apps");
            if self.ui.running.is_empty() {
                menu = menu.item(MenuItem::new("No apps are running").disabled(true));
            }
            let ruled = filter::ruled_apps(&self.ui.doc);
            // Every running app is here, ruled or not: an app takes as many rules as it has
            // kinds of window. One with a rule already asks which windows this rule is for.
            for app in &self.ui.running {
                let bundle_id = app.bundle_id.clone();
                let lead = controls::icon_or_initial(
                    self.p,
                    self.ui.apps.cached(&bundle_id),
                    &app.name,
                    METRICS.icon,
                );
                let label = if ruled.contains(&bundle_id) {
                    format!("{}…", app.name)
                } else {
                    app.name.clone()
                };
                menu = menu.item(
                    MenuItem::new(label)
                        .lead(lead)
                        .on_select(move |ui: &mut Ui| ui.add_app(&bundle_id, place)),
                );
            }
        }
        menu.build(self.p.k, layer)
    }

    /// `item` for `rule`, dragged to move it and right-clicked for its menu.
    fn draggable(self, rule: &Rule, item: El, layer: &mut Vec<El>) -> El {
        let index = rule.index;
        let Some((origin, size)) = self.layout.spot(rule) else {
            return item;
        };
        let dragging = self
            .ui
            .drag
            .as_ref()
            .is_some_and(|drag| drag.rule == index && drag.moved);
        let grid = self.layout.grid.clone();
        let drag_rule = rule.clone();
        let item = item
            .key(format!("rule-{index}"))
            .role("button")
            .label(self.rule_name(rule))
            .help(self.rule_help(rule))
            .cursor(Cursor::Grab)
            .opacity(if dragging { 0.3 } else { 1.0 })
            .on_drag(move |ui: &mut Ui, d: &mut rune::Drag, _: &mut Tx| {
                drag::drag(ui, d, &drag_rule, origin, size, |x, y| grid.target_at(x, y))
            })
            .on_secondary_press(move |ui: &mut Ui, press: PointerPress, _: &mut Tx| {
                ui.menu = Some(Menu::Rule(index, press));
            });
        let point = match self.ui.menu {
            Some(Menu::Rule(open, point)) if open == index => Some(point),
            _ => None,
        };
        let mut menu = rune_kit::context_menu(format!("rule-{index}-menu"), item, point)
            .on_close(|ui: &mut Ui| ui.menu = None);
        if point.is_some() {
            // An exact `title` rule keeps its matcher; config.toml edits that.
            if rule.bundle_id.is_some() && rule.title.is_none() {
                let editing = rule.clone();
                menu = menu.item(
                    MenuItem::new("Title contains…")
                        .on_select(move |ui: &mut Ui| ui.edit_filter(&editing)),
                );
            }
            // A bundle-wide route rule picks which of the app's new windows it sends; a title
            // rule already sends every window whose title matches.
            if matches!(rule.place, Place::Workspace(_)) && rule.app_wide() {
                let all = rule.all_windows && self.ui.doc.lab(LabFeature::RouteAllWindows);
                let expected = rule.clone();
                menu = menu.item(
                    MenuItem::new("All new windows")
                        .description(if all {
                            "Every new window of this app opens here"
                        } else {
                            "Only the app's first window opens here"
                        })
                        .checked(all)
                        .on_select(move |ui: &mut Ui| {
                            ui.menu = None;
                            ui.try_edit(|doc| doc.set_all_windows_matching(&expected, !all));
                        }),
                );
            }
            let other = if rule.place == Place::Float {
                ("Pin on all workspaces", Place::Pin)
            } else {
                ("Float", Place::Float)
            };
            let expected = rule.clone();
            menu = menu.item(MenuItem::new(other.0).on_select(move |ui: &mut Ui| {
                ui.menu = None;
                ui.try_edit(|doc| doc.place_matching(&expected, other.1));
            }));
            let expected = rule.clone();
            menu = menu.item(MenuItem::new("Remove rule").on_select(move |ui: &mut Ui| {
                ui.menu = None;
                ui.try_edit(|doc| doc.remove_matching(&expected));
            }));
        }
        menu.build(self.p.k, layer)
    }

    /// The dragged item, following the pointer above everything else in the map.
    fn ghost(self, drag: &Drag) -> Option<El> {
        let rule = self
            .layout
            .routed
            .iter()
            .flatten()
            .chain(&self.layout.floating)
            .find(|rule| rule.index == drag.rule)?;
        let item = if drag.size.1 == CHIP_H {
            self.chip(rule)
        } else {
            self.app_icon(rule, drag.size.0)
        };
        let mut ghost = item
            .key("ghost")
            .cursor(Cursor::Grabbing)
            .absolute(drag.origin.0 + drag.delta.0, drag.origin.1 + drag.delta.1);
        for shadow in &self.p.k.elevation.floating {
            ghost = ghost.box_shadow(*shadow);
        }
        Some(ghost)
    }

    fn rule_app<'a>(self, rule: &Rule) -> Option<&'a App>
    where
        Self: 'a,
    {
        self.ui.apps.cached(rule.bundle_id.as_deref()?)
    }

    fn rule_name(self, rule: &Rule) -> String {
        match (self.rule_app(rule), &rule.bundle_id, rule.title_filter()) {
            (Some(app), _, _) => app.name.clone(),
            (None, Some(bundle_id), _) => apps::fallback_name(bundle_id),
            (None, None, Some(title)) => title.to_owned(),
            (None, None, None) => "Any app".into(),
        }
    }

    /// A chip's name: the app, and the title filter that picks out some of its windows.
    fn rule_label(self, rule: &Rule) -> String {
        let name = self.rule_name(rule);
        match (&rule.bundle_id, rule.title_filter()) {
            (Some(_), Some(filter)) => format!("{name} “{filter}”"),
            _ => name,
        }
    }

    fn rule_help(self, rule: &Rule) -> String {
        let name = self.rule_name(rule);
        match (&rule.title, &rule.title_contains) {
            (Some(title), _) => format!("{name}, windows titled “{title}”"),
            (None, Some(part)) => format!("{name}, windows whose title contains “{part}”"),
            (None, None) => name,
        }
    }

    fn app_icon(self, rule: &Rule, size: f32) -> El {
        let icon =
            controls::icon_or_initial(self.p, self.rule_app(rule), &self.rule_name(rule), size);
        if rule.app_wide() {
            return icon;
        }
        // A rule for some of an app's windows wears a dot.
        let c = &self.p.k.c;
        icon.child(
            el().key("title-dot")
                .size(TITLE_DOT, TITLE_DOT)
                .radius(TITLE_DOT / 2.0)
                .fill(c.icon_accent)
                .border(1.5, c.layer_01)
                .absolute(size - 6.0, -2.0),
        )
    }
}
