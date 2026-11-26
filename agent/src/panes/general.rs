//! General pane: Accessibility (only while missing), core preferences, and
//! configuration. The Workspace HUD's rare refinements open in its Customize sheet.

use rune::{El, Tx, row};
use rune_kit::SegmentedControl;
use spaceman::{
    app::UiLaunchAtLogin,
    config::{HudPosition, HudStyle, LabFeature, MenuBarIcon, MouseFollowsFocus},
};

use crate::{
    Pane, Sheet, Ui,
    controls::{self, ButtonKind, METRICS, ModalWidth, Paint},
    hud,
    view::scroll_body,
};

/// The HUD's choices, each with what it shows: the row's description says what is true now.
const HUD_MODES: [(&str, Option<HudStyle>, &str); 3] = [
    ("Off", None, "Hidden"),
    ("Icons", Some(HudStyle::Icons), "Each workspace's apps"),
    (
        "Layouts",
        Some(HudStyle::Layouts),
        "Each workspace's layout",
    ),
];

fn hud_mode_index(enabled: bool, style: HudStyle) -> usize {
    let current = enabled.then_some(style);
    HUD_MODES
        .iter()
        .position(|(_, mode, _)| *mode == current)
        .unwrap_or(1)
}

const HUD_POSITIONS: [(&str, HudPosition); 4] = [
    ("Bottom", HudPosition::Bottom),
    ("Top", HudPosition::Top),
    ("Left", HudPosition::Left),
    ("Right", HudPosition::Right),
];

fn hud_position_index(position: HudPosition) -> usize {
    HUD_POSITIONS
        .iter()
        .position(|(_, candidate)| *candidate == position)
        .unwrap_or(0)
}

const MOUSE_FOLLOW_MODES: [(&str, MouseFollowsFocus, &str); 3] = [
    ("Off", MouseFollowsFocus::Off, "The pointer stays put"),
    (
        "Display",
        MouseFollowsFocus::Display,
        "Jumps to the focused display",
    ),
    (
        "Window",
        MouseFollowsFocus::Window,
        "Jumps to the focused window",
    ),
];

fn mouse_follow_index(mode: MouseFollowsFocus) -> usize {
    MOUSE_FOLLOW_MODES
        .iter()
        .position(|(_, candidate, _)| *candidate == mode)
        .unwrap_or(0)
}

/// The menu bar icon's choices (`[labs] menu_bar_icon_style`), each with what it shows.
const MENU_BAR_ICONS: [(&str, MenuBarIcon, &str); 3] = [
    (
        "Full",
        MenuBarIcon::Full,
        "A layout and number for each display",
    ),
    (
        "Compact",
        MenuBarIcon::Compact,
        "One layout and number: the focused display's",
    ),
    (
        "Off",
        MenuBarIcon::Off,
        "Hidden; open Spaceman again to get here",
    ),
];

fn menu_bar_icon_index(icon: MenuBarIcon) -> usize {
    MENU_BAR_ICONS
        .iter()
        .position(|(_, candidate, _)| *candidate == icon)
        .unwrap_or(0)
}

/// The miniature display in the HUD's Customize sheet: as wide as the sheet's cards.
const HUD_PREVIEW_H: f32 = 208.0;

