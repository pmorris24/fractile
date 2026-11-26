//! Setup tour orchestration and live polling.

mod arrange;
mod conflicts;
mod demo;
mod flow;
#[cfg(target_os = "macos")]
mod keys;
#[cfg(test)]
mod motion_tests;
mod prompt;
mod sound;
mod stage;
#[cfg(test)]
mod tests;
mod try_it;
mod view;

use std::time::{Duration, Instant};

pub use arrange::Board;
pub use conflicts::Conflict;
pub use demo::Move;
pub use flow::{Effect, Flow, Input, SetupState, Step, launch_step};
pub use view::overlay;

use fractile::{app::UiLaunchAtLogin, onboarding_plan::Inventory, shortcuts::Modifiers};
use rune::Tx;

use crate::{
    Pane, Ui, apps,
    playground::{Scene, reset_fixture},
    rules::Place,
};

const ADVANCE_BEAT: Duration = Duration::from_millis(700);

pub struct Tour {
    pub flow: Flow,
    /// What is on and would fight Fractile, read as the permission step opens.
    pub conflicts: Vec<Conflict>,
    /// The Arrange step's plan.
    pub board: Board,
    /// The Try it step's miniature desktop, and what is pressed on it.
    pub play: try_it::Play,
    advance_at: Option<Instant>,
    completed: bool,
    /// Arrange & start was pressed, so tiling runs with the plan applied.
    started: bool,
    /// The step the tour's side effects (reading conflicts, asking for the plan, listening
    /// for shortcuts) were last brought in line with.
    synced: Option<Step>,
    /// Hears the Try it step's shortcuts while it shows in a live window.
    #[cfg(target_os = "macos")]
    keys: Option<keys::Listener>,
}

impl Tour {
    fn new(flow: Flow, completed: bool) -> Self {
        Self {
            flow,
            conflicts: Vec::new(),
            board: Board::default(),
            play: try_it::Play::default(),
            advance_at: None,
            completed,
            started: false,
            synced: None,
            #[cfg(target_os = "macos")]
            keys: None,
        }
    }

    pub fn from_launch(step: Step, accessibility: bool, ui: &mut Ui) -> Self {
        look_up_demo_apps(ui);
        let status = SetupState {
            accessibility,
            ..ui_setup_state(ui)
        };
        Self::new(Flow::new(step, status, false), false)
    }

    pub fn manual(ui: &mut Ui) -> Self {
        look_up_demo_apps(ui);
        Self::new(Flow::new(Step::Welcome, ui_setup_state(ui), true), true)
    }

    /// Whether the key tap hears `mv`'s chord, so the window's own key events leave it alone.
    pub fn tap_hears(&self, mv: Move) -> bool {
        #[cfg(target_os = "macos")]
        return self.keys.as_ref().is_some_and(|keys| keys.hears(mv));
        #[cfg(not(target_os = "macos"))]
        {
            let _ = mv;
            false
        }
    }

    fn fixture(step: Step, accessibility: bool, asked: bool, ui: &mut Ui) -> Self {
        if let Some(agent) = &mut ui.agent {
            agent.accessibility = accessibility;
            // A first-run tour holds tiling until Arrange & start.
            agent.automatic = step == Step::TryIt;
            agent.launch_at_login = UiLaunchAtLogin::Off;
        }
        let mut flow = Flow::new(Step::Welcome, ui_setup_state(ui), false);
        while flow.step() != step {
            if flow.step() == Step::Setup && !accessibility {
                // Force fixture into later step only when accessibility is true.
                break;
            }
            flow.on(Input::Continue);
        }
        if step == Step::Setup && asked && !accessibility {
            flow.on(Input::GrantAccessibility);
        }
        let mut tour = Self::new(flow, false);
        tour.board = Board::fixture_laptop();
        tour.started = step == Step::TryIt;
        // A scene shows the data it was given, not this Mac's.
        tour.synced = Some(step);
        for bundle_id in tour.board.bundle_ids() {
            ui.apps.get(&bundle_id);
        }
        look_up_demo_apps(ui);
        tour
    }
}

