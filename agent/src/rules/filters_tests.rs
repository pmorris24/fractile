use fractile::{
    AppInstanceId, ObservedWindow, PhysicalWindowId, Rect, WorkspaceId,
    config::{Config, WindowDisposition},
};

use super::super::{Doc, Place};

const SAFARI: &str = "com.apple.Safari";

fn ws(number: u8) -> WorkspaceId {
    WorkspaceId::new(number).unwrap()
}

fn window(title: &str) -> ObservedWindow {
    ObservedWindow {
        id: PhysicalWindowId {
            app: AppInstanceId {
                pid: 1,
                launched_at: 1,
            },
            window_id: 1,
        },
        frame: Rect {
            x: 0,
            y: 0,
            width: 800,
            height: 600,
        },
        resizable: true,
        title: Some(title.into()),
        document: None,
    }
}

fn disposition(doc: &Doc, title: &str) -> WindowDisposition {
    Config::parse(&doc.source())
        .unwrap()
        .disposition(SAFARI, &window(title))
}

#[test]
fn a_title_contains_rule_writes_valid_toml_ahead_of_the_every_window_rule() {
    let mut doc = Doc::parse("version = 1\n");
    doc.add_rule(SAFARI, None, Place::Workspace(ws(3)));
    doc.add_rule(SAFARI, Some("Work"), Place::Workspace(ws(1)));
    doc.add_rule(SAFARI, Some("Claude"), Place::Workspace(ws(2)));

    let source = doc.source();
    assert_eq!(
        source,
        concat!(
            "version = 1\n\n",
            "[[rules]]\n",
            "bundle_id = \"com.apple.Safari\"\n",
            "title_contains = \"Work\"\n",
            "action = \"route\"\n",
            "workspace = 1\n\n",
            "[[rules]]\n",
            "bundle_id = \"com.apple.Safari\"\n",
            "title_contains = \"Claude\"\n",
            "action = \"route\"\n",
            "workspace = 2\n\n",
            "[[rules]]\n",
            "bundle_id = \"com.apple.Safari\"\n",
            "action = \"route\"\n",
            "workspace = 3\n",
        )
    );
    // The same order survives a save and a reload.
    assert_eq!(Doc::parse(&source).source(), source);
    // Matching is in memory, on a part of the title, whatever its case.
    assert_eq!(
        disposition(&doc, "Work — Pull request 42"),
        WindowDisposition::Route(ws(1))
    );
    assert_eq!(
        disposition(&doc, "Fix layout - claude"),
        WindowDisposition::Route(ws(2))
    );
    assert_eq!(
        disposition(&doc, "Start Page"),
        WindowDisposition::Route(ws(3))
    );
}

#[test]
fn the_same_app_and_filter_moves_its_rule_instead_of_doubling_it() {
    let mut doc = Doc::parse("version = 1\n");
    doc.add_rule(SAFARI, Some("Work"), Place::Workspace(ws(1)));
    doc.add_rule(SAFARI, Some("work"), Place::Workspace(ws(4)));
    doc.add_rule(SAFARI, None, Place::Float);
    doc.add_rule(SAFARI, None, Place::Pin);

    let rules = doc.rules();
    assert_eq!(rules.len(), 2);
    assert_eq!(rules[0].title_contains.as_deref(), Some("Work"));
    assert_eq!(rules[0].place, Place::Workspace(ws(4)));
    assert_eq!(rules[1].place, Place::Pin);
    assert!(doc.rule_matching(SAFARI, Some("WORK"), None).is_some());
    assert!(doc.rule_matching(SAFARI, Some("WORK"), Some(0)).is_none());
    assert!(doc.rule_matching(SAFARI, Some("Wor"), None).is_none());
    Config::parse(&doc.source()).unwrap();
}

#[test]
fn moving_the_every_window_rule_keeps_comments_and_other_tables() {
    let mut doc = Doc::parse(concat!(
        "version = 1\n\n",
        "[settings]\n",
        "hud = false\n\n",
        "[[rules]]\n",
        "bundle_id = \"com.tinyspeck.slackmacgap\"\n",
        "action = \"route\"\n",
        "workspace = 2\n\n",
        "# Safari lives on 1.\n",
        "[[rules]]\n",
        "bundle_id = \"com.apple.Safari\"\n",
        "action = \"route\"\n",
        "workspace = 1 # home\n\n",
        "[[rules]]\n",
        "bundle_id = \"com.apple.mail\"\n",
        "action = \"float\"\n\n",
        "[[workspaces]]\n",
        "workspace = 9\n",
        "monitor = \"main\"\n",
    ));
    doc.add_rule(SAFARI, Some("Claude"), Place::Workspace(ws(2)));

    assert_eq!(
        doc.source(),
        concat!(
            "version = 1\n\n",
            "[settings]\n",
            "hud = false\n\n",
            "[[rules]]\n",
            "bundle_id = \"com.tinyspeck.slackmacgap\"\n",
            "action = \"route\"\n",
            "workspace = 2\n\n",
            "[[rules]]\n",
            "bundle_id = \"com.apple.mail\"\n",
            "action = \"float\"\n\n",
            "[[rules]]\n",
            "bundle_id = \"com.apple.Safari\"\n",
            "title_contains = \"Claude\"\n",
            "action = \"route\"\n",
            "workspace = 2\n\n",
            "# Safari lives on 1.\n",
            "[[rules]]\n",
            "bundle_id = \"com.apple.Safari\"\n",
            "action = \"route\"\n",
            "workspace = 1 # home\n\n",
            "[[workspaces]]\n",
            "workspace = 9\n",
            "monitor = \"main\"\n",
        )
    );
    assert_eq!(disposition(&doc, "Claude"), WindowDisposition::Route(ws(2)));
    assert_eq!(disposition(&doc, "News"), WindowDisposition::Route(ws(1)));
    assert!(!Config::parse(&doc.source()).unwrap().hud());
}

