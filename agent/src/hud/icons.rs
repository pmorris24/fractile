//! The default Icons style: each workspace as its tabular number and the icons of its apps.
//! Nothing changes size; a concentric pill marker glides to the active group while inactive
//! groups sit calmly dimmed.

use rune::{Cursor, El, Spring, TextStyle, Tx, col, el, row, text};
use spaceman::{WorkspaceId, app::UiHudWorkspace};

use super::pill::{INNER, Icons, Track, app_icon, presence, rgba, srgb};
use crate::controls::Paint;

/// Inset from the capsule's inside edge to the first group: `EDGE - PAD = 6 pt`, matching the
/// marker's 6 pt vertical inset (`(INNER - MARKER_H) / 2`) for a concentric pill-in-pill.
pub const EDGE: f32 = 15.0;
pub const ICON: f32 = 16.0;
const ICON_GAP: f32 = 4.0;
/// From the number to its first icon.
const NUMBER_GAP: f32 = 6.0;
pub const GROUP_GAP: f32 = 18.0;
/// The marker's horizontal reach past the active group, and its height/radius.
pub const PAD: f32 = 9.0;
pub const MARKER_H: f32 = 30.0;
pub const MARKER_RADIUS: f32 = MARKER_H / 2.0;
/// From the capsule's outside edge to the marker: the capsule's radius less this is the marker's.
pub const MARKER_INSET: f32 = (super::pill::PILL_HEIGHT - MARKER_H) / 2.0;
const NUMBER_MIN_W: f32 = 10.0;
/// Keeps an empty workspace's active marker at least circular (`MARKER_H` wide).
const EMPTY_MIN_W: f32 = MARKER_H - 2.0 * PAD;
const DIM: f32 = 0.42;
const GROUP: Spring = Spring::new(0.095, 0.0);
/// One icon per app, so a workspace full of windows still fits.
const MAX_APPS: usize = 12;

/// Tabular figures, so a group's width doesn't change with its digit.
fn number_style() -> TextStyle {
    TextStyle {
        features: "'tnum' 1",
        ..TextStyle::new(13.0, 600.0).lh(16.0)
    }
}

/// An icon per window, in tile order, then the floating apps (which the engine sends once each),
/// so an app with two windows here shows twice and two workspaces of the same apps tell apart.
#[must_use]
pub fn apps(entry: &UiHudWorkspace) -> Vec<&str> {
    let tiles = entry.tiles.iter().filter_map(|tile| tile.bundle.as_deref());
    tiles
        .chain(entry.floating.iter().map(String::as_str))
        .take(MAX_APPS)
        .collect()
}

fn number_width(label: &str) -> f32 {
    rune::text::advance(label, number_style())
        .ceil()
        .max(NUMBER_MIN_W)
}

fn group_width(number: f32, apps: usize) -> f32 {
    if apps == 0 {
        return number.max(EMPTY_MIN_W);
    }
    number + NUMBER_GAP + apps as f32 * ICON + (apps - 1) as f32 * ICON_GAP
}

/// A number's line, in a side's column, where the icons stack under it.
const NUMBER_H: f32 = 16.0;

#[cfg(test)]
pub fn track(
    entries: &[UiHudWorkspace],
    focus: Option<WorkspaceId>,
    icons: &Icons,
    p: Paint,
) -> Track {
    track_with_mode(entries, focus, icons, p, false, false)
}

