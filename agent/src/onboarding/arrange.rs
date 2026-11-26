//! The Arrange step: the plan for the open windows as miniature workspaces, on the displays as
//! they sit on the desk (as Settings › Screens draws them). Each card is a workspace with the
//! apps' real icons in the frames Dwindle would give them; a click sends a
//! tile to the next workspace, a drag to any other, and the Floating line lists what stays out
//! of the tiles. Everything sits at computed positions, so a drag knows where the pointer is.

use fractile::{
    Rect, WorkspaceId,
    onboarding_plan::{self, Inventory, Lane, OpenApp, OpenWindow, Plan, Screen},
};
use rune::{Cursor, El, Spring, Tx, el, row, text};

use crate::{
    Ui, apps,
    controls::{self, Paint},
    panes,
};

/// The step's width: the desk scales to fit it.
pub const WIDTH: f32 = 640.0;
const CARD_GAP: f32 = 12.0;
const CARD_MAX_W: f32 = 184.0;
/// A card's "1 · ⌥1" line, and the space under it.
const HEAD_H: f32 = 20.0;
const HEAD_GAP: f32 = 6.0;
const ROW_GAP: f32 = 20.0;
/// Inside a display: the room round its cards, and the line naming it above them.
const SCREEN_PAD: f32 = 10.0;
const NAME_H: f32 = 22.0;
/// Between displays that touch on the desk, so each reads as its own.
const SEAM: f32 = 8.0;
/// The narrowest a card gets before a display stacks its cards in fewer columns.
const CARD_MIN_W: f32 = 96.0;
/// The tallest the desk draws at its true scale; a display grows past it only to fit its cards.
const DESK_MAX_H: f32 = 300.0;
/// A display's width over its height, as the plan's Dwindle frames assume.
const ASPECT: f32 = 1.6;
/// Between a miniature's edge and its tiles, and between tiles.
const INSET: f32 = 5.0;
const TILE_GAP: f32 = 4.0;
const ICON_MAX: f32 = 32.0;
const ICON_MIN: f32 = 12.0;
const STRIP_GAP: f32 = 16.0;
const STRIP_H: f32 = 32.0;
/// Pointer travel before a press becomes a drag.
const SLOP: f32 = 4.0;
const SETTLE: Spring = Spring::new(0.3, 0.0);

/// Where a dragged tile would land.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Target {
    Workspace(WorkspaceId),
    Float,
}

/// A tile being dragged: which one, where it started and how far the pointer has gone.
#[derive(Clone, Debug, PartialEq)]
pub struct TileDrag {
    from: WorkspaceId,
    index: usize,
    bundle_id: String,
    /// The tile's top-left on the board, and its size.
    origin: (f32, f32),
    size: (f32, f32),
    /// Where in the tile the press landed.
    grab: (f32, f32),
    delta: (f32, f32),
    moved: bool,
    target: Option<Target>,
}

/// The plan on screen, the inventory it was made from, and a drag under way.
#[derive(Clone, Debug)]
pub struct Board {
    /// `None` until the worker has read the open windows.
    inventory: Option<Inventory>,
    pub plan: Plan,
    pub drag: Option<TileDrag>,
}

impl Default for Board {
    fn default() -> Self {
        Self {
            inventory: None,
            plan: onboarding_plan::plan(&laptop(Vec::new())),
            drag: None,
        }
    }
}

fn numbered(numbers: std::ops::RangeInclusive<u8>) -> Vec<WorkspaceId> {
    WorkspaceId::all()
        .filter(|workspace| workspace.number().is_some_and(|n| numbers.contains(&n)))
        .collect()
}

fn laptop(apps: Vec<OpenApp>) -> Inventory {
    Inventory {
        apps,
        lanes: vec![Lane {
            name: "Built-in Retina Display".into(),
            frame: Rect {
                x: 0,
                y: 0,
                width: 1512,
                height: 945,
            },
            width: 1512,
            workspaces: numbered(1..=5),
        }],
    }
}

