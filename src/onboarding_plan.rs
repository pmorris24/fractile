//! The setup tour's suggested arrangement: a local heuristic over the windows open right now.
//!
//! Everything here is pure and title-free by construction. An [`Inventory`] holds bundle IDs,
//! an optional Launch Services category, window sizes and resizability, and each display's
//! width and workspaces; nothing else leaves the observation it was built from. [`plan`] groups
//! the open apps by role, caps how many tiles a workspace gets, spreads a many-window app over
//! several workspaces, and sets small utilities aside to float.
//!
//! A plan only moves the windows that are open. It never becomes a route rule: people work in
//! projects, and a rule would pull every later window of a browser or terminal back to one
//! workspace.

use crate::{AppObservation, NativeObservation, WorkspaceId};

#[cfg(test)]
mod tests;

/// A display this wide or narrower is a laptop panel: two tiles a workspace.
pub const LAPTOP_WIDTH: i32 = 1728;
/// A window under this size on both sides is a small utility, not something to tile.
const SMALL_SIDE: i32 = 480;

/// A window as the plan sees it. There is no title field to leak.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OpenWindow {
    pub resizable: bool,
    pub width: i32,
    pub height: i32,
}

impl OpenWindow {
    const fn small(self) -> bool {
        self.width < SMALL_SIDE && self.height < SMALL_SIDE
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpenApp {
    pub bundle_id: String,
    /// The app's `LSApplicationCategoryType`, for apps the role table doesn't know.
    pub category: Option<String>,
    pub windows: Vec<OpenWindow>,
}

/// A display: its usable width and the workspaces that show on it, in order.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Lane {
    pub width: i32,
    pub workspaces: Vec<WorkspaceId>,
}

impl Lane {
    /// The most tiles a workspace on this display gets.
    #[must_use]
    pub const fn cap(&self) -> usize {
        if self.width <= LAPTOP_WIDTH { 2 } else { 3 }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Inventory {
    pub apps: Vec<OpenApp>,
    pub main: Lane,
    pub secondary: Option<Lane>,
}

impl Inventory {
    /// The open apps of `observation`, merged by bundle ID in the order first seen. Only each
    /// window's size and resizability are read.
    #[must_use]
    pub fn from_observation(
        observation: &NativeObservation,
        main: Lane,
        secondary: Option<Lane>,
    ) -> Self {
        let mut inventory = Self {
            apps: Vec::new(),
            main,
            secondary,
        };
        for application in &observation.applications {
            let AppObservation::Complete { app, windows, .. } = application else {
                continue;
            };
            for window in windows {
                inventory.add_window(
                    &app.bundle_id,
                    OpenWindow {
                        resizable: window.resizable,
                        width: window.frame.width,
                        height: window.frame.height,
                    },
                );
            }
        }
        inventory
    }

    /// Adds one open window of `bundle_id`, such as one parked on a hidden workspace that an
    /// observation of the screen leaves out.
    pub fn add_window(&mut self, bundle_id: &str, window: OpenWindow) {
        match self.apps.iter_mut().find(|app| app.bundle_id == bundle_id) {
            Some(app) => app.windows.push(window),
            None => self.apps.push(OpenApp {
                bundle_id: bundle_id.to_owned(),
                category: None,
                windows: vec![window],
            }),
        }
    }

    fn app(&self, bundle_id: &str) -> Option<&OpenApp> {
        self.apps.iter().find(|app| app.bundle_id == bundle_id)
    }
}

/// What an app is for, which decides the workspace it starts on.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Role {
    Web,
    Code,
    Chat,
    Docs,
    Media,
    /// Unknown: shares the Docs workspace.
    Other,
}

impl Role {
    const ORDER: [Self; 6] = [
        Self::Web,
        Self::Code,
        Self::Chat,
        Self::Docs,
        Self::Media,
        Self::Other,
    ];
}

const WEB: &[&str] = &[
    "com.apple.Safari",
    "com.apple.SafariTechnologyPreview",
    "com.google.Chrome",
    "com.google.Chrome.canary",
    "com.brave.Browser",
    "org.mozilla.firefox",
    "org.mozilla.firefoxdeveloperedition",
    "company.thebrowser.Browser",
    "company.thebrowser.dia",
    "com.microsoft.edgemac",
    "com.vivaldi.Vivaldi",
    "com.operasoftware.Opera",
    "com.kagi.kagimacOS",
    "com.duckduckgo.macos.browser",
    "app.zen-browser.zen",
    "net.imput.helium",
];

const CODE: &[&str] = &[
    "com.apple.Terminal",
    "com.googlecode.iterm2",
    "com.mitchellh.ghostty",
    "dev.warp.Warp-Stable",
    "net.kovidgoyal.kitty",
    "org.alacritty",
    "com.github.wez.wezterm",
    "com.microsoft.VSCode",
    "com.microsoft.VSCodeInsiders",
    "com.todesktop.230313mzl4w4u92",
    "dev.zed.Zed",
    "com.apple.dt.Xcode",
    "com.sublimetext.4",
    "com.panic.Nova",
    "ai.opencode.desktop",
    "ai.opencode.desktop.beta",
    "com.github.GitHubClient",
    "com.docker.docker",
];

const CODE_PREFIXES: &[&str] = &["com.jetbrains."];

const CHAT: &[&str] = &[
    "com.tinyspeck.slackmacgap",
    "com.hnc.Discord",
    "com.apple.MobileSMS",
    "com.apple.mail",
    "com.apple.FaceTime",
    "ru.keepcoder.Telegram",
    "net.whatsapp.WhatsApp",
    "org.whispersystems.signal-desktop",
    "us.zoom.xos",
    "com.microsoft.teams2",
    "com.microsoft.Outlook",
    "com.readdle.smartemail-Mac",
    "com.superhuman.electron",
    "com.mimestream.Mimestream",
];

const DOCS: &[&str] = &[
    "com.apple.Notes",
    "com.apple.TextEdit",
    "com.apple.Preview",
    "com.apple.finder",
    "com.apple.iCal",
    "com.apple.reminders",
    "com.apple.iWork.Pages",
    "com.apple.iWork.Numbers",
    "com.apple.iWork.Keynote",
    "com.microsoft.Word",
    "com.microsoft.Excel",
    "com.microsoft.Powerpoint",
    "md.obsidian",
    "notion.id",
    "com.linear",
    "com.figma.Desktop",
    "com.bohemiancoding.sketch3",
    "net.shinyfrog.bear",
    "com.culturedcode.ThingsMac",
];

const MEDIA: &[&str] = &[
    "com.spotify.client",
    "com.apple.Music",
    "com.apple.TV",
    "com.apple.podcasts",
    "com.apple.Photos",
    "com.apple.QuickTimePlayerX",
    "org.videolan.vlc",
    "com.colliderli.iina",
];

fn listed(list: &[&str], bundle_id: &str) -> bool {
    list.iter()
        .any(|known| known.eq_ignore_ascii_case(bundle_id))
}

fn known_role(bundle_id: &str) -> Option<Role> {
    let prefixed = |prefixes: &[&str]| {
        prefixes.iter().any(|prefix| {
            bundle_id
                .get(..prefix.len())
                .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
        })
    };
    [
        (WEB, Role::Web),
        (CODE, Role::Code),
        (CHAT, Role::Chat),
        (DOCS, Role::Docs),
        (MEDIA, Role::Media),
    ]
    .into_iter()
    .find(|(list, _)| listed(list, bundle_id))
    .map(|(_, role)| role)
    .or_else(|| prefixed(CODE_PREFIXES).then_some(Role::Code))
}

/// The role a Launch Services category implies, for an app the table doesn't name.
fn category_role(category: &str) -> Role {
    let kind = category
        .strip_prefix("public.app-category.")
        .unwrap_or(category);
    match kind {
        "developer-tools" => Role::Code,
        "social-networking" | "business" => Role::Chat,
        "productivity" | "graphics-design" | "photography" | "reference" | "education"
        | "finance" | "news" | "medical" | "lifestyle" => Role::Docs,
        "music" | "video" | "entertainment" | "sports" => Role::Media,
        kind if kind.ends_with("games") => Role::Media,
        _ => Role::Other,
    }
}

/// An app's role: the table of well-known bundle IDs first, then its category.
#[must_use]
pub fn role(bundle_id: &str, category: Option<&str>) -> Role {
    known_role(bundle_id)
        .or_else(|| category.map(category_role))
        .unwrap_or(Role::Other)
}

/// Whether the plan starts `app` floating: a utility Fractile floats by default
/// ([`crate::defaults`]; a Stickies note tiled to half a display is nobody's intent), an app
/// with no resizable window, or an app the role table doesn't know whose windows are all small.
#[must_use]
pub fn floats(app: &OpenApp) -> bool {
    crate::defaults::floats_by_default(&app.bundle_id)
        || app.windows.iter().all(|window| !window.resizable)
        || (known_role(&app.bundle_id).is_none() && app.windows.iter().all(|window| window.small()))
}

/// How many tiles `app` takes: its resizable windows, and at least one.
fn tile_count(app: &OpenApp) -> usize {
    app.windows
        .iter()
        .filter(|window| window.resizable)
        .count()
        .max(1)
}

/// Which display a planned workspace shows on.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Screen {
    Main,
    Secondary,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlannedWorkspace {
    pub workspace: WorkspaceId,
    pub screen: Screen,
    /// One bundle ID per tile, in layout order.
    pub tiles: Vec<String>,
}

/// The suggestion the tour draws and the user edits.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Plan {
    /// The workspaces to show, each display's in order: at least the first four on one
    /// display, the first two of each on two.
    pub workspaces: Vec<PlannedWorkspace>,
    /// Bundle IDs of the apps that float.
    pub floating: Vec<String>,
}

/// Where one of an app's windows goes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArrangedSlot {
    Tile(WorkspaceId),
    Float,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArrangedApp {
    pub bundle_id: String,
    /// The app's windows in a stable order; windows past the last slot follow it.
    pub slots: Vec<ArrangedSlot>,
}

/// A plan as the worker applies it: per app, where each open window goes. No titles and no
/// native handles, so it may cross threads.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Arrangement {
    pub apps: Vec<ArrangedApp>,
}

