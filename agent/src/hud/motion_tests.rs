//! Frame-by-frame checks of the HUD's motion: the overlay's own view and flow, stepped at
//! 120 Hz on a virtual clock and read back as presented rects, opacity and scale.
//!
//! `cargo test -p fractile-agent hud::motion_tests::trace -- --ignored --nocapture` prints the
//! capsule's pose and the marker's edges for every frame of a show, a burst and a hide.

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};

use fractile::{
    Rect, WorkspaceId,
    app::{UiHudDisplay, UiHudTile, UiHudWorkspace},
    config::{HudPosition, HudStyle},
};
use rune::{
    Env, Runtime,
    motion_gate::{Run, Snap},
};

use crate::rules::{Doc, Place};

use super::{
    HOLD_MS, HudOverlay, HudSink, HudTick, LEAVE_MS, NoopHudSink, OVERLAY_SIZE, RESUME_HOLD_MS,
    grow, menu, overlay_view, pill,
};

const HZ: f64 = 120.0;
/// The most a spring here moves opacity in one 120 Hz frame (ENTER 0.10 s peaks near 0.19/frame);
/// a pop is several times this.
const OPACITY_STEP: f32 = 0.20;
/// One 8-bit step of alpha: below it nothing is drawn.
const UNSEEN: f32 = 1.0 / 255.0;
/// The furthest the marker's edges go in one frame, crossing the whole demo row (Icons' 3 to 1
/// with MARKER_LEAD 0.13 s peaks near 19 pt/frame); a snap would be several times this.
const MARKER_STEP: f32 = 22.0;
const STYLES: [HudStyle; 2] = [HudStyle::Icons, HudStyle::Layouts];

struct Hud {
    run: Run<HudOverlay>,
    frame: u64,
    clock: Arc<AtomicU64>,
}

impl Hud {
    /// Up and at rest on `row`, as the panel's first frame draws it.
    fn showing(style: HudStyle, row: UiHudDisplay) -> Self {
        Self::start(style, row, false)
    }

    /// Up and at rest on `row`, with the capsule on `position`'s display edge.
    fn showing_at(style: HudStyle, position: HudPosition, row: UiHudDisplay) -> Self {
        Self::with_sink_at(style, position, row, false, Arc::new(NoopHudSink))
    }

    fn start(style: HudStyle, row: UiHudDisplay, reduce_motion: bool) -> Self {
        Self::with_sink(style, row, reduce_motion, Arc::new(NoopHudSink))
    }

    fn with_sink(
        style: HudStyle,
        row: UiHudDisplay,
        reduce_motion: bool,
        sink: Arc<dyn HudSink>,
    ) -> Self {
        Self::with_sink_at(style, HudPosition::Bottom, row, reduce_motion, sink)
    }

    fn with_sink_at(
        style: HudStyle,
        position: HudPosition,
        row: UiHudDisplay,
        reduce_motion: bool,
        sink: Arc<dyn HudSink>,
    ) -> Self {
        let clock = Arc::new(AtomicU64::new(0));
        let mut overlay = HudOverlay::new(true, style, position, sink);
        overlay.reduce_motion = reduce_motion;
        overlay.virtual_clock = Some(Arc::clone(&clock));
        overlay.flow.trigger(row, 0);
        let rt = Runtime::new(overlay, overlay_view, OVERLAY_SIZE, Env::new(true, 2.0));
        let mut run = Run::new(rt, HZ);
        run.step();
        Self {
            run,
            frame: 0,
            clock,
        }
    }

    fn now_ms(&self) -> u64 {
        (self.frame as f64 * 1_000.0 / HZ).round() as u64
    }

    /// One frame: the flow's deadlines first, as the overlay's poll runs them.
    fn step(&mut self) -> HudTick {
        self.frame += 1;
        let now_ms = self.now_ms();
        self.clock.store(now_ms, Ordering::Relaxed);
        let mut tick = HudTick::Idle;
        self.run.rt.update_if(|overlay, _| {
            tick = overlay.flow.tick(now_ms);
            matches!(tick, HudTick::Arrive | HudTick::Leave)
        });
        self.run.step();
        tick
    }

    fn rect_of(&self, path: &str) -> [f32; 4] {
        self.snaps(path, 0).last().unwrap().rect
    }

    fn center_of(&self, path: &str) -> (f32, f32) {
        let [x, y, w, h] = self.rect_of(path);
        (x + w / 2.0, y + h / 2.0)
    }

    fn play(&mut self, ms: u64) {
        let until = self.now_ms() + ms;
        while self.now_ms() < until {
            self.step();
        }
    }

