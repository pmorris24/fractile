//! Spaceman's single-process macOS agent (`spaceman-agent`): runs the window
//! manager, native status bar item, updater, Settings window, and Rune
//! Playground (`⇧⌘D` / `--playground`) on one AppKit + Rune event loop.

#[cfg(not(test))]
#[global_allocator]
static ALLOCATOR: procwatch::alloc::Counting = procwatch::alloc::Counting::system();

mod activity;
mod apps;
mod controls;
#[cfg(test)]
mod design_tokens;
#[cfg(target_os = "macos")]
mod host;
mod hud;
mod map;
mod onboarding;
mod panes;
mod playground;
mod rules;
mod updates;
mod view;

use std::sync::Arc;

pub use panes::Pane;
use rules::{Doc, Place};
use rune::{AppSpec, KeyEvent, PointerPress, Tx, Waker};
use spaceman::app::{UiAction, UiAgentSnapshot, UiBridge};

/// The one modal sheet that is open.
#[derive(Clone, Debug)]
pub enum Sheet {
    /// The Workspace HUD's Customize sheet, with the demo desktop its miniature shows.
    Hud(hud::HudPreview),
    /// Settings › Rules' title filter, for a rule being added or one on the map.
    Filter(map::FilterDraft),
}

/// The one anchored menu that is open.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Menu {
    /// Pick a running app to send to this place.
    Add(Place),
    /// A rule's context menu, at the secondary press.
    Rule(usize, PointerPress),
    /// A built-in float's choices, by its place in `spaceman::defaults::FLOATING_BUNDLES`.
    Default(usize),
    /// The screens a workspace can show on, from its tile's number in Settings › Screens.
    Screen(spaceman::WorkspaceId),
}

pub struct Ui {
    pub pane: Pane,
    pub doc: Doc,
    pub apps: apps::Catalog,
    /// `None` until the runtime publishes its first snapshot.
    pub agent: Option<UiAgentSnapshot>,
    pub activity: activity::ActivityPane,
    bridge: Option<UiBridge>,
    window: Option<rune::AppHandle<Ui>>,
    hud_driver: Option<hud::HudDriver>,
    pub hud_preview: Option<hud::HudPreview>,
    frame_remembered: bool,
    pub tour: Option<onboarding::Tour>,
    pub drag: Option<map::Drag>,
    /// A workspace card on Settings › Screens' arrangement, mid-drag to another screen.
    pub screen_drag: Option<map::ScreenDrag>,
    pub menu: Option<Menu>,
    pub sheet: Option<Sheet>,
    /// The width cards have in the pane on screen, as last laid out.
    pub(crate) card_w: f32,
    pub running: Vec<apps::Running>,
    /// The running apps Spaceman floats with no rule, as Settings › Rules lists them, and when
    /// they were last read on Rune's clock.
    pub(crate) default_floats: Vec<String>,
    pub(crate) default_floats_at: Option<f64>,
    /// What Settings › Rules' title filter sheet has typed in it. It starts empty or with the
    /// rule's own filter from config.toml, never with a window's title.
    pub(crate) title_filter: rune::TextEdit,
    /// Every connected display, as Settings › Workspaces offers them.
    pub(crate) displays: Vec<panes::workspaces::ConnectedDisplay>,
    /// When the display list was last read, on Rune's clock.
    pub(crate) displays_at: Option<f64>,
    /// Lab slug requested by a `spaceman://labs/<slug>` deep link, preserved
    /// cleanly for Labs cards to highlight.
    pub highlight_lab: Option<String>,
    /// Setting or feature slug requested by a `spaceman://` deep link.
    pub highlight_feature: Option<String>,
    /// The last save that failed.
    pub error: Option<String>,
    /// Settings › Layout while a slider is held: the preview follows it, the file waits for
    /// the release.
    pub(crate) spacing_draft: Option<spaceman::spacing::Spacing>,
    /// Whether the four edge sliders show, once someone opened or closed them.
    pub(crate) per_edge: Option<bool>,
    /// Settings › Shortcuts' recorder.
    pub(crate) shortcut_capture: panes::shortcuts::Capture,
    /// Scenes render fixed data and ignore the live agent and file.
    pub(crate) fixture: bool,
}