struct Fill {
    screen: Screen,
    cap: usize,
    workspaces: Vec<(WorkspaceId, Vec<String>)>,
}

impl Fill {
    fn new(screen: Screen, lane: &Lane) -> Self {
        Self {
            screen,
            cap: lane.cap(),
            workspaces: lane
                .workspaces
                .iter()
                .map(|workspace| (*workspace, Vec::new()))
                .collect(),
        }
    }

    fn empty(&self) -> Option<usize> {
        self.workspaces
            .iter()
            .position(|(_, tiles)| tiles.is_empty())
    }
}

/// Splits `count` tiles into the fewest workspaces of at most `cap`, as evenly as they go:
/// four tiles under a cap of three are two and two, not three and one.
fn chunk_sizes(count: usize, cap: usize) -> Vec<usize> {
    if count == 0 {
        return Vec::new();
    }
    let chunks = count.div_ceil(cap);
    let (base, extra) = (count / chunks, count % chunks);
    (0..chunks)
        .map(|index| base + usize::from(index < extra))
        .collect()
}

/// The lane and the place in it where `role` starts.
const fn home(role: Role, dual: bool) -> (usize, usize) {
    match (dual, role) {
        (_, Role::Web) => (0, 0),
        (_, Role::Code) => (0, 1),
        (false, Role::Chat) => (0, 2),
        (false, Role::Docs | Role::Other) => (0, 3),
        (false, Role::Media) => (0, 4),
        (true, Role::Chat) => (1, 0),
        (true, Role::Docs | Role::Other) => (1, 1),
        (true, Role::Media) => (1, 2),
    }
}

