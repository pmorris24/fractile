//! The displays Settings › Workspaces offers: every connected screen, its friendly label, and
//! the exact `config.toml` selector the engine resolves (the same one Option+Shift+Tab writes).

use fractile::Rect;
use fractile::macos::DisplayOutputs;

/// The built-in selector for the display macOS marks as main.
pub(crate) const MAIN: &str = "main";
/// The built-in selector for the leftmost non-main display.
pub(crate) const SECONDARY: &str = "secondary";
/// The built-in selector for the next non-main display. With no such display it falls back to
/// Secondary, as `DisplayOutputs::resolve` does.
pub(crate) const TERTIARY: &str = "tertiary";
/// The longest display name a picker segment shows. A longer one ("Built-in Retina Display")
/// gets its position in the segment and keeps its name in its card's heading.
const SEGMENT_NAME_MAX: usize = 16;

/// One connected display, read from `fractile::macos::outputs`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ConnectedDisplay {
    /// `main`, `secondary`, or the display's UUID.
    pub selector: String,
    /// The display's own ID, as `DisplayOutput::id` gives it.
    pub id: String,
    /// The display's own name, as macOS reports it ("Built-in Retina Display").
    pub name: String,
    pub frame: Rect,
    pub is_main: bool,
    pub is_secondary: bool,
}

impl ConnectedDisplay {
    fn is_built_in(&self) -> bool {
        self.is_main || self.is_secondary
    }
}

/// Every connected display with the selector Settings writes for it.
pub(crate) fn connected(outputs: &DisplayOutputs) -> Vec<ConnectedDisplay> {
    outputs
        .displays()
        .iter()
        .map(|display| {
            let selector = outputs.selector_for(display).as_str().to_owned();
            ConnectedDisplay {
                is_secondary: selector == SECONDARY,
                selector,
                id: display.id.clone(),
                name: display.name.clone(),
                frame: display.frame,
                is_main: display.is_main,
            }
        })
        .collect()
}

/// One segment of a workspace's display picker and one section of the pane.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Choice {
    /// The selector written to `config.toml` when this choice is picked.
    pub selector: String,
    /// The picker segment's label, kept short so segments never overflow: the display's name,
    /// or its position ("Display 3") when the name is long or four displays share the picker.
    pub label: String,
    /// The section heading above the workspaces assigned to it: the display's own name at any
    /// length, so a numbered segment always has a card that says which screen it is.
    pub title: String,
    /// How a row's `Default:` note names this display: the built-in word (`Main`, `Secondary`)
    /// while the picker uses it, else the display's own name, so the note points at a card the
    /// reader can find.
    pub note: String,
    /// The heading's trailing caption.
    pub caption: String,
    /// The display's UUID, so a saved `monitor` naming it resolves here.
    pub id: Option<String>,
    /// The display's name, so a saved `monitor` naming it resolves here.
    pub name: Option<String>,
    /// Whether a display is plugged in for this choice.
    pub connected: bool,
    /// The display's position in discovery order (`DisplayOutputs::displays`), which decides
    /// which display a name shared by several screens resolves to.
    pub discovery: Option<usize>,
}

impl Choice {
    /// Whether the `monitor` value `selector` names this display by UUID or name.
    fn names(&self, selector: &str) -> bool {
        self.id.as_deref() == Some(selector) || self.name.as_deref() == Some(selector)
    }
}

/// The choice `selector` (a `config.toml` `monitor` value) resolves to, if any display here
/// answers to it, exactly as `DisplayOutputs::resolve_exact` does: a built-in selector first,
/// then the first display in discovery order whose UUID or name it is. Choices are sorted by
/// position, so a name two screens share must not resolve to the leftmost one. Issue 420.
pub(crate) fn index_of(choices: &[Choice], selector: &str) -> Option<usize> {
    if let Some(index) = choices
        .iter()
        .position(|choice| choice.selector == selector)
    {
        return Some(index);
    }
    if selector == TERTIARY {
        // No third display: tertiary workspaces show on Secondary.
        return index_of(choices, SECONDARY);
    }
    choices
        .iter()
        .enumerate()
        .filter(|(_, choice)| choice.names(selector))
        .min_by_key(|(_, choice)| choice.discovery)
        .map(|(index, _)| index)
}

