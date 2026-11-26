//! The one drawing of a shortcut's keys, as Hex's: low-profile scissor keys after Apple's Magic
//! Keyboard, a flat face on a cap whose skirt shows below it, resting in a tray recessed into
//! the card. A held key sinks its face into the skirt; the key's bounds never change. Sizes are
//! `KEYS` (`tokens.rs`).

use fractile::shortcuts::Chord;
use rune::{Color, Edge, El, Shadow, Spring, SpringPair, TextStyle, Transition, col, row, text};
use rune_kit::Kit;

use super::{KEYS, Paint};

/// A press sinks the face most of the way, leaving a hairline of skirt.
const TRAVEL: f32 = KEYS.skirt - KEYS.rim;
/// The legend on a key.
const LEGEND: TextStyle = TextStyle::new(11.0, 400.0).lh(14.0);
/// A face sinks fast and comes back up a little slower, both without bounce.
const KEY_DOWN: Spring = Spring::new(0.08, 0.0);
const KEY_UP: Spring = Spring::new(0.18, 0.0);

/// The keys' material, from the theme's tokens: in light a warm off-white face on a cap that
/// shades to warm grey; in dark a space-black face on a cap that falls to the deepest
/// background.
struct Material {
    cap_top: Color,
    cap_bottom: Color,
    face: Color,
    ink: Color,
    /// The tight shadow under the cap and the soft one it casts at rest.
    contact: f32,
    cast: f32,
}

impl Material {
    fn new(k: &Kit) -> Self {
        let c = &k.c;
        if k.dark {
            Self {
                cap_top: c.layer_03.mix(c.layer_04, 0.35),
                cap_bottom: c.bg_deep,
                face: c.bg_base.mix(c.layer_01, 0.6),
                ink: c.text_base.mix(c.text_faint, 0.15),
                contact: 0.8,
                cast: 0.5,
            }
        } else {
            let warm = c.warning.bg;
            Self {
                cap_top: c.white,
                cap_bottom: c.layer_04.mix(c.text_faint, 0.28).mix(c.warning.fg, 0.05),
                face: c.layer_02.mix(warm, 0.35),
                ink: c.text_base.mix(c.text_muted, 0.15),
                contact: 0.2,
                cast: 0.12,
            }
        }
    }
}

/// One key in a tray whose keys can be down: which key it is, its legend, and whether it is
/// down. A key keeps its place in the tray by which key it is, so when a chord changes only its
/// last key, the others stay where they are and that key's legend rolls over.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Held {
    pub id: &'static str,
    pub legend: String,
    pub down: bool,
}

/// One key. `down` sinks the face into the skirt, tightens the cap's shadow and shades the
/// face toward its bottom edge.
pub fn keycap(p: Paint, index: usize, label: &str, down: bool) -> El {
    cap(p, format!("keycap{index}"), label, down, false)
}

/// A key whose legend rolls up and over to its next one inside the face, which springs to the
/// new legend's width.
fn cap(p: Paint, key: String, label: &str, down: bool, rolls: bool) -> El {
    let k = p.k;
    let m = Material::new(k);
    let pressed = if down { 1.0 } else { 0.0 };
    let travel = TRAVEL * pressed;
    let face_height = KEYS.key_h - KEYS.rim - KEYS.skirt;
    let legend = text(label.to_owned()).font(LEGEND).color(m.ink);
    let legend = if rolls {
        legend
            .key(format!("legend-{label}"))
            // The old legend is gone before the key's width closes over it.
            .transition(roll().springs(SpringPair::new(KEY_UP, LEGEND_OUT)))
    } else {
        legend.key("label")
    };
    let mut face = row()
        .key("face")
        .min_w(KEYS.key_h - 2.0 * KEYS.rim)
        .h(face_height)
        .px(KEYS.face_px)
        .center()
        .radius(KEYS.face_radius)
        .continuous_corners()
        .fill(m.face)
        .gradient(
            k.light(0.03 - 0.03 * pressed),
            k.dark_a(0.02 + 0.06 * pressed),
        )
        .child(legend);
    if rolls {
        face = face.clip_rounded(KEYS.face_radius);
    }
    // Shadow offsets never spring, so the resting and pressed casts are two layers that
    // trade strength.
    let cast = |y: f32, blur: f32, strength: f32| Shadow {
        y,
        blur,
        color: k.dark_a(m.cast * strength),
        ..Shadow::default()
    };
    let cap = col()
        .key("cap")
        .px(KEYS.rim)
        .pt(KEYS.rim)
        .pb(KEYS.skirt - travel)
        .radius(KEYS.cap_radius())
        .continuous_corners()
        .fill(m.cap_bottom)
        .gradient(m.cap_top, m.cap_bottom)
        .shadow(0.5, 0.5, k.dark_a(m.contact))
        .box_shadow(cast(KEYS.skirt, 2.0 * KEYS.skirt, 1.0 - pressed))
        .box_shadow(cast(KEYS.rim, 2.0, pressed))
        .child(face);
    col()
        .key(key)
        .h(KEYS.key_h)
        .pt(travel)
        .motion(if down { KEY_DOWN } else { KEY_UP })
        .child(cap)
}

