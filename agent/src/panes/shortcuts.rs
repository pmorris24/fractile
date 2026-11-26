//! Shortcuts pane: every global shortcut with its chord from `[shortcuts]`. Pressing a
//! shortcut's keys records a new chord (`recorder`), the base modifier moves every built-in
//! shortcut at once, and each change is saved to `config.toml` and registered live.

mod catalog;
mod recorder;
#[cfg(test)]
mod tests;
mod workspaces;

use fractile::{
    WorkspaceId,
    config::LabFeature,
    shortcuts::{Chord, Modifiers, Shortcuts},
};
use rune::{El, Shadow, col, row, text};
use rune_kit::SegmentedControl;

pub use recorder::Capture;
pub(crate) use workspaces::switch_keys;

/// The first chord that switches to `workspace`, as the setup tour asks for it.
pub(crate) fn switch_chord(shortcuts: &Shortcuts, workspace: WorkspaceId) -> Option<Chord> {
    workspaces::active_chords(shortcuts, workspaces::WorkspaceKind::Switch, workspace)
        .into_iter()
        .next()
}

/// The first chord that runs the catalog's action `id` ("float", "fullscreen").
pub(crate) fn action_chord(
    shortcuts: &Shortcuts,
    named: &[WorkspaceId],
    id: &str,
) -> Option<Chord> {
    catalog::find(id)?
        .chords(shortcuts, named)
        .into_iter()
        .next()
}

/// A physical key press read as the recorder reads it: the modifiers held, and the chord it
/// spells on the active keyboard layout.
pub(crate) use catalog::PRESETS;
pub(crate) use recorder::{modifiers as held_modifiers, pressed_chord};

use crate::{
    Ui,
    controls::{self, ButtonKind, KEYS, Paint, keycap, roll, tray},
    view::scroll_body,
};
use catalog::Action;

pub fn pane(ui: &Ui, p: Paint) -> El {
    let shortcuts = ui.doc.shortcuts();
    let named = recorder::enabled_named_workspaces(ui);
    let mut workspace_rows = family_rows(p, &shortcuts, &named);
    workspace_rows.extend(action_rows(
        ui,
        p,
        &shortcuts,
        &named,
        catalog::WORKSPACE_ACTIONS,
    ));
    if shortcuts.workspace_steps {
        workspace_rows.extend(action_rows(
            ui,
            p,
            &shortcuts,
            &named,
            catalog::WORKSPACE_STEP_ACTIONS,
        ));
    }
    workspace_rows.push(swipe_row(ui, p));
    scroll_body(
        "shortcuts-body",
        controls::page(
            "sections",
            vec![
                controls::section(p, "modifier", "Modifier", modifier_rows(ui, p, &shortcuts)),
                controls::section(p, "workspaces", "Workspaces", workspace_rows),
                controls::section(
                    p,
                    "windows-layout",
                    "Windows & layout",
                    action_rows(ui, p, &shortcuts, &named, catalog::LAYOUT_ACTIONS),
                ),
            ],
        ),
    )
}

/// The base modifier every built-in shortcut holds, and a way back to the built-ins.
fn modifier_rows(ui: &Ui, p: Paint, shortcuts: &Shortcuts) -> Vec<El> {
    let mut rows = Vec::new();
    if shortcuts.defaults {
        let labels = catalog::PRESETS.map(|(label, _)| label);
        let selected = catalog::PRESETS
            .iter()
            .position(|(_, modifier)| *modifier == shortcuts.modifier);
        let picker = SegmentedControl::new(&labels, selected)
            .label("Base modifier")
            .fit()
            .on_pick(|ui: &mut Ui, index: usize, _: &mut rune::Tx| {
                if let Some((_, modifier)) = catalog::PRESETS.get(index) {
                    set_modifier(ui, *modifier);
                }
            })
            .build(p.k)
            .key("picker");
        rows.push(controls::setting_row(
            p,
            "base-modifier",
            "Base modifier",
            Some(("Moves add ⇧".into(), p.muted())),
            picker,
        ));
    } else {
        rows.push(controls::setting_row(
            p,
            "defaults-off",
            "Built-in shortcuts are off",
            Some((
                "defaults = false in config.toml: only your bindings run".into(),
                p.muted(),
            )),
            row().key("none"),
        ));
    }
    if ui.doc.has_custom_shortcuts() {
        rows.push(controls::setting_row(
            p,
            "reset",
            "Custom shortcuts",
            None,
            controls::button(
                p,
                "reset-button",
                "Reset to defaults",
                ButtonKind::Secondary,
                |ui: &mut Ui, _| reset(ui),
            ),
        ));
    }
    rows
}

