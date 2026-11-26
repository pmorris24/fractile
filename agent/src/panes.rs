//! Settings pane registry and playground scenes.

mod general;
pub mod labs;
mod layout;
pub mod shortcuts;
pub(crate) mod workspaces;

use rune::{El, Icon};
use spaceman::{
    app::{UiLaunchAtLogin, UiUpdateStatus},
    config::{LabFeature, LabsConfig},
};

use crate::{
    Sheet, Ui, activity,
    controls::Paint,
    map,
    playground::{Scene, reset_fixture},
};

static GENERAL_ICON: Icon = Icon {
    name: "spaceman-general",
    view: 16.0,
    body: include_str!("../assets/general.svg"),
};
static LAYOUT_ICON: Icon = Icon {
    name: "spaceman-layout",
    view: 16.0,
    body: include_str!("../assets/layout.svg"),
};
static RULES_ICON: Icon = Icon {
    name: "spaceman-rules",
    view: 16.0,
    body: include_str!("../assets/rules.svg"),
};
static WORKSPACES_ICON: Icon = Icon {
    name: "spaceman-workspaces",
    view: 16.0,
    body: include_str!("../assets/workspaces.svg"),
};
static LABS_ICON: Icon = Icon {
    name: "spaceman-labs",
    view: 16.0,
    body: include_str!("../assets/labs.svg"),
};
static ACTIVITY_ICON: Icon = Icon {
    name: "spaceman-activity",
    view: 24.0,
    body: include_str!("../assets/activity.svg"),
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pane {
    General,
    Layout,
    Rules,
    Workspaces,
    Shortcuts,
    Activity,
    /// Every `[labs]` switch, grouped by what it changes. Always listed, last, so it never
    /// moves another pane's `⌘` digit.
    Labs,
}

impl Pane {
    /// The sidebar's panes. Workspaces is not listed: Screens holds the display assignments,
    /// and a link to Workspaces opens Screens.
    pub const ALL: [Pane; 6] = [
        Pane::General,
        Pane::Layout,
        Pane::Rules,
        Pane::Shortcuts,
        Pane::Activity,
        Pane::Labs,
    ];

    pub const fn key(self) -> &'static str {
        match self {
            Pane::General => "general",
            Pane::Layout => "layout",
            Pane::Rules => "rules",
            Pane::Workspaces => "workspaces",
            Pane::Shortcuts => "shortcuts",
            Pane::Activity => "activity",
            Pane::Labs => "labs",
        }
    }

    pub const fn title(self) -> &'static str {
        match self {
            Pane::General => "General",
            Pane::Layout => "Layout",
            Pane::Rules => "Screens",
            Pane::Workspaces => "Workspaces",
            Pane::Shortcuts => "Shortcuts",
            Pane::Activity => "Activity",
            Pane::Labs => "Labs",
        }
    }

    pub const fn icon(self) -> &'static Icon {
        match self {
            Pane::General => &GENERAL_ICON,
            Pane::Layout => &LAYOUT_ICON,
            Pane::Rules => &RULES_ICON,
            Pane::Workspaces => &WORKSPACES_ICON,
            Pane::Shortcuts => &rune_theme_opencode::icons::KEYBOARD,
            Pane::Activity => &ACTIVITY_ICON,
            Pane::Labs => &LABS_ICON,
        }
    }

    pub fn render(self, ui: &Ui, p: Paint, layer: &mut Vec<El>) -> El {
        match self {
            Pane::General => general::pane(ui, p),
            Pane::Layout => layout::pane(ui, p),
            Pane::Rules => map::pane(ui, p, layer),
            Pane::Workspaces => workspaces::pane(ui, p),
            Pane::Shortcuts => shortcuts::pane(ui, p),
            Pane::Activity => activity::pane(ui, p),
            Pane::Labs => labs::pane(ui, p),
        }
    }

    /// Whether Settings lists this pane with `labs` as configured. A Lab pane stays out of the
    /// sidebar until its `[labs]` switch is on.
    pub const fn visible(self, labs: LabsConfig) -> bool {
        match self {
            Pane::Activity => labs.enabled(LabFeature::Activity),
            Pane::General
            | Pane::Layout
            | Pane::Rules
            | Pane::Workspaces
            | Pane::Shortcuts
            | Pane::Labs => true,
        }
    }

    /// Pane::ALL without the Lab panes whose switch is off, in sidebar order.
    pub fn listed(labs: LabsConfig) -> impl Iterator<Item = Pane> {
        Self::ALL.into_iter().filter(move |pane| pane.visible(labs))
    }

    /// Maps `⌘1`..`⌘N` in the sidebar order `labs` lists, so a hidden Lab pane takes no digit.
    pub fn from_shortcut(digit: char, labs: LabsConfig) -> Option<Pane> {
        let index = digit.to_digit(10)?.checked_sub(1)? as usize;
        Self::listed(labs).nth(index)
    }

    /// Maps a `spaceman://` deep-link pane to a registered Settings pane.
    ///
    /// Returns `None` for panes not registered in this build (such as `WhatsNew`) so the
    /// caller can fall back cleanly while preserving `highlight_lab` / `highlight_feature`.
    pub const fn from_deep_link(pane: spaceman::deep_link::DeepLinkPane) -> Option<Pane> {
        match pane {
            spaceman::deep_link::DeepLinkPane::General => Some(Pane::General),
            spaceman::deep_link::DeepLinkPane::Layout => Some(Pane::Layout),
            spaceman::deep_link::DeepLinkPane::Rules => Some(Pane::Rules),
            spaceman::deep_link::DeepLinkPane::Workspaces => Some(Pane::Rules),
            spaceman::deep_link::DeepLinkPane::Shortcuts => Some(Pane::Shortcuts),
            spaceman::deep_link::DeepLinkPane::Activity => Some(Pane::Activity),
            spaceman::deep_link::DeepLinkPane::Labs => Some(Pane::Labs),
            spaceman::deep_link::DeepLinkPane::WhatsNew => None,
        }
    }
}

