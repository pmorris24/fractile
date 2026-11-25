use super::{drag::drag_step, *};
use crate::Sheet;

/// The cards' width as Settings opens.
const WIDTH: f32 = 660.0;

#[test]
fn the_map_shares_out_the_cards_width() {
    let doc = Doc::parse(include_str!("../../fixtures/config.toml"));
    // The smallest window, the one Settings opens at, and the widest the cards get.
    for width in [600.0, WIDTH, METRICS.content_max] {
        let grid = Layout::new(&doc, width).grid;
        let last_tile = grid.tile_origin(COLS - 1).0 + grid.tile_w();
        assert!((last_tile - width).abs() < 0.01, "tiles end at {last_tile}");
        let last_chip = grid.chip_origin(CHIPS_PER_ROW - 1).0 + grid.chip_w();
        assert!(
            (last_chip - (width - BOX_PAD)).abs() < 0.01,
            "chips end at {last_chip}"
        );
        // Six icons still fit a tile at the narrowest.
        let (x, _) = Layout::icon_offset(ICONS_PER_ROW - 1);
        assert!(x + ICON + TILE_PAD <= grid.tile_w());
    }
}

#[test]
fn hit_tests_tiles_and_the_float_box() {
    let doc = Doc::parse(include_str!("../../fixtures/config.toml"));
    let layout = Layout::new(&doc, WIDTH);
    let grid = &layout.grid;
    assert_eq!(grid.tiles.len(), 11, "1–10 and M");
    let ws = |name| Some(Place::Workspace(WorkspaceId::parse(name).unwrap()));
    // Tiles sit under their screen's heading, so the map's first line is no target.
    assert_eq!(grid.target_at(5.0, 5.0), None);
    let (x, y) = grid.tile_origin(0);
    assert_eq!(grid.target_at(x + 5.0, y + 5.0), ws("1"));
    for name in ["4", "M", "5", "10"] {
        let index = grid
            .tiles
            .iter()
            .position(|tile| Some(Place::Workspace(*tile)) == ws(name))
            .unwrap();
        let (x, y) = grid.tile_origin(index);
        assert_eq!(grid.target_at(x + 50.0, y + 50.0), ws(name), "tile {name}");
    }
    // The gap between two screens' cards is no target either.
    let (_, first_secondary) = grid.screen_tops()[1];
    let (_, y) = grid.tile_origin(first_secondary);
    assert_eq!(grid.target_at(20.0, y - HEAD_GAP - HEAD_H / 2.0), None);
    assert_eq!(
        grid.target_at(20.0, grid.box_top() + 10.0),
        Some(Place::Float)
    );
    assert_eq!(grid.target_at(20.0, grid.height() + 100.0), None);
}

