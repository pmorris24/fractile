# Feature Map

What a Spaceman user can do, how they reach it, and what currently proves it works. Behavior is
described from the user's point of view; [Controls](../controls.md) and
[Configuration](../configuration.md) remain the reference for exact keys and schema.

```ts
docs/features/
├── README.md               // this index: availability, prerequisites, evidence rules
├── workspaces.md           // switch, move, hide/reveal, where new windows go
├── tiling.md               // split layouts: focus, reorder, drag, resize, balance, axis
├── fullscreen-layout.md    // workspace-wide stacked "every window fills the display" mode
├── scratchpad.md           // Lab: windows hidden off-screen until summoned
├── setup-tour.md           // first run: permission, the suggested arrangement, Try it
└── restarts-and-recovery.md // what survives restarts/relaunches/reboots, restoration, doctor
```

## Availability

Baseline: `dev` at 0.3.2 (`93897a4`). Everything below ships in source builds.

| Surface | Status |
| --- | --- |
| Workspaces, tiling, Fullscreen Layout, parking/restoration | Source builds |
| Parking exemption, remembered placements (`window-memory.json`), arrangement journal, `spaceman doctor` | Source builds |
| Settings › Labs | One off-by-default switch per `[labs]` feature, written to `config.toml`; `spaceman://labs/<slug>` highlights a row. Covered by `agent/src/panes/labs.rs` and `agent/src/panes/tests.rs` |
| Single-process Rune Settings, four-step [setup tour](setup-tour.md), non-activating Workspace HUD (`[settings] hud`), dimmed paused menu bar glyph & `[settings] pause_on_builtin_display` auto-pause/resume with transient HUD toast | Source builds |
| Named workspaces A-Z | Opt-in: enabled by a `route` rule or `[[workspaces]]` entry |
| Custom shortcuts (`[shortcuts]`) | Source builds; rebind, unbind, or replace the built-in keys in `config.toml` or by recording a chord in Settings › Shortcuts, with a base `modifier` for the built-ins, applied live without a restart. Covered by unit tests in `src/shortcuts.rs`, `src/hotkeys.rs`, `src/config.rs`, `src/app/app_loop/live_shortcuts.rs`, `agent/src/rules/shortcuts/tests.rs`, and `agent/src/panes/shortcuts/`; no live harness exercises a rebound chord or live re-registration yet |
| Scratchpad (`[labs] scratchpad`) | Opt-in Lab in source builds; off by default, shortcuts register only when on |
| Empty workspace focus (`[labs] empty_workspace_focus`, Settings › Labs › Workspaces › `Focus empty workspaces on other displays`) | Opt-in Lab in source builds; off by default, an empty workspace chosen on another display is the focused one for commands until another window takes focus ([Workspaces](workspaces.md)) |
| Dragged floats join their display's workspace (`[labs] float_follows_display`) | Opt-in Lab in source builds; off by default, switched in Settings › Labs › Windows. Covered by `src/app/tests/three_display.rs` |
| Settings › Activity (`[labs] activity`) | Opt-in Lab in source builds; off by default, the pane stays out of the sidebar while `spaceman doctor` and the journal keep recording |
| Portrait edge rows (`[labs] portrait_edge_rows`) | Opt-in Lab in source builds; off by default, top/bottom display edge drop bands on portrait displays only when on |
| Physical display order and shape (`[labs] physical_displays`) | Opt-in Lab in source builds; off by default, switched in Settings › Labs › Menu bar & HUD. Menu-bar glyphs and menu sections left to right, portrait glyphs and HUD Layouts previews tall, same-named displays labelled by position. Covered by `src/app/tests/physical_displays.rs`, `src/macos/displays.rs` and `src/status_item.rs` unit tests; no live harness checks the drawn glyphs |
| Recent focus (`[labs] recent_focus`) | Opt-in Lab, switched in Settings › Labs or `config.toml`; off by default, `Option+H/J/K/L` picks the most recently focused of several bordering tiles |
| Swipe switch (`[labs] swipe_switch`) | Opt-in Lab; off by default, switched in Settings › Labs › Workspaces. With it on, `[settings] trackpad_swipe = false` (or Settings › Shortcuts › Swipe to switch workspace) stops the three-finger swipe; with it off the setting is ignored |
| Single-window height (`[labs] single_window_height`) | Opt-in Lab; off by default, switched in Settings › Labs › Windows. With it on, `[settings] single_window_max_height` (or Settings › Layout › Wide displays › Single-window height) centers a lone tile, and Fullscreen Layout, at that height on a taller display; with it off the key is ignored. Covered by `src/spacing/tests.rs`, `src/config.rs` and `src/cli/tests.rs`; no live harness |
| Menu bar icon style (`[labs] menu_bar_icon_style`) | Opt-in Lab; off by default, switched in Settings › Labs › Menu bar & HUD. With it on, `[settings] menu_bar_icon` (or Settings › General › Menu bar icon) picks `full`, `compact` (one glyph, the focused workspace's) or `off` (no status item; relaunching Spaceman opens Settings); with it off the key is ignored and the icon is shown in full. Covered by `src/app/tests/physical_displays.rs`, `src/config.rs`, `src/cli/tests/labs.rs` and `agent/src/panes/general.rs`; no live harness checks the hidden status item |
| Route every window (`[labs] route_all_windows`) | Opt-in Lab; off by default, switched in Settings › Labs › Windows. With it on, a `route` rule with `windows = "all"` routes every new window of its app, not only the first; with it off the key is ignored. `spaceman rules add --workspace <ws> --windows all` writes it. Covered by `src/app/tests/rules.rs`, `src/config.rs`, `src/cli/tests/labs.rs` and `agent/src/rules/filters_tests.rs`; no live harness |
| Focus follows mouse (`[labs] focus_follows_mouse`) | Opt-in Lab; off by default, switched in Settings › Labs › Windows. With it on, the visible tile the pointer moves to and rests on for about 150 ms takes focus, without moving the pointer; floats, the Scratchpad, menus, the menu bar, the Dock, the HUD and Settings never do, and nothing changes while a button is held. Covered by `src/app/tests/focus_follows_mouse.rs`, `src/app/app_loop/hover.rs` and `src/macos/tests.rs`; no live harness |
| Four-finger swipe (`[labs] four_finger_swipe`) | Opt-in Lab; off by default, switched in Settings › Labs › Workspaces. With it on, the workspace swipe takes four fingers and three-finger swipes are left to macOS (for Accessibility "Three Finger Drag"). Covered by `src/swipe.rs` and `src/app/tests/workspace_order.rs`; no live harness |
| Workspace steps (`[labs] workspace_steps`) | Opt-in Lab in source builds; off by default. `switch next\|prev` and `move next\|prev [--follow]` in `spaceman ctl` and `[shortcuts.bindings]` (no built-in chord), listed in Settings › Shortcuts while on; bindings are skipped while off. Covered by `src/app/tests/workspace_steps.rs`, `src/workspace/tests.rs`, `src/hotkeys.rs`, `src/shortcuts.rs`, `src/control.rs`; no live harness yet |
| Secondary-display workspaces 5-7 | Require a second display; with one display they fall back to `main` |
| Tertiary-display workspaces 8-10 | Show on the third display; with two displays they fall back to `secondary` |
| Four or more displays | Assign workspaces in Settings › Workspaces (each display past `tertiary` by UUID) or with `[[workspaces]] monitor = "<uuid>"` |
| Display arrangement (Settings › Screens) | The pane's one view of the workspaces: each connected screen at its real width and place (taller when its workspaces need room), holding a card per workspace with its number's screen menu, Add, and in rows an icon for every app routed there (these drag, and right-click for their menu) and for every window open there now, so an app with two windows open shows twice; a screen grows until its busiest card shows all its icons. Dragging a card to another screen writes that workspace's `monitor` (removing it on the built-in default); dragging any app icon onto a card moves the app there: its route rule (written if it has none) and, through `MoveApp`, its open windows; a title rule moves only its rule. Unplugging some of the screens leaves the panel drawing the whole desk as it was (the last arrangement seen, kept in `known-displays.txt` next to `config.toml`, and, while that holds no unplugged screen, the desk macOS's window server remembers around the screens connected; Forget keeps a screen out of both until it is plugged in again): unplugged screens are dimmed and marked Unplugged, keep their workspaces and take drops, and Forget drops one; meanwhile their workspaces stay separate desktops on the screens left and return when the screen does. Sections below the panel list only workspaces saved for a display the desk never had. Covered by `agent/src/map/arrangement.rs`, `agent/src/panes/workspaces/remembered.rs`, `agent/src/map/tests.rs` and `src/app/tests/unplugged_desktops.rs`; scenes `rules-arrangement`, `rules-arrangement-drag` and `rules-arrangement-unplugged`; no live harness |

