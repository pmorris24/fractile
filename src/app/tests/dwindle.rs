use super::*;

fn rect(x: i32, y: i32, width: i32, height: i32) -> crate::Rect {
    crate::Rect {
        x,
        y,
        width,
        height,
    }
}

fn landscape_outputs() -> macos::DisplayOutputs {
    macos::DisplayOutputs::new(vec![macos::DisplayOutput {
        id: "main-id".into(),
        name: "Main".into(),
        frame: rect(0, 0, 1280, 800),
        is_main: true,
    }])
    .unwrap()
}

fn portrait_outputs() -> macos::DisplayOutputs {
    macos::DisplayOutputs::new(vec![macos::DisplayOutput {
        id: "portrait-id".into(),
        name: "Portrait".into(),
        frame: rect(0, 0, 1080, 1920),
        is_main: true,
    }])
    .unwrap()
}

fn columns_config() -> Config {
    Config::parse("version = 1\n[settings]\nlayout = \"columns\"\n").unwrap()
}

#[test]
fn dwindle_default_tiles_one_to_four_windows_on_landscape_and_re_expands_on_close() {
    let w1 = window(10);
    let w2 = window(11);
    let w3 = window(12);
    let w4 = window(13);
    let mut backend = FakeBackend::default();
    backend.frames.insert(w1.clone(), frame(50));
    backend.observation = Some(observation_for_windows(
        "com.example.Test",
        &w1,
        &[(w1.clone(), frame(50))],
    ));
    let mut rt = runtime(backend);
    let outputs = landscape_outputs();
    let cancelled = AtomicBool::new(false);

    // 1 window -> 100%.
    rt.cycle(outputs.clone(), &cancelled).unwrap();
    assert_eq!(rt.backend.frames[&w1], rect(0, 0, 1280, 800));

    // 2 windows -> 50% | 50%.
    rt.backend.frames.insert(w2.clone(), frame(100));
    rt.backend.observation = Some(observation_for_windows(
        "com.example.Test",
        &w2,
        &[(w1.clone(), frame(50)), (w2.clone(), frame(100))],
    ));
    rt.cycle(outputs.clone(), &cancelled).unwrap();
    assert_eq!(rt.backend.frames[&w1], rect(0, 0, 640, 800));
    assert_eq!(rt.backend.frames[&w2], rect(640, 0, 640, 800));

    // 3 windows -> [ w1 | (w2 / w3) ].
    rt.backend.frames.insert(w3.clone(), frame(150));
    rt.backend.observation = Some(observation_for_windows(
        "com.example.Test",
        &w3,
        &[
            (w1.clone(), frame(50)),
            (w2.clone(), frame(100)),
            (w3.clone(), frame(150)),
        ],
    ));
    rt.cycle(outputs.clone(), &cancelled).unwrap();
    assert_eq!(rt.backend.frames[&w1], rect(0, 0, 640, 800));
    assert_eq!(rt.backend.frames[&w2], rect(640, 0, 640, 400));
    assert_eq!(rt.backend.frames[&w3], rect(640, 400, 640, 400));

    // 4 windows -> [ w1 | (w2 / (w3 | w4)) ].
    rt.backend.frames.insert(w4.clone(), frame(200));
    rt.backend.observation = Some(observation_for_windows(
        "com.example.Test",
        &w4,
        &[
            (w1.clone(), frame(50)),
            (w2.clone(), frame(100)),
            (w3.clone(), frame(150)),
            (w4.clone(), frame(200)),
        ],
    ));
    rt.cycle(outputs.clone(), &cancelled).unwrap();
    assert_eq!(rt.backend.frames[&w1], rect(0, 0, 640, 800));
    assert_eq!(rt.backend.frames[&w2], rect(640, 0, 640, 400));
    assert_eq!(rt.backend.frames[&w3], rect(640, 400, 320, 400));
    assert_eq!(rt.backend.frames[&w4], rect(960, 400, 320, 400));

    // Close w4 -> w3 re-expands to the full bottom-right cell.
    rt.backend.frames.remove(&w4);
    rt.backend.observation = Some(observation_for_windows(
        "com.example.Test",
        &w3,
        &[
            (w1.clone(), frame(50)),
            (w2.clone(), frame(100)),
            (w3.clone(), frame(150)),
        ],
    ));
    rt.cycle(outputs.clone(), &cancelled).unwrap();
    assert_eq!(rt.backend.frames[&w3], rect(640, 400, 640, 400));

    // Close w1 -> surviving auto-chosen (w2 / w3) expands into the landscape root
    // and re-orients to two columns (w2 | w3) (#342).
    rt.backend.frames.remove(&w1);
    rt.backend.observation = Some(observation_for_windows(
        "com.example.Test",
        &w2,
        &[(w2.clone(), frame(100)), (w3.clone(), frame(150))],
    ));
    rt.cycle(outputs, &cancelled).unwrap();
    assert_eq!(rt.backend.frames[&w2], rect(0, 0, 640, 800));
    assert_eq!(rt.backend.frames[&w3], rect(640, 0, 640, 800));
    assert_eq!(
        rt.workspaces.engine(WorkspaceId::ONE).root_axis(),
        crate::Axis::Horizontal
    );
}

