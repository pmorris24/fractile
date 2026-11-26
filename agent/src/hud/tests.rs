use spaceman::{
    Rect, WorkspaceId,
    app::{UiHudDisplay, UiHudTile, UiHudWorkspace},
};

use crate::rules::Place;

use super::{
    FRESH_MS, HOLD_MS, HudFlow, HudTick, HudTransition, LEAVE_MS, OVERLAY_MARGIN, OVERLAY_SIZE,
    RESUME_HOLD_MS, TOAST_HOLD_MS, assign_panels, grow, icons, menu, panel_edge, pill,
};

#[test]
fn the_capsule_sits_sixty_points_inside_the_edge_it_picks() {
    use rune::ScreenEdge;
    use spaceman::config::HudPosition;

    assert_eq!(panel_edge(HudPosition::Bottom), ScreenEdge::Bottom);
    assert_eq!(panel_edge(HudPosition::Top), ScreenEdge::Top);
    // The panel is the capsule plus its shadow above and below, so the capsule's centre is one
    // shadow reach in from the panel's edge, and the panel itself `OVERLAY_MARGIN` in.
    let panel_inset = OVERLAY_MARGIN + pill::SHADOW_REACH;
    assert_eq!(panel_inset, pill::CAPSULE_INSET);
    assert_eq!(panel_inset, 60.0);
    let capsule_top = OVERLAY_MARGIN + (OVERLAY_SIZE.1 - pill::PILL_HEIGHT) / 2.0;
    assert_eq!(capsule_top, pill::CAPSULE_INSET);
}

#[test]
fn a_top_capsule_drops_into_place_and_a_bottom_one_rises() {
    use spaceman::config::HudPosition;

    // Away from its resting place, the capsule starts 6 pt outside the display edge it sits on.
    assert_eq!(pill::enter_offset(HudPosition::Bottom, false, false), 6.0);
    assert_eq!(pill::enter_offset(HudPosition::Top, false, false), -6.0);
    // Shown, or under Reduce Motion, it rests exactly where it is drawn.
    assert_eq!(pill::enter_offset(HudPosition::Top, true, false), 0.0);
    assert_eq!(pill::enter_offset(HudPosition::Bottom, true, false), 0.0);
    assert_eq!(pill::enter_offset(HudPosition::Top, false, true), 0.0);
    assert_eq!(pill::enter_offset(HudPosition::Bottom, false, true), 0.0);
}

fn id(number: u8) -> WorkspaceId {
    WorkspaceId::new(number).unwrap()
}

fn workspace(number: u8, apps: &[&str]) -> UiHudWorkspace {
    let count = apps.len().max(1) as i32;
    UiHudWorkspace {
        workspace: id(number),
        fullscreen: false,
        tiles: apps
            .iter()
            .zip(0..)
            .map(|(bundle, index)| UiHudTile {
                key: u64::from(number) * 100 + index as u64,
                rect: Rect {
                    x: index * 1500 / count,
                    y: 0,
                    width: 1500 / count,
                    height: 1100,
                },
                bundle: Some((*bundle).to_owned()),
            })
            .collect(),
        floating: Vec::new(),
    }
}

/// Workspaces 1 to 3 occupied, `visible` shown, arrived at from `from`.
fn row(visible: u8, from: Option<u8>) -> UiHudDisplay {
    let mut workspaces = vec![
        workspace(1, &["a", "b"]),
        workspace(2, &["c"]),
        workspace(3, &["d", "e", "f"]),
    ];
    if visible > 3 {
        workspaces.push(workspace(visible, &[]));
    }
    UiHudDisplay {
        id: "main".to_owned(),
        visible: Some(id(visible)),
        from: from.map(id),
        workspaces,
        order: Vec::new(),
        portrait: false,
    }
}

fn shown(flow: &HudFlow) -> bool {
    flow.view(false).is_some_and(|view| view.shown)
}

fn focus(flow: &HudFlow) -> (Option<WorkspaceId>, bool) {
    let view = flow.view(false).unwrap();
    (view.focus, view.fresh)
}

/// Shown and arrived, as the overlay's first frame and poll leave it.
fn arrived(flow: &mut HudFlow, row: UiHudDisplay, now_ms: u64) {
    flow.trigger(row, now_ms);
    flow.mark_drawn();
    assert_eq!(flow.tick(now_ms + FRESH_MS), HudTick::Arrive);
}

