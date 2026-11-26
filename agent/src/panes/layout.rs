//! Layout pane: one gap for everything by default, or the space between tiled windows and each
//! screen edge on its own, and the width (and, with its Lab, the height) a lone window keeps on a
//! wide display, under a live miniature of the result.

mod preview;
mod slider;

use fractile::{
    config::{LabFeature, LayoutMode},
    spacing::{Spacing, SpacingKey},
};
use rune::{El, Tx, icon, text};
use rune_kit::SegmentedControl;

use crate::{
    Ui,
    controls::{self, Paint},
    view::scroll_body,
};

use slider::{Channel, slider};

/// The sliders' reach; `config.toml` takes more (48 between windows, 128 at an edge).
const SLIDER_MAX: i32 = 32;
/// Single-window widths offered besides Off.
const WIDTHS: [i32; 3] = [1440, 1680, 1920];
/// Single-window heights offered besides Off: the 16:10 counterparts of [`WIDTHS`].
const HEIGHTS: [i32; 3] = [900, 1050, 1200];
const LAYOUT_MODES: [(&str, LayoutMode); 2] = [
    ("Dwindle", LayoutMode::Dwindle),
    ("Columns", LayoutMode::Columns),
];

fn layout_mode_index(mode: LayoutMode) -> usize {
    LAYOUT_MODES
        .iter()
        .position(|(_, candidate)| *candidate == mode)
        .unwrap_or(0)
}

/// Every gap at once: between windows and all four screen edges. It reads the gap between
/// windows, so while the gaps differ a drag evens them out.
const ALL: Channel = Channel {
    get: |spacing| spacing.inner,
    set: |spacing, gap| Spacing {
        inner: gap,
        ..spacing.with_edges(gap)
    },
    max: SLIDER_MAX,
};

const INNER: Channel = Channel {
    get: |spacing| spacing.inner,
    set: |spacing, inner| Spacing { inner, ..spacing },
    max: SLIDER_MAX,
};

/// All four edges at once; while they differ, it shows the top edge and sets them all.
const EDGES: Channel = Channel {
    get: |spacing| spacing.uniform_edges().unwrap_or(spacing.top),
    set: Spacing::with_edges,
    max: SLIDER_MAX,
};

/// Each edge on its own. They are rows like any other, on the card's one text rail, so each
/// names the edge whole rather than leaning on an indent.
const EDGE_CHANNELS: [(SpacingKey, &str, Channel); 4] = [
    (SpacingKey::Top, "Top edge", edge(SpacingKey::Top)),
    (SpacingKey::Bottom, "Bottom edge", edge(SpacingKey::Bottom)),
    (SpacingKey::Left, "Left edge", edge(SpacingKey::Left)),
    (SpacingKey::Right, "Right edge", edge(SpacingKey::Right)),
];

const fn edge(key: SpacingKey) -> Channel {
    match key {
        SpacingKey::Top => Channel {
            get: |spacing| spacing.top,
            set: |spacing, top| Spacing { top, ..spacing },
            max: SLIDER_MAX,
        },
        SpacingKey::Bottom => Channel {
            get: |spacing| spacing.bottom,
            set: |spacing, bottom| Spacing { bottom, ..spacing },
            max: SLIDER_MAX,
        },
        SpacingKey::Left => Channel {
            get: |spacing| spacing.left,
            set: |spacing, left| Spacing { left, ..spacing },
            max: SLIDER_MAX,
        },
        _ => Channel {
            get: |spacing| spacing.right,
            set: |spacing, right| Spacing { right, ..spacing },
            max: SLIDER_MAX,
        },
    }
}

/// Whether the gap between windows and every screen edge match.
fn uniform(spacing: &Spacing) -> bool {
    spacing.uniform_edges() == Some(spacing.inner)
}

/// Whether the separate gap controls show: as the user left them, else while the gaps differ,
/// so a `config.toml` with its own edges opens on them.
pub(crate) fn customize_open(ui: &Ui, spacing: &Spacing) -> bool {
    ui.per_edge.unwrap_or(!uniform(spacing))
}

