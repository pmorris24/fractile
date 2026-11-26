# Configuration

Spaceman loads configuration when `spaceman-agent` starts and reloads it automatically whenever the
file changes on disk:

```text
~/Library/Application Support/Spaceman/config.toml
```

Set `SPACEMAN_CONFIG` in the agent's environment to replace that path. A missing file means default
configuration. Invalid configuration at startup is reported to stderr for terminal launches and to
`~/Library/Application Support/Spaceman/agent.log` for app-bundle launches; Spaceman falls back to
defaults and ignores workspace state so it can prioritize restoring parked windows. If an edit saved
while running fails validation, the running agent logs the error and keeps the previous valid
configuration.

Validate a file before replacing or restarting the app:

```sh
spaceman config-check "$HOME/Library/Application Support/Spaceman/config.toml"
```

The complete example is [config.example.toml](../config.example.toml).

## Current Schema

The parser rejects unknown fields. The root must contain `version = 1` and may contain an optional
`[settings]` table, an optional `[shortcuts]` table (see [Shortcuts](#shortcuts)), an optional
`[labs]` table (see [Labs](#labs)), plus repeated `[[rules]]` and `[[workspaces]]` tables.

```toml
version = 1

[settings]
hud = true                            # optional (default: true); non-activating Workspace HUD
hud_style = "icons"                   # optional (default: "icons"); or "layouts"
hud_position = "bottom"               # optional (default: "bottom"); or "top"
hud_stay = false                      # optional (default: false); keep the HUD on screen, windows tile clear of it
layout = "dwindle"                    # optional (default: "dwindle"); or "columns"
update_channel = "stable"             # optional (default: "stable"); or "dev", a build of every merge
mouse_follows_focus = "off"           # optional (default: "off"); or "display", "window"
trackpad_swipe = true                 # optional (default: true); read only with [labs] swipe_switch
menu_bar_icon = "full"                # optional (default: "full"); or "compact", "off"; read only with [labs] menu_bar_icon_style
pause_on_builtin_display = false      # optional (default: false); auto-pause on built-in display, resume on external
single_window_max_width = 0           # optional (default: 0, off); or 320 through 16384 points
single_window_max_height = 0          # optional (default: 0, off); or 320 through 16384 points; read only with [labs] single_window_height
workspace_order = []                  # optional (default: []); workspaces listed first, in this order

[settings.gaps]                       # optional; every key defaults to 0
inner = 0                             # points between tiled windows, 0 through 48
outer = 0                             # padding at every screen edge left out below, 0 through 128
top = 0                               # one edge's own padding in place of outer, 0 through 128
bottom = 0
left = 0
right = 0

[[rules]]
bundle_id = "com.example.Application" # optional exact match
title = "Preferences"                 # optional exact match
action = "float"                      # required: "float", "pin", "tile", "route", or "ignore"

[[rules]]
bundle_id = "com.apple.Safari"
title_contains = "Work —"             # optional; case-insensitive part of the title
action = "route"
workspace = 2                         # required for route; forbidden for the other actions

[[rules]]
bundle_id = "com.apple.Stickies"
action = "tile"                       # tiles an app Spaceman floats by default

[[rules]]
bundle_id = "ai.opencode.desktop"
action = "route"
workspace = "D"                       # named workspace A through Z
windows = "first"                     # optional, route only: "first" (default) or "all"; "all" needs [labs] route_all_windows

[[workspaces]]
workspace = 9                         # 1 through 10 or "A" through "Z"; unique
monitor = "main"                      # non-empty selector
```

A rule needs at least one of `bundle_id`, `title` or `title_contains`; every matcher present must
match. `bundle_id` and `title` match exactly; `title_contains` (not empty) matches when the window's
title contains it, ignoring case, so `"Work —"` catches a Safari profile's `Work — Pull Request #42`.
Rules run in file order and the first matching rule wins.

A window no rule matches gets Spaceman's built-in default. A short, fixed list of utility, note
and password apps floats at its own size: Stickies, Calculator, Dictionary, Clock, Weather, System
Settings, Passwords, Photo Booth, Voice Memos, Digital Color Meter, Archive Utility, Keychain
Access, 1Password (7 and 8), Bitwarden, Raycast, Alfred, CleanShot X and Hex (the list is
`FLOATING_BUNDLES` in `src/defaults.rs`). Every other window is managed normally. A matching rule
overrides the default for what it decides: `action = "tile"` tiles one of these apps again, and
`action = "route"` keeps it floating but opens it on the routed workspace.

- `float` excludes the window from tiling and resizing, preserves its native frame, and keeps it with
  its logical workspace (a remembered workspace during recall, or the active workspace where it was
  first observed).
- `pin` floats the window like `float` and shows it on every workspace of its display: switching
  workspaces never parks it and focusing it never switches workspaces. It still belongs to the
  workspace where it was first observed (memory and the HUD use that), and pause, quit and restart
  restore it like any other window. `spaceman ctl pin` pins or unpins the focused window by hand
  (floating it first if it is tiled); a hand pin survives an agent restart.
- `tile` tiles the window even when Spaceman would float it as a dialog, settings window, a
  built-in floating utility, or non-resizable window (see [Compatibility](compatibility.md)), like an unmatched resizable window.
  A window that refuses its tile size keeps its own size inside the tile. Toggle Float still floats
  it until toggled back.
- `route` assigns a newly inserted resizable logical window to workspace 1 through 10 or a configured
  named workspace A through Z. A bundle-level `route` rule (without `title` or `title_contains`)
  applies to the application's initial window while secondary windows opened later stay on the
  active workspace; a title-matched `route` rule routes every matching window. With the
  `route_all_windows` Lab on, `windows = "all"` makes a bundle-level `route` rule route every new
  window of its application too (see [Labs](#labs)). Movable
  non-resizable windows remain floating on the workspace where first observed. A rule does not
  repeatedly route existing slots, including a Ghostty native-tab physical replacement.
- `ignore` leaves the window entirely alone: never tiled, never owned by a workspace, never moved or
  parked. Use it for applications that insist on positioning their own windows. A previously managed
  window is released the next time it is observed on screen.

During startup recovery, after a qualifying mid-session relaunch, or for an application marked to
return, a window not matched by a `route` or `ignore` rule (including one matched by a `float` rule)
may return to a remembered workspace; see [Remembered Placements](#remembered-placements). Matching
`route` and `ignore` rules always win over memory.

Title matching (`title` and `title_contains`) occurs only in memory, against the live title of a
newly observed window. Titles are not serialized into persisted state, snapshots, `ctl debug`, or
the HUD; only the filter you write lives in `config.toml`.

## Workspace HUD

`settings.hud` turns the non-activating Workspace HUD capsule on (the default) or off, and
`settings.hud_style` picks `"icons"` (each workspace's number and an icon per window, the default) or
`"layouts"`. `settings.hud_position` puts the capsule on each display's `"bottom"` edge (the
default, just above the Dock) or its `"top"` edge (just below the menu bar); a top capsule slides
down into place and back up, and a bottom one rises and settles back down. A file without the key
keeps the bottom position.

`settings.hud_stay = true` (Settings › General › Customize… › Keep on screen) keeps the capsule
up instead of fading it after each switch. It then sits 8 pt from the area windows tile in
(above the Dock, below the menu bar) and every display keeps a 60 pt strip at that edge from its
windows, on top of `gaps`; the capsule redraws as windows open and close. It counts only with
`settings.hud` on.

```sh
spaceman config set settings.hud_position top
spaceman config get settings.hud_position
spaceman config reset settings.hud_position
```

In Settings › General, **Workspace HUD** picks `Off`, `Icons` or `Layouts`. While it is not
`Off`, **Customize…** beside it opens a sheet: a live miniature of the capsule over **Position**
(`Bottom` or `Top`) and a link to Settings › Workspaces, which decides the workspaces each display's
capsule lists. `spaceman config reset` removes a key that `spaceman config set` accepts, back
to its default; `settings.workspace_order` is edited in the file instead (see
[Display Assignments](#display-assignments)).

## Trackpad Swipe

`settings.trackpad_swipe` is read only while the `swipe_switch` Lab is on (see [Labs](#labs)); with
the Lab off, the three-finger swipe always steps through workspaces, whatever the key says. With the
Lab on, `trackpad_swipe = true` (the default) keeps the swipe, and `trackpad_swipe = false` stops it
from ever changing Spaceman's workspace, so macOS handles it as it would without Spaceman (for
example, moving between macOS desktops). Spaceman only ever listens to the trackpad, so it never
stops macOS from acting on the gesture.

```sh
spaceman config set labs.swipe_switch true
spaceman config set trackpad_swipe false
spaceman config reset trackpad_swipe
```

`spaceman config get trackpad_swipe` and `spaceman config show` print the value the file sets (or
`true` while unset), whether or not the Lab is on.

With the Lab on, Settings › Shortcuts shows **Swipe to switch workspace** under Workspaces, the same
switch; with it off, that row is the fixed `3-finger swipe` keys as before.

## Built-in Display Auto-Pause

`settings.pause_on_builtin_display` (default `false`) automatically pauses tiling and restores
windows when only a built-in laptop display is connected (zero external displays, at least one
built-in display), and automatically resumes tiling when an external display reconnects (provided
Accessibility permission is trusted). Each transition shows a brief non-activating HUD toast
(`Tiling paused — built-in display` / `Tiling resumed — external display`), even when the
Workspace HUD is turned off, and the menu bar shows a dimmed paused glyph while paused.

- **Manual override safety:** Manually pausing while an external display is attached is never
  overridden by reconnecting displays. Manually resuming while on the built-in display keeps
  tiling active until the connected displays actually change again.
- **Desktop Macs:** Macs with no built-in display (Mac Studio, Mac mini, Mac Pro) never
  auto-pause.

```sh
spaceman config set pause_on_builtin_display true
spaceman config get pause_on_builtin_display
spaceman config reset pause_on_builtin_display
```

Settings › General exposes the same toggle as **Pause on built-in display**.

## New-Window Layout

`settings.layout` chooses how newly opened tiled windows split the workspace:

- `"dwindle"` (the default): binary space partition. The first window fills the display, the second
  splits along the display's longer side (side by side on landscape, top and bottom on portrait),
  and each further window splits the focused (or most recently added) tile along that tile's longer
  side. Closing, moving away, or floating a window collapses its split so the surviving tiles
  re-expand, and any auto-chosen splits inside the space that grew re-orient outermost first to
  match their new longer side while keeping tile order and weights. Splits arranged by the user
  (`Option+/`, pointer drops, or perpendicular moves) keep their direction across closes and
  restarts; splits loaded from earlier saved state without a recorded origin are treated as
  user-arranged and keep their direction.
- `"columns"`: each new window joins the focused tile's split as an equal sibling.

```sh
spaceman config set layout columns
spaceman config get layout
spaceman config reset layout
```

Settings › Layout edits `settings.layout` under **New windows** over a live miniature preview that
switches between Dwindle and Columns. Changing it reloads immediately for newly opened windows
without rearranging existing workspaces.

## Gaps, Screen Edges, and Single-Window Width

`[settings.gaps]` spaces tiles out. `inner` is the distance between neighboring tiles, in every split
and at every nesting level; `top`, `bottom`, `left`, and `right` pad the tiled area away from each
screen edge (for example, room for a custom menu bar on top). Outer padding gives way before the
tiled area would get smaller than 320 × 240 points. Equal weights still give equal windows with gaps.

`outer` is shorthand for the four edges: it pads every edge without a key of its own. An edge that
has one uses it instead, even at `0`. This leaves 6 points between windows, 8 at the left, right,
and bottom, and none at the top:

```toml
[settings.gaps]
inner = 6
outer = 8
top = 0
```

`single_window_max_width` caps the width of a window that is alone on its display, either the only
tiled window of a workspace or any window in Fullscreen Layout. The window is centered at that width
inside the padded area; on a display that is narrower, it fills the area as before. It suits wide and
ultrawide displays, where one full-width window is hard to read.

`single_window_max_height` does the same for that window's height, and is read only while the
`single_window_height` Lab is on (see [Labs](#labs)). The window is then centered on both axes: at
the width cap when the padded area is wider, and at the height cap when it is taller. With
`single_window_max_width = 1440` and `single_window_max_height = 900`, a browser alone on an external
display is the size of a 1440 × 900 laptop screen. Each cap works without the other, and with the Lab
off the height key is ignored and a lone window keeps the full height, as before.

Out-of-range values are rejected with a message naming the key and its range, like any other invalid
configuration. Every key defaults to `0` (an edge to `outer`, itself `0`), so a file without them
tiles exactly as before.

Settings › Layout edits the same keys over a live miniature of the result. One **Window gaps**
slider sets `inner` and all four edges together; **Customize separately** (open on its own when the
file's gaps differ) sets between windows, all screen edges, and each edge on its own (**Top edge**, **Bottom edge**,
**Left edge**, **Right edge**). It shows the
padding each edge resolves to and leaves `outer` as the file has it: an edge you move there gets its
own key, and the rest keep following `outer`.

`spaceman config` reads and writes them by dotted key:

```sh
spaceman config set gaps.inner 8
spaceman config set gaps.outer 8
spaceman config set gaps.top 32
spaceman config set single_window_max_width 1680
spaceman config set single_window_max_height 1050   # read with labs.single_window_height on
spaceman config get gaps.inner
```

`get gaps.outer` prints `outer` itself (`0` while it is unset), `get` on an edge and `config show` the
resolved padding. Setting `gaps.outer` leaves every edge that has its own key alone. Setting a key to
`0` removes it from the file, except for an edge while `outer` is set: that edge keeps an explicit
`0`. To make an edge follow `outer` again, run `spaceman config reset gaps.<edge>` or delete its key
from the file.

## Shortcuts

`[shortcuts]` rebinds Spaceman's global shortcuts. Each entry in `[shortcuts.bindings]` maps a chord
to a command in `spaceman ctl` syntax:

```toml
[shortcuts]
defaults = true                       # optional (default: true); false registers only these bindings
modifier = "alt"                      # optional (default: "alt"); every built-in shortcut holds it

[shortcuts.bindings]
"alt+left" = "move-direction left"     # replaces macOS's Option+arrow word jump everywhere
"alt+shift+ctrl+left" = "focus left"
"alt+v" = "float"
"alt+j" = "layout"                    # replaces the built-in Option+J (focus down)
"alt+shift+b" = "balance"
"ctrl+alt+w" = "resize width 1200"
"alt+h" = "none"                      # unbinds the built-in Option+H
```

`modifier` moves every built-in shortcut at once: `"ctrl+alt"` makes Switch to workspace 3
`Control+Option+3` and Move to workspace 3 `Control+Option+Shift+3`. It takes the same modifier names
as a chord, needs `alt`, `cmd`, or `ctrl`, and can't include `shift`, which the built-in Move
shortcuts add. Bindings are exact chords and don't follow it.

A chord is one or more modifiers and one key, joined by `+`, in any order and case:

- modifiers: `alt` (also `opt`, `option`), `cmd` (`command`), `ctrl` (`control`), and `shift`.
  Every chord needs `alt`, `cmd`, or `ctrl`.
- keys: a single character such as `j`, `/`, `-`, `=`, or `` ` `` (found on whatever key types it in
  the active keyboard layout, like the built-in shortcuts); `1` through `9` and `0` (the physical
  number row, so `0` is workspace 10); `left`, `right`, `up`, `down`, `return` (`enter`), `tab`,
  `space`, `escape` (`esc`), `backspace` (`delete`), `forwarddelete`, `home`, `end`, `pageup`,
  `pagedown`, and `f1` through `f12`. `alt++` is the `+` key.

Commands:

| Command | Action |
| --- | --- |
| `switch WORKSPACE` | Switch to workspace 1 through 10 or an enabled named workspace A through Z |
| `move WORKSPACE [--follow]` | Move the focused window there without switching; `--follow` also switches to that workspace and follows the window |
| `move-tab WORKSPACE [--follow]` | Send the focused window's active tab there as its own window (and switch with `--follow`); a single-tab window moves whole ([Controls](controls.md#send-tab-to-workspace)) |
| `switch next\|prev` | Labs (`workspace_steps`): show the next or previous workspace of the focused display, taking the swipe's stops (its visible workspace and those holding windows, in `workspace_order`); stops at either end |
| `move next\|prev [--follow]` | Labs (`workspace_steps`): move the focused window to the next or previous enabled workspace of its display, empty ones included, as `move WORKSPACE [--follow]` does; stops at either end |
| `back-and-forth` | Return to the previous workspace |
| `move-workspace [next\|prev\|DISPLAY]` | Move the focused workspace to another display |
| `focus left\|down\|up\|right` | Focus the nearest tile in that direction (see `recent_focus` under [Labs](#labs)) |
| `focus next` | Focus the next window in the workspace: tiles in stable order, then floating windows |
| `focus prev` | Focus the previous window in that same order |
| `move-direction left\|down\|up\|right` | Move the focused tile in that direction |
| `layout` | Toggle the active layout's root split axis |
| `fullscreen` | Toggle Fullscreen Layout |
| `float` | Float the focused window, or tile it again |
| `pin` | Pin the focused window on every workspace (floating it first), or unpin it |
| `resize shrink\|grow` | Shrink or grow the focused tile |
| `resize width\|height POINTS` | Resize the focused window to that width or height (1 to 16384 points) |
| `balance` | Equalize the focused tile's immediate split |
| `retile` | Retile now |
| `none` | Unbind the chord: no built-in shortcut claims it |

`move WORKSPACE` leaves you on the current workspace; `move WORKSPACE --follow` switches to the destination so the moved window is in view. The CLI accepts `--follow` before or after the workspace.

`resize width POINTS` resizes the focused window to that width (its own frame, gaps excluded) by
moving its boundary at the nearest split that lays tiles out side by side; `resize height POINTS`
does the same at the nearest split that stacks them. The split's other tiles give or take the
difference in proportion to their sizes, and no tile goes below 50 points (nested splits keep room
for each of their tiles), so a size that does not fit is clamped. A window that refuses the
width or height it is given keeps that as its minimum, even when it first refuses during the
resize, and the requested size still lands when it fits beside it. A tile with no such split (a
lone tile, which `settings.single_window_max_width` sizes, or the height of a tile in a single
row) is left alone. Like `resize shrink|grow` and `balance`, it
asks you to leave Fullscreen Layout first. Neither `resize width|height` nor `balance` has a
built-in chord; bind them here.

A global shortcut fires in every app, so a chord that types something takes that key everywhere:
`alt+left` replaces macOS's Option+arrow word jump in text fields. Prefer `ctrl` or `alt+shift` for
rebinds that must not disturb typing.

Bindings claim their chords before the built-in shortcuts, so a binding on a built-in chord replaces
it and the other built-ins stay. With `defaults = false` only the listed bindings register, and the
table must bind at least one command (tiling needs at least one shortcut to start). Two spellings of
the same chord, an unknown modifier, key, or command, or a chord without `alt`, `cmd`, or `ctrl` is a
config error. A binding whose workspace is not enabled, or that the keyboard layout cannot type, is
skipped and logged in `agent.log` while the other shortcuts register.

Shortcut changes apply live: saving `config.toml` registers the new chords within a couple of
seconds, without a restart. If the new table can't be registered, the previous shortcuts keep working
and Settings shows `Shortcuts not applied` with the reason. Workspaces enabled by a config edit still
get their shortcuts on the next start. The menu-bar workspace list keeps showing the built-in
`Option`+number/letter hints next to each workspace. Settings › Workspaces names workspace 10's
keys from `[shortcuts]` (`Workspace 10 (⌃⌥0)` with a `ctrl+alt` base), and none when no shortcut
switches to it.

Settings › Shortcuts edits the same table. Click a shortcut's keys and press a new chord: it is saved
to `[shortcuts.bindings]` and registered at once, and the built-in chord it replaces gets `none`.
While Settings records, every global shortcut is held off so the chord reaches it (for at most a
minute). `esc` cancels, `⌫` or `Delete` on its own clears the shortcut (`none`), a chord without
`⌥`, `⌃`, or `⌘` is refused, and so is a chord another shortcut uses, naming it. Base modifier
sets `modifier`, and Reset to defaults removes the `[shortcuts]` table. Comments and other bindings
in the file are kept.

## Labs

`[labs]` turns on experimental features one by one. Every switch is off by default, and with all of
them off Spaceman behaves exactly as if Labs did not exist: no extra shortcuts register and no
command changes behavior. Unknown keys are config errors. Settings › Labs lists every switch,
grouped by what it changes (`Windows`: Scratchpad, Portrait edge rows, Recent focus, Dragged
floats follow their display, Single-window height, Route every window and Focus follows mouse; `Workspaces`: Swipe switch, Focus empty workspaces on other
displays, Workspace steps and Four-finger swipe; `Menu bar & HUD`: Physical display order and Menu bar icon style; `Settings`: Activity), and flipping one there
writes the same key.

```toml
[labs]
scratchpad = false                    # optional (default: false); the i3/sway-style Scratchpad
activity = false                      # optional (default: false); Settings › Activity
portrait_edge_rows = false            # optional (default: false); top/bottom drop bands on portrait displays
physical_displays = false             # optional (default: false); menu bar in physical display order and shape
recent_focus = false                  # optional (default: false); Option+H/J/K/L returns where you were
swipe_switch = false                  # optional (default: false); lets trackpad_swipe turn swipes off
empty_workspace_focus = false         # optional (default: false); see below
float_follows_display = false         # optional (default: false); dragged floats join that display's workspace
workspace_steps = false               # optional (default: false); switch/move next|prev
four_finger_swipe = false             # optional (default: false); the workspace swipe takes four fingers
single_window_height = false          # optional (default: false); reads settings.single_window_max_height
route_all_windows = false             # optional (default: false); reads a route rule's windows = "all"
menu_bar_icon_style = false           # optional (default: false); reads settings.menu_bar_icon
focus_follows_mouse = false           # optional (default: false); the tile the pointer rests on takes focus
```

`scratchpad = true` enables the Scratchpad (see [Controls › Scratchpad](controls.md#scratchpad-lab)) and
registers its two shortcuts, `Option+\` and `Option+Shift+\`, live without a restart (or on the next
hotkey registration if tiling is paused); a `[shortcuts]` binding on either chord takes it instead. The
`spaceman ctl scratchpad` commands follow the switch live. Turning the switch off while windows are
hidden in the scratchpad brings them back on screen, where they tile again.

`activity = true` adds Settings › Activity, the live arrangement log, journal health, process vitals,
and Save snapshot (see [Restarts and Recovery](features/restarts-and-recovery.md)). With it off the pane
stays out of the Settings sidebar and `⌘6` opens Labs instead; `spaceman doctor` and the journal
keep recording either way. Turning it off while the pane is open returns Settings to General.

`single_window_height = true` makes Spaceman read `settings.single_window_max_height` (see
[Gaps, Screen Edges, and Single-Window Width](#gaps-screen-edges-and-single-window-width)): a window
alone on its display, or in Fullscreen Layout, is centered at that height when the padded area is
taller, as `single_window_max_width` does for its width. It adds **Single-window height** under
Settings › Layout › Wide displays, with Off, 900, 1050 and 1200 (the 16:10 heights of the width
presets 1440, 1680 and 1920) and any other height `config.toml` sets. With it off, the key is
ignored and its row is hidden. It applies live. In Settings it is
`Labs › Windows › Single-window height`.

`route_all_windows = true` makes Spaceman read `windows = "all"` on `route` rules (see
[Current Schema](#current-schema)): every new window of the rule's application goes to the rule's
workspace, not only its first one, so a Chrome window another app opens while you work elsewhere
still lands on Chrome's workspace. Windows already open stay where they are. `windows` is allowed
only on `route` rules; `"first"`, or no key, keeps today's behavior. With the Lab off the key is
ignored. `spaceman rules add --app <bundle-id> --workspace <ws> --windows all` writes such a rule,
and Settings › Rules keeps the key while the rule routes and drops it when the rule stops routing.
In Settings it is `Labs › Windows › Route every window`.

```toml
[labs]
route_all_windows = true

[[rules]]
bundle_id = "com.google.Chrome"
action = "route"
workspace = 4
windows = "all"
```

`focus_follows_mouse = true` focuses a tile when the pointer comes to rest over it, without a
click, so its keyboard shortcuts work right away. The pointer has to move and then stay still for
about 150 ms: passing over a tile on the way to the menu bar does not switch the app the menu bar
belongs to, and a pointer left resting never takes focus back after a keyboard focus change or a
workspace switch. Only tiles on a visible workspace take focus this way: a floating window, the
Scratchpad, or an unmanaged window in front of the pointer keeps focus where it is, and hovering
one does not focus it. Nothing changes while a mouse button is held, a menu is open, a drag or a
macOS Space slide is under way, or the pointer is over the menu bar, the Dock, the Workspace HUD or
Settings. The pointer resting where `settings.mouse_follows_focus` put it is not a hover, so the
two work together without feeding each other. In Settings it is
`Labs › Windows › Focus follows mouse`.

`swipe_switch = true` makes Spaceman read `settings.trackpad_swipe` and turns the fixed three-finger
swipe row in Settings › Shortcuts into the **Swipe to switch workspace** switch (see
[Trackpad Swipe](#trackpad-swipe)). With it off, `trackpad_swipe` is ignored and swipes work as before.
In Settings it is `Labs › Workspaces › Swipe switch`.

`four_finger_swipe = true` makes the workspace swipe take four fingers instead of three, for anyone
who uses Accessibility's "Three Finger Drag" (which moves macOS's own swipes to four fingers).
Three-finger swipes are then left to macOS, and the swipe row in Settings › Shortcuts says
`4-finger swipe`. `settings.trackpad_swipe` and `workspace_order` apply to it as before. macOS
still acts on its own four-finger swipes, so turn "Swipe between full-screen applications" off in
Trackpad settings. It applies live. In Settings it is `Labs › Workspaces › Four-finger swipe`.

`float_follows_display = true` makes a floating window you drag to another display belong to the
workspace showing there, as a dragged tile already does. When the drag ends with the window's center
on another display, it joins that display's workspace and keeps its frame and its floating. Switching
that display hides and brings it back with its new workspace, switching its old display no longer
does, and Option+Tab, Option-backtick, the HUD, the menu, restarts and remembered placements all use
the new workspace. A drop on the same display, a display showing no workspace, a pinned window, a
window shown from the Scratchpad, and moves made by the app or by macOS change nothing. With it off,
a dragged float stays with the workspace it had.

`portrait_edge_rows = true` adds drop bands along the top and bottom edges of a display whose tiled
area is taller than it is wide: dragging a tiled window into one docks it into a full-width,
half-height row on that edge (see [Controls](controls.md)). With it off, only the left and right edge
bands exist. The switch applies to the next drag.

`physical_displays = true` makes the menu bar follow the displays on your desk. The workspace glyphs
and the menu's display sections go left to right, then top to bottom, the order `Option+Shift+Tab`
moves through; a portrait display's glyph, and its previews in the Workspace HUD's Layouts style, are
drawn tall; when displays share a product name, each section says which one it is (`DELL U2720Q
(left)`, `DELL U2720Q (right)`) in place of the `(1)` / `(2)` macOS adds; and VoiceOver reads each
glyph's workspace with its display. Rearranging, rotating, connecting or disconnecting a display
updates them on the next refresh. With it off, or with one landscape display, the menu bar is
unchanged. In Settings it is `Labs › Menu bar & HUD › Physical display order`.

`menu_bar_icon_style = true` makes Spaceman read `settings.menu_bar_icon` and adds **Menu bar icon**
to Settings › General, with `Full`, `Compact` and `Off`. `"full"` (the default) is today's icon: a
layout glyph and workspace label for each display. `"compact"` draws one glyph and label, for the
workspace you last focused (the first display's before anything has focus); the menu still lists
every display. `"off"` hides the icon and its menu. Settings stays reachable: open Spaceman again
(from Spotlight, Launchpad or Finder) or run `spaceman ctl open-settings`, and Settings › General
keeps Automatic tiling, Pause & restore windows and Quit. With the Lab off, the key is ignored and
the row is hidden, so turning the Lab off brings a hidden icon back. It applies live. In Settings it
is `Labs › Menu bar & HUD › Menu bar icon style`.

```sh
spaceman config set labs.menu_bar_icon_style true
spaceman config set menu_bar_icon compact
spaceman config reset menu_bar_icon
```

`recent_focus = true` changes which tile `Option+H/J/K/L` and `spaceman ctl focus` pick when several
tiles on the focused tile's workspace border it on the side pressed: those that share part of its
row or column with no nearer edge between. Of those, the one focused most recently wins, so pressing
one way and back returns to where you were (with `1 | (2 / 3)`, left from 3 and right again lands on
3, not 2). Every way a tile gets focus counts (keys, clicks, `Cmd+Tab`); focusing a floating window
changes nothing, and a native tab keeps its tile's place. The order lives only in memory, and tiles
not focused since Spaceman started rank last; with none of them focused, the nearest choice stands.
A further tile never wins for being more recent. Crossing displays, focus from a floating window or
an empty workspace, Fullscreen Layout, and `Option+Shift+H/J/K/L` are unchanged. It applies live. In Settings it is
`Labs › Windows › Recent focus`.

`empty_workspace_focus = true` makes an empty workspace chosen on another display the focused
workspace for commands. Showing an empty workspace focuses nothing, so the window on the display you
came from keeps keyboard input and, with the switch off, commands such as `Option+Shift+Tab` and
`Option+Shift+F` keep acting on that window's workspace. With it on they act on the empty workspace
you chose, wherever it shows, until another window takes focus (see
[Workspaces](features/workspaces.md)). The switch takes effect live. In Settings it is
`Labs › Workspaces › Focus empty workspaces on other displays`.

`workspace_steps = true` turns on `switch next|prev` and `move next|prev [--follow]` (see the
command table above) for `spaceman ctl` and `[shortcuts.bindings]`, and lists them in Settings ›
Shortcuts › Workspaces as Step to next workspace on display, Step to previous workspace on display,
Move window to next workspace on display, and Move window to previous workspace on display (not to be
confused with Option+Tab's Previous workspace). None has a built-in chord. With the switch
off, bindings to them are skipped (the log names the switch) and `spaceman ctl` reports
`Workspace steps are off`. `switch` acts on the focused display (with no visible window focused, the
display `move-workspace` would use); `move` reports why and moves nothing when no window has focus. In Settings it is
`Labs › Workspaces › Workspace steps`.

```sh
spaceman config set labs.activity true
spaceman config get labs.activity
spaceman config reset labs.activity
```

## Remembered Placements

Window identity in workspace state is tied to a running process, so it cannot survive a reboot or an
application relaunch. Spaceman therefore also remembers, per application bundle identifier, which
workspace each managed window lived on, whether it floated, and its last on-screen frame. It never
records window titles.

A new application process that owns no windows yet may claim its remembered placements in four
cases:

- **Startup recovery:** during the first 60 seconds after Spaceman starts, when an application
  bundle is seen for the first time in the session (such as after a reboot or a Spaceman start
  without valid workspace state).
- **Mid-session relaunch:** after the startup window, when another process of the same application
  quit within the last 30 seconds or still owns windows (for example, an in-app update or restart
  that launches the new process before the old one exits).
- **Restored arrangements:** whenever the application's memory spans two or more workspaces and
  its new process presents two or more windows at once (macOS reopening the windows it had when it
  quit), however long it was closed. A fresh launch that opens one window still opens on the active
  workspace.
- **Returning applications:** when you move a newly opened window to another workspace by hand
  within 120 seconds of it opening on the active workspace, Spaceman marks that application to
  return so later launches recall its remembered placements as well.

Matching `route` and `ignore` rules always take precedence over memory (an application with a
bundle-level `route` rule does not open a recall, and per-window `route` and `ignore` rules skip
recall). An ordinary later launch without a qualifying relaunch or returning mark opens on the
active workspace instead of recalling historical placements.

During a 60-second recall window, eligible windows claim remembered placements: first a window whose
frame matches a remembered frame within 8 pixels, then one whose size alone matches within 8 pixels
(a note reopened somewhere else), then the remaining windows in creation order. A
claimed window joins its remembered workspace (hidden if that workspace is not visible, and floating
again if it was saved floating). Windows beyond the remembered count, windows opened after the
recall window closes, and placements on workspaces that are no longer enabled or managed are adopted
on the active workspace.

An application quit while some of its windows were parked reopens them at the parking spot, one
pixel inside a display corner. Spaceman brings back every such window it did not park itself (not
only the focused one), cascading them onto that display, so recall and tiling can adopt them. Multi-window applications that restore windows with identical frames are
matched by creation order, which is a best guess.

A changed arrangement replaces an application's memory only after it holds for 10 seconds while the
application keeps running, so the windows closing one by one as an application quits do not erase
the arrangement it should return to. Closing every window leaves the previous memory in place.
Memory is advisory: an unreadable or invalid file is logged and ignored. It holds up to 32 windows for
each of 128 applications, evicting the least recently changed application.

## Display Assignments

Defaults assign workspaces 1-4 to `main`, 5-7 to `secondary`, and 8-10 to `tertiary`. `monitor`
accepts:

- `main`, the display macOS marks as main;
- `secondary`, the leftmost attached non-main display;
- `tertiary`, the next non-main display to the right of `secondary`;
- an exact display UUID from `spaceman displays`;
- an exact display name from `spaceman displays`. When two displays share that name, the first
  one `spaceman displays` lists is used, and Settings › Workspaces shows the workspace under that
  same display; use the UUID to pick the other.

```sh
spaceman displays
```

A missing `tertiary` falls back to `secondary`, so with two displays workspaces 5-10 all show on
the secondary display. A missing `secondary`, UUID, or name falls back to the main display. Spaceman retains the preferred
assignment as its own active lane; reconnecting that display makes its assigned active workspace
visible again.

Settings › Workspaces offers every connected display. With two displays the choices are `Main`
and `Secondary`; with three or more, Main stays `Main` and every other display is named after
itself, numbered when two share a name (`Studio Display 1`, `Studio Display 2`). A display's card
always carries its own name. Its picker segment says `Display 3` instead, by its position, when the
name is longer than 16 characters (`Built-in Retina Display`) or four or more displays share the
picker, and the card's caption repeats that number. Assigning the third display writes
`tertiary`; assigning a display past that writes its UUID (the same selector `Option+Shift+Tab` persists), and a
saved UUID renders back as that display's name, in the same words as its picker segment and
card. A row set to a display other than its built-in default says which default it differs
from (`Default: Studio Display 1`); a row already on its default says nothing, including one
saved by the UUID of the display that is also Secondary. A saved display that is unplugged
keeps a `Disconnected displays` card so its assignment is never lost.

Named workspaces are opt-in: reference a letter in a route or `[[workspaces]]` entry to enable it.
They default to `main`. An enabled letter registers `Option+letter` to switch and
`Option+Shift+letter` to move the focused window there. The named workspace shortcut takes
precedence over a built-in letter shortcut when they overlap. A named workspace still referenced by
persisted active/previous history, tiled or floating windows, or Fullscreen Layout remains enabled
across config edits until that state no longer refers to it.

`settings.workspace_order` sets the order a display's workspaces appear in the Workspace HUD, the
menu bar menu, and three-finger swipes. Listed workspaces come first, in the order given; every other
workspace follows in the default order (1 through 10, then A through Z), so a partial list is fine
and an empty or missing list changes nothing. The key is set in the file only: `spaceman config
set` and `spaceman config reset` do not accept it. Entries are workspaces `1` through `10` or
`"A"` through `"Z"`, as numbers or strings; an unknown or repeated entry is rejected. The order only
changes where workspaces are listed: listing a letter does not enable it, and shortcuts, routes and
saved state are unaffected. For the top keyboard row:

```toml
[settings]
workspace_order = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, "Q", "W", "E", "R", "T", "Y", "U", "I", "O", "P"]
```

## Process Scope

`SPACEMAN_MANAGED_DISPLAY` is an internal process-scope override used by isolated tests. It accepts
`main`, `secondary`, `tertiary`, or `both` (the default). It is not a TOML field, and the installed app does not
provide a UI for setting it. A scoped process registers only workspace hotkeys assigned to its managed
main/secondary/tertiary selectors; normal users should leave it unset.

`SPACEMAN_APPLICATION_SUPPORT` is another internal test override. It replaces the Application Support
directory for the default config path, state, logs, and control files so live harnesses cannot touch
normal recovery data.

## Applying Changes

Edits saved in Spaceman Settings or in an external editor take effect automatically: rule changes,
workspace display assignments, `settings.hud`, `settings.hud_style`,
`settings.hud_position`, `settings.mouse_follows_focus`, `settings.trackpad_swipe`,
`settings.workspace_order`, `settings.gaps`, `settings.single_window_max_width`, and
`settings.single_window_max_height` reload without restarting (Settings › Layout retiles at once; an
external edit applies on the next retile);
`settings.update_channel` applies at the next update check. Global shortcuts for a
newly added named workspace letter (`Option+A` ... `Option+Z`) register on the next app start:

```sh
spaceman ctl pause
spaceman ctl status
spaceman ctl quit
open "/Applications/Spaceman.app"
```

`spaceman ctl restart` safely restores parked windows and exits, but currently does not relaunch the
app by itself. The source installer uses it before replacing the bundle, then opens the new bundle.
