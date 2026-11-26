use fractile::shortcuts::parse_chord;
use rune::{Mods, PhysicalKeyCode};

use super::*;
use crate::onboarding::{self, Input, Step, stage, try_it};

/// A US layout, where F is key 3.
fn us(chord: Chord) -> Option<(u32, u32)> {
    (chord.key == Key::Character('f')).then(|| (3, carbon(chord.modifiers)))
}

fn chord(source: &str) -> Chord {
    parse_chord(source).unwrap()
}

fn try_it_ui() -> Ui {
    let mut ui = crate::playground::blank_fixture();
    onboarding::tour_scene(&mut ui, Step::TryIt, true, false);
    ui
}

fn set_base_modifier(ui: &mut Ui, modifier: Modifiers) {
    ui.edit(|doc| doc.set_shortcut_modifier(modifier));
    assert_eq!(ui.error, None);
}

const MOVED: Pressed = Pressed {
    moved: true,
    learned: false,
};
const LEARNED: Pressed = Pressed {
    moved: true,
    learned: true,
};
const NOTHING: Pressed = Pressed {
    moved: false,
    learned: false,
};

#[test]
fn the_seven_lessons_are_asked_for_one_at_a_time_in_order() {
    let mut demo = Demo::default();
    let titles: Vec<&str> = LESSONS.iter().map(|lesson| lesson.title).collect();
    assert_eq!(
        titles,
        [
            "Switch to workspace 2",
            "Switch back to workspace 1",
            "Float this window",
            "Tile it again",
            "Fill the screen",
            "Cycle to the window behind",
            "Back to tiles",
        ]
    );
    let presses = [
        Move::There,
        Move::Back,
        Move::Float,
        Move::Float,
        Move::Fullscreen,
        Move::Back,
        Move::Fullscreen,
    ];
    for (index, mv) in presses.into_iter().enumerate() {
        assert_eq!(demo.learned(), index);
        assert_eq!(demo.next().map(|lesson| lesson.mv), Some(mv));
        assert!(!demo.complete());
        assert_eq!(demo.press(mv), LEARNED, "lesson {index}");
    }
    assert!(demo.complete());
    assert_eq!(demo.next(), None);
    // Each layout was undone by its second press: the desktop ends as it began, but for the
    // window that came forward, which keeps focus.
    assert_eq!(
        (demo.workspace(), demo.mode(0), demo.mode(1)),
        (0, Mode::Tiled, Mode::Tiled)
    );
    assert_eq!(demo.desk(0).focus, 1);
}

#[test]
fn switching_counts_only_when_the_workspace_changes_and_its_number_again_moves_focus() {
    let mut demo = Demo::default();
    // Workspace 1's number on workspace 1 focuses its next window, and teaches nothing yet.
    assert_eq!(demo.press(Move::Back), MOVED);
    assert_eq!((demo.workspace(), demo.desk(0).focus), (0, 1));
    assert_eq!(demo.press(Move::Back), MOVED);
    assert_eq!(demo.desk(0).focus, 0, "and wraps around");
    assert_eq!(demo.learned(), 0);

    assert_eq!(demo.press(Move::There), LEARNED);
    assert_eq!(demo.workspace(), 1);
    assert_eq!(demo.press(Move::There), MOVED, "already there");
    assert_eq!((demo.desk(1).focus, demo.learned()), (1, 1));
    assert_eq!(demo.press(Move::Back), LEARNED);
    assert_eq!((demo.workspace(), demo.learned()), (0, 2));
    // Each workspace keeps its own focus.
    assert_eq!((demo.desk(0).focus, demo.desk(1).focus), (0, 1));
}

#[test]
fn tiles_keep_their_places_as_focus_moves_and_a_floating_window_keeps_focus() {
    let mut demo = Demo::default();
    let tiled = frames(demo.desk(0));
    demo.press(Move::Back);
    assert_eq!(frames(demo.desk(0)), tiled);
    // The focused window is the one that lifts, wherever it tiles.
    demo.press(Move::Float);
    let [safari, terminal] = frames(demo.desk(0));
    assert_eq!((safari.pose, terminal.pose), (Pose::Tiled, Pose::Lifted));
    assert_eq!(demo.press(Move::Back), NOTHING);
    assert_eq!(demo.desk(0).focus, 1);
}