#[test]
fn rules_map_includes_workspace_10_by_default_and_once_when_referenced() {
    let ws10 = WorkspaceId::new(10).unwrap();
    let ws_m = WorkspaceId::named('M').unwrap();

    // Default config shows all ten numbered workspaces and no unmentioned named ones.
    let mut doc = Doc::parse("version = 1\n");
    let layout = Layout::new(&doc, WIDTH);
    let expected_numbered: Vec<WorkspaceId> = WorkspaceId::numbered().collect();
    assert_eq!(layout.grid.tiles, expected_numbered);
    assert_eq!(layout.grid.tiles.len(), 10);
    let (x, y) = layout.grid.tile_origin(9);
    assert_eq!(
        layout.grid.target_at(x + 50.0, y + 50.0),
        Some(Place::Workspace(ws10))
    );

    // Adding an app to tile 10 writes a valid route to workspace 10 and keeps 10 once.
    doc.add("com.example.Editor", Place::Workspace(ws10));
    let source = doc.source();
    let config = spaceman::config::Config::parse(&source).unwrap();
    let sample_window = spaceman::ObservedWindow {
        id: spaceman::PhysicalWindowId {
            app: spaceman::AppInstanceId {
                pid: 1,
                launched_at: 1,
            },
            window_id: 1,
        },
        frame: spaceman::Rect {
            x: 0,
            y: 0,
            width: 800,
            height: 600,
        },
        resizable: true,
        title: None,
        document: None,
    };
    assert_eq!(
        config.disposition("com.example.Editor", &sample_window),
        spaceman::config::WindowDisposition::Route(ws10)
    );
    let after_add = Layout::new(&doc, WIDTH);
    assert_eq!(after_add.grid.tiles, expected_numbered);
    assert_eq!(after_add.routed[9].len(), 1);
    assert!(after_add.spot(&doc.rules()[0]).is_some());

    // Mentioning workspace 10 in [[workspaces]] also keeps workspace 10 once.
    doc.set_monitor(ws10, Some("tertiary"));
    assert_eq!(Layout::new(&doc, WIDTH).grid.tiles, expected_numbered);

    // A named workspace joins its screen's card, after that screen's numbered ones.
    doc.set_monitor(ws_m, Some("main"));
    let with_named = Layout::new(&doc, WIDTH);
    assert_eq!(with_named.grid.tiles.len(), 11);
    assert_eq!(with_named.grid.tiles[4], ws_m);
    assert_eq!(with_named.grid.tiles[10], ws10);
    let (mx, my) = with_named.grid.tile_origin(4);
    assert_eq!(
        with_named.grid.target_at(mx + 50.0, my + 50.0),
        Some(Place::Workspace(ws_m))
    );

    // Moving a workspace to another screen moves its tile into that screen's card.
    doc.set_monitor(ws10, Some("main"));
    let moved = Layout::new(&doc, WIDTH);
    assert_eq!(moved.grid.screens[0].len, 6, "1–4, 10 and M");
    assert_eq!(moved.grid.tiles[4], ws10);
}

#[test]
fn every_rule_has_a_spot() {
    let doc = Doc::parse(include_str!("../../fixtures/config.toml"));
    let layout = Layout::new(&doc, WIDTH);
    for rule in doc.rules() {
        assert!(layout.spot(&rule).is_some(), "{rule:?}");
    }
}