/// Which section or row on Layout a `fractile://` deep link highlights, if any.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Highlight {
    Gaps,
    MaxWidth,
}

fn highlight_target(ui: &Ui) -> Option<Highlight> {
    match ui.highlight_feature.as_deref() {
        Some("gaps") => Some(Highlight::Gaps),
        Some("max-width" | "single-width") => Some(Highlight::MaxWidth),
        _ => None,
    }
}

fn apply_highlight(el: El, p: Paint, target: Option<Highlight>, wanted: Highlight) -> El {
    controls::highlight(p, el, target == Some(wanted))
}

pub fn pane(ui: &Ui, p: Paint) -> El {
    let layout = ui.doc.layout();
    let height_lab = ui.doc.lab(LabFeature::SingleWindowHeight);
    let mut spacing = ui.spacing_draft.unwrap_or_else(|| ui.doc.spacing());
    // With its Lab off the height cap does nothing, so the preview leaves it out too.
    if !height_lab {
        spacing.single_window_max_height = 0;
    }
    let open = customize_open(ui, &spacing);
    let target = highlight_target(ui);
    let new_windows = controls::card(
        p,
        "new-windows-card",
        vec![controls::setting_row(
            p,
            "new-windows",
            "New windows",
            None,
            SegmentedControl::new(
                &LAYOUT_MODES.map(|(label, _)| label.to_owned()),
                Some(layout_mode_index(layout)),
            )
            .fit()
            .on_pick(|ui: &mut Ui, index, _: &mut Tx| {
                if let Some((_, mode)) = LAYOUT_MODES.get(index) {
                    ui.edit(|doc| doc.set_layout(*mode));
                }
            })
            .build(p.k)
            .key("layout-mode-picker"),
        )],
    );
    // A deep link to the gaps lights every row of their card.
    let gap_rows = gap_rows(p, &spacing, open)
        .into_iter()
        .map(|row| apply_highlight(row, p, target, Highlight::Gaps))
        .collect();
    let gaps = controls::section(p, "gaps", "Gaps", gap_rows);
    let width = apply_highlight(
        controls::setting_row(
            p,
            "single-width",
            "Single-window width",
            Some((
                "Centers a lone window, and Fullscreen Layout, at this width".to_owned(),
                p.muted(),
            )),
            cap_picker(
                p,
                "width-picker",
                &WIDTHS,
                spacing.single_window_max_width,
                with_width,
            ),
        ),
        p,
        target,
        Highlight::MaxWidth,
    );
    let mut wide = vec![width];
    if height_lab {
        wide.push(controls::setting_row(
            p,
            "single-height",
            "Single-window height",
            Some((
                "Centers a lone window, and Fullscreen Layout, at this height".to_owned(),
                p.muted(),
            )),
            cap_picker(
                p,
                "height-picker",
                &HEIGHTS,
                spacing.single_window_max_height,
                with_height,
            ),
        ));
    }
    scroll_body(
        "layout-body",
        controls::page(
            "sections",
            vec![
                preview::card(p, layout, spacing),
                new_windows,
                gaps,
                controls::section(p, "wide", "Wide displays", wide),
            ],
        ),
    )
}

/// One slider for every gap, then the disclosure; opened, the slider sets only the space between
/// windows and the screen edges follow, together and one by one.
fn gap_rows(p: Paint, spacing: &Spacing, open: bool) -> Vec<El> {
    let lead = if open {
        controls::setting_row(
            p,
            "inner",
            "Between windows",
            None,
            slider(p, "inner-slider", "Between windows", spacing.inner, INNER),
        )
    } else {
        controls::setting_row(
            p,
            "all",
            "Window gaps",
            (!uniform(spacing)).then(|| ("Dragging evens out every gap".to_owned(), p.muted())),
            slider(p, "all-slider", "Window gaps", (ALL.get)(spacing), ALL),
        )
    };
    let mut rows = vec![lead, disclosure(p, open)];
    if !open {
        return rows;
    }
    rows.push(controls::setting_row(
        p,
        "edges",
        "Screen edges",
        spacing
            .uniform_edges()
            .is_none()
            .then(|| ("Each edge is set on its own".to_owned(), p.muted())),
        slider(
            p,
            "edges-slider",
            "Screen edges",
            (EDGES.get)(spacing),
            EDGES,
        ),
    ));
    rows.extend(EDGE_CHANNELS.iter().map(|(key, title, channel)| {
        let name = key.gap_name().unwrap_or("edge");
        controls::setting_row(
            p,
            &format!("edge-{name}"),
            title,
            None,
            slider(
                p,
                &format!("{name}-slider"),
                title,
                (channel.get)(spacing),
                *channel,
            ),
        )
    }));
    rows
}