#[test]
fn shows_on_first_trigger_and_updates_in_place_while_visible() {
    let mut flow = HudFlow::new(true);
    assert!(flow.view(false).is_none());

    arrived(&mut flow, row(1, None), 1_000);
    assert!(shown(&flow));
    assert_eq!(flow.remaining_ms(1_400), Some(HOLD_MS - 400));

    // A rapid second switch extends the hold without showing the panel again.
    assert_eq!(flow.trigger(row(2, Some(1)), 1_500), HudTransition::Update);
    assert_eq!(flow.tick(1_000 + HOLD_MS), HudTick::Idle);
    assert!(shown(&flow));

    // The hold runs out: the capsule leaves, and the panel stays up until its exit has played.
    let left = 1_500 + HOLD_MS;
    assert_eq!(flow.tick(left), HudTick::Leave);
    assert!(!shown(&flow));
    assert_eq!(flow.remaining_ms(left), Some(LEAVE_MS));
    assert_eq!(flow.tick(left + LEAVE_MS - 1), HudTick::Idle);
    // The last row stays through the exit, so the capsule never blanks on its way out.
    assert_eq!(focus(&flow).0, Some(id(2)));
    assert_eq!(flow.tick(left + LEAVE_MS), HudTick::Gone);
    assert_eq!(flow.remaining_ms(left + LEAVE_MS), None);
    assert_eq!(flow.tick(left + LEAVE_MS + 1), HudTick::Idle);
}

#[test]
fn a_switch_during_the_exit_brings_the_same_capsule_back() {
    let mut flow = HudFlow::new(true);
    arrived(&mut flow, row(1, None), 0);
    assert_eq!(flow.tick(HOLD_MS), HudTick::Leave);

    // Not a new show: the panel is still up and the marker heads straight for the new one.
    assert_eq!(
        flow.trigger(row(2, Some(1)), HOLD_MS + 60),
        HudTransition::Update
    );
    assert!(shown(&flow));
    assert_eq!(focus(&flow), (Some(id(2)), false));
    assert_eq!(flow.tick(HOLD_MS + LEAVE_MS), HudTick::Idle);
    assert_eq!(flow.tick(HOLD_MS + 60 + HOLD_MS), HudTick::Leave);
}

#[test]
fn a_fresh_show_starts_on_the_workspace_the_switch_left() {
    let mut flow = HudFlow::new(true);
    assert_eq!(flow.trigger(row(3, Some(1)), 0), HudTransition::Show);
    assert_eq!(focus(&flow), (Some(id(1)), true));
    // Not until the panel has drawn it there, however long that takes.
    assert_eq!(flow.tick(FRESH_MS), HudTick::Idle);
    assert_eq!(flow.remaining_ms(FRESH_MS), Some(16));
    // Before the first frame drew, a second switch still starts from where the first left.
    flow.trigger(row(2, Some(3)), FRESH_MS + 5);
    assert_eq!(focus(&flow), (Some(id(1)), true));
    flow.mark_drawn();
    assert_eq!(flow.tick(FRESH_MS + 10), HudTick::Arrive);
    assert_eq!(focus(&flow), (Some(id(2)), false));
    assert_eq!(flow.tick(FRESH_MS + 20), HudTick::Idle);

    // Once the first frame HAS drawn, a rapid second switch leaves `fresh` immediately so it
    // springs smoothly instead of snapping with `instant_changes()`.
    let mut rapid = HudFlow::new(true);
    rapid.trigger(row(2, Some(1)), 0);
    rapid.mark_drawn();
    rapid.trigger(row(3, Some(2)), 8);
    assert_eq!(focus(&rapid), (Some(id(3)), false));

    // With nowhere to come from, it lands at rest on the visible one.
    let mut flow = HudFlow::new(true);
    flow.trigger(row(2, None), 0);
    assert_eq!(focus(&flow), (Some(id(2)), true));
}