pub fn pane(ui: &Ui, p: Paint) -> El {
    let c = &p.k.c;
    let focus = ui.highlight_feature.as_deref();
    let highlight = |el: El, key: &str| controls::highlight(p, el, focus == Some(key));
    let mut sections = Vec::new();
    if let Some(agent) = &ui.agent {
        if !agent.accessibility {
            let access = controls::setting_row(
                p,
                "accessibility",
                spaceman::permission_guide::list_name(),
                Some(("Required to move and resize windows".into(), c.warning.fg)),
                controls::button(
                    p,
                    "grant",
                    "Open System Settings",
                    ButtonKind::Primary,
                    |ui, _| ui.open_accessibility(),
                ),
            );
            sections.push(controls::card(p, "accessibility-card", vec![access]));
        }
        let tiling = highlight(
            if agent.accessibility {
                controls::toggle_row(
                    p,
                    "tiling",
                    "Automatic tiling",
                    (!agent.automatic)
                        .then(|| ("Windows stay where you place them".into(), p.muted())),
                    agent.automatic,
                    |ui| ui.toggle_automatic(),
                )
            } else {
                controls::setting_row(
                    p,
                    "tiling",
                    "Automatic tiling",
                    Some(("Requires Accessibility permission".into(), p.muted())),
                    controls::status_dot(c.border_strong),
                )
            },
            "tiling",
        );
        let (on, description) = match &agent.launch_at_login {
            UiLaunchAtLogin::On => (true, None),
            UiLaunchAtLogin::Off => (false, None),
            UiLaunchAtLogin::NeedsApproval => (
                true,
                Some((
                    "Waiting for approval in System Settings › Login Items".to_owned(),
                    c.warning.fg,
                )),
            ),
            UiLaunchAtLogin::Failed(error) => (false, Some((error.clone(), c.danger.fg))),
        };
        let login = highlight(
            controls::toggle_row(p, "login", "Launch at login", description, on, |ui| {
                ui.toggle_launch_at_login();
            }),
            "login",
        );
        let menu_bar_icon = ui
            .doc
            .lab(LabFeature::MenuBarIconStyle)
            .then(|| highlight(menu_bar_icon_row(ui, p), "menu-bar-icon"));
        let pause_on_builtin = highlight(
            controls::toggle_row(
                p,
                "pause-on-builtin-display",
                "Pause on built-in display",
                Some((
                    "Resumes automatically when an external display connects".into(),
                    p.muted(),
                )),
                ui.doc.pause_on_builtin_display(),
                |ui| ui.toggle_pause_on_builtin_display(),
            ),
            "pause-on-builtin-display",
        );
        let mut rows = vec![
            tiling,
            pause_on_builtin,
            highlight(hud_row(ui, p), "hud"),
            highlight(mouse_follows_focus_row(ui, p), "mouse-follows-focus"),
        ];
        rows.extend(menu_bar_icon);
        rows.push(login);
        sections.push(controls::card(p, "agent", rows));
    }
    let config_file = highlight(
        controls::setting_row(
            p,
            "file",
            "config.toml",
            Some((ui.doc.display_path(), p.muted())),
            controls::button(
                p,
                "reveal",
                "Show in Finder",
                ButtonKind::Secondary,
                |_: &mut Ui, _: &mut Tx| {
                    if let Some(path) = spaceman::config::config_path() {
                        controls::reveal_in_finder(&path);
                    }
                },
            ),
        ),
        "config",
    );
    let setup_tour = highlight(
        controls::setting_row(
            p,
            "setup-tour",
            "Setup tour",
            None,
            controls::button(p, "run-tour", "Run", ButtonKind::Secondary, |ui, _| {
                ui.start_tour();
            }),
        ),
        "setup-tour",
    );
    sections.push(controls::section(
        p,
        "config",
        "Configuration",
        vec![config_file, setup_tour],
    ));
    if let Some(agent) = &ui.agent {
        sections.push(controls::card(
            p,
            "leave",
            vec![leave_row(p, agent.automatic)],
        ));
    }
    scroll_body("general-body", controls::page("sections", sections))
}

/// The way out without the menu bar, which a notch can hide: pause tiling and get every
/// window back, or quit, which restores them first.
fn leave_row(p: Paint, tiling: bool) -> El {
    let pause = tiling.then(|| {
        controls::quiet_button(p, "pause", "Pause & restore windows", |ui: &mut Ui, _| {
            ui.toggle_automatic();
        })
    });
    controls::setting_row(
        p,
        "leave",
        "Quit Spaceman",
        Some(("Puts every window back first".to_owned(), p.muted())),
        row()
            .key("leave-controls")
            .gap(8.0)
            .center_y()
            .children(pause)
            .child(controls::button(
                p,
                "quit",
                "Quit",
                ButtonKind::Secondary,
                |ui: &mut Ui, _| ui.quit(),
            )),
    )
}

/// The Workspace HUD in one row: its main choice on the control rail, and a quiet
/// `Customize…` to its left while there is a HUD to customize. The picker is anchored to the
/// rail, so nothing moves when the button comes and goes.
fn hud_row(ui: &Ui, p: Paint) -> El {
    let selected = hud_mode_index(ui.doc.hud(), ui.doc.hud_style());
    let picker = SegmentedControl::new(
        &HUD_MODES.map(|(label, _, _)| label.to_owned()),
        Some(selected),
    )
    .label("Workspace HUD")
    .fit()
    .on_pick(|ui: &mut Ui, index, _: &mut Tx| {
        if let Some((_, mode, _)) = HUD_MODES.get(index) {
            ui.set_hud(*mode);
        }
    })
    .build(p.k)
    .key("hud-mode-picker");
    let customize = ui.doc.hud().then(|| {
        controls::quiet_button(p, "hud-customize", "Customize…", |ui: &mut Ui, _| {
            ui.open_hud_sheet();
        })
    });
    controls::setting_row(
        p,
        "hud",
        "Workspace HUD",
        Some((HUD_MODES[selected].2.to_owned(), p.muted())),
        row()
            .key("hud-controls")
            .gap(8.0)
            .center_y()
            .children(customize)
            .child(picker),
    )
}