    /// Plays until the flow reports `tick`, and returns the recording's frame it landed on.
    fn play_until(&mut self, tick: HudTick) -> usize {
        for _ in 0..(HZ as u64 * 5) {
            if self.step() == tick {
                return self.run.rec.frames - 1;
            }
        }
        panic!("the flow never reached {tick:?}");
    }

    /// A row landing on the next frame; returns that frame.
    fn switch(&mut self, row: UiHudDisplay) -> usize {
        let now_ms = self.now_ms();
        self.run.rec.input("switch");
        self.run.rt.update(move |overlay, _| {
            overlay.flow.trigger(row, now_ms);
        });
        self.step();
        self.run.rec.frames - 1
    }

    fn try_snaps(&self, path: &str, from: usize) -> Option<Vec<Snap>> {
        let suffix = format!("/{path}");
        let track = self
            .run
            .rec
            .tracks
            .iter()
            .rfind(|track| track.path == path || track.path.ends_with(&suffix))?;
        Some(
            track
                .frames
                .iter()
                .filter(|(frame, _)| *frame >= from)
                .map(|(_, snap)| *snap)
                .collect(),
        )
    }

    /// The presented state of the element whose path ends in `path`, from frame `from` on.
    fn snaps(&self, path: &str, from: usize) -> Vec<Snap> {
        self.try_snaps(path, from)
            .unwrap_or_else(|| panic!("nothing drawn at {path}"))
    }

    /// The marker's left and right edges from frame `from` on, measured inside the capsule
    /// and scaled to its resting size, so the capsule's own entrance doesn't move them.
    fn marker(&self, from: usize) -> Vec<(f32, f32)> {
        let pill = self.snaps("hud-pill", from);
        let rest = pill[pill.len() - 1].rect[2];
        self.snaps("hud-marker", from)
            .iter()
            .zip(&pill)
            .map(|(marker, pill)| {
                let k = rest / pill.rect[2];
                let left = (marker.rect[0] - pill.rect[0]) * k;
                (left, left + marker.rect[2] * k)
            })
            .collect()
    }

    fn findings(&self) -> Vec<String> {
        self.run.finish().iter().map(ToString::to_string).collect()
    }
}

fn id(number: u8) -> WorkspaceId {
    WorkspaceId::new(number).unwrap()
}

fn workspace(number: u8, apps: &[&str]) -> UiHudWorkspace {
    let count = apps.len().max(1) as i32;
    UiHudWorkspace {
        workspace: id(number),
        fullscreen: false,
        tiles: apps
            .iter()
            .zip(0..)
            .map(|(bundle, index)| UiHudTile {
                key: u64::from(number) * 100 + index as u64,
                rect: Rect {
                    x: index * 1500 / count,
                    y: 0,
                    width: 1500 / count,
                    height: 1100,
                },
                bundle: Some((*bundle).to_owned()),
            })
            .collect(),
        floating: Vec::new(),
    }
}

fn display(
    id_: &str,
    visible: u8,
    from: Option<u8>,
    workspaces: Vec<UiHudWorkspace>,
) -> UiHudDisplay {
    UiHudDisplay {
        id: id_.to_owned(),
        visible: Some(id(visible)),
        from: from.map(id),
        workspaces,
        order: Vec::new(),
        portrait: false,
    }
}

/// Workspaces 1 to 3 occupied, plus `visible` when it's empty; arrived at from `from`.
fn row(visible: u8, from: Option<u8>) -> UiHudDisplay {
    let mut workspaces = vec![
        workspace(1, &["com.test.term", "com.test.web", "com.test.notes"]),
        workspace(2, &["com.test.chat"]),
        workspace(3, &["com.test.mail", "com.test.web"]),
    ];
    if visible > 3 {
        workspaces.push(workspace(visible, &[]));
    }
    display("main", visible, from, workspaces)
}

/// Every step is small and in one direction; `rising` says which.
fn assert_one_way(values: &[f32], rising: bool, step: f32, what: &str) {
    for pair in values.windows(2) {
        let delta = if rising {
            pair[1] - pair[0]
        } else {
            pair[0] - pair[1]
        };
        assert!(
            (-0.01..=step).contains(&delta),
            "{what}: {} then {} in one frame",
            pair[0],
            pair[1]
        );
    }
}

fn opacity(snaps: &[Snap]) -> Vec<f32> {
    snaps.iter().map(|snap| snap.op).collect()
}

