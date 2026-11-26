//! Display discovery: each connected screen's stable ID, name and visible frame.

use std::{collections::HashMap, ptr::NonNull};

use objc2::{MainThreadMarker, rc::autoreleasepool};
use objc2_app_kit::{NSApplication, NSEvent, NSScreen};
use objc2_core_foundation::{CFRetained, CFUUID};
use objc2_foundation::{NSNumber, ns_string};

use crate::{DisplaySelector, Rect};

use super::SnapshotError;

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn CGDisplayCreateUUIDFromDisplayID(display: u32) -> *const CFUUID;
    fn CGDisplayIsBuiltin(display: u32) -> u32;
}

pub(super) fn main_output() -> Result<Rect, SnapshotError> {
    Ok(outputs()?.main().frame)
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct DisplayOutput {
    pub id: String,
    pub name: String,
    pub frame: Rect,
    pub is_main: bool,
}

/// Summary of connected built-in vs external displays, used for
/// `settings.pause_on_builtin_display`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DisplayTopology {
    pub builtin: usize,
    pub external: usize,
}

impl DisplayTopology {
    /// Builds a topology from each connected display's `is_builtin` flag.
    #[must_use]
    pub fn from_builtin_flags(displays: impl IntoIterator<Item = bool>) -> Self {
        let mut topology = Self::default();
        for is_builtin in displays {
            if is_builtin {
                topology.builtin += 1;
            } else {
                topology.external += 1;
            }
        }
        topology
    }

    /// True when at least one built-in display is connected and zero external displays are attached.
    /// Desktop Macs (0 built-in displays) return `false`.
    #[must_use]
    pub const fn is_builtin_only(self) -> bool {
        self.builtin > 0 && self.external == 0
    }

    /// True when at least one external display is connected.
    #[must_use]
    pub const fn has_external(self) -> bool {
        self.external > 0
    }
}

/// Maximum distance outside a display's visible tiling frame (in points) that a
/// synthetic `DisplayOutputs::new` still attributes to that display's reserved
/// menu-bar or Dock strip when full physical screen bounds were not provided.
const SYNTHETIC_STRIP_MARGIN: i64 = 256;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DisplayOutputs {
    displays: Vec<DisplayOutput>,
    /// Full physical screen bounds (including menu bar, notch, and Dock), parallel
    /// to `displays`, when discovered from macOS `NSScreen` or supplied via
    /// `with_bounds`. `None` when constructed from usable frames alone.
    bounds: Option<Vec<Rect>>,
    main_index: usize,
}

impl DisplayOutputs {
    pub fn new(displays: Vec<DisplayOutput>) -> Result<Self, SnapshotError> {
        let Some(main_index) = displays.iter().position(|display| display.is_main) else {
            return Err(SnapshotError::MainScreenUnavailable);
        };
        Ok(Self {
            displays,
            bounds: None,
            main_index,
        })
    }

    pub fn with_bounds(
        displays: Vec<DisplayOutput>,
        bounds: Vec<Rect>,
    ) -> Result<Self, SnapshotError> {
        let mut outputs = Self::new(displays)?;
        if bounds.len() == outputs.displays.len() {
            outputs.bounds = Some(bounds);
        }
        Ok(outputs)
    }

    #[must_use]
    pub fn displays(&self) -> &[DisplayOutput] {
        &self.displays
    }

    #[must_use]
    pub fn main(&self) -> &DisplayOutput {
        &self.displays[self.main_index]
    }

    /// The non-main displays left to right, then top to bottom, ties broken by ID.
    fn others(&self) -> impl Iterator<Item = &DisplayOutput> {
        self.ordered()
            .into_iter()
            .filter(|display| !display.is_main)
    }

    /// The leftmost non-main display.
    #[must_use]
    pub fn secondary(&self) -> Option<&DisplayOutput> {
        self.others().next()
    }

    /// The second non-main display from the left.
    #[must_use]
    pub fn tertiary(&self) -> Option<&DisplayOutput> {
        self.others().nth(1)
    }