#[test]
fn another_shortcut_moves_the_desktop_without_learning_the_lesson_asked_for() {
    let mut demo = Demo::default();
    // Float is not the first lesson: the window lifts, and Switch to 2 is still asked for.
    assert_eq!(demo.press(Move::Float), MOVED);
    assert_eq!(demo.mode(0), Mode::Floated);
    assert_eq!(demo.next().map(|lesson| lesson.mv), Some(Move::There));
    assert_eq!(demo.press(Move::Fullscreen), MOVED);
    assert_eq!(demo.mode(0), Mode::Fullscreen);
    assert_eq!(demo.learned(), 0);
}

#[test]
fn floating_lifts_the_focused_window_and_the_second_press_tiles_it_again() {
    let mut demo = Demo::default();
    demo.press(Move::There);
    demo.press(Move::Back);
    assert_eq!(demo.press(Move::Float), LEARNED);
    assert_eq!(demo.mode(0), Mode::Floated);
    assert_eq!(
        demo.mode(1),
        Mode::Tiled,
        "the other workspace is untouched"
    );
    assert_eq!(
        demo.next().map(|lesson| lesson.title),
        Some("Tile it again")
    );

    let [lifted, other] = frames(demo.desk(0));
    assert_eq!(lifted.pose, Pose::Lifted);
    // Off the grid: inset from every edge.
    assert!(lifted.x > 0.0 && lifted.y > 0.0);
    assert!(lifted.x + lifted.w < 1.0 && lifted.y + lifted.h < 1.0);
    assert_eq!((other.x, other.y, other.w, other.h), (0.0, 0.0, 1.0, 1.0));
    assert_eq!(other.pose, Pose::Tiled);
    // On the stage the tile that stays is more than twice as wide as it was beside the other,
    // and starts at the screen's left edge.
    let [left, right] = frames(Desk::default());
    let (left_x, _, _, _) = stage::rect(left, Mode::Tiled);
    let (_, _, tiled_w, _) = stage::rect(right, Mode::Tiled);
    let (wide_x, _, wide_w, _) = stage::rect(other, Mode::Floated);
    assert!(wide_w > 2.0 * tiled_w, "{wide_w} against {tiled_w}");
    assert_eq!(wide_x, left_x);

    // Floating the other workspace's window is not tiling this one again.
    demo.press(Move::There);
    assert_eq!(demo.press(Move::Float), MOVED);
    assert_eq!(demo.learned(), 3);
    // Tiling it is.
    assert_eq!(demo.press(Move::Float), LEARNED);
    assert_eq!((demo.mode(1), demo.learned()), (Mode::Tiled, 4));
}

#[test]
fn fullscreen_layout_keeps_the_other_window_behind_and_the_second_press_brings_the_tiles_back() {
    let mut demo = Demo::default();
    for mv in [Move::There, Move::Back, Move::Float, Move::Float] {
        demo.press(mv);
    }
    assert_eq!(demo.press(Move::Fullscreen), LEARNED);
    assert_eq!(demo.mode(0), Mode::Fullscreen);

    let [front, behind] = frames(demo.desk(0));
    assert_eq!((front.w, front.h, front.pose), (1.0, 1.0, Pose::Tiled));
    assert_eq!(behind.pose, Pose::Behind);
    // The window behind shows as a narrower tab above the one in front.
    let (front_x, front_y, front_w, _) = stage::rect(front, Mode::Fullscreen);
    let (tab_x, tab_y, tab_w, tab_h) = stage::rect(behind, Mode::Fullscreen);
    assert!(tab_y < front_y && tab_y + tab_h > front_y);
    assert!(tab_x > front_x && tab_x + tab_w < front_x + front_w);

    // Float takes over from Fullscreen Layout, which is not going back to the tiles.
    assert_eq!(demo.press(Move::Float), MOVED);
    assert_eq!(demo.mode(0), Mode::Floated);
    assert_eq!(demo.press(Move::Fullscreen), MOVED);
    assert_eq!((demo.mode(0), demo.learned()), (Mode::Fullscreen, 5));

    // Workspace 1's number again brings Terminal forward from behind, and Safari tucks behind.
    assert_eq!(
        demo.next().map(|lesson| lesson.title),
        Some("Cycle to the window behind")
    );
    assert_eq!(demo.press(Move::Back), LEARNED);
    let [safari, terminal] = frames(demo.desk(0));
    assert_eq!((safari.pose, terminal.pose), (Pose::Behind, Pose::Tiled));
    assert_eq!((terminal.w, terminal.h), (1.0, 1.0));
    assert_eq!(demo.press(Move::Fullscreen), LEARNED);
    assert_eq!(demo.mode(0), Mode::Tiled);
    assert!(demo.complete());
}