#[test]
fn rule_drag_preserves_external_save_and_flushes_deferred_reload_on_cancel() {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "spaceman-map-drag-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config.toml");
    let initial = concat!(
        "version = 1\n\n",
        "[settings]\n",
        "hud = true\n\n",
        "[[rules]]\n",
        "bundle_id = \"com.apple.TextEdit\"\n",
        "action = \"route\"\n",
        "workspace = 1\n",
    );
    let external_hud_off = concat!(
        "version = 1\n\n",
        "[settings]\n",
        "hud = false\n\n",
        "[[rules]]\n",
        "bundle_id = \"com.apple.TextEdit\"\n",
        "action = \"route\"\n",
        "workspace = 1\n",
    );
    std::fs::write(&path, initial).unwrap();

    let mut ui = crate::playground::blank_fixture();
    ui.fixture = false;
    ui.doc = Doc::load_from(path.clone());
    let layout = Layout::new(&ui.doc, WIDTH);
    let rule = ui.doc.rules()[0].clone();
    let (origin, size) = layout.spot(&rule).unwrap();
    let ws2 = layout.grid.tile_origin(1);
    let dx = ws2.0 + layout.grid.tile_w() / 2.0 - origin.0 - size.0 / 2.0;
    let dy = ws2.1 + TILE_H / 2.0 - origin.1 - size.1 / 2.0;

    let grab = (size.0 / 2.0, size.1 / 2.0);

    // 1. Hold pointer down on TextEdit, receive an external save mid-drag, then drop on ws 2.
    drag_step(
        &mut ui,
        rune::Phase::Down,
        grab,
        (0.0, 0.0),
        false,
        &rule,
        origin,
        size,
        |x, y| layout.grid.target_at(x, y),
    );
    assert!(ui.drag.is_some());
    std::fs::write(&path, external_hud_off).unwrap();
    assert!(
        !ui.on_config_changed(),
        "reload must defer while a drag is active"
    );
    assert!(ui.doc.reload_pending());
    drag_step(
        &mut ui,
        rune::Phase::Move,
        grab,
        (dx, dy),
        false,
        &rule,
        origin,
        size,
        |x, y| layout.grid.target_at(x, y),
    );
    drag_step(
        &mut ui,
        rune::Phase::Up,
        grab,
        (dx, dy),
        false,
        &rule,
        origin,
        size,
        |x, y| layout.grid.target_at(x, y),
    );
    assert!(ui.drag.is_none());
    assert_eq!(ui.error, None);
    assert!(!ui.doc.reload_pending());
    let saved = std::fs::read_to_string(&path).unwrap();
    let parsed = spaceman::config::Config::parse(&saved).unwrap();
    assert!(!parsed.hud(), "external hud = false must survive:\n{saved}");
    assert_eq!(ui.doc.rules()[0].place, Place::Workspace(WorkspaceId::TWO));
    assert!(saved.contains("workspace = 2"), "{saved}");

    // 2. Cancelled drag flushes a deferred reload immediately instead of leaving Rules stale.
    let rule_ws2 = ui.doc.rules()[0].clone();
    drag_step(
        &mut ui,
        rune::Phase::Down,
        grab,
        (0.0, 0.0),
        false,
        &rule_ws2,
        origin,
        size,
        |x, y| layout.grid.target_at(x, y),
    );
    std::fs::write(&path, initial).unwrap();
    assert!(!ui.on_config_changed());
    drag_step(
        &mut ui,
        rune::Phase::Up,
        grab,
        (0.0, 0.0),
        true,
        &rule_ws2,
        origin,
        size,
        |x, y| layout.grid.target_at(x, y),
    );
    assert!(ui.drag.is_none());
    assert!(!ui.doc.reload_pending());
    assert!(ui.doc.hud(), "deferred reload must apply after drag cancel");
    assert_eq!(ui.doc.rules()[0].place, Place::Workspace(WorkspaceId::ONE));

    // 3. Control: idle config_changed notification reloads immediately.
    std::fs::write(&path, external_hud_off).unwrap();
    assert!(ui.on_config_changed());
    assert!(!ui.doc.hud());

    drop(std::fs::remove_dir_all(dir));
}

#[test]
fn pin_and_title_contains_rules_round_trip_as_non_app_wide_floats() {
    let mut doc = Doc::parse(
        "version = 1\n\n[[rules]]\nbundle_id = \"com.apple.Stickies\"\naction = \"pin\"\n\n[[rules]]\nbundle_id = \"com.apple.Safari\"\ntitle_contains = \"Work\"\naction = \"route\"\nworkspace = 2\n",
    );
    assert!(doc.error.is_none(), "{:?}", doc.error);
    let rules = doc.rules();
    assert_eq!(rules[0].place, Place::Pin);
    assert!(rules[0].app_wide());
    assert_eq!(rules[1].title_contains.as_deref(), Some("Work"));
    assert!(!rules[1].app_wide());
    assert_eq!(doc.app_place("com.apple.Safari"), None);
    assert_eq!(Layout::new(&doc, WIDTH).floating.len(), 1);

    // An app-wide Safari rule is added beside the title rule, never over it.
    doc.set_app_place("com.apple.Safari", Some(Place::Workspace(WorkspaceId::ONE)));
    let source = doc.source();
    assert!(source.contains("title_contains = \"Work\""), "{source}");
    assert!(source.contains("action = \"pin\""), "{source}");
    spaceman::config::Config::parse(&source).unwrap();
}

const SAFARI: &str = "com.apple.Safari";
const STICKIES: &str = "com.apple.Stickies";
const CALCULATOR: &str = "com.apple.calculator";