    #[must_use]
    pub fn resolve_exact(&self, selector: &DisplaySelector) -> Option<&DisplayOutput> {
        match selector {
            DisplaySelector::Main => Some(self.main()),
            DisplaySelector::Secondary => self.secondary(),
            DisplaySelector::Tertiary => self.tertiary(),
            DisplaySelector::Preferred(preferred) => self
                .displays
                .iter()
                .find(|display| display.id == *preferred || display.name == *preferred),
        }
    }

    #[must_use]
    pub fn resolve(&self, selector: &DisplaySelector) -> &DisplayOutput {
        self.resolve_exact(selector)
            .or_else(|| match selector {
                // With two displays, the tertiary workspaces share the secondary display.
                DisplaySelector::Tertiary => self.secondary(),
                _ => None,
            })
            .unwrap_or_else(|| self.main())
    }

    /// The canonical `config.toml` selector for `display`: `main`, `secondary` (the leftmost
    /// non-main display), `tertiary` (the next one), or the display's own ID for every
    /// display past those three.
    #[must_use]
    pub fn selector_for(&self, display: &DisplayOutput) -> DisplaySelector {
        if display.is_main {
            DisplaySelector::Main
        } else if self
            .secondary()
            .is_some_and(|secondary| secondary.id == display.id)
        {
            DisplaySelector::Secondary
        } else if self
            .tertiary()
            .is_some_and(|tertiary| tertiary.id == display.id)
        {
            DisplaySelector::Tertiary
        } else {
            DisplaySelector::Preferred(display.id.clone())
        }
    }

    /// Every display left to right, then top to bottom, ties broken by ID.
    #[must_use]
    pub fn ordered(&self) -> Vec<&DisplayOutput> {
        let mut displays: Vec<_> = self.displays.iter().collect();
        displays.sort_by(|left, right| {
            (left.frame.x, left.frame.y, left.id.as_str()).cmp(&(
                right.frame.x,
                right.frame.y,
                right.id.as_str(),
            ))
        });
        displays
    }

    /// `display`'s name for people: its product name, and when another connected display
    /// shares that name, where it sits among them in place of the "(1)" / "(2)" macOS appends.
    /// Twins whose horizontal ranges all overlap are a stack ("top", "bottom", "2 from top");
    /// any others go left to right ("left", "right", "2 from left").
    #[must_use]
    pub fn label(&self, display: &DisplayOutput) -> String {
        let base = product_name(&display.name);
        let mut twins: Vec<&DisplayOutput> = self
            .ordered()
            .into_iter()
            .filter(|other| product_name(&other.name) == base)
            .collect();
        if twins.len() < 2 || !twins.iter().any(|other| other.id == display.id) {
            return display.name.clone();
        }
        let stacked = twins.iter().enumerate().all(|(index, first)| {
            twins[index + 1..]
                .iter()
                .all(|second| overlap_horizontally(first.frame, second.frame))
        });
        if stacked {
            twins.sort_by_key(|twin| (twin.frame.y, twin.frame.x, twin.id.as_str()));
        }
        let Some(index) = twins.iter().position(|other| other.id == display.id) else {
            return display.name.clone();
        };
        let position = match (twins.len(), stacked, index) {
            (2, true, 0) => "top".to_owned(),
            (2, true, _) => "bottom".to_owned(),
            (2, false, 0) => "left".to_owned(),
            (2, false, _) => "right".to_owned(),
            (_, true, _) => format!("{} from top", index + 1),
            (_, false, _) => format!("{} from left", index + 1),
        };
        format!("{base} ({position})")
    }

