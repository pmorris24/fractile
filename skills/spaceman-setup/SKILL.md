---
name: spaceman-setup
description: Install, configure, verify, troubleshoot, or uninstall the Spaceman tiling window manager on macOS. Use when a user asks to set up Spaceman, configure window rules or workspaces, transition from another window manager, recover off-screen windows, or remove Spaceman.
---

# Spaceman Setup

Spaceman is an experimental native macOS tiling window manager for Apple silicon. It installs from
source only, signed with your own Developer ID Application identity. It declares macOS 14.0 as its
minimum; compatibility is not yet broadly qualified.

## Install

Disable any other window manager first — two managers fight over the same windows:

```sh
aerospace enable off   # AeroSpace
yabai --stop-service   # yabai
```

Build, sign, install and open from a repository checkout (needs the Xcode command-line tools,
Rust 1.100.0 and a Developer ID Application identity; set `SPACEMAN_CODESIGN_IDENTITY` to pick
one). Run the same command again to upgrade:

```sh
./scripts/install-app.sh
```

Spaceman opens a four-step setup tour: `Welcome`, `Give Spaceman window access`,
`Your workspaces`, then `Try it`. Choose `Get started` and grant Accessibility access in
**System Settings > Privacy & Security**. `Your workspaces` shows the windows open now as miniature
workspaces and suggests where each app would tile; click or drag an icon to move it, or `Skip`.
Tiling stays paused while the tour is open, so granting permission never rearranges windows behind
it; pressing `Arrange & start` moves them, and `Undo & restore` on `Try it` puts them back.
`Try it` teaches the shortcuts on a miniature desktop - `Option+2` and `Option+1` to switch
workspaces, `Option+Shift+Space` to float, `Option+Shift+F` to fill the screen - and
`Launch at login` lives there. None of your real windows move while it has the keyboard.

The permission list is named **Accessibility** through macOS 26, **Device Control and Data Access**
on macOS 27.0 through 27.2, and **System & App Protection Override** on macOS 27.3 and later. On 27.3
and later System Settings refuses dragged app tiles: click the Privacy & Security notification, or
use the list's `+` button to add Spaceman. Never tell someone to drag Spaceman into that list, and
never float a drag guide over System Settings there.

Source and issues: <https://github.com/anomalyco/spaceman>.

## Verify

The CLI ships inside the bundle:

```sh
mkdir -p "$HOME/.local/bin"
ln -sf "/Applications/Spaceman.app/Contents/MacOS/spaceman" "$HOME/.local/bin/spaceman"
"$HOME/.local/bin/spaceman" ctl status   # healthy: "mode":"automatic" and "busy":false
```

## Controls

| Keys | Action |
| --- | --- |
| `Option+1` … `Option+0` | Switch to workspace 1 … 10 |
| `Option+Shift+1` … `Option+Shift+0` | Move focused window to workspace 1 … 10 |
| `Option+A` … `Option+Z` | Switch to a configured named workspace |
| `Option+Shift+A` … `Option+Shift+Z` | Move focused window to a configured named workspace |
| `Option+Tab` | Focus the workspace focused before this one, on any display |
| `Option+backtick` | Focus the next window in the workspace (tiles in stable order, then floating windows) |
| `Option+Shift+backtick` | Focus the previous window in that same order |
| `Option+H/J/K/L` | Focus left/down/up/right |
| `Option+Shift+H/J/K/L` | Move the focused tile |
| `Option+/` | Toggle the root split axis |
| `Option+-`, `Option+=` | Shrink/grow the focused tile |
| `Option+Shift+F` | Toggle workspace-wide Fullscreen Layout |

CLI: `spaceman ctl switch 2`, `spaceman ctl move 7`, `spaceman ctl back-and-forth`,
`spaceman ctl layout`, `spaceman ctl balance`, `spaceman ctl pause`, `spaceman ctl resume`.

## Configure

Config lives at `~/Library/Application Support/Spaceman/config.toml`, read at startup
(`SPACEMAN_CONFIG` overrides the path). Workspaces 1–5 prefer the main display, 6–10 the
secondary; assignments accept `main`, `secondary`, a display UUID, or a display name. Schema
version is `1`; rules are ordered and first-match. A rule accepts exact `bundle_id` and/or `title`
matching, with an `action` of `float`, `tile`, `route`, or `ignore`; `route` also requires `workspace`.

```sh
spaceman config-check "$HOME/Library/Application Support/Spaceman/config.toml"
spaceman displays
```

## Troubleshoot

- **Tiling never starts**: the setup tour is still open and waiting for `Arrange & start`, or
  Accessibility permission is missing. Open Settings, run the tour again, and check the menu bar item.
- **Hotkeys do nothing**: another app owns the `Option` shortcuts, or Accessibility permission is
  missing — the menu bar item reports which.
- **Windows off-screen after a crash**: reopen the same app, relaunch Spaceman, and let it restore.
  Keep `parked-windows.json` — it records where those windows return. Do not remove recovery state
  until pause or quit has restored every window.
- **Windows keep moving on their own**: another window manager is running; quit it, then
  `spaceman ctl resume`.
- **Upgrades**: pull the checkout and run `./scripts/install-app.sh` again; it restores every
  window before the old version quits.

## Uninstall

Turn off `Launch at login` in Spaceman Settings first, then restore and quit:

```sh
spaceman ctl pause   # wait for "mode":"paused" and "busy":false
spaceman ctl quit    # wait for spaceman-agent to exit
```

Confirm windows returned, then remove the app and CLI (this preserves your configuration and workspace state for a later reinstall):

```sh
rm -rf "/Applications/Spaceman.app"
rm -f "$HOME/.local/bin/spaceman"
```

Only if you also want to erase saved configuration, workspace state, and recovery files after confirming every window has returned:

```sh
rm -rf "$HOME/Library/Application Support/Spaceman"
```
