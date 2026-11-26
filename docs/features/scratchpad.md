# Scratchpad (Lab)

Keep a few windows (a terminal, notes, a chat) one key away: hide them off-screen and summon one
floating, centred, on whichever display you are using, then hide it again. Opt-in with
the `Scratchpad` toggle in Settings › Labs, or `[labs] scratchpad = true`; off by default.

## Sub-features

```ts
Send to the scratchpad (Option+Shift+\ / ctl scratchpad mark)      // scratchpad.mark
├── Focused tile or floating window leaves its workspace; remaining tiles re-fill its space
├── Recovery frame written to parked-windows.json, then the window parks off-screen
└── Focus returns to the workspace's remembered tile

Summon / hide (Option+\ / ctl scratchpad toggle)                    // scratchpad.toggle
├── Empty scratchpad -> status "Scratchpad is empty", nothing moves
├── None shown on the focused workspace -> next window floats there, centred, focused
│   └── Size: as it last floated, else 3/4 of the display, never larger than the display
├── One shown there -> it parks again and focus returns to the tiles
└── Several windows -> each summon takes the next one in turn

Leave the scratchpad                                                 // scratchpad.leave
├── Option+Shift+\ on a shown scratchpad window -> tiles on the workspace showing it, whatever
│   its route rule says (floats there when a rule or its fixed size keeps it out of the tiles)
└── Move it to a workspace, tile it (Option+Shift+Space), or close it

Stays safe                                                           // scratchpad.recovery
├── Switch away from a workspace showing one -> it hides, stays hidden until summoned
├── Pause / quit / restart / crash -> hidden windows restored on screen, then tiled
├── Lab switched off while windows are hidden -> they come back on screen
└── Membership is not persisted; titles never leave memory
```

Unit (FakeBackend), `src/app/tests/scratchpad.rs`:
`marking_a_tiled_window_parks_it_durably_and_reflows_its_siblings`,
`summoning_on_workspace_two_floats_it_centred_without_disturbing_the_tiles`,
`marking_a_shown_scratchpad_window_tiles_it_on_the_current_workspace`,
`scratchpad_mark_keeps_current_workspace` (`src/app/tests/residency.rs`, #414),
`toggles_cycle_through_several_scratchpad_windows`,
`a_closed_scratchpad_window_is_pruned_with_its_recovery_frame`,
`a_shown_window_hides_when_its_workspace_is_switched_away_and_stays_hidden`,
`pause_restores_hidden_scratchpad_windows_without_leaks`,
`a_crash_while_hidden_restores_the_window_on_the_next_start`,
`turning_the_lab_off_brings_hidden_windows_back`,
`the_scratchpad_does_nothing_while_the_lab_is_off`, and
`an_empty_scratchpad_toggle_is_a_clean_no_op`. Toggle order and summon frame: `src/scratchpad.rs`; membership is the window's residency in
`src/workspace/residency.rs`, so a scratchpad window is never also a float or a tile;
shortcut gating: `scratchpad_shortcuts_register_only_while_the_lab_is_on` in `src/hotkeys.rs`.

Live gauntlet, `scripts/gauntlet.sh`: `scratchpad mark` parks the window with its frame in
`parked-windows.json` while the rest re-fill, `scratchpad toggle` summons it on screen, floating and
focused, and `mark` on the shown window tiles it again.

## Gaps

- No live check drives the scratchpad shortcuts themselves.
- Activating a hidden window's app (Dock, Cmd+Tab) does not summon it.