#[test]
fn dwindle_portrait_display_splits_vertically_first() {
    let w1 = window(10);
    let w2 = window(11);
    let w3 = window(12);
    let mut backend = FakeBackend::default();
    backend.frames.insert(w1.clone(), frame(50));
    backend.frames.insert(w2.clone(), frame(100));
    backend.observation = Some(observation_for_windows(
        "com.example.Test",
        &w2,
        &[(w1.clone(), frame(50)), (w2.clone(), frame(100))],
    ));
    let mut rt = runtime(backend);
    let outputs = portrait_outputs();
    let cancelled = AtomicBool::new(false);

    // 2 windows on portrait -> 50% / 50% top/bottom (#208).
    rt.cycle(outputs.clone(), &cancelled).unwrap();
    assert_eq!(rt.backend.frames[&w1], rect(0, 0, 1080, 960));
    assert_eq!(rt.backend.frames[&w2], rect(0, 960, 1080, 960));

    // 3rd window splits the bottom 1080x960 tile left/right.
    rt.backend.frames.insert(w3.clone(), frame(150));
    rt.backend.observation = Some(observation_for_windows(
        "com.example.Test",
        &w3,
        &[
            (w1.clone(), frame(50)),
            (w2.clone(), frame(100)),
            (w3.clone(), frame(150)),
        ],
    ));
    rt.cycle(outputs, &cancelled).unwrap();
    assert_eq!(rt.backend.frames[&w1], rect(0, 0, 1080, 960));
    assert_eq!(rt.backend.frames[&w2], rect(0, 960, 540, 960));
    assert_eq!(rt.backend.frames[&w3], rect(540, 960, 540, 960));
}

#[test]
fn live_config_reload_switches_between_dwindle_and_columns() {
    let w1 = window(10);
    let w2 = window(11);
    let w3 = window(12);
    let mut backend = FakeBackend::default();
    backend.frames.insert(w1.clone(), frame(50));
    backend.frames.insert(w2.clone(), frame(100));
    backend.observation = Some(observation_for_windows(
        "com.example.Test",
        &w2,
        &[(w1.clone(), frame(50)), (w2.clone(), frame(100))],
    ));
    let mut rt = runtime(backend);
    let outputs = landscape_outputs();
    let cancelled = AtomicBool::new(false);

    // Start in Dwindle with 2 windows -> [ w1 | w2 ].
    rt.cycle(outputs.clone(), &cancelled).unwrap();
    assert_eq!(rt.backend.frames[&w1], rect(0, 0, 640, 800));
    assert_eq!(rt.backend.frames[&w2], rect(640, 0, 640, 800));

    // Hot-reload to Columns without restarting -> 3rd window joins as 3 equal columns.
    rt.replace_config(columns_config());
    rt.backend.frames.insert(w3.clone(), frame(150));
    rt.backend.observation = Some(observation_for_windows(
        "com.example.Test",
        &w3,
        &[
            (w1.clone(), frame(50)),
            (w2.clone(), frame(100)),
            (w3.clone(), frame(150)),
        ],
    ));
    rt.cycle(outputs.clone(), &cancelled).unwrap();
    assert_eq!(rt.backend.frames[&w1].height, 800);
    assert_eq!(rt.backend.frames[&w2].height, 800);
    assert_eq!(rt.backend.frames[&w3].height, 800);
    assert_eq!(
        rt.backend.frames[&w1].width + rt.backend.frames[&w2].width + rt.backend.frames[&w3].width,
        1280
    );

    // Hot-reload back to Dwindle -> 4th window splits w3 vertically (427x800).
    rt.replace_config(Config::default());
    let w4 = window(13);
    rt.backend.frames.insert(w4.clone(), frame(200));
    rt.backend.observation = Some(observation_for_windows(
        "com.example.Test",
        &w4,
        &[
            (w1.clone(), frame(50)),
            (w2.clone(), frame(100)),
            (w3.clone(), frame(150)),
            (w4.clone(), frame(200)),
        ],
    ));
    rt.cycle(outputs, &cancelled).unwrap();
    assert_eq!(rt.backend.frames[&w3].height, 400);
    assert_eq!(rt.backend.frames[&w4].height, 400);
    assert_eq!(rt.backend.frames[&w4].y, 400);
}

