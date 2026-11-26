use std::sync::Arc;

use super::*;
use crate::{AppInstanceId, ObservedApplication, ObservedWindow, PhysicalWindowId, Rect};

const LAPTOP: i32 = 1512;
const WIDE: i32 = 2560;

fn lane(width: i32, workspaces: std::ops::RangeInclusive<u8>) -> Lane {
    Lane {
        width,
        frame: Rect {
            width,
            height: 900,
            ..Rect::default()
        },
        name: String::new(),
        workspaces: workspaces
            .map(|number| WorkspaceId::new(number).unwrap())
            .collect(),
    }
}

fn window() -> OpenWindow {
    OpenWindow {
        resizable: true,
        width: 1200,
        height: 800,
    }
}

fn fixed(width: i32, height: i32) -> OpenWindow {
    OpenWindow {
        resizable: false,
        width,
        height,
    }
}

fn app(bundle_id: &str, windows: usize) -> OpenApp {
    OpenApp {
        bundle_id: bundle_id.into(),
        category: None,
        windows: vec![window(); windows],
    }
}

fn single(width: i32, apps: Vec<OpenApp>) -> Inventory {
    Inventory {
        apps,
        lanes: vec![lane(width, 1..=10)],
    }
}

fn dual(apps: Vec<OpenApp>) -> Inventory {
    Inventory {
        apps,
        lanes: vec![lane(LAPTOP, 1..=4), lane(WIDE, 5..=10)],
    }
}

fn ws(number: u8) -> WorkspaceId {
    WorkspaceId::new(number).unwrap()
}

/// Each shown workspace's tiles, by number.
fn tiles(plan: &Plan) -> Vec<(u8, Vec<&str>)> {
    plan.workspaces
        .iter()
        .map(|entry| {
            (
                entry.workspace.number().unwrap(),
                entry.tiles.iter().map(String::as_str).collect(),
            )
        })
        .collect()
}

#[test]
fn well_known_apps_have_roles_whatever_their_case() {
    assert_eq!(role("com.apple.Safari", None), Role::Web);
    assert_eq!(role("COM.GOOGLE.CHROME", None), Role::Web);
    assert_eq!(role("com.mitchellh.ghostty", None), Role::Code);
    assert_eq!(role("com.jetbrains.intellij", None), Role::Code);
    assert_eq!(role("com.tinyspeck.slackmacgap", None), Role::Chat);
    assert_eq!(role("com.apple.mail", None), Role::Chat);
    assert_eq!(role("com.figma.Desktop", None), Role::Docs);
    assert_eq!(role("com.spotify.client", None), Role::Media);
    assert_eq!(role("org.example.unknown", None), Role::Other);
    // A prefix is a prefix, not a substring.
    assert_eq!(role("org.com.jetbrains.fake", None), Role::Other);
}

#[test]
fn an_unknown_app_falls_back_to_its_launch_services_category() {
    let category = |kind: &str| {
        let full = format!("public.app-category.{kind}");
        role("org.example.unknown", Some(&full))
    };
    assert_eq!(category("developer-tools"), Role::Code);
    assert_eq!(category("social-networking"), Role::Chat);
    assert_eq!(category("business"), Role::Chat);
    assert_eq!(category("productivity"), Role::Docs);
    assert_eq!(category("graphics-design"), Role::Docs);
    assert_eq!(category("music"), Role::Media);
    assert_eq!(category("video"), Role::Media);
    assert_eq!(category("action-games"), Role::Media);
    assert_eq!(category("utilities"), Role::Other);
    assert_eq!(role("org.example.unknown", Some("nonsense")), Role::Other);
    // The table wins over a category that disagrees with it.
    assert_eq!(
        role("com.apple.Safari", Some("public.app-category.productivity")),
        Role::Web
    );

    // The category decides the workspace: an unknown developer tool joins the terminal.
    let mut tool = app("org.example.profiler", 1);
    tool.category = Some("public.app-category.developer-tools".into());
    let plan = plan(&single(WIDE, vec![app("com.apple.Terminal", 1), tool]));
    assert_eq!(
        tiles(&plan)[1],
        (2, vec!["com.apple.Terminal", "org.example.profiler"])
    );
}

