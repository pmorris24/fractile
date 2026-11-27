//! Fractile's mark and wordmark, as Settings and the setup tour show them. The source artwork
//! lives in `brand/`; the app icon is `app/AppIcon.icon`.

use rune::{Color, El, Icon, icon, row, text};

use crate::controls::Paint;

/// The F mark, drawn in the current colour.
pub static MARK: Icon = Icon {
    name: "fractile-mark",
    view: 120.0,
    body: include_str!("../assets/mark.svg"),
};

/// The brand's warm off-white and near-black, as the app icon uses them.
pub fn paper() -> Color {
    crate::hud::pill_srgb(0xeeece6)
}

pub fn ink() -> Color {
    crate::hud::pill_srgb(0x111111)
}

/// The mark `size` points tall beside the lowercase wordmark in the strong type role, in the
/// text colour.
pub fn lockup(p: Paint, size: f32) -> El {
    row()
        .key("brand-lockup")
        .label("Fractile")
        .gap((size * 0.32).round())
        .center_y()
        .child(icon(&MARK, size).color(p.text()))
        .child(
            text("fractile")
                .key("wordmark")
                .font(p.strong())
                .color(p.text()),
        )
}
