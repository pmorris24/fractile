# Restarts And Recovery

Quit, reinstall, crash, or reboot without losing windows off-screen, and come back to the same
workspaces and layouts; when something looks wrong, find out what happened.

## Sub-features

```ts
Spaceman restarts, apps keep running                       // recover.resume
└── workspace-state.json valid -> same workspaces, layouts, Fullscreen Layout, last focus

Pause / auto-pause on built-in display / quit / macOS termination   // recover.restore-on-exit
├── Every parked window moved back to its original frame
│   └── that frame's display unplugged -> moved onto the main display instead
├── Recovery entry removed only after the window server confirms arrival
│   └── Accessibility says "gone" but the window server still lists it -> entry kept, retried
│       for as long as it lasts (a window on another macOS desktop restores once that desktop
│       is shown); only the window server dropping it or its app quitting retires the entry
└── Restoration fails -> exit blocked, menu shows "Restoring windows · N failed", retries

Crash or hard kill                                         // recover.crash
├── Next launch, valid state   -> parked windows reveal as their workspaces become visible
├── Next launch, invalid/missing state -> every parked window restored first
└── Parked entry with no owner -> restored before automatic tiling resumes

Placement recall (reboot, fresh start, or app relaunch)     // recover.memory
├── Old slots for dead processes retire
├── During the first 60 s after Spaceman starts, each unseen bundle's first instance may claim
│   remembered placements
│   ├── recall closes early once every placement is claimed or the app quits
│   ├── exact AXDocument, then exact title, then fuzzy title, then frame (8 px), then size (8 px), then creation order
│   ├── a placement saved floating floats again at its remembered frame, on its workspace's
│   │   display, until Toggle Float tiles it
│   ├── route/ignore rules win; disabled/unmanaged workspaces skipped
│   └── apps launched after the startup window open on the active workspace
├── An app relaunched mid-session (a reinstall or restart) recalls its placements too: its new
│   instance appears within 30 s of the old one quitting, or while the old one still owns windows
│   (a quit is noticed whichever workspace the app was on, visible or hidden)
├── Kit moves a window back by hand within 120 s of it opening on the active workspace
│   -> journaled as a wrong placement; its app then recalls its placements on every launch
└── Arrangement changes recorded after holding 10 s (quitting does not shrink the memory)

Diagnose                                                   // recover.doctor
├── Settings › Activity ([labs] activity = true) -> 24h health, recent journal rows, procwatch
│                                vitals, Save snapshot
├── spaceman doctor [hours] -> starts, restorations, recalls, wrong placements, anomalies,
│                                last arrangement
├── spaceman ctl wrong -> marks the journal and snapshots the arrangement right now
├── spaceman ctl debug / spaceman hud -> live sanitized model
└── agent.log (+ .prev) -> decisions, slow worker commands, worker errors (not routine cancellations)
```

## How To Get To It (User POV)

```ts
Menu bar: workspaces (switch) | Pause Tiling (pause + restore) | Workspace HUD | Settings... (⌘,)
          | Quit Spaceman (⌘Q)
Settings: sidebar foot -> version
Settings: Activity ([labs] activity = true) -> Journal (Show in Finder) | Save snapshot
CLI:      spaceman ctl pause | resume | quit | restart | status | debug
          spaceman doctor [hours]      // reads files directly; works with the agent stopped
          spaceman hud
Implicit: macOS restart/log out/shutdown, scripts/install-app.sh,
          reopening Spaceman.app after a crash
Files:    ~/Library/Application Support/Spaceman/
          ├── workspace-state.json   parked-windows.json   window-memory.json
          └── journal.jsonl(.1)      agent.log(.prev)
```

See [Recovery](../recovery.md) for the safe manual procedure; never delete `parked-windows.json` first.

## Existing Checks