#[test]
fn moving_windows_to_another_workspace_follows_dwindle_layout() {
    let w1 = window(10);
    let w2 = window(11);
    let w3 = window(12);
    let mut backend = FakeBackend::default();
    backend.frames.insert(w1.clone(), frame(50));
    backend.frames.insert(w2.clone(), frame(100));
    backend.frames.insert(w3.clone(), frame(150));
    let mut rt = runtime(backend);
    let outputs = landscape_outputs();
    let cancelled = AtomicBool::new(false);

    // Put w1 and w2 on workspace 2, w3 on workspace 1.
    rt.workspaces
        .engine_mut(WorkspaceId::TWO)
        .insert_slot(Slot {
            id: SlotId(1),
            binding: BindingState::Bound(w1.clone()),
        });
    rt.workspaces
        .engine_mut(WorkspaceId::TWO)
        .insert_slot(Slot {
            id: SlotId(2),
            binding: BindingState::Bound(w2.clone()),
        });
    rt.workspaces
        .engine_mut(WorkspaceId::ONE)
        .insert_slot(Slot {
            id: SlotId(3),
            binding: BindingState::Bound(w3.clone()),
        });

    rt.move_window(w3.clone(), WorkspaceId::TWO, outputs.clone(), &cancelled)
        .unwrap();
    let placements = rt
        .workspaces
        .engine(WorkspaceId::TWO)
        .placements(&Settings::for_output(outputs.main().frame));
    assert_eq!(placements.len(), 3);
    assert_eq!(placements[0].frame, rect(0, 0, 640, 800));
    assert_eq!(placements[1].frame, rect(640, 0, 640, 400));
    assert_eq!(placements[2].frame, rect(640, 400, 640, 400));
}

#[test]
fn dwindle_supports_axis_toggle_and_fullscreen_layout() {
    let w1 = window(10);
    let w2 = window(11);
    let w3 = window(12);
    let mut backend = FakeBackend {
        focused: Some(w2.clone()),
        ..FakeBackend::default()
    };
    backend.frames.insert(w1.clone(), frame(50));
    backend.frames.insert(w2.clone(), frame(100));
    backend.frames.insert(w3.clone(), frame(150));
    backend.observation = Some(observation_for_windows(
        "com.example.Test",
        &w2,
        &[
            (w1.clone(), frame(50)),
            (w2.clone(), frame(100)),
            (w3.clone(), frame(150)),
        ],
    ));
    let mut rt = runtime(backend);
    let outputs = landscape_outputs();
    let cancelled = AtomicBool::new(false);

    rt.cycle(outputs.clone(), &cancelled).unwrap();
    assert_eq!(rt.backend.frames[&w1], rect(0, 0, 640, 800));
    assert_eq!(rt.backend.frames[&w2], rect(640, 0, 640, 400));
    assert_eq!(rt.backend.frames[&w3], rect(640, 400, 640, 400));

    // Toggle root axis flips [ w1 | (w2 / w3) ] to [ w1 / (w2 / w3) ].
    rt.toggle_layout(outputs.clone(), &cancelled).unwrap();
    assert_eq!(
        rt.workspaces.engine(WorkspaceId::ONE).root_axis(),
        crate::Axis::Vertical
    );
    assert_eq!(rt.backend.frames[&w1], rect(0, 0, 1280, 400));

    // Fullscreen Layout still fills the display and restores the layout on toggle off.
    rt.toggle_fullscreen_layout(outputs.clone(), &cancelled)
        .unwrap();
    assert!(rt.workspaces.deck(WorkspaceId::ONE).is_monocle());
    assert_eq!(rt.backend.frames[&w2], rect(0, 0, 1280, 800));

    rt.toggle_fullscreen_layout(outputs, &cancelled).unwrap();
    assert!(!rt.workspaces.deck(WorkspaceId::ONE).is_monocle());
    assert_eq!(rt.backend.frames[&w1], rect(0, 0, 1280, 400));
}