/// A chord's keys at rest in their well, lit from above so its top edge falls into shadow.
/// Keys a new chord adds rise into the well and keys it drops leave through its top.
pub fn tray(p: Paint, key: &str, caps: &[String]) -> El {
    tray_of(p, key, caps.iter().map(|cap| (cap.as_str(), false)))
}

/// A tray whose keys can be down and stay put from one chord to the next: a key the next chord
/// adds springs in between its neighbours as they part, one it drops leaves as they close up,
/// and the tray's width follows them.
pub fn tray_held(p: Paint, key: &str, caps: &[Held]) -> El {
    well(p, key)
        .motion(KEY_UP)
        .children(caps.iter().map(|held| {
            cap(
                p,
                format!("keycap-{}", held.id),
                &held.legend,
                held.down,
                true,
            )
            .transition(Transition::scale(0.96).springs(SpringPair::new(KEY_UP, KEY_DOWN)))
        }))
}

fn tray_of<'a>(p: Paint, key: &str, caps: impl Iterator<Item = (&'a str, bool)>) -> El {
    well(p, key).children(
        caps.enumerate()
            .map(|(index, (cap, down))| keycap(p, index, cap, down).transition(roll())),
    )
}

/// The recess a chord's keys rest in.
fn well(p: Paint, key: &str) -> El {
    let k = p.k;
    let c = &k.c;
    let (top, bottom, rim, well) = if k.dark {
        (
            c.bg_deep,
            c.bg_base.mix(c.layer_01, 0.7),
            k.light(0.07),
            0.6,
        )
    } else {
        (
            c.layer_04.mix(c.layer_03, 0.35),
            c.layer_03.mix(c.layer_02, 0.6),
            k.dark_a(0.07),
            0.14,
        )
    };
    // Concentric with the caps inside it.
    let radius = KEYS.tray_radius();
    row()
        .key(key)
        .padding(KEYS.tray_pad)
        .gap(KEYS.gap)
        .center_y()
        .radius(radius)
        .continuous_corners()
        .clip_rounded(radius)
        .fill(bottom)
        .gradient(top, bottom)
        .border(p.hair.max(0.5), rim)
        .box_shadow(Shadow {
            y: 1.0,
            blur: 2.5,
            color: k.dark_a(well),
            inset: true,
            ..Shadow::default()
        })
}

const LEGEND_OUT: Spring = Spring::new(0.1, 0.0);

/// In from the bottom, out through the top, inside the parent's clip.
pub fn roll() -> Transition {
    Transition::push(Edge::Bottom)
}

/// A shortcut's chords as trays joined by "or" (two at most), or "Not set" in the same slot.
pub fn chords(p: Paint, chords: &[Chord]) -> El {
    let mut keys = row().key("keys").gap(6.0).center_y();
    if chords.is_empty() {
        keys = keys.child(
            row().key("off").h(KEYS.tray_h()).px(8.0).center().child(
                text("Not set")
                    .key("label")
                    .font(p.row_title())
                    .color(p.faint()),
            ),
        );
    }
    for (index, chord) in chords.iter().take(2).enumerate() {
        if index > 0 {
            keys = keys.child(text("or").key("or").font(p.meta()).color(p.faint()));
        }
        keys = keys.child(tray(p, &format!("tray{index}"), &chord.keycaps()));
    }
    keys
}