fn open(bundle_id: &str, windows: usize, (width, height): (i32, i32)) -> OpenApp {
    OpenApp {
        bundle_id: bundle_id.into(),
        category: None,
        windows: vec![
            OpenWindow {
                resizable: true,
                width,
                height,
            };
            windows
        ],
    }
}

impl Board {
    pub fn with(inventory: Inventory) -> Self {
        let mut board = Self::default();
        board.set_inventory(inventory);
        board
    }

    /// A laptop mid-afternoon: four browser windows, a terminal, mail and messages, notes,
    /// and two small utilities.
    pub fn fixture_laptop() -> Self {
        const WINDOW: (i32, i32) = (1100, 760);
        Self::with(laptop(vec![
            open("com.apple.Safari", 4, WINDOW),
            open("com.apple.Terminal", 1, WINDOW),
            open("com.apple.mail", 1, WINDOW),
            open("com.apple.MobileSMS", 1, WINDOW),
            open("com.apple.Notes", 1, WINDOW),
            open("com.apple.calculator", 1, (230, 408)),
            open("com.apple.Stickies", 2, (300, 220)),
        ]))
    }

    /// The same desk with a wide second display: Web and Code stay on the laptop.
    pub fn fixture_dual() -> Self {
        const WINDOW: (i32, i32) = (1100, 760);
        let mut inventory = laptop(vec![
            open("com.apple.Safari", 2, WINDOW),
            open("com.apple.Terminal", 2, WINDOW),
            open("com.apple.mail", 1, WINDOW),
            open("com.apple.MobileSMS", 1, WINDOW),
            open("com.apple.Notes", 1, WINDOW),
            open("com.apple.calculator", 1, (230, 408)),
        ]);
        inventory.lanes[0].workspaces = numbered(1..=4);
        inventory.lanes.push(Lane {
            name: "Studio Display".into(),
            frame: Rect {
                x: 1512,
                y: -300,
                width: 2560,
                height: 1415,
            },
            width: 2560,
            workspaces: numbered(5..=10),
        });
        Self::with(inventory)
    }

    /// A desk of three: a laptop on the left, a wide main display, a smaller one raised on the
    /// right.
    pub fn fixture_triple() -> Self {
        const WINDOW: (i32, i32) = (1100, 760);
        let display = |name: &str, (x, y, width, height), workspaces| Lane {
            name: String::from(name),
            frame: Rect {
                x,
                y,
                width,
                height,
            },
            width,
            workspaces,
        };
        Self::with(Inventory {
            apps: vec![
                open("org.mozilla.firefox", 1, WINDOW),
                open("com.apple.Terminal", 1, WINDOW),
                open("com.microsoft.VSCode", 2, WINDOW),
                open("com.tinyspeck.slackmacgap", 1, WINDOW),
                open("com.apple.Notes", 1, WINDOW),
                open("com.spotify.client", 1, WINDOW),
            ],
            lanes: vec![
                display("Studio Display", (0, 0, 3360, 1890), numbered(1..=4)),
                display(
                    "Built-in Retina Display",
                    (-1800, 358, 1800, 1169),
                    numbered(5..=7),
                ),
                display("LG UltraFine", (3360, -508, 2560, 1440), numbered(8..=10)),
            ],
        })
    }

    /// Where every display, card and the Floating line sit.
    pub fn geometry(&self) -> Geometry {
        Geometry::new(
            &self.plan,
            self.inventory
                .as_ref()
                .map_or(&[][..], |inventory| inventory.lanes.as_slice()),
        )
    }

    /// A Mac with one window open: nothing to arrange.
    pub fn fixture_single() -> Self {
        Self::with(laptop(vec![open("com.apple.Safari", 1, (1100, 760))]))
    }

    pub fn set_inventory(&mut self, inventory: Inventory) {
        self.plan = onboarding_plan::plan(&inventory);
        self.inventory = Some(inventory);
        self.drag = None;
    }

