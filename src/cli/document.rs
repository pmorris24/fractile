use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use spaceman::{
    DisplaySelector, WorkspaceId, Workspaces,
    config::{
        Config, HudPosition, HudStyle, LabFeature, LayoutMode, MenuBarIcon, MouseFollowsFocus,
        RouteWindows, UpdateChannel, config_path, lock_config, write_config_atomic,
    },
    spacing::{Spacing, SpacingKey, check_outer},
};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ConfigDoc {
    pub version: u32,
    #[serde(default, skip_serializing_if = "SettingsDoc::is_empty")]
    pub settings: SettingsDoc,
    /// `[labs]`: the Lab switches, edited by `spaceman config set|reset labs.<key>`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub labs: Option<LabsDoc>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<RuleDoc>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub workspaces: Vec<WorkspaceDoc>,
    /// `[shortcuts]`, carried through untouched: no CLI command edits it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shortcuts: Option<ShortcutsDoc>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LabsDoc {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scratchpad: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activity: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub portrait_edge_rows: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub physical_displays: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recent_focus: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub swipe_switch: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub empty_workspace_focus: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub float_follows_display: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_steps: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub four_finger_swipe: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub single_window_height: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route_all_windows: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub menu_bar_icon_style: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focus_follows_mouse: Option<bool>,
}

impl LabsDoc {
    fn is_empty(&self) -> bool {
        self.scratchpad.is_none()
            && self.activity.is_none()
            && self.portrait_edge_rows.is_none()
            && self.physical_displays.is_none()
            && self.recent_focus.is_none()
            && self.swipe_switch.is_none()
            && self.empty_workspace_focus.is_none()
            && self.float_follows_display.is_none()
            && self.workspace_steps.is_none()
            && self.four_finger_swipe.is_none()
            && self.single_window_height.is_none()
            && self.route_all_windows.is_none()
            && self.menu_bar_icon_style.is_none()
            && self.focus_follows_mouse.is_none()
    }

    /// The switch as the file asks for it: off while unset.
    pub(crate) fn enabled(&self, feature: LabFeature) -> bool {
        self.slot(feature).unwrap_or(false)
    }

    /// Whether the file sets the switch at all.
    pub(crate) fn is_set(&self, feature: LabFeature) -> bool {
        self.slot(feature).is_some()
    }

    /// Sets the switch; off removes the key, and the table with it once empty.
    pub(crate) fn set(&mut self, feature: LabFeature, on: bool) {
        *self.slot_mut(feature) = on.then_some(true);
    }

    const fn slot(&self, feature: LabFeature) -> Option<bool> {
        match feature {
            LabFeature::Scratchpad => self.scratchpad,
            LabFeature::Activity => self.activity,
            LabFeature::PortraitEdgeRows => self.portrait_edge_rows,
            LabFeature::PhysicalDisplays => self.physical_displays,
            LabFeature::RecentFocus => self.recent_focus,
            LabFeature::SwipeSwitch => self.swipe_switch,
            LabFeature::EmptyWorkspaceFocus => self.empty_workspace_focus,
            LabFeature::FloatFollowsDisplay => self.float_follows_display,
            LabFeature::WorkspaceSteps => self.workspace_steps,
            LabFeature::FourFingerSwipe => self.four_finger_swipe,
            LabFeature::SingleWindowHeight => self.single_window_height,
            LabFeature::RouteAllWindows => self.route_all_windows,
            LabFeature::MenuBarIconStyle => self.menu_bar_icon_style,
            LabFeature::FocusFollowsMouse => self.focus_follows_mouse,
        }
    }