fn open_dwindle_windows(
    outputs: &macos::DisplayOutputs,
    ids: &[u32],
    config: Config,
) -> (WorkspaceRuntime<FakeBackend>, Vec<crate::PhysicalWindowId>) {
    let windows: Vec<_> = ids.iter().map(|&id| window(id)).collect();
    let backend = FakeBackend {
        focused: Some(windows[0].clone()),
        ..FakeBackend::default()
    };
    let mut rt = runtime_with_config(backend, config);
    let cancelled = AtomicBool::new(false);
    let mut active = Vec::new();
    for (i, w) in windows.iter().enumerate() {
        let f = frame(50 + i as i32 * 50);
        rt.backend.frames.insert(w.clone(), f);
        active.push((w.clone(), f));
        rt.backend.focused = Some(w.clone());
        rt.backend.observation = Some(observation_for_windows("com.example.Test", w, &active));
        rt.cycle(outputs.clone(), &cancelled).unwrap();
    }
    (rt, windows)
}

fn restart_runtime(
    rt: &WorkspaceRuntime<FakeBackend>,
    focused: crate::PhysicalWindowId,
    config: Config,
) -> WorkspaceRuntime<FakeBackend> {
    let state = rt.workspaces.state();
    let json = serde_json::to_string(&state).unwrap();
    let restored_state: crate::workspace::WorkspaceState = serde_json::from_str(&json).unwrap();
    let backend = FakeBackend {
        frames: rt.backend.frames.clone(),
        focused: Some(focused),
        ..FakeBackend::default()
    };
    WorkspaceRuntime::with_backend_config_state(
        ManagementScope::Both,
        backend,
        HashMap::new(),
        config,
        Some(restored_state),
    )
}

#[test]
fn dwindle_landscape_closing_first_of_four_windows_matches_fresh_three() {
    let outputs = landscape_outputs();
    let cancelled = AtomicBool::new(false);
    let (mut rt, ws) = open_dwindle_windows(&outputs, &[10, 11, 12, 13], Config::default());

    // 4 windows: 1 | (2 / (3 | 4)).
    assert_eq!(rt.backend.frames[&ws[0]], rect(0, 0, 640, 800));
    assert_eq!(rt.backend.frames[&ws[1]], rect(640, 0, 640, 400));
    assert_eq!(rt.backend.frames[&ws[2]], rect(640, 400, 320, 400));
    assert_eq!(rt.backend.frames[&ws[3]], rect(960, 400, 320, 400));

    // Close window 1 -> surviving 2 / (3 | 4) re-orients outermost first into 2 | (3 / 4).
    rt.backend.frames.remove(&ws[0]);
    rt.backend.observation = Some(observation_for_windows(
        "com.example.Test",
        &ws[1],
        &[
            (ws[1].clone(), rt.backend.frames[&ws[1]]),
            (ws[2].clone(), rt.backend.frames[&ws[2]]),
            (ws[3].clone(), rt.backend.frames[&ws[3]]),
        ],
    ));
    rt.cycle(outputs, &cancelled).unwrap();
    assert_eq!(rt.backend.frames[&ws[1]], rect(0, 0, 640, 800));
    assert_eq!(rt.backend.frames[&ws[2]], rect(640, 0, 640, 400));
    assert_eq!(rt.backend.frames[&ws[3]], rect(640, 400, 640, 400));
}

