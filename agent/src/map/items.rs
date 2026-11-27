//! What a workspace's card holds: an icon for each rule that sends new windows there, and one
//! for each window open there now. An app with two windows open shows twice; an app a rule
//! sends there shows even with none open; a window no rule sent shows too. Each open window's
//! icon carries that window, so dragging it moves only it.

use fractile::{PhysicalWindowId, WorkspaceId, app::UiWorkspaceWindows};

use crate::rules::Rule;

/// One icon on a card.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Item {
    /// The workspace's `n`th rule, which drags and takes the rule's menu.
    Rule(usize),
    /// Another open window of the app with this bundle ID: that window, when the engine named
    /// it.
    Window(String, Option<PhysicalWindowId>),
}

/// One of a workspace's open windows: its app, and the window when the engine named it.
pub(super) type Open = (String, Option<PhysicalWindowId>);

/// The bundle IDs of `workspace`'s open windows, tiles in layout order then floats, as the
/// engine last reported them.
pub(super) fn open(windows: &[UiWorkspaceWindows], workspace: WorkspaceId) -> Vec<Open> {
    windows
        .iter()
        .filter(|entry| entry.windows.workspace == workspace)
        .flat_map(|entry| {
            let named: Vec<Open> = entry
                .open
                .iter()
                .map(|open| (open.bundle.clone(), Some(open.window.clone())))
                .collect();
            if !named.is_empty() {
                return named;
            }
            // An older engine names no windows: fall back to the preview's apps.
            entry
                .windows
                .tiles
                .iter()
                .filter_map(|tile| tile.bundle.clone())
                .chain(entry.windows.floating.iter().cloned())
                .map(|bundle| (bundle, None))
                .collect()
        })
        .collect()
}

/// The card's icons: each rule, followed (after its app's last rule) by the app's open windows
/// past one per rule; then the windows of apps no rule sends here, one each.
pub(super) fn items(rules: &[Rule], open: &[Open]) -> Vec<Item> {
    let mut out = Vec::new();
    for (index, rule) in rules.iter().enumerate() {
        out.push(Item::Rule(index));
        let Some(bundle) = rule.bundle_id.as_deref() else {
            continue;
        };
        let last = !rules[index + 1..]
            .iter()
            .any(|other| other.bundle_id.as_deref() == Some(bundle));
        if last {
            let ruled = rules
                .iter()
                .filter(|other| other.bundle_id.as_deref() == Some(bundle))
                .count();
            // The rules' icons stand for the app's first windows; the rest show on their own.
            out.extend(
                open.iter()
                    .filter(|(b, _)| b == bundle)
                    .skip(ruled)
                    .map(|(bundle, window)| Item::Window(bundle.clone(), window.clone())),
            );
        }
    }
    for (bundle, window) in open {
        if !rules
            .iter()
            .any(|rule| rule.bundle_id.as_deref() == Some(bundle.as_str()))
        {
            out.push(Item::Window(bundle.clone(), window.clone()));
        }
    }
    out
}

/// Open windows for stills: two Safari windows on 1, two Firefox windows and Slack on 2.
pub(super) fn fixture() -> Vec<UiWorkspaceWindows> {
    use fractile::{
        Rect,
        app::{UiHudTile, UiHudWorkspace},
    };
    let entry = |workspace, bundles: &[&str]| UiWorkspaceWindows {
        windows: UiHudWorkspace {
            workspace,
            fullscreen: false,
            tiles: bundles
                .iter()
                .enumerate()
                .map(|(key, bundle)| UiHudTile {
                    key: key as u64,
                    rect: Rect::default(),
                    bundle: Some((*bundle).to_owned()),
                })
                .collect(),
            floating: Vec::new(),
        },
        portrait: false,
        open: bundles
            .iter()
            .enumerate()
            .map(|(index, bundle)| fractile::app::UiOpenWindow {
                bundle: (*bundle).to_owned(),
                window: PhysicalWindowId {
                    app: fractile::AppInstanceId {
                        pid: 100,
                        launched_at: 1,
                    },
                    window_id: u32::from(workspace.number().unwrap_or(0)) * 10 + index as u32,
                },
            })
            .collect(),
    };
    vec![
        entry(WorkspaceId::ONE, &["com.apple.Safari", "com.apple.Safari"]),
        entry(
            WorkspaceId::TWO,
            &[
                "org.mozilla.firefox",
                "org.mozilla.firefox",
                "com.tinyspeck.slackmacgap",
            ],
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::Place;

    fn rule(index: usize, bundle: &str) -> Rule {
        Rule {
            index,
            bundle_id: Some(bundle.to_owned()),
            title: None,
            title_contains: None,
            place: Place::Workspace(WorkspaceId::TWO),
            all_windows: false,
        }
    }

    #[test]
    fn an_app_shows_once_per_open_window_and_once_for_its_rule_with_none_open() {
        let open: Vec<Open> = ["firefox", "firefox", "slack", "notes"]
            .map(|bundle| (bundle.to_owned(), None))
            .to_vec();
        let rules = [rule(0, "firefox"), rule(1, "safari"), rule(2, "slack")];
        assert_eq!(
            items(&rules, &open),
            vec![
                Item::Rule(0),
                Item::Window("firefox".into(), None),
                Item::Rule(1),
                Item::Rule(2),
                Item::Window("notes".into(), None),
            ]
        );
        // Two rules for one app take two of its windows; the third follows the second rule.
        let three: Vec<Open> = ["safari"; 3].map(|b| (b.to_owned(), None)).to_vec();
        let rules = [rule(0, "safari"), rule(1, "safari")];
        assert_eq!(
            items(&rules, &three),
            vec![
                Item::Rule(0),
                Item::Rule(1),
                Item::Window("safari".into(), None)
            ]
        );
        assert_eq!(items(&[], &[]), vec![]);
    }
}
