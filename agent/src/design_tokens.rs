//! The design-token ratchet: counts hand-typed measurements in the Settings UI and fails when a
//! file has more than its ceiling in `scripts/design-token-exceptions.txt`. Ceilings only go
//! down. See `docs/design-system.md`.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

const EXCEPTIONS: &str = "scripts/design-token-exceptions.txt";
const UPDATE_ENV: &str = "SPACEMAN_UPDATE_TOKEN_CEILINGS";

/// The spacing scale. A literal outside it is a one-off.
const SCALE: [f64; 13] = [
    0.0, 1.0, 2.0, 4.0, 6.0, 8.0, 12.0, 16.0, 20.0, 24.0, 32.0, 40.0, 48.0,
];
const SPACING: [&str; 16] = [
    "gap", "px", "py", "pt", "pb", "pl", "pr", "p", "m", "mt", "mb", "ml", "mr", "mx", "my",
    "padding",
];
/// Kit widgets the design system replaces with its own (`modal_panel`, `controls::keycaps`).
const REPLACED_KIT: [&str; 3] = ["Dialog", "Keybind", "ShortcutRecorder"];

/// Files the ratchet leaves alone: tests, and the surfaces that draw objects, not settings rows.
fn checked(path: &str) -> bool {
    let test = path.ends_with("/tests.rs")
        || path.ends_with("_tests.rs")
        || path.contains("/tests/")
        || path == "src/design_tokens.rs";
    let drawn = path.starts_with("src/onboarding")
        || path.starts_with("src/hud")
        || path == "src/panes/layout/preview.rs"
        || path == "src/playground.rs";
    !test && !drawn
}

/// `controls.rs` and its submodules define the tokens, so they may name the kit's ramp.
fn defines_tokens(path: &str) -> bool {
    path == "src/controls.rs" || path.starts_with("src/controls/")
}

/// Files made of settings rows, whose height and vertical padding belong to `controls::row_frame`.
fn made_of_rows(path: &str) -> bool {
    ["src/panes", "src/activity", "src/rules"]
        .iter()
        .any(|prefix| path.starts_with(prefix))
}

/// The file's lines up to its inline test module, without line comments.
fn code_lines(text: &str) -> Vec<&str> {
    let lines: Vec<&str> = text.lines().collect();
    let mut code = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index];
        if line.trim() == "#[cfg(test)]" {
            match lines.get(index + 1) {
                Some(next) if next.trim_start().starts_with("mod ") && next.contains('{') => break,
                Some(_) => {
                    index += 2;
                    continue;
                }
                None => break,
            }
        }
        code.push(line.split("//").next().unwrap_or(""));
        index += 1;
    }
    code
}

