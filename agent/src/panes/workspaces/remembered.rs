//! The displays Settings › Screens remembers after they are unplugged, so the arrangement keeps
//! every screen in the place it had, with its workspaces, until it is plugged back in.
//!
//! The record is the last arrangement seen: a set of displays and their frames. Unplugging some
//! of them leaves it as it is, so the panel keeps drawing that desk, each screen where it was
//! and each workspace on the screen it returns to. Any other set, more displays or different
//! ones, replaces it.
//!
//! When the record has no unplugged screen to show (Settings never saw the desk, or saw only
//! the laptop), it takes the desk macOS remembers around the connected displays: the window
//! server keeps every set of displays it has arranged, with each screen's place, in a
//! preferences file of its own. That file is private and undocumented, so it is only a fallback:
//! when it moves or changes shape the record is Fractile's own alone. A forgotten display stays
//! out of both until it is plugged in again.

use std::path::PathBuf;

use fractile::Rect;

use fractile::macos::{DisplayOutput, DisplayOutputs};

use super::{ConnectedDisplay, displays};

/// The file next to `config.toml` that holds the record, one display per line.
const FILE: &str = "known-displays.txt";
/// The first field of a line naming a forgotten display.
const FORGOTTEN: &str = "forgotten";

/// The known arrangement, and the displays forgotten from it, which the desk macOS remembers
/// must not bring back.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Record {
    pub(crate) displays: Vec<ConnectedDisplay>,
    pub(crate) forgotten: Vec<String>,
}

impl Record {
    /// Takes in a read of the `connected` displays. When the record has no unplugged display
    /// to show, and `look` says the displays just changed, it asks `macos` (given the forgotten
    /// IDs) for the desk macOS remembers around them. Returns whether the record changed.
    pub(crate) fn update(
        &mut self,
        connected: &[ConnectedDisplay],
        look: bool,
        macos: impl FnOnce(&[ConnectedDisplay], &[String]) -> Option<Vec<ConnectedDisplay>>,
    ) -> bool {
        // A forgotten display plugged in again is part of the desk again.
        let before = self.forgotten.len();
        self.forgotten
            .retain(|id| !connected.iter().any(|display| display.id == *id));
        let mut changed = self.forgotten.len() != before;
        changed |= note(&mut self.displays, connected);
        if look
            && away(&self.displays, connected).is_empty()
            && let Some(desk) = macos(connected, &self.forgotten)
        {
            self.displays = desk;
            changed = true;
        }
        changed
    }

    /// Forgets the unplugged display `id`, so neither the record nor macOS's shows it again
    /// until it is plugged in. Returns whether it was in the record.
    pub(crate) fn forget(&mut self, id: &str) -> bool {
        let before = self.displays.len();
        self.displays.retain(|display| display.id != id);
        if !self.forgotten.iter().any(|other| other == id) {
            self.forgotten.push(id.to_owned());
        }
        self.displays.len() != before
    }
}

/// The displays `known` holds, or none before it is read.
pub(crate) fn displays(known: Option<&Record>) -> &[ConnectedDisplay] {
    known.map_or(&[], |record| record.displays.as_slice())
}

/// Records `connected` as the arrangement unless it is part of the one `known` holds. Returns
/// whether the record changed.
pub(crate) fn note(known: &mut Vec<ConnectedDisplay>, connected: &[ConnectedDisplay]) -> bool {
    if connected.is_empty() {
        return false;
    }
    let part_of_known = connected.len() < known.len()
        && connected
            .iter()
            .all(|display| known.iter().any(|other| other.id == display.id));
    if part_of_known || known.as_slice() == connected {
        return false;
    }
    *known = connected.to_vec();
    true
}

/// The IDs of the known displays that are unplugged, while the connected ones are all part of
/// the known arrangement; none when a screen it never saw is plugged in.
pub(crate) fn away(known: &[ConnectedDisplay], connected: &[ConnectedDisplay]) -> Vec<String> {
    let in_record = connected
        .iter()
        .all(|display| known.iter().any(|other| other.id == display.id));
    if !in_record {
        return Vec::new();
    }
    known
        .iter()
        .filter(|display| !connected.iter().any(|other| other.id == display.id))
        .map(|display| display.id.clone())
        .collect()
}

