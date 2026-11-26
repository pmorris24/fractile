//! `config.toml` as the settings window edits it: rules and workspace displays, changed in
//! place so comments, order and anything else in the file survive. Every save is checked by
//! the agent's own parser first, and written atomically.

use std::path::PathBuf;

use fractile::{
    WorkspaceId,
    config::{LabFeature, LabsConfig},
};
use toml_edit::{
    Array, ArrayOfTables, DocumentMut, InlineTable, Item, Table, TableLike, Value, value,
};

const EMPTY: &str = "# Fractile configuration.\nversion = 1\n";

/// Where a rule sends an app's windows.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Place {
    Workspace(WorkspaceId),
    Float,
    /// Floating, and shown on every workspace.
    Pin,
    /// Tiled even when the window looks like a dialog.
    Tile,
    /// Never tiled, moved or parked.
    Ignore,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Rule {
    /// Its position among the file's `[[rules]]`.
    pub index: usize,
    pub bundle_id: Option<String>,
    pub title: Option<String>,
    /// A case-insensitive title substring the rule requires, as written in config.toml.
    pub title_contains: Option<String>,
    pub place: Place,
    /// `windows = "all"`: a route rule sends every new window of its app, not only the first.
    /// The engine reads it only with `[labs] route_all_windows` on.
    pub all_windows: bool,
}

impl Rule {
    /// Whether the rule covers every window of its app rather than only some titles.
    pub fn app_wide(&self) -> bool {
        self.title.is_none() && self.title_contains.is_none()
    }

    /// The title filter as written, exact or substring, for display.
    pub fn title_filter(&self) -> Option<&str> {
        self.title.as_deref().or(self.title_contains.as_deref())
    }
}

#[derive(Clone)]
pub struct Doc {
    path: Option<PathBuf>,
    doc: DocumentMut,
    /// Why the file couldn't be read, while it can't; saving is refused until it can.
    pub error: Option<String>,
    /// True when an external config change arrived while a drag deferred reloading.
    pending_reload: bool,
}

impl Doc {
    pub fn load() -> Doc {
        match fractile::config::config_path() {
            Some(path) => Doc::load_from(path),
            None => Doc::broken(None, "HOME is unavailable".into()),
        }
    }

    pub fn load_from(path: PathBuf) -> Doc {
        let source = match std::fs::read_to_string(&path) {
            Ok(source) => source,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => EMPTY.into(),
            Err(error) => return Doc::broken(Some(path), format!("Couldn't read config: {error}")),
        };
        let mut doc = Doc::parse(&source);
        doc.path = Some(path);
        doc
    }

    pub fn parse(source: &str) -> Doc {
        match source.parse::<DocumentMut>() {
            Ok(doc) => Doc {
                path: None,
                doc,
                error: fractile::config::Config::parse(source).err(),
                pending_reload: false,
            },
            Err(error) => Doc::broken(None, error.to_string()),
        }
    }

    fn broken(path: Option<PathBuf>, error: String) -> Doc {
        Doc {
            path,
            doc: DocumentMut::new(),
            error: Some(error),
            pending_reload: false,
        }
    }

    pub fn mark_reload_pending(&mut self) {
        self.pending_reload = true;
    }

    pub fn reload_pending(&self) -> bool {
        self.pending_reload
    }

    /// The `[labs]` switches as this document sets them.
    pub fn labs(&self) -> LabsConfig {
        LabsConfig {
            scratchpad: self.lab(LabFeature::Scratchpad),
            activity: self.lab(LabFeature::Activity),
            portrait_edge_rows: self.lab(LabFeature::PortraitEdgeRows),
            physical_displays: self.lab(LabFeature::PhysicalDisplays),
            recent_focus: self.lab(LabFeature::RecentFocus),
            swipe_switch: self.lab(LabFeature::SwipeSwitch),
            empty_workspace_focus: self.lab(LabFeature::EmptyWorkspaceFocus),
            float_follows_display: self.lab(LabFeature::FloatFollowsDisplay),
            workspace_steps: self.lab(LabFeature::WorkspaceSteps),
            four_finger_swipe: self.lab(LabFeature::FourFingerSwipe),
            single_window_height: self.lab(LabFeature::SingleWindowHeight),
            route_all_windows: self.lab(LabFeature::RouteAllWindows),
            menu_bar_icon_style: self.lab(LabFeature::MenuBarIconStyle),
            focus_follows_mouse: self.lab(LabFeature::FocusFollowsMouse),
        }
    }

