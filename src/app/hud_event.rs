//! What the Workspace HUD shows: each display's row of workspaces, with their layouts and
//! the apps in them. Title-free: bundle IDs and preview rects only.

use super::{bridge::hud_preview_rect, *};

impl<B: WorkspaceBackend> WorkspaceRuntime<B> {
    /// Per display, every workspace that shows on it, empty ones included.
    pub(super) fn workspace_menu(
        &self,
        visible: &[ResolvedWorkspace],
        outputs: &macos::DisplayOutputs,
    ) -> Vec<status_item::DisplayWorkspaces> {
        let physical = self.config.labs().physical_displays;
        menu_displays(outputs, physical)
            .into_iter()
            .filter_map(|display| {
                let current = visible
                    .iter()
                    .find(|resolved| resolved.display_id == display.id)
                    .map(|resolved| resolved.workspace);
                let workspaces = self
                    .owned_workspaces(&display.id, outputs)
                    .into_iter()
                    .map(|workspace| {
                        let mut apps = Vec::new();
                        for window in self.workspaces.windows(workspace) {
                            if !apps.contains(&window.app.pid) {
                                apps.push(window.app.pid);
                            }
                        }
                        status_item::MenuWorkspace {
                            workspace,
                            current: current == Some(workspace),
                            apps,
                        }
                    })
                    .collect::<Vec<_>>();
                (!workspaces.is_empty()).then(|| status_item::DisplayWorkspaces {
                    display_id: display.id.clone(),
                    display: if physical {
                        outputs.label(display)
                    } else {
                        display.name.clone()
                    },
                    workspaces,
                })
            })
            .collect()
    }

    /// The menu-bar glyphs: each managed display's visible workspace. With `[labs]
    /// physical_displays` they go left to right, a portrait display's glyph is tall, and each
    /// names its display for VoiceOver. A compact menu bar icon (`settings.menu_bar_icon`, with
    /// `[labs] menu_bar_icon_style`) keeps only the focused workspace's glyph.
    pub(super) fn status_layouts(
        &self,
        visible: &[ResolvedWorkspace],
        outputs: &macos::DisplayOutputs,
    ) -> Vec<status_item::WorkspaceLayout> {
        let physical = self.config.labs().physical_displays;
        let ordered = menu_displays(outputs, physical);
        let mut shown: Vec<&ResolvedWorkspace> = visible
            .iter()
            .filter(|resolved| self.scope.manages_selector(&resolved.selector))
            .collect();
        if physical {
            shown.sort_by_key(|resolved| {
                ordered
                    .iter()
                    .position(|display| display.id == resolved.display_id)
                    .unwrap_or(usize::MAX)
            });
        }
        if self.config.menu_bar_icon() == crate::config::MenuBarIcon::Compact {
            // One glyph: the focused workspace's display, or the first one before any focus.
            let focused = self.focus.history().current;
            let keep = shown
                .iter()
                .position(|resolved| Some(resolved.workspace) == focused)
                .unwrap_or(0);
            shown = shown.into_iter().skip(keep).take(1).collect();
        }
        shown
            .into_iter()
            .map(|resolved| {
                let display = ordered
                    .iter()
                    .find(|display| display.id == resolved.display_id);
                let portrait = display.is_some_and(|display| shows_portrait(display, physical));
                status_item::WorkspaceLayout {
                    workspace: resolved.workspace.to_string(),
                    mode: self.workspace_preview(resolved.workspace, hud_preview_rect(portrait)),
                    portrait,
                    display: display
                        .filter(|_| physical)
                        .map(|display| outputs.label(display)),
                }
            })
            .collect()
    }

