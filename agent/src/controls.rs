//! Settings controls on `rune-kit` with OpenCode's theme, the same as Hex's and Invoke's: the
//! page's rails, the raised card and its rows, switches, buttons, the modal shell, shortcut
//! keys and the permission control. Every size is a token (`tokens.rs`); the rules are in
//! `docs/design-system.md`.

mod keycaps;
mod modal;
mod tokens;

use std::sync::LazyLock;

use rune::{
    Beat, Color, Easing, Edge, El, Icon, LoopSpec, Sequence, Shadow, Spring, SpringPair, TextStyle,
    Transition, Tx, col, el, icon, row, text,
};
use rune_kit::{Button, ButtonVariant, ControlSize, Kit, Switch};

use crate::{Ui, apps::App};

pub use keycaps::{Held, chords, keycap, roll, tray, tray_held};
pub use modal::{modal_panel, modal_sheet};
pub use tokens::{KEYS, METRICS, MODAL, ModalWidth, WINDOW};

/// Row descriptions and secondary copy.
const DESCRIPTION: TextStyle = TextStyle::new(12.0, 440.0).lh(16.0);
/// A section label's line: the label's own height, so its card sits `label_gap` under it.
const SECTION_LABEL_LINE: f32 = 16.0;
/// Clear space above and below a row's copy when a description wraps to a second line.
const ROW_PY: f32 = 8.0;
/// Between a row's copy and its control.
const ROW_GAP: f32 = 24.0;
const PULSE: LoopSpec = LoopSpec::opacity(2.0, 0.0, 1.0, 0.45);

/// Everything a control needs to paint itself.
#[derive(Clone, Copy)]
pub struct Paint {
    pub k: &'static Kit,
    pub hair: f32,
}

impl Paint {
    pub fn new(dark: bool, scale: f32) -> Self {
        Self {
            k: rune_theme_opencode::kit(dark),
            hair: 1.0 / scale,
        }
    }
    /// A section label: above its card, on the page's text rail. Screen readers move between
    /// sections by it.
    pub fn section_label(self, title: &str) -> El {
        text(title)
            .key("title")
            .role("heading")
            .level(2)
            .font(self.label())
            .color(self.muted())
    }
    /// Pane title and modal title (14/500/21).
    pub fn title(self) -> TextStyle {
        self.k.ty.body_medium
    }
    /// Section label above a card, and a row's value (13/530/16).
    pub fn label(self) -> TextStyle {
        self.k.ty.ui_medium.lh(SECTION_LABEL_LINE)
    }
    /// Row title, sidebar item and a pressable row (13/440/20).
    pub fn row_title(self) -> TextStyle {
        self.k.ty.ui
    }
    /// Button label, a tile's number, the recorder's prompt (13/530/20).
    pub fn strong(self) -> TextStyle {
        self.k.ty.ui_medium
    }
    /// Row description, note and empty row (12/440/16).
    pub fn description(self) -> TextStyle {
        DESCRIPTION
    }
    /// Label accessory, sidebar foot and preview caption (11/440/12).
    pub fn meta(self) -> TextStyle {
        self.k.ty.small
    }
    /// A count or an initial in a chip (11/530/12).
    pub fn tag(self) -> TextStyle {
        self.k.ty.small_medium
    }
    /// Tabular figures, for a number that changes in place or sits in a column.
    pub fn tabular(self, style: TextStyle) -> TextStyle {
        TextStyle {
            features: "'tnum' 1",
            ..style
        }
    }
    pub fn text(self) -> Color {
        self.k.c.text_base
    }
    pub fn muted(self) -> Color {
        self.k.c.text_muted
    }
    pub fn faint(self) -> Color {
        self.k.c.text_faint
    }
    pub fn success(self) -> Color {
        self.k.c.success.fg
    }
    pub fn warning(self) -> Color {
        self.k.c.warning.fg
    }
    pub fn danger(self) -> Color {
        self.k.c.danger.fg
    }
    pub fn accent(self) -> Color {
        self.k.c.text_accent
    }
}

/// A settings page: its sections, `section_gap` apart. `view::scroll_body` puts it on the rails.
pub fn page(key: &str, sections: Vec<El>) -> El {
    col().key(key).gap(METRICS.section_gap).children(sections)
}

/// A titled card of rows.
pub fn section(p: Paint, key: &str, title: &str, rows: Vec<El>) -> El {
    col()
        .key(key)
        .gap(METRICS.label_gap)
        .child(section_header(p, title, None))
        .child(card(p, "card", rows))
}

/// A [`section`] with an accessory on its label's line: a caption (see [`caption`]) or a small
/// control. The line keeps the label's height, and the accessory ends where the rows' controls
/// do.
pub fn section_with(p: Paint, key: &str, title: &str, accessory: El, rows: Vec<El>) -> El {
    col()
        .key(key)
        .gap(METRICS.label_gap)
        .child(section_header(p, title, Some(accessory)))
        .child(card(p, "card", rows))
}

