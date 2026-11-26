//! Read-modify-write edits to `config.toml`: locking, symlink-following
//! atomic saves, and the workspace display assignment editor.
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{
    Config, HudPosition, HudStyle, LayoutMode, MenuBarIcon, MouseFollowsFocus, RouteWindows,
    UpdateChannel, parse_path,
};
use crate::{DisplaySelector, WorkspaceId, Workspaces};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct EditableConfig {
    version: u32,
    #[serde(default, skip_serializing_if = "EditableSettings::is_empty")]
    settings: EditableSettings,
    /// Carried through untouched; editing a workspace never changes a Lab.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    labs: Option<toml::Table>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    rules: Vec<EditableRule>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    workspaces: Vec<EditableWorkspace>,
    /// Carried through untouched; this editor never changes shortcuts (Settings › Shortcuts
    /// edits them in place with `toml_edit`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    shortcuts: Option<toml::Table>,
}

impl Default for EditableConfig {
    fn default() -> Self {
        Self {
            version: 1,
            settings: EditableSettings::default(),
            labs: None,
            rules: Vec::new(),
            workspaces: Vec::new(),
            shortcuts: None,
        }
    }
}

#[derive(Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct EditableSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    hud: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    hud_style: Option<HudStyle>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    hud_position: Option<HudPosition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    hud_stay: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    layout: Option<LayoutMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    update_channel: Option<UpdateChannel>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    mouse_follows_focus: Option<MouseFollowsFocus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    trackpad_swipe: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    menu_bar_icon: Option<MenuBarIcon>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pause_on_builtin_display: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    single_window_max_width: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    single_window_max_height: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    workspace_order: Option<Vec<WorkspaceId>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    gaps: Option<toml::Table>,
}

impl EditableSettings {
    const fn is_empty(&self) -> bool {
        self.hud.is_none()
            && self.hud_style.is_none()
            && self.hud_position.is_none()
            && self.hud_stay.is_none()
            && self.layout.is_none()
            && self.update_channel.is_none()
            && self.mouse_follows_focus.is_none()
            && self.trackpad_swipe.is_none()
            && self.menu_bar_icon.is_none()
            && self.pause_on_builtin_display.is_none()
            && self.single_window_max_width.is_none()
            && self.single_window_max_height.is_none()
            && self.workspace_order.is_none()
            && self.gaps.is_none()
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct EditableRule {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    bundle_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    workspace: Option<WorkspaceId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    windows: Option<RouteWindows>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct EditableWorkspace {
    workspace: WorkspaceId,
    monitor: String,
}

/// Exclusive advisory lock guarding read-modify-write updates to a config file.
///
/// Locks the resolved target's parent directory via `flock(2)` so atomic renames
/// of the config file never swap the locked inode and no sidecar lock file is
/// left behind (for example inside a dotfiles repository).
pub struct ConfigLock {
    _dir: std::fs::File,
}

/// Resolves symbolic links at `path` so config locking and atomic saves operate
/// on the target file instead of replacing the symbolic link itself.
pub fn resolve_write_target(path: &Path) -> std::io::Result<PathBuf> {
    const MAX_SYMLINK_DEPTH: usize = 40;
    match std::fs::symlink_metadata(path) {
        Ok(meta) if !meta.file_type().is_symlink() => return Ok(path.to_path_buf()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(path.to_path_buf());
        }
        Err(error) => return Err(error),
        Ok(_) => {}
    }
    match std::fs::canonicalize(path) {
        Ok(resolved) => return Ok(resolved),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let mut current = path.to_path_buf();
    for _ in 0..MAX_SYMLINK_DEPTH {
        match std::fs::symlink_metadata(&current) {
            Ok(meta) if meta.file_type().is_symlink() => {
                let target = std::fs::read_link(&current)?;
                current = if target.is_absolute() {
                    target
                } else {
                    let parent = current
                        .parent()
                        .filter(|parent| !parent.as_os_str().is_empty())
                        .unwrap_or_else(|| Path::new("."));
                    let base =
                        std::fs::canonicalize(parent).unwrap_or_else(|_| parent.to_path_buf());
                    base.join(target)
                };
            }
            Ok(_) => return Ok(current),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(current),
            Err(error) => return Err(error),
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        format!(
            "too many levels of symbolic links while resolving {}",
            path.display()
        ),
    ))
}

/// Acquires an exclusive advisory lock for editing the config file at `path`.
pub fn lock_config(path: &Path) -> std::io::Result<ConfigLock> {
    let target = resolve_write_target(path)?;
    let parent = target
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;
    let dir = std::fs::File::open(parent)?;
    dir.lock()?;
    Ok(ConfigLock { _dir: dir })
}

/// Atomically replaces the config file at `path`, following symbolic links so a
/// symlinked `config.toml` stays linked to its updated target.
pub fn write_config_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let target = resolve_write_target(path)?;
    crate::state_file::write_atomic(&target, bytes)
}

/// Atomically updates `workspace`'s display assignment in `path`, clearing the
/// `[[workspaces]]` override when it returns to the built-in default (while
/// keeping named workspaces that have no route rule declared).
pub fn save_workspace_monitor_at(
    path: &Path,
    workspace: WorkspaceId,
    selector: &DisplaySelector,
) -> Result<Config, String> {
    let _lock =
        lock_config(path).map_err(|error| format!("failed to lock {}: {error}", path.display()))?;
    let mut doc = match std::fs::read_to_string(path) {
        Ok(source) => {
            parse_path(path, &source)?;
            toml::from_str::<EditableConfig>(&source)
                .map_err(|error| format!("{}: {error}", path.display()))?
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => EditableConfig::default(),
        Err(error) => return Err(format!("failed to read {}: {error}", path.display())),
    };
    let referenced_by_rule = doc
        .rules
        .iter()
        .any(|rule| rule.workspace == Some(workspace));
    let clear = *selector == Workspaces::default_assignment(workspace)
        && (workspace.number().is_some() || referenced_by_rule);
    if clear {
        doc.workspaces.retain(|entry| entry.workspace != workspace);
    } else {
        let monitor = selector.as_str().to_owned();
        if let Some(existing) = doc
            .workspaces
            .iter_mut()
            .find(|entry| entry.workspace == workspace)
        {
            existing.monitor = monitor;
        } else {
            doc.workspaces
                .push(EditableWorkspace { workspace, monitor });
        }
    }
    let rendered = toml::to_string_pretty(&doc)
        .map_err(|error| format!("failed to format config.toml: {error}"))?;
    let parsed = Config::parse(&rendered)
        .map_err(|error| format!("refusing to write invalid config.toml: {error}"))?;
    write_config_atomic(path, rendered.as_bytes())
        .map_err(|error| format!("failed to write {}: {error}", path.display()))?;
    Ok(parsed)
}