impl Ui {
    /// Settings as it opens: on General, with nothing selected, open or dragged. Rules are
    /// optional, so they are a pane to visit, not the first thing Settings says.
    pub(crate) fn new(doc: Doc, bridge: Option<UiBridge>) -> Ui {
        Ui {
            pane: Pane::General,
            doc,
            apps: apps::Catalog::default(),
            agent: None,
            activity: activity::ActivityPane::default(),
            bridge,
            window: None,
            hud_driver: None,
            hud_preview: None,
            frame_remembered: false,
            tour: None,
            drag: None,
            screen_drag: None,
            menu: None,
            sheet: None,
            card_w: controls::WINDOW.card_w(controls::WINDOW.size.0),
            running: Vec::new(),
            default_floats: Vec::new(),
            default_floats_at: None,
            title_filter: rune::TextEdit::default(),
            displays: Vec::new(),
            displays_at: None,
            highlight_lab: None,
            highlight_feature: None,
            error: None,
            spacing_draft: None,
            per_edge: None,
            shortcut_capture: panes::shortcuts::Capture::Idle,
            fixture: false,
        }
    }

    #[cfg(target_os = "macos")]
    fn load_live(
        doc: Doc,
        bridge: UiBridge,
        initial_tour: Option<onboarding::Step>,
        accessibility: bool,
        initial_target: Option<spaceman::deep_link::OpenTarget>,
    ) -> Ui {
        let mut ui = Ui::new(doc, Some(bridge));
        ui.look_up_apps();
        if let Some(step) = initial_tour {
            ui.tour = Some(onboarding::Tour::from_launch(step, accessibility, &mut ui));
        }
        if let Some(target) = initial_target {
            ui.open_target(target);
        }
        ui
    }

    /// Opens the Workspace HUD's Customize sheet. With the HUD off there is nothing to customize.
    pub fn open_hud_sheet(&mut self) {
        if self.doc.hud() {
            self.menu = None;
            self.sheet = Some(Sheet::Hud(hud::HudPreview::sample()));
        }
    }

    pub fn close_sheet(&mut self) {
        self.sheet = None;
    }

    pub fn start_tour(&mut self) {
        self.sheet = None;
        self.menu = None;
        self.drag = None;
        self.flush_pending_reload();
        // Re-running setup from Settings replaces a first-run tour, and that
        // tour's hold on automatic tiling ends with it.
        if self.tour.as_ref().is_some_and(|tour| !tour.flow.manual()) {
            onboarding::dismiss(self);
        }
        self.tour = Some(onboarding::Tour::manual(self));
    }

    pub fn select_pane(&mut self, pane: Pane) {
        // Display assignments moved into Settings › Screens, beside the rules.
        let pane = if pane == Pane::Workspaces {
            Pane::Rules
        } else {
            pane
        };
        self.pane = pane;
        self.menu = None;
        self.sheet = None;
        self.drag = None;
        self.flush_pending_reload();
        self.highlight_lab = None;
        self.highlight_feature = None;
        if pane == Pane::Rules {
            // Read the displays and the running built-in floats before the next frame paints
            // them.
            self.displays_at = None;
            self.default_floats_at = None;
        }
    }

    /// Opens Settings onto a validated `spaceman://` deep-link target.
    ///
    /// Security invariant: only changes UI pane selection and highlight state;
    /// never mutates `self.doc` or enables any setting/Lab.
    pub fn open_target(&mut self, target: spaceman::deep_link::OpenTarget) {
        onboarding::dismiss(self);
        self.hud_preview = None;
        let pane = match target.pane {
            Some(deep_pane) => match Pane::from_deep_link(deep_pane) {
                Some(pane) if pane.visible(self.doc.labs()) => pane,
                Some(pane) => {
                    let line = format!(
                        "spaceman: deep link pane '{}' is behind an off [labs] switch; opening General\n",
                        pane.key()
                    );
                    drop(std::io::Write::write_all(
                        &mut std::io::stderr().lock(),
                        line.as_bytes(),
                    ));
                    Pane::General
                }
                None => {
                    let line = format!(
                        "spaceman: deep link pane '{}' is not registered in this build; opening General\n",
                        deep_pane.as_str()
                    );
                    drop(std::io::Write::write_all(
                        &mut std::io::stderr().lock(),
                        line.as_bytes(),
                    ));
                    Pane::General
                }
            },
            None => {
                if let Some(note) = &target.diagnostic {
                    let line = format!("spaceman: deep link: {note}\n");
                    drop(std::io::Write::write_all(
                        &mut std::io::stderr().lock(),
                        line.as_bytes(),
                    ));
                }
                Pane::General
            }
        };
        self.select_pane(pane);
        self.highlight_lab = target.highlight_lab;
        self.highlight_feature = target.highlight_feature;
    }