/// Settings on Rules, with the fixture's rules.
fn rules_ui() -> Ui {
    let mut ui = crate::playground::blank_fixture();
    reset_fixture(&mut ui, Pane::Rules);
    ui
}

fn disposition(doc: &Doc, bundle_id: &str) -> spaceman::config::WindowDisposition {
    let window = spaceman::ObservedWindow {
        id: spaceman::PhysicalWindowId {
            app: spaceman::AppInstanceId {
                pid: 1,
                launched_at: 1,
            },
            window_id: 1,
        },
        frame: spaceman::Rect {
            x: 0,
            y: 0,
            width: 800,
            height: 600,
        },
        resizable: true,
        title: None,
        document: None,
    };
    spaceman::config::Config::parse(&doc.source())
        .unwrap()
        .disposition(bundle_id, &window)
}

#[test]
fn the_add_menu_lists_apps_that_already_have_rules() {
    let mut ui = rules_ui();
    ui.edit(|doc| doc.add_rule(SAFARI, Some("Work"), Place::Workspace(WorkspaceId::ONE)));

    // Slack and Notes have every-window rules in the fixture, and Safari now has a
    // title_contains rule. All three used to be filtered out of every other workspace's menu.
    let place = Place::Workspace(WorkspaceId::TWO);
    ui.open_add_menu(place);
    assert_eq!(ui.menu, Some(Menu::Add(place)));
    let offered: Vec<&str> = ui
        .running
        .iter()
        .map(|app| app.bundle_id.as_str())
        .collect();
    assert_eq!(offered.len(), apps::fixture_running().len());
    let ruled = filter::ruled_apps(&ui.doc);
    for bundle_id in [SAFARI, "com.tinyspeck.slackmacgap", "com.apple.Notes"] {
        assert!(ruled.iter().any(|id| id == bundle_id), "{bundle_id}");
        assert!(offered.contains(&bundle_id), "{bundle_id} left out of +");
    }
    let mut layer = Vec::new();
    let _ = pane(&ui, Paint::new(true, 2.0), &mut layer);
    assert!(!layer.is_empty(), "the open menu mounts above the page");
}

#[test]
fn a_second_rule_for_an_app_takes_a_title_filter_that_starts_blank() {
    let (one, two) = (
        Place::Workspace(WorkspaceId::ONE),
        Place::Workspace(WorkspaceId::TWO),
    );
    let mut ui = rules_ui();

    // An app with no rule takes one for every window at once, with no sheet.
    ui.add_app(SAFARI, one);
    assert!(ui.sheet.is_none());
    assert_eq!(ui.doc.app_place(SAFARI), Some(one));

    // Its second rule asks which windows, in a field nothing has filled in.
    ui.add_app(SAFARI, two);
    let Some(Sheet::Filter(draft)) = ui.sheet.clone() else {
        panic!("a second Safari rule opens the filter sheet");
    };
    assert_eq!((draft.place, draft.editing), (two, None));
    assert_eq!(
        ui.title_filter.text, "",
        "never filled from a window's title"
    );

    // Left blank it would double the every-window rule, so nothing is written.
    let before = ui.doc.source();
    ui.commit_filter();
    assert!(ui.sheet.is_some());
    assert_eq!(ui.doc.source(), before);

    // Typed, it writes a rule Spaceman's own parser accepts, ahead of the every-window rule.
    ui.title_filter = rune::TextEdit::new("  Claude ");
    ui.commit_filter();
    assert!(ui.sheet.is_none());
    assert_eq!(ui.error, None);
    let source = ui.doc.source();
    spaceman::config::Config::parse(&source).unwrap();
    assert!(
        source.contains(concat!(
            "[[rules]]\n",
            "bundle_id = \"com.apple.Safari\"\n",
            "title_contains = \"Claude\"\n",
            "action = \"route\"\n",
            "workspace = 2\n\n",
            "[[rules]]\n",
            "bundle_id = \"com.apple.Safari\"\n",
            "action = \"route\"\n",
            "workspace = 1\n",
        )),
        "{source}"
    );

    // Both Safari rules are on the map, each on its workspace.
    let layout = Layout::new(&ui.doc, WIDTH);
    let safari = |tile: usize| {
        layout.routed[tile]
            .iter()
            .find(|rule| rule.bundle_id.as_deref() == Some(SAFARI))
            .cloned()
            .unwrap()
    };
    assert!(safari(0).app_wide());
    assert_eq!(safari(1).title_contains.as_deref(), Some("Claude"));

    // The filtered rule's menu reopens the sheet with its own filter from config.toml.
    ui.edit_filter(&safari(1));
    assert_eq!(ui.title_filter.text, "Claude");
    ui.title_filter = rune::TextEdit::new("Docs");
    ui.commit_filter();
    assert!(ui.sheet.is_none());
    let docs = ui.doc.rule_matching(SAFARI, Some("docs"), None).unwrap();
    assert_eq!(docs.place, two);
    assert!(ui.doc.rule_matching(SAFARI, Some("Claude"), None).is_none());

    // Clearing it would collide with the every-window rule: the sheet stays, the file too.
    ui.edit_filter(&docs);
    ui.title_filter = rune::TextEdit::new("");
    let before = ui.doc.source();
    ui.commit_filter();
    assert!(ui.sheet.is_some());
    assert_eq!(ui.doc.source(), before);
    ui.close_sheet();
    assert!(ui.sheet.is_none());
}