/// The miniature desktop's windows wear their apps' icons, looked up before the view runs.
fn look_up_demo_apps(ui: &mut Ui) {
    for bundle_id in demo::APPS.iter().flatten() {
        ui.apps.get(bundle_id);
    }
}

pub fn poll(ui: &mut Ui, tx: &mut Tx) -> bool {
    let status = ui_setup_state(ui);
    let live = !ui.fixture;
    let Some(tour) = &mut ui.tour else {
        return false;
    };
    let before = tour.flow.view();
    let step_before = before.step;
    let mut effect = tour.flow.on(Input::Status(status));
    if tour
        .advance_at
        .is_some_and(|at| at.elapsed() >= ADVANCE_BEAT)
    {
        tour.advance_at = None;
        if tour.flow.step() == Step::Setup {
            effect = tour.flow.on(Input::Continue);
        }
    }
    if status.accessibility && !tour.completed {
        tour.completed = true;
        if live {
            flow::record_completion();
        }
    }
    apply(ui, effect);
    remember_step(ui, step_before);
    sync(ui);
    let settled = settle_play(ui, tx);
    if let Some(at) = ui.tour.as_ref().and_then(|tour| tour.advance_at) {
        tx.wake_after(
            ADVANCE_BEAT
                .saturating_sub(at.elapsed())
                .max(Duration::from_millis(1)),
        );
    }
    let after = ui.tour.as_ref().map(|tour| tour.flow.view());
    after.as_ref() != Some(&before) || settled
}

/// Lets a pressed key back up, a learned lesson's tick go and the Workspace HUD leave once
/// their beats have passed, and asks to be woken for whichever is due next.
fn settle_play(ui: &mut Ui, tx: &mut Tx) -> bool {
    let Some(tour) = &mut ui.tour else {
        return false;
    };
    let settled = tour.play.settle(tx.clock());
    if settled {
        // A beat ending isn't input to this window, so ask for its motion.
        tx.animated();
    }
    if let Some(due) = tour.play.due() {
        tx.wake_at(due);
    }
    settled
}

pub fn input(ui: &mut Ui, input: Input) {
    let Some(tour) = &mut ui.tour else {
        return;
    };
    let step_before = tour.flow.step();
    let effect = tour.flow.on(input);
    apply(ui, effect);
    remember_step(ui, step_before);
    sync(ui);
}

/// The worker's answer to the Arrange step: the open windows, title-free. Each app gets its
/// Launch Services category and its icon here, on the main thread.
pub fn inventory(ui: &mut Ui, mut inventory: Inventory) -> bool {
    if ui.tour.is_none() {
        return false;
    }
    for app in &mut inventory.apps {
        app.category = apps::category(&app.bundle_id);
        ui.apps.get(&app.bundle_id);
    }
    if let Some(tour) = &mut ui.tour {
        tour.board.set_inventory(inventory);
    }
    true
}

/// A move's shortcut was pressed on the Try it step at `now` on the window's clock: its keys
/// go down and the miniature desktop makes the move. `None` anywhere else.
pub fn press(ui: &mut Ui, mv: Move, now: f64) -> Option<demo::Pressed> {
    match &mut ui.tour {
        Some(tour) if tour.flow.step() == Step::TryIt => Some(tour.play.press(mv, now)),
        _ => None,
    }
}

/// As [`press`], from the keyboard: the keys and the desktop move on their springs, the
/// window wakes to let the keys back up, and the real setup window plays a learned lesson's
/// sound. Returns whether the view changed.
pub fn pressed(ui: &mut Ui, mv: Move, tx: &mut Tx) -> bool {
    let Some(pressed) = press(ui, mv, tx.clock()) else {
        return false;
    };
    // A press from the key tap isn't input to this window, so ask for its motion.
    tx.animated();
    if let Some(due) = ui.tour.as_ref().and_then(|tour| tour.play.due()) {
        tx.wake_at(due);
    }
    // A scene, a headless render and a test show fixed data and stay silent.
    if pressed.learned && !ui.fixture {
        let complete = ui
            .tour
            .as_ref()
            .is_some_and(|tour| tour.play.demo.complete());
        sound::play(if complete {
            sound::Cue::Chime
        } else {
            sound::Cue::Tick
        });
    }
    true
}

