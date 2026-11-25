//! Which panes Settings lists: every pane, minus the Lab panes whose `[labs]` switch is off.

use super::*;

fn activity_lab() -> LabsConfig {
    LabsConfig {
        activity: true,
        ..LabsConfig::default()
    }
}

#[test]
fn lab_panes_stay_out_of_the_sidebar_until_their_switch_is_on() {
    assert!(!Pane::Activity.visible(LabsConfig::default()));
    assert_eq!(
        Pane::listed(LabsConfig::default()).collect::<Vec<_>>(),
        vec![
            Pane::General,
            Pane::Layout,
            Pane::Rules,
            Pane::Shortcuts,
            Pane::Labs,
        ]
    );

    assert!(Pane::Activity.visible(activity_lab()));
    assert_eq!(
        Pane::listed(activity_lab()).collect::<Vec<_>>(),
        Pane::ALL.to_vec()
    );
    // The plain panes never depend on a switch.
    for pane in [
        Pane::General,
        Pane::Layout,
        Pane::Rules,
        Pane::Shortcuts,
        Pane::Labs,
    ] {
        assert!(pane.visible(LabsConfig::default()));
        assert!(pane.visible(activity_lab()));
    }
}

#[test]
fn command_shortcuts_skip_a_hidden_lab_pane() {
    assert_eq!(
        Pane::from_shortcut('4', LabsConfig::default()),
        Some(Pane::Shortcuts)
    );
    // Labs is always listed, last, so it takes the digit after the last pane shown.
    assert_eq!(
        Pane::from_shortcut('5', LabsConfig::default()),
        Some(Pane::Labs)
    );
    assert_eq!(Pane::from_shortcut('6', LabsConfig::default()), None);
    assert_eq!(
        Pane::from_shortcut('5', activity_lab()),
        Some(Pane::Activity)
    );
    assert_eq!(Pane::from_shortcut('6', activity_lab()), Some(Pane::Labs));
    assert_eq!(Pane::from_shortcut('7', activity_lab()), None);
}

#[test]
fn the_labs_deep_link_opens_the_labs_pane() {
    assert_eq!(
        Pane::from_deep_link(spaceman::deep_link::DeepLinkPane::Labs),
        Some(Pane::Labs)
    );
}