#[test]
fn the_cycle_is_learned_only_in_fullscreen_layout() {
    let mut demo = Demo::default();
    for mv in [Move::There, Move::Back, Move::Float, Move::Float] {
        demo.press(mv);
    }
    demo.press(Move::Fullscreen);
    // Leaving Fullscreen Layout first moves the desktop but is not the cycle.
    assert_eq!(demo.press(Move::Fullscreen), MOVED);
    assert_eq!(demo.press(Move::Back), MOVED);
    assert_eq!(demo.learned(), 5);
    // Back in Fullscreen Layout, the next press is.
    demo.press(Move::Fullscreen);
    assert_eq!(demo.press(Move::Back), LEARNED);
    assert_eq!(demo.desk(0).focus, 0, "round again to Safari");
}

#[test]
fn free_play_goes_on_after_the_last_lesson() {
    let mut demo = Demo::default();
    for lesson in LESSONS {
        demo.press(lesson.mv);
    }
    assert!(demo.complete());
    // Every key still moves the desktop, and each workspace keeps its own layout.
    assert_eq!(demo.press(Move::Float), MOVED);
    assert_eq!(demo.press(Move::There), MOVED);
    assert_eq!(demo.press(Move::Fullscreen), MOVED);
    assert_eq!(
        (demo.mode(0), demo.mode(1)),
        (Mode::Floated, Mode::Fullscreen)
    );
    assert_eq!(demo.press(Move::Back), MOVED);
    assert_eq!(demo.learned(), LESSONS.len());
    assert!(demo.complete());
}

#[test]
fn the_default_chords_are_heard_by_key_code() {
    let ui = crate::playground::blank_fixture();
    let chords = chords(&ui);
    assert_eq!(
        chords,
        [
            Some(chord("alt+2")),
            Some(chord("alt+1")),
            Some(chord("alt+shift+space")),
            Some(chord("alt+shift+f")),
        ]
    );
    let presses = presses(&chords, us);
    assert!(presses.iter().all(|press| hearable(*press)));
    assert_eq!(heard(&presses, 19, OPTION), Some(Move::There));
    assert_eq!(heard(&presses, 18, OPTION), Some(Move::Back));
    assert_eq!(heard(&presses, 49, OPTION | SHIFT), Some(Move::Float));
    assert_eq!(heard(&presses, 3, OPTION | SHIFT), Some(Move::Fullscreen));
    // Option-Shift-2 moves a window; Option-Space, Option-F and Option-3 are not moves.
    assert_eq!(heard(&presses, 19, OPTION | SHIFT), None);
    assert_eq!(heard(&presses, 49, OPTION), None);
    assert_eq!(heard(&presses, 3, OPTION), None);
    assert_eq!(heard(&presses, 20, OPTION), None);

    // A layout with no key for a character leaves that one move to the window's own keys.
    let unmapped = super::presses(&chords, |_| None);
    assert_eq!(unmapped.map(hearable), [true, true, true, false]);
}