/// Saves the base modifier every built-in shortcut holds.
pub(crate) fn set_modifier(ui: &mut Ui, modifier: Modifiers) {
    recorder::cancel(ui);
    ui.shortcut_capture = Capture::Idle;
    recorder::edit_shortcuts(ui, |doc| doc.set_shortcut_modifier(modifier));
}

fn reset(ui: &mut Ui) {
    recorder::cancel(ui);
    ui.shortcut_capture = Capture::Idle;
    recorder::edit_shortcuts(ui, crate::rules::Doc::reset_shortcuts);
}

/// The workspace shortcuts, reflecting `[shortcuts.bindings]`, `defaults`, and `modifier`.
fn family_rows(p: Paint, shortcuts: &Shortcuts, named: &[WorkspaceId]) -> Vec<El> {
    workspaces::rows(shortcuts, named)
        .into_iter()
        .map(|entry| match entry.keys {
            workspaces::RowKeys::Range(caps) => fixed_row(p, &entry.key, &entry.title, caps),
            workspaces::RowKeys::Chords(chords) => controls::setting_row(
                p,
                &format!("{}-row", entry.key),
                &entry.title,
                None,
                keys_face(p, &chords, false),
            ),
        })
        .collect()
}

fn fixed_row(p: Paint, key: &str, title: &str, caps: Vec<String>) -> El {
    controls::setting_row(
        p,
        &format!("{key}-row"),
        title,
        None,
        tray(p, "tray", &caps),
    )
}

/// What the workspace swipe's row shows.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SwipeRow {
    /// The fixed keycap tray, as always while `[labs] swipe_switch` is off.
    Fixed,
    /// With the Lab on, a switch for `settings.trackpad_swipe`.
    Switch { on: bool },
}

impl SwipeRow {
    fn of(doc: &crate::rules::Doc) -> Self {
        if doc.lab(LabFeature::SwipeSwitch) {
            Self::Switch {
                on: doc.trackpad_swipe(),
            }
        } else {
            Self::Fixed
        }
    }
}

/// The workspace swipe's row: a fixed keycap tray, or with `[labs] swipe_switch` on, a
/// switch for `settings.trackpad_swipe` (off leaves the gesture to macOS alone). It names
/// four fingers while `[labs] four_finger_swipe` is on.
fn swipe_row(ui: &Ui, p: Paint) -> El {
    let fingers = if ui.doc.lab(LabFeature::FourFingerSwipe) {
        4
    } else {
        3
    };
    let on = match SwipeRow::of(&ui.doc) {
        SwipeRow::Fixed => {
            return fixed_row(
                p,
                "swipe",
                "Switch adjacent workspace",
                vec![format!("{fingers}-finger swipe")],
            );
        }
        SwipeRow::Switch { on } => on,
    };
    let description = if on {
        format!("{fingers}-finger swipe left or right")
    } else {
        format!("Off: {fingers}-finger swipes are left to macOS")
    };
    controls::toggle_row(
        p,
        "swipe-row",
        "Swipe to switch workspace",
        Some((description, p.muted())),
        on,
        move |ui| ui.edit(|doc| doc.set_trackpad_swipe(!on)),
    )
}

fn action_rows(
    ui: &Ui,
    p: Paint,
    shortcuts: &Shortcuts,
    named: &[WorkspaceId],
    actions: &'static [Action],
) -> Vec<El> {
    actions
        .iter()
        .map(|action| {
            controls::setting_row(
                p,
                &format!("{}-row", action.id),
                action.title,
                None,
                shortcut_button(ui, p, shortcuts, named, action),
            )
        })
        .collect()
}

