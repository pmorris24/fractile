//! Workspaces pane: display assignments for numbered and referenced named workspaces.

mod displays;
pub(crate) mod remembered;

pub(crate) use displays::{
    Choice, ConnectedDisplay, THIRD_FIXTURE_ID, choices, fixture_long_name, fixture_pair,
    fixture_quad, fixture_three, index_of,
};

use rune::{El, Tx, text};
use rune_kit::SegmentedControl;
use spaceman::{WorkspaceId, shortcuts::Shortcuts};

use crate::{
    Pane, Ui,
    controls::{self, Paint},
    panes::shortcuts::switch_keys,
    rules::Doc,
    view::scroll_body,
};

use displays::{MAIN, SECONDARY, TERTIARY};

/// The selector of the display macOS marks as main.
pub(crate) const MAIN_SELECTOR: &str = MAIN;

/// How often the pane reads the connected displays while it is on screen, in seconds.
const DISPLAY_REFRESH_SECONDS: f64 = 1.0;

/// The label of the extra segment a workspace gets when its saved display is unplugged.
const UNPLUGGED: &str = "Not connected";

/// Built-in default display for `workspace` when `config.toml` sets no explicit `monitor`.
const fn default_monitor(workspace: WorkspaceId) -> &'static str {
    match workspace.number() {
        Some(n) if n >= 8 => TERTIARY,
        Some(n) if n >= 5 => SECONDARY,
        _ => MAIN,
    }
}

/// Which display `workspace` currently targets: explicit override from `config.toml`, or its
/// built-in default (1–4 and named workspaces on Main, 5–7 on Secondary, 8–10 on Tertiary).
pub(crate) fn effective_monitor(doc: &Doc, workspace: WorkspaceId) -> String {
    doc.monitor(workspace)
        .unwrap_or_else(|| default_monitor(workspace).to_owned())
}

/// Reads the connected displays when Settings › Workspaces is on screen, about once a second, so
/// plugging a display in shows its segment without reopening Settings.
pub(crate) fn poll_displays(ui: &mut Ui, clock: f64) -> bool {
    if ui.fixture || !matches!(ui.pane, Pane::Rules | Pane::Workspaces) {
        return false;
    }
    if ui
        .displays_at
        .is_some_and(|read| clock - read < DISPLAY_REFRESH_SECONDS)
    {
        return false;
    }
    ui.displays_at = Some(clock);
    let Some(connected) = read_displays() else {
        return false;
    };
    let first = ui.known_displays.is_none();
    let known = ui.known_displays.get_or_insert_with(remembered::load);
    // macOS's file is read only when the displays change, not on every poll.
    let look = first || connected != ui.displays;
    let noted = known.update(&connected, look, remembered::from_macos);
    if noted {
        remembered::save(known);
    }
    if connected == ui.displays {
        return noted;
    }
    ui.displays = connected;
    true
}

#[cfg(target_os = "macos")]
fn read_displays() -> Option<Vec<displays::ConnectedDisplay>> {
    spaceman::macos::outputs()
        .ok()
        .map(|outputs| displays::connected(&outputs))
}

#[cfg(not(target_os = "macos"))]
fn read_displays() -> Option<Vec<displays::ConnectedDisplay>> {
    None
}

/// One card: a display and the workspaces assigned to it.
pub(crate) struct Section {
    pub(crate) key: String,
    pub(crate) title: String,
    pub(crate) caption: String,
    pub(crate) workspaces: Vec<WorkspaceId>,
}

/// The pane's cards in display order, then the displays a saved selector names but that are not
/// plugged in, which keep their assignment instead of losing it.
pub(crate) fn sections(
    doc: &Doc,
    choices: &[Choice],
    workspaces: Vec<WorkspaceId>,
) -> Vec<Section> {
    let mut buckets: Vec<Vec<WorkspaceId>> = (0..choices.len()).map(|_| Vec::new()).collect();
    let mut unplugged = Vec::new();
    for workspace in workspaces {
        match displays::index_of(choices, &effective_monitor(doc, workspace)) {
            Some(index) => buckets[index].push(workspace),
            None => unplugged.push(workspace),
        }
    }
    let mut sections = Vec::new();
    for (index, choice) in choices.iter().enumerate() {
        let workspaces = std::mem::take(&mut buckets[index]);
        if workspaces.is_empty() && !choice.connected {
            continue;
        }
        sections.push(Section {
            key: format!("display-{index}"),
            title: choice.title.clone(),
            caption: choice.caption.clone(),
            workspaces,
        });
    }
    if !unplugged.is_empty() {
        sections.push(Section {
            key: "unplugged-display".to_owned(),
            title: "Disconnected displays".to_owned(),
            caption: "Saved in config.toml".to_owned(),
            workspaces: unplugged,
        });
    }
    sections
}