/// Suggests where the open windows go.
#[must_use]
pub fn plan(inventory: &Inventory) -> Plan {
    let mut fills = vec![Fill::new(Screen::Main, &inventory.main)];
    if let Some(secondary) = inventory
        .secondary
        .as_ref()
        .filter(|lane| !lane.workspaces.is_empty())
    {
        fills.push(Fill::new(Screen::Secondary, secondary));
    }
    let dual = fills.len() == 2;
    let floating: Vec<String> = inventory
        .apps
        .iter()
        .filter(|app| floats(app))
        .map(|app| app.bundle_id.clone())
        .collect();
    if fills[0].workspaces.is_empty() {
        return Plan {
            workspaces: Vec::new(),
            floating,
        };
    }

    // Each role's tiles go to its home first; what doesn't fit waits for an empty workspace.
    let mut spill: Vec<(usize, Vec<String>)> = Vec::new();
    for role in Role::ORDER {
        let tiles: Vec<String> = inventory
            .apps
            .iter()
            .filter(|app| {
                !floats(app) && self::role(&app.bundle_id, app.category.as_deref()) == role
            })
            .flat_map(|app| std::iter::repeat_n(app.bundle_id.clone(), tile_count(app)))
            .collect();
        if tiles.is_empty() {
            continue;
        }
        let (lane, place) = home(role, dual);
        let fill = &mut fills[lane];
        let place = place.min(fill.workspaces.len() - 1);
        let room = fill.cap.saturating_sub(fill.workspaces[place].1.len());
        let first = chunk_sizes(tiles.len(), fill.cap)
            .first()
            .copied()
            .unwrap_or(0)
            .min(room);
        let mut tiles = tiles.into_iter();
        fill.workspaces[place].1.extend(tiles.by_ref().take(first));
        let rest: Vec<String> = tiles.collect();
        let sizes = chunk_sizes(rest.len(), fill.cap);
        let mut rest = rest.into_iter();
        for size in sizes {
            spill.push((lane, rest.by_ref().take(size).collect()));
        }
    }
    for (lane, chunk) in spill {
        let target = fills[lane]
            .empty()
            .map(|place| (lane, place))
            .or_else(|| {
                fills
                    .iter()
                    .enumerate()
                    .find_map(|(other, fill)| fill.empty().map(|place| (other, place)))
            })
            .unwrap_or_else(|| {
                // Every workspace holds something: the emptiest of this display's takes it.
                let place = fills[lane]
                    .workspaces
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, (_, tiles))| tiles.len())
                    .map_or(0, |(place, _)| place);
                (lane, place)
            });
        fills[target.0].workspaces[target.1].1.extend(chunk);
    }

    let shown = if dual { 2 } else { 4 };
    let workspaces = fills
        .into_iter()
        .flat_map(|fill| {
            let used = fill
                .workspaces
                .iter()
                .rposition(|(_, tiles)| !tiles.is_empty())
                .map_or(0, |last| last + 1);
            let screen = fill.screen;
            fill.workspaces
                .into_iter()
                .take(used.max(shown))
                .map(move |(workspace, tiles)| PlannedWorkspace {
                    workspace,
                    screen,
                    tiles,
                })
        })
        .collect();
    Plan {
        workspaces,
        floating,
    }
}