    pub fn frames(&self) -> impl Iterator<Item = Rect> + '_ {
        self.displays.iter().map(|display| display.frame)
    }

    /// Each display's full bounds, menu bar and Dock included, or its visible
    /// frame when only those were provided.
    #[must_use]
    pub fn screens(&self) -> Vec<Rect> {
        self.bounds
            .clone()
            .unwrap_or_else(|| self.frames().collect())
    }

    #[must_use]
    pub fn union(&self) -> Rect {
        self.frames().fold(self.main().frame, union_rect)
    }

    /// Physical display containing `point`, including its menu-bar and Dock strips.
    ///
    /// Uses full `NSScreen::frame()` bounds when available (with a 1-point tolerance
    /// for rounded screen-edge pointer coordinates). When only usable frames were
    /// provided (synthetic fixtures), falls back to the nearest display whose frame
    /// lies within a reserved-strip margin.
    #[must_use]
    pub fn display_at(&self, point: (i32, i32)) -> Option<&DisplayOutput> {
        let (x, y) = point;
        if let Some(bounds) = &self.bounds {
            if let Some(index) = bounds.iter().position(|rect| rect.contains(x, y)) {
                return Some(&self.displays[index]);
            }
            return self.nearest_within(bounds, x, y, 1);
        }
        if let Some(display) = self.displays.iter().find(|d| d.frame.contains(x, y)) {
            return Some(display);
        }
        let frames: Vec<Rect> = self.displays.iter().map(|d| d.frame).collect();
        self.nearest_within(&frames, x, y, SYNTHETIC_STRIP_MARGIN)
    }

    /// Physical display for `point`, matching menu-bar and Dock strips and falling
    /// back to the nearest display rather than jumping straight to main.
    #[must_use]
    pub fn display_for_point(&self, point: (i32, i32)) -> &DisplayOutput {
        if let Some(display) = self.display_at(point) {
            return display;
        }
        let (x, y) = point;
        let rects = self.bounds.as_deref();
        self.displays
            .iter()
            .enumerate()
            .min_by_key(|(index, display)| {
                let rect = rects.map_or(display.frame, |bounds| bounds[*index]);
                let (dx, dy) = rect_axis_distance(rect, x, y);
                (dx * dx + dy * dy, !display.is_main)
            })
            .map_or_else(|| self.main(), |(_, display)| display)
    }

    fn nearest_within(
        &self,
        rects: &[Rect],
        x: i32,
        y: i32,
        max_margin: i64,
    ) -> Option<&DisplayOutput> {
        self.displays
            .iter()
            .enumerate()
            .filter_map(|(index, display)| {
                let (dx, dy) = rect_axis_distance(rects[index], x, y);
                (dx <= max_margin && dy <= max_margin).then_some((
                    dx * dx + dy * dy,
                    !display.is_main,
                    display,
                ))
            })
            .min_by_key(|(distance, not_main, _)| (*distance, *not_main))
            .map(|(_, _, display)| display)
    }
}

pub fn outputs() -> Result<DisplayOutputs, SnapshotError> {
    autoreleasepool(|_| {
        let marker = MainThreadMarker::new().ok_or(SnapshotError::MainThreadRequired)?;
        // Without an initialized `NSApplication` (e.g. in the standalone `fractile displays`
        // CLI), AppKit reports `screensHaveSeparateSpaces == false` and includes secondary
        // displays' menu bars inside `NSScreen::visibleFrame` (#357).
        let _ = NSApplication::sharedApplication(marker);
        let screens = NSScreen::screens(marker);
        let primary_height = (0..screens.count())
            .map(|index| screens.objectAtIndex(index).frame())
            .find(|frame| frame.origin.x == 0.0 && frame.origin.y == 0.0)
            .map(|frame| frame.size.height)
            .ok_or(SnapshotError::MainScreenUnavailable)?;
        let mut displays = Vec::with_capacity(screens.count());
        let mut bounds = Vec::with_capacity(screens.count());
        for index in 0..screens.count() {
            let screen = screens.objectAtIndex(index);
            let frame = screen.frame();
            let display_id =
                screen_display_id(&screen).ok_or(SnapshotError::MainScreenUnavailable)?;
            displays.push(DisplayOutput {
                id: display_uuid(display_id).unwrap_or_else(|| display_id.to_string()),
                name: screen.localizedName().to_string(),
                frame: screen_output(&screen, primary_height),
                is_main: frame.origin.x == 0.0 && frame.origin.y == 0.0,
            });
            bounds.push(screen_rect(frame, primary_height));
        }
        DisplayOutputs::with_bounds(displays, bounds)
    })
}