    pub fn inventory(&self) -> Option<&Inventory> {
        self.inventory.as_ref()
    }

    /// The windows were read and at most one would move.
    pub fn nothing_to_arrange(&self) -> bool {
        self.inventory.is_some() && self.plan.is_trivial()
    }

    /// Every app the board draws, for looking up names and icons.
    pub fn bundle_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self
            .plan
            .workspaces
            .iter()
            .flat_map(|workspace| workspace.tiles.iter().cloned())
            .chain(self.plan.floating.iter().cloned())
            .collect();
        ids.sort();
        ids.dedup();
        ids
    }

    /// A click on a tile: it moves to the next workspace shown.
    pub fn click(&mut self, from: WorkspaceId, index: usize) -> bool {
        self.plan
            .next_workspace(from)
            .is_some_and(|next| self.plan.move_tile(from, index, next))
    }

    /// Floats an app's windows, or tiles them again where its role starts.
    pub fn set_floating(&mut self, bundle_id: &str, floating: bool) {
        if let Some(inventory) = &self.inventory {
            self.plan.set_floating(bundle_id, floating, inventory);
        }
    }

    /// One phase of a tile's drag. A press that never travels is a click.
    #[allow(clippy::too_many_arguments)]
    pub fn drag_step(
        &mut self,
        phase: rune::Phase,
        start: (f32, f32),
        delta: (f32, f32),
        cancelled: bool,
        tile: &Tile,
        geometry: &Geometry,
    ) {
        let same = |drag: &TileDrag| drag.from == tile.workspace && drag.index == tile.index;
        match phase {
            rune::Phase::Down => {
                self.drag = Some(TileDrag {
                    from: tile.workspace,
                    index: tile.index,
                    bundle_id: tile.bundle_id.clone(),
                    origin: (tile.frame[0], tile.frame[1]),
                    size: (tile.frame[2], tile.frame[3]),
                    grab: start,
                    delta: (0.0, 0.0),
                    moved: false,
                    target: None,
                });
            }
            rune::Phase::Move => {
                let Some(drag) = self.drag.as_mut().filter(|drag| same(drag)) else {
                    return;
                };
                drag.delta = delta;
                drag.moved |= delta.0.abs().max(delta.1.abs()) >= SLOP;
                drag.target = geometry.target_at(
                    drag.origin.0 + drag.grab.0 + delta.0,
                    drag.origin.1 + drag.grab.1 + delta.1,
                );
            }
            rune::Phase::Up => {
                let Some(drag) = self.drag.take().filter(same) else {
                    return;
                };
                if cancelled {
                    return;
                }
                match (drag.moved, drag.target) {
                    (false, _) => {
                        self.click(drag.from, drag.index);
                    }
                    (true, Some(Target::Workspace(to))) => {
                        self.plan.move_tile(drag.from, drag.index, to);
                    }
                    (true, Some(Target::Float)) => self.set_floating(&drag.bundle_id, true),
                    (true, None) => {}
                }
            }
        }
    }

    /// Terminal held over workspace 3, for stills.
    pub fn fixture_drag(&mut self) {
        let geometry = self.geometry();
        let Some((card, planned)) =
            geometry
                .cards
                .iter()
                .zip(&self.plan.workspaces)
                .find(|(_, planned)| {
                    planned
                        .tiles
                        .iter()
                        .any(|tile| tile == "com.apple.Terminal")
                })
        else {
            return;
        };
        let Some(target) = geometry.cards.get(2) else {
            return;
        };
        let frame = geometry.tiles(card, planned.tiles.len())[0];
        let centre = (
            target.x + target.w / 2.0,
            target.y + HEAD_H + HEAD_GAP + target.mini_h / 2.0,
        );
        let grab = (frame[2] / 2.0, frame[3] / 2.0);
        self.drag = Some(TileDrag {
            from: card.workspace,
            index: 0,
            bundle_id: "com.apple.Terminal".into(),
            origin: (frame[0], frame[1]),
            size: (frame[2], frame[3]),
            grab,
            delta: (centre.0 - frame[0] - grab.0, centre.1 - frame[1] - grab.1),
            moved: true,
            target: geometry.target_at(centre.0, centre.1),
        });
    }
}