/// Main, then Secondary, then every further display left to right. Main and Secondary are always
/// offered, even when nothing is plugged into them, because workspaces fall back to main.
pub(crate) fn choices(displays: &[ConnectedDisplay]) -> Vec<Choice> {
    let discovery = |display: &ConnectedDisplay| {
        displays
            .iter()
            .position(|other| std::ptr::eq(other, display))
    };
    let mut ordered: Vec<&ConnectedDisplay> = displays.iter().collect();
    ordered.sort_by(|left, right| {
        rank(left).cmp(&rank(right)).then_with(|| {
            (left.frame.x, left.frame.y, left.selector.as_str()).cmp(&(
                right.frame.x,
                right.frame.y,
                right.selector.as_str(),
            ))
        })
    });
    let main = ordered.iter().copied().find(|display| display.is_main);
    let secondary = ordered.iter().copied().find(|display| display.is_secondary);
    let mut choices = vec![
        built_in(MAIN, "Main", "Main display", main),
        built_in(SECONDARY, "Secondary", "Secondary display", secondary),
    ];
    choices[0].discovery = main.and_then(discovery);
    choices[1].discovery = secondary.and_then(discovery);
    choices[0].caption = match &main {
        Some(display) if !display.name.is_empty() => {
            format!("{} · Default for 1–4", display.name)
        }
        _ => "Default for 1–4".to_owned(),
    };
    // Secondary also hosts 8–10 until a tertiary display is plugged in.
    let has_tertiary = ordered.iter().any(|display| display.selector == TERTIARY);
    let secondary_range = if has_tertiary { "5–7" } else { "5–10" };
    choices[1].caption = match &secondary {
        Some(display) if !display.name.is_empty() => {
            format!("{} · Default for {secondary_range}", display.name)
        }
        Some(_) => format!("Default for {secondary_range}"),
        None => format!("Default for {secondary_range} · main when unplugged"),
    };
    let extras: Vec<&ConnectedDisplay> = ordered
        .iter()
        .copied()
        .filter(|display| !display.is_built_in())
        .collect();
    for display in &extras {
        let position = choices.len() + 1;
        let title = extra_name(position, display, &ordered);
        let label = extra_label(position, &title, ordered.len());
        choices.push(Choice {
            selector: display.selector.clone(),
            title: title.clone(),
            note: title,
            label,
            caption: if display.selector == TERTIARY {
                format!("Display {position} · Default for 8–10")
            } else {
                format!("Display {position}")
            },
            id: Some(display.id.clone()),
            name: Some(display.name.clone()),
            connected: true,
            discovery: discovery(display),
        });
    }
    // Past two displays the picker names every screen, so Secondary names its own too: with two
    // identically named monitors that reads "Studio Display 1" | "Studio Display 2" rather than
    // "Secondary" | "Studio Display 2", and a row saved by its UUID says the same thing the card
    // and the picker do.
    if !extras.is_empty()
        && let Some(display) = secondary
    {
        let title = extra_name(2, display, &ordered);
        let label = extra_label(2, &title, ordered.len());
        // A numbered segment is found by its number, so the card says the number too.
        let role = if label == title { "Secondary" } else { &label };
        choices[1].caption = format!("{role} · Default for {secondary_range}");
        choices[1].label = label;
        choices[1].note = title.clone();
        choices[1].title = title;
    }
    choices
}

fn rank(display: &ConnectedDisplay) -> u8 {
    if display.is_main {
        0
    } else if display.is_secondary {
        1
    } else {
        2
    }
}

fn built_in(
    selector: &str,
    label: &str,
    title: &str,
    display: Option<&ConnectedDisplay>,
) -> Choice {
    Choice {
        selector: selector.to_owned(),
        label: label.to_owned(),
        title: title.to_owned(),
        note: label.to_owned(),
        caption: String::new(),
        id: display.map(|display| display.id.clone()),
        name: display.map(|display| display.name.clone()),
        connected: display.is_some(),
        discovery: None,
    }
}