/// Counts connected built-in and external displays via `CGDisplayIsBuiltin`. Main thread only;
/// returns `None` outside the main thread or when no screens are reported.
#[must_use]
pub fn display_topology() -> Option<DisplayTopology> {
    autoreleasepool(|_| {
        let marker = MainThreadMarker::new()?;
        let screens = NSScreen::screens(marker);
        let count = screens.count();
        if count == 0 {
            return None;
        }
        let mut flags = Vec::with_capacity(count);
        for index in 0..count {
            let screen = screens.objectAtIndex(index);
            let display_id = screen_display_id(&screen)?;
            // SAFETY: `CGDisplayIsBuiltin` queries Core Graphics display metadata for a display ID
            // and returns a `boolean_t` (0 = false, non-zero = true) without taking pointers.
            let is_builtin = unsafe { CGDisplayIsBuiltin(display_id) != 0 };
            flags.push(is_builtin);
        }
        Some(DisplayTopology::from_builtin_flags(flags))
    })
}

/// Each connected display's Core Graphics ID, keyed by the ID `outputs` gives it. Main thread
/// only; empty elsewhere.
#[must_use]
pub fn display_ids() -> HashMap<String, u32> {
    autoreleasepool(|_| {
        let Some(marker) = MainThreadMarker::new() else {
            return HashMap::new();
        };
        let screens = NSScreen::screens(marker);
        (0..screens.count())
            .filter_map(|index| screen_display_id(&screens.objectAtIndex(index)))
            .map(|display_id| {
                let id = display_uuid(display_id).unwrap_or_else(|| display_id.to_string());
                (id, display_id)
            })
            .collect()
    })
}

/// The ID `outputs` gives the display under the mouse pointer: the screen whose menu bar a
/// click on the status item came from. `None` off the main thread, when the pointer is on no
/// screen, or when macOS shows one menu bar for every display ("Displays have separate Spaces"
/// off), so the click says nothing about a display.
#[must_use]
pub fn display_under_pointer() -> Option<String> {
    autoreleasepool(|_| {
        let marker = MainThreadMarker::new()?;
        if !NSScreen::screensHaveSeparateSpaces(marker) {
            return None;
        }
        let pointer = NSEvent::mouseLocation();
        let screens = NSScreen::screens(marker);
        let screen = (0..screens.count())
            .map(|index| screens.objectAtIndex(index))
            .find(|screen| {
                let frame = screen.frame();
                pointer.x >= frame.origin.x
                    && pointer.x <= frame.origin.x + frame.size.width
                    && pointer.y >= frame.origin.y
                    && pointer.y <= frame.origin.y + frame.size.height
            })?;
        let display_id = screen_display_id(&screen)?;
        Some(display_uuid(display_id).unwrap_or_else(|| display_id.to_string()))
    })
}

pub(super) fn screen_display_id(screen: &NSScreen) -> Option<u32> {
    let description = screen.deviceDescription();
    let number = description.objectForKey(ns_string!("NSScreenNumber"))?;
    number
        .downcast_ref::<NSNumber>()
        .map(NSNumber::unsignedIntValue)
}

fn display_uuid(display_id: u32) -> Option<String> {
    // SAFETY: `CGDisplayCreateUUIDFromDisplayID` returns null or a +1 retained `CFUUIDRef`.
    let raw = unsafe { CGDisplayCreateUUIDFromDisplayID(display_id) };
    // SAFETY: `raw` is non-null and +1 retained under the Core Foundation Create rule.
    let uuid = unsafe { CFRetained::from_raw(NonNull::new(raw.cast_mut())?) };
    CFUUID::new_string(None, Some(&uuid)).map(|uuid| uuid.to_string())
}

fn screen_output(screen: &NSScreen, primary_height: f64) -> Rect {
    screen_rect(screen.visibleFrame(), primary_height)
}

fn screen_rect(rect: objc2_foundation::NSRect, primary_height: f64) -> Rect {
    Rect {
        x: rect.origin.x.round() as i32,
        y: (primary_height - rect.origin.y - rect.size.height).round() as i32,
        width: rect.size.width.round() as i32,
        height: rect.size.height.round() as i32,
    }
}

