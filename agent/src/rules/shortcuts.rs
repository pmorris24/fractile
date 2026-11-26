//! `[shortcuts]` as Settings › Shortcuts edits it: one binding at a time, in place, so comments
//! and every other binding survive. A built-in shortcut moved to a new chord leaves `none` on
//! its old one, and moving it back home removes both.

use fractile::{
    WorkspaceId,
    shortcuts::{self, Chord, Command, Key, Modifiers, Shortcuts},
};
use toml_edit::{Item, Table, TableLike, value};

use super::{
    Doc,
    comments::{merge_detached_comments, prepend_key_comment, remove_table_key},
};

impl Doc {
    /// `[shortcuts]` as Fractile reads it, or the built-ins while the file has an error.
    pub fn shortcuts(&self) -> Shortcuts {
        fractile::config::Config::parse(&self.source())
            .map(|config| config.shortcuts().clone())
            .unwrap_or_default()
    }

    /// Whether `[shortcuts]` changes anything from the built-in shortcuts.
    pub fn has_custom_shortcuts(&self) -> bool {
        self.shortcuts_table()
            .is_some_and(|table| !table.is_empty())
    }

    /// Gives `command` (in `fractile ctl` syntax) the one chord `chord`, or none at all.
    /// `default` is the chord its built-in shortcut uses, if it has one.
    pub fn set_shortcut(&mut self, command: &str, default: Option<Chord>, chord: Option<Chord>) {
        let Ok(Some(target)) = shortcuts::parse_command(command) else {
            return;
        };
        let shortcuts = self.shortcuts();
        let named = self.named_workspaces();
        let default = default.filter(|_| shortcuts.defaults);
        let bindings = self.bindings_mut();
        let bindings_pos = bindings.position();
        let ours = bound_keys(bindings, |bound| bound == Some(&target));
        let mut vacated_built_ins = Vec::new();
        let mut detached = None;
        for key in ours {
            if let Ok(old) = shortcuts::parse_chord(&key) {
                let keep_as_none = chord != Some(old)
                    && (default == Some(old) || is_implicit_shortcut(old, &shortcuts, &named));
                if keep_as_none {
                    super::set_table_item(bindings, &key, value("none"));
                    continue;
                }
                if default != Some(old)
                    && chord != Some(old)
                    && is_implicit_shortcut(old, &shortcuts, &named)
                {
                    vacated_built_ins.push(old);
                }
                if chord == Some(old) && default != Some(old) {
                    continue;
                }
            }
            detached = merge_detached_comments(detached, remove_table_key(bindings, &key));
        }
        if let Some(chord) = chord {
            if default == Some(chord) {
                detached = merge_detached_comments(detached, remove_chord(bindings, chord));
            } else if let Some(existing) = find_chord(bindings, chord) {
                super::set_table_item(bindings, &existing, value(command));
            } else {
                let key = chord.source();
                bindings.insert(&key, value(command));
                if let Some(comment) = detached.take() {
                    prepend_key_comment(bindings, &key, &comment);
                }
            }
        }
        // Moving or clearing a command that had taken over another built-in chord leaves `none`
        // on that chord so the previously cleared or moved built-in shortcut stays inactive.
        for vacated in vacated_built_ins {
            if find_chord(bindings, vacated).is_none() {
                let key = vacated.source();
                bindings.insert(&key, value("none"));
                if let Some(comment) = detached.take() {
                    prepend_key_comment(bindings, &key, &comment);
                }
            }
        }
        // The built-in shortcut keeps its chord unless something claims it.
        if let Some(default) = default
            && chord != Some(default)
            && find_chord(bindings, default).is_none()
        {
            let key = default.source();
            bindings.insert(&key, value("none"));
            if let Some(comment) = detached.take() {
                prepend_key_comment(bindings, &key, &comment);
            }
        }
        if let Some(comment) = detached {
            self.splice_detached_after_table(bindings_pos, comment);
        }
        self.tidy_shortcuts();
    }