    const fn slot_mut(&mut self, feature: LabFeature) -> &mut Option<bool> {
        match feature {
            LabFeature::Scratchpad => &mut self.scratchpad,
            LabFeature::Activity => &mut self.activity,
            LabFeature::PortraitEdgeRows => &mut self.portrait_edge_rows,
            LabFeature::PhysicalDisplays => &mut self.physical_displays,
            LabFeature::RecentFocus => &mut self.recent_focus,
            LabFeature::SwipeSwitch => &mut self.swipe_switch,
            LabFeature::EmptyWorkspaceFocus => &mut self.empty_workspace_focus,
            LabFeature::FloatFollowsDisplay => &mut self.float_follows_display,
            LabFeature::WorkspaceSteps => &mut self.workspace_steps,
            LabFeature::FourFingerSwipe => &mut self.four_finger_swipe,
            LabFeature::SingleWindowHeight => &mut self.single_window_height,
            LabFeature::RouteAllWindows => &mut self.route_all_windows,
            LabFeature::MenuBarIconStyle => &mut self.menu_bar_icon_style,
            LabFeature::FocusFollowsMouse => &mut self.focus_follows_mouse,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ShortcutsDoc {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub defaults: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modifier: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub bindings: BTreeMap<String, String>,
}

impl Default for ConfigDoc {
    fn default() -> Self {
        Self {
            version: 1,
            settings: SettingsDoc::default(),
            labs: None,
            rules: Vec::new(),
            workspaces: Vec::new(),
            shortcuts: None,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SettingsDoc {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hud: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "serialize_optional_hud_style"
    )]
    pub hud_style: Option<HudStyle>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "serialize_optional_hud_position"
    )]
    pub hud_position: Option<HudPosition>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "serialize_optional_layout"
    )]
    pub layout: Option<LayoutMode>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "serialize_optional_update_channel"
    )]
    pub update_channel: Option<UpdateChannel>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "serialize_optional_mouse_follows_focus"
    )]
    pub mouse_follows_focus: Option<MouseFollowsFocus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trackpad_swipe: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub menu_bar_icon: Option<MenuBarIcon>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pause_on_builtin_display: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub single_window_max_width: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub single_window_max_height: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_order: Option<Vec<WorkspaceId>>,
    #[serde(default, skip_serializing_if = "GapsDoc::is_empty")]
    pub gaps: GapsDoc,
}

/// `[settings.gaps]` as written: only the keys the file sets.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GapsDoc {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inner: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outer: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bottom: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub left: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub right: Option<i64>,
}

impl GapsDoc {
    fn is_empty(&self) -> bool {
        self.inner.is_none()
            && self.outer.is_none()
            && self.top.is_none()
            && self.bottom.is_none()
            && self.left.is_none()
            && self.right.is_none()
    }
}

impl SettingsDoc {
    fn is_empty(&self) -> bool {
        self.hud.is_none()
            && self.hud_style.is_none()
            && self.hud_position.is_none()
            && self.layout.is_none()
            && self.update_channel.is_none()
            && self.mouse_follows_focus.is_none()
            && self.trackpad_swipe.is_none()
            && self.menu_bar_icon.is_none()
            && self.pause_on_builtin_display.is_none()
            && self.single_window_max_width.is_none()
            && self.single_window_max_height.is_none()
            && self.workspace_order.is_none()
            && self.gaps.is_empty()
    }

    fn spacing_slot(&mut self, key: SpacingKey) -> &mut Option<i64> {
        match key {
            SpacingKey::Inner => &mut self.gaps.inner,
            SpacingKey::Top => &mut self.gaps.top,
            SpacingKey::Bottom => &mut self.gaps.bottom,
            SpacingKey::Left => &mut self.gaps.left,
            SpacingKey::Right => &mut self.gaps.right,
            SpacingKey::SingleWindowMaxWidth => &mut self.single_window_max_width,
            SpacingKey::SingleWindowMaxHeight => &mut self.single_window_max_height,
        }
    }

    fn outer(&self) -> Option<i32> {
        self.gaps.outer.and_then(|value| check_outer(value).ok())
    }

    /// `gaps.outer` as the file sets it, whatever the edges are.
    pub(crate) fn effective_outer(&self) -> i32 {
        self.outer().unwrap_or(0)
    }

    /// Sets `gaps.outer` after checking its range; `0` removes it. Edges keep their own keys.
    pub(crate) fn set_outer(&mut self, value: i64) -> Result<i32, String> {
        let checked = check_outer(value)?;
        self.gaps.outer = (checked != 0).then_some(i64::from(checked));
        Ok(checked)
    }

    /// The spacing the file asks for, every unset key at its fallback.
    pub(crate) fn effective_spacing(&self) -> Spacing {
        let mut doc = self.clone();
        let mut spacing = Spacing::NONE;
        for key in SpacingKey::ALL {
            let value = match *doc.spacing_slot(key) {
                Some(value) => i32::try_from(value).unwrap_or(0),
                None => key.fallback(self.outer()),
            };
            key.set(&mut spacing, value);
        }
        spacing
    }