#[test]
fn dwindle_portrait_closing_first_of_three_and_four_windows_reorients() {
    let outputs = portrait_outputs();
    let cancelled = AtomicBool::new(false);

    // 3 windows on portrait: 1 / (2 | 3). Close 1 -> 2 / 3 (two 1080x960 rows).
    let (mut rt, ws) = open_dwindle_windows(&outputs, &[10, 11, 12], Config::default());
    assert_eq!(rt.backend.frames[&ws[0]], rect(0, 0, 1080, 960));
    assert_eq!(rt.backend.frames[&ws[1]], rect(0, 960, 540, 960));
    assert_eq!(rt.backend.frames[&ws[2]], rect(540, 960, 540, 960));

    rt.backend.frames.remove(&ws[0]);
    rt.backend.observation = Some(observation_for_windows(
        "com.example.Test",
        &ws[1],
        &[
            (ws[1].clone(), rt.backend.frames[&ws[1]]),
            (ws[2].clone(), rt.backend.frames[&ws[2]]),
        ],
    ));
    rt.cycle(outputs.clone(), &cancelled).unwrap();
    assert_eq!(rt.backend.frames[&ws[1]], rect(0, 0, 1080, 960));
    assert_eq!(rt.backend.frames[&ws[2]], rect(0, 960, 1080, 960));

    // 4 windows on portrait: 1 / (2 | (3 / 4)). Close 1 -> 2 / (3 | 4).
    let (mut rt, ws) = open_dwindle_windows(&outputs, &[20, 21, 22, 23], Config::default());
    rt.backend.frames.remove(&ws[0]);
    rt.backend.observation = Some(observation_for_windows(
        "com.example.Test",
        &ws[1],
        &[
            (ws[1].clone(), rt.backend.frames[&ws[1]]),
            (ws[2].clone(), rt.backend.frames[&ws[2]]),
            (ws[3].clone(), rt.backend.frames[&ws[3]]),
        ],
    ));
    rt.cycle(outputs, &cancelled).unwrap();
    assert_eq!(rt.backend.frames[&ws[1]], rect(0, 0, 1080, 960));
    assert_eq!(rt.backend.frames[&ws[2]], rect(0, 960, 540, 960));
    assert_eq!(rt.backend.frames[&ws[3]], rect(540, 960, 540, 960));
}

#[test]
fn dwindle_reorients_on_move_to_workspace_or_display_and_on_float() {
    let cancelled = AtomicBool::new(false);

    // 1. Move first window to another workspace on the same display.
    let outputs = landscape_outputs();
    let (mut rt, ws) = open_dwindle_windows(&outputs, &[10, 11, 12], Config::default());
    rt.move_window(ws[0].clone(), WorkspaceId::TWO, outputs.clone(), &cancelled)
        .unwrap();
    assert_eq!(rt.backend.frames[&ws[1]], rect(0, 0, 640, 800));
    assert_eq!(rt.backend.frames[&ws[2]], rect(640, 0, 640, 800));

    // 2. Move first window to another display's active workspace.
    let dual = macos::DisplayOutputs::new(vec![
        macos::DisplayOutput {
            id: "main-id".into(),
            name: "Main".into(),
            frame: rect(0, 0, 1280, 800),
            is_main: true,
        },
        macos::DisplayOutput {
            id: "side-id".into(),
            name: "Side".into(),
            frame: rect(1280, 0, 1080, 1920),
            is_main: false,
        },
    ])
    .unwrap();
    let (mut rt, ws) = open_dwindle_windows(&dual, &[10, 11, 12], Config::default());
    rt.move_window(ws[0].clone(), WorkspaceId::SIX, dual, &cancelled)
        .unwrap();
    assert_eq!(rt.backend.frames[&ws[1]], rect(0, 0, 640, 800));
    assert_eq!(rt.backend.frames[&ws[2]], rect(640, 0, 640, 800));

    // 3. Float first window with Toggle Float on both landscape and portrait.
    let (mut rt, ws) = open_dwindle_windows(&outputs, &[10, 11, 12], Config::default());
    rt.backend.focused = Some(ws[0].clone());
    rt.toggle_float(outputs, &cancelled).unwrap();
    assert_eq!(rt.backend.frames[&ws[1]], rect(0, 0, 640, 800));
    assert_eq!(rt.backend.frames[&ws[2]], rect(640, 0, 640, 800));

    let portrait = portrait_outputs();
    let (mut rt, ws) = open_dwindle_windows(&portrait, &[10, 11, 12], Config::default());
    rt.backend.focused = Some(ws[0].clone());
    rt.toggle_float(portrait, &cancelled).unwrap();
    assert_eq!(rt.backend.frames[&ws[1]], rect(0, 0, 1080, 960));
    assert_eq!(rt.backend.frames[&ws[2]], rect(0, 960, 1080, 960));
}