    /// Whether `feature`'s switch is on, read straight from the edited file so a toggle shows
    /// in Settings without waiting for a reload.
    pub fn lab(&self, feature: LabFeature) -> bool {
        self.doc
            .get("labs")
            .and_then(Item::as_table_like)
            .and_then(|labs| labs.get(feature.key()))
            .and_then(Item::as_bool)
            .unwrap_or(false)
    }

    /// Reloads from this document's path (or the active config path if unset).
    pub fn reloaded(&self) -> Doc {
        match &self.path {
            Some(path) => Doc::load_from(path.clone()),
            None => Doc::load(),
        }
    }

    pub fn rules(&self) -> Vec<Rule> {
        match self.doc.get("rules") {
            Some(Item::ArrayOfTables(rules)) => rules
                .iter()
                .enumerate()
                .filter_map(|(index, table)| read_rule(index, table))
                .collect(),
            Some(Item::Value(Value::Array(rules))) => rules
                .iter()
                .enumerate()
                .filter_map(|(index, item)| read_rule(index, item.as_inline_table()?))
                .collect(),
            _ => Vec::new(),
        }
    }

    /// Sends rule `index`'s windows to `place`.
    pub fn place(&mut self, index: usize, place: Place) {
        match self.doc.get_mut("rules") {
            Some(Item::ArrayOfTables(rules)) => {
                if let Some(table) = rules.get_mut(index) {
                    set_place(table, place);
                }
            }
            Some(Item::Value(Value::Array(rules))) => {
                if let Some(table) = rules.get_mut(index).and_then(Value::as_inline_table_mut) {
                    set_place(table, place);
                }
            }
            _ => {}
        }
    }

    /// Moves the rule matching `expected` to `place`, or reports a conflict if an
    /// external edit modified, removed, or ambiguously shifted it.
    pub fn place_matching(&mut self, expected: &Rule, place: Place) -> Result<(), String> {
        let index = self.resolve_rule_index(expected).ok_or_else(|| {
            "config.toml changed externally while editing this rule; reloaded current settings"
                .to_owned()
        })?;
        self.place(index, place);
        Ok(())
    }

    /// Makes the route rule matching `expected` send every new window of its app (`all`), or
    /// only the first. Every window needs `[labs] route_all_windows`, so this turns it on.
    pub fn set_all_windows_matching(&mut self, expected: &Rule, all: bool) -> Result<(), String> {
        let index = self.resolve_rule_index(expected).ok_or_else(|| {
            "config.toml changed externally while editing this rule; reloaded current settings"
                .to_owned()
        })?;
        let set = |table: &mut dyn TableLike| {
            if all {
                set_table_item(table, "windows", value("all"));
            } else {
                table.remove("windows");
            }
        };
        match self.doc.get_mut("rules") {
            Some(Item::ArrayOfTables(rules)) => {
                if let Some(table) = rules.get_mut(index) {
                    set(table);
                }
            }
            Some(Item::Value(Value::Array(rules))) => {
                if let Some(table) = rules.get_mut(index).and_then(Value::as_inline_table_mut) {
                    set(table);
                }
            }
            _ => {}
        }
        if all {
            self.set_lab(LabFeature::RouteAllWindows, true);
        }
        Ok(())
    }

    /// Removes the rule matching `expected`, or reports a conflict if an external
    /// edit modified, removed, or ambiguously shifted it.
    pub fn remove_matching(&mut self, expected: &Rule) -> Result<(), String> {
        let index = self.resolve_rule_index(expected).ok_or_else(|| {
            "config.toml changed externally while editing this rule; reloaded current settings"
                .to_owned()
        })?;
        self.remove(index);
        Ok(())
    }

    fn resolve_rule_index(&self, expected: &Rule) -> Option<usize> {
        let rules = self.rules();
        let same_rule = |rule: &Rule| {
            rule.bundle_id == expected.bundle_id
                && rule.title == expected.title
                && rule.title_contains == expected.title_contains
                && rule.place == expected.place
        };
        if rules.get(expected.index).is_some_and(same_rule) {
            return Some(expected.index);
        }
        let mut matches = rules.iter().filter(|rule| same_rule(rule));
        let matched = matches.next()?;
        matches.next().is_none().then_some(matched.index)
    }