## Common Prerequisites

- macOS with Accessibility permission granted to `Spaceman.app`. Without it, or without global
  hotkey registration, Spaceman starts paused and only restores parked windows.
- No other window manager running (AeroSpace, yabai, Amethyst): two managers fight over frames.
- Only standard, movable, on-screen, non-native-fullscreen `AXWindow`s are managed. Panels,
  dialogs, and other subroles are invisible to Spaceman and stay global (see
  [Compatibility](../compatibility.md)).
- CLI paths assume `~/.local/bin/spaceman` is on `PATH`; layout-changing `ctl` commands require
  automatic tiling to be enabled.

## Evidence Conventions

Each feature file labels evidence as one of:

- **Unit (FakeBackend)**: a test in `src/app/tests/` driving `WorkspaceRuntime` through the fake
  backend. Proves runtime decisions and the native calls requested, not real application behavior,
  z-order, timing, or what is painted on screen. CI runs the whole suite for every release.
- **Live harness**: `scripts/live-workspaces.sh` and `scripts/live-ghostty-tabs.sh` drive real
  hotkeys against disposable Ghostty windows. They require AeroSpace enabled initially and two
  displays.
- **Live gauntlet**: `scripts/gauntlet.sh` (see [Development](../development.md#live-gauntlet)) runs a
  real Spaceman.app against disposable windows on one display and asserts each step against both
  the window server and Spaceman's state files, with a JSON receipt. Proves real frames, parking,
  focus, timing and restoration; it does not drive hotkeys or a second display.
- **Observed**: behavior seen in `agent.log` or `journal.jsonl` on the maintainer's machine, with the
  date. A log line proves what Spaceman requested, not what the user saw.
- **User report**: what the user saw, unverified by tooling.

Absence of a check is a **gap**, not a defect; a defect is a reproduced or reported wrong outcome.

## Keeping This Map Current

Update the affected feature file when a change alters a shortcut, CLI command, menu item, rule
semantics, what survives a restart, or which tests cover a flow. Add a new file only for a distinct
user goal, not for a new module.
