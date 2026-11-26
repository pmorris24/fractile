# Security And Privacy

## Accessibility Scope

Fractile needs macOS Accessibility permission to enumerate eligible application windows, read their
role, state, title, `AXDocument` URL/path, position, and size, determine focus, and focus, move,
or resize them. It also reads
the on-screen Core Graphics window list, display topology, keyboard input source, pointer position,
and mouse-button state needed for tiling and gestures. Two listen-only event taps watch three- and
four-finger trackpad swipes and Option key presses. The key tap reads only the virtual key code and
modifier flags of presses with Option held, never characters, to notice a Fractile shortcut macOS
did not deliver; other presses are ignored and nothing is stored. The setup tour's Try it step uses
the same kind of tap, only while that step is on screen, to hear the four shortcuts it teaches.
While the setup window has the keyboard on that step, Fractile's global hotkeys are held off, so
those shortcuts move the step's miniature desktop and no real window.

The permission is broad because macOS grants Accessibility at application scope. Fractile limits its
own writes to eligible standard, non-minimized, non-fullscreen windows whose position is settable.
Resizable windows may be moved and resized; movable non-resizable windows may be parked off-screen
and restored without changing their size. The one action Fractile performs inside another app is
`move-tab`: when you run it, Fractile reads the focused app's trailing menu-bar menus (item
identifiers, titles, and enabled state, never stored) to find its own `Move Tab to New Window`
command and presses it. It does not require disabling System Integrity Protection.

## Data Handling

Fractile has no telemetry or analytics and makes no network requests.
Other runtime data stays on the Mac unless the user shares CLI output or files.

The optional `Launch at Login` menu action registers or unregisters the signed main app through
macOS Service Management. Registration is stored and enforced by macOS; Fractile does not install a
separate helper, daemon, or launch-agent plist.

The setup tour suggests an arrangement from the windows open at that moment. The suggestion is
computed on the Mac from each app's bundle ID, its `Info.plist` `LSApplicationCategoryType`, window
sizes and resizability, and each display's width; window titles are never part of it, and nothing is
sent anywhere. The tour's permission step reads two switches in the `com.apple.WindowManager`
preferences (`GloballyEnabled` for Stage Manager, `EnableTilingByEdgeDrag` for macOS edge tiling)
and writes one only when you press its `Turn off`, which System Settings › Desktop & Dock undoes.

Fractile reads window titles and `AXDocument` paths for config-rule matching (including
`title_contains`) and for matching multi-window applications across restarts. They may be stored
locally in mode-`0600` `window-memory.json` solely for placement recall, and are explicitly
excluded from serialized observations, `workspace-state.json`, `parked-windows.json`, the
arrangement journal, `agent.log`, `fractile ctl debug`, `fractile doctor`, fixtures, and
`fractile hud` output.

The Workspace HUD shows each workspace's app icons. The runtime sends the agent each window's bundle
identifier and its layout rectangle, never a title, and the agent reads the icons from the installed
apps on the Mac. Nothing about the HUD is written to disk or logged.

Local files live under `~/Library/Application Support/Fractile/`:

- `config.toml`: user-authored configuration;
- `workspace-state.json`: active workspace selectors, previous-workspace history, the display
  selector each connected display (by its display UUID) showed last, logical slots,
  last-focused tile intent, Fullscreen Layout state, floating-window ownership, hand-pinned floating
  windows, and layout trees;
- `parked-windows.json`: physical process/window identifiers and original frames needed for recovery;
- `window-memory.json`: mode `0600`; per-application bundle identifiers with each remembered
  window's workspace, floating state, last frame, and optional local `AXDocument` path and window
  title, used solely to match and restore multi-window placements after relaunches and reboots;
- `agent.log`: timestamped diagnostic events (switches, parking, restoration, ownership changes) with
  process/window identifiers and frames, never window titles. Focus-follow switches also record
  how long ago a key, click, or modifier change happened and whether Command was held, never which
  keys were pressed; mode `0600`, moved to `agent.log.prev`
  (replacing any previous generation) past 5 MB at the next launch. Terminal launches keep
  diagnostics on stderr instead;
- `journal.jsonl` and `journal.jsonl.1`: the arrangement journal summarized by `fractile doctor`.
  It records process IDs, window-server IDs, bundle identifiers, frames, workspace ownership, parked
  state, and debounced model/screen disagreements, never window titles; mode `0600`, rotated past
  4 MiB with one previous generation kept. App-bundle and terminal launches both write it;
- `control.sock`: local Unix control socket, mode `0600`;
- `control.lock`: single-agent lock, mode `0600`.

`workspace-state.json` and `parked-windows.json` contain application process/window identity,
bundle identifiers, and geometry, never window titles or `AXDocument` paths. `window-memory.json`
is written atomically at mode `0600` and may store local window titles and `AXDocument` paths for
recall. The code uses atomic replacement for the JSON state files and restricts the Application
Support directory to mode `0700`; do not sync or publish recovery state without reviewing it.

## Native Interfaces

Most integration uses macOS Accessibility, AppKit, Carbon hotkeys, Core Foundation, and Core Graphics.
Fractile dynamically resolves the private `_AXUIElementGetWindow` function to associate an
Accessibility element with a Core Graphics window ID. If that symbol disappears or changes, window
discovery can fail. This private API prevents Mac App Store distribution and is a compatibility and
review risk for any public release.

See [SECURITY.md](../SECURITY.md) for vulnerability reporting.
