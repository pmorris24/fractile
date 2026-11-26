# Fullscreen Layout

Make every tiled window on a workspace fill the whole display, showing one at a time, without
entering macOS native fullscreen; leave the mode to get the exact previous split layout back.

## Sub-features

```ts
Toggle on (focused workspace)                         // fullscreen.toggle
├── Every tiled window -> frame = whole display, less settings.gaps edge padding, and centered at
│   settings.single_window_max_width when the display is wider (and at
│   settings.single_window_max_height when taller, with [labs] single_window_height)
├── Windows stay stacked: focused one raised, the rest full-size underneath (not hidden)
├── Leaving the workspace parks them back to front -> nothing underneath is uncovered
├── Split tree, order, and weights untouched
└── Menu-bar workspace glyph becomes filled

Toggle off -> unchanged split layout re-applied       // fullscreen.toggle
Option+/  -> leaves the mode AND flips the root axis   // fullscreen.exit-via-axis

Choose which window shows                              // fullscreen.focus
├── Option+` / ctl focus next -> next tile in stable order, then floats, wraps (raises it)
├── Option+Shift+` / ctl focus prev -> the same ring, backward
├── Option+H/K -> previous tile, Option+L/J -> next tile, no wrap
│   └── Past either end -> adjacent display, as in split layouts
├── Command+`       -> macOS same-app cycle; Fractile remembers the result
└── Click / Dock / app switch -> whatever macOS focuses is shown and remembered

Mode persistence                                        // fullscreen.persist
├── Switch away and back -> still fullscreen, same window on top
├── Fractile restart     -> mode and remembered top window restored
├── Ghostty native-tab replacement -> keeps mode and slot
├── Floating / dialog window opens -> floats above without exiting Fullscreen Layout
├── New tiled window opens, routes, reappears, moves in, or unfloats -> stays in Fullscreen Layout
│   (fills the display, remembers focus if focused) and cues the Workspace HUD when visible
└── Last tiled window closes or moves away -> mode turns off

Blocked while on                                        // fullscreen.guards
├── Option+-/= resize, balance, Option+Shift+H/J/K/L reorder -> error "Exit Fullscreen Layout before ..."
├── Pointer drag/resize of tiles -> ignored; on release the tile fills its own display again
└── Floating windows -> keep their own frame
```

Unit (FakeBackend), `src/app/tests/layout.rs`:
`fullscreen_layout_toggles_without_changing_tile_order`,
`leaving_a_fullscreen_workspace_parks_the_front_window_last`,
`directional_focus_steps_through_a_fullscreen_stack_then_crosses_displays`,
`removing_the_last_window_exits_fullscreen_layout`,
`layout_toggle_exits_fullscreen_and_applies_the_next_split_axis`,
`workspace_focus_cycle_wraps_through_tile_order_in_every_layout`,
`fullscreen_layout_remembers_native_same_app_focus_changes`,
`fullscreen_layout_activates_persisted_cross_app_focus_after_restart`,
`switching_away_and_back_restores_fullscreen_layout_focus`,
`selecting_an_active_fullscreen_workspace_preserves_its_current_focus`,
`opening_a_new_tiled_window_keeps_fullscreen_layout_and_emits_hud`,
`opening_a_floating_window_preserves_fullscreen_layout`,
`reappearing_parked_tile_keeps_fullscreen_layout_and_emits_hud` (issue 153),
`reappearing_parked_floating_window_preserves_fullscreen_layout`,
`routing_moving_and_unfloating_preserve_fullscreen_layout`,
`routing_onto_a_hidden_fullscreen_workspace_preserves_mode_without_hud`, and
`ghostty_native_tab_replacement_preserves_workspace_and_fullscreen_layout`. They prove frames,
persisted mode, and which window Fractile asks to focus or raise; none observes real z-order.

## How To Get To It (User POV)

```ts
Focus a window on the target workspace (a tile or a floating window it owns)
├── Option+Shift+F / fractile ctl fullscreen -> toggle
└── Option+/                          -> exits (and flips the axis); there is no "exit only" key besides Option+Shift+F
```

The mode is per workspace, not per window and not per display: two workspaces can each be in it
independently.

## Existing Checks

- `float.rs` `workspace_layout_commands_accept_a_focused_floating_window` (issue 135): entering and
  both exits work with a floating window focused.
- Unit tests listed above. Only `fullscreen_layout_activates_persisted_cross_app_focus_after_restart`
  exercises persistence through `WorkspaceState`; it does not restart a real process.
- No live harness covers Fullscreen Layout. The back-to-front test proves the order of park requests
  against a supplied stacking order, not what macOS paints.

## Gotchas

- **Fixed in the working tree: stacked windows flashed during switches (user report and Observed,
  2026-10-01).** Windows used to be parked front first, uncovering each window beneath for about
  10 ms (a Preview image behind Ghostty flashed when switching to an empty workspace). Parking now
  follows the window server's stacking order back to front; if that order is unavailable the old
  order is used. Not yet confirmed visually after the fix.
- **Fixed in the working tree: direction keys could not move within the stack.** Every tile has the
  same frame, so spatial navigation found no target; the stack now uses tile order instead.
- **Nothing shows how many windows are stacked or which is next.** Hidden windows are full-size
  underneath rather than hidden, so closing the top window reveals whatever macOS raises next, not
  necessarily the next tile in Fractile's order. No test covers that close path.
- **Fullscreen Layout is sticky:** new tiled windows (opened, routed, reappearing from a park,
  moved in, or unfloated) join the stack without exiting Fullscreen Layout and show the Workspace HUD
  when the workspace is visible. Only `Option+Shift+F`, `Option+/`, or emptying the workspace leaves
  the mode.
- Native macOS fullscreen is a different thing: such windows are not managed at all. **Known
  defect (Observed, 2026-10-01):** a tiled browser window whose video enters native fullscreen
  drops out of Accessibility observation, loses its slot after the 2 s omission grace, and is adopted
  as a new window when it returns, losing its position, weight, and nesting. Fixing it needs the
  snapshot to report native-fullscreen windows and the layout to hold their slots without placing
  them.