    /// Releases the first-run setup tour's hold on automatic tiling.
    pub fn finish_setup(&self) {
        self.send_action(UiAction::FinishSetup);
    }

    /// Ends a first-run tour's hold without starting tiling: windows stay where they are.
    pub fn skip_setup(&self) {
        self.send_action(UiAction::SkipSetup);
    }

    /// Asks the worker which windows are open, for the setup tour's plan.
    pub fn plan_setup(&self) {
        self.send_action(UiAction::PlanSetup);
    }

    /// Starts tiling with the open windows moved onto the plan's workspaces.
    pub fn arrange(&self, arrangement: spaceman::onboarding_plan::Arrangement) {
        self.send_action(UiAction::Arrange(arrangement));
    }

    /// Pauses tiling and puts every window back where it was before the arrangement.
    pub fn undo_arrangement(&self) {
        self.send_action(UiAction::UndoArrangement);
    }

    /// Restores every window, then quits Spaceman.
    pub fn quit(&self) {
        self.send_action(UiAction::Quit);
    }

    fn send_action(&self, action: UiAction) {
        if !self.fixture
            && let Some(bridge) = &self.bridge
        {
            bridge.send(action);
        }
    }

    pub fn toggle_automatic(&self) {
        self.send_action(UiAction::ToggleAutomatic);
    }

    pub fn toggle_pause_on_builtin_display(&mut self) {
        let next = !self.doc.pause_on_builtin_display();
        self.edit(|doc| doc.set_pause_on_builtin_display(next));
        if self.error.is_none() {
            self.send_action(UiAction::SetPauseOnBuiltinDisplay(next));
        }
    }

    pub fn open_accessibility(&self) {
        self.send_action(UiAction::OpenAccessibility);
    }

    pub fn toggle_launch_at_login(&self) {
        self.send_action(UiAction::ToggleLaunchAtLogin);
    }

    pub fn check_for_updates(&self) {
        self.send_action(UiAction::CheckForUpdates);
    }

    /// Restores every window, then installs the ready update and relaunches.
    pub fn install_update(&self) {
        self.send_action(UiAction::InstallUpdate);
    }

    /// Switches to the update channel's release on offer.
    pub fn confirm_update_switch(&self) {
        self.send_action(UiAction::ConfirmUpdateSwitch);
    }

    /// Opens the release's download, for a copy that can't update itself.
    pub fn download_update(&self) {
        self.send_action(UiAction::DownloadUpdate);
    }

    /// Saves the channel, then checks it at once.
    pub fn set_update_channel(&mut self, channel: spaceman::config::UpdateChannel) {
        self.edit(|doc| doc.set_update_channel(channel));
        if self.error.is_none() {
            self.check_for_updates();
        }
    }

    /// Sets the Workspace HUD off (`None`) or on with `style` (`Some(style)`).
    pub fn set_hud(&mut self, mode: Option<spaceman::config::HudStyle>) {
        let enabled = mode.is_some();
        self.edit(|doc| {
            doc.set_hud(enabled);
            if let Some(style) = mode {
                doc.set_hud_style(style);
            }
        });
        if self.error.is_none() {
            if let Some(driver) = &self.hud_driver {
                driver.set_enabled(enabled);
                if let Some(style) = mode {
                    driver.set_style(style);
                }
            }
            self.send_action(UiAction::SetHud(enabled));
        }
    }

    /// Saves where the Workspace HUD capsule sits, then moves every panel to that edge.
    pub fn set_hud_position(&mut self, position: spaceman::config::HudPosition) {
        self.edit(|doc| doc.set_hud_position(position));
        if self.error.is_none()
            && let Some(driver) = &self.hud_driver
        {
            driver.set_position(position);
        }
    }

    /// Saves gaps, edge padding and the single-window width, then retiles so every display
    /// takes them at once.
    pub fn set_spacing(&mut self, spacing: spaceman::spacing::Spacing) {
        self.spacing_draft = None;
        if self.doc.spacing() == spacing {
            return;
        }
        self.edit(|doc| doc.set_spacing(spacing));
        if self.error.is_none() {
            self.send_action(UiAction::Retile);
        }
    }