fn edges(marker: &[(f32, f32)]) -> (Vec<f32>, Vec<f32>) {
    marker.iter().copied().unzip()
}

#[test]
fn a_burst_of_switches_a_hide_and_a_show_pass_the_motion_gate() {
    for style in STYLES {
        let mut hud = Hud::showing(style, row(1, None));
        hud.play(300);
        // 1, 2, 3, then back to 1: each lands while the last is still moving.
        hud.switch(row(2, Some(1)));
        hud.play(160);
        hud.switch(row(3, Some(2)));
        hud.play(160);
        hud.switch(row(1, Some(3)));
        hud.play_until(HudTick::Gone);
        hud.play(100);
        // Shown again on an empty workspace, which joins the row as the marker heads for it.
        hud.switch(row(4, Some(1)));
        hud.play(700);
        // Pops, jumps, cut fades, overshoot, flicker, motion that never settles.
        assert_eq!(hud.findings(), Vec::<String>::new(), "{style:?}");
    }
}

#[test]
fn the_capsule_leaves_and_comes_back_along_one_path() {
    for style in STYLES {
        let mut hud = Hud::showing(style, row(1, None));
        let rest = hud.snaps("hud-slot", 0)[0];
        assert_eq!(
            (rest.op, rest.scale),
            (1.0, 1.0),
            "the first frame is at rest"
        );

        let left = hud.play_until(HudTick::Leave);
        let gone = hud.play_until(HudTick::Gone);
        let exit = hud.snaps("hud-slot", left - 1);
        let exit = &exit[..=gone - left];
        assert_one_way(&opacity(exit), false, OPACITY_STEP, "exit opacity");
        assert_one_way(
            &exit.iter().map(|snap| snap.rect[1]).collect::<Vec<_>>(),
            true,
            1.5,
            "exit drop",
        );
        let last = exit[exit.len() - 1];
        assert!(
            last.op < UNSEEN,
            "ordering the panel out after {LEAVE_MS} ms cuts a capsule still at {}",
            last.op
        );
        assert!(
            (last.scale - 0.96).abs() < 0.001,
            "it shrank to {}",
            last.scale
        );

        // Hidden, then another workspace: it enters from the pose it left in.
        hud.play(400);
        let shown = hud.switch(row(3, Some(1)));
        hud.play(LEAVE_MS + 300);
        let enter = hud.snaps("hud-slot", shown);
        assert!(enter[0].op < 0.1, "it popped in at {}", enter[0].op);
        assert_one_way(&opacity(&enter), true, OPACITY_STEP, "enter opacity");
        assert_one_way(
            &enter.iter().map(|snap| snap.rect[1]).collect::<Vec<_>>(),
            false,
            1.5,
            "enter rise",
        );
        let settled = enter[enter.len() - 1];
        assert_eq!(
            (settled.op, settled.scale, settled.rect),
            (1.0, 1.0, rest.rect)
        );
        let in_200_ms = enter[(0.2 * HZ) as usize];
        assert!(in_200_ms.op > 0.97, "{} after 200 ms", in_200_ms.op);
        assert_eq!(hud.findings(), Vec::<String>::new(), "{style:?}");
    }
}

#[test]
fn a_top_capsule_leaves_upward_and_drops_back_into_place() {
    for style in STYLES {
        let mut hud = Hud::showing_at(style, HudPosition::Top, row(1, None));
        let rest = hud.snaps("hud-slot", 0)[0];
        assert_eq!(
            (rest.op, rest.scale),
            (1.0, 1.0),
            "the first frame is at rest"
        );

        let left = hud.play_until(HudTick::Leave);
        let gone = hud.play_until(HudTick::Gone);
        let exit = hud.snaps("hud-slot", left - 1);
        let exit = &exit[..=gone - left];
        // Away from the top edge, the capsule's y only falls as it fades and shrinks upward.
        assert_one_way(
            &exit.iter().map(|snap| snap.rect[1]).collect::<Vec<_>>(),
            false,
            1.5,
            "exit lift",
        );

        hud.play(400);
        let shown = hud.switch(row(3, Some(1)));
        hud.play(LEAVE_MS + 300);
        let enter = hud.snaps("hud-slot", shown);
        // Back from above: it starts higher and only descends into place.
        assert!(
            enter[0].rect[1] < rest.rect[1],
            "it entered at {}, above the rest pose {}",
            enter[0].rect[1],
            rest.rect[1]
        );
        assert_one_way(
            &enter.iter().map(|snap| snap.rect[1]).collect::<Vec<_>>(),
            true,
            1.5,
            "enter drop",
        );
        assert_eq!(enter[enter.len() - 1].rect, rest.rect);
        assert_eq!(hud.findings(), Vec::<String>::new(), "{style:?}");
    }
}