impl Plan {
    /// How many tiles the plan places.
    #[must_use]
    pub fn tile_count(&self) -> usize {
        self.workspaces
            .iter()
            .map(|workspace| workspace.tiles.len())
            .sum()
    }

    /// Nothing to arrange: at most one window would move.
    #[must_use]
    pub fn is_trivial(&self) -> bool {
        self.tile_count() <= 1
    }

    /// The workspace after `workspace` among those shown, wrapping: where a click sends a tile.
    #[must_use]
    pub fn next_workspace(&self, workspace: WorkspaceId) -> Option<WorkspaceId> {
        let index = self
            .workspaces
            .iter()
            .position(|entry| entry.workspace == workspace)?;
        let next = &self.workspaces[(index + 1) % self.workspaces.len()];
        (next.workspace != workspace).then_some(next.workspace)
    }

    /// Moves tile `index` of `from` to the end of `to`. Returns whether anything moved.
    pub fn move_tile(&mut self, from: WorkspaceId, index: usize, to: WorkspaceId) -> bool {
        if from == to || !self.workspaces.iter().any(|entry| entry.workspace == to) {
            return false;
        }
        let Some(source) = self
            .workspaces
            .iter_mut()
            .find(|entry| entry.workspace == from)
            .filter(|entry| index < entry.tiles.len())
        else {
            return false;
        };
        let tile = source.tiles.remove(index);
        if let Some(target) = self
            .workspaces
            .iter_mut()
            .find(|entry| entry.workspace == to)
        {
            target.tiles.push(tile);
        }
        true
    }