#[test]
fn one_display_gives_each_role_its_own_workspace() {
    let plan = plan(&single(
        LAPTOP,
        vec![
            app("com.spotify.client", 1),
            app("com.apple.Notes", 1),
            app("com.tinyspeck.slackmacgap", 1),
            app("com.apple.Terminal", 1),
            app("com.apple.Safari", 1),
            app("org.example.unknown", 1),
        ],
    ));
    assert_eq!(
        tiles(&plan),
        vec![
            (1, vec!["com.apple.Safari"]),
            (2, vec!["com.apple.Terminal"]),
            (3, vec!["com.tinyspeck.slackmacgap"]),
            // An app nobody knows shares the Docs workspace.
            (4, vec!["com.apple.Notes", "org.example.unknown"]),
            (5, vec!["com.spotify.client"]),
        ]
    );
    assert!(plan.floating.is_empty());
    assert!(
        plan.workspaces
            .iter()
            .all(|entry| entry.screen == Screen::MAIN)
    );
}

#[test]
fn four_browser_windows_on_a_laptop_spread_over_two_workspaces() {
    // A first-run report: four Chrome windows must not all be crammed onto workspace 1.
    let alone = plan(&single(LAPTOP, vec![app("com.google.Chrome", 4)]));
    assert_eq!(
        tiles(&alone),
        vec![
            (1, vec!["com.google.Chrome"; 2]),
            (2, vec!["com.google.Chrome"; 2]),
            (3, vec![]),
            (4, vec![]),
        ]
    );

    // With other roles at home, the rest takes the first workspace still empty.
    let busy = plan(&single(
        LAPTOP,
        vec![
            app("com.google.Chrome", 4),
            app("com.apple.Terminal", 1),
            app("com.tinyspeck.slackmacgap", 1),
        ],
    ));
    assert_eq!(
        tiles(&busy),
        vec![
            (1, vec!["com.google.Chrome"; 2]),
            (2, vec!["com.apple.Terminal"]),
            (3, vec!["com.tinyspeck.slackmacgap"]),
            (4, vec!["com.google.Chrome"; 2]),
        ]
    );
    for entry in &busy.workspaces {
        assert!(entry.tiles.len() <= 2, "a laptop workspace holds two tiles");
    }
}

#[test]
fn a_wide_display_takes_three_tiles_and_splits_four_evenly() {
    let three = plan(&single(WIDE, vec![app("com.google.Chrome", 3)]));
    assert_eq!(tiles(&three)[0], (1, vec!["com.google.Chrome"; 3]));

    let four = plan(&single(WIDE, vec![app("com.google.Chrome", 4)]));
    assert_eq!(tiles(&four)[0], (1, vec!["com.google.Chrome"; 2]));
    assert_eq!(tiles(&four)[1], (2, vec!["com.google.Chrome"; 2]));

    assert_eq!(lane(1728, 1..=5).cap(), 2);
    assert_eq!(lane(1729, 1..=5).cap(), 3);
    assert_eq!(chunk_sizes(7, 2), vec![2, 2, 2, 1]);
    assert_eq!(chunk_sizes(5, 3), vec![3, 2]);
    assert_eq!(chunk_sizes(0, 3), Vec::<usize>::new());
}

#[test]
fn utilities_and_fixed_size_windows_float_beside_the_tiles() {
    // A first-run report: tiled apps and floating utilities open together.
    let inventory = single(
        LAPTOP,
        vec![
            app("com.google.Chrome", 1),
            OpenApp {
                bundle_id: "com.apple.calculator".into(),
                category: None,
                windows: vec![fixed(230, 408)],
            },
            // Resizable, and still a utility.
            app("com.1password.1password", 1),
            app("com.apple.Stickies", 3),
            app("anomaly.hex", 1),
            // One real window and one fixed-size panel: only the window is a tile.
            OpenApp {
                bundle_id: "com.apple.Terminal".into(),
                category: None,
                windows: vec![window(), fixed(600, 400)],
            },
            // An unknown app whose only window can't be resized.
            OpenApp {
                bundle_id: "org.example.panel".into(),
                category: None,
                windows: vec![fixed(900, 700)],
            },
        ],
    );
    let plan = plan(&inventory);
    assert_eq!(
        plan.floating,
        vec![
            "com.apple.calculator",
            "com.1password.1password",
            "com.apple.Stickies",
            "anomaly.hex",
            "org.example.panel",
        ]
    );
    assert_eq!(tiles(&plan)[0], (1, vec!["com.google.Chrome"]));
    assert_eq!(tiles(&plan)[1], (2, vec!["com.apple.Terminal"]));
    assert_eq!(plan.tile_count(), 2);
    // Only a floating app Fractile would otherwise tile needs a rule: these utilities float by
    // default, and the panel can't be resized.
    assert!(plan.float_rules(&inventory).is_empty());
    assert!(plan.tile_rules(&inventory).is_empty());
}