fn ident(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// `4.0)` → 4.0; anything but a lone number before the closing parenthesis → None.
fn lone_number(after_paren: &str) -> Option<f64> {
    let end = after_paren.find(')')?;
    let arg = after_paren[..end].trim();
    if arg.is_empty()
        || !arg
            .bytes()
            .all(|b| b.is_ascii_digit() || b == b'.' || b == b'_' || b == b'-')
    {
        return None;
    }
    arg.replace('_', "").parse().ok()
}

fn whole_word(code: &str, word: &str) -> usize {
    let bytes = code.as_bytes();
    code.match_indices(word)
        .filter(|(start, _)| {
            let end = start + word.len();
            (*start == 0 || !ident(bytes[*start - 1])) && (end == bytes.len() || !ident(bytes[end]))
        })
        .count()
}

/// Every `.name(<number>)` call on the line.
fn literal_calls(code: &str) -> Vec<(&str, f64)> {
    let bytes = code.as_bytes();
    let mut calls = Vec::new();
    for (dot, _) in code.match_indices('.') {
        let start = dot + 1;
        let mut end = start;
        while end < bytes.len() && ident(bytes[end]) {
            end += 1;
        }
        if end > start
            && bytes.get(end) == Some(&b'(')
            && let Some(value) = lone_number(&code[end + 1..])
        {
            calls.push((&code[start..end], value));
        }
    }
    calls
}

fn violations(path: &str, text: &str) -> usize {
    if !checked(path) {
        return 0;
    }
    let tokens = defines_tokens(path);
    let rows = made_of_rows(path);
    let mut count = 0;
    for code in code_lines(text) {
        if !tokens {
            count += code.matches("TextStyle::new(").count();
            count += code.matches(".k.ty.").count();
            count += REPLACED_KIT
                .iter()
                .map(|name| whole_word(code, name))
                .sum::<usize>();
        }
        for (method, value) in literal_calls(code) {
            let off_scale = SPACING.contains(&method) && !SCALE.contains(&value);
            let round = method == "radius" && value >= 3.0;
            let row_height = !tokens && method == "min_h" && value > 0.0;
            let row_padding = rows && method == "py";
            count += usize::from(off_scale)
                + usize::from(round)
                + usize::from(row_height)
                + usize::from(row_padding);
        }
    }
    count
}

fn rust_files(dir: &Path, root: &Path, out: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, root, out);
        } else if path.extension().is_some_and(|ext| ext == "rs")
            && let Ok(relative) = path.strip_prefix(root)
        {
            out.push(relative.to_string_lossy().replace('\\', "/"));
        }
    }
}

/// `agent/src/view.rs 2` per line; `#` starts a comment.
fn parse_ceilings(text: &str) -> BTreeMap<String, usize> {
    text.lines()
        .map(|line| line.split('#').next().unwrap_or("").trim())
        .filter(|line| !line.is_empty())
        .filter_map(|line| {
            let (path, count) = line.rsplit_once(char::is_whitespace)?;
            Some((path.trim().to_owned(), count.parse().ok()?))
        })
        .collect()
}

/// What is wrong with the counts, given the ceilings: a file over its ceiling, and a ceiling
/// above its file's count (a ratchet that was not tightened).
fn failures(counts: &BTreeMap<String, usize>, ceilings: &BTreeMap<String, usize>) -> Vec<String> {
    let mut failures = Vec::new();
    for (path, count) in counts {
        let ceiling = ceilings.get(path).copied().unwrap_or(0);
        if *count > ceiling {
            failures.push(format!(
                "{path}: {count} hand-typed measurements, ceiling {ceiling}. Use METRICS, KEYS or a Paint role"
            ));
        }
    }
    for (path, ceiling) in ceilings {
        let count = counts.get(path).copied().unwrap_or(0);
        if count < *ceiling {
            failures.push(format!(
                "{path}: ceiling {ceiling} but only {count} left. Lower it: {UPDATE_ENV}=1 cargo test -p spaceman-agent design_tokens"
            ));
        }
    }
    failures
}

/// The exceptions file for these counts, or the files whose number would go up.
fn tightened(
    counts: &BTreeMap<String, usize>,
    ceilings: &BTreeMap<String, usize>,
) -> Result<String, Vec<String>> {
    let raised: Vec<String> = counts
        .iter()
        .filter(|(path, count)| **count > ceilings.get(*path).copied().unwrap_or(0))
        .map(|(path, count)| format!("{path}: {count}"))
        .collect();
    if !raised.is_empty() {
        return Err(raised);
    }
    let mut text = String::from(
        "# Hand-typed measurements left in the Settings UI, per file (agent/src/design_tokens.rs).\n\
         # A ceiling only goes down: fix the file, then run\n\
         #   SPACEMAN_UPDATE_TOKEN_CEILINGS=1 cargo test -p spaceman-agent design_tokens\n",
    );
    for (path, count) in counts.iter().filter(|(_, count)| **count > 0) {
        text.push_str(&format!("{path} {count}\n"));
    }
    Ok(text)
}

fn repository() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("agent/ sits in the repository")
        .to_path_buf()
}