#[test]
fn a_workspace_the_row_no_longer_lists_stays_while_the_marker_leaves_it() {
    let mut flow = HudFlow::new(true);
    // Left an empty workspace 5, which drops out of the row once you're not on it.
    flow.trigger(row(2, Some(5)), 0);
    let numbers = |flow: &HudFlow| -> Vec<WorkspaceId> {
        flow.view(false)
            .unwrap()
            .entries()
            .iter()
            .map(|entry| entry.workspace)
            .collect()
    };
    assert_eq!(numbers(&flow), [id(1), id(2), id(3), id(5)]);
    flow.mark_drawn();
    flow.tick(FRESH_MS);
    assert_eq!(numbers(&flow), [id(1), id(2), id(3)]);
}

#[test]
fn disabling_hud_suppresses_triggers_and_dismisses_active_pill() {
    let mut flow = HudFlow::new(true);
    assert_eq!(flow.trigger(row(3, None), 500), HudTransition::Show);
    assert!(flow.set_enabled(false, 600));
    assert!(!shown(&flow));
    assert_eq!(flow.trigger(row(4, Some(3)), 650), HudTransition::Ignored);
    assert_eq!(flow.tick(600 + LEAVE_MS), HudTick::Gone);
    // Turning it off again while it's already hidden leaves nothing to dismiss.
    assert!(!flow.set_enabled(false, 2_000));
}

#[test]
fn the_layouts_row_is_as_wide_as_the_prototype_s() {
    for count in 1..=10_u8 {
        let entries: Vec<_> = (1..=count)
            .map(|number| workspace(number, &["a"]))
            .collect();
        let (_, width) = grow::rest(&entries, Some(id(1)), false);
        let n = f32::from(count);
        // `24 + w * (1 + (n - 1) * small) + gap * (n - 1)`, inside the capsule's border.
        let expected =
            24.0 + grow::SCREEN_W * (1.0 + (n - 1.0) * grow::SMALL) + grow::GAP * (n - 1.0);
        assert!(
            (width - expected).abs() < 0.01,
            "{count}: {width} vs {expected}"
        );
    }
}

#[test]
fn icons_show_each_app_once_in_tile_order_then_floating() {
    let mut entry = workspace(1, &["term", "web", "term", "notes"]);
    entry.floating = vec!["chat".to_owned(), "web".to_owned()];
    assert_eq!(icons::apps(&entry), ["term", "web", "notes", "chat"]);
    assert!(icons::apps(&workspace(2, &[])).is_empty());
}

#[test]
fn icons_marker_is_concentric_and_stays_circular_when_empty() {
    use super::pill::{INNER, Icons};
    use crate::controls::Paint;

    let entries = vec![workspace(1, &["term", "web"]), workspace(4, &[])];
    let paint = Paint::new(true, 2.0);

    let first = icons::track(&entries, Some(id(1)), &Icons::new(), paint);
    let [mx, my, _, mh] = first.marker.unwrap();
    assert_eq!(first.marker_radius, icons::MARKER_RADIUS);
    // Horizontal inset at the capsule end matches the vertical inset for a concentric pill.
    assert!((mx - my).abs() < 0.01, "x={mx} vs y={my}");
    assert!((my - (INNER - mh) / 2.0).abs() < 0.01);

    let empty = icons::track(&entries, Some(id(4)), &Icons::new(), paint);
    let [_, _, empty_w, empty_h] = empty.marker.unwrap();
    assert!(
        empty_w >= empty_h,
        "empty marker should be at least circular: {empty_w}x{empty_h}"
    );
}

#[test]
fn displays_keep_their_panels_as_others_come_and_go() {
    let some = |id: &str| Some(id.to_owned());
    let none = || None::<String>;
    let start = assign_panels(&[none(), none(), none(), none()], &["main", "side"]);
    assert_eq!(start, [some("main"), some("side"), none(), none()]);
    // The main display goes away: its panel is freed, the other keeps its own.
    let unplugged = assign_panels(&start, &["side"]);
    assert_eq!(unplugged, [none(), some("side"), none(), none()]);
    // A display plugged in takes the free panel, never one in use.
    let plugged = assign_panels(&unplugged, &["side", "new"]);
    assert_eq!(plugged, [some("new"), some("side"), none(), none()]);
    // More displays than panels: the extras go without.
    let crowded = assign_panels(&plugged, &["a", "b", "c", "d", "e"]);
    assert_eq!(crowded, [some("a"), some("b"), some("c"), some("d")]);
}

