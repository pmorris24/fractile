//! Space around and between tiles: `[settings.gaps]`, `settings.single_window_max_width` and
//! `settings.single_window_max_height`.
//!
//! The *usable* rect is the output less its outer padding. The layout tree partitions the
//! *canvas*, the usable rect grown by half the inner gap on every side, and its weights, resize
//! and fitting math stay in that space untouched. Every tile is then inset by that same half gap
//! on every side: neighbors sit exactly `inner` apart, the outermost tiles meet the usable rect
//! exactly, and equal weights still give equal windows, however deeply the splits nest.

use serde::{Deserialize, Serialize};

use crate::Rect;

/// Largest `gaps.inner`, in points.
pub const INNER_MAX: i32 = 48;
/// Largest outer padding on one screen edge, in points.
pub const EDGE_MAX: i32 = 128;
/// The padding of every screen edge `config.toml` leaves out. Only the file has it: [`Spacing`]
/// holds the four edges it resolves to.
pub const OUTER: &str = "gaps.outer";
/// Outer padding gives way before the usable rect gets narrower or shorter than this.
pub const MIN_USABLE: (i32, i32) = (320, 240);
/// Largest `single_window_max_width` and `single_window_max_height`, in points.
pub const MAX_WIDTH_LIMIT: i32 = 16_384;
/// Smallest `single_window_max_width` and `single_window_max_height` other than `0` (off).
pub const MIN_SINGLE: i32 = 320;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Spacing {
    /// Between tiled windows.
    pub inner: i32,
    /// Outer padding on each screen edge.
    pub top: i32,
    pub bottom: i32,
    pub left: i32,
    pub right: i32,
    /// A lone tile (one tiled window, or Fullscreen Layout) is centered at this width when the
    /// usable rect is wider; `0` is off.
    pub single_window_max_width: i32,
    /// A lone tile is centered at this height when the usable rect is taller; `0` is off. It
    /// is read only with `[labs] single_window_height` on (`Config::spacing` clears it).
    pub single_window_max_height: i32,
}

/// One spacing setting, as `config.toml`, `fractile config` and Settings name it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpacingKey {
    Inner,
    Top,
    Bottom,
    Left,
    Right,
    SingleWindowMaxWidth,
    SingleWindowMaxHeight,
}

impl SpacingKey {
    pub const ALL: [Self; 7] = [
        Self::Inner,
        Self::Top,
        Self::Bottom,
        Self::Left,
        Self::Right,
        Self::SingleWindowMaxWidth,
        Self::SingleWindowMaxHeight,
    ];
    pub const EDGES: [Self; 4] = [Self::Top, Self::Bottom, Self::Left, Self::Right];

