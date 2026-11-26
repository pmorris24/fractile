//! The three-finger swipe's row: the fixed tray while `[labs] swipe_switch` is off, a switch
//! for `settings.trackpad_swipe` only with it on.

use fractile::config::{Config, LabFeature};

use super::SwipeRow;
use crate::{controls::Paint, rules::Doc};

#[test]
fn the_swipe_row_stays_fixed_until_its_lab_is_on() {
    let plain = Doc::parse("version = 1\n");
    assert_eq!(SwipeRow::of(&plain), SwipeRow::Fixed);
    // The setting alone changes nothing while the Lab is off.
    let off = Doc::parse("version = 1\n[settings]\ntrackpad_swipe = false\n");
    assert_eq!(SwipeRow::of(&off), SwipeRow::Fixed);

    let mut doc = Doc::parse("version = 1\n[labs]\nswipe_switch = true\n");
    assert_eq!(SwipeRow::of(&doc), SwipeRow::Switch { on: true });
    doc.set_trackpad_swipe(false);
    assert_eq!(SwipeRow::of(&doc), SwipeRow::Switch { on: false });
    assert!(!Config::parse(&doc.source()).unwrap().trackpad_swipe());

    // Turning the Lab back off brings the fixed row back, and swipes switch again.
    doc.set_lab(LabFeature::SwipeSwitch, false);
    assert_eq!(SwipeRow::of(&doc), SwipeRow::Fixed);
    assert!(Config::parse(&doc.source()).unwrap().trackpad_swipe());
}

#[test]
fn the_shortcuts_pane_draws_with_the_swipe_lab_off_and_on() {
    let p = Paint::new(true, 2.0);
    let mut ui = crate::playground::blank_fixture();
    let _ = super::pane(&ui, p);
    ui.doc.set_lab(LabFeature::SwipeSwitch, true);
    let _ = super::pane(&ui, p);
}