#[test]
fn dwindle_preserves_manual_option_slash_drop_and_move_splits_across_close_and_restart() {
    let outputs = landscape_outputs();
    let cancelled = AtomicBool::new(false);

    // 1. Option+/ flips 1 | 2 -> 1 / 2 (manual); opening 3 and closing 3 keeps 1 / 2,
    // including across a restart.
    let (mut rt, ws) = open_dwindle_windows(&outputs, &[10, 11], Config::default());
    rt.backend.focused = Some(ws[0].clone());
    rt.toggle_layout(outputs.clone(), &cancelled).unwrap();
    assert_eq!(rt.backend.frames[&ws[0]], rect(0, 0, 1280, 400));
    assert_eq!(rt.backend.frames[&ws[1]], rect(0, 400, 1280, 400));

    let w3 = window(12);
    rt.backend.frames.insert(w3.clone(), frame(200));
    rt.backend.focused = Some(ws[1].clone());
    rt.backend.observation = Some(observation_for_windows(
        "com.example.Test",
        &ws[1],
        &[
            (ws[0].clone(), rt.backend.frames[&ws[0]]),
            (ws[1].clone(), rt.backend.frames[&ws[1]]),
            (w3.clone(), frame(200)),
        ],
    ));
    rt.cycle(outputs.clone(), &cancelled).unwrap();

    let mut rt = restart_runtime(&rt, ws[0].clone(), Config::default());
    rt.backend.frames.remove(&w3);
    rt.backend.observation = Some(observation_for_windows(
        "com.example.Test",
        &ws[0],
        &[
            (ws[0].clone(), rt.backend.frames[&ws[0]]),
            (ws[1].clone(), rt.backend.frames[&ws[1]]),
        ],
    ));
    rt.cycle(outputs.clone(), &cancelled).unwrap();
    assert_eq!(rt.backend.frames[&ws[0]], rect(0, 0, 1280, 400));
    assert_eq!(rt.backend.frames[&ws[1]], rect(0, 400, 1280, 400));

    // 2. Drag-drop places w3 below w2 inside 1 | (2 / 3); after restart, closing w1
    // preserves the user-dropped vertical split (2 / 3).
    let (mut rt, ws) = open_dwindle_windows(&outputs, &[10, 11, 12], Config::default());
    // Drop w3 onto w2's bottom edge -> user-set vertical split.
    rt.drop_window(
        ws[2].clone(),
        (960, 380),
        false,
        outputs.clone(),
        &cancelled,
    )
    .unwrap();
    let mut rt = restart_runtime(&rt, ws[1].clone(), Config::default());
    rt.backend.frames.remove(&ws[0]);
    rt.backend.observation = Some(observation_for_windows(
        "com.example.Test",
        &ws[1],
        &[
            (ws[1].clone(), rt.backend.frames[&ws[1]]),
            (ws[2].clone(), rt.backend.frames[&ws[2]]),
        ],
    ));
    rt.cycle(outputs.clone(), &cancelled).unwrap();
    assert_eq!(rt.backend.frames[&ws[1]], rect(0, 0, 1280, 400));
    assert_eq!(rt.backend.frames[&ws[2]], rect(0, 400, 1280, 400));

    // 3. Perpendicular reorder (Option+Shift+J) creates a manual vertical split (2 / 1);
    // opening 3 and closing 3 after restart keeps 2 / 1.
    let (mut rt, ws) = open_dwindle_windows(&outputs, &[10, 11], Config::default());
    rt.backend.focused = Some(ws[0].clone());
    rt.reorder_focused(Direction::Down, outputs.clone(), &cancelled)
        .unwrap();
    assert_eq!(rt.backend.frames[&ws[1]], rect(0, 0, 1280, 400));
    assert_eq!(rt.backend.frames[&ws[0]], rect(0, 400, 1280, 400));
    let mut rt = restart_runtime(&rt, ws[0].clone(), Config::default());
    rt.backend.frames.insert(w3.clone(), frame(200));
    rt.backend.observation = Some(observation_for_windows(
        "com.example.Test",
        &ws[0],
        &[
            (ws[0].clone(), rt.backend.frames[&ws[0]]),
            (ws[1].clone(), rt.backend.frames[&ws[1]]),
            (w3.clone(), frame(200)),
        ],
    ));
    rt.cycle(outputs.clone(), &cancelled).unwrap();
    rt.backend.frames.remove(&w3);
    rt.backend.observation = Some(observation_for_windows(
        "com.example.Test",
        &ws[0],
        &[
            (ws[0].clone(), rt.backend.frames[&ws[0]]),
            (ws[1].clone(), rt.backend.frames[&ws[1]]),
        ],
    ));
    rt.cycle(outputs, &cancelled).unwrap();
    assert_eq!(rt.backend.frames[&ws[1]], rect(0, 0, 1280, 400));
    assert_eq!(rt.backend.frames[&ws[0]], rect(0, 400, 1280, 400));
}