pub fn track_with_mode(
    entries: &[UiHudWorkspace],
    focus: Option<WorkspaceId>,
    icons: &Icons,
    _p: Paint,
    interactive: bool,
    column: bool,
) -> Track {
    let mut x = EDGE;
    let mut marker = None;
    let mut cells = Vec::with_capacity(entries.len());
    for entry in entries {
        let label = entry.workspace.to_string();
        let number = number_width(&label);
        let apps = apps(entry);
        // In a column the number sits on its own line with the icons stacked under it.
        let width = if column {
            group_width(NUMBER_H, apps.len())
        } else {
            group_width(number, apps.len())
        };
        let current = Some(entry.workspace) == focus;
        if current {
            let across = (INNER - MARKER_H) / 2.0;
            marker = Some(if column {
                [across, x - PAD, MARKER_H, width + 2.0 * PAD]
            } else {
                [x - PAD, across, width + 2.0 * PAD, MARKER_H]
            });
        }
        let cell = group(
            entry.workspace,
            &label,
            number,
            width,
            &apps,
            icons,
            current,
            interactive,
            column,
        );
        let cell = if column {
            cell.absolute(0.0, x).w(INNER).h(width)
        } else {
            cell.absolute(x, 0.0).w(width).h(INNER)
        };
        cells.push(
            cell.opacity(if current { 1.0 } else { DIM })
                .opacity_motion(GROUP)
                .motion(GROUP)
                .transition(presence()),
        );
        x += width + GROUP_GAP;
    }
    let width = if entries.is_empty() {
        2.0 * EDGE
    } else {
        x - GROUP_GAP + EDGE
    };
    Track {
        width,
        marker,
        marker_radius: MARKER_RADIUS,
        cells,
    }
}

#[allow(clippy::too_many_arguments)]
fn group(
    workspace: WorkspaceId,
    label: &str,
    number: f32,
    width: f32,
    apps: &[&str],
    icons: &Icons,
    current: bool,
    interactive: bool,
    column: bool,
) -> El {
    let digit_w = if apps.is_empty() { width } else { number };
    let digits = row().key("number").center().child(
        text(label.to_owned())
            .font(number_style())
            .color(srgb(0xf4f4f6)),
    );
    let digits = if column {
        digits
            .w(INNER)
            .h(if apps.is_empty() { width } else { NUMBER_H })
    } else {
        digits.w(digit_w).h(16.0)
    };
    let mut group = if column {
        col().key(format!("ws{workspace}")).center_y()
    } else {
        row().key(format!("ws{workspace}")).center_y()
    };
    if interactive {
        group = group
            .role("button")
            .label(format!("Switch to workspace {workspace}"))
            .cursor(Cursor::Pointer)
            .hover_group()
            .on_hover_change(move |overlay: &mut super::HudOverlay, inside, _| {
                overlay.set_hovered_workspace(workspace, inside);
            })
            .on_press_tx(move |overlay: &mut super::HudOverlay, tx: &mut Tx| {
                overlay.switch_workspace(workspace, tx);
            })
            .child(
                el().key("hover")
                    .absolute(
                        if column {
                            (INNER - MARKER_H) / 2.0
                        } else {
                            -PAD
                        },
                        if column {
                            -PAD
                        } else {
                            (INNER - MARKER_H) / 2.0
                        },
                    )
                    .size(
                        if column { MARKER_H } else { width + 2.0 * PAD },
                        if column { width + 2.0 * PAD } else { MARKER_H },
                    )
                    .radius(MARKER_RADIUS)
                    .fill(rgba(0xffffff, 0.08))
                    .group_opacity(0.0, if current { 0.0 } else { 1.0 }),
            );
    }
    group = group.child(digits);
    if !apps.is_empty() {
        let list = if column {
            col().key("apps").pt(NUMBER_GAP).center_y()
        } else {
            row().key("apps").pl(NUMBER_GAP).center_y()
        };
        group = group.child(list.gap(ICON_GAP).children(apps.iter().enumerate().map(
            |(index, bundle)| {
                // The same app twice: keyed by which of its windows it is.
                let nth = apps[..index]
                    .iter()
                    .filter(|other| *other == bundle)
                    .count();
                let icon = app_icon(icons, workspace, bundle, ICON, interactive);
                if nth == 0 {
                    icon
                } else {
                    icon.key(format!("app-{bundle}-{nth}"))
                }
            },
        )));
    }
    group
}