/// A section's label line on its own, for a view that lays its own content under it (the Rules
/// map). Inset `row_px` from the card edge, so the label sits on the text rail.
pub fn section_header(p: Paint, title: &str, accessory: Option<El>) -> El {
    row()
        .key("header")
        .h(SECTION_LABEL_LINE)
        .mx(METRICS.row_px)
        .gap(8.0)
        .center_y()
        .child(p.section_label(title).grow(1.0))
        .children(accessory)
}

/// Quiet copy for a section label's line.
pub fn caption(p: Paint, copy: &str) -> El {
    text(copy.to_owned())
        .key("caption")
        .font(p.meta())
        .color(p.muted())
}

/// A raised card of rows with hairline separators between them. The card has no padding: rows
/// reach its edges and take its corners, so a row's fill (a deep link's highlight, a press) is
/// full-bleed and needs no radius of its own.
pub fn card(p: Paint, key: &str, rows: Vec<El>) -> El {
    let radius = p.k.radius.lg;
    let last = rows.len().saturating_sub(1);
    let mut card = surface(p, col().key(key));
    for (index, row) in rows.into_iter().enumerate() {
        if index > 0 {
            card = card.child(
                el().key(format!("sep{index}"))
                    .h(p.hair)
                    .mx(METRICS.row_px)
                    .fill(p.k.c.border_muted)
                    .a11y_hidden(),
            );
        }
        let top = if index == 0 { radius } else { 0.0 };
        let bottom = if index == last { radius } else { 0.0 };
        card = card.child(row.corners([top, top, bottom, bottom]));
    }
    card
}

/// The card's raised surface on any element.
pub fn surface(p: Paint, element: El) -> El {
    let mut element = element.fill(p.k.c.bg_base).radius(p.k.radius.lg);
    for shadow in &p.k.elevation.raised {
        element = element.box_shadow(*shadow);
    }
    element
}

/// The fill a `fractile://` deep link gives the row or card it points at.
pub fn highlight(p: Paint, element: El, on: bool) -> El {
    if on {
        element.fill(p.k.c.layer_02)
    } else {
        element
    }
}

/// A card row's frame: its height (as a minimum, so wrapped copy grows it), its inset to the
/// text rail, and its content centred. Every row in a card is built on it.
pub fn row_frame(key: &str, described: bool) -> El {
    row()
        .key(key)
        .min_h(if described {
            METRICS.row_h_described
        } else {
            METRICS.row_h
        })
        .px(METRICS.row_px)
        .py(ROW_PY)
        .center_y()
}

/// A setting: title and optional description on the left, its control on the right.
pub fn setting_row(
    p: Paint,
    key: &str,
    title: &str,
    description: Option<(String, Color)>,
    control: El,
) -> El {
    leading_row(p, key, None, title, description, control)
}

/// A setting row led by a 16-point glyph on a shared icon spine.
pub fn icon_row(
    p: Paint,
    key: &str,
    glyph: &'static Icon,
    title: &str,
    description: Option<(String, Color)>,
    control: El,
) -> El {
    let lead = icon(glyph, METRICS.icon).key("icon").color(p.k.c.icon_base);
    leading_row(p, key, Some(lead), title, description, control)
}

fn leading_row(
    p: Paint,
    key: &str,
    lead: Option<El>,
    title: &str,
    description: Option<(String, Color)>,
    control: El,
) -> El {
    let frame = row_frame(key, description.is_some());
    let copy = col()
        .key("copy")
        .grow(1.0)
        .shrink()
        .gap(2.0)
        .child(text(title).key("label").font(p.row_title()).color(p.text()))
        .children(description.map(|(copy, color)| {
            text(copy)
                .key("description")
                .font(p.description())
                .color(color)
                .wrap()
        }));
    frame
        .gap(ROW_GAP)
        .justify_between()
        .label(title)
        .child(match lead {
            None => copy,
            Some(lead) => row()
                .key("lead")
                .grow(1.0)
                .shrink()
                .gap(METRICS.icon_lead - METRICS.icon)
                .center_y()
                .child(lead)
                .child(copy),
        })
        .child(control)
}

/// A whole-row switch: the row presses, the switch shows the state.
pub fn toggle_row(
    p: Paint,
    key: &str,
    title: &str,
    description: Option<(String, Color)>,
    on: bool,
    flip: impl Fn(&mut Ui) + 'static,
) -> El {
    let switch = Switch::new(on).build(p.k).key("switch").focusable(false);
    setting_row(p, key, title, description, switch)
        .role("toggle")
        .focusable(true)
        .on_press(flip)
}