#[test]
fn hovering_holds_the_hud_open_and_resumes_dismiss_on_leave() {
    let mut flow = HudFlow::new(true);
    arrived(&mut flow, row(1, None), 0);

    assert!(flow.set_hovered(true, 400));
    assert!(flow.is_held());
    assert_eq!(flow.remaining_ms(400), None);
    // Past the original hold deadline: stays shown while hovered.
    assert_eq!(flow.tick(HOLD_MS * 3), HudTick::Idle);
    assert!(shown(&flow));

    // Pointer leaves: resumes auto-dismiss after RESUME_HOLD_MS.
    let leave_hover = HOLD_MS * 3;
    assert!(flow.set_hovered(false, leave_hover));
    assert!(!flow.is_held());
    assert_eq!(flow.remaining_ms(leave_hover), Some(RESUME_HOLD_MS));
    assert_eq!(flow.tick(leave_hover + RESUME_HOLD_MS - 1), HudTick::Idle);
    assert_eq!(flow.tick(leave_hover + RESUME_HOLD_MS), HudTick::Leave);

    // Catching the pill mid-exit with hover restores it cleanly.
    let caught = leave_hover + RESUME_HOLD_MS + 40;
    assert!(flow.set_hovered(true, caught));
    assert!(shown(&flow));
    assert_eq!(flow.tick(caught + LEAVE_MS + 100), HudTick::Idle);

    // An open context menu keeps the pill up even after the pointer leaves the capsule.
    flow.set_menu_open(true, caught + 200);
    flow.set_hovered(false, caught + 250);
    assert!(flow.is_held());
    assert_eq!(flow.tick(caught + HOLD_MS * 2), HudTick::Idle);
    let closed = caught + HOLD_MS * 2;
    flow.set_menu_open(false, closed);
    assert!(!flow.is_held());
    assert_eq!(flow.tick(closed + RESUME_HOLD_MS), HudTick::Leave);
    assert_eq!(flow.tick(closed + RESUME_HOLD_MS + LEAVE_MS), HudTick::Gone);
}

#[test]
fn visited_empty_workspaces_stay_in_the_row_until_hud_hides() {
    let mut flow = HudFlow::new(true);
    arrived(&mut flow, row(1, None), 0);
    let numbers = |flow: &HudFlow| -> Vec<WorkspaceId> {
        flow.view(false)
            .unwrap()
            .entries()
            .iter()
            .map(|entry| entry.workspace)
            .collect()
    };
    assert_eq!(numbers(&flow), [id(1), id(2), id(3)]);

    flow.trigger(row(4, Some(1)), 80);
    assert_eq!(numbers(&flow), [id(1), id(2), id(3), id(4)]);
    // Flick to another empty workspace (5): 4 stays in the row so width doesn't jitter.
    flow.trigger(row(5, Some(4)), 160);
    assert_eq!(numbers(&flow), [id(1), id(2), id(3), id(4), id(5)]);
    // Back to occupied workspace 2: 4 and 5 stay for the remainder of this show.
    flow.trigger(row(2, Some(5)), 240);
    assert_eq!(numbers(&flow), [id(1), id(2), id(3), id(4), id(5)]);

    // Once the HUD hides and opens again, empty workspaces are cleared.
    assert_eq!(flow.tick(240 + HOLD_MS), HudTick::Leave);
    assert_eq!(flow.tick(240 + HOLD_MS + LEAVE_MS), HudTick::Gone);
    arrived(&mut flow, row(2, None), 240 + HOLD_MS + LEAVE_MS + 50);
    assert_eq!(numbers(&flow), [id(1), id(2), id(3)]);
}

#[test]
fn visited_empty_workspaces_keep_the_configured_workspace_order() {
    let named = |letter| WorkspaceId::named(letter).unwrap();
    let keyboard_row = [id(1), named('Q'), named('W'), named('E')];
    let order: Vec<WorkspaceId> = keyboard_row
        .into_iter()
        .chain(WorkspaceId::all().filter(|workspace| !keyboard_row.contains(workspace)))
        .collect();
    // As the runtime sends it: occupied 1, then the visible (empty) one, in `order`.
    let row = |visible: char, from: Option<char>| UiHudDisplay {
        id: "main".to_owned(),
        visible: Some(named(visible)),
        from: from.map(named),
        workspaces: vec![
            workspace(1, &["a"]),
            UiHudWorkspace {
                workspace: named(visible),
                fullscreen: false,
                tiles: Vec::new(),
                floating: Vec::new(),
            },
        ],
        order: order.clone(),
        portrait: false,
    };
    let mut flow = HudFlow::new(true);
    arrived(&mut flow, row('Q', None), 0);

    // Flick across the empty Q, W and E: the ones left stay where `order` puts them.
    flow.trigger(row('W', Some('Q')), 80);
    flow.trigger(row('E', Some('W')), 160);
    let entries: Vec<_> = flow
        .view(false)
        .unwrap()
        .entries()
        .iter()
        .map(|entry| entry.workspace)
        .collect();
    assert_eq!(entries, keyboard_row);
}

