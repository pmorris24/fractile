//! The four-step Rune setup tour view: Welcome, Accessibility, Your workspaces, and Try it.

use rune::{
    Color, El, Icon, Shadow, Spring, SpringPair, TextStyle, Transition, Tx, col, el, icon, row,
    text,
};
use rune_kit::{Button, ButtonVariant, ControlSize, IconButton, IconButtonVariant, glyphs};

use super::{
    Input, Step, arrange, arrange::Board, conflicts::Conflict, flow::View, input, resolve, stage,
    try_it,
};
use crate::{
    Ui,
    controls::{self, ButtonKind, Paint},
};

const WIDTH: f32 = 520.0;
const LEAD: TextStyle = TextStyle::new(15.0, 400.0).lh(22.0);
const STEP_ENTER: Spring = Spring::new(0.3, 0.0);
const STEP_EXIT: Spring = Spring::new(0.25, 0.0);

static ACCESSIBILITY_ICON: Icon = Icon {
    name: "fractile-accessibility",
    view: 16.0,
    body: include_str!("../../assets/layout.svg"),
};

fn title_style(dark: bool) -> TextStyle {
    TextStyle::new(26.0, if dark { 540.0 } else { 600.0 })
        .lh(32.0)
        .tracking(-0.3)
}

/// The tour over the window. `still` is Reduce Motion.
pub fn overlay(ui: &Ui, p: Paint, still: bool) -> Option<El> {
    let tour = ui.tour.as_ref()?;
    let view = tour.flow.view();
    let step_body = match view.step {
        Step::Welcome => welcome(p),
        Step::Setup => setup(p, &view, &tour.conflicts),
        Step::Arrange => workspaces(ui, p, &tour.board),
        Step::TryIt => try_it::view(ui, p, &view, &tour.play, still),
    };
    let width = step_width(view.step);
    let page = col()
        .key(format!("step-{}", view.step.key()))
        .w(width)
        .transition(step_transition(view.direction))
        .child(step_body);
    let top = row()
        .window_drag()
        .key("tour-top")
        .w_full()
        .h(48.0)
        .px(20.0)
        .center_y()
        .justify_end()
        .child(
            IconButton::new(&rune_theme_opencode::icons::CLOSE)
                .variant(IconButtonVariant::Ghost)
                .control_size(ControlSize::Large)
                .label("Close setup tour")
                .on_press(|ui: &mut Ui| input(ui, Input::Dismiss))
                .build(p.k)
                .key("tour-close"),
        );
    Some(
        col()
            .key("tour")
            .role("dialog")
            .label("Fractile setup")
            .absolute(0.0, 0.0)
            .w_full()
            .h_full()
            .fill(p.k.c.bg_deep)
            .transition(Transition::blur_replace())
            .on_press(|_: &mut Ui| {})
            .child(top)
            .child(
                col().key("tour-page").grow(1.0).shrink().center().child(
                    col()
                        .key("tour-column")
                        .w(width)
                        .motion(STEP_ENTER)
                        .child(page),
                ),
            )
            .child(
                col()
                    .key("tour-bottom")
                    .pt(12.0)
                    .pb(28.0)
                    .center()
                    .child(footer(p, &view, tour.board.nothing_to_arrange(), width)),
            ),
    )
}

/// The workspaces step widens to hold five cards across.
fn step_width(step: Step) -> f32 {
    if step == Step::Arrange {
        arrange::WIDTH
    } else {
        WIDTH
    }
}

fn step_transition(direction: i8) -> Transition {
    if direction == 0 {
        return Transition::blur_replace();
    }
    let sign = if direction > 0 { 1.0 } else { -1.0 };
    let enter = Transition::offset(sign * 24.0, 0.0).combined(Transition::opacity());
    let exit = Transition::offset(-sign * 24.0, 0.0).combined(Transition::opacity());
    Transition::asymmetric(enter, exit).springs(SpringPair::new(STEP_ENTER, STEP_EXIT))
}