/// The Try it step's Modifier picker: saves the base modifier every built-in shortcut holds,
/// and listens for the new chords at once, so they can be tried on the spot.
pub fn set_modifier(ui: &mut Ui, modifier: Modifiers) {
    crate::panes::shortcuts::set_modifier(ui, modifier);
    if !ui.fixture
        && ui
            .tour
            .as_ref()
            .is_some_and(|tour| tour.flow.step() == Step::TryIt)
    {
        listen_on_try_it(ui, Step::TryIt);
    }
}

/// The button on a conflict's row: turn the macOS feature off, or quit the other window
/// manager. The row goes once that worked.
pub fn resolve(ui: &mut Ui, index: usize) {
    let live = !ui.fixture;
    let Some(tour) = &mut ui.tour else {
        return;
    };
    let Some(conflict) = tour.conflicts.get(index) else {
        return;
    };
    if !live || conflicts::resolve(conflict) {
        tour.conflicts.remove(index);
    }
}

/// Brings the tour's side effects in line with the step on screen: the permission step reads
/// what is in the way, Arrange asks the worker for the open windows, and Try it listens for
/// its shortcuts with the window kept above the tiles and the real hotkeys held off.
fn sync(ui: &mut Ui) {
    let live = !ui.fixture;
    let Some(tour) = &mut ui.tour else {
        return;
    };
    let step = tour.flow.step();
    if tour.synced == Some(step) {
        return;
    }
    tour.synced = Some(step);
    tour.board.drag = None;
    if !live {
        return;
    }
    if step == Step::Setup {
        tour.conflicts = conflicts::current();
    }
    listen_on_try_it(ui, step);
    if step == Step::Arrange {
        ui.plan_setup();
    }
}

/// Listens for the step's shortcuts only while a live window shows Try it. There the window
/// also floats above the tiles Arrange & start just placed, and the global hotkeys are held
/// off whenever it has the keyboard, so a shortcut moves the miniature desktop and no real
/// window.
#[cfg(target_os = "macos")]
fn listen_on_try_it(ui: &mut Ui, step: Step) {
    let trying = step == Step::TryIt;
    let presses = trying.then(|| demo::presses(&demo::chords(ui), fractile::macos::key_press));
    let window = ui.window.clone();
    let Some(tour) = &mut ui.tour else {
        return;
    };
    crate::host::float_setup_window(trying);
    crate::host::set_shortcut_practice(trying);
    tour.keys = match (presses, window) {
        (Some(presses), Some(window)) => keys::listen(window, presses),
        _ => None,
    };
}

#[cfg(not(target_os = "macos"))]
fn listen_on_try_it(_: &mut Ui, _: Step) {}

/// Ends the tour from the window closing or Escape.
pub fn dismiss(ui: &mut Ui) {
    input(ui, Input::Dismiss);
}

fn remember_step(ui: &Ui, before: Step) {
    if !ui.fixture
        && let Some(tour) = &ui.tour
        && !tour.flow.manual()
        && !tour.completed
        && tour.flow.step() != before
    {
        flow::record_step(tour.flow.step());
    }
}