/// Non-negative distance along each axis from `(x, y)` to the closed pixel
/// region covered by `rect`.
fn rect_axis_distance(rect: Rect, x: i32, y: i32) -> (i64, i64) {
    let min_x = i64::from(rect.x);
    let max_x = min_x + i64::from(rect.width).saturating_sub(1).max(0);
    let min_y = i64::from(rect.y);
    let max_y = min_y + i64::from(rect.height).saturating_sub(1).max(0);
    let dx = if i64::from(x) < min_x {
        min_x - i64::from(x)
    } else if i64::from(x) > max_x {
        i64::from(x) - max_x
    } else {
        0
    };
    let dy = if i64::from(y) < min_y {
        min_y - i64::from(y)
    } else if i64::from(y) > max_y {
        i64::from(y) - max_y
    } else {
        0
    };
    (dx, dy)
}

fn union_rect(left: Rect, right: Rect) -> Rect {
    let x = left.x.min(right.x);
    let y = left.y.min(right.y);
    Rect {
        x,
        y,
        width: (left.x + left.width).max(right.x + right.width) - x,
        height: (left.y + left.height).max(right.y + right.height) - y,
    }
}

/// Whether `first` and `second` share some horizontal span, as stacked displays do.
const fn overlap_horizontally(first: Rect, second: Rect) -> bool {
    first.x < second.x + second.width && second.x < first.x + first.width
}

