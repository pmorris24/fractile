//! Rune Playground (`⇧⌘D` and `--playground`) and headless `--scene` registry,
//! assembling scenes registered by each feature module on isolated fixture state.

use std::sync::OnceLock;

use fractile::{
    WorkspaceId,
    app::{UiAgentSnapshot, UiLaunchAtLogin},
};
use rune::{App, Playground};

use crate::{Pane, Ui, activity, controls::WINDOW, hud, map, onboarding, panes, rules::Doc};

pub(crate) type Scene = (&'static str, fn(&mut Ui));

const SCENE_GROUPS: &[&[Scene]] = &[
    onboarding::SCENES,
    panes::SCENES,
    map::SCENES,
    activity::SCENES,
    hud::SCENES,
];

pub fn attach(app: App<Ui>) -> App<Ui> {
    let playground = Playground::new()
        .enabled(true)
        .bar(false)
        .scenes(|name| name.contains('/'))
        .sizes(&[
            ("Fractile window", WINDOW.size),
            ("Smallest window", WINDOW.min),
            ("Wide window", (1200.0, 840.0)),
        ]);
    app.playground(hud::triggers(playground))
}

pub fn scenes() -> &'static [Scene] {
    static ALL: OnceLock<Vec<Scene>> = OnceLock::new();
    ALL.get_or_init(|| {
        let base: Vec<Scene> = SCENE_GROUPS
            .iter()
            .flat_map(|scenes| scenes.iter().copied())
            .collect();
        let mut grouped: Vec<(usize, Scene)> = base
            .iter()
            .map(|&(name, apply)| {
                let mut ui = blank_fixture();
                apply(&mut ui);
                let (rank, group) = group(&ui);
                let path: &'static str = Box::leak(format!("{group}/{name}").into_boxed_str());
                (rank, (path, apply))
            })
            .collect();
        grouped.sort_by_key(|(rank, _)| *rank);
        let mut out = base;
        out.extend(grouped.into_iter().map(|(_, scene)| scene));
        out
    })
}

pub(crate) fn blank_fixture() -> Ui {
    let mut ui = Ui::new(Doc::parse(include_str!("../fixtures/config.toml")), None);
    ui.frame_remembered = true;
    ui.card_w = WINDOW.card_w(render_width());
    reset_fixture(&mut ui, Pane::General);
    ui
}

/// The window width a headless render asked for (`--size 1200x840`), else the width Settings
/// opens at. A snap draws one frame, before the pane's layout has reported its cards' width.
fn render_width() -> f32 {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--size" {
            let width = args
                .next()
                .and_then(|size| size.split_once('x')?.0.parse::<f32>().ok());
            return width.unwrap_or(WINDOW.size.0).max(WINDOW.min.0);
        }
    }
    WINDOW.size.0
}

pub(crate) fn reset_fixture(ui: &mut Ui, pane: Pane) {
    ui.fixture = true;
    ui.pane = pane;
    ui.tour = None;
    ui.sheet = None;
    ui.hud_preview = None;
    ui.spacing_draft = None;
    ui.per_edge = None;
    ui.displays = panes::workspaces::fixture_pair();
    ui.displays_at = None;
    ui.highlight_lab = None;
    ui.highlight_feature = None;
    ui.shortcut_capture = panes::shortcuts::Capture::Idle;
    ui.doc = Doc::parse(include_str!("../fixtures/config.toml"));
    ui.menu = None;
    ui.drag = None;
    ui.screen_drag = None;
    ui.error = None;
    ui.title_filter = rune::TextEdit::default();
    ui.default_floats = map::fixture_default_floats();
    ui.look_up_apps();
    ui.agent = Some(UiAgentSnapshot {
        accessibility: true,
        automatic: true,
        hud: true,
        launch_at_login: UiLaunchAtLogin::On,
        active_workspaces: vec![
            WorkspaceId::THREE,
            WorkspaceId::named('M').unwrap_or(WorkspaceId::SIX),
        ],
        windows: Vec::new(),
        last_error: None,
    });
    ui.look_up_apps();
}

fn group(ui: &Ui) -> (usize, &'static str) {
    if ui.tour.is_some() {
        return (0, "Onboarding");
    }
    if ui.hud_preview.is_some() {
        return (Pane::ALL.len() + 1, "HUD");
    }
    let rank = Pane::ALL
        .iter()
        .position(|pane| *pane == ui.pane)
        .unwrap_or(0);
    (rank + 1, ui.pane.title())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_every_base_scene_under_its_pane() {
        let base_count: usize = SCENE_GROUPS.iter().map(|scenes| scenes.len()).sum();
        let grouped: Vec<&str> = scenes()
            .iter()
            .map(|(name, _)| *name)
            .filter(|name| name.contains('/'))
            .collect();
        assert_eq!(grouped.len(), base_count);
        for expected in [
            "Onboarding/onboarding-welcome",
            "Onboarding/onboarding-permission",
            "Onboarding/onboarding-permission-waiting",
            "Onboarding/onboarding-permission-ready",
            "Onboarding/onboarding-permission-conflict",
            "Onboarding/onboarding-arrange",
            "Onboarding/onboarding-arrange-dual",
            "Onboarding/onboarding-arrange-drag",
            "Onboarding/onboarding-arrange-empty",
            "Onboarding/onboarding-try-it",
            "Onboarding/onboarding-try-it-ws2",
            "Onboarding/onboarding-try-it-floated",
            "Onboarding/onboarding-try-it-fullscreen",
            "Onboarding/onboarding-try-it-cycled",
            "Onboarding/onboarding-try-it-complete",
            "General/general",
            "General/general-setup",
            "General/general-hud-off",
            "General/hud-customize",
            "General/hud-customize-top",
            "Layout/layout",
            "Layout/layout-spaced",
            "Screens/rules",
            "Screens/rules-drag",
            "Screens/rules-add",
            "Screens/rules-filter",
            "Screens/rules-filter-taken",
            "Screens/rules-default",
            "Workspaces/workspaces",
            "Workspaces/workspaces-three-displays",
            "Workspaces/workspaces-long-name",
            "Workspaces/workspaces-custom-modifier",
            "Workspaces/workspaces-four-displays",
            "Workspaces/workspaces-unplugged",
            "Shortcuts/shortcuts",
            "Shortcuts/shortcuts-recording",
            "Shortcuts/shortcuts-refused",
            "Shortcuts/shortcuts-custom",
            "Shortcuts/shortcuts-swipe-off",
            "Activity/activity",
            "HUD/hud-switch",
            "HUD/hud-empty",
            "HUD/hud-fullscreen",
            "HUD/hud-many",
        ] {
            assert!(
                grouped.contains(&expected),
                "{expected} missing from {grouped:?}"
            );
        }
    }

    #[test]
    fn command_shortcuts_follow_sidebar_pane_order() {
        // Every pane is registered; a Lab pane takes a digit only while its switch is on.
        let labs = fractile::config::LabsConfig {
            activity: true,
            ..fractile::config::LabsConfig::default()
        };
        assert_eq!(Pane::from_shortcut('0', labs), None);
        for (index, expected) in Pane::ALL.into_iter().enumerate() {
            let digit = char::from_digit((index + 1) as u32, 10).unwrap();
            assert_eq!(Pane::from_shortcut(digit, labs), Some(expected));
        }
        assert_eq!(Pane::from_shortcut('8', labs), None);
    }
}