/// Where the window server keeps the display sets it has arranged.
const MACOS_FILE: &str = "/Library/Preferences/com.apple.windowserver.displays.plist";

/// The desk macOS remembers around the `connected` displays, less the `forgotten` ones: the first set it lists (it keeps
/// the latest first) holding every connected display and more besides. Names come from the
/// connected displays; macOS keeps none for the others.
pub(crate) fn from_macos(
    connected: &[ConnectedDisplay],
    forgotten: &[String],
) -> Option<Vec<ConnectedDisplay>> {
    let sets: Vec<Vec<(String, Rect)>> = display_sets(&read_macos()?)
        .into_iter()
        .map(|set| {
            set.into_iter()
                .filter(|(id, _)| !forgotten.contains(id))
                .collect()
        })
        .collect();
    pick(&sets, connected)
}

#[cfg(target_os = "macos")]
fn read_macos() -> Option<serde_json::Value> {
    let output = std::process::Command::new("/usr/bin/plutil")
        .args(["-convert", "json", "-o", "-", MACOS_FILE])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    serde_json::from_slice(&output.stdout).ok()
}

#[cfg(not(target_os = "macos"))]
fn read_macos() -> Option<serde_json::Value> {
    None
}

/// Every display set in the window server's file: each display's UUID and frame, y down from
/// the main display's top-left as Core Graphics has it.
fn display_sets(plist: &serde_json::Value) -> Vec<Vec<(String, Rect)>> {
    let Some(configs) = plist
        .pointer("/DisplaySets/Configs")
        .and_then(serde_json::Value::as_array)
    else {
        return Vec::new();
    };
    configs
        .iter()
        .filter_map(|config| {
            config
                .get("DisplayConfig")?
                .as_array()?
                .iter()
                .map(|display| {
                    let id = display.get("UUID")?.as_str()?.to_owned();
                    let info = display.get("CurrentInfo")?;
                    let number = |key: &str| {
                        let value = info.get(key)?.as_f64()?;
                        #[allow(clippy::cast_possible_truncation)]
                        Some(value.round() as i32)
                    };
                    let frame = Rect {
                        x: number("OriginX")?,
                        y: number("OriginY")?,
                        width: number("Wide")?,
                        height: number("High")?,
                    };
                    Some((id, frame))
                })
                .collect::<Option<Vec<_>>>()
        })
        .collect()
}

/// The first set holding every connected display and more, whose screens sit side by side
/// (a mirrored set stacks them on one spot) with one at the origin, which macOS makes main.
fn pick(
    sets: &[Vec<(String, Rect)>],
    connected: &[ConnectedDisplay],
) -> Option<Vec<ConnectedDisplay>> {
    let overlap = |a: Rect, b: Rect| {
        a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height
    };
    let set = sets.iter().find(|set| {
        set.len() > connected.len()
            && connected
                .iter()
                .all(|display| set.iter().any(|(id, _)| *id == display.id))
            && set
                .iter()
                .filter(|(_, frame)| frame.x == 0 && frame.y == 0)
                .count()
                == 1
            && set
                .iter()
                .enumerate()
                .all(|(i, (_, a))| set.iter().skip(i + 1).all(|(_, b)| !overlap(*a, *b)))
    })?;
    let outputs = DisplayOutputs::new(
        set.iter()
            .map(|(id, frame)| DisplayOutput {
                id: id.clone(),
                name: connected
                    .iter()
                    .find(|display| display.id == *id)
                    .map(|display| display.name.clone())
                    .unwrap_or_default(),
                frame: *frame,
                is_main: frame.x == 0 && frame.y == 0,
            })
            .collect(),
    )
    .ok()?;
    Some(displays::connected(&outputs))
}

fn path() -> Option<PathBuf> {
    fractile::config::application_support_path(FILE)
}

/// The record saved on disk, or none when there is no file or it does not read.
pub(crate) fn load() -> Record {
    path()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .map(|text| parse(&text))
        .unwrap_or_default()
}

/// Saves the record; a failure leaves the old one, which only costs a stale picture.
pub(crate) fn save(known: &Record) {
    let Some(path) = path() else {
        return;
    };
    let written = path
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| std::fs::write(&path, write(known)));
    if let Err(error) = written {
        eprintln!("fractile: could not save {}: {error}", path.display());
    }
}

