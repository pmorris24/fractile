//! Which of an app's windows a rule is for: the sheet that sets a rule's `title_contains` filter,
//! and the Add menu that opens it. An app can have a rule for every window and any number for
//! the windows a filter picks out, so Safari's Work windows open on one workspace and its
//! Claude windows on another.
//!
//! The field starts empty, or with the filter the rule already has in config.toml. Nothing here
//! reads, suggests or stores a live window title.

use rune::{El, TextEdit, Tx};
use rune_kit::TextInput;

use crate::{
    Sheet, Ui, apps,
    controls::{self, ButtonKind, ModalWidth, Paint},
    rules::{Doc, Place, Rule},
};

pub(super) const SAFARI: &str = "com.apple.Safari";
/// The filter field, wide enough for a few words.
const FIELD_W: f32 = 180.0;

/// A rule's title filter while its sheet is open. The text being typed is `Ui::title_filter`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FilterDraft {
    pub bundle_id: String,
    pub place: Place,
    /// The rule on the map being changed; `None` adds a rule.
    pub editing: Option<Rule>,
}

impl FilterDraft {
    /// The rule that already answers for these windows. A second one could never match, so the
    /// sheet won't add it.
    fn taken(&self, doc: &Doc, filter: Option<&str>) -> Option<Rule> {
        let except = self.editing.as_ref().map(|rule| rule.index);
        doc.rule_matching(&self.bundle_id, filter, except)
    }
}

/// The bundle IDs with a rule of any kind.
pub(super) fn ruled_apps(doc: &Doc) -> Vec<String> {
    doc.rules()
        .into_iter()
        .filter_map(|rule| rule.bundle_id)
        .collect()
}

/// What the field holds: the filter, or `None` for every window.
fn typed(ui: &Ui) -> Option<&str> {
    let filter = ui.title_filter.text.trim();
    (!filter.is_empty()).then_some(filter)
}

impl Ui {
    /// Opens the Add menu of `place` with every running app. None is left out for having a
    /// rule: that hid Safari from every workspace but one.
    pub fn open_add_menu(&mut self, place: Place) {
        self.running = if self.fixture {
            apps::fixture_running()
        } else {
            apps::running()
        };
        for app in &self.running {
            self.apps.get(&app.bundle_id);
        }
        self.menu = Some(crate::Menu::Add(place));
    }

    /// Adds a running app to `place`. An app with no rule gets one for every window at once; an
    /// app that has a rule gets the sheet, to say which windows this one is for.
    pub fn add_app(&mut self, bundle_id: &str, place: Place) {
        self.menu = None;
        if ruled_apps(&self.doc).iter().any(|id| id == bundle_id) {
            self.open_filter(
                FilterDraft {
                    bundle_id: bundle_id.to_owned(),
                    place,
                    editing: None,
                },
                "",
            );
        } else {
            let bundle_id = bundle_id.to_owned();
            self.edit(move |doc| doc.add_rule(&bundle_id, None, place));
        }
    }

    /// Opens the sheet on a rule from the map, with the filter config.toml has for it.
    pub fn edit_filter(&mut self, rule: &Rule) {
        let Some(bundle_id) = rule.bundle_id.clone() else {
            return;
        };
        let filter = rule.title_contains.clone().unwrap_or_default();
        self.open_filter(
            FilterDraft {
                bundle_id,
                place: rule.place,
                editing: Some(rule.clone()),
            },
            &filter,
        );
    }

    fn open_filter(&mut self, draft: FilterDraft, filter: &str) {
        self.menu = None;
        self.drag = None;
        self.title_filter = TextEdit::new(filter);
        self.sheet = Some(Sheet::Filter(draft));
    }

    /// Saves the sheet: a new rule, or the edited rule's filter set or cleared. Nothing is
    /// written while another rule already answers for the same windows.
    pub fn commit_filter(&mut self) {
        let Some(Sheet::Filter(draft)) = self.sheet.clone() else {
            return;
        };
        let filter = typed(self).map(str::to_owned);
        if draft.taken(&self.doc, filter.as_deref()).is_some() {
            return;
        }
        self.sheet = None;
        self.title_filter = TextEdit::default();
        match draft.editing {
            Some(rule) => {
                if rule.title_contains != filter {
                    self.try_edit(|doc| doc.set_title_contains_matching(&rule, filter.as_deref()));
                }
            }
            None => self.edit(|doc| doc.add_rule(&draft.bundle_id, filter.as_deref(), draft.place)),
        }
    }
}

/// "Safari on workspace 2", "Stickies pinned".
fn heading(name: &str, place: Place) -> String {
    match place {
        Place::Workspace(workspace) => format!("{name} on workspace {workspace}"),
        Place::Float => format!("{name} floating"),
        Place::Pin => format!("{name} pinned"),
        Place::Tile => format!("{name} tiled"),
        Place::Ignore => format!("{name} ignored"),
    }
}

/// The filter sheet: one row, its field ready to type in. Return saves, Escape cancels.
pub fn sheet(ui: &Ui, draft: &FilterDraft, p: Paint, layer: &mut Vec<El>) {
    let name = ui.apps.cached(&draft.bundle_id).map_or_else(
        || apps::fallback_name(&draft.bundle_id),
        |app| app.name.clone(),
    );
    let filter = typed(ui);
    let taken = draft.taken(&ui.doc, filter).is_some();
    // The row says what the rule covers as it stands.
    let covers = match (taken, filter.is_some()) {
        (true, false) => "Every window already has a rule",
        (true, true) => "These windows already have a rule",
        (false, false) => "Every window",
        (false, true) => "Only matching windows",
    };
    let field = TextInput::new(&ui.title_filter)
        .label("Title contains")
        .placeholder("Any title")
        .width(Some(FIELD_W))
        .on_edit(|ui: &mut Ui| &mut ui.title_filter)
        .on_submit(|ui: &mut Ui, _: &mut Tx| ui.commit_filter())
        .build(p.k)
        .key("title-filter");
    let row = controls::setting_row(
        p,
        "title-contains",
        "Title contains",
        Some((covers.to_owned(), p.muted())),
        field,
    );
    let confirm = if draft.editing.is_some() {
        "Save"
    } else {
        "Add"
    };
    let panel = controls::modal_panel(
        p,
        "filter-sheet",
        &heading(&name, draft.place),
        ModalWidth::Narrow,
        vec![controls::card(p, "card", vec![row])],
        vec![
            controls::button(
                p,
                "cancel",
                "Cancel",
                ButtonKind::Secondary,
                |ui: &mut Ui, _| {
                    ui.close_sheet();
                },
            ),
            controls::button_enabled(
                p,
                "confirm",
                confirm,
                ButtonKind::Primary,
                !taken,
                |ui: &mut Ui, _| ui.commit_filter(),
            ),
        ],
    );
    controls::modal_sheet(p, "filter", panel, Ui::close_sheet, layer);
}

/// Safari being added to workspace 3 with `filter` typed, for stills.
pub(super) fn fixture_sheet(ui: &mut Ui, filter: &str) {
    ui.apps.get(SAFARI);
    ui.add_app(SAFARI, Place::Workspace(fractile::WorkspaceId::THREE));
    ui.title_filter = TextEdit::new(filter);
}