/// `path` with the home directory shortened to `~`.
pub fn home_relative(path: &std::path::Path) -> String {
    std::env::var_os("HOME")
        .and_then(|home| path.strip_prefix(home).ok())
        .map_or_else(
            || path.display().to_string(),
            |rest| format!("~/{}", rest.display()),
        )
}

/// Reveals `path` in Finder, or opens its parent folder if the file has not been created yet.
pub fn reveal_in_finder(path: &std::path::Path) {
    let target = if path.exists() {
        path
    } else {
        path.parent().unwrap_or(path)
    };
    drop(
        std::process::Command::new("open")
            .arg("-R")
            .arg(target)
            .spawn(),
    );
}

/// A push button's text inset; the granted permission pill keeps it, so `Allow` holds still as
/// it becomes `Allowed`.
const BUTTON_PX: f32 = 12.0;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ButtonKind {
    Primary,
    Secondary,
}

pub fn button(
    p: Paint,
    key: &str,
    label: &str,
    kind: ButtonKind,
    press: impl Fn(&mut Ui, &mut Tx) + 'static,
) -> El {
    button_enabled(p, key, label, kind, true, press)
}

/// A [`button`] that is dimmed and takes no press while there is nothing for it to do.
pub fn button_enabled(
    p: Paint,
    key: &str,
    label: &str,
    kind: ButtonKind,
    enabled: bool,
    press: impl Fn(&mut Ui, &mut Tx) + 'static,
) -> El {
    Button::new(label)
        .variant(match kind {
            ButtonKind::Primary => ButtonVariant::Contrast,
            ButtonKind::Secondary => ButtonVariant::Neutral,
        })
        .on_press_tx(press)
        .disabled(!enabled)
        .control_size(ControlSize::Large)
        .build(p.k)
        .px(BUTTON_PX)
        .radius(p.k.radius.md)
        .key(key)
}

/// A quiet button beside a row's main control: it opens that feature's refinements
/// (`Customize…`). No fill at rest, so the main control stays the row's one loud thing.
pub fn quiet_button(
    p: Paint,
    key: &str,
    label: &str,
    press: impl Fn(&mut Ui, &mut Tx) + 'static,
) -> El {
    Button::new(label)
        .variant(ButtonVariant::GhostMuted)
        .on_press_tx(press)
        .control_size(ControlSize::Large)
        .build(p.k)
        .px(8.0)
        .radius(p.k.radius.md)
        .key(key)
}

pub fn status_dot(color: Color) -> El {
    el().key("dot")
        .size(METRICS.dot, METRICS.dot)
        .radius(METRICS.dot / 2.0)
        .fill(color)
}

pub fn pulsing_dot(color: Color) -> El {
    status_dot(color).looping(PULSE)
}

/// An app's Finder icon at `size`, or its initial in a rounded square when unavailable.
pub fn icon_or_initial(p: Paint, app: Option<&App>, name: &str, size: f32) -> El {
    match app.and_then(|app| app.icon.as_ref()) {
        Some(pixels) => el().size(size, size).pixels(pixels),
        None => el()
            .size(size, size)
            .radius(size * 0.25)
            .fill(p.k.c.layer_03)
            .center()
            .child(
                text(name.chars().next().map(String::from).unwrap_or_default())
                    .font(p.tag())
                    .color(p.muted()),
            ),
    }
}

/// How a permission's control reads in onboarding and Settings.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PermissionControl {
    Allow,
    Waiting,
    /// Granted (`animated` plays the two-beat check draw-on after `Allow` was pressed).
    Allowed {
        animated: bool,
    },
}

const GATHER: Spring = Spring::new(0.16, 0.0);
const GATHER_SECS: f64 = 0.14;
const DIP: f32 = 0.94;
const RELEASE: Spring = Spring::new(0.3, 0.16);
const RELEASE_SECS: f64 = 0.45;
const DRAW: Spring = Spring::new(0.42, 0.0);
const SUFFIX: Spring = Spring::new(0.4, 0.0);
const INK: Spring = Spring::new(0.3, 0.0);
const APPEAR: &[Beat] = &[
    Beat::new(0.0).opacity(0.0),
    Beat::new(0.1).opacity(0.0),
    Beat::new(0.15).opacity(1.0).easing(Easing::EASE_OUT),
];
const CHECK_INK: [f32; 4] = [4.6, 4.6, 6.8, 6.8];
const CHECK_START: [f32; 2] = [5.2, 9.15];
const CHECK_X: f32 = 4.0;