/// The heading for a display past Main: its own name at any length, "Studio Display 2" when
/// two monitors share one, and its position ("Display 3") only when it has no name. Issue 349.
fn extra_name(
    position: usize,
    display: &ConnectedDisplay,
    displays: &[&ConnectedDisplay],
) -> String {
    let name = display.name.trim();
    let same: Vec<&&ConnectedDisplay> = displays
        .iter()
        .filter(|other| other.name.trim() == name)
        .collect();
    if same.len() > 1 && !name.is_empty() {
        let ordinal = same
            .iter()
            .position(|other| other.selector == display.selector)
            .unwrap_or_default()
            + 1;
        return format!("{name} {ordinal}");
    }
    if name.is_empty() {
        format!("Display {position}")
    } else {
        name.to_owned()
    }
}

/// The picker segment for a display past Main. A long name, or any name once four displays
/// share the segmented control, would crowd the row's title, so the segment gets the display's
/// position and the heading keeps the name.
fn extra_label(position: usize, name: &str, displays: usize) -> String {
    if displays > 3 || name.chars().count() > SEGMENT_NAME_MAX {
        format!("Display {position}")
    } else {
        name.to_owned()
    }
}

/// Two displays, as a fresh Mac reports them: the built-in main and one Studio Display.
pub(crate) fn fixture_pair() -> Vec<ConnectedDisplay> {
    vec![
        ConnectedDisplay {
            selector: MAIN.to_owned(),
            id: "37D8832A-2D66-02CA-B9F7-8F30A301B230".to_owned(),
            name: "Built-in Retina Display".to_owned(),
            frame: Rect {
                x: 0,
                y: 0,
                width: 1728,
                height: 1117,
            },
            is_main: true,
            is_secondary: false,
        },
        ConnectedDisplay {
            selector: SECONDARY.to_owned(),
            id: "4A1B2C3D-5E6F-7081-92A3-B4C5D6E7F801".to_owned(),
            name: "Studio Display".to_owned(),
            frame: Rect {
                x: 1728,
                y: 0,
                width: 2560,
                height: 1440,
            },
            is_main: false,
            is_secondary: true,
        },
    ]
}

/// The third display's UUID in the Playground's three-display scene.
pub(crate) const THIRD_FIXTURE_ID: &str = "8F14E45F-EA61-4B2A-9C3D-1B2C3D4E5F60";

/// The fourth display's UUID in the Playground's four-display scene.
pub(crate) const FOURTH_FIXTURE_ID: &str = "A1B2C3D4-E5F6-7081-92A3-B4C5D6E7F809";

/// Three displays: the built-in main and two identically named external monitors.
pub(crate) fn fixture_three() -> Vec<ConnectedDisplay> {
    let pair = fixture_pair();
    vec![
        pair[0].clone(),
        pair[1].clone(),
        ConnectedDisplay {
            selector: TERTIARY.to_owned(),
            id: THIRD_FIXTURE_ID.to_owned(),
            name: "Studio Display".to_owned(),
            frame: Rect {
                x: -2560,
                y: 0,
                width: 2560,
                height: 1440,
            },
            is_main: false,
            is_secondary: false,
        },
    ]
}

/// Three displays where a MacBook's own screen is the third: a Studio Display as main, an LG
/// as Secondary, and "Built-in Retina Display", whose name is too long for a segment.
pub(crate) fn fixture_long_name() -> Vec<ConnectedDisplay> {
    let mut displays = fixture_three();
    for (display, name) in displays.iter_mut().zip([
        "Studio Display",
        "LG UltraFine 5K",
        "Built-in Retina Display",
    ]) {
        name.clone_into(&mut display.name);
    }
    displays
}

/// Four displays: the three-display pair plus a differently named external on the right.
pub(crate) fn fixture_quad() -> Vec<ConnectedDisplay> {
    let mut displays = fixture_three();
    displays.push(ConnectedDisplay {
        selector: FOURTH_FIXTURE_ID.to_owned(),
        id: FOURTH_FIXTURE_ID.to_owned(),
        name: "LG UltraFine 5K".to_owned(),
        frame: Rect {
            x: 4288,
            y: 0,
            width: 2560,
            height: 1440,
        },
        is_main: false,
        is_secondary: false,
    });
    displays
}