/// Mounts the open modal sheet, if there is one, above the pane.
pub fn sheet(ui: &Ui, p: Paint, reduce_motion: bool, layer: &mut Vec<El>) {
    match &ui.sheet {
        Some(Sheet::Hud(preview)) => general::hud_sheet(ui, preview, p, reduce_motion, layer),
        Some(Sheet::Filter(draft)) => map::filter_sheet(ui, draft, p, layer),
        None => {}
    }
}

#[cfg(test)]
mod tests;

pub(crate) const SCENES: &[Scene] = &[
    ("general", |ui| reset_fixture(ui, Pane::General)),
    ("general-setup", |ui| {
        reset_fixture(ui, Pane::General);
        if let Some(agent) = &mut ui.agent {
            agent.accessibility = false;
            agent.automatic = false;
            agent.launch_at_login = UiLaunchAtLogin::Off;
        }
    }),
    ("general-hud-off", |ui| {
        reset_fixture(ui, Pane::General);
        ui.set_hud(None);
    }),
    // The Workspace HUD's Customize sheet, with the capsule on each edge.
    ("hud-customize", |ui| {
        reset_fixture(ui, Pane::General);
        general::fixture_hud_sheet(ui, spaceman::config::HudPosition::Bottom);
    }),
    ("hud-customize-left", |ui| {
        reset_fixture(ui, Pane::General);
        general::fixture_hud_sheet(ui, spaceman::config::HudPosition::Left);
    }),
    ("hud-customize-top", |ui| {
        reset_fixture(ui, Pane::General);
        general::fixture_hud_sheet(ui, spaceman::config::HudPosition::Top);
    }),
    ("general-update-ready", |ui| {
        reset_fixture(ui, Pane::General);
        if let Some(agent) = &mut ui.agent {
            agent.update = UiUpdateStatus::Ready {
                version: "0.3.3".into(),
            };
        }
    }),
    ("layout", |ui| reset_fixture(ui, Pane::Layout)),
    // Gaps, uneven edges for a custom bar, and a width cap that brings in the ultrawide.
    ("layout-spaced", |ui| {
        reset_fixture(ui, Pane::Layout);
        ui.edit(|doc| {
            doc.set_spacing(spaceman::spacing::Spacing {
                inner: 10,
                top: 32,
                bottom: 8,
                left: 8,
                right: 8,
                single_window_max_width: 1680,
                single_window_max_height: 0,
            });
        });
    }),
    ("workspaces", |ui| reset_fixture(ui, Pane::Workspaces)),
    // A third monitor: workspace 9 assigned to it, its segment in every row's picker.
    ("workspaces-three-displays", |ui| {
        reset_fixture(ui, Pane::Workspaces);
        ui.displays = workspaces::fixture_three();
        let nine = spaceman::WorkspaceId::new(9).unwrap_or(spaceman::WorkspaceId::SIX);
        ui.edit(|doc| doc.set_monitor(nine, Some(workspaces::THIRD_FIXTURE_ID)));
    }),
    // A display whose name is too long for a segment: its card keeps the name (issue 349).
    ("workspaces-long-name", |ui| {
        reset_fixture(ui, Pane::Workspaces);
        ui.displays = workspaces::fixture_long_name();
    }),
    // The Control + Option base: workspace 10 names the keys that switch to it (issue 350).
    ("workspaces-custom-modifier", |ui| {
        reset_fixture(ui, Pane::Workspaces);
        shortcuts::fixture_custom(ui);
        ui.shortcut_capture = shortcuts::Capture::default();
    }),
    // A fourth monitor: the picker numbers the last two so it stays one readable row.
    ("workspaces-four-displays", |ui| {
        reset_fixture(ui, Pane::Workspaces);
        ui.displays = workspaces::fixture_quad();
    }),
    // The third monitor unplugged: its saved assignment keeps its own card.
    ("workspaces-unplugged", |ui| {
        reset_fixture(ui, Pane::Workspaces);
        let nine = spaceman::WorkspaceId::new(9).unwrap_or(spaceman::WorkspaceId::SIX);
        ui.edit(|doc| doc.set_monitor(nine, Some(workspaces::THIRD_FIXTURE_ID)));
    }),
    ("shortcuts", |ui| reset_fixture(ui, Pane::Shortcuts)),
    // With `[labs] swipe_switch` on, the swipe row becomes a switch, here turned off.
    ("shortcuts-swipe-off", |ui| {
        reset_fixture(ui, Pane::Shortcuts);
        ui.edit(|doc| {
            doc.set_lab(LabFeature::SwipeSwitch, true);
            doc.set_trackpad_swipe(false);
        });
    }),
    ("labs", |ui| reset_fixture(ui, Pane::Labs)),
    // Physical display order on and highlighted, as `spaceman://labs/physical-displays` opens it.
    ("labs-physical-displays", |ui| {
        reset_fixture(ui, Pane::Labs);
        ui.edit(|doc| doc.set_lab(spaceman::config::LabFeature::PhysicalDisplays, true));
        ui.highlight_lab = Some(labs::slug(spaceman::config::LabFeature::PhysicalDisplays));
    }),
    ("shortcuts-recording", |ui| {
        reset_fixture(ui, Pane::Shortcuts);
        shortcuts::fixture_recording(ui, None);
    }),
    ("shortcuts-refused", |ui| {
        reset_fixture(ui, Pane::Shortcuts);
        shortcuts::fixture_recording(ui, Some("Used by Next window"));
    }),
    ("shortcuts-custom", |ui| {
        reset_fixture(ui, Pane::Shortcuts);
        shortcuts::fixture_custom(ui);
    }),
];