/// The pressable row that shows or hides the separate gap controls: a row like the rest, with
/// its chevron on the control rail.
fn disclosure(p: Paint, open: bool) -> El {
    let chevron = if open {
        p.k.glyph.chevron_down
    } else {
        p.k.glyph.caret_right
    };
    controls::row_frame("customize", false)
        .role("button")
        .label("Customize separately")
        .expanded(open)
        .justify_between()
        .press(p.k.c.pressed)
        .focusable(true)
        .focus_ring(2.0, -2.0, p.k.c.border_focus)
        .on_press(move |ui: &mut Ui| ui.per_edge = Some(!open))
        .child(
            text("Customize separately")
                .key("label")
                .font(p.row_title())
                .color(p.muted()),
        )
        .child(icon(chevron, 12.0).key("chevron").color(p.k.c.icon_muted))
}

/// Off, the common sizes, and a size only `config.toml` set when it holds one.
fn cap_options(presets: &[i32], current: i32) -> (Vec<(String, i32)>, usize) {
    let mut options = vec![("Off".to_owned(), 0)];
    options.extend(presets.iter().map(|size| (size.to_string(), *size)));
    if !options.iter().any(|(_, size)| *size == current) {
        options.push((format!("{current} px"), current));
    }
    let selected = options
        .iter()
        .position(|(_, size)| *size == current)
        .unwrap_or(0);
    (options, selected)
}

const fn with_width(spacing: Spacing, single_window_max_width: i32) -> Spacing {
    Spacing {
        single_window_max_width,
        ..spacing
    }
}

const fn with_height(spacing: Spacing, single_window_max_height: i32) -> Spacing {
    Spacing {
        single_window_max_height,
        ..spacing
    }
}