/// Two-beat permission morph: `Allow` -> pulsing dot while macOS asks -> green `Allowed`.
pub fn permission_control(
    p: Paint,
    key: &str,
    control: PermissionControl,
    kind: ButtonKind,
    allow: impl Fn(&mut Ui) + 'static,
) -> El {
    let (mark, gap, pill) = match control {
        PermissionControl::Allow => (
            None,
            12.0,
            button(p, key, "Allow", kind, move |ui: &mut Ui, _| allow(ui)),
        ),
        PermissionControl::Waiting => (
            Some(
                el().key("mark")
                    .size(8.0, 8.0)
                    .child(pulsing_dot(p.accent()).absolute(0.0, 0.0)),
            ),
            12.0,
            button(
                p,
                key,
                "Allow",
                ButtonKind::Secondary,
                move |ui: &mut Ui, _| allow(ui),
            ),
        ),
        PermissionControl::Allowed { animated: true } => {
            (Some(check_mark(p)), 6.0, allowed_pill(p, key, kind))
        }
        PermissionControl::Allowed { animated: false } => {
            return row()
                .key(format!("{key}-control"))
                .gap(6.0)
                .pr(4.0)
                .center_y()
                .child(
                    icon(p.k.glyph.check_small, 16.0)
                        .key("check")
                        .color(p.success()),
                )
                .child(
                    text("Allowed")
                        .key("label")
                        .font(p.strong())
                        .color(p.success()),
                );
        }
    };
    let control_row = row()
        .key(format!("{key}-control"))
        .gap(gap)
        .center_y()
        .motion(GATHER)
        .children(mark)
        .child(pill.motion(GATHER));
    if matches!(control, PermissionControl::Allowed { animated: true }) {
        control_row.sequence(Sequence::new(grant_beats()))
    } else {
        control_row
    }
}

fn grant_beats() -> &'static [Beat] {
    static BEATS: LazyLock<[Beat; 3]> = LazyLock::new(|| {
        static CURVE: LazyLock<Vec<[f32; 2]>> = LazyLock::new(|| {
            const STOPS: usize = 48;
            (0..=STOPS)
                .map(|i| {
                    let x = i as f32 / STOPS as f32;
                    let t = x * RELEASE_SECS as f32;
                    let y = if i == STOPS {
                        1.0
                    } else {
                        RELEASE.sample(0.0, 0.0, 1.0, t).0
                    };
                    [x, y]
                })
                .collect()
        });
        [
            Beat::new(0.0),
            Beat::new(GATHER_SECS).scale(DIP).easing(Easing::EASE_IN),
            Beat::new(GATHER_SECS + RELEASE_SECS).easing(Easing::Piecewise(&CURVE)),
        ]
    });
    &*BEATS
}

fn check_mark(p: Paint) -> El {
    let [x, y, w, h] = CHECK_INK;
    el().key("mark")
        .size(9.0, 16.0)
        .child(
            status_dot(p.success())
                .absolute(CHECK_X + CHECK_START[0], CHECK_START[1])
                .size(0.0, 0.0)
                .radius(0.0)
                .opacity(0.0)
                .motion(GATHER),
        )
        .child(
            el().key("ink")
                .absolute(CHECK_X + x, y)
                .size(w, h)
                .transition(wipe(DRAW, 3.0))
                .sequence(Sequence::new(APPEAR))
                .child(
                    icon(p.k.glyph.check_small, 16.0)
                        .key("check")
                        .absolute(-x, -y)
                        .color(p.success()),
                ),
        )
}

fn wipe(spring: Spring, feather: f32) -> Transition {
    let mut wipe = Transition::reveal(Edge::Leading).springs(SpringPair::new(spring, spring));
    for reveal in [&mut wipe.insertion.reveal, &mut wipe.removal.reveal]
        .into_iter()
        .flatten()
    {
        reveal.feather = feather;
    }
    wipe
}

fn allowed_pill(p: Paint, key: &str, kind: ButtonKind) -> El {
    let k = p.k;
    let (fill, shadows) = match kind {
        ButtonKind::Primary => (k.c.bg_contrast, &k.elevation.button_contrast),
        ButtonKind::Secondary => (k.c.bg_button_neutral, &k.elevation.button_neutral),
    };
    let font = p.strong();
    let ink = p.success();
    let mut pill = row()
        .key(key)
        .label("Allowed")
        .h(28.0)
        .pl(BUTTON_PX)
        .center_y()
        .radius(k.radius.md)
        .fill(fill.alpha(0.0));
    for shadow in k.shadow_layers(shadows) {
        pill = pill.box_shadow(Shadow {
            color: shadow.color.alpha(0.0),
            ..shadow
        });
    }
    if kind == ButtonKind::Primary {
        pill = pill.gradient(Color::TRANSPARENT, Color::TRANSPARENT);
    }
    pill.child(text("Allow").key("label").font(font).color(ink).motion(INK))
        .child(
            text("ed")
                .key("suffix")
                .font(font)
                .color(ink)
                .transition(wipe(SUFFIX, 2.0))
                .sequence(Sequence::new(APPEAR)),
        )
}