#[test]
fn a_switch_during_the_exit_turns_the_capsule_around_where_it_is() {
    for style in STYLES {
        let mut hud = Hud::showing(style, row(1, None));
        let left = hud.play_until(HudTick::Leave);
        hud.play(35);
        let before = *hud.snaps("hud-slot", left).last().unwrap();
        assert!(
            (0.2..0.8).contains(&before.op),
            "{style:?}: mid-exit, at {}",
            before.op
        );

        let back = hud.switch(row(2, Some(1)));
        hud.play(400);
        let after = hud.snaps("hud-slot", back);
        // It carries on from where it was: no restart from nothing, no snap to full.
        assert!((after[0].op - before.op).abs() < OPACITY_STEP);
        assert!((after[0].scale - before.scale).abs() < 0.01);
        assert!((after[0].rect[1] - before.rect[1]).abs() < 1.5);
        let lowest = opacity(&after).into_iter().fold(1.0, f32::min);
        assert!(
            lowest > before.op - OPACITY_STEP,
            "{style:?}: it dipped to {lowest}"
        );
        for pair in after.windows(2) {
            assert!((pair[1].op - pair[0].op).abs() <= OPACITY_STEP);
        }
        assert_eq!(after[after.len() - 1].op, 1.0);
        // The gate reads a capsule that dims and comes back as a flicker; here a switch asked for it.
        let findings = hud.findings();
        assert!(
            findings
                .iter()
                .all(|finding| finding.starts_with("flicker")),
            "{style:?}: {findings:?}"
        );
    }
}

#[test]
fn the_marker_goes_one_way_per_switch_its_leading_edge_first() {
    for style in STYLES {
        let mut hud = Hud::showing(style, row(1, None));
        hud.play(300);
        for (to, from, rising) in [(3, 1, true), (1, 3, false)] {
            let switched = hud.switch(row(to, Some(from)));
            hud.play(700);
            let marker = hud.marker(switched - 1);
            let (left, right) = edges(&marker);
            assert_one_way(&left, rising, MARKER_STEP, &format!("{style:?} left edge"));
            assert_one_way(
                &right,
                rising,
                MARKER_STEP,
                &format!("{style:?} right edge"),
            );
            let width = |(left, right): (f32, f32)| right - left;
            let (start, end) = (marker[0], marker[marker.len() - 1]);
            // It stretches toward where it's going, then the trailing edge catches up.
            let widest = marker.iter().copied().map(width).fold(0.0, f32::max);
            assert!(
                widest > width(start).max(width(end)) + 4.0,
                "{style:?}: it never stretched ({widest})"
            );
            let lead = |frame: usize| {
                let (left, right) = marker[frame];
                if rising {
                    right - start.1
                } else {
                    start.0 - left
                }
            };
            let trail = |frame: usize| {
                let (left, right) = marker[frame];
                if rising {
                    left - start.0
                } else {
                    start.1 - right
                }
            };
            assert!(lead(8) > trail(8), "{style:?}: the trailing edge led");
        }
        assert_eq!(hud.findings(), Vec::<String>::new(), "{style:?}");
    }
}

#[test]
fn a_switch_mid_flight_carries_the_marker_on_without_a_jump() {
    for style in STYLES {
        let mut hud = Hud::showing(style, row(1, None));
        hud.play(300);
        let first = hud.switch(row(2, Some(1)));
        hud.play(80);
        let second = hud.switch(row(3, Some(2)));
        hud.play(700);
        let marker = hud.marker(first - 1);
        let (left, right) = edges(&marker);
        // One way the whole time, never back toward 2 and never from rest again.
        assert_one_way(&left, true, MARKER_STEP, &format!("{style:?} left edge"));
        assert_one_way(&right, true, MARKER_STEP, &format!("{style:?} right edge"));
        let at = second - (first - 1);
        let speed = |edge: &[f32], frame: usize| edge[frame] - edge[frame - 1];
        for edge in [&left, &right] {
            assert!(
                speed(edge, at) >= speed(edge, at - 1) * 0.9,
                "{style:?}: the second switch slowed it from {} to {}",
                speed(edge, at - 1),
                speed(edge, at)
            );
        }
        assert_eq!(hud.findings(), Vec::<String>::new(), "{style:?}");
    }
}

