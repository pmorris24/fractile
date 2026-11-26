//! `[settings]` keys that pick one of a few named choices: where the pointer goes on focus
//! changes, and how the menu bar icon looks.

use serde::{Deserialize, Serialize};

/// Whether Fractile moves the pointer when an explicit focus action lands elsewhere:
/// `settings.mouse_follows_focus`.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum MouseFollowsFocus {
    /// Leave the pointer where it is.
    #[default]
    Off,
    /// Move the pointer to the focused window (or empty display) when it is on another display.
    Display,
    /// Move the pointer to the focused window (or empty display) whenever it is outside it.
    Window,
}

impl MouseFollowsFocus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Display => "display",
            Self::Window => "window",
        }
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "off" => Some(Self::Off),
            "display" => Some(Self::Display),
            "window" => Some(Self::Window),
            _ => None,
        }
    }
}

/// How the menu bar icon looks: `settings.menu_bar_icon`, read only with
/// `[labs] menu_bar_icon_style` on.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum MenuBarIcon {
    /// One layout glyph and workspace label per display.
    #[default]
    Full,
    /// One glyph and label: the focused display's workspace.
    Compact,
    /// No menu bar icon; Settings stays a relaunch away.
    Off,
}

impl MenuBarIcon {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Compact => "compact",
            Self::Off => "off",
        }
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "full" => Some(Self::Full),
            "compact" => Some(Self::Compact),
            "off" => Some(Self::Off),
            _ => None,
        }
    }
}