/// One drawn tile: which of a workspace's tiles it is, and where it sits on the board.
#[derive(Clone, Debug, PartialEq)]
pub struct Tile {
    pub workspace: WorkspaceId,
    pub index: usize,
    pub bundle_id: String,
    pub frame: [f32; 4],
}

#[derive(Clone, Debug, PartialEq)]
pub struct CardRect {
    pub workspace: WorkspaceId,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub mini_h: f32,
}

/// A display drawn on the board: its name and where it sits.
#[derive(Clone, Debug, PartialEq)]
pub struct ScreenRect {
    pub screen: Screen,
    pub name: String,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// Where every display, card and the Floating line sit, from the plan and the displays.
#[derive(Clone, Debug, PartialEq)]
pub struct Geometry {
    pub screens: Vec<ScreenRect>,
    pub cards: Vec<CardRect>,
    pub strip_y: f32,
    pub height: f32,
}

impl Geometry {
    /// Each display at its place on the desk, scaled to the board's width, holding its planned
    /// workspaces' cards in rows; a display grows taller to fit them, and pushes down one under
    /// it. `lanes` are the inventory's displays; without them (or their frames) the displays sit
    /// side by side.
    pub fn new(plan: &Plan, lanes: &[Lane]) -> Self {
        let mut screens: Vec<Screen> = Vec::new();
        for planned in &plan.workspaces {
            if !screens.contains(&planned.screen) {
                screens.push(planned.screen);
            }
        }
        if screens.is_empty() {
            screens.push(Screen::MAIN);
        }
        // Each display's frame on the desk, or side by side when it is not known.
        let mut next_x = 0;
        let frames: Vec<Rect> = screens
            .iter()
            .map(|screen| {
                let lane = lanes.get(screen.0);
                let frame = lane.map_or(Rect::default(), |lane| lane.frame);
                if frame.width > 0 && frame.height > 0 {
                    return frame;
                }
                let width = lane.map_or(1512, |lane| lane.width.max(1));
                let frame = Rect {
                    x: next_x,
                    y: 0,
                    width,
                    height: (width as f32 / ASPECT) as i32,
                };
                next_x += width;
                frame
            })
            .collect();
        let left = frames.iter().map(|f| f.x).min().unwrap_or(0);
        let top = frames.iter().map(|f| f.y).min().unwrap_or(0);
        let right = frames.iter().map(|f| f.x + f.width).max().unwrap_or(1);
        let bottom = frames.iter().map(|f| f.y + f.height).max().unwrap_or(1);
        // One display takes the whole width; a desk of several keeps their proportions.
        let across = WIDTH / (right - left).max(1) as f32;
        let scale = if frames.len() == 1 {
            across
        } else {
            across.min(DESK_MAX_H / (bottom - top).max(1) as f32)
        };
        let desk_w = (right - left) as f32 * scale;
        let shift = ((WIDTH - desk_w) / 2.0).max(0.0);
        let mut boxes: Vec<ScreenRect> = Vec::new();
        let mut cards = Vec::new();
        for (screen, frame) in screens.iter().zip(&frames) {
            let planned: Vec<&onboarding_plan::PlannedWorkspace> = plan
                .workspaces
                .iter()
                .filter(|planned| planned.screen == *screen)
                .collect();
            let x = shift + (frame.x - left) as f32 * scale;
            let w = frame.width as f32 * scale - SEAM;
            let inner = w - 2.0 * SCREEN_PAD;
            let count = planned.len().max(1);
            let cols = ((inner + CARD_GAP) / (CARD_MIN_W + CARD_GAP))
                .floor()
                .clamp(1.0, count as f32) as usize;
            let card_w = ((inner - CARD_GAP * (cols - 1) as f32) / cols as f32)
                .min(CARD_MAX_W)
                .floor();
            let mini_h = (card_w / ASPECT).round();
            let row_h = HEAD_H + HEAD_GAP + mini_h;
            let rows = count.div_ceil(cols);
            let needed =
                NAME_H + rows as f32 * row_h + (rows - 1) as f32 * ROW_GAP + 2.0 * SCREEN_PAD;
            let h = (frame.height as f32 * scale - SEAM).max(needed);
            let mut y = (frame.y - top) as f32 * scale;
            // A display already drawn where this one would go pushes it down.
            for other in &boxes {
                let beside = x + w <= other.x || other.x + other.w <= x;
                if !beside && y < other.y + other.h + SEAM && other.y < y + h {
                    y = other.y + other.h + SEAM;
                }
            }
            let line_w = cols.min(count) as f32 * card_w + (cols.min(count) - 1) as f32 * CARD_GAP;
            let first_x = x + SCREEN_PAD + ((inner - line_w) / 2.0).max(0.0);
            for (index, planned) in planned.iter().enumerate() {
                let (line, column) = (index / cols, index % cols);
                cards.push(CardRect {
                    workspace: planned.workspace,
                    x: (first_x + column as f32 * (card_w + CARD_GAP)).round(),
                    y: (y + SCREEN_PAD + NAME_H + line as f32 * (row_h + ROW_GAP)).round(),
                    w: card_w,
                    mini_h,
                });
            }
            let name = lanes
                .get(screen.0)
                .map(|lane| lane.name.clone())
                .filter(|name| !name.trim().is_empty())
                .unwrap_or_else(|| {
                    if *screen == Screen::MAIN {
                        "Main display".to_owned()
                    } else {
                        format!("Display {}", screen.0 + 1)
                    }
                });
            boxes.push(ScreenRect {
                screen: *screen,
                name,
                x,
                y,
                w,
                h,
            });
        }
        // Cards in the plan's order, so they zip with its workspaces.
        cards.sort_by_key(|card| {
            plan.workspaces
                .iter()
                .position(|planned| planned.workspace == card.workspace)
        });
        let strip_y = boxes.iter().map(|b| b.y + b.h).fold(0.0, f32::max) + STRIP_GAP;
        Self {
            screens: boxes,
            cards,
            strip_y,
            height: strip_y + STRIP_H,
        }
    }