    /// Sets the modifiers every built-in shortcut holds. A built-in shortcut unbound with
    /// `none` stays unbound on its new chord; fixed chords (such as Scratchpad's `alt+\`)
    /// stay put because they do not follow `modifier`.
    pub fn set_shortcut_modifier(&mut self, modifier: Modifiers) {
        let shortcuts = self.shortcuts();
        let old = shortcuts.modifier;
        if old == modifier {
            return;
        }
        if shortcuts.defaults {
            let named = self.named_workspaces();
            let bindings = self.bindings_mut();
            let bindings_pos = bindings.position();
            let unbound = bound_keys(bindings, |bound| bound.is_none());
            let overridden = bound_keys(bindings, |bound| bound.is_some());
            let mut suppress_moved = Vec::new();
            let mut detached = None;
            for key in unbound {
                let Ok(chord) = shortcuts::parse_chord(&key) else {
                    continue;
                };
                if !shortcuts::is_built_in_chord(chord, old, &named) {
                    continue;
                }
                detached = merge_detached_comments(detached, remove_table_key(bindings, &key));
                suppress_moved.push(chord);
            }
            for key in overridden {
                let Ok(chord) = shortcuts::parse_chord(&key) else {
                    continue;
                };
                if shortcuts::is_built_in_chord(chord, old, &named) {
                    suppress_moved.push(chord);
                }
            }
            for chord in suppress_moved {
                let modifiers = if chord.modifiers.shift {
                    modifier.with_shift()
                } else {
                    modifier
                };
                let moved = Chord { modifiers, ..chord };
                if find_chord(bindings, moved).is_none() {
                    let key = moved.source();
                    bindings.insert(&key, value("none"));
                    if let Some(comment) = detached.take() {
                        prepend_key_comment(bindings, &key, &comment);
                    }
                }
            }
            if let Some(comment) = detached {
                self.splice_detached_after_table(bindings_pos, comment);
            }
        }
        if modifier == Modifiers::OPTION {
            self.shortcuts_table_mut();
            self.remove_key_in_table("shortcuts", "modifier");
        } else {
            let table = self.shortcuts_table_mut();
            super::set_table_item(table, "modifier", value(modifier.source()));
        }
        self.tidy_shortcuts();
    }

    /// Back to the built-in shortcuts: `[shortcuts]` leaves the file.
    pub fn reset_shortcuts(&mut self) {
        self.remove_table("shortcuts");
    }

    fn shortcuts_table(&self) -> Option<&dyn TableLike> {
        self.doc.get("shortcuts").and_then(Item::as_table_like)
    }

    fn shortcuts_table_mut(&mut self) -> &mut Table {
        table_at(self.doc.entry("shortcuts").or_insert_with(implicit_table))
    }

    fn bindings_mut(&mut self) -> &mut Table {
        table_at(
            self.shortcuts_table_mut()
                .entry("bindings")
                .or_insert(Item::Table(Table::new())),
        )
    }

    /// Drops a `[shortcuts.bindings]` or `[shortcuts]` table an edit emptied.
    fn tidy_shortcuts(&mut self) {
        let bindings_empty = self
            .doc
            .get("shortcuts")
            .and_then(Item::as_table_like)
            .and_then(|table| table.get("bindings"))
            .and_then(Item::as_table_like)
            .is_some_and(TableLike::is_empty);
        if bindings_empty {
            self.remove_subtable("shortcuts", "bindings");
        }
        if self
            .doc
            .get("shortcuts")
            .and_then(Item::as_table_like)
            .is_some_and(TableLike::is_empty)
        {
            self.remove_table("shortcuts");
        }
    }
}

/// `item` as a standard table: an inline one becomes a table section, anything else a new one.
fn table_at(item: &mut Item) -> &mut Table {
    if let Item::Value(toml_edit::Value::InlineTable(inline)) = item {
        *item = Item::Table(std::mem::take(inline).into_table());
    }
    if !item.is_table() {
        *item = Item::Table(Table::new());
    }
    let Item::Table(table) = item else {
        unreachable!("just made a table")
    };
    table
}

fn implicit_table() -> Item {
    let mut table = Table::new();
    table.set_implicit(true);
    Item::Table(table)
}

/// The keys of every binding whose command passes `keep` (`None` for `none`).
fn bound_keys(bindings: &dyn TableLike, keep: impl Fn(Option<&Command>) -> bool) -> Vec<String> {
    bindings
        .iter()
        .filter(|(_, item)| {
            item.as_str()
                .and_then(|source| shortcuts::parse_command(source).ok())
                .is_some_and(|command| keep(command.as_ref()))
        })
        .map(|(key, _)| key.to_owned())
        .collect()
}

/// The key that spells `chord`, however it is written (`alt+h`, `Option + H`).
fn find_chord(bindings: &dyn TableLike, chord: Chord) -> Option<String> {
    bindings
        .iter()
        .map(|(key, _)| key)
        .find(|key| shortcuts::parse_chord(key).is_ok_and(|parsed| parsed == chord))
        .map(str::to_owned)
}

fn remove_chord(bindings: &mut Table, chord: Chord) -> Option<String> {
    let key = find_chord(bindings, chord)?;
    remove_table_key(bindings, &key)
}

/// Whether removing `chord` from `[shortcuts.bindings]` would re-expose an implicit shortcut
/// (a built-in default under `shortcuts.defaults`, or a fixed Scratchpad chord).
fn is_implicit_shortcut(chord: Chord, shortcuts: &Shortcuts, named: &[WorkspaceId]) -> bool {
    (shortcuts.defaults && shortcuts::is_built_in_chord(chord, shortcuts.modifier, named))
        || (shortcuts.scratchpad && is_scratchpad_chord(chord))
}

fn is_scratchpad_chord(chord: Chord) -> bool {
    chord.key == Key::Character('\\')
        && !chord.modifiers.control
        && !chord.modifiers.command
        && chord.modifiers.option
}

#[cfg(test)]
mod tests;