/// `name` without the " (1)" / " (2)" macOS appends to tell same-model displays apart.
fn product_name(name: &str) -> &str {
    name.rsplit_once(" (")
        .filter(|(_, suffix)| {
            suffix.strip_suffix(')').is_some_and(|digits| {
                !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())
            })
        })
        .map_or(name, |(base, _)| base)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn display(id: &str, name: &str, x: i32, is_main: bool) -> DisplayOutput {
        DisplayOutput {
            id: id.into(),
            name: name.into(),
            frame: Rect {
                x,
                y: 0,
                width: 100,
                height: 100,
            },
            is_main,
        }
    }

    fn three_displays() -> DisplayOutputs {
        DisplayOutputs::new(vec![
            display("built-in", "Built-in Retina Display", 0, true),
            display("studio-a", "Studio Display", 100, false),
            display("studio-b", "Studio Display", -100, false),
        ])
        .unwrap()
    }

    #[test]
    fn selector_for_names_main_secondary_and_every_further_display_by_id() {
        let outputs = DisplayOutputs::new(vec![
            display("built-in", "Built-in Retina Display", 0, true),
            display("studio-a", "Studio Display", 100, false),
            display("studio-b", "Studio Display", -100, false),
            display("studio-c", "Studio Display", 200, false),
        ])
        .unwrap();
        let selectors: Vec<String> = outputs
            .displays()
            .iter()
            .map(|display| outputs.selector_for(display).as_str().to_owned())
            .collect();
        assert_eq!(selectors, ["main", "tertiary", "secondary", "studio-c"]);
    }

    #[test]
    fn secondary_and_tertiary_are_the_non_main_displays_left_to_right() {
        let right = display("right", "Right", 200, false);
        let left = display("left", "Left", -100, false);
        let main = display("main", "Main", 0, true);
        for order in [
            vec![main.clone(), right.clone(), left.clone()],
            vec![right.clone(), left.clone(), main.clone()],
            vec![left.clone(), main.clone(), right.clone()],
        ] {
            let outputs = DisplayOutputs::new(order).unwrap();
            assert_eq!(outputs.secondary().unwrap().id, "left");
            assert_eq!(outputs.tertiary().unwrap().id, "right");
            assert_eq!(outputs.resolve(&DisplaySelector::Tertiary).id, "right");
        }
    }

    #[test]
    fn tertiary_falls_back_to_secondary_with_two_displays() {
        let outputs = DisplayOutputs::new(vec![
            display("main", "Main", 0, true),
            display("side", "Side", 100, false),
        ])
        .unwrap();
        assert!(outputs.tertiary().is_none());
        assert!(outputs.resolve_exact(&DisplaySelector::Tertiary).is_none());
        assert_eq!(outputs.resolve(&DisplaySelector::Tertiary).id, "side");
        let single = DisplayOutputs::new(vec![display("main", "Main", 0, true)]).unwrap();
        assert_eq!(single.resolve(&DisplaySelector::Tertiary).id, "main");
    }

    #[test]
    fn label_tells_same_named_displays_apart_by_position() {
        let outputs = DisplayOutputs::new(vec![
            display("built-in", "Built-in Retina Display", 0, true),
            display("dell-a", "DELL U2720Q (1)", -100, false),
            display("dell-b", "DELL U2720Q (2)", 100, false),
        ])
        .unwrap();
        let labels: Vec<String> = outputs
            .displays()
            .iter()
            .map(|display| outputs.label(display))
            .collect();
        assert_eq!(
            labels,
            [
                "Built-in Retina Display",
                "DELL U2720Q (left)",
                "DELL U2720Q (right)"
            ]
        );
        let stacked = DisplayOutputs::new(vec![
            display("upper", "Studio Display", 0, true),
            DisplayOutput {
                frame: Rect {
                    y: 100,
                    ..display("lower", "Studio Display", 0, false).frame
                },
                ..display("lower", "Studio Display", 0, false)
            },
        ])
        .unwrap();
        assert_eq!(
            stacked.label(&stacked.displays()[1]),
            "Studio Display (bottom)"
        );
        // Offset sideways but still above one another: a stack, not left and right.
        let offset = DisplayOutputs::new(vec![
            display("upper", "Studio Display", 0, true),
            DisplayOutput {
                frame: Rect {
                    x: 40,
                    y: 100,
                    ..display("lower", "Studio Display", 0, false).frame
                },
                ..display("lower", "Studio Display", 0, false)
            },
        ])
        .unwrap();
        assert_eq!(offset.label(&offset.displays()[0]), "Studio Display (top)");
        assert_eq!(
            offset.label(&offset.displays()[1]),
            "Studio Display (bottom)"
        );
        let tower = DisplayOutputs::new(
            [(0, 0), (10, 100), (-10, 200)]
                .into_iter()
                .enumerate()
                .map(|(index, (x, y))| DisplayOutput {
                    frame: Rect {
                        x,
                        y,
                        ..display("", "LG", 0, false).frame
                    },
                    ..display(
                        &format!("t{index}"),
                        &format!("LG ({})", index + 1),
                        0,
                        index == 0,
                    )
                })
                .collect(),
        )
        .unwrap();
        assert_eq!(tower.label(&tower.displays()[2]), "LG (3 from top)");
        let three = DisplayOutputs::new(vec![
            display("a", "LG (1)", 0, true),
            display("b", "LG (2)", 100, false),
            display("c", "LG (3)", 200, false),
        ])
        .unwrap();
        assert_eq!(three.label(&three.displays()[1]), "LG (2 from left)");
        assert_eq!(product_name("Display (Work)"), "Display (Work)");
    }

    #[test]
    fn ordered_lists_displays_left_to_right() {
        let outputs = three_displays();
        let ids: Vec<&str> = outputs
            .ordered()
            .iter()
            .map(|display| display.id.as_str())
            .collect();
        assert_eq!(ids, ["studio-b", "built-in", "studio-a"]);
    }

    #[test]
    fn display_at_matches_reserved_strips_with_bounds_and_synthetic_frames() {
        let visible = Rect {
            x: 0,
            y: 25,
            width: 1000,
            height: 750,
        };
        let second_visible = Rect { x: 1000, ..visible };
        let synthetic = DisplayOutputs::new(vec![
            DisplayOutput {
                id: "main".into(),
                name: "Main".into(),
                frame: visible,
                is_main: true,
            },
            DisplayOutput {
                id: "second".into(),
                name: "Second".into(),
                frame: second_visible,
                is_main: false,
            },
        ])
        .unwrap();
        assert_eq!(
            synthetic.display_at((500, 10)).map(|d| d.id.as_str()),
            Some("main")
        );
        assert_eq!(
            synthetic.display_at((500, 790)).map(|d| d.id.as_str()),
            Some("main")
        );
        assert_eq!(
            synthetic.display_at((1500, 10)).map(|d| d.id.as_str()),
            Some("second")
        );
        assert_eq!(
            synthetic.display_at((1500, 790)).map(|d| d.id.as_str()),
            Some("second")
        );
        assert_eq!(synthetic.display_at((5000, 5000)), None);
        assert_eq!(synthetic.display_for_point((1500, 10)).id, "second");

        // Full physical bounds also resolve asymmetric Docks unambiguously (e.g. a
        // bottom Dock on main adjacent to a full-height secondary display).
        let with_bounds = DisplayOutputs::with_bounds(
            vec![
                DisplayOutput {
                    id: "main".into(),
                    name: "Main".into(),
                    frame: Rect {
                        x: 0,
                        y: 25,
                        width: 1000,
                        height: 700,
                    },
                    is_main: true,
                },
                DisplayOutput {
                    id: "second".into(),
                    name: "Second".into(),
                    frame: Rect {
                        x: 1000,
                        y: 25,
                        width: 1000,
                        height: 775,
                    },
                    is_main: false,
                },
            ],
            vec![
                Rect {
                    x: 0,
                    y: 0,
                    width: 1000,
                    height: 800,
                },
                Rect {
                    x: 1000,
                    y: 0,
                    width: 1000,
                    height: 800,
                },
            ],
        )
        .unwrap();
        assert_eq!(
            with_bounds.display_at((990, 750)).map(|d| d.id.as_str()),
            Some("main")
        );
        assert_eq!(
            with_bounds.display_at((1000, 10)).map(|d| d.id.as_str()),
            Some("second")
        );
        assert_eq!(
            with_bounds.display_at((2000, 800)).map(|d| d.id.as_str()),
            Some("second")
        );
        assert_eq!(with_bounds.display_at((2010, 800)), None);
    }

    #[test]
    fn screen_rect_converts_visible_frames_excluding_menu_bars_on_side_displays() {
        use objc2_foundation::{NSPoint, NSRect, NSSize};

        // Main 2560x1440 with 30pt menu bar flanked by two 1440x2560 portrait displays (#357).
        let primary_height = 1440.0;
        let rect = |x, y, width, height| {
            screen_rect(
                NSRect::new(NSPoint::new(x, y), NSSize::new(width, height)),
                primary_height,
            )
        };
        assert_eq!(
            rect(0.0, 0.0, 2560.0, 1410.0),
            Rect {
                x: 0,
                y: 30,
                width: 2560,
                height: 1410,
            }
        );
        // Side displays' visible frames exclude their 30pt menu bars just like main.
        assert_eq!(
            rect(-1440.0, -507.0, 1440.0, 2530.0),
            Rect {
                x: -1440,
                y: -583,
                width: 1440,
                height: 2530,
            }
        );
        assert_eq!(
            rect(2560.0, -551.0, 1440.0, 2530.0),
            Rect {
                x: 2560,
                y: -539,
                width: 1440,
                height: 2530,
            }
        );
    }

    #[test]
    fn display_topology_distinguishes_laptop_builtin_clamshell_and_desktop_macs() {
        // MacBook alone: built-in only.
        let laptop_only = DisplayTopology::from_builtin_flags([true]);
        assert!(laptop_only.is_builtin_only());
        assert!(!laptop_only.has_external());

        // MacBook + external monitor: not built-in only, has external.
        let laptop_docked = DisplayTopology::from_builtin_flags([true, false]);
        assert!(!laptop_docked.is_builtin_only());
        assert!(laptop_docked.has_external());

        // MacBook in clamshell mode with one external monitor.
        let clamshell = DisplayTopology::from_builtin_flags([false]);
        assert!(!clamshell.is_builtin_only());
        assert!(clamshell.has_external());

        // Mac Studio / Mac mini with 1 or 2 external monitors: 0 built-in displays.
        let studio_single = DisplayTopology::from_builtin_flags([false]);
        let studio_dual = DisplayTopology::from_builtin_flags([false, false]);
        assert!(!studio_single.is_builtin_only());
        assert!(!studio_dual.is_builtin_only());

        // Empty / headless: neither built-in only nor external.
        let empty = DisplayTopology::from_builtin_flags([]);
        assert!(!empty.is_builtin_only());
        assert!(!empty.has_external());
    }
}