#[test]
fn app_context_menu_names_route_rules_always_open_and_keeps_pin_for_the_pin_action() {
    let ws2 = id(2);
    let ws4 = id(4);
    let title = |menu: &rune::PopupMenu, item: &str| menu.find(item).unwrap().title().to_owned();

    // No rule: Float, Pin on all workspaces, then where new windows open.
    let default_menu = menu::build_app_menu("Safari", ws2, None, false);
    assert_eq!(title(&default_menu, menu::ID_TOGGLE_FLOAT), "Float Safari");
    assert_eq!(
        title(&default_menu, menu::ID_TOGGLE_PIN),
        "Pin Safari on all workspaces"
    );
    assert_eq!(
        title(&default_menu, menu::ID_TOGGLE_ROUTE),
        "Always open Safari on Workspace 2"
    );
    assert!(
        !default_menu
            .find(menu::ID_TOGGLE_ROUTE)
            .unwrap()
            .is_checked()
    );
    assert!(default_menu.find(menu::ID_CLEAR_ROUTE).is_none());
    let pick = |item, current| menu::resolve_app_menu_pick(item, ws2, current, false);
    assert_eq!(pick(menu::ID_TOGGLE_FLOAT, None), Some(Some(Place::Float)));
    // "Pin" writes the pin action, never a route.
    assert_eq!(pick(menu::ID_TOGGLE_PIN, None), Some(Some(Place::Pin)));
    assert_eq!(
        pick(menu::ID_TOGGLE_ROUTE, None),
        Some(Some(Place::Workspace(ws2)))
    );
    assert_eq!(pick("unknown", None), None);

    // No item of a route rule's menu says "Pin … Workspace".
    for current in [
        None,
        Some(Place::Workspace(ws2)),
        Some(Place::Workspace(ws4)),
    ] {
        let menu = menu::build_app_menu("Safari", ws2, current, false);
        for item in [menu::ID_TOGGLE_ROUTE, menu::ID_CLEAR_ROUTE] {
            if let Some(item) = menu.find(item) {
                assert!(item.title().starts_with("Always open Safari on Workspace "));
            }
        }
        assert_eq!(
            title(&menu, menu::ID_TOGGLE_PIN),
            "Pin Safari on all workspaces"
        );
    }

    // Floating by rule: Tile removes the rule.
    let float_menu = menu::build_app_menu("Safari", ws2, Some(Place::Float), false);
    assert_eq!(title(&float_menu, menu::ID_TOGGLE_FLOAT), "Tile Safari");
    assert_eq!(pick(menu::ID_TOGGLE_FLOAT, Some(Place::Float)), Some(None));

    // Pinned: Unpin leaves it floating, Tile removes the rule.
    let pinned_menu = menu::build_app_menu("Safari", ws2, Some(Place::Pin), false);
    assert_eq!(title(&pinned_menu, menu::ID_TOGGLE_PIN), "Unpin Safari");
    assert_eq!(title(&pinned_menu, menu::ID_TOGGLE_FLOAT), "Tile Safari");
    assert_eq!(
        pick(menu::ID_TOGGLE_PIN, Some(Place::Pin)),
        Some(Some(Place::Float))
    );
    assert_eq!(pick(menu::ID_TOGGLE_FLOAT, Some(Place::Pin)), Some(None));

    // Opening here: the item is checked, and picking it removes the rule.
    let here = Some(Place::Workspace(ws2));
    let here_menu = menu::build_app_menu("Safari", ws2, here, false);
    assert!(here_menu.find(menu::ID_TOGGLE_ROUTE).unwrap().is_checked());
    assert!(here_menu.find(menu::ID_CLEAR_ROUTE).is_none());
    assert_eq!(pick(menu::ID_TOGGLE_ROUTE, here), Some(None));

    // Opening on another workspace: move it here, or pick the checked one to remove it.
    let other = Some(Place::Workspace(ws4));
    let other_menu = menu::build_app_menu("Safari", ws2, other, false);
    assert!(!other_menu.find(menu::ID_TOGGLE_ROUTE).unwrap().is_checked());
    assert_eq!(
        title(&other_menu, menu::ID_CLEAR_ROUTE),
        "Always open Safari on Workspace 4"
    );
    assert!(other_menu.find(menu::ID_CLEAR_ROUTE).unwrap().is_checked());
    assert_eq!(
        pick(menu::ID_TOGGLE_ROUTE, other),
        Some(Some(Place::Workspace(ws2)))
    );
    assert_eq!(pick(menu::ID_CLEAR_ROUTE, other), Some(None));
}