#[test]
fn shown_again_the_marker_starts_where_the_switch_left() {
    for style in STYLES {
        let mut hud = Hud::showing(style, row(2, None));
        hud.play(300);
        let on_two = hud.marker(0)[0];
        hud.switch(row(1, Some(2)));
        hud.play(400);
        let on_one = *hud.marker(0).last().unwrap();
        hud.play_until(HudTick::Gone);
        hud.play(300);

        // Hidden on 1. A switch from 2 to 3 (made while it was hidden, say on another display's
        // shortcut) shows it with the marker on 2, which then travels to 3.
        let shown = hud.switch(row(3, Some(2)));
        hud.play(800);
        let marker = hud.marker(shown);
        let close =
            |a: (f32, f32), b: (f32, f32)| (a.0 - b.0).abs() < 0.5 && (a.1 - b.1).abs() < 0.5;
        assert!(
            close(marker[0], on_two),
            "{style:?}: started at {:?}, not {on_two:?}",
            marker[0]
        );
        assert!(!close(marker[0], on_one));
        let (left, right) = edges(&marker);
        assert_one_way(&left, true, MARKER_STEP, &format!("{style:?} left edge"));
        assert_one_way(&right, true, MARKER_STEP, &format!("{style:?} right edge"));
        assert!(
            right[right.len() - 1] > on_two.1 + 10.0,
            "{style:?}: it never left 2"
        );
        if style == HudStyle::Layouts {
            // The large cell starts on 2 as well.
            let two = hud.snaps("ws2", shown);
            let three = hud.snaps("ws3", shown);
            assert!(
                two[0].rect[2] > three[0].rect[2],
                "{style:?}: 3 was already large"
            );
            assert!(two[two.len() - 1].rect[2] < three[three.len() - 1].rect[2]);
        }

        // With nowhere to come from, it shows at rest.
        hud.play_until(HudTick::Gone);
        hud.play(300);
        let shown = hud.switch(row(1, None));
        hud.play(500);
        let marker = hud.marker(shown);
        assert!(
            marker.iter().all(|edges| close(*edges, marker[0])),
            "{style:?}: it travelled"
        );
        assert_eq!(hud.findings(), Vec::<String>::new(), "{style:?}");
    }
}

#[test]
fn reduce_motion_fades_and_nothing_travels() {
    for style in STYLES {
        let mut hud = Hud::start(style, row(2, None), true);
        hud.play(300);
        let rest = hud.snaps("hud-slot", 0)[0];

        let switched = hud.switch(row(3, Some(2)));
        hud.play(400);
        let mut faded = false;
        for track in hud
            .run
            .rec
            .tracks
            .iter()
            .filter(|track| track.path.contains("/hud-row"))
        {
            let (_, first) = track.frames[0];
            for (_, snap) in &track.frames {
                assert_eq!(
                    (snap.rect, snap.scale),
                    (first.rect, first.scale),
                    "{style:?}: {} moved",
                    track.path
                );
                faded |= track.path.ends_with("hud-marker-3") && (0.2..0.8).contains(&snap.op);
            }
        }
        assert!(faded, "{style:?}: the marker should fade in on 3");
        assert!(hud.try_snaps("hud-marker-3", switched).is_some());

        let left = hud.play_until(HudTick::Leave);
        let gone = hud.play_until(HudTick::Gone);
        let exit = hud.snaps("hud-slot", left);
        let exit = &exit[..=gone - left];
        for snap in exit {
            assert_eq!(
                (snap.scale, snap.rect),
                (1.0, rest.rect),
                "no scale, no drop"
            );
        }
        assert!(
            exit.iter().any(|snap| (0.2..0.8).contains(&snap.op)),
            "a fade, not a cut"
        );
        assert_one_way(&opacity(exit), false, OPACITY_STEP, "reduced exit");
        assert!(exit[exit.len() - 1].op < UNSEEN);
        hud.play(600);
        assert_eq!(hud.findings(), Vec::<String>::new(), "{style:?}");
    }
}