#[test]
fn a_small_window_floats_only_for_an_app_nobody_knows() {
    let small = OpenWindow {
        resizable: true,
        width: 320,
        height: 240,
    };
    let unknown = OpenApp {
        bundle_id: "org.example.widget".into(),
        category: Some("public.app-category.utilities".into()),
        windows: vec![small],
    };
    let terminal = OpenApp {
        bundle_id: "com.apple.Terminal".into(),
        category: None,
        windows: vec![small],
    };
    assert!(floats(&unknown));
    assert!(!floats(&terminal));
    // One side small is a narrow window, not a utility.
    let narrow = OpenApp {
        bundle_id: "org.example.sidebar".into(),
        category: None,
        windows: vec![OpenWindow {
            resizable: true,
            width: 320,
            height: 900,
        }],
    };
    assert!(!floats(&narrow));
}

#[test]
fn two_displays_keep_web_and_code_on_main_and_the_rest_on_secondary() {
    let plan = plan(&dual(vec![
        app("com.apple.Safari", 1),
        app("com.apple.Terminal", 1),
        app("com.tinyspeck.slackmacgap", 1),
        app("com.apple.mail", 1),
        app("com.apple.Notes", 1),
        app("com.spotify.client", 1),
    ]));
    assert_eq!(
        tiles(&plan),
        vec![
            (1, vec!["com.apple.Safari"]),
            (2, vec!["com.apple.Terminal"]),
            (5, vec!["com.tinyspeck.slackmacgap", "com.apple.mail"]),
            (6, vec!["com.apple.Notes"]),
            (7, vec!["com.spotify.client"]),
        ]
    );
    let screens: Vec<_> = plan.workspaces.iter().map(|entry| entry.screen).collect();
    assert_eq!(
        screens,
        vec![Screen::MAIN, Screen::MAIN, Screen(1), Screen(1), Screen(1),]
    );

    // With little open, each display still shows its first two workspaces.
    let quiet = super::plan(&dual(vec![app("com.apple.Safari", 2)]));
    assert_eq!(
        tiles(&quiet)
            .iter()
            .map(|(number, _)| *number)
            .collect::<Vec<_>>(),
        vec![1, 2, 5, 6]
    );

    // A browser's overflow stays on its own display while there is room.
    let many = super::plan(&dual(vec![
        app("com.google.Chrome", 4),
        app("com.tinyspeck.slackmacgap", 1),
    ]));
    assert_eq!(tiles(&many)[0], (1, vec!["com.google.Chrome"; 2]));
    assert_eq!(tiles(&many)[1], (2, vec!["com.google.Chrome"; 2]));
}

#[test]
fn every_tile_is_placed_even_when_every_workspace_is_taken() {
    let inventory = Inventory {
        apps: vec![
            app("com.google.Chrome", 9),
            app("com.apple.Terminal", 2),
            app("com.tinyspeck.slackmacgap", 1),
        ],
        lanes: vec![lane(LAPTOP, 1..=3)],
    };
    let plan = plan(&inventory);
    assert_eq!(plan.tile_count(), 12);
    assert_eq!(plan.workspaces.len(), 3);
}

#[test]
fn a_display_without_workspaces_plans_nothing_to_tile() {
    let inventory = Inventory {
        apps: vec![app("com.apple.Safari", 2), app("com.apple.Stickies", 1)],
        lanes: vec![Lane::default()],
    };
    let plan = plan(&inventory);
    assert!(plan.workspaces.is_empty());
    assert_eq!(plan.floating, vec!["com.apple.Stickies"]);
    assert!(plan.is_trivial());
}

#[test]
fn one_window_or_none_is_nothing_to_arrange() {
    assert!(plan(&single(LAPTOP, Vec::new())).is_trivial());
    assert!(plan(&single(LAPTOP, vec![app("com.apple.Safari", 1)])).is_trivial());
    // Floating utilities alone don't make an arrangement.
    assert!(
        plan(&single(
            LAPTOP,
            vec![app("com.apple.Safari", 1), app("com.apple.Stickies", 4)]
        ))
        .is_trivial()
    );
    assert!(!plan(&single(LAPTOP, vec![app("com.apple.Safari", 2)])).is_trivial());
}