fn mouse_follows_focus_row(ui: &Ui, p: Paint) -> El {
    let selected = mouse_follow_index(ui.doc.mouse_follows_focus());
    controls::setting_row(
        p,
        "mouse-follows-focus",
        "Mouse follows focus",
        Some((MOUSE_FOLLOW_MODES[selected].2.to_owned(), p.muted())),
        SegmentedControl::new(
            &MOUSE_FOLLOW_MODES.map(|(label, _, _)| label.to_owned()),
            Some(selected),
        )
        .label("Mouse follows focus")
        .fit()
        .on_pick(|ui: &mut Ui, index, _: &mut Tx| {
            if let Some((_, mode, _)) = MOUSE_FOLLOW_MODES.get(index) {
                ui.edit(|doc| doc.set_mouse_follows_focus(*mode));
            }
        })
        .build(p.k)
        .key("mouse-follows-focus-picker"),
    )
}

/// The menu bar icon, shown with `[labs] menu_bar_icon_style` on: a glyph per display, one
/// glyph, or none.
fn menu_bar_icon_row(ui: &Ui, p: Paint) -> El {
    let selected = menu_bar_icon_index(ui.doc.menu_bar_icon());
    controls::setting_row(
        p,
        "menu-bar-icon",
        "Menu bar icon",
        Some((MENU_BAR_ICONS[selected].2.to_owned(), p.muted())),
        SegmentedControl::new(
            &MENU_BAR_ICONS.map(|(label, _, _)| label.to_owned()),
            Some(selected),
        )
        .label("Menu bar icon")
        .fit()
        .on_pick(|ui: &mut Ui, index, _: &mut Tx| {
            if let Some((_, icon, _)) = MENU_BAR_ICONS.get(index) {
                ui.edit(|doc| doc.set_menu_bar_icon(*icon));
            }
        })
        .build(p.k)
        .key("menu-bar-icon-picker"),
    )
}

/// The Workspace HUD's Customize sheet: a miniature display with the real capsule on the edge
/// it is set to, over the rows that refine it. Changes apply at once; Done only closes.
pub fn hud_sheet(
    ui: &Ui,
    preview: &hud::HudPreview,
    p: Paint,
    reduce_motion: bool,
    layer: &mut Vec<El>,
) {
    let position = ui.doc.hud_position();
    let width = ModalWidth::Medium;
    let miniature = hud::miniature(
        preview,
        position,
        ui.doc.hud_style(),
        (width.px() - 2.0 * METRICS.card_edge(), HUD_PREVIEW_H),
        p,
        reduce_motion,
    );
    let position_row = controls::setting_row(
        p,
        "hud-position",
        "Position",
        (position.is_side() && ui.doc.hud_style() == HudStyle::Layouts).then(|| {
            (
                "On a side it shows Icons; Layouts runs along the top or bottom".to_owned(),
                p.muted(),
            )
        }),
        SegmentedControl::new(
            &HUD_POSITIONS.map(|(label, _)| label.to_owned()),
            Some(hud_position_index(position)),
        )
        .label("Position")
        .fit()
        .on_pick(|ui: &mut Ui, index, _: &mut Tx| {
            if let Some((_, position)) = HUD_POSITIONS.get(index) {
                ui.set_hud_position(*position);
            }
        })
        .build(p.k)
        .key("hud-position-picker"),
    );
    let stay = ui.doc.hud_stay();
    let stay_row = controls::toggle_row(
        p,
        "hud-stay",
        "Keep on screen",
        Some((
            if stay {
                "Always showing; windows tile clear of it"
            } else {
                "Shows when you switch, then fades"
            }
            .to_owned(),
            p.muted(),
        )),
        stay,
        move |ui| ui.set_hud_stay(!stay),
    );
    let displays_row = controls::setting_row(
        p,
        "hud-displays",
        "Workspaces on each display",
        Some((
            "Each display's capsule lists the workspaces assigned to it".to_owned(),
            p.muted(),
        )),
        controls::quiet_button(p, "hud-workspaces", "Workspaces", |ui: &mut Ui, _| {
            ui.select_pane(Pane::Workspaces);
        }),
    );
    let panel = controls::modal_panel(
        p,
        "hud-sheet",
        "Workspace HUD",
        width,
        vec![
            miniature,
            controls::card(p, "card", vec![stay_row, position_row, displays_row]),
        ],
        vec![
            controls::button(
                p,
                "done",
                "Done",
                ButtonKind::Secondary,
                |ui: &mut Ui, _| {
                    ui.close_sheet();
                },
            )
            .default_button(),
        ],
    );
    controls::modal_sheet(p, "hud-sheet", panel, Ui::close_sheet, layer);
}