/// A lone-window cap's picker; `apply` writes the picked size into the file's spacing.
fn cap_picker(
    p: Paint,
    key: &str,
    presets: &[i32],
    current: i32,
    apply: fn(Spacing, i32) -> Spacing,
) -> El {
    let (options, selected) = cap_options(presets, current);
    let labels: Vec<String> = options.iter().map(|(label, _)| label.clone()).collect();
    let sizes: Vec<i32> = options.iter().map(|(_, size)| *size).collect();
    SegmentedControl::new(&labels, Some(selected))
        .fit()
        .on_pick(move |ui: &mut Ui, index, _: &mut Tx| {
            if let Some(size) = sizes.get(index) {
                let base = ui.doc.spacing();
                ui.set_spacing(apply(base, *size));
            }
        })
        .build(p.k)
        .key(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn width_picker_offers_off_the_presets_and_a_custom_width() {
        let (options, selected) = cap_options(&WIDTHS, 0);
        assert_eq!(options.len(), 4);
        assert_eq!(selected, 0);
        let (_, selected) = cap_options(&WIDTHS, 1680);
        assert_eq!(selected, 2);
        let (options, selected) = cap_options(&WIDTHS, 1600);
        assert_eq!(options.len(), 5);
        assert_eq!(options[selected], ("1600 px".to_owned(), 1600));
        let (options, selected) = cap_options(&HEIGHTS, 1050);
        assert_eq!(options.len(), 4);
        assert_eq!(options[selected], ("1050".to_owned(), 1050));
    }

    #[test]
    fn edge_channels_write_only_their_edge() {
        let base = Spacing::NONE.with_edges(4);
        for (key, _, channel) in EDGE_CHANNELS {
            let moved = (channel.set)(base, 20);
            assert_eq!(key.get(&moved), 20);
            assert_eq!((channel.get)(&moved), 20);
            for other in SpacingKey::EDGES.into_iter().filter(|other| *other != key) {
                assert_eq!(other.get(&moved), 4);
            }
        }
        assert_eq!((EDGES.set)(base, 9).uniform_edges(), Some(9));
        assert_eq!((INNER.set)(base, 9).inner, 9);
    }

    #[test]
    fn the_locked_slider_moves_every_gap_together() {
        let mixed = Spacing {
            inner: 6,
            top: 30,
            ..Spacing::NONE
        };
        assert!(!uniform(&mixed));
        let locked = (ALL.set)(mixed, 12);
        assert!(uniform(&locked));
        assert_eq!((ALL.get)(&locked), 12);
        assert_eq!(locked.uniform_edges(), Some(12));
        assert!(uniform(&Spacing::NONE));
        let wide = Spacing {
            single_window_max_width: 1680,
            ..Spacing::NONE
        };
        assert_eq!((ALL.set)(wide, 8).single_window_max_width, 1680);
    }

    #[test]
    fn gaps_deep_link_highlights_gaps_while_ordinary_layout_link_does_not() {
        use fractile::deep_link::parse_url;

        let p = Paint::new(true, 2.0);
        let mut ui = crate::playground::blank_fixture();

        ui.open_target(parse_url("fractile://features/gaps").unwrap().resolve());
        assert_eq!(ui.pane, crate::Pane::Layout);
        assert_eq!(highlight_target(&ui), Some(Highlight::Gaps));
        let _ = pane(&ui, p);

        ui.open_target(parse_url("fractile://settings/layout").unwrap().resolve());
        assert_eq!(ui.pane, crate::Pane::Layout);
        assert_eq!(highlight_target(&ui), None);
        let _ = pane(&ui, p);

        ui.open_target(
            parse_url("fractile://features/max-width")
                .unwrap()
                .resolve(),
        );
        assert_eq!(ui.pane, crate::Pane::Layout);
        assert_eq!(highlight_target(&ui), Some(Highlight::MaxWidth));
        let _ = pane(&ui, p);
    }

    #[test]
    fn layout_mode_picker_maps_dwindle_and_columns_and_round_trips_config() {
        assert_eq!(layout_mode_index(LayoutMode::Dwindle), 0);
        assert_eq!(layout_mode_index(LayoutMode::Columns), 1);

        let mut doc = crate::rules::Doc::parse("# Keep me.\nversion = 1\n");
        assert_eq!(doc.layout(), LayoutMode::Dwindle);

        doc.set_layout(LayoutMode::Columns);
        assert_eq!(doc.layout(), LayoutMode::Columns);
        let columns = doc.source();
        assert!(columns.contains("layout = \"columns\""), "{columns}");
        assert!(columns.contains("# Keep me."), "{columns}");
        assert_eq!(
            fractile::config::Config::parse(&columns).unwrap().layout(),
            LayoutMode::Columns
        );

        doc.set_layout(LayoutMode::Dwindle);
        assert_eq!(doc.layout(), LayoutMode::Dwindle);
        let dwindle = doc.source();
        assert!(!dwindle.contains("layout"), "{dwindle}");
        assert!(!dwindle.contains("[settings]"), "{dwindle}");
        assert!(dwindle.contains("# Keep me."), "{dwindle}");
        assert_eq!(
            fractile::config::Config::parse(&dwindle).unwrap().layout(),
            LayoutMode::Dwindle
        );

        let p = Paint::new(true, 2.0);
        let mut ui = crate::playground::blank_fixture();
        ui.doc.set_layout(LayoutMode::Columns);
        let _ = pane(&ui, p);
        ui.doc.set_layout(LayoutMode::Dwindle);
        let _ = pane(&ui, p);
    }
}