/// What the primary button says and whether it leads on to another step.
fn primary_label(view: &View, nothing_to_arrange: bool) -> (&'static str, bool) {
    match view.step {
        Step::Welcome => ("Get started", true),
        Step::Setup => ("Continue", true),
        // Tiling is already on when the tour is re-run from Settings.
        Step::Arrange if view.tiling => ("Arrange", false),
        Step::Arrange if nothing_to_arrange => ("Start tiling", false),
        Step::Arrange => ("Arrange & start", false),
        Step::TryIt => ("Done", false),
    }
}

fn footer(p: Paint, view: &View, nothing_to_arrange: bool, width: f32) -> El {
    let (label, forward) = primary_label(view, nothing_to_arrange);
    let enabled = view.step != Step::Setup || view.ready;
    let mut btn = Button::new(label)
        .variant(ButtonVariant::Contrast)
        .control_size(ControlSize::Large);
    if forward {
        btn = btn.label_with(move |font, ink| {
            row()
                .gap(6.0)
                .center_y()
                .child(text(label).key("label").font(font).color(ink))
                .child(icon(&glyphs::ARROW_RIGHT, 16.0).key("arrow").color(ink))
        });
    }
    let primary = if enabled {
        btn.on_press_tx(|ui: &mut Ui, tx: &mut Tx| advance(ui, Input::Continue, tx))
            .build(p.k)
            .px(12.0)
            .radius(p.k.radius.md)
            .key("tour-continue")
            .default_button()
    } else {
        btn.disabled(true)
            .build(p.k)
            .px(12.0)
            .radius(p.k.radius.md)
            .opacity(if p.k.dark { 0.3 } else { 1.0 })
            .key("tour-continue")
    };
    let start = match view.step {
        Step::TryIt => Some(controls::quiet_button(
            p,
            "tour-undo",
            "Undo & restore",
            |ui, _| input(ui, Input::Undo),
        )),
        _ if view.can_go_back => Some(controls::button(
            p,
            "tour-back",
            "Back",
            ButtonKind::Secondary,
            |ui, _| input(ui, Input::Back),
        )),
        _ => None,
    };
    let skip = (view.step == Step::Arrange).then(|| {
        controls::button(p, "tour-skip", "Skip", ButtonKind::Secondary, |ui, tx| {
            advance(ui, Input::Skip, tx);
        })
        .help("Leave every window where it is")
    });
    row()
        .key("tour-footer")
        .w(width)
        .motion(STEP_ENTER)
        .h(32.0)
        .center_y()
        .child(row().key("start").grow(1.0).w(0.0).children(start))
        .child(row().key("middle").center().children(step_dots(p, view)))
        .child(
            row()
                .key("end")
                .grow(1.0)
                .w(0.0)
                .gap(8.0)
                .justify_end()
                .children(skip)
                .child(primary),
        )
}

/// Moves the tour on, and gives the keyboard to the miniature desktop when that lands on Try it.
fn advance(ui: &mut Ui, to: Input, tx: &mut Tx) {
    input(ui, to);
    if ui
        .tour
        .as_ref()
        .is_some_and(|tour| tour.flow.step() == Step::TryIt)
    {
        tx.focus(stage::KEY, false);
    }
}

fn step_dots(p: Paint, view: &View) -> Option<El> {
    if view.count < 2 {
        return None;
    }
    let mut dots = row().key("step-dots").gap(6.0).center_y();
    for index in 0..view.count {
        let current = index == view.index;
        dots = dots.child(
            el().key(format!("dot{index}"))
                .size(if current { 16.0 } else { 6.0 }, 6.0)
                .radius(3.0)
                .fill(if current { p.text() } else { p.k.c.border_base })
                .motion(STEP_ENTER),
        );
    }
    Some(dots)
}

pub(super) fn title_block(p: Paint, heading: &str, body: Option<&str>) -> El {
    col()
        .key("title-block")
        .gap(10.0)
        .center()
        .child(
            text(heading)
                .key("title")
                .font(title_style(p.k.dark))
                .color(p.text()),
        )
        .children(body.map(|body| {
            text(body)
                .key("body")
                .font(LEAD)
                .color(p.muted())
                .align(rune::Align::Center)
                .wrap()
                .w(440.0)
        }))
}

fn welcome(p: Paint) -> El {
    col()
        .key("welcome")
        .gap(28.0)
        .child(welcome_hero(p))
        .child(title_block(
            p,
            "Spaces, tiled.",
            Some(
                "Your windows stay put until you start. Pausing or quitting puts every window back.",
            ),
        ))
}

