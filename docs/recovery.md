# Recovery And Uninstall

Fractile moves inactive-workspace windows beyond visible display edges, checking that no neighboring
display is covered (apart from a one-pixel-wide slice of the title bar at a bottom corner, which macOS
keeps on screen). Before doing so, it atomically records each original frame in:

```text
~/Library/Application Support/Fractile/parked-windows.json
```

**Preserve that file until pause or quit restoration is confirmed.** Deleting it first can discard
the only record of where an off-screen window belongs.

## Normal Recovery

1. Ask Fractile to pause: `fractile ctl pause`.
2. Poll `fractile ctl status` until mode is `paused`, not `restoring`.
3. If needed, ask it to quit: `fractile ctl quit`.
4. Wait for `pgrep -x fractile-agent` to return no process.
5. Check that formerly hidden windows are visible before changing state files.

Pause and quit retry failed restorations. Do not kill the process while the menu says `Restoring
windows` unless it is irrecoverably stuck.

## After A Crash

Reopen the same installed app without deleting state:

```sh
open "/Applications/Fractile.app"
```

If workspace state is absent or invalid, startup first attempts to restore every window recorded in
`parked-windows.json`. If valid workspace state exists, the app resumes that persisted workspace
model and restores windows as their workspaces become visible. Parked entries without an owner in
that model are recovered before automatic reconciliation; this handles a crash between persisting a
park and persisting its new workspace ownership. A failed or unconfirmed recovery retains its entry
and prevents automatic reconciliation until a retry succeeds. Then use `fractile ctl pause` if the
goal is to return all windows and stop tiling. If Accessibility permission or startup hotkey
registration is unavailable, startup remains paused and attempts to restore all parked windows.

If the app will not stay open, preserve a copy of the whole state directory before manual diagnosis:

```sh
cp -R "$HOME/Library/Application Support/Fractile" \
  "$HOME/Desktop/Fractile-recovery-state"
```

Do not edit physical window IDs or frames casually. They are recovery metadata, not a supported
configuration surface. A stale entry for a window that no longer exists is harmless compared with
losing metadata for a live parked window.

## Diagnosing Odd Arrangements

When windows end up on the wrong workspace, stay visible after a switch, or vanish, run:

```sh
fractile doctor        # the whole journal
fractile doctor 24     # the last 24 hours
```

The arrangement journal (`journal.jsonl`) records a full snapshot of every window's owner, frame, and
parked state at startup and every 30 minutes, a diff whenever that arrangement changes, and the
start and end of each model/screen disagreement that lasts three seconds. It includes agent starts,
exit restorations, and remembered placements recalled after relaunches, so a reboot that lost
placements shows whether restoration ran before exit and which windows were recalled afterwards.
See [Controls](controls.md#cli) for the anomaly kinds. Share journal excerpts only after reviewing them:
they contain bundle identifiers and geometry, though never window titles.

## Safe Uninstall

1. Turn off `Launch at login` in Fractile Settings and confirm it is disabled.
2. Disable automatic tiling with `fractile ctl pause`.
3. Wait for `fractile ctl status` to report `"mode":"paused"` and `"busy":false`.
4. Run `fractile ctl quit` and wait for `fractile-agent` to exit.
5. Visually confirm that parked windows returned.
6. Re-enable the previous window manager only after Fractile exits.
7. Remove the app and CLI link.

```sh
rm -rf "/Applications/Fractile.app"
rm -f "$HOME/.local/bin/fractile"
```

Only after restoration is confirmed may you remove configuration and state:

```sh
rm -rf "$HOME/Library/Application Support/Fractile"
```

Remove Fractile from **System Settings > Privacy & Security > Accessibility** (System & App Protection
Override on macOS 27.3 and later) if desired. Fractile
does not install a daemon, kernel extension, system extension, or network service.