    pub fn add(&mut self, bundle_id: &str, place: Place) {
        if let Some(Item::Value(Value::Array(rules))) = self.doc.get_mut("rules") {
            let mut table = InlineTable::new();
            table.insert("bundle_id", Value::from(bundle_id));
            set_place(&mut table, place);
            push_inline_table(rules, table);
            return;
        }
        let mut table = Table::new();
        table.insert("bundle_id", value(bundle_id));
        set_place(&mut table, place);
        self.rule_tables().push(table);
    }

    /// Where `bundle_id`'s app-wide rule sends windows, if one is set.
    pub fn app_place(&self, bundle_id: &str) -> Option<Place> {
        self.rules()
            .into_iter()
            .find(|rule| rule.app_wide() && rule.bundle_id.as_deref() == Some(bundle_id))
            .map(|rule| rule.place)
    }

    /// Sets or removes `bundle_id`'s app-wide rule, leaving title-specific rules untouched.
    pub fn set_app_place(&mut self, bundle_id: &str, place: Option<Place>) {
        let existing = self
            .rules()
            .into_iter()
            .find(|rule| rule.app_wide() && rule.bundle_id.as_deref() == Some(bundle_id))
            .map(|rule| rule.index);
        match (existing, place) {
            (Some(index), Some(place)) => self.place(index, place),
            (Some(index), None) => self.remove(index),
            (None, Some(place)) => self.add(bundle_id, place),
            (None, None) => {}
        }
    }

    /// The display a workspace asks for in `[[workspaces]]` or `workspaces = [...]`, if it names one.
    pub fn monitor(&self, workspace: WorkspaceId) -> Option<String> {
        match self.doc.get("workspaces")? {
            Item::ArrayOfTables(tables) => tables
                .iter()
                .find(|table| table_workspace(*table) == Some(workspace))
                .and_then(|table| string(table, "monitor")),
            Item::Value(Value::Array(entries)) => entries
                .iter()
                .filter_map(Value::as_inline_table)
                .find(|table| table_workspace(*table) == Some(workspace))
                .and_then(|table| string(table, "monitor")),
            _ => None,
        }
    }

    /// Named workspaces (`A`–`Z`) enabled by `[[rules]]` or `[[workspaces]]` in this file.
    pub fn named_workspaces(&self) -> Vec<WorkspaceId> {
        self.workspaces_with_referenced(std::iter::empty())
            .into_iter()
            .filter(|workspace| workspace.letter().is_some())
            .collect()
    }

    /// `defaults`, followed by any other workspaces referenced in `[[rules]]` or `[[workspaces]]`.
    pub fn workspaces_with_referenced(
        &self,
        defaults: impl IntoIterator<Item = WorkspaceId>,
    ) -> Vec<WorkspaceId> {
        let mut out: Vec<WorkspaceId> = defaults.into_iter().collect();
        let mut extra: Vec<WorkspaceId> = self
            .rules()
            .into_iter()
            .filter_map(|rule| match rule.place {
                Place::Workspace(workspace) => Some(workspace),
                Place::Float | Place::Pin | Place::Tile | Place::Ignore => None,
            })
            .chain(WorkspaceId::all().filter(|workspace| self.monitor(*workspace).is_some()))
            .collect();
        extra.sort();
        for workspace in extra {
            if !out.contains(&workspace) {
                out.push(workspace);
            }
        }
        out
    }

    fn has_workspace_route(&self, workspace: WorkspaceId) -> bool {
        self.rules()
            .into_iter()
            .any(|rule| rule.place == Place::Workspace(workspace))
    }

