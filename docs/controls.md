# Controls

Spaceman registers global hotkeys while automatic tiling is enabled. Character keys follow the
current macOS input source; Spaceman refreshes registrations after an input-source change. An invalid
layout prevents startup registration and leaves tiling paused. During a layout change, Spaceman keeps
the existing registrations if it cannot construct a replacement plan; if replacement registration
fails after the old registrations are released, it pauses and reports `Hotkeys unavailable`.

| Keys | Action |
| --- | --- |
| `Option+1` ... `Option+9`, `Option+0` | Switch workspace 1 ... 10 |
| Add `Shift` to a workspace key | Move the focused window there without switching |
| `Option+A` ... `Option+Z` | Switch to that configured named workspace |
| Add `Shift` to a named workspace key | Move the focused window there without switching |
| `Option+Tab` | Focus the workspace focused before this one, on any display; only a hidden one switches its display |
| `Option+Shift+Tab` | Move the focused workspace to the next display (wrapping left-to-right) |
| Option-backtick | Focus the next window on the workspace: tiles in stable order, then floating windows, wrapping |
| Option-Shift-backtick | Focus the previous window in that same order |
| `Option+H/J/K/L` | Focus the nearest tile left/down/up/right, including across displays (with `[labs] recent_focus`, the most recently focused of several bordering tiles) |
| `Option+Shift+H/J/K/L` | Move the focused tile in that direction |
| `Option+/` | Toggle the active workspace layout's root horizontal/vertical axis |
| `Option+-` / `Option+=` | Shrink/grow the focused tile by 50 pixels at its immediate split |
| `Option+Shift+F` | Toggle the workspace-wide Fullscreen Layout without entering native fullscreen |
| `Option+Shift+Space` | Float the focused window above the tiles, or tile it again |
| Three-finger swipe left/right | Next/previous workspace on the display under the pointer |