fn apply(ui: &mut Ui, effect: Effect) {
    match effect {
        Effect::None => {}
        Effect::OpenAccessibility => ui.open_accessibility(),
        Effect::ToggleLaunchAtLogin => {
            if ui.fixture {
                if let Some(agent) = &mut ui.agent {
                    agent.launch_at_login = match agent.launch_at_login {
                        UiLaunchAtLogin::On | UiLaunchAtLogin::NeedsApproval => {
                            UiLaunchAtLogin::Off
                        }
                        UiLaunchAtLogin::Off | UiLaunchAtLogin::Failed(_) => UiLaunchAtLogin::On,
                    };
                }
                let status = ui_setup_state(ui);
                if let Some(tour) = &mut ui.tour {
                    tour.flow.on(Input::Status(status));
                }
            } else {
                ui.toggle_launch_at_login();
            }
        }
        Effect::AdvanceSoon => {
            if let Some(tour) = &mut ui.tour {
                tour.advance_at = Some(Instant::now());
            }
        }
        Effect::Arrange => arrange_and_start(ui),
        Effect::Undo => {
            if let Some(tour) = &mut ui.tour {
                tour.started = false;
                tour.play = try_it::Play::default();
            }
            ui.undo_arrangement();
            set_fixture_tiling(ui, false);
        }
        Effect::Finish => finish(ui),
    }
}

/// Arrange & start: the open windows move to the plan's workspaces and tiling starts. The
/// only things saved are a `float` rule for each app the plan floats that Fractile would tile,
/// and a `tile` rule for each built-in floating utility the plan tiles; no window is routed by
/// rule, so a browser's or a terminal's next window opens where you are.
fn arrange_and_start(ui: &mut Ui) {
    let Some(tour) = &mut ui.tour else {
        return;
    };
    tour.started = true;
    tour.play = try_it::Play::default();
    let arrangement = tour.board.plan.arrangement();
    let (floats, tiles) = tour
        .board
        .inventory()
        .map(|inventory| {
            let plan = &tour.board.plan;
            (plan.float_rules(inventory), plan.tile_rules(inventory))
        })
        .unwrap_or_default();
    let unsaved: Vec<(String, Place)> = floats
        .into_iter()
        .map(|bundle_id| (bundle_id, Place::Float))
        .chain(tiles.into_iter().map(|bundle_id| (bundle_id, Place::Tile)))
        .filter(|(bundle_id, place)| !placed_by_rule(ui, bundle_id, *place))
        .collect();
    if !unsaved.is_empty() {
        ui.edit(|doc| {
            for (bundle_id, place) in &unsaved {
                doc.set_app_place(bundle_id, Some(*place));
            }
        });
    }
    ui.arrange(arrangement);
    set_fixture_tiling(ui, true);
}

fn placed_by_rule(ui: &Ui, bundle_id: &str, place: Place) -> bool {
    ui.doc.rules().iter().any(|rule| {
        rule.app_wide() && rule.bundle_id.as_deref() == Some(bundle_id) && rule.place == place
    })
}

/// A scene has no agent to answer, so it plays the agent's part.
fn set_fixture_tiling(ui: &mut Ui, tiling: bool) {
    if !ui.fixture {
        return;
    }
    if let Some(agent) = &mut ui.agent {
        agent.automatic = tiling;
    }
    let status = ui_setup_state(ui);
    if let Some(tour) = &mut ui.tour {
        tour.flow.on(Input::Status(status));
    }
}

/// Ends the tour. A first-run tour holds automatic tiling, and how it ends decides what
/// happens to that hold: started with Arrange & start, tiling is already running; ended
/// before Accessibility was granted, tiling starts once it is; ended with Accessibility but
/// without starting (Skip, or the window closed), the windows stay where they are, paused.
fn finish(ui: &mut Ui) {
    let Some(tour) = ui.tour.take() else {
        return;
    };
    #[cfg(target_os = "macos")]
    if !ui.fixture {
        crate::host::float_setup_window(false);
        crate::host::set_shortcut_practice(false);
    }
    if !ui.fixture {
        flow::record_completion();
    }
    if tour.flow.manual() {
        return;
    }
    if tour.started || !ui_setup_state(ui).accessibility {
        ui.finish_setup();
    } else {
        ui.skip_setup();
    }
}

fn ui_setup_state(ui: &Ui) -> SetupState {
    let agent = ui.agent.as_ref();
    SetupState {
        accessibility: agent.is_some_and(|agent| agent.accessibility),
        launch_at_login: agent.is_some_and(|agent| {
            matches!(
                agent.launch_at_login,
                UiLaunchAtLogin::On | UiLaunchAtLogin::NeedsApproval
            )
        }),
        tiling: agent.is_some_and(|agent| agent.automatic),
    }
}