#[test]
fn the_filter_sheet_mounts_over_rules_whatever_the_hud_setting() {
    let p = Paint::new(true, 2.0);
    let mut ui = rules_ui();
    fixture(&mut ui);
    filter::fixture_sheet(&mut ui, "Docs");
    assert!(matches!(ui.sheet, Some(Sheet::Filter(_))));

    // Turning the HUD off closes only the HUD's own sheet.
    ui.edit(|doc| doc.set_hud(false));
    assert!(matches!(ui.sheet, Some(Sheet::Filter(_))));
    let mut layer = Vec::new();
    crate::panes::sheet(&ui, p, false, &mut layer);
    assert_eq!(layer.len(), 2, "a scrim and the sheet");

    ui.select_pane(Pane::General);
    assert!(ui.sheet.is_none());
}

#[test]
fn built_in_floats_are_default_chips_and_take_a_tile_or_pin_override() {
    use spaceman::config::WindowDisposition;

    let mut ui = rules_ui();
    ui.default_floats = [STICKIES, CALCULATOR, "com.apple.systempreferences", SAFARI]
        .map(str::to_owned)
        .into();
    let chips = |ui: &Ui| -> Vec<(&'static str, bool)> {
        Layout::with_running(&ui.doc, WIDTH, &ui.displays, &[], &ui.default_floats)
            .defaults
            .iter()
            .map(|item| (item.bundle_id, item.tiled))
            .collect()
    };
    let on_map = |ui: &Ui, bundle_id: &str| {
        Layout::with_running(&ui.doc, WIDTH, &ui.displays, &[], &ui.default_floats)
            .floating
            .iter()
            .find(|rule| rule.bundle_id.as_deref() == Some(bundle_id))
            .map(|rule| rule.place)
    };

    // Running built-in floats show as chips. System Settings has a float rule of its own in
    // the fixture, so it is on the map as that rule, and Safari is no built-in float.
    assert_eq!(chips(&ui), [(STICKIES, false), (CALCULATOR, false)]);
    assert_eq!(
        on_map(&ui, "com.apple.systempreferences"),
        Some(Place::Float)
    );
    // A chip is not a rule: nothing about Stickies is in the file, and it floats anyway.
    assert_eq!(on_map(&ui, STICKIES), None);
    assert!(!ui.doc.source().contains(STICKIES));
    assert_eq!(disposition(&ui.doc, STICKIES), WindowDisposition::Floating);
    // The chips sit under the map and change none of its drop targets.
    let (with, without) = (
        Layout::with_running(&ui.doc, WIDTH, &ui.displays, &[], &ui.default_floats).grid,
        Layout::new(&ui.doc, WIDTH).grid,
    );
    assert_eq!(with.height(), without.height());

    // Tile writes the override, and the chip says so, with Stickies running or not.
    ui.set_default_float(STICKIES, Some(Place::Tile));
    assert_eq!(ui.error, None);
    assert!(
        ui.doc
            .source()
            .contains("[[rules]]\nbundle_id = \"com.apple.Stickies\"\naction = \"tile\"\n"),
        "{}",
        ui.doc.source()
    );
    assert_eq!(disposition(&ui.doc, STICKIES), WindowDisposition::Tiled);
    assert_eq!(chips(&ui), [(STICKIES, true), (CALCULATOR, false)]);
    ui.default_floats.clear();
    assert_eq!(chips(&ui), [(STICKIES, true)]);

    // Float takes the rule away again: the default needs none.
    ui.set_default_float(STICKIES, None);
    assert!(!ui.doc.source().contains(STICKIES));
    assert!(chips(&ui).is_empty());

    // Pin on all workspaces is a rule of the user's, so it joins the float box.
    ui.default_floats = vec![STICKIES.to_owned()];
    ui.set_default_float(STICKIES, Some(Place::Pin));
    assert!(ui.doc.source().contains("action = \"pin\""));
    assert_eq!(disposition(&ui.doc, STICKIES), WindowDisposition::Pinned);
    assert!(chips(&ui).is_empty());
    assert_eq!(on_map(&ui, STICKIES), Some(Place::Pin));
}