/// One line a display: id, selector, x, y, width, height, main, secondary, name; tab-separated,
/// the name last so it may hold anything but a newline. Then a `forgotten` line per forgotten ID.
fn write(known: &Record) -> String {
    let forgotten = known
        .forgotten
        .iter()
        .map(|id| format!("{FORGOTTEN}\t{}\n", id.replace(['\t', '\n'], " ")));
    known
        .displays
        .iter()
        .map(|display| {
            let f = display.frame;
            format!(
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                display.id.replace(['\t', '\n'], " "),
                display.selector.replace(['\t', '\n'], " "),
                f.x,
                f.y,
                f.width,
                f.height,
                u8::from(display.is_main),
                u8::from(display.is_secondary),
                display.name.replace(['\t', '\n'], " "),
            )
        })
        .chain(forgotten)
        .collect()
}

fn parse(text: &str) -> Record {
    let forgotten = text
        .lines()
        .filter_map(|line| line.strip_prefix(FORGOTTEN)?.strip_prefix('\t'))
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
        .collect();
    let displays = text
        .lines()
        .filter(|line| !line.starts_with(FORGOTTEN))
        .filter_map(|line| {
            let mut field = line.splitn(9, '\t');
            let id = field.next()?.to_owned();
            let selector = field.next()?.to_owned();
            let mut number = || field.next()?.parse::<i32>().ok();
            let frame = Rect {
                x: number()?,
                y: number()?,
                width: number()?,
                height: number()?,
            };
            let is_main = number()? == 1;
            let is_secondary = number()? == 1;
            let name = field.next()?.to_owned();
            (!id.is_empty() && frame.width > 0 && frame.height > 0).then_some(ConnectedDisplay {
                selector,
                id,
                name,
                frame,
                is_main,
                is_secondary,
            })
        })
        .collect();
    Record {
        displays,
        forgotten,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn display(id: &str, x: i32, y: i32, main: bool) -> ConnectedDisplay {
        ConnectedDisplay {
            selector: if main { "main".into() } else { id.into() },
            id: id.into(),
            name: format!("Screen {id}"),
            frame: Rect {
                x,
                y,
                width: 1000,
                height: 600,
            },
            is_main: main,
            is_secondary: false,
        }
    }

    #[test]
    fn unplugging_keeps_the_arrangement_and_plugging_a_new_screen_replaces_it() {
        let desk = vec![
            display("studio", 0, 0, true),
            display("laptop", -1000, 300, false),
        ];
        let mut known = Vec::new();
        assert!(note(&mut known, &desk));
        // Unplugged, the laptop is main at the origin; the record keeps the desk.
        let alone = vec![display("laptop", 0, 0, true)];
        assert!(!note(&mut known, &alone));
        assert_eq!(known, desk);
        assert_eq!(away(&known, &alone), vec!["studio".to_owned()]);
        // Plugged back in, nothing is away.
        assert!(!note(&mut known, &desk));
        assert!(away(&known, &desk).is_empty());
        // A different desk replaces the record.
        let hotel = vec![display("laptop", 0, 0, true), display("tv", 1000, 0, false)];
        assert!(note(&mut known, &hotel));
        assert_eq!(known, hotel);
    }

    #[test]
    fn a_screen_the_record_never_saw_shows_only_whats_connected() {
        let known = vec![
            display("studio", 0, 0, true),
            display("side", 1000, -200, false),
        ];
        assert!(away(&known, &[display("tv", 0, 0, true)]).is_empty());
    }

    #[test]
    fn the_desk_macos_remembers_seeds_the_record() {
        let plist = serde_json::json!({ "DisplaySets": { "Configs": [
            // Mirrored: both on one spot.
            { "DisplayConfig": [
                { "UUID": "laptop", "CurrentInfo": { "OriginX": 0, "OriginY": 0, "Wide": 1512, "High": 982 } },
                { "UUID": "tv", "CurrentInfo": { "OriginX": 0, "OriginY": 0, "Wide": 1920, "High": 1080 } },
            ] },
            { "DisplayConfig": [
                { "UUID": "laptop", "CurrentInfo": { "OriginX": -1800, "OriginY": 358, "Wide": 1800, "High": 1169 } },
                { "UUID": "studio", "CurrentInfo": { "OriginX": 0, "OriginY": 0, "Wide": 3360, "High": 1890 } },
                { "UUID": "side", "CurrentInfo": { "OriginX": 3360, "OriginY": -508, "Wide": 2560, "High": 1440 } },
            ] },
            { "DisplayConfig": [
                { "UUID": "laptop", "CurrentInfo": { "OriginX": 0, "OriginY": 0, "Wide": 1512, "High": 982 } },
            ] },
        ] } });
        let sets = display_sets(&plist);
        assert_eq!(sets.len(), 3);
        let mut laptop = display("laptop", 0, 0, true);
        laptop.name = "Built-in Retina Display".into();
        let desk = pick(&sets, std::slice::from_ref(&laptop)).unwrap();
        assert_eq!(desk.len(), 3);
        let studio = desk.iter().find(|d| d.id == "studio").unwrap();
        assert!(studio.is_main);
        assert_eq!(studio.selector, "main");
        let seen = desk.iter().find(|d| d.id == "laptop").unwrap();
        assert_eq!(seen.name, "Built-in Retina Display");
        assert_eq!((seen.frame.x, seen.frame.y), (-1800, 358));
        assert!(!seen.is_main);
        // The laptop alone is part of it, so the record keeps the desk.
        let mut known = desk.clone();
        assert!(!note(&mut known, &[laptop]));
        assert_eq!(away(&known, &[display("laptop", 0, 0, true)]).len(), 2);
        // No set holds more than what is connected, or the file is not there: no head start.
        assert!(pick(&sets, &desk).is_none());
        assert!(display_sets(&serde_json::json!({})).is_empty());
    }

    #[test]
    fn the_record_reads_back_what_it_wrote_and_forgets_on_request() {
        let mut known = Record {
            displays: vec![
                display("studio", 0, 0, true),
                display("side", 1000, -200, false),
            ],
            forgotten: Vec::new(),
        };
        known.displays[1].name = "LG\tUltraFine".into();
        assert!(known.forget("side"));
        assert!(!known.forget("side"));
        assert_eq!(known.displays.len(), 1);
        assert_eq!(known.forgotten, vec!["side".to_owned()]);
        let back = parse(&write(&known));
        assert_eq!(back, known);
        let mut named = known.clone();
        named.displays.push(display("x", 0, 0, false));
        named.displays[1].name = "LG\tUltraFine".into();
        assert_eq!(parse(&write(&named)).displays[1].name, "LG UltraFine");
        assert_eq!(parse("garbage\n\n"), Record::default());
    }

    #[test]
    fn a_record_of_the_laptop_alone_takes_the_desk_macos_remembers_but_not_a_forgotten_screen() {
        let laptop = display("laptop", 0, 0, true);
        let desk = vec![
            display("studio", 0, 0, true),
            display("laptop", -1000, 300, false),
        ];
        // Settings saw only the laptop before it could ask macOS.
        let mut known = Record {
            displays: vec![laptop.clone()],
            forgotten: Vec::new(),
        };
        let macos = |forgotten: &[String]| {
            let desk: Vec<ConnectedDisplay> = desk
                .iter()
                .filter(|display| !forgotten.contains(&display.id))
                .cloned()
                .collect();
            (desk.len() > 1).then_some(desk)
        };
        let one = std::slice::from_ref(&laptop);
        assert!(known.update(one, true, |_, forgotten| macos(forgotten)));
        assert_eq!(known.displays, desk);
        assert_eq!(away(&known.displays, one), vec!["studio".to_owned()]);
        // While a screen is away, macOS is not asked.
        assert!(!known.update(one, true, |_, _| panic!("asked macOS")));
        // Forgotten, the studio stays gone, even from macOS's desk.
        assert!(known.forget("studio"));
        known.update(one, true, |_, forgotten| macos(forgotten));
        assert!(away(&known.displays, one).is_empty());
        assert_eq!(known.displays, vec![laptop.clone()]);
        // Plugged in again, it is remembered again.
        assert!(known.update(&desk, true, |_, _| None));
        assert!(known.forgotten.is_empty());
        assert_eq!(known.displays, desk);
    }
}
