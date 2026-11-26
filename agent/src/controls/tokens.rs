//! The design system's layout tokens: one set of measurements for the Settings window and its
//! modals, the same as Hex's wherever both apps have the same object. See
//! `docs/design-system.md`.

use crate::hud;

/// A settings page's measurements. The pane title, section labels and row text share the
/// gutter; cards hang outward by `row_px` on both sides, so a row's control ends where a
/// section label's accessory does.
#[derive(Clone, Copy)]
pub struct Metrics {
    /// The page's text rail: the title's, section labels' and rows'.
    pub gutter: f32,
    pub section_gap: f32,
    /// From a section label to its card.
    pub label_gap: f32,
    /// A row's text inset inside its card, and how far cards hang past the gutter.
    pub row_px: f32,
    /// A one-line row.
    pub row_h: f32,
    /// A row with a title and a description.
    pub row_h_described: f32,
    /// A sidebar item, a keycap tray, a small control.
    pub row_h_compact: f32,
    pub header_h: f32,
    /// A row's leading glyph, and the column it takes with its gap.
    pub icon: f32,
    pub icon_lead: f32,
    /// A status dot's diameter.
    pub dot: f32,
    pub sidebar_w: f32,
    /// Cards stop growing here and centre in a wider panel.
    pub content_max: f32,
}

pub const METRICS: Metrics = Metrics {
    gutter: 32.0,
    section_gap: 24.0,
    label_gap: 8.0,
    row_px: 16.0,
    row_h: 48.0,
    row_h_described: 60.0,
    row_h_compact: 28.0,
    header_h: 52.0,
    icon: 16.0,
    icon_lead: 28.0,
    dot: 8.0,
    sidebar_w: 220.0,
    content_max: 720.0,
};

impl Metrics {
    /// A card's edge from the panel's: the gutter less the row inset.
    pub const fn card_edge(self) -> f32 {
        self.gutter - self.row_px
    }
}

/// The Settings window: the size it opens at, the smallest it gets, and the content panel's
/// inset from the window's top, right and bottom.
#[derive(Clone, Copy)]
pub struct Window {
    pub size: (f32, f32),
    pub min: (f32, f32),
    pub panel_inset: f32,
}

pub const WINDOW: Window = Window {
    size: (1180.0, 780.0),
    min: (860.0, 580.0),
    panel_inset: 8.0,
};

impl Window {
    /// The width cards get in a window `width` wide.
    pub const fn card_w(self, width: f32) -> f32 {
        let room = width - METRICS.sidebar_w - self.panel_inset - 2.0 * METRICS.card_edge();
        if room > METRICS.content_max {
            METRICS.content_max
        } else {
            room
        }
    }
}

/// A modal's width. A modal is a small page and reuses the page's rails: cards at
/// `METRICS.card_edge()`, its title and footer on `METRICS.gutter`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModalWidth {
    /// A confirmation, a short list.
    #[allow(dead_code)]
    Narrow = 460,
    /// A preview over one card of rows (Workspace HUD).
    Medium = 560,
    /// Two columns, or a long reference list.
    #[allow(dead_code)]
    Wide = 640,
}

impl ModalWidth {
    pub const fn px(self) -> f32 {
        self as u16 as f32
    }
}

/// A modal's vertical rhythm, and how far it stays from the window's edges.
#[derive(Clone, Copy)]
pub struct Modal {
    pub top: f32,
    pub bottom: f32,
    pub gap: f32,
    pub margin: f32,
}

pub const MODAL: Modal = Modal {
    top: 20.0,
    bottom: 16.0,
    gap: 16.0,
    margin: 24.0,
};

/// A shortcut's keys: low-profile caps in a recessed tray. Each nested corner is the one
/// outside it less the inset between them.
#[derive(Clone, Copy)]
pub struct Keys {
    /// A key's height, skirt included.
    pub key_h: f32,
    /// The skirt showing below a key at rest.
    pub skirt: f32,
    /// The cap's rim beside and above the face.
    pub rim: f32,
    pub face_radius: f32,
    pub face_px: f32,
    /// Between the keys of a chord.
    pub gap: f32,
    /// Between a tray's edge and its keys.
    pub tray_pad: f32,
    /// Between a recorder's hover fill and the tray inside it.
    pub focus_pad: f32,
}

pub const KEYS: Keys = Keys {
    key_h: 22.0,
    skirt: 1.5,
    rim: 0.5,
    face_radius: 4.0,
    face_px: 6.0,
    gap: 3.0,
    tray_pad: 3.0,
    focus_pad: 2.0,
};

impl Keys {
    pub const fn cap_radius(self) -> f32 {
        self.face_radius + self.rim
    }
    pub const fn tray_radius(self) -> f32 {
        self.cap_radius() + self.tray_pad
    }
    /// The recorder's hover and listening fill, round the tray.
    pub const fn button_radius(self) -> f32 {
        self.tray_radius() + self.focus_pad
    }
    pub const fn tray_h(self) -> f32 {
        self.key_h + 2.0 * self.tray_pad
    }
}

// The rails and the concentric radius table of docs/design-system.md, checked at compile time.
// A card's edge, on a page and in a modal.
const _: () = assert!(METRICS.card_edge() == 16.0);
// A tray of keys is a small control.
const _: () = assert!(KEYS.tray_h() == METRICS.row_h_compact);
const _: () = assert!(KEYS.cap_radius() == 4.5);
const _: () = assert!(KEYS.tray_radius() == 7.5);
const _: () = assert!(KEYS.button_radius() == 9.5);
// The HUD's marker sits concentric in its capsule: 22 - 7 = 15.
const _: () = assert!(hud::PILL_HEIGHT / 2.0 - hud::MARKER_INSET == hud::MARKER_RADIUS);
// A Medium modal clears the sidebar at the smallest window.
const _: () = assert!(
    ModalWidth::Medium.px() + 2.0 * MODAL.margin
        <= WINDOW.min.0 - METRICS.sidebar_w - WINDOW.panel_inset
);
// Cards are 600 wide at the smallest window, 660 as it opens, and stop at the content width.
const _: () = assert!(WINDOW.card_w(WINDOW.min.0) == 600.0);
const _: () = assert!(WINDOW.card_w(WINDOW.size.0) == METRICS.content_max);
const _: () = assert!(WINDOW.card_w(1200.0) == METRICS.content_max);
const _: () = assert!(WINDOW.size.0 >= WINDOW.min.0 && WINDOW.size.1 >= WINDOW.min.1);