    pub fn mark_wrong(&self) {
        self.send_action(UiAction::MarkWrong);
    }

    /// Names and icons for every app the rules and the built-in floats mention, before the view
    /// needs them.
    pub(crate) fn look_up_apps(&mut self) {
        for rule in self.doc.rules() {
            if let Some(bundle_id) = &rule.bundle_id {
                self.apps.get(bundle_id);
            }
        }
        for bundle_id in &self.default_floats {
            self.apps.get(bundle_id);
        }
        // And every app with an open window Settings › Screens previews.
        let live: Vec<String> = self
            .agent
            .iter()
            .flat_map(|agent| &agent.windows)
            .flat_map(|entry| {
                let windows = &entry.windows;
                windows
                    .tiles
                    .iter()
                    .filter_map(|tile| tile.bundle.clone())
                    .chain(windows.floating.iter().cloned())
            })
            .collect();
        for bundle_id in &live {
            self.apps.get(bundle_id);
        }
    }

    /// Applies `edit` and saves against the latest file on disk under the config lock.
    pub fn edit(&mut self, edit: impl FnOnce(&mut Doc)) {
        self.try_edit(|doc| {
            edit(doc);
            Ok(())
        });
    }

    /// Applies a fallible `edit` against the latest file on disk under the config lock.
    pub fn try_edit(&mut self, edit: impl FnOnce(&mut Doc) -> Result<(), String>) {
        if self.fixture {
            let before = self.doc.clone();
            match edit(&mut self.doc) {
                Ok(()) => self.error = None,
                Err(error) => {
                    self.error = Some(error);
                    self.doc = before;
                }
            }
            self.look_up_apps();
            self.sync_hud_from_doc();
            return;
        }
        match self.doc.edit_and_save(edit) {
            Ok(()) => self.error = None,
            Err(error) => self.error = Some(error),
        }
        self.sync_hud_from_doc();
        self.look_up_apps();
    }

    fn sync_hud_from_doc(&mut self) {
        if !self.doc.hud() && matches!(self.sheet, Some(Sheet::Hud(_))) {
            // The HUD went off, here or in config.toml: its Customize sheet has nothing to show.
            self.sheet = None;
        }
        if let Some(driver) = &self.hud_driver {
            driver.set_enabled(self.doc.hud());
            driver.set_style(self.doc.hud_style());
            driver.set_position(self.doc.hud_position());
        }
    }

    pub(crate) fn reload_doc(&mut self) {
        if self.fixture {
            return;
        }
        self.doc = self.doc.reloaded();
        self.look_up_apps();
        self.sync_hud_from_doc();
        self.leave_hidden_lab_pane();
    }

    /// Moves Settings off a Lab pane whose switch just turned off.
    pub(crate) fn leave_hidden_lab_pane(&mut self) {
        if !self.pane.visible(self.doc.labs()) {
            self.pane = Pane::General;
            self.menu = None;
            self.sheet = None;
            self.drag = None;
            self.screen_drag = None;
        }
    }

    /// Handles a config-changed notification from the agent pump: reloads immediately when
    /// idle, or defers until an active Rules drag finishes.
    pub(crate) fn on_config_changed(&mut self) -> bool {
        self.doc.mark_reload_pending();
        if self.drag.is_none() {
            self.reload_doc();
            return true;
        }
        false
    }

    /// Applies a config reload that arrived while a Rules drag was in progress.
    pub(crate) fn flush_pending_reload(&mut self) {
        if self.drag.is_none() && self.doc.reload_pending() {
            self.reload_doc();
        }
    }
}

fn setup(ui: &mut Ui, waker: Waker) {
    if !ui.fixture
        && let Some(bridge) = &ui.bridge
    {
        bridge.set_waker(Arc::new(move || waker()));
    }
}