#[test]
fn each_display_draws_its_own_row_and_only_the_switching_one_moves() {
    let side = |from| {
        display(
            "side",
            6,
            from,
            vec![
                workspace(6, &["com.test.chat"]),
                workspace(7, &["com.test.web"]),
            ],
        )
    };
    for style in STYLES {
        let mut main = Hud::showing(style, row(1, None));
        let mut other = Hud::showing(style, side(None));
        main.play(300);
        other.play(300);
        let start = other.marker(0)[0];

        // A switch on the main display: both capsules get their own rows on the same frame.
        let switched = main.switch(row(2, Some(1)));
        assert_eq!(other.switch(side(None)), switched);
        main.play(600);
        other.play(600);
        let moved = main.marker(switched);
        assert!(
            moved[moved.len() - 1].0 > moved[0].0 + 10.0,
            "{style:?}: main's marker stayed"
        );
        for (frame, edges) in other.marker(0).iter().enumerate() {
            assert_eq!(
                *edges, start,
                "{style:?}: the other display's marker moved on {frame}"
            );
        }
        for path in ["ws6", "ws7"] {
            let cell = other.snaps(path, 0);
            assert!(
                cell.iter()
                    .all(|snap| snap.rect == cell[0].rect && snap.op == cell[0].op)
            );
        }
        // They hold and leave together.
        assert_eq!(
            main.play_until(HudTick::Leave),
            other.play_until(HudTick::Leave)
        );
        main.play_until(HudTick::Gone);
        other.play_until(HudTick::Gone);
        main.play(550);
        other.play(550);
        assert_eq!(main.findings(), Vec::<String>::new(), "{style:?}");
        assert_eq!(other.findings(), Vec::<String>::new(), "{style:?}");
    }
}

#[test]
fn a_layout_change_moves_the_tiles_it_has() {
    let mut hud = Hud::showing(
        HudStyle::Layouts,
        display("main", 1, None, vec![workspace(1, &["a", "b"])]),
    );
    hud.play(300);
    let before = hud.snaps("ws1/t101", 0)[0].rect;

    let mut rows = workspace(1, &["a", "b"]);
    for (tile, y) in rows.tiles.iter_mut().zip([0, 550]) {
        tile.rect = Rect {
            x: 0,
            y,
            width: 1500,
            height: 550,
        };
    }
    let changed = hud.switch(display("main", 1, None, vec![rows]));
    hud.play(600);
    // The same tile, from where it was to where it goes, with no slide of the whole set.
    let tile = hud.snaps("ws1/t101", changed - 1);
    assert_eq!(tile[0].rect, before);
    for pair in tile.windows(2) {
        for edge in 0..4 {
            assert!((pair[1].rect[edge] - pair[0].rect[edge]).abs() < 3.5);
        }
        assert_eq!(pair[1].op, 1.0);
    }
    let after = tile[tile.len() - 1].rect;
    assert!(after[0] < before[0] - 5.0 && after[1] > before[1] + 5.0);
    assert_eq!(hud.findings(), Vec::<String>::new());
}

#[test]
fn a_moved_window_s_icon_leaves_one_workspace_for_the_other() {
    let before = vec![
        workspace(1, &["com.test.a", "com.test.b"]),
        workspace(2, &["com.test.c"]),
    ];
    let after = vec![
        workspace(1, &["com.test.a"]),
        workspace(2, &["com.test.c", "com.test.b"]),
    ];
    let mut hud = Hud::showing(HudStyle::Icons, display("main", 1, None, before));
    hud.play(300);
    let two = hud.snaps("ws2", 0)[0].rect;

    let moved = hud.switch(display("main", 1, None, after));
    hud.play(600);
    let leaving = hud.snaps("ws1/apps/app-com.test.b", moved);
    assert!(leaving[leaving.len() - 1].op < 0.05, "it left workspace 1");
    let arriving = hud.snaps("ws2/apps/app-com.test.b", moved);
    assert!(
        arriving[0].op < 0.2,
        "it popped into workspace 2 at {}",
        arriving[0].op
    );
    assert_one_way(&opacity(&arriving), true, OPACITY_STEP, "icon fading in");
    // Workspace 2 slides over into the room workspace 1 gave up.
    let slide = hud.snaps("ws2", moved);
    assert!(slide[slide.len() - 1].rect[0] < two[0] - 10.0);
    assert_one_way(
        &slide.iter().map(|snap| snap.rect[0]).collect::<Vec<_>>(),
        false,
        4.0,
        "slide",
    );
    // The marker stays with you on 1.
    let marker = hud.snaps("hud-marker", moved);
    let one = hud.snaps("ws1", moved);
    let (m, g) = (marker[marker.len() - 1].rect, one[one.len() - 1].rect);
    assert!(
        m[0] < g[0] && m[0] + m[2] > g[0] + g[2],
        "{m:?} around {g:?}"
    );
    assert_eq!(hud.findings(), Vec::<String>::new());
}