    /// Sets the display a workspace asks for; `None` clears the override back to the default,
    /// retaining a solely declared named workspace on `"main"` so it stays enabled.
    pub fn set_monitor(&mut self, workspace: WorkspaceId, monitor: Option<&str>) {
        let keep_named = workspace.letter().is_some() && !self.has_workspace_route(workspace);
        if let Some(Item::Value(Value::Array(entries))) = self.doc.get_mut("workspaces") {
            let found = entries.iter().position(|item| {
                item.as_inline_table()
                    .and_then(|table| table_workspace(table))
                    == Some(workspace)
            });
            let remove_index = match (found, monitor) {
                (Some(index), Some(monitor)) => {
                    if let Some(table) = entries.get_mut(index).and_then(Value::as_inline_table_mut)
                    {
                        table.insert("monitor", Value::from(monitor));
                    }
                    None
                }
                (Some(index), None) if keep_named => {
                    if let Some(table) = entries.get_mut(index).and_then(Value::as_inline_table_mut)
                    {
                        table.insert("monitor", Value::from("main"));
                    }
                    None
                }
                (Some(index), None) => Some(index),
                (None, Some(monitor)) => {
                    let mut table = InlineTable::new();
                    TableLike::insert(&mut table, "workspace", workspace_value(workspace));
                    table.insert("monitor", Value::from(monitor));
                    push_inline_table(entries, table);
                    None
                }
                (None, None) => None,
            };
            if let Some(index) = remove_index {
                self.remove_entry("workspaces", index);
            } else if let Some(Item::Value(Value::Array(entries))) = self.doc.get("workspaces")
                && entries.is_empty()
            {
                self.doc.remove("workspaces");
            }
            return;
        }
        if monitor.is_none()
            && !self
                .doc
                .get("workspaces")
                .is_some_and(Item::is_array_of_tables)
        {
            return;
        }
        if !self
            .doc
            .get("workspaces")
            .is_some_and(Item::is_array_of_tables)
        {
            self.doc.remove("workspaces");
            self.doc
                .insert("workspaces", Item::ArrayOfTables(ArrayOfTables::new()));
        }
        let Some(Item::ArrayOfTables(tables)) = self.doc.get_mut("workspaces") else {
            return;
        };
        let found = tables
            .iter()
            .position(|table| table_workspace(table) == Some(workspace));
        let remove_index = match (found, monitor) {
            (Some(index), Some(monitor)) => {
                if let Some(table) = tables.get_mut(index) {
                    set_table_item(table, "monitor", value(monitor));
                }
                None
            }
            (Some(index), None) if keep_named => {
                if let Some(table) = tables.get_mut(index) {
                    set_table_item(table, "monitor", value("main"));
                }
                None
            }
            (Some(index), None) => Some(index),
            (None, Some(monitor)) => {
                let mut table = Table::new();
                table.insert("workspace", workspace_value(workspace));
                table.insert("monitor", value(monitor));
                tables.push(table);
                None
            }
            (None, None) => None,
        };
        if let Some(index) = remove_index {
            self.remove_entry("workspaces", index);
        } else if let Some(Item::ArrayOfTables(tables)) = self.doc.get("workspaces")
            && tables.is_empty()
        {
            self.doc.remove("workspaces");
        }
    }

    fn rule_tables(&mut self) -> &mut ArrayOfTables {
        if !self.doc.get("rules").is_some_and(Item::is_array_of_tables) {
            self.doc.remove("rules");
            self.doc
                .insert("rules", Item::ArrayOfTables(ArrayOfTables::new()));
        }
        let Some(Item::ArrayOfTables(tables)) = self.doc.get_mut("rules") else {
            unreachable!()
        };
        tables
    }

    /// `config.toml`'s path shortened with `~`, falling back to the default location in fixtures.
    pub fn display_path(&self) -> String {
        self.path
            .clone()
            .or_else(fractile::config::config_path)
            .map_or_else(
                || "~/Library/Application Support/Fractile/config.toml".into(),
                |path| crate::controls::home_relative(&path),
            )
    }

    pub fn source(&self) -> String {
        self.doc.to_string()
    }

    /// Writes the file once Fractile's own parser accepts it; the agent picks it up from there.
    pub fn save(&mut self) -> Result<(), String> {
        if let Some(error) = &self.error {
            return Err(format!("config.toml has an error to fix first: {error}"));
        }
        let source = self.source();
        fractile::config::Config::parse(&source)?;
        let Some(path) = &self.path else {
            return Ok(());
        };
        let _lock = fractile::config::lock_config(path)
            .map_err(|error| format!("failed to lock {}: {error}", path.display()))?;
        fractile::config::write_config_atomic(path, source.as_bytes())
            .map_err(|error| error.to_string())
    }