fn poll(ui: &mut Ui, clock: f64, tx: &mut Tx) -> bool {
    if !ui.fixture && !ui.frame_remembered {
        ui.frame_remembered = rune_mac::remember_frame("Spaceman");
    }
    let mut changed = onboarding::poll(ui, tx);
    changed |= panes::workspaces::poll_displays(ui, clock);
    changed |= map::poll_default_floats(ui, clock);
    changed |= ui.activity.poll(
        ui.pane == Pane::Activity && ui.tour.is_none() && ui.hud_preview.is_none(),
        ui.fixture,
    );
    let Some(bridge) = ui.bridge.clone() else {
        return changed;
    };
    let news = bridge.take_news();
    if let Some(snapshot) = news.snapshot {
        if let Some(driver) = &ui.hud_driver {
            driver.set_enabled(snapshot.hud);
        }
        ui.agent = Some(snapshot);
        ui.look_up_apps();
        changed = true;
        if ui.tour.is_some() {
            changed |= onboarding::poll(ui, tx);
        }
    }
    if let Some(hud_event) = news.hud
        && let Some(driver) = &ui.hud_driver
    {
        driver.trigger(hud_event);
    }
    if let Some(toast) = news.toast
        && let Some(driver) = &ui.hud_driver
    {
        driver.trigger_toast(toast);
    }
    if let Some(inventory) = news.inventory {
        changed |= onboarding::inventory(ui, inventory);
    }
    if news.config_changed {
        changed |= ui.on_config_changed();
    } else if ui.doc.reload_pending() && ui.drag.is_none() {
        ui.reload_doc();
        changed = true;
    }
    changed
}

fn on_key(ui: &mut Ui, event: &KeyEvent, _tx: &mut Tx) -> bool {
    if event.key == rune::Key::Escape {
        if ui.sheet.is_some() && ui.tour.is_none() {
            ui.close_sheet();
            return true;
        }
        if ui.drag.is_some() || ui.screen_drag.is_some() || ui.menu.is_some() {
            ui.drag = None;
            ui.screen_drag = None;
            ui.menu = None;
            ui.flush_pending_reload();
            return true;
        }
        if let Some(tour) = &mut ui.tour
            && tour.board.drag.take().is_some()
        {
            return true;
        }
        if ui.tour.is_some() {
            onboarding::dismiss(ui);
            return true;
        }
    }
    if ui.tour.is_none()
        && ui.hud_preview.is_none()
        && ui.sheet.is_none()
        && !event.mods.shift
        && !event.mods.alt
        && let rune::Key::Command(digit) = event.key
        && let Some(pane) = Pane::from_shortcut(digit, ui.doc.labs())
    {
        ui.select_pane(pane);
        return true;
    }
    false
}

fn on_close(ui: &mut Ui, _tx: &mut Tx) {
    ui.drag = None;
    ui.screen_drag = None;
    ui.menu = None;
    ui.sheet = None;
    ui.flush_pending_reload();
    // Closing the setup window dismisses the tour. A first-run tour that never reached
    // Arrange & start leaves the windows where they are, paused (`onboarding::finish`).
    onboarding::dismiss(ui);
    if let Some(window) = &ui.window {
        window.set_dock_icon(false);
    }
}

#[cfg(target_os = "macos")]
fn on_reopen(ui: &mut Ui, _has_visible_windows: bool, _tx: &mut Tx) -> bool {
    if let Some(window) = &ui.window {
        host::bring_window_forward(window);
    }
    true
}

#[cfg(target_os = "macos")]
fn on_open_url(ui: &mut Ui, link: &str, _tx: &mut Tx) {
    match spaceman::deep_link::parse_url(link) {
        Ok(parsed) => {
            ui.open_target(parsed.resolve());
            if let Some(window) = &ui.window {
                host::bring_window_forward(window);
            }
        }
        Err(error) => {
            let line = format!("spaceman: ignored deep link ({error})\n");
            drop(std::io::Write::write_all(
                &mut std::io::stderr().lock(),
                line.as_bytes(),
            ));
        }
    }
}

fn spec() -> AppSpec<Ui> {
    AppSpec {
        title: "Spaceman",
        size: controls::WINDOW.size,
        min_size: Some(controls::WINDOW.min),
        init: playground::blank_fixture,
        view: view::view,
        scenes: playground::scenes(),
        fonts: rune_theme_opencode::FONTS,
        families: rune_theme_opencode::FAMILIES,
        setup: Some(setup),
        poll: Some(poll),
        on_key: Some(on_key),
    }
}

/// Settings resizes freely, but never below `WINDOW.min`: a titlebar double click or a
/// dragged edge can then make the window bigger, while every row keeps fitting.
fn window_options() -> rune::WindowOptions {
    rune::WindowOptions {
        min_size: Some(controls::WINDOW.min),
        resizable: true,
        ..Default::default()
    }
}