#[test]
fn app_context_menu_respects_the_built_in_float_defaults() {
    let ws1 = id(1);
    let stickies = "com.apple.Stickies";
    assert!(spaceman::defaults::floats_by_default(stickies));
    let title = |current| {
        menu::build_app_menu("Stickies", ws1, current, true)
            .find(menu::ID_TOGGLE_FLOAT)
            .unwrap()
            .title()
            .to_owned()
    };
    let pick = |item, current| menu::resolve_app_menu_pick(item, ws1, current, true);

    // It floats with no rule, so the menu offers Tile, and Tile writes the override.
    assert_eq!(title(None), "Tile Stickies");
    assert_eq!(pick(menu::ID_TOGGLE_FLOAT, None), Some(Some(Place::Tile)));
    // Tiled by that override: Float takes the rule away again instead of adding one.
    assert_eq!(title(Some(Place::Tile)), "Float Stickies");
    assert_eq!(pick(menu::ID_TOGGLE_FLOAT, Some(Place::Tile)), Some(None));
    // Pinned, then unpinned: back to the built-in float, with no rule left behind.
    assert_eq!(pick(menu::ID_TOGGLE_PIN, None), Some(Some(Place::Pin)));
    assert_eq!(pick(menu::ID_TOGGLE_PIN, Some(Place::Pin)), Some(None));
    assert_eq!(
        pick(menu::ID_TOGGLE_FLOAT, Some(Place::Pin)),
        Some(Some(Place::Tile))
    );

    // The picks write rules Spaceman's own parser reads back the same way.
    let mut doc = crate::rules::Doc::parse("version = 1\n");
    doc.set_app_place(stickies, pick(menu::ID_TOGGLE_FLOAT, None).unwrap());
    assert!(
        doc.source().contains("action = \"tile\""),
        "{}",
        doc.source()
    );
    spaceman::config::Config::parse(&doc.source()).unwrap();
    doc.set_app_place(
        stickies,
        pick(menu::ID_TOGGLE_PIN, Some(Place::Tile)).unwrap(),
    );
    assert!(
        doc.source().contains("action = \"pin\""),
        "{}",
        doc.source()
    );
    assert!(!doc.source().contains("workspace"), "{}", doc.source());
    spaceman::config::Config::parse(&doc.source()).unwrap();
}

// #419: an empty workspace moved to another display leaves this display's row.
fn empty_display(id_: &str, visible: u8, from: Option<u8>) -> UiHudDisplay {
    UiHudDisplay {
        id: id_.into(),
        visible: Some(id(visible)),
        from: from.map(id),
        workspaces: vec![workspace(visible, &[])],
        order: WorkspaceId::all().collect(),
        portrait: false,
    }
}

fn rows(flows: &[HudFlow; 3]) -> [Vec<WorkspaceId>; 3] {
    flows.each_ref().map(|flow| {
        flow.view(false)
            .unwrap()
            .entries()
            .iter()
            .map(|entry| entry.workspace)
            .collect()
    })
}