/// Playground: the Customize sheet open over General, with the capsule on `position`.
pub(crate) fn fixture_hud_sheet(ui: &mut Ui, position: HudPosition) {
    ui.edit(|doc| doc.set_hud_position(position));
    ui.sheet = Some(Sheet::Hud(hud::HudPreview::sample()));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hud_mode_picker_maps_off_icons_and_layouts() {
        assert_eq!(hud_mode_index(false, HudStyle::Icons), 0);
        assert_eq!(hud_mode_index(false, HudStyle::Layouts), 0);
        assert_eq!(hud_mode_index(true, HudStyle::Icons), 1);
        assert_eq!(hud_mode_index(true, HudStyle::Layouts), 2);
    }

    #[test]
    fn hud_position_picker_maps_bottom_and_top() {
        assert_eq!(hud_position_index(HudPosition::Bottom), 0);
        assert_eq!(hud_position_index(HudPosition::Top), 1);
    }

    #[test]
    fn mouse_follow_picker_maps_off_display_and_window() {
        assert_eq!(mouse_follow_index(MouseFollowsFocus::Off), 0);
        assert_eq!(mouse_follow_index(MouseFollowsFocus::Display), 1);
        assert_eq!(mouse_follow_index(MouseFollowsFocus::Window), 2);
    }

    #[test]
    fn menu_bar_icon_picker_maps_full_compact_and_off() {
        assert_eq!(menu_bar_icon_index(MenuBarIcon::Full), 0);
        assert_eq!(menu_bar_icon_index(MenuBarIcon::Compact), 1);
        assert_eq!(menu_bar_icon_index(MenuBarIcon::Off), 2);
    }

    #[test]
    fn customize_opens_the_hud_sheet_and_only_while_there_is_a_hud() {
        let p = Paint::new(true, 2.0);
        let mut ui = crate::playground::blank_fixture();
        crate::playground::reset_fixture(&mut ui, Pane::General);
        assert!(ui.doc.hud());
        assert!(ui.sheet.is_none());

        ui.open_hud_sheet();
        assert!(matches!(ui.sheet, Some(Sheet::Hud(_))));
        // The sheet's Position control saves at once, with the sheet still open.
        ui.set_hud_position(HudPosition::Top);
        assert_eq!(ui.doc.hud_position(), HudPosition::Top);
        assert!(ui.sheet.is_some());
        let mut layer = Vec::new();
        crate::panes::sheet(&ui, p, false, &mut layer);
        assert_eq!(layer.len(), 2, "a scrim and the sheet");

        // Its link to Workspaces leaves the sheet behind.
        ui.select_pane(Pane::Workspaces);
        assert!(ui.sheet.is_none());

        // With the HUD off there is nothing to customize, and a sheet left open closes.
        ui.select_pane(Pane::General);
        ui.open_hud_sheet();
        ui.set_hud(None);
        assert!(ui.sheet.is_none());
        ui.open_hud_sheet();
        assert!(ui.sheet.is_none(), "no HUD, no sheet");
        let _ = pane(&ui, p);
    }

    #[test]
    fn keep_on_screen_round_trips_and_omits_its_off_default() {
        let mut ui = crate::playground::blank_fixture();
        assert!(!ui.doc.hud_stay());
        ui.set_hud_stay(true);
        assert!(ui.doc.hud_stay());
        let on = ui.doc.source();
        assert!(on.contains("hud_stay = true"), "{on}");
        assert!(spaceman::config::Config::parse(&on).unwrap().hud_stay());
        ui.set_hud_stay(false);
        assert!(!ui.doc.source().contains("hud_stay"));
    }

    #[test]
    fn pause_on_builtin_display_toggle_round_trips_and_omits_false_default() {
        let mut ui = crate::playground::blank_fixture();
        crate::playground::reset_fixture(&mut ui, Pane::General);
        assert!(!ui.doc.pause_on_builtin_display());

        ui.toggle_pause_on_builtin_display();
        assert!(ui.doc.pause_on_builtin_display());
        let enabled = ui.doc.source();
        assert!(
            enabled.contains("pause_on_builtin_display = true"),
            "{enabled}"
        );
        assert!(
            spaceman::config::Config::parse(&enabled)
                .unwrap()
                .pause_on_builtin_display()
        );

        ui.toggle_pause_on_builtin_display();
        assert!(!ui.doc.pause_on_builtin_display());
        let disabled = ui.doc.source();
        assert!(!disabled.contains("pause_on_builtin_display"), "{disabled}");
        assert!(
            !spaceman::config::Config::parse(&disabled)
                .unwrap()
                .pause_on_builtin_display()
        );
    }
}