#[test]
fn settings_ui_stays_on_its_tokens() {
    let root = repository();
    let mut files = Vec::new();
    rust_files(&root.join("agent/src"), &root.join("agent"), &mut files);
    assert!(files.len() > 20, "found only {} agent sources", files.len());

    let mut counts = BTreeMap::new();
    for file in &files {
        let text = fs::read_to_string(root.join("agent").join(file)).expect("readable source");
        let count = violations(file, &text);
        if count > 0 {
            counts.insert(format!("agent/{file}"), count);
        }
    }
    let exceptions = root.join(EXCEPTIONS);
    let ceilings = parse_ceilings(&fs::read_to_string(&exceptions).unwrap_or_default());

    if std::env::var_os(UPDATE_ENV).is_some() {
        match tightened(&counts, &ceilings) {
            Ok(text) => fs::write(&exceptions, text).expect("writable exceptions file"),
            Err(raised) => panic!(
                "{UPDATE_ENV} never raises a ceiling. Fix these files instead:\n  {}",
                raised.join("\n  ")
            ),
        }
        return;
    }
    let failures = failures(&counts, &ceilings);
    assert!(
        failures.is_empty(),
        "design tokens (docs/design-system.md):\n  {}",
        failures.join("\n  ")
    );
}

#[test]
fn each_kind_of_one_off_is_counted() {
    let sample = "\
fn view(p: Paint) -> El {
    row()
        .gap(10.0)          // off the scale
        .px(METRICS.gutter) // a token
        .py(8.0)
        .min_h(44.0)
        .radius(6.0)
        .text_style(p.k.ty.ui)
        .child(Dialog::new())
        .child(text(\"x\").style(TextStyle::new(12.0, 400.0)))
}
";
    // gap, radius, min_h, .k.ty., Dialog and TextStyle::new anywhere; py only in a file of rows.
    assert_eq!(violations("src/view.rs", sample), 6);
    assert_eq!(violations("src/panes/general.rs", sample), 7);
    // The token files may name the ramp and set row heights; a one-off is still a one-off.
    assert_eq!(violations("src/controls.rs", sample), 2);
    assert_eq!(violations("src/controls/keycaps.rs", sample), 2);
    for skipped in [
        "src/onboarding/desk.rs",
        "src/hud/pill.rs",
        "src/playground.rs",
        "src/panes/layout/preview.rs",
        "src/panes/shortcuts/tests.rs",
    ] {
        assert_eq!(violations(skipped, sample), 0, "{skipped}");
    }
}

#[test]
fn inline_tests_and_scale_values_are_not_counted() {
    let sample = "\
fn view() -> El {
    row().gap(8.0).px(16.0).radius(2.0).min_h(0.0)
}

#[cfg(test)]
mod tests {
    fn fixture() -> El {
        row().gap(11.0).radius(9.0)
    }
}
";
    assert_eq!(violations("src/view.rs", sample), 0);
}

#[test]
fn ceilings_only_go_down() {
    let counts = BTreeMap::from([("agent/src/a.rs".to_owned(), 2)]);
    let exact = BTreeMap::from([("agent/src/a.rs".to_owned(), 2)]);
    let loose = BTreeMap::from([("agent/src/a.rs".to_owned(), 3)]);
    let tight = BTreeMap::from([("agent/src/a.rs".to_owned(), 1)]);
    let gone = BTreeMap::from([("agent/src/b.rs".to_owned(), 1)]);

    assert!(failures(&counts, &exact).is_empty());
    assert_eq!(failures(&counts, &loose).len(), 1);
    assert_eq!(failures(&counts, &tight).len(), 1);
    assert_eq!(failures(&BTreeMap::new(), &gone).len(), 1);

    assert!(tightened(&counts, &tight).is_err());
    assert!(tightened(&counts, &BTreeMap::new()).is_err());
    let text = tightened(&counts, &loose).expect("lowering is allowed");
    assert_eq!(parse_ceilings(&text), exact);
}