fn tour_scene(ui: &mut Ui, step: Step, accessibility: bool, asked: bool) {
    reset_fixture(ui, Pane::Rules);
    ui.tour = Some(Tour::fixture(step, accessibility, asked, ui));
}

fn board_scene(ui: &mut Ui, board: Board) {
    tour_scene(ui, Step::Arrange, true, false);
    for bundle_id in board.bundle_ids() {
        ui.apps.get(&bundle_id);
    }
    if let Some(tour) = &mut ui.tour {
        tour.board = board;
    }
}

/// Try it after `moves`, at rest: no key down, no tick showing and the HUD away.
fn play_scene(ui: &mut Ui, moves: &[Move]) {
    tour_scene(ui, Step::TryIt, true, false);
    for mv in moves {
        press(ui, *mv, 0.0);
    }
    if let Some(tour) = &mut ui.tour {
        tour.play.rest();
    }
}

/// The presses that learn every lesson, in order.
const ALL_LESSONS: [Move; 7] = [
    Move::There,
    Move::Back,
    Move::Float,
    Move::Float,
    Move::Fullscreen,
    Move::Back,
    Move::Fullscreen,
];

pub(crate) const SCENES: &[Scene] = &[
    ("onboarding-welcome", |ui| {
        tour_scene(ui, Step::Welcome, false, false)
    }),
    ("onboarding-permission", |ui| {
        tour_scene(ui, Step::Setup, false, false)
    }),
    ("onboarding-permission-waiting", |ui| {
        tour_scene(ui, Step::Setup, false, true)
    }),
    ("onboarding-permission-ready", |ui| {
        tour_scene(ui, Step::Setup, true, false)
    }),
    // Stage Manager and macOS edge tiling are on, and another window manager is running.
    ("onboarding-permission-conflict", |ui| {
        tour_scene(ui, Step::Setup, false, false);
        if let Some(tour) = &mut ui.tour {
            tour.conflicts = vec![
                Conflict::StageManager,
                Conflict::EdgeTiling,
                Conflict::WindowManager("AeroSpace".into()),
            ];
        }
    }),
    ("onboarding-arrange", |ui| {
        board_scene(ui, Board::fixture_laptop())
    }),
    ("onboarding-arrange-dual", |ui| {
        board_scene(ui, Board::fixture_dual())
    }),
    // Terminal held over workspace 3.
    ("onboarding-arrange-drag", |ui| {
        let mut board = Board::fixture_laptop();
        board.fixture_drag();
        board_scene(ui, board);
    }),
    // One window open: nothing to arrange.
    ("onboarding-arrange-empty", |ui| {
        board_scene(ui, Board::fixture_single())
    }),
    ("onboarding-try-it", |ui| play_scene(ui, &[])),
    ("onboarding-try-it-ws2", |ui| play_scene(ui, &[Move::There])),
    ("onboarding-try-it-floated", |ui| {
        play_scene(ui, &[Move::There, Move::Back, Move::Float]);
    }),
    ("onboarding-try-it-fullscreen", |ui| {
        play_scene(ui, &ALL_LESSONS[..5]);
    }),
    // Option-1 again in Fullscreen Layout: Terminal has come forward from behind Safari.
    ("onboarding-try-it-cycled", |ui| {
        play_scene(ui, &ALL_LESSONS[..6]);
    }),
    // Every lesson learned, back on the tiles and free to play.
    ("onboarding-try-it-complete", |ui| {
        play_scene(ui, &ALL_LESSONS);
        if let Some(agent) = &mut ui.agent {
            agent.launch_at_login = UiLaunchAtLogin::On;
        }
    }),
    // Control-Option as the base modifier: every prompt's keys follow it.
    ("onboarding-try-it-modifier", |ui| {
        play_scene(ui, &[Move::There, Move::Back]);
        ui.edit(|doc| doc.set_shortcut_modifier(crate::panes::shortcuts::PRESETS[1].1));
    }),
];