#[test]
fn dwindle_reorient_keeps_order_weights_and_untouched_sibling_splits() {
    let outputs = landscape_outputs();
    let cancelled = AtomicBool::new(false);
    let (mut rt, ws) = open_dwindle_windows(&outputs, &[10, 11, 12], Config::default());

    // Resize w2 taller than w3 inside the right vertical split (600 vs 200 -> 3:1).
    let settings = rt.layout_settings(outputs.main().frame);
    assert!(rt.workspaces.engine_mut(WorkspaceId::ONE).resize_window(
        &ws[1],
        rect(640, 0, 640, 600),
        &settings,
        &std::collections::HashMap::new(),
    ));

    // Close w1 -> 2 | 3 re-orients horizontally while keeping order [w2, w3] and 3:1 weights.
    rt.backend.frames.remove(&ws[0]);
    rt.backend.observation = Some(observation_for_windows(
        "com.example.Test",
        &ws[1],
        &[
            (ws[1].clone(), rt.backend.frames[&ws[1]]),
            (ws[2].clone(), rt.backend.frames[&ws[2]]),
        ],
    ));
    rt.cycle(outputs.clone(), &cancelled).unwrap();
    assert_eq!(rt.backend.frames[&ws[1]], rect(0, 0, 960, 800));
    assert_eq!(rt.backend.frames[&ws[2]], rect(960, 0, 320, 800));

    // Untouched branch stays unchanged: start on landscape with left split (10 / 11) and right
    // split (12 / 13), then switch to a taller display where left would flip if touched, and
    // close 13 on the right: only the right side grew, so the left split keeps its vertical axis.
    let (mut rt, ws) = open_dwindle_windows(&outputs, &[10, 11], Config::default());
    // Split left (10) vertically -> (10 / 12) | 11, then split right (11) vertically -> (10 / 12) | (11 / 13).
    let w12 = window(12);
    let w13 = window(13);
    rt.backend.frames.insert(w12.clone(), frame(200));
    rt.backend.focused = Some(ws[0].clone());
    rt.workspaces
        .engine_mut(WorkspaceId::ONE)
        .remember_insertion_focus(&ws[0]);
    rt.backend.observation = Some(observation_for_windows(
        "com.example.Test",
        &ws[0],
        &[
            (ws[0].clone(), rt.backend.frames[&ws[0]]),
            (ws[1].clone(), rt.backend.frames[&ws[1]]),
            (w12.clone(), frame(200)),
        ],
    ));
    rt.cycle(outputs.clone(), &cancelled).unwrap();
    rt.backend.frames.insert(w13.clone(), frame(250));
    rt.backend.focused = Some(ws[1].clone());
    rt.workspaces
        .engine_mut(WorkspaceId::ONE)
        .remember_insertion_focus(&ws[1]);
    rt.backend.observation = Some(observation_for_windows(
        "com.example.Test",
        &ws[1],
        &[
            (ws[0].clone(), rt.backend.frames[&ws[0]]),
            (ws[1].clone(), rt.backend.frames[&ws[1]]),
            (w12.clone(), rt.backend.frames[&w12]),
            (w13.clone(), frame(250)),
        ],
    ));
    rt.cycle(outputs, &cancelled).unwrap();

    // Move to a wider 2400x800 display (left half is now 1200x800 -> wider than tall) and close w13.
    // The left half did NOT grow from w13 closing, so (10 / 12) stays vertical!
    let ultrawide = macos::DisplayOutputs::new(vec![macos::DisplayOutput {
        id: "main-id".into(),
        name: "Main".into(),
        frame: rect(0, 0, 2400, 800),
        is_main: true,
    }])
    .unwrap();
    rt.backend.frames.remove(&w13);
    rt.backend.observation = Some(observation_for_windows(
        "com.example.Test",
        &ws[1],
        &[
            (ws[0].clone(), rt.backend.frames[&ws[0]]),
            (ws[1].clone(), rt.backend.frames[&ws[1]]),
            (w12.clone(), rt.backend.frames[&w12]),
        ],
    ));
    rt.cycle(ultrawide, &cancelled).unwrap();
    assert_eq!(rt.backend.frames[&ws[0]], rect(0, 0, 1200, 400));
    assert_eq!(rt.backend.frames[&w12], rect(0, 400, 1200, 400));
    assert_eq!(rt.backend.frames[&ws[1]], rect(1200, 0, 1200, 800));
}

