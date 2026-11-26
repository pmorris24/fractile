//! `[settings.gaps]`, `settings.single_window_max_width` and `settings.single_window_max_height`,
//! edited in place like the rest of the file. A value set to its default `0` leaves the file, and so does a table it empties.
//! `gaps.outer` is the file's own: Settings shows the edges it resolves to and never writes it.

use fractile::spacing::{Spacing, SpacingKey, check_outer};
use toml_edit::{Item, Table, TableLike};

use super::Doc;

impl Doc {
    /// The spacing the file asks for; a missing or out-of-range key reads as its fallback.
    pub fn spacing(&self) -> Spacing {
        let outer = self.outer();
        let mut spacing = Spacing::NONE;
        for key in SpacingKey::ALL {
            let checked = self
                .spacing_item(key)
                .and_then(Item::as_integer)
                .and_then(|raw| key.check(raw).ok())
                .unwrap_or(key.fallback(outer));
            key.set(&mut spacing, checked);
        }
        spacing
    }

    /// Writes every spacing key whose value changes, removing the ones that need no key. The
    /// rest stay as the file has them: an edge that follows `gaps.outer` keeps following it, and
    /// an explicit zero keeps overriding it.
    pub fn set_spacing(&mut self, spacing: Spacing) {
        let current = self.spacing();
        let outer = self.outer();
        for key in SpacingKey::ALL {
            let value = key.get(&spacing);
            if key.get(&current) == value {
                continue;
            }
            let item = key
                .is_written(value, outer)
                .then(|| toml_edit::value(i64::from(value)));
            match key.gap_name() {
                Some(name) => self.set_gap(name, item),
                None => self.set_setting(key.path(), item),
            }
        }
    }

    /// `gaps.outer`, if the file sets one in range.
    fn outer(&self) -> Option<i32> {
        let raw = self.gaps()?.get("outer")?.as_integer()?;
        check_outer(raw).ok()
    }

    fn spacing_item(&self, key: SpacingKey) -> Option<&Item> {
        match key.gap_name() {
            Some(name) => self.gaps()?.get(name),
            None => self.settings()?.get(key.path()),
        }
    }

    fn settings(&self) -> Option<&dyn TableLike> {
        self.doc.get("settings")?.as_table_like()
    }

    fn gaps(&self) -> Option<&dyn TableLike> {
        self.settings()?.get("gaps")?.as_table_like()
    }

    /// Sets `name` in `[settings.gaps]`, or removes it (and the tables it empties) for `None`.
    fn set_gap(&mut self, name: &str, item: Option<Item>) {
        let Some(item) = item else {
            let Some(settings) = self
                .doc
                .get_mut("settings")
                .and_then(Item::as_table_like_mut)
            else {
                return;
            };
            let emptied = settings
                .get_mut("gaps")
                .and_then(Item::as_table_like_mut)
                .is_some_and(|gaps| {
                    gaps.remove(name);
                    gaps.is_empty()
                });
            if emptied {
                self.remove_subtable("settings", "gaps");
            }
            if self
                .doc
                .get("settings")
                .and_then(Item::as_table_like)
                .is_some_and(toml_edit::TableLike::is_empty)
            {
                self.remove_table("settings");
            }
            return;
        };
        let settings = self.doc.entry("settings").or_insert_with(|| {
            // Holding only `[settings.gaps]`, the parent needs no header of its own.
            let mut table = Table::new();
            table.set_implicit(true);
            Item::Table(table)
        });
        if !settings.is_table_like() {
            *settings = Item::Table(Table::new());
        }
        let position = settings.as_table().and_then(Table::position);
        let Some(settings) = settings.as_table_like_mut() else {
            return;
        };
        if !settings.get("gaps").is_some_and(Item::is_table_like) {
            settings.insert("gaps", Item::Table(Table::new()));
        }
        let Some(gaps) = settings.get_mut("gaps") else {
            return;
        };
        // Sharing `[settings]`'s place draws it right after its parent, not at the file's end.
        if let Some(gaps) = gaps.as_table_mut()
            && let (None, Some(position)) = (gaps.position(), position)
        {
            gaps.set_position(position);
        }
        if let Some(gaps) = gaps.as_table_like_mut() {
            super::set_table_item(gaps, name, item);
        }
    }
}