#[test]
fn every_rules_scene_builds_at_both_widths_and_themes() {
    for (name, apply) in SCENES {
        for dark in [true, false] {
            for width in [600.0, WIDTH] {
                let p = Paint::new(dark, 2.0);
                let mut ui = crate::playground::blank_fixture();
                ui.card_w = width;
                apply(&mut ui);
                assert_eq!(ui.pane, Pane::Rules, "{name}");
                assert_eq!(ui.error, None, "{name}");
                let mut layer = Vec::new();
                let _ = pane(&ui, p, &mut layer);
                crate::panes::sheet(&ui, p, false, &mut layer);
                let overlay = ui.menu.is_some() || ui.sheet.is_some();
                assert_eq!(!layer.is_empty(), overlay, "{name}");
            }
        }
    }
}

#[test]
fn a_screen_with_open_windows_gets_taller_tiles_that_still_hit_test() {
    let doc = Doc::parse("version = 1\n");
    let five = WorkspaceId::FIVE;
    let layout = Layout::with_running(&doc, WIDTH, &[], &[five], &[]);
    let grid = &layout.grid;
    // Main (1–4) has no open window; Secondary (5–10) has one on 5.
    assert_eq!(grid.screens[0].tile_h, TILE_H);
    assert_eq!(grid.screens[1].tile_h, LIVE_TILE_H);
    let index = grid.tiles.iter().position(|tile| *tile == five).unwrap();
    let (x, y) = grid.tile_origin(index);
    assert_eq!(grid.tile_h(index), LIVE_TILE_H);
    // The preview's bottom edge still drops onto workspace 5.
    assert_eq!(
        grid.target_at(x + 10.0, y + LIVE_TILE_H - 5.0),
        Some(Place::Workspace(five))
    );
    let below = grid.tiles[index + COLS];
    let (_, next_row) = grid.tile_origin(index + COLS);
    assert!((next_row - (y + LIVE_TILE_H + GAP)).abs() < 0.01);
    assert_eq!(
        grid.target_at(x + 10.0, next_row + 5.0),
        Some(Place::Workspace(below))
    );
}