#[test]
fn moving_an_empty_workspace_away_retires_its_slot_on_the_old_display() {
    let mut flows = [HudFlow::new(true), HudFlow::new(true), HudFlow::new(true)];
    for (flow, row) in flows.iter_mut().zip([
        empty_display("main-id", 2, Some(1)),
        empty_display("secondary-id", 6, None),
        empty_display("third-id", 9, None),
    ]) {
        arrived(flow, row, 0);
    }

    let mut control = flows.clone();
    control[0].trigger(empty_display("main-id", 1, Some(2)), 100);
    assert_eq!(
        rows(&control),
        [vec![id(1), id(2)], vec![id(6)], vec![id(9)]]
    );

    for (flow, row) in flows.iter_mut().zip([
        empty_display("main-id", 1, None),
        empty_display("secondary-id", 2, Some(6)),
        empty_display("third-id", 9, None),
    ]) {
        assert_eq!(flow.trigger(row, 100), HudTransition::Update);
    }
    let observed = rows(&flows);
    println!("rows after moving workspace 2: {observed:?}");
    assert_eq!(observed, [vec![id(1)], vec![id(2), id(6)], vec![id(9)]]);
}

#[test]
fn transient_status_toast_shows_even_when_workspace_hud_is_off_and_holds_for_toast_duration() {
    use spaceman::app::UiHudToast;

    let mut flow = HudFlow::new(false);
    // Normal workspace HUD events stay ignored while HUD is off.
    assert_eq!(flow.trigger(row(1, None), 1_000), HudTransition::Ignored);
    assert!(flow.view(false).is_none());
    assert!(flow.toast_view(false).is_none());

    // A status toast still shows cleanly when Workspace HUD is off.
    let paused = UiHudToast::paused("Tiling paused — built-in display");
    assert_eq!(
        flow.trigger_toast(paused.clone(), 1_000),
        HudTransition::Show
    );
    assert!(flow.view(false).is_none());
    let view = flow.toast_view(false).expect("toast view");
    assert!(view.shown);
    assert_eq!(view.toast, &paused);
    assert_eq!(flow.remaining_ms(1_000), Some(TOAST_HOLD_MS));

    // Re-affirming HUD off does not prematurely dismiss an active status toast.
    assert!(!flow.set_enabled(false, 1_500));
    assert!(flow.toast_view(false).is_some_and(|v| v.shown));

    // Still shown before TOAST_HOLD_MS, then leaves and hides cleanly.
    assert_eq!(flow.tick(1_000 + TOAST_HOLD_MS - 1), HudTick::Idle);
    assert_eq!(flow.tick(1_000 + TOAST_HOLD_MS), HudTick::Leave);
    assert!(flow.toast_view(false).is_some_and(|v| !v.shown));
    assert_eq!(flow.tick(1_000 + TOAST_HOLD_MS + LEAVE_MS), HudTick::Gone);
    assert!(!flow.is_up());

    // A resumed toast also renders and replaces an active toast in place.
    let resumed = UiHudToast::resumed("Tiling resumed — external display");
    assert_eq!(flow.trigger_toast(paused, 5_000), HudTransition::Show);
    assert_eq!(
        flow.trigger_toast(resumed.clone(), 5_200),
        HudTransition::Update
    );
    assert_eq!(flow.toast_view(false).unwrap().toast, &resumed);
}

#[test]
fn a_capsule_kept_on_screen_never_leaves_and_comes_back_after_a_toast() {
    let mut flow = HudFlow::new(true);
    flow.set_stay(true, 0);
    assert_eq!(flow.trigger(row(1, None), 0), HudTransition::Show);
    // Long past the hold, it is still up.
    for now in [HOLD_MS, HOLD_MS * 10, 60_000] {
        assert_eq!(flow.tick(now), HudTick::Idle, "{now}");
        assert!(shown(&flow));
    }
    // A toast takes its place and leaves as toasts do; then the row is there to show again.
    flow.trigger_toast(spaceman::app::UiHudToast::paused("Paused"), 60_000);
    assert!(flow.resume_row().is_none(), "not while the toast is up");
    let mut now = 60_000;
    while flow.is_up() {
        now += 50;
        flow.tick(now);
        assert!(now < 70_000, "the toast never left");
    }
    assert_eq!(flow.resume_row(), Some(row(1, None)));
    // Let go, it leaves on its own again.
    assert_eq!(flow.trigger(row(1, None), now), HudTransition::Show);
    flow.set_stay(false, now);
    assert!(flow.resume_row().is_none());
    let mut later = now;
    while flow.is_up() {
        later += 50;
        flow.tick(later);
        assert!(later < now + 10_000, "it never left");
    }
}
