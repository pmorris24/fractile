# Known Limitations

Fractile is an experimental public preview, not a broadly qualified release.

- Builds are currently arm64-only; x86_64 and universal packaging are not validated.
- The declared macOS 14 minimum lacks a documented validation matrix across supported macOS releases.
- Clean-machine install, permission, upgrade, recovery, and uninstall have not been release-qualified.
- The app depends on private `_AXUIElementGetWindow` behavior and cannot target the Mac App Store.
- Accessibility behavior varies by application; only standard, movable, on-screen windows can be
  workspace-owned. Nonstandard or immovable panels remain global.
- Native-tab identity has no perfect public identifier. Ambiguous transitions intentionally defer
  replacement and may need another cycle.
- After a reboot or application relaunch, remembered placements restore each window's workspace and
  floating state, not its tile position, weight, or nesting within the layout. Without titles,
  windows of a multi-window application that reopen with identical frames are matched in creation
  order and may swap workspaces.
- Configuration supports ordered exact-match `float`/`tile`/`route`/`ignore` rules and display assignment. It
  intentionally does not support orthogonal named app spaces or regex title matching.
- Configuration changes to rules, display assignments, and `[settings]` apply automatically; newly
  referenced named workspace letters register their global shortcuts on the next app start. `ctl restart`
  exits safely but does not relaunch.
- Fractile assumes one macOS Space per display. Windows on another Space are adopted into the active
  workspace when that Space is shown, and tiles whose windows are ordered out while it is shown may
  be released. Layout waits about 0.7 s after each Space slide so the two Spaces' windows are not
  tiled together. During a trackpad swipe macOS can report the arriving Space's windows before
  it reports the Space change, and a window picked up then can briefly be tiled beside the outgoing
  Space's window (#431). Accessibility cannot reach a parked window on a Space that isn't showing, so
  pausing or quitting there waits, with its recovery entry kept, until that Space is shown.
- `secondary` and `tertiary` select the non-main displays from left to right; `tertiary` falls back
  to `secondary` when only two displays are connected. Duplicate display names may surprise.
- Global shortcuts may collide with other applications or fail on keyboard layouts that cannot
  produce required characters.
- Settings covers Accessibility, Launch at login, Workspace HUD, placement rules for an app's every
  window or for the windows a `title_contains` filter picks out, overrides for the built-in
  floats, workspace display assignments, keyboard shortcuts, and Activity diagnostics; exact
  `title` rules, `tile` rules for other apps and `settings.workspace_order` are edited in
  `config.toml`. There is no telemetry or analytics.
- The repository still lacks a concise product demo.

Use the isolated live harnesses in [Development](development.md) before changing window-management,
workspace, display, or recovery behavior.