#[test]
fn a_tile_moves_to_another_workspace_by_drag_or_click() {
    let inventory = single(
        LAPTOP,
        vec![app("com.apple.Safari", 2), app("com.apple.Terminal", 1)],
    );
    let mut plan = plan(&inventory);
    assert!(plan.move_tile(ws(1), 1, ws(2)));
    assert_eq!(tiles(&plan)[0], (1, vec!["com.apple.Safari"]));
    assert_eq!(
        tiles(&plan)[1],
        (2, vec!["com.apple.Terminal", "com.apple.Safari"])
    );
    // Nothing moves onto the same workspace, from an empty place or to one not shown.
    assert!(!plan.move_tile(ws(1), 0, ws(1)));
    assert!(!plan.move_tile(ws(3), 0, ws(1)));
    assert!(!plan.move_tile(ws(1), 0, ws(9)));
    assert_eq!(plan.tile_count(), 3);

    // A click sends a tile to the next workspace shown, and wraps.
    assert_eq!(plan.next_workspace(ws(1)), Some(ws(2)));
    assert_eq!(plan.next_workspace(ws(4)), Some(ws(1)));
    assert_eq!(plan.next_workspace(ws(9)), None);
}

#[test]
fn floating_toggles_both_ways_and_tiling_returns_to_the_roles_workspace() {
    let inventory = single(
        LAPTOP,
        vec![
            app("com.apple.Safari", 1),
            app("com.apple.Notes", 2),
            app("com.apple.Stickies", 2),
        ],
    );
    let mut plan = plan(&inventory);
    assert_eq!(plan.floating, vec!["com.apple.Stickies"]);

    plan.set_floating("com.apple.Notes", true, &inventory);
    assert_eq!(plan.floating, vec!["com.apple.Stickies", "com.apple.Notes"]);
    assert_eq!(tiles(&plan)[3], (4, vec![]));
    // Stickies floats by default; only Notes needs a rule.
    assert_eq!(plan.float_rules(&inventory), vec!["com.apple.Notes"]);

    plan.set_floating("com.apple.Notes", false, &inventory);
    assert_eq!(plan.floating, vec!["com.apple.Stickies"]);
    assert_eq!(tiles(&plan)[3], (4, vec!["com.apple.Notes"; 2]));

    // A utility can be tiled if that is what someone wants: it joins the Docs workspace.
    plan.set_floating("com.apple.Stickies", false, &inventory);
    assert!(plan.floating.is_empty());
    assert!(plan.float_rules(&inventory).is_empty());
    // It floats by default, so tiling it takes a tile rule.
    assert_eq!(plan.tile_rules(&inventory), vec!["com.apple.Stickies"]);
    assert_eq!(plan.workspaces[3].tiles.len(), 4);

    // Asking for what is already so, or for an app that isn't open, changes nothing.
    let before = plan.clone();
    plan.set_floating("com.apple.Safari", false, &inventory);
    plan.set_floating("org.example.closed", false, &inventory);
    assert_eq!(plan, before);
}

#[test]
fn the_arrangement_lists_each_apps_windows_in_plan_order_and_never_a_route_rule() {
    let inventory = single(
        LAPTOP,
        vec![
            app("com.google.Chrome", 4),
            app("com.apple.Terminal", 1),
            app("com.apple.Stickies", 2),
        ],
    );
    let plan = plan(&inventory);
    let arrangement = plan.arrangement();
    assert_eq!(
        arrangement.apps,
        vec![
            ArrangedApp {
                bundle_id: "com.google.Chrome".into(),
                slots: vec![
                    ArrangedSlot::Tile(ws(1)),
                    ArrangedSlot::Tile(ws(1)),
                    ArrangedSlot::Tile(ws(3)),
                    ArrangedSlot::Tile(ws(3)),
                ],
            },
            ArrangedApp {
                bundle_id: "com.apple.Terminal".into(),
                slots: vec![ArrangedSlot::Tile(ws(2))],
            },
            ArrangedApp {
                bundle_id: "com.apple.Stickies".into(),
                slots: vec![ArrangedSlot::Float],
            },
        ]
    );
    // The plan moves open windows and asks for no rule here: Stickies floats by default, and a
    // browser or a terminal is never pinned to a workspace.
    assert!(plan.float_rules(&inventory).is_empty());
    assert!(plan.tile_rules(&inventory).is_empty());
}

