//! A points slider for one spacing value. Dragging only redraws the preview; letting go saves
//! `config.toml` once, and the arrow keys step and save a point at a time.

use fractile::spacing::Spacing;
use rune::{Align, Drag, El, Phase, Tx, el, row, text};

use crate::{Ui, controls::Paint};

const WIDTH: f32 = 168.0;
const HEIGHT: f32 = 20.0;
const TRACK: f32 = 4.0;
const THUMB: f32 = 16.0;
const READOUT_WIDTH: f32 = 44.0;

/// How a slider reads its value from, and writes it into, the spacing.
#[derive(Clone, Copy)]
pub(super) struct Channel {
    pub get: fn(&Spacing) -> i32,
    pub set: fn(Spacing, i32) -> Spacing,
    pub max: i32,
}

/// The spacing on screen: a drag in progress, else what the file holds.
fn current(ui: &Ui) -> Spacing {
    ui.spacing_draft.unwrap_or_else(|| ui.doc.spacing())
}

/// The value at `x` points along a slider `width` wide.
pub(super) fn value_at(x: f32, width: f32, max: i32) -> i32 {
    let travel = (width - THUMB).max(1.0);
    let fraction = ((x - THUMB / 2.0) / travel).clamp(0.0, 1.0);
    (fraction * max as f32).round() as i32
}

pub(super) fn slider(p: Paint, key: &str, label: &str, value: i32, channel: Channel) -> El {
    let c = &p.k.c;
    let fraction = (value as f32 / channel.max.max(1) as f32).clamp(0.0, 1.0);
    let thumb_x = fraction * (WIDTH - THUMB);
    let track = el()
        .key("track")
        .absolute(0.0, (HEIGHT - TRACK) / 2.0)
        .size(WIDTH, TRACK)
        .radius(TRACK / 2.0)
        .fill(c.layer_03);
    let filled = el()
        .key("fill")
        .absolute(0.0, (HEIGHT - TRACK) / 2.0)
        .size(thumb_x + THUMB / 2.0, TRACK)
        .radius(TRACK / 2.0)
        .fill(c.text_accent);
    let mut thumb = el()
        .key("thumb")
        .absolute(thumb_x, (HEIGHT - THUMB) / 2.0)
        .size(THUMB, THUMB)
        .radius(THUMB / 2.0)
        .fill(c.bg_button_neutral)
        .border(p.hair, c.border_base);
    for shadow in p.k.shadow_layers(&p.k.elevation.button_neutral) {
        thumb = thumb.box_shadow(shadow);
    }
    let bar = el()
        .key(key)
        .role("slider")
        .label(label)
        .size(WIDTH, HEIGHT)
        .on_drag(move |ui: &mut Ui, drag: &mut Drag, tx: &mut Tx| {
            let base = current(ui);
            let at = value_at(drag.start_x + drag.dx, drag.w, channel.max);
            match drag.phase {
                Phase::Down => ui.spacing_draft = Some((channel.set)(base, at)),
                Phase::Move => {
                    tx.instant();
                    ui.spacing_draft = Some((channel.set)(base, at));
                }
                Phase::Up if drag.cancelled() => ui.spacing_draft = None,
                Phase::Up => ui.set_spacing((channel.set)(base, at)),
            }
        })
        .on_step(move |ui: &mut Ui, delta, _: &mut Tx| {
            let base = ui.doc.spacing();
            let value = ((channel.get)(&base) + delta).clamp(0, channel.max);
            ui.set_spacing((channel.set)(base, value));
        })
        .focus_ring(2.0, 2.0, c.border_focus)
        .child(track)
        .child(filled)
        .child(thumb);
    row()
        .key(format!("{key}-control"))
        .gap(12.0)
        .center_y()
        .child(bar)
        .child(
            text(format!("{value} px"))
                .key("readout")
                .font(p.tabular(p.meta()))
                .color(p.muted())
                .w(READOUT_WIDTH)
                .align(Align::End),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_thumb_centre_maps_to_the_value() {
        assert_eq!(value_at(0.0, WIDTH, 32), 0);
        assert_eq!(value_at(THUMB / 2.0, WIDTH, 32), 0);
        assert_eq!(value_at(WIDTH / 2.0, WIDTH, 32), 16);
        assert_eq!(value_at(WIDTH - THUMB / 2.0, WIDTH, 32), 32);
        assert_eq!(value_at(WIDTH + 40.0, WIDTH, 32), 32);
    }
}