- Unit, `src/status_item.rs`, `src/macos/displays.rs`, `src/app/app_loop/builtin_display.rs`, `src/cli/tests/builtin_display.rs`, `agent/src/hud/tests.rs`, `agent/src/panes/general.rs`:
  dimmed paused menu bar glyph and full-brightness restoration on resume; `CGDisplayIsBuiltin`
  topology classification (laptop built-in, docked, clamshell, desktop Macs with 0 built-in
  displays); auto-pause on external disconnect and auto-resume on reconnect with manual pause and
  manual resume override safety; `pause_on_builtin_display` config/CLI/Settings round-trip; and
  transient HUD toast display even when Workspace HUD is off.

- Unit (FakeBackend): `parking.rs` `restore_all_repositions_windows_at_their_original_frames`,
  `restore_keeps_recovery_frame_until_the_window_server_confirms_arrival`,
  `persisted_frames_recover_after_a_simulated_crash`; `reliability.rs`
  `startup_recovers_parked_window_missing_from_valid_workspace_state`;
  `reconciliation.rs` `restored_slots_for_a_dead_application_are_retired`;
  `startup.rs` `invalid_startup_metadata_falls_back_so_parked_recovery_can_start`;
  `tear_regressions.rs` `restore_all_keeps_recovery_for_a_window_the_window_server_still_lists`,
  `restore_all_after_unplugging_lands_windows_on_a_connected_display`,
  `display_disconnect.rs` `restore_all_sends_a_window_from_an_unplugged_display_to_main` (#406),
  `restore_all_command_moves_recovery_frames_onto_the_current_displays`;
  `space_transitions.rs` `restore_keeps_recovery_for_a_parked_window_on_another_desktop` (kept
  past the Accessibility grace, restored once reachable; a closed window still lets exit finish).
- Unit (FakeBackend), `memory.rs`: startup recall by frame then order, no recall for instances that
  already own windows, route precedence, recall expiry, recall closing when the app quits,
  mid-session launches opening on the active workspace, mid-session relaunches (quit first or
  start first) returning to their workspace, floating placements staying floating
  (`a_recalled_floating_placement_keeps_the_window_floating`, issue 122), and settle-before-record. Pure tests in
  `src/window_memory.rs` and `src/journal.rs` cover matching, eviction, diffing, debouncing,
  rotation, and the doctor summary. None involves a real reboot.
- Live harness (not run): `scripts/live-workspaces.sh` hard-kills the agent with a parked window and
  checks startup recovery.
- Live gauntlet, `scripts/gauntlet.sh`: SIGKILL with a window parked keeps it parked and its record on
  disk, and a relaunch keeps it on its workspace; `ctl quit` returns every parked window to its
  recorded frame and empties `parked-windows.json`; `agent.log` has no panic or error line; `window-memory.json` (when present) is mode `0600` and no
  window title reaches any other state file, log, snapshot, or `doctor` output; `doctor` counts every start, every clean
  stop and no anomalies.
- Observed (2026-10-01, two reinstalls via `scripts/install-app.sh`): restoration before exit
  succeeded both times (one retry). The first restart silently dropped five hidden-workspace slots
  when Accessibility could not find them right after launch, then stalled 81 s; fixed in the working
  tree (slots are kept while the window server still lists the window). The second restart kept all
  seven slots.

## Gotchas

- **Reboot is unproven.** No test or observation yet covers: Spaceman restoring before other apps
  save their window positions at shutdown, or memory recall after login. On 2026-10-01 apps
  relaunched at Spaceman's off-screen parking edge after a reboot, suggesting they saved parked
  frames; read `spaceman doctor` after the next reboot.
- Memory restores the workspace and floating state during a recall, not tile position
  or weight; multi-window apps reopening with identical frames match by `AXDocument` and title
  first, falling back to creation order only when neither distinguishes them.
- `ctl restart` exits without relaunching.
- `doctor` only sees disagreements between the model and the screen. A window that is consistently on
  the wrong workspace looks healthy; its moves are in the journal diffs.
- Startup stalls (9 minutes after the 2026-10-01 boot, 81 s after a reinstall) are not yet
  explained; `slow worker command` lines in `agent.log` now record them.