#[test]
fn setting_and_clearing_a_filter_edits_the_rule_in_place() {
    let mut doc = Doc::parse(concat!(
        "version = 1\n\n",
        "[[rules]]\n",
        "bundle_id = \"com.apple.Safari\"\n",
        "action = \"route\"\n",
        "workspace = 1\n\n",
        "[[rules]]\n",
        "bundle_id = \"com.apple.Safari\"\n",
        "action = \"float\"\n",
    ));
    // The second rule could never match: give it a filter and it goes ahead of the first.
    let second = doc.rules()[1].clone();
    doc.set_title_contains_matching(&second, Some("Picture in Picture"))
        .unwrap();
    assert_eq!(
        doc.source(),
        concat!(
            "version = 1\n\n",
            "[[rules]]\n",
            "bundle_id = \"com.apple.Safari\"\n",
            "title_contains = \"Picture in Picture\"\n",
            "action = \"float\"\n\n",
            "[[rules]]\n",
            "bundle_id = \"com.apple.Safari\"\n",
            "action = \"route\"\n",
            "workspace = 1\n",
        )
    );
    assert_eq!(
        disposition(&doc, "Picture in Picture"),
        WindowDisposition::Floating
    );

    // A rule the file no longer has is a conflict, not a guess.
    assert!(doc.set_title_contains_matching(&second, None).is_err());

    let filtered = doc.rules()[0].clone();
    doc.set_title_contains_matching(&filtered, None).unwrap();
    assert!(doc.rules().iter().all(|rule| rule.app_wide()));
    assert!(!doc.source().contains("title_contains"));
    Config::parse(&doc.source()).unwrap();
}

#[test]
fn inline_rules_take_filters_and_keep_their_order_rule() {
    let mut doc = Doc::parse(concat!(
        "version = 1\n",
        "rules = [\n",
        "  { bundle_id = \"com.apple.Safari\", action = \"route\", workspace = 1 },\n",
        "  { bundle_id = \"com.apple.mail\", action = \"float\" },\n",
        "]\n",
    ));
    doc.add_rule(SAFARI, Some("Work"), Place::Workspace(ws(2)));

    let rules = doc.rules();
    assert_eq!(rules.len(), 3);
    assert_eq!(rules[0].bundle_id.as_deref(), Some("com.apple.mail"));
    assert_eq!(rules[1].title_contains.as_deref(), Some("Work"));
    assert!(rules[2].app_wide());
    let source = doc.source();
    assert!(source.starts_with("version = 1\nrules = [\n"), "{source}");
    assert_eq!(
        disposition(&doc, "Work — inbox"),
        WindowDisposition::Route(ws(2))
    );
    assert_eq!(disposition(&doc, "News"), WindowDisposition::Route(ws(1)));

    let filtered = rules[1].clone();
    doc.set_title_contains_matching(&filtered, Some("Docs"))
        .unwrap();
    assert_eq!(doc.rules()[1].title_contains.as_deref(), Some("Docs"));
    Config::parse(&doc.source()).unwrap();
}

#[test]
fn a_filter_with_quotes_and_unicode_round_trips() {
    let mut doc = Doc::parse("version = 1\n");
    let filter = "Work — \"Q4\" \\ plan";
    doc.add_rule(SAFARI, Some(filter), Place::Workspace(ws(1)));
    let reloaded = Doc::parse(&doc.source());
    assert!(reloaded.error.is_none(), "{:?}", reloaded.error);
    assert_eq!(reloaded.rules()[0].title_contains.as_deref(), Some(filter));
}

#[test]
fn windows_key_stays_while_a_rule_routes_and_goes_when_it_stops() {
    let mut doc = Doc::parse(
        "version = 1\n[[rules]]\nbundle_id = \"com.google.Chrome\"\naction = \"route\"\nworkspace = 4\nwindows = \"all\"\n",
    );
    doc.place(0, Place::Workspace(ws(2)));
    let source = doc.source();
    assert!(source.contains("windows = \"all\""), "{source}");
    assert!(Config::parse(&source).is_ok());
    doc.place(0, Place::Float);
    let source = doc.source();
    assert!(!source.contains("windows"), "{source}");
    assert!(Config::parse(&source).is_ok(), "{source}");
}