pub fn pane(ui: &Ui, p: Paint) -> El {
    let choices = displays::choices(&ui.displays);
    let shortcuts = ui.doc.shortcuts();
    let sections = sections(
        &ui.doc,
        &choices,
        ui.doc.workspaces_with_referenced(WorkspaceId::numbered()),
    );
    let cards: Vec<El> = sections
        .iter()
        .map(|section| {
            let rows: Vec<El> = if section.workspaces.is_empty() {
                vec![empty_display_row(p)]
            } else {
                section
                    .workspaces
                    .iter()
                    .map(|workspace| {
                        let target = effective_monitor(&ui.doc, *workspace);
                        monitor_row(p, &choices, &shortcuts, *workspace, &target)
                    })
                    .collect()
            };
            controls::section_with(
                p,
                &section.key,
                &section.title,
                controls::caption(p, &section.caption),
                rows,
            )
        })
        .collect();
    scroll_body("workspaces-body", controls::page("sections", cards))
}

fn empty_display_row(p: Paint) -> El {
    controls::row_frame("empty", false).child(
        text("No workspaces assigned")
            .key("label")
            .font(p.row_title())
            .color(p.faint()),
    )
}

/// The `config.toml` override picking `chosen` writes; `None` clears it, so picking a
/// workspace's built-in default leaves no redundant entry behind.
pub(crate) fn override_for(workspace: WorkspaceId, chosen: &str) -> Option<String> {
    (chosen != default_monitor(workspace)).then(|| chosen.to_owned())
}

/// The row's muted caption: the display the row differs from, named as its card and picker do,
/// or the fallback note for an unplugged one. An unplugged preferred display always falls back to
/// Main in `DisplayInventory::resolve`, even for workspaces 5–10 whose built-in default is
/// Secondary or Tertiary.
///
/// `effective` is the row's saved `monitor` text, which may be a UUID or a name, so both it and
/// the built-in default resolve to displays first: a workspace saved by the Secondary display's
/// UUID picks that display, and needs no note about a default it already is.
fn row_description(choices: &[Choice], workspace: WorkspaceId, effective: &str) -> Option<String> {
    let Some(current) = displays::index_of(choices, effective) else {
        return Some("Not connected · uses Main until it returns".to_owned());
    };
    let default = default_monitor(workspace);
    let built_in = displays::index_of(choices, default);
    if built_in == Some(current) {
        return None;
    }
    let name = built_in.map_or_else(
        || {
            if default == TERTIARY {
                "Tertiary"
            } else if default == SECONDARY {
                "Secondary"
            } else {
                "Main"
            }
        },
        |index| choices[index].note.as_str(),
    );
    Some(format!("Default: {name}"))
}

/// The picker's segments and the selected one: every display's label, then `Not connected` when
/// `effective` names a display that is not plugged in.
fn picker(choices: &[Choice], effective: &str) -> (Vec<String>, usize) {
    let mut labels: Vec<String> = choices.iter().map(|choice| choice.label.clone()).collect();
    let selected = displays::index_of(choices, effective).unwrap_or_else(|| {
        labels.push(UNPLUGGED.to_owned());
        choices.len()
    });
    (labels, selected)
}

/// A row's title. Workspace 10 also names the keys that switch to it, because its key is `0`,
/// not its number. They are read from `[shortcuts]`, so they follow the base modifier and a
/// rebinding, and the title says nothing when no shortcut switches to it. Issue 350.
fn row_title(shortcuts: &Shortcuts, workspace: WorkspaceId) -> String {
    let keys = (workspace.number() == Some(10))
        .then(|| switch_keys(shortcuts, workspace))
        .flatten();
    match keys {
        Some(keys) => format!("Workspace {workspace} ({keys})"),
        None => format!("Workspace {workspace}"),
    }
}

