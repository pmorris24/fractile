use std::path::Path;

use fractile::{WorkspaceId, config::RouteWindows};
use serde::Serialize;

use super::{
    Outcome,
    document::{
        ConfigDoc, RuleDoc, RuleView, format_rule_summary, resolve_config_path, to_json_pretty,
    },
    help,
};

#[derive(Serialize)]
struct RulesListOutput {
    path: String,
    rules: Vec<RuleView>,
}

#[derive(Serialize)]
struct RuleAddedOutput {
    path: String,
    rule: RuleView,
}

#[derive(Serialize)]
struct RuleRemovedOutput {
    path: String,
    removed: Vec<RuleView>,
    remaining: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ActionArg {
    Workspace(WorkspaceId),
    Float,
    Pin,
    Tile,
    Ignore,
}

const ONE_ACTION: &str = "specify only one of --workspace, --float, --pin, --tile, or --ignore";

struct AddOptions {
    app: Option<String>,
    title: Option<String>,
    title_contains: Option<String>,
    action: ActionArg,
    /// `--windows first|all`, written as the route rule's `windows` key.
    windows: Option<RouteWindows>,
    position: Option<usize>,
    json: bool,
}

enum RemoveTarget {
    Index(usize),
    App(String),
}

pub(crate) fn run(executable: &str, args: &[&str], path_override: Option<&Path>) -> Outcome {
    let Some((&sub, rest)) = args.split_first() else {
        return Outcome::usage(help::rules_help(executable));
    };
    if help::is_help_flag(sub)
        || sub == "help"
        || match sub {
            "add" => add_requests_help(rest),
            _ => rest.iter().any(|arg| help::is_help_flag(arg)),
        }
    {
        return Outcome::ok(help::rules_help(executable));
    }

    let result = match sub {
        "list" => run_list(rest, path_override),
        "add" => run_add(rest, path_override),
        "remove" => run_remove(rest, path_override),
        _ => {
            return Outcome::usage(format!(
                "unknown rules subcommand: {sub}\n\n{}",
                help::rules_help(executable)
            ));
        }
    };

    match result {
        Ok(output) => Outcome::ok(output),
        Err(error) => Outcome::err(error),
    }
}

fn run_list(args: &[&str], path_override: Option<&Path>) -> Result<String, String> {
    let mut json = false;
    for &arg in args {
        if arg == "--json" {
            json = true;
        } else {
            return Err(format!("unexpected argument: {arg}"));
        }
    }
    let path = resolve_config_path(path_override)?;
    let (doc, _) = ConfigDoc::load_or_default(&path)?;
    if json {
        return to_json_pretty(&RulesListOutput {
            path: path.display().to_string(),
            rules: doc.rule_views(),
        });
    }
    if doc.rules.is_empty() {
        return Ok("No rules configured.".into());
    }
    let lines = doc
        .rules
        .iter()
        .enumerate()
        .map(|(index, rule)| format!("{}. {}", index + 1, format_rule_summary(rule)))
        .collect::<Vec<_>>();
    Ok(lines.join("\n"))
}

fn add_requests_help(args: &[&str]) -> bool {
    let mut index = 0;
    while index < args.len() {
        let arg = args[index];
        if help::is_help_flag(arg) {
            return true;
        }
        if matches!(
            arg,
            "--app"
                | "--bundle-id"
                | "--title"
                | "--title-contains"
                | "--workspace"
                | "--windows"
                | "--position"
        ) {
            index += 2;
        } else {
            index += 1;
        }
    }
    false
}

fn parse_add_options(args: &[&str]) -> Result<AddOptions, String> {
    let mut app: Option<String> = None;
    let mut title: Option<String> = None;
    let mut title_contains: Option<String> = None;
    let mut action: Option<ActionArg> = None;
    let mut position: Option<usize> = None;
    let mut windows: Option<RouteWindows> = None;
    let mut json = false;

    let mut index = 0;
    while index < args.len() {
        let arg = args[index];
        let (flag, inline_value) = match arg.split_once('=') {
            Some((flag, value)) if flag.starts_with("--") => (flag, Some(value)),
            _ => (arg, None),
        };
        let next_value =
            |flag: &str, inline: Option<&str>, index: &mut usize| -> Result<String, String> {
                if let Some(value) = inline {
                    return Ok(value.to_owned());
                }
                *index += 1;
                args.get(*index)
                    .map(|value| (*value).to_owned())
                    .ok_or_else(|| format!("missing value for {flag}"))
            };

        match flag {
            "--app" | "--bundle-id" => {
                if app.is_some() {
                    return Err(format!("{flag} specified more than once"));
                }
                let value = next_value(flag, inline_value, &mut index)?;
                if value.trim().is_empty() {
                    return Err(format!("{flag} cannot be empty"));
                }
                app = Some(value);
            }
            "--title" => {
                if title.is_some() {
                    return Err("--title specified more than once".into());
                }
                let value = next_value(flag, inline_value, &mut index)?;
                if value.is_empty() {
                    return Err("--title cannot be empty".into());
                }
                title = Some(value);
            }
            "--title-contains" => {
                if title_contains.is_some() {
                    return Err("--title-contains specified more than once".into());
                }
                let value = next_value(flag, inline_value, &mut index)?;
                if value.is_empty() {
                    return Err("--title-contains cannot be empty".into());
                }
                title_contains = Some(value);
            }
            "--workspace" => {
                if action.is_some() {
                    return Err(ONE_ACTION.into());
                }
                let value = next_value(flag, inline_value, &mut index)?;
                let workspace = WorkspaceId::parse(&value)?;
                action = Some(ActionArg::Workspace(workspace));
            }
            "--float" => {
                if inline_value.is_some() {
                    return Err("--float does not take a value".into());
                }
                if action.is_some() {
                    return Err(ONE_ACTION.into());
                }
                action = Some(ActionArg::Float);
            }
            "--pin" => {
                if inline_value.is_some() {
                    return Err("--pin does not take a value".into());
                }
                if action.is_some() {
                    return Err(ONE_ACTION.into());
                }
                action = Some(ActionArg::Pin);
            }
            "--tile" => {
                if inline_value.is_some() {
                    return Err("--tile does not take a value".into());
                }
                if action.is_some() {
                    return Err(ONE_ACTION.into());
                }
                action = Some(ActionArg::Tile);
            }
            "--ignore" => {
                if inline_value.is_some() {
                    return Err("--ignore does not take a value".into());
                }
                if action.is_some() {
                    return Err(ONE_ACTION.into());
                }
                action = Some(ActionArg::Ignore);
            }
            "--windows" => {
                if windows.is_some() {
                    return Err("--windows specified more than once".into());
                }
                let value = next_value(flag, inline_value, &mut index)?;
                windows = Some(RouteWindows::parse(&value).ok_or_else(|| {
                    format!("--windows takes \"first\" or \"all\", got {value:?}")
                })?);
            }
            "--position" => {
                if position.is_some() {
                    return Err("--position specified more than once".into());
                }
                let value = next_value(flag, inline_value, &mut index)?;
                let parsed = value
                    .parse::<usize>()
                    .ok()
                    .filter(|value| *value > 0)
                    .ok_or_else(|| {
                        format!("--position must be a positive 1-based integer, got {value:?}")
                    })?;
                position = Some(parsed);
            }
            "--json" => {
                if inline_value.is_some() {
                    return Err("--json does not take a value".into());
                }
                json = true;
            }
            _ => return Err(format!("unknown argument for rules add: {arg}")),
        }
        index += 1;
    }

    if app.is_none() && title.is_none() && title_contains.is_none() {
        return Err("rules add requires --app <bundle-id> (or --title <title>)".into());
    }
    let Some(action) = action else {
        return Err(
            "rules add requires one of --workspace <ws>, --float, --pin, --tile, or --ignore"
                .into(),
        );
    };

    if windows.is_some() && !matches!(action, ActionArg::Workspace(_)) {
        return Err("--windows applies only to a route rule; add --workspace <ws>".into());
    }

    Ok(AddOptions {
        app,
        title,
        title_contains,
        action,
        windows,
        position,
        json,
    })
}

fn run_add(args: &[&str], path_override: Option<&Path>) -> Result<String, String> {
    let options = parse_add_options(args)?;
    let path = resolve_config_path(path_override)?;
    let (action, workspace) = match options.action {
        ActionArg::Workspace(workspace) => ("route".to_owned(), Some(workspace)),
        ActionArg::Float => ("float".to_owned(), None),
        ActionArg::Pin => ("pin".to_owned(), None),
        ActionArg::Tile => ("tile".to_owned(), None),
        ActionArg::Ignore => ("ignore".to_owned(), None),
    };
    let rule = RuleDoc {
        bundle_id: options.app,
        title: options.title,
        title_contains: options.title_contains,
        action,
        workspace,
        windows: options.windows,
    };

    let display_index = ConfigDoc::edit_locked(&path, |doc| {
        let insert_at = match options.position {
            Some(position) => {
                let max_position = doc.rules.len() + 1;
                if position > max_position {
                    return Err(format!(
                        "--position {position} is out of range (valid: 1..={max_position})"
                    ));
                }
                position - 1
            }
            None => doc.rules.len(),
        };
        doc.rules.insert(insert_at, rule.clone());
        Ok((insert_at + 1, true))
    })?;
    if options.json {
        to_json_pretty(&RuleAddedOutput {
            path: path.display().to_string(),
            rule: RuleView::from_doc(display_index, &rule),
        })
    } else {
        Ok(format!(
            "added rule {display_index}: {}",
            format_rule_summary(&rule)
        ))
    }
}

fn parse_remove_args(args: &[&str]) -> Result<(RemoveTarget, bool), String> {
    let mut json = false;
    let mut explicit_app: Option<String> = None;
    let mut positional: Vec<&str> = Vec::new();
    let mut index = 0;
    let mut after_double_dash = false;

    while index < args.len() {
        let arg = args[index];
        if !after_double_dash && arg == "--" {
            after_double_dash = true;
            index += 1;
            continue;
        }
        if !after_double_dash {
            let (flag, inline) = match arg.split_once('=') {
                Some((flag, value)) if flag.starts_with("--") => (flag, Some(value)),
                _ => (arg, None),
            };
            match flag {
                "--json" => {
                    if inline.is_some() {
                        return Err("--json does not take a value".into());
                    }
                    json = true;
                    index += 1;
                    continue;
                }
                "--app" | "--bundle-id" => {
                    if explicit_app.is_some() {
                        return Err(format!("{flag} specified more than once"));
                    }
                    let value = if let Some(value) = inline {
                        value.to_owned()
                    } else {
                        index += 1;
                        args.get(index)
                            .map(|value| (*value).to_owned())
                            .ok_or_else(|| format!("missing value for {flag}"))?
                    };
                    if value.trim().is_empty() {
                        return Err(format!("{flag} cannot be empty"));
                    }
                    explicit_app = Some(value);
                    index += 1;
                    continue;
                }
                other if other.starts_with('-') => {
                    return Err(format!("unknown option for rules remove: {arg}"));
                }
                _ => {}
            }
        }
        positional.push(arg);
        index += 1;
    }

    match (explicit_app, positional.as_slice()) {
        (Some(bundle_id), []) => Ok((RemoveTarget::App(bundle_id), json)),
        (Some(_), _) => Err(
            "pass either --app <bundle-id> or a positional <index-or-bundle-id>, not both".into(),
        ),
        (None, [target]) => {
            if !target.is_empty() && target.chars().all(|character| character.is_ascii_digit()) {
                let parsed = target
                    .parse::<usize>()
                    .ok()
                    .filter(|value| *value > 0)
                    .ok_or_else(|| {
                        format!("rule index must be a positive 1-based integer, got {target:?}")
                    })?;
                Ok((RemoveTarget::Index(parsed), json))
            } else if target.trim().is_empty() {
                Err("bundle ID cannot be empty".into())
            } else {
                Ok((RemoveTarget::App((*target).to_owned()), json))
            }
        }
        (None, _) => Err(
            "usage: fractile rules remove (<index-or-bundle-id> | --app <bundle-id>) [--json]"
                .into(),
        ),
    }
}

fn run_remove(args: &[&str], path_override: Option<&Path>) -> Result<String, String> {
    let (target, json) = parse_remove_args(args)?;
    let path = resolve_config_path(path_override)?;

    let (removed, remaining) = ConfigDoc::edit_locked(&path, |doc| {
        let mut removed = Vec::new();
        match target {
            RemoveTarget::Index(position) => {
                if position == 0 || position > doc.rules.len() {
                    return Err(format!(
                        "rule index {position} is out of range ({} rules configured)",
                        doc.rules.len()
                    ));
                }
                let rule = doc.rules.remove(position - 1);
                removed.push(RuleView::from_doc(position, &rule));
            }
            RemoveTarget::App(bundle_id) => {
                let mut kept = Vec::with_capacity(doc.rules.len());
                for (index, rule) in doc.rules.drain(..).enumerate() {
                    if rule.bundle_id.as_deref() == Some(bundle_id.as_str()) {
                        removed.push(RuleView::from_doc(index + 1, &rule));
                    } else {
                        kept.push(rule);
                    }
                }
                doc.rules = kept;
                if removed.is_empty() {
                    return Err(format!("no rules found for bundle_id {bundle_id:?}"));
                }
            }
        }
        let remaining = doc.rules.len();
        Ok(((removed, remaining), true))
    })?;
    if json {
        to_json_pretty(&RuleRemovedOutput {
            path: path.display().to_string(),
            removed,
            remaining,
        })
    } else if removed.len() == 1 {
        let entry = &removed[0];
        let rule = RuleDoc {
            bundle_id: entry.bundle_id.clone(),
            title: entry.title.clone(),
            title_contains: entry.title_contains.clone(),
            action: entry.action.clone(),
            workspace: entry.workspace,
            windows: entry.windows,
        };
        Ok(format!(
            "removed rule {}: {}",
            entry.index,
            format_rule_summary(&rule)
        ))
    } else {
        Ok(format!(
            "removed {} rules ({remaining} remaining)",
            removed.len()
        ))
    }
}