    /// Reloads the current file under the config lock, applies `edit`, validates, and writes
    /// atomically so external edits survive. On a conflict or error the document reflects the
    /// current file on disk so the user can retry without losing external changes.
    pub fn edit_and_save(
        &mut self,
        edit: impl FnOnce(&mut Doc) -> Result<(), String>,
    ) -> Result<(), String> {
        let Some(path) = self.path.clone() else {
            let before = self.clone();
            if let Err(error) = edit(self) {
                *self = before;
                return Err(error);
            }
            if let Err(error) = self.save() {
                *self = before;
                return Err(error);
            }
            return Ok(());
        };
        let _lock = fractile::config::lock_config(&path)
            .map_err(|error| format!("failed to lock {}: {error}", path.display()))?;
        let mut fresh = Doc::load_from(path.clone());
        if let Some(error) = &fresh.error {
            let message = format!("config.toml has an error to fix first: {error}");
            *self = fresh;
            return Err(message);
        }
        let loaded = fresh.clone();
        if let Err(error) = edit(&mut fresh) {
            *self = loaded;
            return Err(error);
        }
        let source = fresh.source();
        if let Err(error) = fractile::config::Config::parse(&source) {
            *self = loaded;
            return Err(error);
        }
        if let Err(error) = fractile::config::write_config_atomic(&path, source.as_bytes()) {
            *self = loaded;
            return Err(error.to_string());
        }
        *self = fresh;
        Ok(())
    }
}

fn read_rule(index: usize, table: &dyn TableLike) -> Option<Rule> {
    let place = match table.get("action").and_then(Item::as_str)? {
        "float" => Place::Float,
        "pin" => Place::Pin,
        "tile" => Place::Tile,
        "ignore" => Place::Ignore,
        "route" => Place::Workspace(workspace(table.get("workspace")?)?),
        _ => return None,
    };
    Some(Rule {
        index,
        bundle_id: string(table, "bundle_id"),
        title: string(table, "title"),
        title_contains: string(table, "title_contains"),
        all_windows: matches!(place, Place::Workspace(_))
            && table.get("windows").and_then(Item::as_str) == Some("all"),
        place,
    })
}

fn table_workspace(table: &dyn TableLike) -> Option<WorkspaceId> {
    table.get("workspace").and_then(workspace)
}

fn push_inline_table(array: &mut Array, mut table: InlineTable) {
    table.fmt();
    let mut item = Value::InlineTable(table);
    if let Some(prefix) = array
        .iter()
        .last()
        .and_then(|last| last.decor().prefix())
        .and_then(|raw| raw.as_str())
        .and_then(multiline_indent_prefix)
    {
        item.decor_mut().set_prefix(prefix);
        array.push_formatted(item);
    } else {
        array.push(item);
    }
}

fn multiline_indent_prefix(prefix: &str) -> Option<String> {
    let newline = prefix.rfind('\n')?;
    let start = if newline > 0 && prefix.as_bytes()[newline - 1] == b'\r' {
        newline - 1
    } else {
        newline
    };
    let indent = &prefix[newline + 1..];
    indent
        .chars()
        .all(|ch| ch == ' ' || ch == '\t')
        .then(|| prefix[start..].to_owned())
}

fn string(table: &dyn TableLike, key: &str) -> Option<String> {
    table.get(key).and_then(Item::as_str).map(str::to_owned)
}

fn workspace(item: &Item) -> Option<WorkspaceId> {
    match item.as_value()? {
        Value::Integer(number) => WorkspaceId::new(u8::try_from(*number.value()).ok()?).ok(),
        Value::String(name) => WorkspaceId::parse(name.value()).ok(),
        _ => None,
    }
}

fn workspace_value(workspace: WorkspaceId) -> Item {
    match workspace.number() {
        Some(number) => value(i64::from(number)),
        None => value(workspace.to_string()),
    }
}

fn set_place(table: &mut dyn TableLike, place: Place) {
    match place {
        Place::Workspace(workspace) => {
            set_table_item(table, "action", value("route"));
            set_table_item(table, "workspace", workspace_value(workspace));
        }
        Place::Float | Place::Pin | Place::Tile | Place::Ignore => {
            let action = match place {
                Place::Float => "float",
                Place::Pin => "pin",
                Place::Tile => "tile",
                _ => "ignore",
            };
            set_table_item(table, "action", value(action));
            table.remove("workspace");
            // `windows` belongs to route rules only; the parser rejects it anywhere else.
            table.remove("windows");
        }
    }
}

mod comments;
mod filters;
mod settings;
mod shortcuts;
mod spacing;
#[cfg(test)]
mod tests;

/// Updates an existing key in place so its key comment decor and value
/// spacing/trailing comments are preserved across Settings edits.
pub(super) fn set_table_item(table: &mut dyn TableLike, key: &str, mut item: Item) {
    if let Some(existing) = table.get_mut(key) {
        if let (Some(old), Some(new)) = (existing.as_value(), item.as_value_mut()) {
            *new.decor_mut() = old.decor().clone();
        }
        *existing = item;
    } else {
        table.insert(key, item);
    }
}
