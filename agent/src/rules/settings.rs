//! `[settings]` and `[labs]` scalar accessors on `Doc`.

use spaceman::config::{
    HudPosition, HudStyle, LabFeature, LayoutMode, MenuBarIcon, MouseFollowsFocus, UpdateChannel,
};
use toml_edit::{Item, Table, value};

use super::{Doc, set_table_item};

impl Doc {
    /// Whether the Workspace HUD overlay is enabled in `[settings]` (defaults to `true`).
    pub fn hud(&self) -> bool {
        self.doc
            .get("settings")
            .and_then(Item::as_table_like)
            .and_then(|table| table.get("hud"))
            .and_then(Item::as_bool)
            .unwrap_or(true)
    }

    pub fn set_hud(&mut self, enabled: bool) {
        self.set_setting("hud", (!enabled).then(|| value(false)));
    }

    /// How the Workspace HUD draws its row, from `settings.hud_style` (defaults to icons).
    pub fn hud_style(&self) -> HudStyle {
        self.doc
            .get("settings")
            .and_then(Item::as_table_like)
            .and_then(|table| table.get("hud_style"))
            .and_then(Item::as_str)
            .and_then(HudStyle::parse)
            .unwrap_or_default()
    }

    pub fn set_hud_style(&mut self, style: HudStyle) {
        self.set_setting(
            "hud_style",
            (style != HudStyle::default()).then(|| value(style.as_str())),
        );
    }

    /// Where the Workspace HUD capsule sits, from `settings.hud_position` (defaults to the
    /// bottom of each display).
    pub fn hud_position(&self) -> HudPosition {
        self.doc
            .get("settings")
            .and_then(Item::as_table_like)
            .and_then(|table| table.get("hud_position"))
            .and_then(Item::as_str)
            .and_then(HudPosition::parse)
            .unwrap_or_default()
    }

    /// Whether the Workspace HUD stays on screen, from `settings.hud_stay` (defaults to off).
    pub fn hud_stay(&self) -> bool {
        self.doc
            .get("settings")
            .and_then(Item::as_table_like)
            .and_then(|table| table.get("hud_stay"))
            .and_then(Item::as_bool)
            .unwrap_or(false)
    }

    pub fn set_hud_stay(&mut self, stay: bool) {
        self.set_setting("hud_stay", stay.then(|| value(true)));
    }

    pub fn set_hud_position(&mut self, position: HudPosition) {
        self.set_setting(
            "hud_position",
            (position != HudPosition::default()).then(|| value(position.as_str())),
        );
    }

    /// How newly opened tiled windows split the workspace, from `settings.layout` (defaults to
    /// dwindle).
    pub fn layout(&self) -> LayoutMode {
        self.doc
            .get("settings")
            .and_then(Item::as_table_like)
            .and_then(|table| table.get("layout"))
            .and_then(Item::as_str)
            .and_then(LayoutMode::parse)
            .unwrap_or_default()
    }

    pub fn set_layout(&mut self, mode: LayoutMode) {
        self.set_setting(
            "layout",
            (mode != LayoutMode::default()).then(|| value(mode.as_str())),
        );
    }

    /// The release channel the updater follows, from `settings.update_channel` (defaults to
    /// stable).
    pub fn update_channel(&self) -> UpdateChannel {
        self.doc
            .get("settings")
            .and_then(Item::as_table_like)
            .and_then(|table| table.get("update_channel"))
            .and_then(Item::as_str)
            .and_then(UpdateChannel::parse)
            .unwrap_or_default()
    }

    pub fn set_update_channel(&mut self, channel: UpdateChannel) {
        self.set_setting(
            "update_channel",
            (channel != UpdateChannel::default()).then(|| value(channel.as_str())),
        );
    }

    /// Whether Spaceman moves the pointer on explicit focus changes, from
    /// `settings.mouse_follows_focus` (defaults to off).
    pub fn mouse_follows_focus(&self) -> MouseFollowsFocus {
        self.doc
            .get("settings")
            .and_then(Item::as_table_like)
            .and_then(|table| table.get("mouse_follows_focus"))
            .and_then(Item::as_str)
            .and_then(MouseFollowsFocus::parse)
            .unwrap_or_default()
    }

    pub fn set_mouse_follows_focus(&mut self, mode: MouseFollowsFocus) {
        self.set_setting(
            "mouse_follows_focus",
            (mode != MouseFollowsFocus::default()).then(|| value(mode.as_str())),
        );
    }

    /// How the menu bar icon looks, from `settings.menu_bar_icon` (defaults to full).
    pub fn menu_bar_icon(&self) -> MenuBarIcon {
        self.doc
            .get("settings")
            .and_then(Item::as_table_like)
            .and_then(|table| table.get("menu_bar_icon"))
            .and_then(Item::as_str)
            .and_then(MenuBarIcon::parse)
            .unwrap_or_default()
    }

    pub fn set_menu_bar_icon(&mut self, icon: MenuBarIcon) {
        self.set_setting(
            "menu_bar_icon",
            (icon != MenuBarIcon::default()).then(|| value(icon.as_str())),
        );
    }

    /// Whether a three-finger horizontal swipe steps through workspaces, from
    /// `settings.trackpad_swipe` (defaults to on).
    pub fn trackpad_swipe(&self) -> bool {
        self.doc
            .get("settings")
            .and_then(Item::as_table_like)
            .and_then(|table| table.get("trackpad_swipe"))
            .and_then(Item::as_bool)
            .unwrap_or(true)
    }

    pub fn set_trackpad_swipe(&mut self, enabled: bool) {
        self.set_setting("trackpad_swipe", (!enabled).then(|| value(false)));
    }

    /// Whether Spaceman auto-pauses when only a built-in display is connected and resumes
    /// when an external display reconnects, from `settings.pause_on_builtin_display`
    /// (defaults to false).
    pub fn pause_on_builtin_display(&self) -> bool {
        self.doc
            .get("settings")
            .and_then(Item::as_table_like)
            .and_then(|table| table.get("pause_on_builtin_display"))
            .and_then(Item::as_bool)
            .unwrap_or(false)
    }

    pub fn set_pause_on_builtin_display(&mut self, enabled: bool) {
        self.set_setting("pause_on_builtin_display", enabled.then(|| value(true)));
    }

    /// Sets `key` in `[settings]`, or removes it (and an emptied table) for `None`, the default.
    pub(super) fn set_setting(&mut self, key: &str, item: Option<Item>) {
        self.set_table_key("settings", key, item);
    }

    /// Turns `feature`'s `[labs]` switch on, or removes it (and an emptied table) to turn it off.
    pub fn set_lab(&mut self, feature: LabFeature, on: bool) {
        self.set_table_key("labs", feature.key(), on.then(|| value(true)));
    }

    /// Sets `key` in the `[name]` table, or removes it (and an emptied table) for `None`.
    fn set_table_key(&mut self, name: &str, key: &str, item: Option<Item>) {
        let Some(item) = item else {
            let empty =
                if let Some(table) = self.doc.get_mut(name).and_then(Item::as_table_like_mut) {
                    table.remove(key);
                    table.is_empty()
                } else {
                    false
                };
            if empty {
                self.remove_table(name);
            }
            return;
        };
        let entry = self
            .doc
            .entry(name)
            .or_insert_with(|| Item::Table(Table::new()));
        if !entry.is_table_like() {
            *entry = Item::Table(Table::new());
        }
        if let Some(table) = entry.as_table_like_mut() {
            set_table_item(table, key, item);
        }
    }
}