#[test]
fn a_custom_base_modifier_moves_every_chord_and_the_window_hears_what_the_tap_cannot() {
    let mut ui = try_it_ui();
    set_base_modifier(
        &mut ui,
        Modifiers {
            control: true,
            ..Modifiers::default()
        },
    );
    let chords = chords(&ui);
    assert_eq!(
        chords,
        [
            Some(chord("ctrl+2")),
            Some(chord("ctrl+1")),
            Some(chord("ctrl+shift+space")),
            Some(chord("ctrl+shift+f")),
        ]
    );
    assert_eq!(asked(&chords, chord("ctrl+2")), Some(Move::There));
    assert_eq!(
        asked(&chords, chord("ctrl+shift+f")),
        Some(Move::Fullscreen)
    );
    assert_eq!(
        asked(&chords, chord("alt+2")),
        None,
        "the old chord is gone"
    );
    // Without Option the key tap hears none of them.
    let presses = super::presses(&chords, us);
    assert_eq!(presses[0], Some((19, CONTROL)));
    assert!(presses.iter().all(|press| !hearable(*press)));
    // The stage's own keyboard does, by the chord a physical press spells.
    let control = Mods {
        ctrl: true,
        ..Mods::default()
    };
    assert_eq!(
        try_it::asked(&ui, PhysicalKeyCode::Digit2, control),
        Some(Move::There)
    );
    assert_eq!(
        try_it::asked(&ui, PhysicalKeyCode::Digit1, control),
        Some(Move::Back)
    );
    let control_shift = Mods {
        shift: true,
        ..control
    };
    assert_eq!(
        try_it::asked(&ui, PhysicalKeyCode::Space, control_shift),
        Some(Move::Float)
    );
    let option = Mods {
        alt: true,
        ..Mods::default()
    };
    assert_eq!(try_it::asked(&ui, PhysicalKeyCode::Digit2, option), None);
    // No tap is up in a scene, so the window's keys make the move.
    assert!(!ui.tour.as_ref().unwrap().tap_hears(Move::There));

    // Control-Option keeps Option, so the tap hears it again.
    set_base_modifier(
        &mut ui,
        Modifiers {
            control: true,
            option: true,
            ..Modifiers::default()
        },
    );
    let presses = super::presses(&super::chords(&ui), us);
    assert_eq!(heard(&presses, 19, CONTROL | OPTION), Some(Move::There));
    assert_eq!(
        heard(&presses, 3, CONTROL | OPTION | SHIFT),
        Some(Move::Fullscreen)
    );
    assert_eq!(heard(&presses, 19, OPTION), None);
}

#[test]
fn the_modifier_picker_saves_the_base_modifier_and_the_prompt_asks_for_the_new_chord() {
    let mut ui = try_it_ui();
    let control_option = crate::panes::shortcuts::PRESETS[1].1;
    onboarding::set_modifier(&mut ui, control_option);
    assert_eq!(ui.error, None);
    assert_eq!(ui.doc.shortcuts().modifier, control_option);
    assert_eq!(
        chords(&ui),
        [
            Some(chord("ctrl+alt+2")),
            Some(chord("ctrl+alt+1")),
            Some(chord("ctrl+alt+shift+space")),
            Some(chord("ctrl+alt+shift+f")),
        ]
    );
    // The lessons learned so far stay learned.
    assert_eq!(onboarding::press(&mut ui, Move::There, 0.0), Some(LEARNED));
    onboarding::set_modifier(&mut ui, crate::panes::shortcuts::PRESETS[0].1);
    assert_eq!(ui.tour.as_ref().unwrap().play.demo.learned(), 1);
    assert_eq!(chords(&ui)[1], Some(chord("alt+1")));
}

#[test]
fn a_press_sinks_its_keys_for_a_beat_and_a_held_modifier_for_as_long_as_it_is_held() {
    let mut play = try_it::Play::default();
    let there = chord("alt+2");
    let float = chord("alt+shift+space");
    let down = |play: &try_it::Play, mv, chord| -> Vec<bool> {
        play.caps(mv, chord)
            .into_iter()
            .map(|held| held.down)
            .collect()
    };
    // Each key is the same key from one chord to the next, by which key it is.
    let ids = |chord| -> Vec<&str> {
        play.caps(Move::There, chord)
            .into_iter()
            .map(|held| held.id)
            .collect()
    };
    assert_eq!(ids(there), ["option", "action"]);
    assert_eq!(ids(float), ["option", "shift", "action"]);
    assert_eq!(
        ids(chord("ctrl+alt+cmd+f")),
        ["control", "option", "command", "action"]
    );
    assert_eq!(down(&play, Move::There, there), [false, false]);

    // Holding Option sinks every Option key, and nothing else.
    assert!(play.hold(Modifiers::OPTION));
    assert!(!play.hold(Modifiers::OPTION), "no change, no redraw");
    assert_eq!(down(&play, Move::There, there), [true, false]);
    assert_eq!(down(&play, Move::Float, float), [true, false, false]);

    // The press sinks the whole chord, on that move only, and makes the move.
    play.press(Move::There, 10.0);
    assert_eq!(play.demo.workspace(), 1);
    assert_eq!(down(&play, Move::There, there), [true, true]);
    assert_eq!(down(&play, Move::Float, float), [true, false, false]);
    assert_eq!(play.due(), Some(10.0 + try_it::KEY_BEAT));

    // The beats run on the clock they are given, so a scripted run plays them exactly.
    assert!(!play.settle(10.0 + try_it::KEY_BEAT - 0.01), "not yet");
    assert!(play.settle(10.0 + try_it::KEY_BEAT));
    assert!(!play.settle(10.0 + try_it::KEY_BEAT), "settled once");
    assert_eq!(down(&play, Move::There, there), [true, false]);
    assert!(play.hold(Modifiers::default()));
    assert_eq!(down(&play, Move::There, there), [false, false]);
}