    /// Floats `bundle_id`'s windows, or tiles them again on the workspace its role starts on.
    pub fn set_floating(&mut self, bundle_id: &str, floating: bool, inventory: &Inventory) {
        let is_floating = self.floating.iter().any(|known| known == bundle_id);
        if floating == is_floating {
            return;
        }
        if floating {
            for workspace in &mut self.workspaces {
                workspace.tiles.retain(|tile| tile != bundle_id);
            }
            self.floating.push(bundle_id.to_owned());
            return;
        }
        let Some(app) = inventory.app(bundle_id) else {
            return;
        };
        let dual = self
            .workspaces
            .iter()
            .any(|workspace| workspace.screen == Screen::Secondary);
        let (lane, place) = home(role(bundle_id, app.category.as_deref()), dual);
        let screen = if lane == 0 {
            Screen::Main
        } else {
            Screen::Secondary
        };
        let lane_len = self
            .workspaces
            .iter()
            .filter(|workspace| workspace.screen == screen)
            .count();
        let Some(target) = self
            .workspaces
            .iter_mut()
            .filter(|workspace| workspace.screen == screen)
            .nth(place.min(lane_len.saturating_sub(1)))
        else {
            return;
        };
        target
            .tiles
            .extend(std::iter::repeat_n(bundle_id.to_owned(), tile_count(app)));
        self.floating.retain(|known| known != bundle_id);
    }

    /// The plan for the worker: each app's windows in order, tiles by workspace then position.
    #[must_use]
    pub fn arrangement(&self) -> Arrangement {
        let mut apps: Vec<ArrangedApp> = Vec::new();
        for workspace in &self.workspaces {
            for tile in &workspace.tiles {
                let slot = ArrangedSlot::Tile(workspace.workspace);
                match apps.iter_mut().find(|app| &app.bundle_id == tile) {
                    Some(app) => app.slots.push(slot),
                    None => apps.push(ArrangedApp {
                        bundle_id: tile.clone(),
                        slots: vec![slot],
                    }),
                }
            }
        }
        for bundle_id in &self.floating {
            if !apps.iter().any(|app| &app.bundle_id == bundle_id) {
                apps.push(ArrangedApp {
                    bundle_id: bundle_id.clone(),
                    slots: vec![ArrangedSlot::Float],
                });
            }
        }
        Arrangement { apps }
    }

    /// The floating apps that need a `float` rule to stay floating: those with a resizable
    /// window, which Fractile would otherwise tile. A utility that floats by default
    /// ([`crate::defaults`]) needs none.
    #[must_use]
    pub fn float_rules(&self, inventory: &Inventory) -> Vec<String> {
        self.floating
            .iter()
            .filter(|bundle_id| {
                !crate::defaults::floats_by_default(bundle_id)
                    && inventory
                        .app(bundle_id)
                        .is_some_and(|app| app.windows.iter().any(|window| window.resizable))
            })
            .cloned()
            .collect()
    }

    /// The open utilities that float by default but this plan tiles: each needs a `tile` rule,
    /// or Fractile floats it again. With [`Self::float_rules`], the only rules a plan asks for.
    #[must_use]
    pub fn tile_rules(&self, inventory: &Inventory) -> Vec<String> {
        inventory
            .apps
            .iter()
            .map(|app| &app.bundle_id)
            .filter(|bundle_id| {
                crate::defaults::floats_by_default(bundle_id)
                    && !self.floating.iter().any(|floating| floating == *bundle_id)
            })
            .cloned()
            .collect()
    }
}

/// Dwindle's frames for `count` tiles in a landscape workspace, in the unit square
/// (`[x, y, width, height]`): each new tile halves the last one's space, across its longer side.
#[must_use]
pub fn dwindle(count: usize) -> Vec<[f32; 4]> {
    /// A display's width over its height, near enough for a miniature.
    const ASPECT: f32 = 1.6;
    let mut frames = Vec::with_capacity(count);
    let mut rest = [0.0, 0.0, 1.0, 1.0_f32];
    for index in 0..count {
        if index + 1 == count {
            frames.push(rest);
            break;
        }
        let [x, y, w, h] = rest;
        if w * ASPECT >= h {
            frames.push([x, y, w / 2.0, h]);
            rest = [x + w / 2.0, y, w / 2.0, h];
        } else {
            frames.push([x, y, w, h / 2.0]);
            rest = [x, y + h / 2.0, w, h / 2.0];
        }
    }
    frames
}
