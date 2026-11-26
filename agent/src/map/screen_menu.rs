//! A workspace tile's number: pressed, it opens the menu of screens the workspace can show on,
//! and picking one writes the workspace's `monitor` to config.toml.

use fractile::WorkspaceId;
use rune::{El, row, text};
use rune_kit::{Menu as KitMenu, MenuItem, Placement};

use super::MapView;
use crate::{Menu, Ui, panes::workspaces as screens};

impl MapView<'_> {
    /// A tile's number, pressed for the menu that moves the workspace to another screen. With
    /// `chevron` it says so; without it (a card that drags to another screen) it is just the
    /// number.
    pub(super) fn screen_menu(
        self,
        workspace: WorkspaceId,
        active: bool,
        chevron: bool,
        layer: &mut Vec<El>,
    ) -> El {
        let c = &self.p.k.c;
        let open = self.ui.menu == Some(Menu::Screen(workspace));
        let trigger = row()
            .key("number")
            .role("button")
            .label(format!("Workspace {workspace}: choose its screen"))
            .help("Choose the screen this workspace shows on")
            .h(24.0)
            .px(6.0)
            .gap(2.0)
            .center_y()
            .radius(self.p.k.radius.sm)
            .hover(c.hover)
            .press(c.pressed)
            .on_press(move |ui: &mut Ui| ui.menu = Some(Menu::Screen(workspace)))
            .child(
                text(workspace.to_string())
                    .font(self.p.tabular(self.p.strong()))
                    .color(if active {
                        self.p.text()
                    } else {
                        self.p.muted()
                    }),
            )
            .children(chevron.then(|| {
                rune::icon(&rune_theme_opencode::icons::CHEVRON_DOWN, 12.0).color(c.icon_muted)
            }));
        let mut menu = KitMenu::new(format!("screen-{workspace}-menu"))
            .trigger(trigger)
            .placement(Placement::BottomStart)
            .gutter(4.0)
            .min_width(220.0)
            .open(open)
            .on_close(|ui: &mut Ui| ui.menu = None);
        if open {
            let current = screens::effective_monitor(&self.ui.doc, workspace);
            let shown = screens::index_of(&self.layout.choices, &current);
            menu = menu.group_label(format!("Show workspace {workspace} on"));
            for (index, choice) in self.layout.choices.iter().enumerate() {
                if !choice.connected && shown != Some(index) {
                    continue;
                }
                let selector = choice.selector.clone();
                menu = menu.item(
                    MenuItem::new(choice.title.clone())
                        .description(choice.caption.clone())
                        .checked(shown == Some(index))
                        .on_select(move |ui: &mut Ui| {
                            ui.menu = None;
                            let monitor = screens::override_for(workspace, &selector);
                            ui.edit(|doc| doc.set_monitor(workspace, monitor.as_deref()));
                        }),
                );
            }
        }
        menu.build(self.p.k, layer)
    }
}