fn base_app() -> rune::App<Ui> {
    let app = playground::attach(
        rune::App::new(spec())
            .chrome(rune::Chrome {
                full_size_content: true,
                traffic_lights: Some((18.0, 18.0)),
                drag_regions: true,
            })
            .window_options(window_options())
            .on_close(on_close)
            .dev_stats(activity::process_vitals)
            .standard_menus(),
    );
    let window = app.handle();
    window.update_if({
        let window = window.clone();
        move |ui, _| {
            ui.window = Some(window);
            false
        }
    });
    app
}

#[cfg(target_os = "macos")]
pub(crate) fn live_app(
    bridge: spaceman::app::UiBridge,
    initial_tour: Option<onboarding::Step>,
    accessibility: bool,
    initial_target: Option<spaceman::deep_link::OpenTarget>,
) -> rune::App<Ui> {
    let doc = Doc::load();
    let initial_hud = doc.hud();
    let initial_style = doc.hud_style();
    let initial_position = doc.hud_position();
    let hud_bridge = bridge.clone();
    let mut app = base_app()
        .init(move || Ui::load_live(doc, bridge, initial_tour, accessibility, initial_target))
        .on_reopen(on_reopen)
        .on_open_url(on_open_url);
    let hud_sink = Arc::new(hud::LiveHudSink::new(hud_bridge, app.handle()));
    let hud_driver = hud::install(
        &mut app,
        initial_hud,
        initial_style,
        initial_position,
        hud_sink,
    );
    app.handle().update_if(move |ui, _| {
        ui.hud_driver = Some(hud_driver);
        false
    });
    app
}

fn preview_mode() -> bool {
    rune::tooling::is_headless()
        || std::env::args()
            .skip(1)
            .any(|arg| matches!(arg.as_str(), "--playground" | "--help" | "-h"))
}

fn main() {
    if preview_mode() {
        base_app().run();
        return;
    }
    #[cfg(target_os = "macos")]
    if let Err(error) = host::run() {
        drop(std::io::Write::write_all(
            &mut std::io::stderr(),
            format!("Spaceman failed: {error}\n").as_bytes(),
        ));
        std::process::exit(1);
    }
    #[cfg(not(target_os = "macos"))]
    base_app().run();
}

#[cfg(test)]
mod tests {
    use super::*;
    use spaceman::deep_link::parse_url;

    #[test]
    fn the_settings_window_grows_but_never_below_the_panes() {
        // Issue 338: a titlebar double click shrank the window to 800×600, which clipped the
        // Workspaces rows. It resizes freely now, with a floor every pane fits in.
        let options = window_options();
        assert!(options.resizable, "a bigger window shows more of a pane");
        assert_eq!(options.min_size, Some(controls::WINDOW.min));
        assert_eq!(options.max_size, None, "no ceiling to grow into");
        assert!(
            !options.borderless,
            "a titled window keeps its traffic lights"
        );
        // It opens at 920 × 660, where General, Layout and Rules show whole.
        assert_eq!(spec().size, (920.0, 660.0));
        assert_eq!(spec().min_size, Some((860.0, 580.0)));
    }

    #[test]
    fn settings_opens_on_general_not_on_rules() {
        // Rules are optional defaults for new windows. Opening on them told people every app
        // needs a workspace; a deep link is the only thing that opens another pane.
        let ui = Ui::new(Doc::parse(include_str!("../fixtures/config.toml")), None);
        assert_eq!(ui.pane, Pane::General);
        assert!(ui.sheet.is_none() && ui.menu.is_none());
        assert_eq!(playground::blank_fixture().pane, Pane::General);

        let mut ui = playground::blank_fixture();
        ui.open_target(parse_url("spaceman://settings/rules").unwrap().resolve());
        assert_eq!(ui.pane, Pane::Rules);
    }

