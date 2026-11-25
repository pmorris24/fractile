use std::path::Path;

use serde::Serialize;
use spaceman::WorkspaceId;

use super::{
    Outcome,
    document::{ConfigDoc, WorkspaceDoc, default_monitor_for, resolve_config_path, to_json_pretty},
    help,
};

#[derive(Serialize)]
struct WorkspaceEntry {
    workspace: WorkspaceId,
    monitor: String,
    default_monitor: &'static str,
    configured: bool,
}

#[derive(Serialize)]
struct WorkspacesListOutput {
    path: String,
    workspaces: Vec<WorkspaceEntry>,
}

#[derive(Serialize)]
struct WorkspaceAssignOutput {
    path: String,
    workspace: WorkspaceId,
    monitor: String,
}

#[derive(Serialize)]
struct WorkspaceClearOutput {
    path: String,
    workspace: WorkspaceId,
    cleared: bool,
    monitor: &'static str,
}

pub(crate) fn run(executable: &str, args: &[&str], path_override: Option<&Path>) -> Outcome {
    let Some((&sub, rest)) = args.split_first() else {
        return Outcome::usage(help::workspaces_help(executable));
    };
    if help::is_help_flag(sub) || sub == "help" || rest.iter().any(|arg| help::is_help_flag(arg)) {
        return Outcome::ok(help::workspaces_help(executable));
    }

    let result = match sub {
        "list" => run_list(rest, path_override),
        "assign" => run_assign(rest, path_override),
        "clear" => run_clear(rest, path_override),
        _ => {
            return Outcome::usage(format!(
                "unknown workspaces subcommand: {sub}\n\n{}",
                help::workspaces_help(executable)
            ));
        }
    };

    match result {
        Ok(output) => Outcome::ok(output),
        Err(error) => Outcome::err(error),
    }
}

fn split_json_flag<'a>(args: &[&'a str]) -> Result<(bool, Vec<&'a str>), String> {
    let mut json = false;
    let mut positional = Vec::new();
    let mut after_double_dash = false;
    for &arg in args {
        if !after_double_dash && arg == "--" {
            after_double_dash = true;
            continue;
        }
        if !after_double_dash && arg == "--json" {
            json = true;
            continue;
        }
        if !after_double_dash && arg.starts_with('-') {
            return Err(format!("unknown option: {arg}"));
        }
        positional.push(arg);
    }
    Ok((json, positional))
}

fn run_list(args: &[&str], path_override: Option<&Path>) -> Result<String, String> {
    let (json, positional) = split_json_flag(args)?;
    if !positional.is_empty() {
        return Err(format!("unexpected argument: {}", positional[0]));
    }
    let path = resolve_config_path(path_override)?;
    let (doc, _) = ConfigDoc::load_or_default(&path)?;

    let entries = doc
        .active_workspaces()
        .into_iter()
        .map(|workspace| {
            let default_monitor = default_monitor_for(workspace);
            let configured = doc.workspace_monitor(workspace);
            WorkspaceEntry {
                workspace,
                monitor: configured.unwrap_or(default_monitor).to_owned(),
                default_monitor,
                configured: configured.is_some(),
            }
        })
        .collect::<Vec<_>>();

    if json {
        return to_json_pretty(&WorkspacesListOutput {
            path: path.display().to_string(),
            workspaces: entries,
        });
    }

    let lines = entries
        .iter()
        .map(|entry| {
            let workspace = entry.workspace.to_string();
            let note = if entry.configured {
                format!("(configured; default: {})", entry.default_monitor)
            } else {
                "(default)".into()
            };
            format!("{workspace:<3} {:<12} {note}", entry.monitor)
        })
        .collect::<Vec<_>>();
    Ok(lines.join("\n"))
}

fn run_assign(args: &[&str], path_override: Option<&Path>) -> Result<String, String> {
    let (json, positional) = split_json_flag(args)?;
    let [raw_workspace, raw_monitor] = positional.as_slice() else {
        return Err(
            "usage: spaceman workspaces assign <ws> <main|secondary|tertiary|display-name-or-uuid> [--json]"
                .into(),
        );
    };
    let workspace = WorkspaceId::parse(raw_workspace)?;
    if raw_monitor.trim().is_empty() {
        return Err(format!("workspace {workspace} monitor cannot be empty"));
    }
    let monitor = (*raw_monitor).to_owned();

    let path = resolve_config_path(path_override)?;
    ConfigDoc::edit_locked(&path, |doc| {
        if let Some(existing) = doc
            .workspaces
            .iter_mut()
            .find(|entry| entry.workspace == workspace)
        {
            existing.monitor = monitor.clone();
        } else {
            doc.workspaces.push(WorkspaceDoc {
                workspace,
                monitor: monitor.clone(),
            });
        }
        Ok(((), true))
    })?;

    if json {
        to_json_pretty(&WorkspaceAssignOutput {
            path: path.display().to_string(),
            workspace,
            monitor,
        })
    } else {
        Ok(format!("assigned workspace {workspace} -> {monitor}"))
    }
}

fn run_clear(args: &[&str], path_override: Option<&Path>) -> Result<String, String> {
    let (json, positional) = split_json_flag(args)?;
    let [raw_workspace] = positional.as_slice() else {
        return Err("usage: spaceman workspaces clear <ws> [--json]".into());
    };
    let workspace = WorkspaceId::parse(raw_workspace)?;
    let default_monitor = default_monitor_for(workspace);

    let path = resolve_config_path(path_override)?;
    let cleared = ConfigDoc::edit_locked(&path, |doc| {
        let referenced_by_rule = doc
            .rules
            .iter()
            .any(|rule| rule.workspace == Some(workspace));
        let cleared = if workspace.number().is_some() || referenced_by_rule {
            let before = doc.workspaces.len();
            doc.workspaces.retain(|entry| entry.workspace != workspace);
            doc.workspaces.len() != before
        } else if let Some(existing) = doc
            .workspaces
            .iter_mut()
            .find(|entry| entry.workspace == workspace)
        {
            if existing.monitor == default_monitor {
                false
            } else {
                existing.monitor = default_monitor.to_owned();
                true
            }
        } else {
            false
        };
        Ok((cleared, cleared))
    })?;

    if json {
        to_json_pretty(&WorkspaceClearOutput {
            path: path.display().to_string(),
            workspace,
            cleared,
            monitor: default_monitor,
        })
    } else if cleared {
        Ok(format!(
            "cleared workspace {workspace} (default: {default_monitor})"
        ))
    } else {
        Ok(format!(
            "workspace {workspace} already uses default ({default_monitor})"
        ))
    }
}