#[test]
fn the_widest_capsule_and_its_shadow_fit_the_panel() {
    let apps: Vec<String> = (0..16)
        .map(|index| format!("com.test.app{index}"))
        .collect();
    let apps: Vec<&str> = apps.iter().map(String::as_str).collect();
    for style in STYLES {
        for visible in [1, 5, 10] {
            let workspaces = (1..=10).map(|number| workspace(number, &apps)).collect();
            let hud = Hud::showing(style, display("main", visible, None, workspaces));
            let [x, y, width, height] = hud.snaps("hud-pill", 0)[0].rect;
            // The shadow's widest layer blurs 32 pt; its lowest reaches `SHADOW_REACH` below.
            assert!(
                x >= pill::SHADOW_SIDE && x + width <= OVERLAY_SIZE.0 - pill::SHADOW_SIDE,
                "{x} + {width}"
            );
            assert!(y + height + pill::SHADOW_REACH <= OVERLAY_SIZE.1);
            // A row that overflows still keeps your workspace in view.
            let marker = hud.snaps("hud-marker", 0)[0].rect;
            assert!(
                marker[0] >= x && marker[0] + marker[2] <= x + width,
                "{style:?} on {visible}: the marker at {marker:?} is outside {x} + {width}"
            );
        }
    }
    // Ten workspaces of layouts fit without scrolling.
    let ten: Vec<_> = (1..=10).map(|number| workspace(number, &["a"])).collect();
    let (_, width) = grow::rest(&ten, Some(id(1)), false);
    assert!(width <= pill::max_row_width(OVERLAY_SIZE.0));
}

#[derive(Default)]
struct RecordingSink {
    switched: Mutex<Vec<WorkspaceId>>,
    doc: Mutex<Option<Doc>>,
}

impl RecordingSink {
    fn new() -> Self {
        Self {
            switched: Mutex::new(Vec::new()),
            doc: Mutex::new(Some(Doc::parse("version = 1\n"))),
        }
    }

    fn doc_place(&self, bundle: &str) -> Option<Place> {
        self.doc
            .lock()
            .unwrap()
            .as_ref()
            .and_then(|doc| doc.app_place(bundle))
    }
}

impl HudSink for RecordingSink {
    fn switch_workspace(&self, workspace: WorkspaceId) {
        self.switched.lock().unwrap().push(workspace);
    }

    fn app_place(&self, bundle_id: &str) -> Option<Place> {
        self.doc_place(bundle_id)
    }

    fn set_app_place(&self, bundle_id: &str, place: Option<Place>) -> Result<(), String> {
        if let Some(doc) = self.doc.lock().unwrap().as_mut() {
            doc.set_app_place(bundle_id, place);
        }
        Ok(())
    }
}

#[test]
fn hovering_the_pill_holds_it_open_and_leaving_resumes_a_clean_exit() {
    for style in STYLES {
        let mut hud = Hud::showing(style, row(1, None));
        hud.play(100);
        let (px, py) = hud.center_of("hud-pill");
        hud.run.rt.pointer_move(px, py);
        // Hold well past HOLD_MS while hovering: stays at full opacity.
        hud.play(HOLD_MS + 500);
        let held = *hud.snaps("hud-slot", 0).last().unwrap();
        assert_eq!(held.op, 1.0, "{style:?}: hovered pill should not fade out");

        // Move pointer off the capsule: auto-dismiss resumes and exits cleanly.
        hud.run.rt.pointer_move(2.0, 2.0);
        let left_at = hud.now_ms();
        let left_frame = hud.play_until(HudTick::Leave);
        assert!(
            hud.now_ms() >= left_at + RESUME_HOLD_MS - 20,
            "{style:?}: left too early"
        );
        hud.play_until(HudTick::Gone);
        let exit = hud.snaps("hud-slot", left_frame);
        assert!(exit.last().unwrap().op < UNSEEN, "{style:?}: cut exit");
        hud.play(550);
        assert_eq!(hud.findings(), Vec::<String>::new(), "{style:?}");
    }
}

#[test]
fn clicking_a_workspace_slot_switches_it() {
    for style in STYLES {
        let sink = Arc::new(RecordingSink::new());
        let mut hud = Hud::with_sink(style, row(1, None), false, sink.clone());
        hud.play(200);

        let (wx, wy) = hud.center_of("ws3");
        hud.run.rt.pointer_move(wx, wy);
        hud.step();
        hud.run.rt.pointer_down(wx, wy);
        hud.run.rt.pointer_up(wx, wy);
        hud.step();
        assert_eq!(*sink.switched.lock().unwrap(), vec![id(3)], "{style:?}");
        assert!(
            hud.try_snaps("hud-settings", 0).is_none(),
            "{style:?}: HUD capsule should not render a trailing Settings gear"
        );
        hud.play(200);
        assert_eq!(hud.findings(), Vec::<String>::new(), "{style:?}");
    }
}

