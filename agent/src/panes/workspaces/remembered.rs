//! The displays Settings › Screens remembers after they are unplugged, so the arrangement keeps
//! every screen in the place it had, with its workspaces, until it is plugged back in.
//!
//! The record is the last arrangement seen: a set of displays and their frames. Unplugging some
//! of them leaves it as it is, so the panel keeps drawing that desk, each screen where it was
//! and each workspace on the screen it returns to. Any other set, more displays or different
//! ones, replaces it.

use std::path::PathBuf;

use spaceman::Rect;

use super::ConnectedDisplay;

/// The file next to `config.toml` that holds the record, one display per line.
const FILE: &str = "known-displays.txt";

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

/// Forgets the unplugged display `id`. Returns whether it was in the record.
pub(crate) fn forget(known: &mut Vec<ConnectedDisplay>, id: &str) -> bool {
    let before = known.len();
    known.retain(|display| display.id != id);
    known.len() != before
}

fn path() -> Option<PathBuf> {
    spaceman::config::application_support_path(FILE)
}

/// The record saved on disk, or none when there is no file or it does not read.
pub(crate) fn load() -> Vec<ConnectedDisplay> {
    path()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .map(|text| parse(&text))
        .unwrap_or_default()
}

/// Saves the record; a failure leaves the old one, which only costs a stale picture.
pub(crate) fn save(known: &[ConnectedDisplay]) {
    let Some(path) = path() else {
        return;
    };
    let written = path
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| std::fs::write(&path, write(known)));
    if let Err(error) = written {
        eprintln!("spaceman: could not save {}: {error}", path.display());
    }
}

/// One line a display: id, selector, x, y, width, height, main, secondary, name; tab-separated,
/// the name last so it may hold anything but a newline.
fn write(known: &[ConnectedDisplay]) -> String {
    known
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
        .collect()
}

fn parse(text: &str) -> Vec<ConnectedDisplay> {
    text.lines()
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
        .collect()
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
    fn the_record_reads_back_what_it_wrote_and_forgets_on_request() {
        let mut known = vec![
            display("studio", 0, 0, true),
            display("side", 1000, -200, false),
        ];
        known[1].name = "LG\tUltraFine".into();
        let back = parse(&write(&known));
        assert_eq!(back[0], known[0]);
        assert_eq!(back[1].name, "LG UltraFine");
        assert!(parse("garbage\n\n").is_empty());
        assert!(forget(&mut known, "side"));
        assert!(!forget(&mut known, "side"));
        assert_eq!(known.len(), 1);
    }
}