    fn mini(card: &CardRect) -> [f32; 4] {
        [card.x, card.y + HEAD_H + HEAD_GAP, card.w, card.mini_h]
    }

    /// The board frames of a card's `count` tiles, in Dwindle's order.
    pub fn tiles(&self, card: &CardRect, count: usize) -> Vec<[f32; 4]> {
        let [x, y, w, h] = Self::mini(card);
        let span = |origin: f32, start: f32, length: f32, size: f32| {
            let inner = size - 2.0 * INSET;
            let lead =
                origin + INSET + start * inner + if start > 0.001 { TILE_GAP / 2.0 } else { 0.0 };
            let end = start + length;
            let trail =
                origin + INSET + end * inner - if end < 0.999 { TILE_GAP / 2.0 } else { 0.0 };
            (lead, trail - lead)
        };
        onboarding_plan::dwindle(count)
            .into_iter()
            .map(|[ux, uy, uw, uh]| {
                let (tx, tw) = span(x, ux, uw, w);
                let (ty, th) = span(y, uy, uh, h);
                [tx, ty, tw, th]
            })
            .collect()
    }

    /// What lies under a board point: a card (its header included) or the Floating line.
    pub fn target_at(&self, x: f32, y: f32) -> Option<Target> {
        if let Some(card) = self.cards.iter().find(|card| {
            (card.x..=card.x + card.w).contains(&x)
                && (card.y..=card.y + HEAD_H + HEAD_GAP + card.mini_h).contains(&y)
        }) {
            return Some(Target::Workspace(card.workspace));
        }
        ((self.strip_y - STRIP_GAP / 2.0..=self.height).contains(&y) && (0.0..=WIDTH).contains(&x))
            .then_some(Target::Float)
    }
}

pub fn app_name(ui: &Ui, bundle_id: &str) -> String {
    ui.apps
        .cached(bundle_id)
        .map_or_else(|| apps::fallback_name(bundle_id), |app| app.name.clone())
}

fn app_icon(ui: &Ui, p: Paint, bundle_id: &str, size: f32) -> El {
    controls::icon_or_initial(p, ui.apps.cached(bundle_id), &app_name(ui, bundle_id), size)
}

pub fn view(ui: &Ui, p: Paint, board: &Board) -> El {
    let c = &p.k.c;
    let geometry = board.geometry();
    let shortcuts = ui.doc.shortcuts();
    let dragging = board.drag.as_ref().filter(|drag| drag.moved);
    let target = dragging.and_then(|drag| drag.target);
    let radius = p.k.radius.lg;
    let mut layer = el().key("board").size(WIDTH, geometry.height);
    for screen in &geometry.screens {
        let key = screen.screen.0;
        layer = layer
            .child(
                el().key(format!("screen-{key}"))
                    .absolute(screen.x, screen.y)
                    .size(screen.w, screen.h)
                    .radius(p.k.radius.lg)
                    .fill(c.layer_01)
                    .border(p.hair, c.border_weak)
                    .a11y_hidden(),
            )
            .child(
                text(screen.name.clone())
                    .key(format!("screen-name-{key}"))
                    .absolute(screen.x + SCREEN_PAD, screen.y + SCREEN_PAD)
                    .w(screen.w - 2.0 * SCREEN_PAD)
                    .truncate()
                    .font(p.meta())
                    .color(p.faint()),
            );
    }
    for (card, planned) in geometry.cards.iter().zip(&board.plan.workspaces) {
        let workspace = card.workspace;
        let mut head = row()
            .key(format!("head-{workspace}"))
            .absolute(card.x, card.y)
            .size(card.w, HEAD_H)
            .gap(6.0)
            .center_y()
            .child(
                text(workspace.to_string())
                    .key("number")
                    .font(p.tabular(p.strong()))
                    .color(p.text()),
            );
        if let Some(keys) = panes::shortcuts::switch_keys(&shortcuts, workspace) {
            head = head
                .child(text("·").key("dot").font(p.row_title()).color(p.faint()))
                .child(text(keys).key("keys").font(p.row_title()).color(p.muted()));
        }
        let [x, y, w, h] = Geometry::mini(card);
        let aimed = target == Some(Target::Workspace(workspace));
        let mini = controls::surface(p, el().key(format!("card-{workspace}")))
            .absolute(x, y)
            .size(w, h)
            .label(format!("Workspace {workspace}"))
            .border(
                if aimed { 1.5 } else { p.hair },
                if aimed { p.accent() } else { c.border_weak },
            )
            .opacity(if planned.tiles.is_empty() && !aimed {
                0.55
            } else {
                1.0
            })
            .motion(SETTLE);
        layer = layer.child(head).child(mini);
        let frames = geometry.tiles(card, planned.tiles.len());
        for (index, (bundle_id, frame)) in planned.tiles.iter().zip(frames).enumerate() {
            let tile = Tile {
                workspace,
                index,
                bundle_id: bundle_id.clone(),
                frame,
            };
            let held = dragging.is_some_and(|drag| drag.from == workspace && drag.index == index);
            layer = layer.child(tile_view(ui, p, &tile, &geometry, radius - INSET, held));
        }
    }
    layer = layer.children(strip(
        ui,
        p,
        board,
        &geometry,
        target == Some(Target::Float),
    ));
    if let Some(drag) = dragging {
        let size = icon_size(drag.size);
        let mut ghost = app_icon(ui, p, &drag.bundle_id, size)
            .key("ghost")
            .cursor(Cursor::Grabbing)
            .absolute(
                drag.origin.0 + drag.grab.0 + drag.delta.0 - size / 2.0,
                drag.origin.1 + drag.grab.1 + drag.delta.1 - size / 2.0,
            );
        for shadow in &p.k.elevation.floating {
            ghost = ghost.box_shadow(*shadow);
        }
        layer = layer.child(ghost);
    }
    layer
}

fn icon_size((w, h): (f32, f32)) -> f32 {
    (w.min(h) - 8.0).clamp(ICON_MIN, ICON_MAX)
}

fn tile_view(ui: &Ui, p: Paint, tile: &Tile, geometry: &Geometry, radius: f32, held: bool) -> El {
    let c = &p.k.c;
    let [x, y, w, h] = tile.frame;
    let name = app_name(ui, &tile.bundle_id);
    let source = tile.clone();
    let geometry = geometry.clone();
    el().key(format!("tile-{}-{}", tile.workspace, tile.index))
        .absolute(x, y)
        .size(w, h)
        .radius(radius.max(2.0))
        .fill(c.layer_02)
        .border(p.hair, c.border_muted)
        .center()
        .role("button")
        .label(format!("{name}, workspace {}", tile.workspace))
        .help("Click to send it to the next workspace, or drag it to another")
        .cursor(Cursor::Grab)
        .opacity(if held { 0.3 } else { 1.0 })
        .motion(SETTLE)
        .on_drag(move |ui: &mut Ui, d: &mut rune::Drag, _: &mut Tx| {
            if let Some(tour) = &mut ui.tour {
                tour.board.drag_step(
                    d.phase,
                    (d.start_x, d.start_y),
                    (d.dx, d.dy),
                    d.cancelled(),
                    &source,
                    &geometry,
                );
            }
        })
        .child(app_icon(ui, p, &tile.bundle_id, icon_size((w, h))).key("icon"))
}

/// The Floating line: the apps that stay out of the tiles, each pressed to tile it. While a
/// tile is dragged it is also where a drop floats that app.
fn strip(ui: &Ui, p: Paint, board: &Board, geometry: &Geometry, aimed: bool) -> Option<El> {
    let dragging = board.drag.as_ref().is_some_and(|drag| drag.moved);
    let starter = board.nothing_to_arrange() && !dragging;
    if board.plan.floating.is_empty() && !dragging && !starter {
        return None;
    }
    let mut line = row()
        .key("floating")
        .absolute(0.0, geometry.strip_y)
        .size(WIDTH, STRIP_H)
        .radius(p.k.radius.md)
        .center()
        .motion(SETTLE);
    if aimed {
        line = line.fill(p.k.c.layer_02);
    }
    if starter {
        // One window or none: say so where the Floating line would be.
        return Some(
            line.child(
                text("Nothing to arrange yet. New windows tile on the workspace you're on.")
                    .key("starter")
                    .font(p.row_title())
                    .color(p.muted()),
            ),
        );
    }
    if board.plan.floating.is_empty() {
        return Some(
            line.child(
                text("Drop here to float")
                    .key("hint")
                    .font(p.row_title())
                    .color(p.faint()),
            ),
        );
    }
    line = line.child(
        text("Floating:")
            .key("label")
            .font(p.row_title())
            .color(p.muted()),
    );
    for (index, bundle_id) in board.plan.floating.iter().enumerate() {
        if index > 0 {
            line = line.child(
                text("·")
                    .key(format!("dot{index}"))
                    .font(p.row_title())
                    .color(p.faint()),
            );
        }
        let name = app_name(ui, bundle_id);
        let bundle_id = bundle_id.clone();
        line = line.child(
            controls::quiet_button(p, &format!("float-{index}"), &name, move |ui, _| {
                if let Some(tour) = &mut ui.tour {
                    tour.board.set_floating(&bundle_id, false);
                }
            })
            .help(format!("Tile {name} instead")),
        );
    }
    Some(line)
}