/// A shortcut's keys, pressed to record a new chord; while recording, the recorder in their
/// place, taking every key until it saves, refuses or cancels.
fn shortcut_button(
    ui: &Ui,
    p: Paint,
    shortcuts: &Shortcuts,
    named: &[WorkspaceId],
    action: &'static Action,
) -> El {
    let c = &p.k.c;
    let id = action.id;
    let (content, label) = match &ui.shortcut_capture {
        Capture::Listening {
            action: active,
            held,
            message,
        } if *active == id => (
            recorder_face(p, *held, message.as_deref()),
            message.clone().unwrap_or_else(|| "Press shortcut".into()),
        ),
        capture => {
            let saved = *capture == Capture::Saved { action: id };
            let chords = action.chords(shortcuts, named);
            let label = chords
                .first()
                .map_or_else(|| "Not set".to_owned(), |chord| chord.keycaps().concat());
            (
                keys_face(p, &chords, saved),
                format!("Change shortcut, {label}"),
            )
        }
    };
    let listening = ui.shortcut_capture.listening() == Some(id);
    let button = row()
        .key(format!("{id}-shortcut"))
        .role("button")
        .label(label)
        .center_y()
        // The tray, not the pad round it, lands on the control rail; the pad's corner is the
        // tray's plus the pad, so the hover fill is concentric with it.
        .padding(KEYS.focus_pad)
        .mr(-KEYS.focus_pad)
        .radius(KEYS.button_radius())
        .focusable(true)
        .focus_ring(2.0, 0.0, c.border_focus)
        .on_press(move |ui: &mut Ui| recorder::begin(ui, id))
        .on_physical_input(|ui: &mut Ui, event, _| recorder::on_input(ui, event))
        .child(col().key("content").child(content));
    if listening {
        // A soft glow says the keyboard belongs to this shortcut now.
        button.fill(c.bg_button_neutral).box_shadow(Shadow {
            blur: 6.0,
            spread: 1.0,
            color: c.border_focus.alpha(0.45),
            ..Shadow::default()
        })
    } else {
        button.hover(c.hover).press(c.pressed)
    }
}

fn keys_face(p: Paint, chords: &[Chord], saved: bool) -> El {
    let keys = controls::chords(p, chords);
    row()
        .key("face")
        .gap(4.0)
        .center_y()
        .children(saved.then(|| {
            rune::icon(&rune_theme_opencode::icons::CHECK_SMALL, 16.0)
                .key("saved")
                .color(p.success())
        }))
        .child(keys)
}

/// Listening: a breathing dot, then the held modifiers sunk into their keys (or why the last
/// chord was refused), then the two keys that end it: ⌫ clears the shortcut, esc keeps it.
fn recorder_face(p: Paint, held: Modifiers, message: Option<&str>) -> El {
    let style = p.label();
    let held = held.keycaps();
    let prompt = match message {
        Some(message) => text(message.to_owned())
            .key("refused")
            .font(style)
            .color(p.danger()),
        None if held.is_empty() => text("Press shortcut")
            .key("prompt")
            .font(style)
            .color(p.text()),
        None => row()
            .key("held")
            .gap(4.0)
            .center_y()
            .child(
                row().key("keys").gap(KEYS.gap).children(
                    held.iter()
                        .enumerate()
                        .map(|(index, cap)| keycap(p, index, cap, true)),
                ),
            )
            .child(text("…").key("ellipsis").font(style).color(p.muted())),
    };
    row()
        .key("listening")
        .h(KEYS.tray_h())
        .gap(8.0)
        .center_y()
        .pl(12.0)
        .pr(KEYS.tray_pad)
        // The prompt rolls in from below and out through the top, inside the recorder.
        .clip_rounded(KEYS.tray_radius())
        .child(controls::pulsing_dot(p.danger()))
        .child(prompt.transition(roll()))
        .child(
            row()
                .key("ends")
                .gap(KEYS.gap)
                .child(
                    keycap(p, 0, "⌫", false)
                        .key("clear")
                        .label("Delete to clear"),
                )
                .child(
                    keycap(p, 1, "esc", false)
                        .key("escape")
                        .label("Esc to cancel"),
                ),
        )
}

/// Playground: Previous workspace listening, holding ⌃⌥, or refused with `message`.
pub(crate) fn fixture_recording(ui: &mut Ui, message: Option<&str>) {
    ui.shortcut_capture = Capture::Listening {
        action: "back-and-forth",
        held: catalog::PRESETS[1].1,
        message: message.map(str::to_owned),
    };
}

/// Playground: the Control + Option base, Previous workspace on ⌘⌥P just saved, and Move
/// workspace to next display unbound.
pub(crate) fn fixture_custom(ui: &mut Ui) {
    ui.edit(|doc| doc.set_shortcut_modifier(catalog::PRESETS[1].1));
    let shortcuts = ui.doc.shortcuts();
    for (id, source) in [
        ("back-and-forth", Some("cmd+alt+p")),
        ("move-workspace", None),
    ] {
        if let Some(action) = catalog::find(id) {
            let chord = source.and_then(|source| fractile::shortcuts::parse_chord(source).ok());
            let default = action.default_chord(shortcuts.modifier);
            ui.edit(|doc| doc.set_shortcut(action.command, default, chord));
        }
    }
    ui.shortcut_capture = Capture::Saved {
        action: "back-and-forth",
    };
}