fn monitor_row(
    p: Paint,
    choices: &[Choice],
    shortcuts: &Shortcuts,
    workspace: WorkspaceId,
    effective: &str,
) -> El {
    let (labels, selected) = picker(choices, effective);
    let selectors: Vec<String> = choices
        .iter()
        .map(|choice| choice.selector.clone())
        .collect();
    let picker = SegmentedControl::new(&labels, Some(selected))
        .fit()
        .on_pick(move |ui: &mut Ui, index, _: &mut Tx| {
            let Some(chosen) = selectors.get(index) else {
                return;
            };
            let monitor = override_for(workspace, chosen);
            ui.edit(|doc| doc.set_monitor(workspace, monitor.as_deref()));
        })
        .build(p.k)
        .key("monitor");
    let title = row_title(shortcuts, workspace);
    let description = row_description(choices, workspace, effective).map(|copy| (copy, p.muted()));
    controls::setting_row(
        p,
        &format!("workspace-{workspace}"),
        &title,
        description,
        picker,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use displays::{FOURTH_FIXTURE_ID, choices, fixture_long_name, index_of};

    #[test]
    fn workspace_monitor_defaults_split_main_and_secondary() {
        let doc = Doc::parse("version = 1\n");
        for number in 1..=4 {
            let ws = WorkspaceId::new(number).unwrap();
            assert_eq!(default_monitor(ws), MAIN);
            assert_eq!(effective_monitor(&doc, ws), MAIN);
        }
        for number in 8..=10 {
            let ws = WorkspaceId::new(number).unwrap();
            assert_eq!(default_monitor(ws), TERTIARY);
            assert_eq!(effective_monitor(&doc, ws), TERTIARY);
        }
        for number in 5..=7 {
            let ws = WorkspaceId::new(number).unwrap();
            assert_eq!(default_monitor(ws), SECONDARY);
            assert_eq!(effective_monitor(&doc, ws), SECONDARY);
        }
        let named = WorkspaceId::named('M').unwrap();
        assert_eq!(default_monitor(named), MAIN);
        assert_eq!(effective_monitor(&doc, named), MAIN);

        let mut overridden = doc;
        overridden.set_monitor(WorkspaceId::new(2).unwrap(), Some(SECONDARY));
        overridden.set_monitor(WorkspaceId::new(7).unwrap(), Some(MAIN));
        assert_eq!(
            effective_monitor(&overridden, WorkspaceId::new(2).unwrap()),
            SECONDARY
        );
        assert_eq!(
            effective_monitor(&overridden, WorkspaceId::new(7).unwrap()),
            MAIN
        );
    }

    #[test]
    fn two_displays_keep_the_crisp_main_and_secondary_picker() {
        let choices = choices(&fixture_pair());
        assert_eq!(
            choices
                .iter()
                .map(|choice| choice.selector.as_str())
                .collect::<Vec<_>>(),
            [MAIN, SECONDARY]
        );
        assert_eq!(
            picker(&choices, MAIN).0,
            ["Main".to_owned(), "Secondary".to_owned()]
        );
        assert_eq!(picker(&choices, SECONDARY).1, 1);
        // The section captions name the actual displays.
        assert_eq!(
            choices[0].caption,
            "Built-in Retina Display · Default for 1–4"
        );
        // With two displays, Secondary also hosts the tertiary workspaces.
        assert_eq!(choices[1].caption, "Studio Display · Default for 5–10");
    }

    #[test]
    fn three_displays_offer_every_display_and_pick_the_third_by_uuid() {
        let choices = choices(&fixture_three());
        assert_eq!(
            choices
                .iter()
                .map(|choice| choice.selector.as_str())
                .collect::<Vec<_>>(),
            [MAIN, SECONDARY, TERTIARY]
        );
        // Two identically named external monitors are told apart by their own names.
        assert_eq!(
            picker(&choices, THIRD_FIXTURE_ID).0,
            [
                "Main".to_owned(),
                "Studio Display 1".to_owned(),
                "Studio Display 2".to_owned()
            ]
        );
        assert_eq!(picker(&choices, THIRD_FIXTURE_ID).1, 2);
        assert_eq!(choices[1].title, "Studio Display 1");
        assert_eq!(choices[1].caption, "Secondary · Default for 5–7");
        assert_eq!(choices[2].title, "Studio Display 2");
        assert_eq!(choices[2].caption, "Display 3 · Default for 8–10");

        // Selecting the third display writes `tertiary`, exactly as Option+Shift+Tab does.
        let mut doc = Doc::parse("version = 1\n");
        let chosen = choose(&choices, 2, WorkspaceId::new(6).unwrap());
        doc.set_monitor(WorkspaceId::new(6).unwrap(), chosen.as_deref());
        assert_eq!(
            doc.monitor(WorkspaceId::new(6).unwrap()).as_deref(),
            Some(TERTIARY)
        );
        // Rendering that saved selector resolves it back to the display's friendly name.
        let saved = effective_monitor(&doc, WorkspaceId::new(6).unwrap());
        assert_eq!(saved, TERTIARY);
        assert_eq!(index_of(&choices, &saved), Some(2));
        assert_eq!(picker(&choices, &saved).0[2], "Studio Display 2");
    }

    #[test]
    fn four_displays_number_the_picker_and_keep_names_in_the_headings() {
        let choices = choices(&fixture_quad());
        assert_eq!(
            choices
                .iter()
                .map(|choice| choice.label.as_str())
                .collect::<Vec<_>>(),
            ["Main", "Display 2", "Display 3", "Display 4"]
        );
        assert_eq!(
            choices
                .iter()
                .map(|choice| choice.title.as_str())
                .collect::<Vec<_>>(),
            [
                "Main display",
                "Studio Display 1",
                "Studio Display 2",
                "LG UltraFine 5K"
            ]
        );
        assert_eq!(
            choose(&choices, 3, WorkspaceId::new(9).unwrap()).as_deref(),
            Some(FOURTH_FIXTURE_ID)
        );

        // A row's note names the display it differs from by its card, not its numbered segment.
        assert_eq!(
            row_description(&choices, WorkspaceId::new(9).unwrap(), MAIN).as_deref(),
            Some("Default: Studio Display 2")
        );
    }

    #[test]
    fn a_saved_uuid_for_the_main_display_resolves_to_main() {
        let choices = choices(&fixture_pair());
        let main_id = choices[0].id.clone().unwrap();
        assert_eq!(index_of(&choices, &main_id), Some(0));
        let secondary_id = choices[1].id.clone().unwrap();
        assert_eq!(index_of(&choices, &secondary_id), Some(1));
    }

    #[test]
    fn unplugged_display_keeps_its_assignment_and_offers_not_connected() {
        // A UUID that no connected display answers to, as after unplugging the third monitor.
        let choices = choices(&fixture_pair());
        let saved = THIRD_FIXTURE_ID;
        assert_eq!(index_of(&choices, saved), None);
        let (labels, selected) = picker(&choices, saved);
        assert_eq!(selected, choices.len());
        assert_eq!(labels.last().map(String::as_str), Some(UNPLUGGED));

        let mut doc = Doc::parse("version = 1\n");
        doc.set_monitor(WorkspaceId::new(7).unwrap(), Some(saved));
        assert_eq!(
            effective_monitor(&doc, WorkspaceId::new(7).unwrap()),
            saved,
            "rendering keeps the config.toml entry"
        );
    }

    #[test]
    fn a_workspace_saved_by_the_secondary_displays_uuid_needs_no_default_note() {
        // Settings never writes this (choosing Secondary saves `secondary`), but
        // `spaceman workspaces assign` and a hand-edited config.toml do, and a display can
        // become Secondary after its UUID was saved. Issue 337.
        let displays = fixture_three();
        let choices = choices(&displays);
        let six = WorkspaceId::new(6).unwrap();
        let seven = WorkspaceId::new(7).unwrap();
        let mut doc = Doc::parse("version = 1\n");
        doc.set_monitor(six, Some(displays[1].id.as_str()));
        doc.set_monitor(seven, Some(THIRD_FIXTURE_ID));

        // Row 6 sits in the Secondary card and picks it: the UUID resolves to that display, so
        // the row says no default it already is.
        let saved = effective_monitor(&doc, six);
        let (labels, selected) = picker(&choices, &saved);
        assert_eq!(selected, 1);
        assert_eq!(labels[selected], "Studio Display 1");
        assert_eq!(row_description(&choices, six, &saved), None);
        assert_eq!(
            sections(&doc, &choices, vec![six, seven])[1].workspaces,
            [six]
        );

        // Row 7 on the third display keeps its note, which names the display that differs.
        let saved = effective_monitor(&doc, seven);
        let (labels, selected) = picker(&choices, &saved);
        assert_eq!(labels[selected], "Studio Display 2");
        assert_eq!(
            row_description(&choices, seven, &saved).as_deref(),
            Some("Default: Studio Display 1")
        );
    }

    #[test]
    fn a_two_display_secondary_uuid_is_still_just_secondary() {
        // With two displays Secondary keeps its short label, and its UUID resolves to it: no
        // note, because the row already says the display it is set to.
        let displays = fixture_pair();
        let choices = choices(&displays);
        let seven = WorkspaceId::new(7).unwrap();
        let mut doc = Doc::parse("version = 1\n");
        doc.set_monitor(seven, Some(displays[1].id.as_str()));
        let saved = effective_monitor(&doc, seven);
        assert_eq!(
            picker(&choices, &saved),
            (vec!["Main".to_owned(), "Secondary".to_owned()], 1)
        );
        assert_eq!(row_description(&choices, seven, &saved), None);

        // Main's UUID is a workspace 1–4 row's default too, and the other display's is not.
        let two = WorkspaceId::new(2).unwrap();
        let mut doc = Doc::parse("version = 1\n");
        doc.set_monitor(two, Some(displays[0].id.as_str()));
        let saved = effective_monitor(&doc, two);
        assert_eq!(row_description(&choices, two, &saved), None);
        doc.set_monitor(two, Some(displays[1].id.as_str()));
        let saved = effective_monitor(&doc, two);
        assert_eq!(
            row_description(&choices, two, &saved).as_deref(),
            Some("Default: Main")
        );
    }

    #[test]
    fn picking_a_built_in_default_clears_the_override() {
        let choices = choices(&fixture_three());
        let seven = WorkspaceId::new(7).unwrap();
        assert_eq!(choose(&choices, 0, seven).as_deref(), Some(MAIN));
        assert_eq!(choose(&choices, 1, seven), None);
        let three = WorkspaceId::new(3).unwrap();
        assert_eq!(choose(&choices, 0, three), None);
        assert_eq!(choose(&choices, 1, three).as_deref(), Some(SECONDARY));
        // The extra segment is not a choice: picking it leaves the saved assignment alone.
        assert_eq!(choose(&choices, choices.len(), seven), None);
    }

    #[test]
    fn secondary_is_offered_even_when_only_one_display_is_connected() {
        let single = vec![fixture_pair()[0].clone()];
        let choices = choices(&single);
        assert_eq!(
            choices
                .iter()
                .map(|choice| choice.selector.as_str())
                .collect::<Vec<_>>(),
            [MAIN, SECONDARY]
        );
        assert!(!choices[1].connected);
        assert_eq!(choices[1].caption, "Default for 5–10 · main when unplugged");
        assert_eq!(
            picker(&choices, SECONDARY).0,
            ["Main".to_owned(), "Secondary".to_owned()]
        );
    }

    #[test]
    fn a_long_display_name_stays_in_its_heading_and_only_its_segment_is_numbered() {
        // Issue 349: "Built-in Retina Display" as the third of three displays lost its name
        // everywhere on the pane.
        let displays = fixture_long_name();
        let choices = choices(&displays);
        let seen: Vec<(&str, &str, &str)> = choices
            .iter()
            .map(|choice| {
                (
                    choice.title.as_str(),
                    choice.label.as_str(),
                    choice.caption.as_str(),
                )
            })
            .collect();
        assert_eq!(
            seen,
            [
                ("Main display", "Main", "Studio Display · Default for 1–4"),
                // Control: a name that fits keeps its segment.
                (
                    "LG UltraFine 5K",
                    "LG UltraFine 5K",
                    "Secondary · Default for 5–7"
                ),
                // The caption ties the numbered segment to the card that names the screen.
                (
                    "Built-in Retina Display",
                    "Display 3",
                    "Display 3 · Default for 8–10"
                ),
            ]
        );
        // A row that differs from this display names it, as its card does.
        let mut swapped = displays;
        swapped[1].name = "Built-in Retina Display".to_owned();
        swapped[2].name = "Studio Display".to_owned();
        swapped[0].name = "LG UltraFine 5K".to_owned();
        let choices = super::displays::choices(&swapped);
        assert_eq!(choices[1].title, "Built-in Retina Display");
        assert_eq!(choices[1].label, "Display 2");
        assert_eq!(choices[1].caption, "Display 2 · Default for 5–7");
        assert_eq!(choices[2].title, "Studio Display");
        assert_eq!(choices[2].label, "Studio Display");
        assert_eq!(
            row_description(&choices, WorkspaceId::new(6).unwrap(), MAIN).as_deref(),
            Some("Default: Built-in Retina Display")
        );
        // A saved UUID or name still resolves to the display whatever its segment says.
        assert_eq!(index_of(&choices, "Built-in Retina Display"), Some(1));
        assert_eq!(picker(&choices, THIRD_FIXTURE_ID).1, 2);
    }

    #[test]
    fn two_long_names_that_macos_suffixed_keep_their_names_and_an_unnamed_display_its_position() {
        // macOS tells two identical monitors apart itself ("Studio Display (1)"), which makes
        // each name unique and longer than a segment holds.
        let mut displays = fixture_three();
        displays[1].name = "Studio Display (1)".to_owned();
        displays[2].name = "Studio Display (2)".to_owned();
        let suffixed = choices(&displays);
        assert_eq!(suffixed[1].title, "Studio Display (1)");
        assert_eq!(suffixed[2].title, "Studio Display (2)");
        assert_eq!(
            picker(&suffixed, MAIN).0,
            ["Main", "Display 2", "Display 3"].map(str::to_owned)
        );

        // Control: a display that reports no name has only its position to show.
        displays[2].name = String::new();
        let unnamed = choices(&displays);
        assert_eq!(unnamed[2].title, "Display 3");
        assert_eq!(unnamed[2].label, "Display 3");

        // Control: four displays number every segment past Main and keep every name.
        let quad = choices(&fixture_quad());
        assert_eq!(quad[1].caption, "Display 2 · Default for 5–7");
        assert_eq!(quad[3].title, "LG UltraFine 5K");
        assert_eq!(quad[3].label, "Display 4");
    }

    #[test]
    fn workspace_10_names_the_keys_that_really_switch_to_it() {
        // Issue 350: the title said "(⌥0)" whatever `[shortcuts]` said.
        let shortcuts = |body: &str| Doc::parse(&format!("version = 1\n{body}")).shortcuts();
        let ten = WorkspaceId::new(10).unwrap();
        // Control: the default base.
        assert_eq!(row_title(&shortcuts(""), ten), "Workspace 10 (⌥0)");
        assert_eq!(
            row_title(&shortcuts("[shortcuts]\nmodifier = \"ctrl+alt\"\n"), ten),
            "Workspace 10 (⌃⌥0)"
        );
        // Rebound: the chord that does it now.
        assert_eq!(
            row_title(
                &shortcuts(
                    "[shortcuts.bindings]\n\"alt+0\" = \"none\"\n\"cmd+alt+0\" = \"switch 10\"\n"
                ),
                ten
            ),
            "Workspace 10 (⌥⌘0)"
        );
        // Nothing switches to it: unbound, taken by another command, or no built-ins at all.
        for body in [
            "[shortcuts.bindings]\n\"alt+0\" = \"none\"\n",
            "[shortcuts.bindings]\n\"alt+0\" = \"switch 1\"\n",
            "[shortcuts]\ndefaults = false\n[shortcuts.bindings]\n\"alt+1\" = \"switch 1\"\n",
        ] {
            assert_eq!(row_title(&shortcuts(body), ten), "Workspace 10", "{body}");
        }
        // Every other workspace's key is its own name, so its title stays bare.
        let base = shortcuts("[shortcuts]\nmodifier = \"ctrl+alt\"\n");
        assert_eq!(
            row_title(&base, WorkspaceId::new(9).unwrap()),
            "Workspace 9"
        );
        assert_eq!(
            row_title(&base, WorkspaceId::named('M').unwrap()),
            "Workspace M"
        );
    }

    /// The `monitor` override the picker writes for `index`; `None` clears it.
    fn choose(choices: &[Choice], index: usize, workspace: WorkspaceId) -> Option<String> {
        override_for(workspace, &choices.get(index)?.selector)
    }

    /// The pane's cards for `doc`: workspace 9 assigned in every case here.
    fn cards(doc: &Doc, displays: Vec<displays::ConnectedDisplay>) -> Vec<Section> {
        let choices = choices(&displays);
        sections(
            doc,
            &choices,
            doc.workspaces_with_referenced(WorkspaceId::numbered()),
        )
    }

    fn assigned(display: &str) -> Doc {
        let mut doc = Doc::parse("version = 1\n");
        doc.set_monitor(WorkspaceId::new(9).unwrap(), Some(display));
        doc
    }

    #[test]
    fn every_connected_display_gets_its_own_card() {
        let sections = cards(&assigned(THIRD_FIXTURE_ID), fixture_three());
        let titles: Vec<&str> = sections
            .iter()
            .map(|section| section.title.as_str())
            .collect();
        assert_eq!(
            titles,
            ["Main display", "Studio Display 1", "Studio Display 2"]
        );
        assert_eq!(sections[0].workspaces.len(), 4);
        assert_eq!(sections[1].workspaces.len(), 3);
        assert_eq!(
            sections[2].workspaces,
            [8, 9, 10].map(|n| WorkspaceId::new(n).unwrap()),
            "workspaces 8-10 sit on the third display"
        );
        assert_eq!(sections[2].caption, "Display 3 · Default for 8–10");
    }

    #[test]
    fn an_unplugged_display_keeps_its_own_card_and_assignment() {
        let doc = assigned(THIRD_FIXTURE_ID);
        let sections = cards(&doc, fixture_pair());
        let titles: Vec<&str> = sections
            .iter()
            .map(|section| section.title.as_str())
            .collect();
        assert_eq!(
            titles,
            ["Main display", "Secondary display", "Disconnected displays"]
        );
        assert_eq!(sections[2].workspaces, [WorkspaceId::new(9).unwrap()]);
        assert_eq!(sections[2].caption, "Saved in config.toml");
        assert_eq!(
            doc.monitor(WorkspaceId::new(9).unwrap()).as_deref(),
            Some(THIRD_FIXTURE_ID),
            "the assignment survives rendering"
        );
    }

    #[test]
    fn disconnected_workspace_6_to_10_names_main_as_fallback_and_restores_on_reconnect() {
        let seven = WorkspaceId::new(7).unwrap();
        let mut doc = Doc::parse("version = 1\n");
        doc.set_monitor(seven, Some(THIRD_FIXTURE_ID));

        // Third display unplugged while Main and Secondary remain connected.
        let pair_choices = choices(&fixture_pair());
        let saved = effective_monitor(&doc, seven);
        let (_, selected) = picker(&pair_choices, &saved);
        assert_eq!(selected, pair_choices.len());
        assert_eq!(
            row_description(&pair_choices, seven, &saved).as_deref(),
            Some("Not connected · uses Main until it returns")
        );
        assert_eq!(doc.monitor(seven).as_deref(), Some(THIRD_FIXTURE_ID));

        // Reconnecting the third display resumes showing it without losing the assignment, and
        // the note names the display it differs from as the picker does.
        let three_choices = choices(&fixture_three());
        let (_, reconnected) = picker(&three_choices, &saved);
        assert_eq!(reconnected, 2);
        assert_eq!(
            row_description(&three_choices, seven, &saved).as_deref(),
            Some("Default: Studio Display 1")
        );

        // An ordinary workspace 7 on Secondary still shows Secondary with no override note.
        let default_doc = Doc::parse("version = 1\n");
        let default_effective = effective_monitor(&default_doc, seven);
        let (_, default_selected) = picker(&pair_choices, &default_effective);
        assert_eq!(default_selected, 1);
        assert_eq!(
            row_description(&pair_choices, seven, &default_effective),
            None
        );
    }

    #[test]
    fn every_connected_display_has_a_card_by_default() {
        let doc = Doc::parse("version = 1\n");
        let titles = |displays| -> Vec<String> {
            cards(&doc, displays).into_iter().map(|s| s.title).collect()
        };
        let labels: Vec<String> = choices(&fixture_quad())
            .into_iter()
            .map(|c| c.label)
            .collect();
        assert_eq!(
            titles(fixture_three()),
            ["Main display", "Studio Display 1", "Studio Display 2"]
        );
        assert_eq!(
            titles(fixture_quad()),
            [
                "Main display",
                "Studio Display 1",
                "Studio Display 2",
                "LG UltraFine 5K"
            ]
        );
        assert_eq!(labels, ["Main", "Display 2", "Display 3", "Display 4"]);

        let three_sections = cards(&doc, fixture_three());
        let tertiary = [8, 9, 10].map(|n| WorkspaceId::new(n).unwrap());
        assert_eq!(three_sections[2].workspaces, tertiary);
        assert_eq!(three_sections[2].caption, "Display 3 · Default for 8–10");

        // Assigning workspace 6 to the third display and moving it back leaves the card in place.
        let six = WorkspaceId::new(6).unwrap();
        let mut moved = Doc::parse("version = 1\n");
        moved.set_monitor(six, Some(THIRD_FIXTURE_ID));
        assert_eq!(
            cards(&moved, fixture_three())[2].workspaces,
            [6, 8, 9, 10].map(|n| WorkspaceId::new(n).unwrap())
        );
        moved.set_monitor(six, None);
        let after_move = cards(&moved, fixture_three());
        assert_eq!(
            after_move
                .iter()
                .map(|s| s.title.as_str())
                .collect::<Vec<_>>(),
            ["Main display", "Studio Display 1", "Studio Display 2"]
        );
        assert_eq!(after_move[2].workspaces, tertiary);

        // Control: a disconnected third display with no saved workspaces gets no card.
        assert_eq!(
            titles(fixture_pair()),
            ["Main display", "Secondary display"]
        );

        // Rendering the pane with an empty connected display card succeeds cleanly.
        let mut ui = crate::playground::blank_fixture();
        ui.doc = doc;
        ui.displays = fixture_three();
        let p = crate::controls::Paint::new(true, 2.0);
        let _ = pane(&ui, p);
    }

    #[test]
    fn a_name_two_extra_displays_share_resolves_where_the_engine_puts_it() {
        use spaceman::macos::{DisplayOutput, DisplayOutputs};
        use spaceman::{DisplaySelector, Rect};
        // The right-hand screen is discovered first, the left-hand one sorts first. Issue 420.
        let display = |id: &str, name: &str, x, is_main| DisplayOutput {
            id: id.into(),
            name: name.into(),
            frame: Rect {
                x,
                y: 0,
                width: 1000,
                height: 800,
            },
            is_main,
        };
        let outputs = DisplayOutputs::new(vec![
            display("center-id", "Main Panel", 0, true),
            display("second-id", "Second Panel", 1000, false),
            display("right-id", "External Panel", 2000, false),
            display("left-id", "External Panel", -1000, false),
        ])
        .unwrap();
        let choices = choices(&displays::connected(&outputs));
        let nine = WorkspaceId::new(9).unwrap();
        for saved in [
            "right-id",
            "left-id",
            "External Panel",
            "Main Panel",
            "main",
        ] {
            let doc = Doc::parse(&format!(
                "version = 1\n[[workspaces]]\nworkspace = 9\nmonitor = '{saved}'\n"
            ));
            let monitor = effective_monitor(&doc, nine);
            let engine = &outputs.resolve(&DisplaySelector::parse(&monitor)).id;
            let (_, checked) = picker(&choices, &monitor);
            assert_eq!(choices[checked].id.as_ref(), Some(engine), "{saved}");
            let cards = sections(&doc, &choices, vec![nine]);
            let card = cards.iter().find(|card| card.workspaces.contains(&nine));
            assert_eq!(card.map(|card| &card.title), Some(&choices[checked].title));
        }
    }
}