    /// Whether the file sets this spacing key explicitly.
    pub(crate) fn has_spacing(&self, key: SpacingKey) -> bool {
        match key {
            SpacingKey::Inner => self.gaps.inner.is_some(),
            SpacingKey::Top => self.gaps.top.is_some(),
            SpacingKey::Bottom => self.gaps.bottom.is_some(),
            SpacingKey::Left => self.gaps.left.is_some(),
            SpacingKey::Right => self.gaps.right.is_some(),
            SpacingKey::SingleWindowMaxWidth => self.single_window_max_width.is_some(),
            SpacingKey::SingleWindowMaxHeight => self.single_window_max_height.is_some(),
        }
    }

    /// Removes one spacing key from the file so it falls back to its default (or `gaps.outer`).
    pub(crate) fn clear_spacing(&mut self, key: SpacingKey) {
        *self.spacing_slot(key) = None;
    }

    /// Sets one spacing key after checking its range, or removes it when its value needs no key.
    pub(crate) fn set_spacing(&mut self, key: SpacingKey, value: i64) -> Result<i32, String> {
        let checked = key.check(value)?;
        let written = key.is_written(checked, self.outer());
        *self.spacing_slot(key) = written.then_some(i64::from(checked));
        Ok(checked)
    }

    pub(crate) fn effective_hud(&self) -> bool {
        self.hud.unwrap_or(true)
    }

    pub(crate) fn effective_hud_style(&self) -> HudStyle {
        self.hud_style.unwrap_or_default()
    }

    pub(crate) fn effective_hud_position(&self) -> HudPosition {
        self.hud_position.unwrap_or_default()
    }

    pub(crate) fn effective_layout(&self) -> LayoutMode {
        self.layout.unwrap_or_default()
    }

    pub(crate) fn effective_update_channel(&self) -> UpdateChannel {
        self.update_channel.unwrap_or_default()
    }

    pub(crate) fn effective_mouse_follows_focus(&self) -> MouseFollowsFocus {
        self.mouse_follows_focus.unwrap_or_default()
    }

    /// The file's `trackpad_swipe`, `true` while unset. Swipes follow it only with
    /// `[labs] swipe_switch` on; `config get` and `config show` report the file's value either way.
    pub(crate) fn effective_trackpad_swipe(&self) -> bool {
        self.trackpad_swipe.unwrap_or(true)
    }

    /// The file's `menu_bar_icon`, `full` while unset. The menu bar follows it only with
    /// `[labs] menu_bar_icon_style` on; `config get` and `config show` report the file's value.
    pub(crate) fn effective_menu_bar_icon(&self) -> MenuBarIcon {
        self.menu_bar_icon.unwrap_or_default()
    }

    pub(crate) fn effective_pause_on_builtin_display(&self) -> bool {
        self.pause_on_builtin_display.unwrap_or(false)
    }
}

fn serialize_optional_hud_style<S: serde::Serializer>(
    style: &Option<HudStyle>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    match style {
        Some(style) => serializer.serialize_str(style.as_str()),
        None => serializer.serialize_none(),
    }
}

fn serialize_optional_hud_position<S: serde::Serializer>(
    position: &Option<HudPosition>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    match position {
        Some(position) => serializer.serialize_str(position.as_str()),
        None => serializer.serialize_none(),
    }
}

fn serialize_optional_layout<S: serde::Serializer>(
    layout: &Option<LayoutMode>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    match layout {
        Some(layout) => serializer.serialize_str(layout.as_str()),
        None => serializer.serialize_none(),
    }
}

fn serialize_optional_update_channel<S: serde::Serializer>(
    channel: &Option<UpdateChannel>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    match channel {
        Some(channel) => serializer.serialize_str(channel.as_str()),
        None => serializer.serialize_none(),
    }
}