A three-finger horizontal swipe steps through the display's visible workspace and every one of its
workspaces holding windows, stopping at either end. The direction follows
macOS Spaces and the Natural scrolling setting in Trackpad settings: with it on (the default),
swiping left goes to the next one; with it off, swiping right does. The setting is read on every
swipe, so changing it needs no restart. Spaceman only listens to the trackpad, so macOS still acts on the
same swipe: set "Swipe between full-screen applications" to four fingers or off in Trackpad
settings. Four-finger swipes (unless the Four-finger swipe Lab below is on) and vertical swipes are
left to macOS. To keep the macOS gesture and drop Spaceman's instead, turn on **Swipe switch** in
Settings › Labs (`[labs] swipe_switch = true`), then turn off **Swipe to switch workspace** in
Settings › Shortcuts (`trackpad_swipe = false`); see [Configuration › Trackpad
Swipe](configuration.md#trackpad-swipe). The switch exists only with that Lab on.

With **Four-finger swipe** on in Settings › Labs (`[labs] four_finger_swipe = true`), the swipe
takes four fingers instead, and three-finger swipes are left to macOS. That suits Accessibility's
"Three Finger Drag", which already moves macOS's own swipes to four fingers: set "Swipe between
full-screen applications" off in Trackpad settings so only Spaceman moves.

Every shortcut above can be rebound, unbound, or replaced in the config file's `[shortcuts]` table;
see [Configuration › Shortcuts](configuration.md#shortcuts). The table lists the built-in defaults.
Settings › Shortcuts shows each shortcut's current chord: click its keys and press a new chord to
rebind it (`esc` cancels, `⌫` clears it), or pick a Base modifier (`⌥`, `⌃⌥`, or `⌃⌥⌘`) to move
every built-in shortcut at once. Changes register live, with no restart.

Named workspace shortcuts are registered only when referenced by config. If a configured letter
overlaps a built-in shortcut, the workspace shortcut takes precedence. A `[shortcuts]` binding claims
its chord before both, so binding `alt+h` replaces the built-in Focus Left and a named workspace H's
switch shortcut too, and whichever was skipped is logged in `agent.log`.

Spaceman remembers the last-focused window (tile or floating) in each workspace and restores that
focus when the workspace becomes active again, keeping the remembered tile as a fallback if a float
closes, moves, or tiles. Pressing an already-active workspace shortcut again while one of that
workspace's windows already has focus cycles through its tiles in stable order (wrapping around, in
both split and Fullscreen Layout); if focus is on another display, the first press brings focus back
to the workspace and a second press cycles. Clicking an already-active workspace in the menu bar or
HUD preserves current focus.

Fullscreen Layout is a workspace mode: every tiled window receives the full display frame (less any
`settings.gaps` edge padding, and centered at `settings.single_window_max_width` when set, and at
`settings.single_window_max_height` with the `single_window_height` Lab), and the
focused window is raised above the others. Its menu-bar workspace glyph is filled while the mode is
active. Option-backtick focuses the next window and wraps
through the workspace's stable tile order, then its floating windows; it provides the same ordering in split layouts. In
Fullscreen Layout, `Option+H`/`Option+K` step to the previous tile and `Option+L`/`Option+J` to the
next one in that order without wrapping; past either end of the stack they continue to the adjacent
display, as in a split layout. Leaving the workspace hides its windows back to front, so stacked
windows are never uncovered mid-switch.
Command-backtick continues to use macOS's same-application window cycle, and Spaceman remembers the
resulting focus. Press `Option+Shift+F` again to restore the unchanged split layout. Fullscreen Layout
and its active tile survive new tiled windows (which join the stack and cue the Workspace HUD when
visible), workspace switches, native-tab physical-window replacement, and Spaceman
restarts. `Option+/` exits Fullscreen Layout, toggles the preserved split axis, and immediately applies
the resulting normal layout. Keyboard resize (including `resize width|height`), balance, and tile
reordering require leaving Fullscreen Layout first. Pointer dragging and resizing are ignored while the mode is active. Floating windows are
not expanded. Removing or moving away the workspace's final tiled window exits Fullscreen Layout.

`Option+Shift+Space` floats the focused window: it leaves the tiles, keeps its exact frame and
focus, and the remaining tiles re-fill the space it held. Floating is per window and follows that
window's workspace, so it parks and restores with a switch and survives an agent restart; it clears
when the window closes. A floating window moves, resizes, and moves between workspaces like any
other window. Dragging a floating window to another display leaves it with its workspace unless
the `Dragged floats follow their display` Lab is on (Settings › Labs, or `[labs]
float_follows_display`; see [Configuration › Labs](configuration.md#labs)): then a drag that ends
with the window's center on another display gives it to the workspace showing there, at the frame
it was dropped at and still floating. Press `Option+Shift+Space` again to put it back on the tiles,
where it joins them in stable order. Windows that a `float` rule keeps floating, or that their
application will not let Spaceman resize, stay floating; toggle them after removing the rule
instead.

Dragging a managed tile over a sibling swaps it. Dropping near a tile edge creates a nested split;
dropping inside a band along the display's left or right edge (including any outer gap, once the
pointer has been outside that band since the drag began) docks the window into a full-height,
half-width column on that side and keeps the remaining tiles' arrangement in the other half.
With `[labs] portrait_edge_rows = true` (see [Configuration › Labs](configuration.md#labs)), a display
whose tiled area is taller than it is wide also has bands along its top and bottom edges, by the same
rules: dropping there docks the window into a full-width, half-height row on that edge. These bands
also cover the menu bar and any Dock strip above or below the tiled area. Where a side band and a top
or bottom band meet, the side band wins, even before
the pointer has left it. Landscape and square displays are unchanged.
Dropping onto another display moves the window to that display's active workspace. Resizing a
managed window with the pointer updates the relevant split weights. Spaceman previews the rest of
the layout non-destructively against the layout at drag start and commits only on mouse-up, so
crossing a sibling or leaving a drop zone restores the preview and never leaves an intermediate edit
behind. Releasing where the last preview was shown commits that preview, unless the pointer went
somewhere new after it (out of an edge band and back, say, before the next preview could show):
then the release is classified afresh. A drag or resize that ends
without a mouse-up commit (pausing while the button is held, or a mouse-up whose window frame could
not be read) puts the siblings back where the saved layout has them.

While the worker is busy, keyboard commands and completed pointer gestures are queued in arrival
order. Repeated relative commands (resize, focus, move, and toggles) are not discarded. The queue holds
up to 64 pending inputs; excess input is rejected with `Input queue full; command was not queued` in
the menu status and log. Pausing, quitting, or losing hotkeys clears pending input so it cannot replay
after resuming. A gesture is classified from its starting and final geometry even if the worker was
too busy to show a preview. Pointer tracking reads window-server geometry rather than making
synchronous Accessibility requests on the main thread.

A workspace switch reveals incoming parked windows before hiding outgoing windows. If restoration
cannot be confirmed by the window server within the bounded reveal wait, the switch is rolled back:
the outgoing workspace stays active and pending incoming reveals are re-parked. Retry once the slow
application is responsive. Successful Accessibility writes alone do not confirm arrival.

## Send Tab to Workspace

`spaceman ctl move-tab WORKSPACE` sends the focused window's active tab to a workspace as a window of
its own; the rest of the tabs stay where they are. It presses the app's own `Move Tab to New Window`
menu command through Accessibility (found by AppKit's `moveTabToNewWindow:` identifier, which works
in every language, or by its title, including Chromium's `Move Tabs to New Window`), so it works in
Safari, Finder, Terminal, Ghostty, TextEdit, Preview, Chrome, Brave, and other apps with native tabs,
with no page reloading and no extra permission. The new window goes straight to the target workspace,
ahead of any `route` rule for the app, and focus stays on the window you were in. Add `--follow` to
switch to the target with the tab. If the window has a single tab, or the app has no such command,
`move-tab` moves the whole window, exactly like `move`. A tab whose window has not appeared within
800 ms is left where the app put it. `move-tab` has no default shortcut; bind one in
[`[shortcuts.bindings]`](configuration.md#shortcuts), such as `"ctrl+alt+shift+2" = "move-tab 2"`.

## Scratchpad (Lab)

With `[labs] scratchpad = true` (see [Configuration › Labs](configuration.md#labs)), Spaceman keeps an
i3/sway-style scratchpad: windows hidden off-screen until summoned. Its shortcuts register only while
the Lab is on, so a default install has none of them.

| Keys | Action |
| --- | --- |
| `Option+Shift+\` | Hide the focused window in the scratchpad, or take a shown scratchpad window out of it |
| `Option+\` | Summon the next scratchpad window onto the focused display, or hide the one shown there |

Sending a window to the scratchpad removes it from its workspace (the remaining tiles re-fill its
space), parks it off-screen and focuses the workspace's remembered tile. Summoning floats it above
the focused display's tiles, centred, at the size it last floated at (three quarters of the display
the first time, never larger than the display), and focuses it; the tiles stay as they were.
Pressing `Option+\` again hides it and returns focus to the tiles. With several windows in the
scratchpad, each press summons the next one in turn. A shown scratchpad window hides when its
workspace is switched away and stays hidden until summoned again, on whichever workspace is then
focused. `Option+Shift+\` on a shown scratchpad window tiles it on the current workspace. Moving it to
a workspace, tiling it with `Option+Shift+Space`, or closing it also takes it out of the
scratchpad. `Scratchpad is empty` in the menu status means there was nothing to summon.

A hidden scratchpad window is parked like a window on an inactive workspace, with its recovery frame
in `parked-windows.json`, so pausing, quitting, restarting, installing an update, a crash, or turning
the Lab off brings it back on screen, where it tiles again. Which windows are in the scratchpad is not
saved across agent restarts.

## Menu

The menu-bar control shows active workspace/layout glyphs and runtime errors. With
`[labs] menu_bar_icon_style = true` (Settings › Labs › `Menu bar icon style`), `Settings > General >
Menu bar icon` picks `Full` (a glyph per display, the default), `Compact` (one glyph, the focused
workspace's) or `Off` (no icon; open Spaceman again for Settings), `[settings] menu_bar_icon`
(see [Configuration › Labs](configuration.md#labs)). Its actions are:

- `Setup Tour…`, shown while Accessibility permission is missing;
- while tiling, the workspaces of each display (under the display's name when there are several):
  the visible one checked, plus every workspace holding windows, each with its apps' icons (names on
  hover) and its `Option` shortcut. Choosing one switches to it. With `[labs] physical_displays = true`
  (Settings › Labs › `Physical display order`) the glyphs and sections go left to right, portrait displays' glyphs are tall, and same-named
  displays are told apart by position (see [Configuration › Labs](configuration.md#labs));
- `Pause Tiling`, or `Resume Tiling` while paused;
- `Workspace HUD`, checked while the non-activating workspace capsule (`[settings] hud = true`) is enabled;
  `Settings > General > Workspace HUD` picks `Off`, `Icons`, or `Layouts` (`[settings] hud` and `hud_style`, default `Icons`),
  and, while the HUD is on, `Customize…` beside it opens a sheet whose `Position` picks `Bottom` or `Top`
  (`[settings] hud_position`, default `Bottom`) under a live miniature of the capsule;
- `Launch at Login`, shown checked when macOS has enabled the signed app bundle;
- `Settings...` (`⌘,`), opening the Rune Settings window (`General`, `Layout`, `Rules`, `Workspaces`, `Shortcuts`, with `[labs] activity = true`, `Activity`, and `Labs`, the experimental features' switches, plus `⇧⌘D` Playground);
- `Check for Updates...`, which checks and opens Settings (its sidebar and `General > Updates` show
  the result), or `Install Spaceman <version> and Restart` once an update is downloaded and verified;
- `Quit Spaceman` (`⌘Q`).

If macOS reports that the login item requires approval, the menu label says
`Launch at Login (Approval Required...)`; choosing it opens Login Items settings. Registration does
not bypass macOS consent and does not affect the currently running process.

`Settings > General` ends with a `Quit Spaceman` row for when the menu-bar icon is hidden (behind a
notch, or by a menu-bar manager): `Pause & restore windows` (shown while tiling) does what
`Pause Tiling` does, and `Quit` what `Quit Spaceman` does.

Pausing, quitting, and external macOS termination requests restore parked windows before completing.
Restoration retries are visible in the menu status. Installing an update uses the same restoration
gate: Spaceman swaps in the new bundle and relaunches only after every parked window is restored and
its recovery metadata is durably removed. If the install then fails, Spaceman keeps running and
resumes tiling. Quitting with an update ready installs it the same way, after the restore, for the
next launch. `Settings > General > Updates` shows the running version and the updater's state,
and picks the release channel (`Stable` or `Dev`, `[settings] update_channel`, default `Stable`).
Picking a channel other than the running build's offers `Switch to Dev` (or `Switch to Stable`),
and a copy Spaceman can't replace offers `Download`.

## CLI

Homebrew cask installs link the bundled CLI into `$(brew --prefix)/bin/spaceman` (typically
`/opt/homebrew/bin/spaceman`, already on `PATH`). Source installs (`scripts/install-app.sh`) link
`~/.local/bin/spaceman` (add `~/.local/bin` to `PATH` or invoke that absolute path), and direct ZIP
installs bundle the CLI at `/Applications/Spaceman.app/Contents/MacOS/spaceman`. Commands that
alter layouts require automatic tiling to be enabled.

```sh
spaceman ctl status
spaceman ctl debug
spaceman ctl switch 2
spaceman ctl switch next
spaceman ctl switch prev
spaceman ctl back-and-forth
spaceman ctl move-workspace
spaceman ctl move-workspace prev
spaceman ctl move-workspace secondary
spaceman ctl move 7
spaceman ctl move 7 --follow
spaceman ctl move next
spaceman ctl move prev --follow
spaceman ctl move-tab 2
spaceman ctl move-tab 2 --follow
spaceman ctl move-app com.tinyspeck.slackmacgap 6
spaceman ctl focus left
spaceman ctl focus next
spaceman ctl focus prev
spaceman ctl move-direction right
spaceman ctl layout
spaceman ctl fullscreen
spaceman ctl float
spaceman ctl pin
spaceman ctl resize shrink
spaceman ctl resize grow
spaceman ctl resize width 1200
spaceman ctl resize height 600
spaceman ctl balance
spaceman ctl scratchpad toggle
spaceman ctl scratchpad mark
spaceman ctl pause
spaceman ctl resume
spaceman ctl retile
spaceman ctl wrong
spaceman ctl restart
spaceman ctl quit
spaceman open spaceman://settings/workspaces
spaceman open spaceman://labs/physical-displays
spaceman ctl open features/gaps
spaceman hud
spaceman doctor
spaceman doctor 24
```

Directions are `left`, `down`, `up`, and `right`. `focus next` is Option-backtick,
`focus prev` is Option-Shift-backtick, `fullscreen` toggles the focused workspace's Fullscreen Layout, like `Option+Shift+F`, and `float`
toggles the focused window's floating, like `Option+Shift+Space`. `pin` floats the focused window
and shows it on every workspace, or unpins it (no built-in shortcut). `scratchpad toggle` and
`scratchpad mark` are `Option+\` and `Option+Shift+\`; while `[labs] scratchpad` is off they report
`Scratchpad is off`. `switch next|prev` and `move next|prev [--follow]` need
`[labs] workspace_steps = true` and have no built-in shortcut: `switch` steps through the focused
display's visible and occupied workspaces as a swipe does, and `move` sends the focused window to the
next or previous enabled workspace of its display, empty ones included; both stop at either end
(see [Configuration › Labs](configuration.md#labs)). `resize width|height POINTS` resizes the focused window to that width or
height (its own frame, gaps excluded) by moving its boundary at the nearest split along that axis;
the split's other tiles give or take the difference in proportion, never below 50 points. `balance`
equalizes the focused tile's immediate split while respecting nested minimum sizes; it does not
touch the workspace's other splits. Neither has a default shortcut (see
[Configuration › Shortcuts](configuration.md#shortcuts)). `hud` repeatedly renders sanitized
runtime geometry in the terminal until interrupted. `ctl wrong` marks the journal the moment something looks wrong and
writes a full arrangement snapshot, so `doctor` shows what the screen held then.

`ctl debug` excludes window titles. Its directional `targets` are the tiles `ctl focus` would focus
from the same state, including Fullscreen Layout, a focused float, an empty workspace and a focus
command still settling. It reports each display's Fullscreen Layout state and tiled
geometry plus floating-window workspace, parked state, and original recovery frame so workspace leaks
can be diagnosed safely. Its `parking_exempt` list names managed windows that repeatedly rejected
parking positions and are therefore left visible across workspace switches for the current session.

`doctor` summarizes the arrangement journal (all of it, or the last N hours): agent starts and their
loaded state, exit restorations, remembered placements recalled or left unclaimed, wrong placements
(windows the user moved to another workspace by hand within 120 seconds of Spaceman placing them, by
placement source: active workspace, recall, or rule), the times `ctl wrong` was run, and every
model/screen disagreement that lasted at least three seconds, grouped by kind with counts, total and
longest durations, and the applications involved. It ends with the last known per-workspace
arrangement. It reads files directly and works while the agent is stopped. The journaled kinds are:

| Kind | Meaning |
| --- | --- |
| `leaked` | owned by a hidden workspace, not parked, yet on a display |
| `stranded` | owned by a visible workspace and not parked, yet off every display |
| `parked_visible` | recorded as parked, yet on a display |
| `unadopted` | managed by configuration and on a display, but owned by no workspace |
| `constrained_overlap` | a tile that resisted resizing overlaps another visible tile on its workspace |
| `misplaced` | tiled on a visible workspace more than 16 pixels from its layout target |
| `missing` | owned, but the window server no longer lists it |
| `parking_exempt` | left visible across switches after repeatedly un-parking itself |

The agent listens at `~/Library/Application Support/Spaceman/control.sock`; both socket and lock file
are mode `0600`. Requests are newline-delimited, limited to 4 KiB, and served locally over a Unix
socket. The lock prevents two agents from running at once.

## Deep Links (`spaceman://`)

Spaceman registers the `spaceman://` URL scheme (and clickable web bounce links under
`https://anoma.ly/spaceman/labs/<slug>`, `https://anoma.ly/spaceman/features/<slug>`, and
`https://anoma.ly/spaceman/open/?to=...`) to open Settings directly onto a pane, feature, or Lab:

- `spaceman://settings` — open Settings
- `spaceman://settings/<pane>` — open `general`, `rules`, `workspaces`, `shortcuts`, `labs`, `layout`, or `activity` (plus optional `/<focus>`); `activity` falls back to `general` while `[labs] activity` is off
- `spaceman://labs` / `spaceman://labs/<slug>` — open Settings › Labs and highlight `<slug>`, a switch's key with `-` for `_` (`scratchpad`, `activity`, `portrait-edge-rows`, `physical-displays`, `recent-focus`, `swipe-switch`, `empty-workspace-focus`)
- `spaceman://features/<slug>` — open the Settings pane and highlight for a feature (e.g. `workspaces`, `rules`, `shortcuts`, `gaps`, `hud`)

Deep links only open Settings and highlight the requested pane or card. They never modify
`config.toml`, never enable a Lab, and never run window-tiling or destructive commands.
Unrecognized hosts, panes, or slugs log a diagnostic line and open Settings cleanly; malformed or
percent-encoded URLs are rejected. You can also drive deep links locally over `control.sock` with
`spaceman open <url-or-target>` or `spaceman ctl open <url-or-target>`.