    /// The key under `[settings]`: `gaps.inner`, `gaps.top`, ... or `single_window_max_width` /
    /// `single_window_max_height`.
    #[must_use]
    pub const fn path(self) -> &'static str {
        match self {
            Self::Inner => "gaps.inner",
            Self::Top => "gaps.top",
            Self::Bottom => "gaps.bottom",
            Self::Left => "gaps.left",
            Self::Right => "gaps.right",
            Self::SingleWindowMaxWidth => "single_window_max_width",
            Self::SingleWindowMaxHeight => "single_window_max_height",
        }
    }

    /// The key inside `[settings.gaps]`, or `None` for the lone-window caps.
    #[must_use]
    pub const fn gap_name(self) -> Option<&'static str> {
        match self {
            Self::Inner => Some("inner"),
            Self::Top => Some("top"),
            Self::Bottom => Some("bottom"),
            Self::Left => Some("left"),
            Self::Right => Some("right"),
            Self::SingleWindowMaxWidth | Self::SingleWindowMaxHeight => None,
        }
    }

    const fn is_edge(self) -> bool {
        matches!(self, Self::Top | Self::Bottom | Self::Left | Self::Right)
    }

    /// The value while `config.toml` leaves the key out: [`OUTER`], if set, at a screen edge.
    #[must_use]
    pub const fn fallback(self, outer: Option<i32>) -> i32 {
        match outer {
            Some(outer) if self.is_edge() => outer,
            _ => 0,
        }
    }

    /// Whether `value` needs its key in `config.toml`. Zero is the default and does without,
    /// except at an edge while [`OUTER`] is set, which would pad that edge again.
    #[must_use]
    pub const fn is_written(self, value: i32, outer: Option<i32>) -> bool {
        value != 0 || (outer.is_some() && self.is_edge())
    }

    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        let raw = raw.strip_prefix("settings.").unwrap_or(raw);
        Self::ALL.into_iter().find(|key| key.path() == raw)
    }

    #[must_use]
    pub const fn get(self, spacing: &Spacing) -> i32 {
        match self {
            Self::Inner => spacing.inner,
            Self::Top => spacing.top,
            Self::Bottom => spacing.bottom,
            Self::Left => spacing.left,
            Self::Right => spacing.right,
            Self::SingleWindowMaxWidth => spacing.single_window_max_width,
            Self::SingleWindowMaxHeight => spacing.single_window_max_height,
        }
    }

    pub const fn set(self, spacing: &mut Spacing, value: i32) {
        match self {
            Self::Inner => spacing.inner = value,
            Self::Top => spacing.top = value,
            Self::Bottom => spacing.bottom = value,
            Self::Left => spacing.left = value,
            Self::Right => spacing.right = value,
            Self::SingleWindowMaxWidth => spacing.single_window_max_width = value,
            Self::SingleWindowMaxHeight => spacing.single_window_max_height = value,
        }
    }

    /// `value` as this setting accepts it, or why it doesn't.
    pub fn check(self, value: i64) -> Result<i32, String> {
        let key = self.path();
        let max = match self {
            Self::Inner => INNER_MAX,
            Self::Top | Self::Bottom | Self::Left | Self::Right => EDGE_MAX,
            Self::SingleWindowMaxWidth | Self::SingleWindowMaxHeight => {
                if value == 0
                    || (i64::from(MIN_SINGLE)..=i64::from(MAX_WIDTH_LIMIT)).contains(&value)
                {
                    return Ok(value as i32);
                }
                return Err(format!(
                    "settings.{key} must be 0 (off) or between {MIN_SINGLE} and {MAX_WIDTH_LIMIT} px, got {value}"
                ));
            }
        };
        check_points(key, value, max)
    }
}

/// `value` as [`OUTER`] accepts it, or why it doesn't.
pub fn check_outer(value: i64) -> Result<i32, String> {
    check_points(OUTER, value, EDGE_MAX)
}

/// The span `start..start + length` narrowed to `max` and centered in it; `max <= 0` (off) or
/// `max >= length` keeps it whole.
const fn cap_span(start: i32, length: i32, max: i32) -> (i32, i32) {
    if max <= 0 || max >= length {
        (start, length)
    } else {
        (start + (length - max) / 2, max)
    }
}

fn check_points(key: &str, value: i64, max: i32) -> Result<i32, String> {
    if (0..=i64::from(max)).contains(&value) {
        Ok(value as i32)
    } else {
        Err(format!(
            "settings.{key} must be between 0 and {max} px, got {value}"
        ))
    }
}

impl Spacing {
    pub const NONE: Self = Self {
        inner: 0,
        top: 0,
        bottom: 0,
        left: 0,
        right: 0,
        single_window_max_width: 0,
        single_window_max_height: 0,
    };

    #[must_use]
    pub const fn is_none(&self) -> bool {
        self.inner == 0
            && self.top == 0
            && self.bottom == 0
            && self.left == 0
            && self.right == 0
            && self.single_window_max_width == 0
            && self.single_window_max_height == 0
    }

    /// Every screen edge at `padding`.
    #[must_use]
    pub const fn with_edges(self, padding: i32) -> Self {
        Self {
            top: padding,
            bottom: padding,
            left: padding,
            right: padding,
            ..self
        }
    }