#[test]
fn a_learned_lesson_keeps_its_tick_for_a_beat_then_the_next_is_asked_for() {
    use try_it::Card;
    let mut play = try_it::Play::default();
    let asking = |index| Card::Lesson {
        index,
        learned: false,
    };
    assert_eq!(play.card(), asking(0));
    // A press that is not the lesson changes nothing on the card.
    play.press(Move::Float, 1.0);
    play.press(Move::Float, 1.1);
    assert_eq!(play.card(), asking(0));

    play.press(Move::There, 2.0);
    assert_eq!(
        play.card(),
        Card::Lesson {
            index: 0,
            learned: true
        }
    );
    play.settle(2.0 + try_it::KEY_BEAT);
    assert_eq!(
        play.card(),
        Card::Lesson {
            index: 0,
            learned: true
        },
        "the tick outlasts the keys"
    );
    assert_eq!(play.due(), Some(2.0 + try_it::TICK_BEAT));
    assert!(play.settle(2.0 + try_it::TICK_BEAT));
    assert_eq!(play.card(), asking(1));
    assert_eq!(play.due(), None);

    // A quick second press moves the tick on to its own lesson.
    play.press(Move::Back, 5.0);
    play.press(Move::Float, 5.1);
    assert_eq!(
        play.card(),
        Card::Lesson {
            index: 2,
            learned: true
        }
    );
    play.settle(5.0 + try_it::TICK_BEAT);
    play.press(Move::Float, 9.0);

    for mv in [Move::Fullscreen, Move::Back, Move::Fullscreen] {
        play.press(mv, 12.0);
    }
    assert_eq!(
        play.card(),
        Card::Lesson {
            index: 6,
            learned: true
        }
    );
    play.rest();
    assert_eq!(play.card(), Card::Ready);
    // Free play never brings a lesson back.
    play.press(Move::Fullscreen, 20.0);
    play.settle(30.0);
    assert_eq!(play.card(), Card::Ready);
}

#[test]
fn a_press_counts_only_on_try_it_and_undo_starts_the_desktop_over() {
    let mut ui = crate::playground::blank_fixture();
    assert_eq!(
        onboarding::press(&mut ui, Move::There, 0.0),
        None,
        "no tour"
    );
    onboarding::tour_scene(&mut ui, Step::Arrange, true, false);
    assert_eq!(
        onboarding::press(&mut ui, Move::There, 0.0),
        None,
        "not on Try it yet"
    );

    let mut ui = try_it_ui();
    assert_eq!(onboarding::press(&mut ui, Move::Float, 0.0), Some(MOVED));
    // Even a press that moves nothing shows on its keys: a floating window keeps focus.
    let floated = ui.tour.as_ref().unwrap().play.demo;
    assert_eq!(onboarding::press(&mut ui, Move::Back, 0.0), Some(NOTHING));
    assert_eq!(ui.tour.as_ref().unwrap().play.demo, floated);
    assert_eq!(onboarding::press(&mut ui, Move::There, 0.0), Some(LEARNED));
    assert_eq!(onboarding::press(&mut ui, Move::Float, 0.0), Some(MOVED));
    let demo = ui.tour.as_ref().unwrap().play.demo;
    assert_eq!((demo.workspace(), demo.mode(1)), (1, Mode::Floated));

    onboarding::input(&mut ui, Input::Undo);
    let tour = ui.tour.as_ref().unwrap();
    assert_eq!(tour.flow.step(), Step::Arrange);
    assert_eq!(tour.play.demo, Demo::default());
}