    /// The HUD for an action on `workspace`. `from` is the workspace a switch left on that
    /// workspace's display, so the HUD can start there.
    pub(super) fn hud_for(
        &self,
        workspace: WorkspaceId,
        kind: UiHudKind,
        from: Option<WorkspaceId>,
        outputs: &macos::DisplayOutputs,
    ) -> UiHudEvent {
        let visible = self.visible_workspaces(outputs);
        let physical = self.config.labs().physical_displays;
        let displays = outputs
            .displays()
            .iter()
            .take(HUD_MAX_DISPLAYS)
            .map(|display| {
                let current = visible
                    .iter()
                    .find(|resolved| resolved.display_id == display.id)
                    .map(|resolved| resolved.workspace);
                let from = from.filter(|from| {
                    *from != workspace
                        && current == Some(workspace)
                        && outputs.resolve(self.workspaces.assignment(*from)).id == display.id
                });
                let portrait = shows_portrait(display, physical);
                UiHudDisplay {
                    id: display.id.clone(),
                    visible: current,
                    from,
                    workspaces: self
                        .listed_workspaces(&display.id, current, outputs)
                        .into_iter()
                        .map(|workspace| self.hud_workspace(workspace, hud_preview_rect(portrait)))
                        .collect(),
                    order: self.config.ordered_workspaces().collect(),
                    portrait,
                }
            })
            .collect();
        UiHudEvent {
            workspace,
            kind,
            displays,
        }
    }

    /// Every enabled, managed workspace holding windows, previewed in the shape of the screen it
    /// shows on: what Settings › Screens draws in each workspace's tile.
    pub(super) fn workspace_windows(
        &self,
        outputs: &macos::DisplayOutputs,
    ) -> Vec<UiWorkspaceWindows> {
        self.config
            .ordered_workspaces()
            .filter(|workspace| {
                let selector = self.workspaces.assignment(*workspace);
                self.workspaces.is_enabled(*workspace)
                    && self.scope.manages_selector(selector)
                    && !self.workspaces.windows(*workspace).is_empty()
            })
            .map(|workspace| {
                let frame = outputs.resolve(self.workspaces.assignment(workspace)).frame;
                let portrait = frame.height > frame.width;
                UiWorkspaceWindows {
                    windows: self.hud_workspace(workspace, hud_preview_rect(portrait)),
                    portrait,
                }
            })
            .collect()
    }

    /// `workspace` for the HUD, its tiles in `preview` space.
    fn hud_workspace(&self, workspace: WorkspaceId, preview: crate::Rect) -> UiHudWorkspace {
        let fullscreen = self.workspaces.deck(workspace).is_monocle();
        let engine = self.workspaces.engine(workspace);
        let mut tiles: Vec<UiHudTile> = engine
            .preview_placements(preview)
            .into_iter()
            .take(HUD_MAX_TILES)
            .map(|placement| UiHudTile {
                key: placement.slot.0,
                rect: placement.frame,
                bundle: placement
                    .window
                    .as_ref()
                    .and_then(|window| self.bundle_id(&window.app))
                    .map(str::to_owned),
            })
            .collect();
        // Fullscreen Layout shows the window in front first.
        if fullscreen
            && let Some(front) = self
                .workspaces
                .preferred_focus(workspace)
                .and_then(|window| engine.slot_for_window(&window))
            && let Some(index) = tiles.iter().position(|tile| tile.key == front.0)
        {
            let tile = tiles.remove(index);
            tiles.insert(0, tile);
        }
        let mut owned: Vec<_> = self
            .workspaces
            .floating_entries()
            .filter(|(_, owner)| *owner == workspace)
            .map(|(window, _)| window)
            .collect();
        owned.sort_by_key(|window| (window.app.pid, window.window_id));
        let mut floating: Vec<String> = Vec::new();
        for window in owned {
            if floating.len() == HUD_MAX_FLOATING {
                break;
            }
            if let Some(bundle) = self.bundle_id(&window.app)
                && !floating.iter().any(|known| known == bundle)
            {
                floating.push(bundle.to_owned());
            }
        }
        UiHudWorkspace {
            workspace,
            fullscreen,
            tiles,
            floating,
        }
    }
}

/// The displays in menu order: as macOS lists them, or left to right then top to bottom with
/// `[labs] physical_displays` (`physical`), the order `Option+Shift+Tab` moves through.
fn menu_displays(outputs: &macos::DisplayOutputs, physical: bool) -> Vec<&macos::DisplayOutput> {
    if physical {
        outputs.ordered()
    } else {
        outputs.displays().iter().collect()
    }
}

/// Whether the menu bar and HUD draw `display` tall: with `[labs] physical_displays`
/// (`physical`) on, when its frame stands taller than it is wide. The one test both use, so a
/// nearly square display gets the same shape in each.
const fn shows_portrait(display: &macos::DisplayOutput, physical: bool) -> bool {
    physical && display.frame.height > display.frame.width
}