    /// The four screen edges' padding when they match.
    #[must_use]
    pub const fn uniform_edges(&self) -> Option<i32> {
        if self.top == self.bottom && self.top == self.left && self.top == self.right {
            Some(self.top)
        } else {
            None
        }
    }

    /// `output` less the outer padding. Padding gives way, each side in proportion, before the
    /// rect drops below [`MIN_USABLE`]; an output already smaller keeps its whole size.
    #[must_use]
    pub fn usable(self, output: Rect) -> Rect {
        let (x, width) = inset_span(output.x, output.width, self.left, self.right, MIN_USABLE.0);
        let (y, height) = inset_span(output.y, output.height, self.top, self.bottom, MIN_USABLE.1);
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    /// What the layout tree partitions: `usable` grown by half the inner gap on every side.
    #[must_use]
    pub fn canvas(self, usable: Rect) -> Rect {
        let (lead, trail) = self.halves();
        Rect {
            x: usable.x - lead,
            y: usable.y - lead,
            width: usable.width + lead + trail,
            height: usable.height + lead + trail,
        }
    }

    /// A lone tile's frame in `usable`: centered at `single_window_max_width` when narrower, and
    /// at `single_window_max_height` when shorter, each axis on its own.
    #[must_use]
    pub fn single(self, usable: Rect) -> Rect {
        let (x, width) = cap_span(usable.x, usable.width, self.single_window_max_width);
        let (y, height) = cap_span(usable.y, usable.height, self.single_window_max_height);
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    /// The window frame for `cell`, one tile of the layout's partition of the canvas.
    #[must_use]
    pub fn tile(self, cell: Rect) -> Rect {
        let (lead, trail) = self.halves();
        Rect {
            x: cell.x + lead,
            y: cell.y + lead,
            width: (cell.width - lead - trail).max(1),
            height: (cell.height - lead - trail).max(1),
        }
    }

    /// [`Spacing::tile`] undone: a window's frame back in the canvas, so a window dragged to a
    /// new size reads as the boundary the user moved.
    #[must_use]
    pub fn untile(self, frame: Rect) -> Rect {
        let (lead, trail) = self.halves();
        Rect {
            x: frame.x - lead,
            y: frame.y - lead,
            width: frame.width + lead + trail,
            height: frame.height + lead + trail,
        }
    }

    /// Window frames for a layout's cells: a lone tile is [`Spacing::single`] in `usable`,
    /// every other tile [`Spacing::tile`] of its cell.
    #[must_use]
    pub fn place<T>(self, usable: Rect, cells: Vec<(T, Rect)>) -> Vec<(T, Rect)> {
        let lone = cells.len() == 1;
        cells
            .into_iter()
            .map(|(id, cell)| {
                let frame = if lone {
                    self.single(usable)
                } else {
                    self.tile(cell)
                };
                (id, frame)
            })
            .collect()
    }

    /// The inner gap split in two, the leading (left and top) half taking an odd point, so two
    /// neighbors always add up to exactly `inner`.
    const fn halves(self) -> (i32, i32) {
        let inner = if self.inner > 0 { self.inner } else { 0 };
        (inner - inner / 2, inner / 2)
    }
}

/// One axis of [`Spacing::usable`].
fn inset_span(start: i32, length: i32, lead: i32, trail: i32, minimum: i32) -> (i32, i32) {
    let (lead, trail) = (lead.max(0), trail.max(0));
    let room = (length - minimum).max(0);
    let total = lead + trail;
    if total == 0 || room == 0 {
        return (start, length);
    }
    let (lead, trail) = if total <= room {
        (lead, trail)
    } else {
        let lead = (i64::from(lead) * i64::from(room) / i64::from(total)) as i32;
        (lead, room - lead)
    };
    (start + lead, length - lead - trail)
}

#[cfg(test)]
mod tests;