    #[test]
    fn open_target_selects_pane_and_preserves_highlights_without_mutating_config() {
        let mut ui = playground::blank_fixture();
        let initial_hud = ui.doc.hud();
        let initial_rules = ui.doc.rules().len();

        let workspaces = parse_url("spaceman://settings/workspaces")
            .unwrap()
            .resolve();
        ui.open_target(workspaces);
        // Display assignments live in Settings › Screens now.
        assert_eq!(ui.pane, Pane::Rules);
        assert_eq!(ui.highlight_lab, None);
        assert_eq!(ui.highlight_feature, None);

        let lab = parse_url("spaceman://labs/scrolling-strip")
            .unwrap()
            .resolve();
        ui.open_target(lab);
        // Settings › Labs opens with the slug kept for its row's highlight.
        assert_eq!(ui.pane, Pane::Labs);
        assert_eq!(ui.highlight_lab.as_deref(), Some("scrolling-strip"));
        assert_eq!(ui.highlight_feature.as_deref(), Some("scrolling-strip"));

        for (url, expected_feature) in [
            ("spaceman://settings/layout", None),
            ("spaceman://features/gaps", Some("gaps")),
            ("spaceman://features/max-width", Some("max-width")),
        ] {
            let target = parse_url(url).unwrap().resolve();
            ui.open_target(target);
            assert_eq!(ui.pane, Pane::Layout, "{url}");
            assert_eq!(ui.highlight_lab, None, "{url}");
            assert_eq!(ui.highlight_feature.as_deref(), expected_feature, "{url}");
        }

        let hud_feature = parse_url("spaceman://features/hud").unwrap().resolve();
        ui.open_target(hud_feature);
        assert_eq!(ui.pane, Pane::General);
        assert_eq!(ui.highlight_lab, None);
        assert_eq!(ui.highlight_feature.as_deref(), Some("hud"));

        // Manual pane switching clears deep-link highlights.
        ui.select_pane(Pane::Shortcuts);
        assert_eq!(ui.pane, Pane::Shortcuts);
        assert_eq!(ui.highlight_lab, None);
        assert_eq!(ui.highlight_feature, None);

        // Security invariant: config doc is untouched.
        assert_eq!(ui.doc.hud(), initial_hud);
        assert_eq!(ui.doc.rules().len(), initial_rules);
    }

    fn doc_with_activity_lab() -> Doc {
        Doc::parse(concat!(
            include_str!("../fixtures/config.toml"),
            "\n[labs]\nactivity = true\n"
        ))
    }

    #[test]
    fn activity_deep_link_opens_its_pane_only_with_the_lab_on() {
        let activity = parse_url("spaceman://settings/activity").unwrap().resolve();

        let mut ui = playground::blank_fixture();
        ui.open_target(activity.clone());
        assert_eq!(ui.pane, Pane::General, "hidden until the Lab is on");
        // Security invariant: opening a deep link never turns a Lab on.
        assert!(!ui.doc.labs().activity);

        ui.doc = doc_with_activity_lab();
        ui.open_target(activity);
        assert_eq!(ui.pane, Pane::Activity);
    }

    #[test]
    fn turning_the_activity_lab_off_leaves_the_pane_for_general() {
        let mut ui = playground::blank_fixture();
        ui.doc = doc_with_activity_lab();
        ui.select_pane(Pane::Activity);
        ui.leave_hidden_lab_pane();
        assert_eq!(ui.pane, Pane::Activity, "still open while the Lab is on");

        ui.doc = Doc::parse(include_str!("../fixtures/config.toml"));
        ui.leave_hidden_lab_pane();
        assert_eq!(ui.pane, Pane::General);
    }

    #[test]
    fn labs_are_read_from_the_edited_document_not_a_reload() {
        let mut ui = playground::blank_fixture();
        assert!(!ui.doc.labs().activity);
        assert!(!ui.doc.labs().scratchpad);

        let doc = Doc::parse(concat!(
            include_str!("../fixtures/config.toml"),
            "\n[labs]\nscratchpad = true\nactivity = true\n"
        ));
        assert!(doc.lab(spaceman::config::LabFeature::Scratchpad));
        assert!(doc.lab(spaceman::config::LabFeature::Activity));
        ui.doc = doc;
        assert!(ui.doc.labs().activity);
    }

    #[test]
    fn a_labs_switch_turns_on_and_off_without_touching_the_others() {
        use spaceman::config::LabFeature;
        let mut ui = playground::blank_fixture();
        ui.edit(|doc| doc.set_lab(LabFeature::PhysicalDisplays, true));
        assert!(ui.doc.labs().physical_displays);
        assert!(!ui.doc.labs().activity && !ui.doc.labs().scratchpad);
        ui.edit(|doc| doc.set_lab(LabFeature::PhysicalDisplays, false));
        assert!(!ui.doc.labs().physical_displays);
    }
}