fn serialize_optional_mouse_follows_focus<S: serde::Serializer>(
    mode: &Option<MouseFollowsFocus>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    match mode {
        Some(mode) => serializer.serialize_str(mode.as_str()),
        None => serializer.serialize_none(),
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RuleDoc {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bundle_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title_contains: Option<String>,
    pub action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<WorkspaceId>,
    /// Which of the app's new windows a route takes, as the file writes it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub windows: Option<RouteWindows>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WorkspaceDoc {
    pub workspace: WorkspaceId,
    pub monitor: String,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct EffectiveSettings {
    pub hud: bool,
    pub hud_style: &'static str,
    pub hud_position: &'static str,
    pub layout: &'static str,
    pub update_channel: &'static str,
    pub mouse_follows_focus: &'static str,
    pub trackpad_swipe: bool,
    pub menu_bar_icon: &'static str,
    pub pause_on_builtin_display: bool,
    pub single_window_max_width: i32,
    pub single_window_max_height: i32,
    pub workspace_order: Vec<WorkspaceId>,
    pub gaps: EffectiveGaps,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct EffectiveGaps {
    pub inner: i32,
    pub top: i32,
    pub bottom: i32,
    pub left: i32,
    pub right: i32,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct RuleView {
    pub index: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bundle_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title_contains: Option<String>,
    pub action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace: Option<WorkspaceId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub windows: Option<RouteWindows>,
}

impl RuleView {
    pub(crate) fn from_doc(index: usize, rule: &RuleDoc) -> Self {
        Self {
            index,
            bundle_id: rule.bundle_id.clone(),
            title: rule.title.clone(),
            title_contains: rule.title_contains.clone(),
            action: rule.action.clone(),
            workspace: rule.workspace,
            windows: rule.windows,
        }
    }
}

#[derive(Serialize)]
struct EffectiveConfigToml<'a> {
    version: u32,
    settings: EffectiveSettings,
    #[serde(skip_serializing_if = "Option::is_none")]
    labs: Option<LabsDoc>,
    #[serde(skip_serializing_if = "<[RuleDoc]>::is_empty")]
    rules: &'a [RuleDoc],
    #[serde(skip_serializing_if = "<[WorkspaceDoc]>::is_empty")]
    workspaces: &'a [WorkspaceDoc],
    #[serde(skip_serializing_if = "Option::is_none")]
    shortcuts: Option<&'a ShortcutsDoc>,
}

impl ConfigDoc {
    /// The `[labs]` table, created when a command first edits it.
    pub(crate) fn labs_mut(&mut self) -> &mut LabsDoc {
        self.labs.get_or_insert_with(LabsDoc::default)
    }

    /// Drops `[labs]` once it sets no switch, the way an emptied `[settings]` table goes.
    pub(crate) fn prune_labs(&mut self) {
        if self.labs.is_some_and(|labs| labs.is_empty()) {
            self.labs = None;
        }
    }

    pub(crate) fn load_or_default(path: &Path) -> Result<(Self, bool), String> {
        let source = match fs::read_to_string(path) {
            Ok(source) => source,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok((Self::default(), false));
            }
            Err(error) => {
                return Err(format!("failed to read {}: {error}", path.display()));
            }
        };
        Config::parse(&source).map_err(|error| format!("{}: {error}", path.display()))?;
        let doc: Self =
            toml::from_str(&source).map_err(|error| format!("{}: {error}", path.display()))?;
        Ok((doc, true))
    }

    pub(crate) fn effective_settings(&self) -> EffectiveSettings {
        let spacing = self.settings.effective_spacing();
        EffectiveSettings {
            hud: self.settings.effective_hud(),
            hud_style: self.settings.effective_hud_style().as_str(),
            hud_position: self.settings.effective_hud_position().as_str(),
            layout: self.settings.effective_layout().as_str(),
            update_channel: self.settings.effective_update_channel().as_str(),
            mouse_follows_focus: self.settings.effective_mouse_follows_focus().as_str(),
            trackpad_swipe: self.settings.effective_trackpad_swipe(),
            menu_bar_icon: self.settings.effective_menu_bar_icon().as_str(),
            pause_on_builtin_display: self.settings.effective_pause_on_builtin_display(),
            single_window_max_width: spacing.single_window_max_width,
            single_window_max_height: spacing.single_window_max_height,
            workspace_order: self.settings.workspace_order.clone().unwrap_or_default(),
            gaps: EffectiveGaps {
                inner: spacing.inner,
                top: spacing.top,
                bottom: spacing.bottom,
                left: spacing.left,
                right: spacing.right,
            },
        }
    }

    pub(crate) fn render_toml(&self) -> Result<String, String> {
        let rendered = toml::to_string_pretty(self)
            .map_err(|error| format!("failed to format config.toml: {error}"))?;
        Config::parse(&rendered)
            .map_err(|error| format!("refusing to write invalid config.toml: {error}"))?;
        Ok(rendered)
    }

    pub(crate) fn render_effective_toml(&self) -> Result<String, String> {
        let view = EffectiveConfigToml {
            version: self.version,
            settings: self.effective_settings(),
            labs: self.labs,
            rules: &self.rules,
            workspaces: &self.workspaces,
            shortcuts: self.shortcuts.as_ref(),
        };
        let rendered = toml::to_string_pretty(&view)
            .map_err(|error| format!("failed to format config.toml: {error}"))?;
        Config::parse(&rendered)
            .map_err(|error| format!("config.toml failed validation: {error}"))?;
        Ok(rendered)
    }

    pub(crate) fn save_atomic(&self, path: &Path) -> Result<(), String> {
        let rendered = self.render_toml()?;
        write_config_atomic(path, rendered.as_bytes())
            .map_err(|error| format!("failed to write {}: {error}", path.display()))
    }

    /// Acquires an exclusive config lock, reads the latest document from `path`,
    /// runs `edit`, and writes atomically when `edit` reports a change.
    pub(crate) fn edit_locked<T>(
        path: &Path,
        edit: impl FnOnce(&mut Self) -> Result<(T, bool), String>,
    ) -> Result<T, String> {
        let _lock = lock_config(path)
            .map_err(|error| format!("failed to lock {}: {error}", path.display()))?;
        let (mut doc, _) = Self::load_or_default(path)?;
        let (result, changed) = edit(&mut doc)?;
        if changed {
            doc.save_atomic(path)?;
        }
        Ok(result)
    }

    pub(crate) fn rule_views(&self) -> Vec<RuleView> {
        self.rules
            .iter()
            .enumerate()
            .map(|(index, rule)| RuleView::from_doc(index + 1, rule))
            .collect()
    }

    pub(crate) fn workspace_monitor(&self, workspace: WorkspaceId) -> Option<&str> {
        self.workspaces
            .iter()
            .find(|entry| entry.workspace == workspace)
            .map(|entry| entry.monitor.as_str())
    }

    pub(crate) fn active_workspaces(&self) -> Vec<WorkspaceId> {
        let mut out: Vec<WorkspaceId> = WorkspaceId::numbered().collect();
        let mut named = BTreeSet::new();
        for entry in &self.workspaces {
            if entry.workspace.letter().is_some() {
                named.insert(entry.workspace);
            }
        }
        for rule in &self.rules {
            if let Some(workspace) = rule.workspace
                && workspace.letter().is_some()
            {
                named.insert(workspace);
            }
        }
        out.extend(named);
        out
    }
}

pub(crate) fn resolve_config_path(override_path: Option<&Path>) -> Result<PathBuf, String> {
    if let Some(path) = override_path {
        return Ok(path.to_path_buf());
    }
    config_path().ok_or_else(|| "HOME is unavailable".into())
}

pub(crate) fn default_monitor_for(workspace: WorkspaceId) -> &'static str {
    match Workspaces::default().assignment(workspace) {
        DisplaySelector::Main => "main",
        DisplaySelector::Secondary => "secondary",
        DisplaySelector::Tertiary => "tertiary",
        DisplaySelector::Preferred(_) => "main",
    }
}

pub(crate) fn format_rule_summary(rule: &RuleDoc) -> String {
    let mut matchers = Vec::new();
    if let Some(bundle_id) = &rule.bundle_id {
        matchers.push(format!("bundle_id={bundle_id}"));
    }
    if let Some(title) = &rule.title {
        matchers.push(format!("title={title:?}"));
    }
    if let Some(part) = &rule.title_contains {
        matchers.push(format!("title_contains={part:?}"));
    }
    let target = match (rule.action.as_str(), rule.workspace) {
        ("route", Some(workspace)) => format!("workspace {workspace}"),
        (action, _) => action.to_owned(),
    };
    if let Some(windows) = rule.windows {
        return format!(
            "{} -> {target} [windows={}]",
            matchers.join(" "),
            windows.as_str()
        );
    }
    format!("{} -> {target}", matchers.join(" "))
}

pub(crate) fn to_json_pretty(value: &impl Serialize) -> Result<String, String> {
    serde_json::to_string_pretty(value)
        .map_err(|error| format!("failed to encode JSON output: {error}"))
}