#[test]
fn secondary_clicking_an_app_icon_opens_its_rule_menu() {
    let sink = Arc::new(RecordingSink::new());
    let mut hud = Hud::with_sink(HudStyle::Icons, row(1, None), false, sink.clone());
    hud.play(200);

    let (ax, ay) = hud.center_of("ws1/apps/app-com.test.web");
    hud.run.rt.pointer_move(ax, ay);
    hud.step();
    hud.run.rt.secondary_down(ax, ay);
    hud.run.rt.secondary_up(ax, ay);
    hud.step();

    // Opening the popup holds the HUD up even when the pointer moves onto the menu outside the pill.
    hud.run.rt.pointer_move(2.0, 2.0);
    hud.play(HOLD_MS + 300);
    assert_eq!(hud.snaps("hud-slot", 0).last().unwrap().op, 1.0);

    let popup = hud.run.rt.popup_menu().expect("popup should be queued");
    assert_eq!(
        popup.find(menu::ID_TOGGLE_FLOAT).unwrap().title(),
        "Float Web"
    );
    assert_eq!(
        popup.find(menu::ID_TOGGLE_PIN).unwrap().title(),
        "Pin Web on all workspaces"
    );
    assert_eq!(
        popup.find(menu::ID_TOGGLE_ROUTE).unwrap().title(),
        "Always open Web on Workspace 1"
    );

    // Pick Float -> writes float rule through Doc, then HUD resumes dismiss.
    hud.run
        .rt
        .resolve_popup(Some(menu::ID_TOGGLE_FLOAT))
        .unwrap();
    hud.step();
    assert_eq!(sink.doc_place("com.test.web"), Some(Place::Float));
    hud.play_until(HudTick::Leave);
    hud.play_until(HudTick::Gone);
}

#[test]
fn rapid_80ms_switches_across_empty_workspaces_keep_motion_smooth() {
    for style in STYLES {
        let mut hud = Hud::showing(style, row(1, None));
        hud.play(80);
        // Rapid 80 ms keypresses across occupied & empty workspaces (4, 5, 6, then back to 2).
        for (to, from) in [(4, 1), (5, 4), (6, 5), (2, 6), (5, 2)] {
            hud.switch(row(to, Some(from)));
            hud.play(80);
        }
        // Once empty workspaces 4, 5, 6 joined the row, flicking back to 2 and 5 never shrinks
        // the pill underneath the moving marker.
        let back_to_2 = hud.switch(row(2, Some(5)));
        let widths: Vec<f32> = hud
            .snaps("hud-pill", back_to_2)
            .iter()
            .map(|snap| snap.rect[2])
            .collect();
        let min_w = widths.iter().copied().fold(f32::INFINITY, f32::min);
        let max_w = widths.iter().copied().fold(0.0, f32::max);
        assert!(
            max_w - min_w < 2.0,
            "{style:?}: pill width wobbled during rapid empty switches ({min_w}..{max_w})"
        );
        hud.play(400);
        assert_eq!(hud.findings(), Vec::<String>::new(), "{style:?}");
    }
}

#[test]
#[ignore = "prints the capsule's pose per frame for tuning; asserts nothing"]
fn trace() {
    let mut hud = Hud::showing(HudStyle::default(), row(1, None));
    hud.play(100);
    hud.switch(row(2, Some(1)));
    hud.play(160);
    hud.switch(row(3, Some(2)));
    hud.play_until(HudTick::Gone);
    hud.switch(row(1, Some(3)));
    hud.play(300);
    println!("frame,ms,opacity,scale,y,capsule_x,capsule_width,marker_left,marker_right");
    let (slot, capsule, marker) = (
        hud.snaps("hud-slot", 0),
        hud.snaps("hud-pill", 0),
        hud.marker(0),
    );
    for (frame, ((slot, capsule), (left, right))) in
        slot.iter().zip(&capsule).zip(&marker).enumerate()
    {
        println!(
            "{frame},{:.0},{:.3},{:.3},{:.2},{:.2},{:.2},{left:.2},{right:.2}",
            frame as f64 * 1_000.0 / HZ,
            slot.op,
            slot.scale,
            slot.rect[1],
            capsule.rect[0],
            capsule.rect[2]
        );
    }
    println!("hold {HOLD_MS} ms, leave {LEAVE_MS} ms");
}