#[test]
fn nothing_built_from_an_observation_carries_a_window_title() {
    const TITLE: &str = "Quarterly numbers, do not share";
    let observed = |pid: i32, bundle_id: &str, windows: u32| AppObservation::Complete {
        app: ObservedApplication {
            id: AppInstanceId {
                pid,
                launched_at: 1,
            },
            bundle_id: bundle_id.into(),
        },
        focused: None,
        windows: (0..windows)
            .map(|index| ObservedWindow {
                id: PhysicalWindowId {
                    app: AppInstanceId {
                        pid,
                        launched_at: 1,
                    },
                    window_id: index + 1,
                },
                frame: Rect {
                    x: 10,
                    y: 20,
                    width: 1100,
                    height: 700,
                },
                resizable: true,
                title: Some(Arc::from(TITLE)),
                document: None,
            })
            .collect(),
    };
    let observation = NativeObservation {
        cycle: 0,
        mouse_down: false,
        applications: vec![
            observed(10, "com.google.Chrome", 2),
            // A second instance of the same app merges into the first.
            observed(11, "com.google.Chrome", 1),
            observed(12, "com.apple.Stickies", 1),
            // An app with no windows isn't open as far as a plan goes.
            observed(13, "com.apple.Notes", 0),
            AppObservation::Unavailable {
                app: AppInstanceId {
                    pid: 14,
                    launched_at: 1,
                },
                error: "busy".into(),
            },
        ],
    };
    let inventory = Inventory::from_observation(&observation, vec![lane(LAPTOP, 1..=10)]);
    assert_eq!(inventory.apps.len(), 2);
    assert_eq!(inventory.apps[0].windows.len(), 3);
    assert_eq!(
        inventory.apps[0].windows[0],
        OpenWindow {
            resizable: true,
            width: 1100,
            height: 700,
        }
    );

    let plan = plan(&inventory);
    let arrangement = plan.arrangement();
    for text in [
        format!("{inventory:?}"),
        format!("{inventory:#?}"),
        format!("{plan:?}"),
        format!("{arrangement:?}"),
        format!("{:?}", plan.float_rules(&inventory)),
    ] {
        assert!(!text.contains(TITLE), "a title leaked into {text}");
        assert!(!text.contains("title"), "a title field in {text}");
        assert!(!text.contains("Quarterly"), "part of a title in {text}");
    }
}

#[test]
fn dwindle_halves_the_last_tiles_space_across_its_longer_side() {
    assert!(dwindle(0).is_empty());
    assert_eq!(dwindle(1), vec![[0.0, 0.0, 1.0, 1.0]]);
    assert_eq!(dwindle(2), vec![[0.0, 0.0, 0.5, 1.0], [0.5, 0.0, 0.5, 1.0]]);
    assert_eq!(
        dwindle(3),
        vec![
            [0.0, 0.0, 0.5, 1.0],
            [0.5, 0.0, 0.5, 0.5],
            [0.5, 0.5, 0.5, 0.5]
        ]
    );
    let four = dwindle(4);
    assert_eq!(four[2], [0.5, 0.5, 0.25, 0.5]);
    assert_eq!(four[3], [0.75, 0.5, 0.25, 0.5]);
    // The tiles cover the workspace exactly.
    for count in 1..=6 {
        let area: f32 = dwindle(count).iter().map(|frame| frame[2] * frame[3]).sum();
        assert!((area - 1.0).abs() < 1e-5, "{count} tiles cover {area}");
    }
}

#[test]
fn three_displays_each_get_their_share() {
    let inventory = Inventory {
        apps: vec![
            app("com.apple.Safari", 1),
            app("com.apple.Terminal", 1),
            app("com.tinyspeck.slackmacgap", 1),
            app("com.apple.Notes", 1),
            app("com.spotify.client", 1),
        ],
        lanes: vec![lane(LAPTOP, 1..=4), lane(WIDE, 5..=7), lane(WIDE, 8..=10)],
    };
    let plan = plan(&inventory);
    let on = |screen: usize| {
        plan.workspaces
            .iter()
            .filter(|entry| entry.screen == Screen(screen))
            .flat_map(|entry| entry.tiles.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(on(0), vec!["com.apple.Safari", "com.apple.Terminal"]);
    assert_eq!(on(1), vec!["com.tinyspeck.slackmacgap", "com.apple.Notes"]);
    assert_eq!(on(2), vec!["com.spotify.client"]);
    // Every display shows at least its first two workspaces.
    for screen in 0..3 {
        assert!(
            plan.workspaces
                .iter()
                .filter(|entry| entry.screen == Screen(screen))
                .count()
                >= 2
        );
    }
}