/// Fractile's split-tile mark inside a concentric halo ring.
fn welcome_hero(p: Paint) -> El {
    let c = &p.k.c;
    let badge_ink = rune::oklch(0.21, 0.012, 265.0);
    let tile_ink = rune::oklch(0.97, 0.012, 270.0);
    let mut badge = el()
        .key("mark-badge")
        .size(92.0, 92.0)
        .radius(22.0)
        .fill(badge_ink)
        .border(p.hair, Color::WHITE.alpha(0.10))
        .box_shadow(Shadow {
            y: 2.0,
            blur: 4.0,
            color: Color::BLACK.alpha(if p.k.dark { 0.45 } else { 0.16 }),
            ..Shadow::default()
        })
        .box_shadow(Shadow {
            y: 12.0,
            blur: 28.0,
            color: Color::BLACK.alpha(if p.k.dark { 0.55 } else { 0.18 }),
            ..Shadow::default()
        })
        .child(
            el().key("tile-master")
                .size(34.0, 44.0)
                .radius(7.0)
                .fill(tile_ink)
                .absolute(17.0, 24.0),
        )
        .child(
            el().key("tile-top")
                .size(22.0, 20.5)
                .radius(5.5)
                .fill(tile_ink.alpha(0.88))
                .absolute(53.0, 24.0),
        )
        .child(
            el().key("tile-bottom")
                .size(22.0, 20.5)
                .radius(5.5)
                .fill(tile_ink.alpha(0.66))
                .absolute(53.0, 47.5),
        );
    if !p.k.dark {
        badge = badge.border(p.hair, Color::BLACK.alpha(0.12));
    }
    row().key("hero").w_full().h(132.0).center().child(
        row()
            .key("halo")
            .padding(16.0)
            .radius(38.0)
            .border(p.hair, c.border_base.alpha(0.35))
            .child(badge),
    )
}

/// The permission step asks for one thing. Under it, one quiet row for each thing that would
/// fight Fractile for the same windows and is really on; nothing when none is.
fn setup(p: Paint, view: &View, conflicts: &[Conflict]) -> El {
    let waiting = view.accessibility == controls::PermissionControl::Waiting;
    let access_desc = if view.ready {
        (
            "Moves windows and listens for Option shortcuts.".to_owned(),
            p.muted(),
        )
    } else if waiting {
        (fractile::permission_guide::add_instructions(), p.warning())
    } else {
        (
            "Needed to move windows and register Option shortcuts.".to_owned(),
            p.muted(),
        )
    };
    let access_kind = if view.ready {
        ButtonKind::Secondary
    } else {
        ButtonKind::Primary
    };
    let access_row = controls::icon_row(
        p,
        "setup-accessibility",
        &ACCESSIBILITY_ICON,
        fractile::permission_guide::list_name(),
        Some(access_desc),
        controls::permission_control(p, "setup-grant", view.accessibility, access_kind, |ui| {
            input(ui, Input::GrantAccessibility);
        }),
    );
    let conflict_rows: Vec<El> = conflicts
        .iter()
        .enumerate()
        .map(|(index, conflict)| {
            controls::setting_row(
                p,
                &format!("conflict-{}", conflict.key()),
                &conflict.label(),
                None,
                controls::button(
                    p,
                    "resolve",
                    conflict.action(),
                    ButtonKind::Secondary,
                    move |ui, _| resolve(ui, index),
                ),
            )
        })
        .collect();
    col()
        .key("setup")
        .gap(24.0)
        .child(title_block(p, "Give Fractile window access", None))
        .child(
            col()
                .key("setup-cards")
                .gap(12.0)
                .child(controls::card(p, "setup-card", vec![access_row]))
                .children(
                    (!conflict_rows.is_empty())
                        .then(|| controls::card(p, "conflict-card", conflict_rows)),
                ),
        )
}

fn workspaces(ui: &Ui, p: Paint, board: &Board) -> El {
    col()
        .key("workspaces")
        .gap(24.0)
        .center()
        .child(title_block(p, "Your workspaces", None))
        .child(arrange::view(ui, p, board))
}
