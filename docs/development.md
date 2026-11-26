# Development

Read the root [AGENTS.md](../AGENTS.md) before changing implementation. It defines architecture and
safety invariants. [PLAN.md](../PLAN.md) and [RESEARCH.md](RESEARCH.md) are historical design records,
not current product documentation.

## Dev Loop

```sh
scripts/dev          # build and install now, then again on every save
scripts/dev --once   # build and install once
```

Each install restores every window, swaps `/Applications/Fractile.app`, and resumes tiling, so
workspaces and layouts come back. `scripts/install-app.sh --bundle PATH --wait-idle SECS` installs a
built bundle once the keyboard and mouse have been idle.

## Verify

```sh
scripts/check            # fmt, clippy, tests, audit, replay, config

```

### Focused Reliability Checks

The reliability regressions in `src/app/tests/reliability.rs` exercise ambiguous multi-display tabs,
cross-file crash recovery, delayed window-server confirmation, and display-gap parking through the
fake backend. `src/app/input.rs` tests ordered input, queue limits, and complete gestures without
preview ticks. Layout and geometry tests cover resize-then-drag and multi-display parking. Run the
focused checks without touching desktop windows:

```sh
cargo test --locked --lib app::tests::reliability
cargo test --locked --lib app::input::tests
cargo test --locked --lib layout_tree::tests
cargo test --locked --lib geometry::tests
```

Read-only or dry-run macOS probes:

```sh
cargo run --locked -- config-check config.example.toml
cargo run --locked -- displays
cargo run --locked -- snapshot
cargo run --locked -- retile
cargo run --locked -- watch 20
cargo run --locked -p fractile-agent -- --size 1324x580 --playground Activity/activity --snap /tmp/activity.png
```

Settings scenes render headless, so a UI change carries its before and after
([Design system › Verification](design-system.md#verification)):

```sh
cargo run --locked -p fractile-agent -- --list-scenes
OUT=captures/after SIZES="860x580 920x660" THEMES="dark light" scripts/snap-set.sh general hud-customize
```

Commands with `--apply` and the menu-bar app move real windows. Disable every other window manager and
use disposable windows before running them. The live harnesses below are the exception: they require
AeroSpace to be enabled initially so they can capture a baseline, then they disable and restore it.

## Live Acceptance

```sh
./scripts/live-ghostty-tabs.sh
./scripts/live-workspaces.sh
```

`live-workspaces.sh` requires a current signed bundle at `target/Fractile.app`; build it with
`./scripts/build-app.sh` first. It isolates control and recovery files with
`FRACTILE_APPLICATION_SUPPORT`. `live-ghostty-tabs.sh` instead uses `cargo run` to start a scoped,
non-persistent `watch-window` process. Both restore their baseline but still exercise live
Accessibility writes. Start them with AeroSpace enabled, read each script, preserve unrelated
windows, and confirm its expected display and Ghostty assumptions. Never run a destructive live test
on a user's normal workspace merely because unit tests pass.

## Live Gauntlet

```sh
./scripts/build-app.sh
./scripts/gauntlet.sh                                 # on this Mac
./scripts/gauntlet.sh --host user@test-mac            # on another Mac over ssh
./scripts/gauntlet.sh --only switch,reflow --reflow-budget-ms 300
```

The gauntlet runs a real `Fractile.app` against three disposable windows and checks every step
twice: against the window server (`CGWindowListCopyWindowInfo` bounds, on-screen state, front
window, frontmost app) and against Fractile's own `ctl debug`, `ctl status`,
`parked-windows.json`, `agent.log` and `fractile doctor`. It covers boot and tiling, workspace
switches and parking, focus (directional, repeat switch, `focus next`), minimize and hide reflow
against a time budget, float, the Scratchpad, a hard kill and relaunch, and a clean quit that must
put every parked window back. Nothing is judged by eye.

Each run writes `receipt.json` (every step with its timing, expected and measured frames),
`snapshots/` (title-free `ctl debug` and window-server state after each step), `doctor.txt`, and
the run's `state/` (`agent.log`, `journal.jsonl`, `parked-windows.json`,
`workspace-state.json`) to `--out` (default `/tmp/fractile-gauntlet-<timestamp>`). It exits 1 if any
step fails.

Isolation: Fractile starts through LaunchServices with `FRACTILE_APPLICATION_SUPPORT` and
`FRACTILE_CONFIG` pointing into the receipt directory. Its config
ignores every app already running, so it only ever tiles, parks or focuses the gauntlet's own
windows, and each window is a separate process the gauntlet closes at the end. It refuses to start
while another Fractile, AeroSpace, yabai or Amethyst runs. Interrupting it quits Fractile (which
restores parked windows) and closes the test windows.

The target Mac needs a logged-in desktop and Accessibility granted to the Fractile.app under test.
The grant follows the Developer ID signature, so granting it once (System Settings › Privacy &
Security › Accessibility) covers later builds. Windows appear on that Mac's main display. A
disposable macOS VM is the best place to run it; `--host` copies both apps there over ssh and
copies the receipt back, and `GAUNTLET_SSH` replaces the ssh command (a key, a jump host).
The harness itself is `scripts/gauntlet/*.swift`, compiled by `gauntlet.sh` into
`target/gauntlet/FractileGauntlet.app` with `swiftc`; the target Mac needs no developer tools.

## Documentation Checks

When commands, menu labels, config fields, state paths, shortcuts, packaging, or recovery semantics
change, update README and the corresponding document in the same change. Inspect links, stale labels,
and whitespace:

```sh
rg -n '\[[^]]+\]\([^)]+' --glob '*.md'
rg -n 'Pause Tiling|Resume Tiling|Settings\.\.\.|Quit Fractile' --glob '*.md'
rg -n 'Automatic Tiling|Retile Now|starts paused|paused startup' --glob '*.md' --glob '!docs/development.md'
git diff --check
```