#[test]
fn legacy_saved_layout_without_split_origin_loads_and_keeps_direction() {
    let outputs = landscape_outputs();
    let cancelled = AtomicBool::new(false);
    let (rt, ws) = open_dwindle_windows(&outputs, &[10, 11, 12], Config::default());

    // Strip all `auto_axis` keys from the serialized state to simulate a state file
    // saved by an earlier Fractile build before split origins were recorded.
    let mut value = serde_json::to_value(rt.workspaces.state()).unwrap();
    fn strip_auto_axis(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Object(map) => {
                map.remove("auto_axis");
                for child in map.values_mut() {
                    strip_auto_axis(child);
                }
            }
            serde_json::Value::Array(items) => {
                for child in items {
                    strip_auto_axis(child);
                }
            }
            _ => {}
        }
    }
    strip_auto_axis(&mut value);
    let legacy_state: crate::workspace::WorkspaceState = serde_json::from_value(value).unwrap();
    legacy_state.validate().unwrap();

    let backend = FakeBackend {
        frames: rt.backend.frames.clone(),
        focused: Some(ws[1].clone()),
        ..FakeBackend::default()
    };
    let mut restored = WorkspaceRuntime::with_backend_config_state(
        ManagementScope::Both,
        backend,
        HashMap::new(),
        Config::default(),
        Some(legacy_state),
    );
    restored.backend.frames.remove(&ws[0]);
    restored.backend.observation = Some(observation_for_windows(
        "com.example.Test",
        &ws[1],
        &[
            (ws[1].clone(), restored.backend.frames[&ws[1]]),
            (ws[2].clone(), restored.backend.frames[&ws[2]]),
        ],
    ));
    restored.cycle(outputs, &cancelled).unwrap();
    // Splits without a recorded origin are treated as user-set and keep their vertical direction.
    assert_eq!(restored.backend.frames[&ws[1]], rect(0, 0, 1280, 400));
    assert_eq!(restored.backend.frames[&ws[2]], rect(0, 400, 1280, 400));
}

#[test]
fn columns_mode_does_not_reorient_splits_on_close() {
    let outputs = landscape_outputs();
    let cancelled = AtomicBool::new(false);
    let (mut rt, ws) = open_dwindle_windows(&outputs, &[10, 11, 12], Config::default());
    // Switch config to Columns mode while keeping the auto-created 1 | (2 / 3) tree.
    rt.replace_config(columns_config());
    rt.backend.frames.remove(&ws[0]);
    rt.backend.observation = Some(observation_for_windows(
        "com.example.Test",
        &ws[1],
        &[
            (ws[1].clone(), rt.backend.frames[&ws[1]]),
            (ws[2].clone(), rt.backend.frames[&ws[2]]),
        ],
    ));
    rt.cycle(outputs, &cancelled).unwrap();
    assert_eq!(rt.backend.frames[&ws[1]], rect(0, 0, 1280, 400));
    assert_eq!(rt.backend.frames[&ws[2]], rect(0, 400, 1280, 400));
}
