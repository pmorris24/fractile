# Fractile Contributor Guide

This file is the current engineering contract. README and `docs/` describe current user behavior;
`PLAN.md` and `docs/RESEARCH.md` are historical records.

## Working Here

- Agents merge their own Fractile PRs and redeploy (build and install) without asking.
- CI's macOS checks run only on dispatch and tags, so run `scripts/check` locally before merging
  and say so in the PR.
- Fractile installs only from source: `scripts/install-app.sh` builds the signed bundle and
  installs it to `/Applications`; `scripts/dev --once` does the same once.
- Dev loop: `scripts/dev` rebuilds and reinstalls on every save; restart restores every window and
  resumes tiling.
- Release notes are one file per version in `changelog/<version>.md` (frontmatter `date` and
  `summary`, then `## Major`, `## Improvements`, and `## Fixes`). `.github/` changes require
  CODEOWNERS review.
- Bundle ID is `app.fractile`. State and config
  paths (`~/Library/Application Support/Fractile`, `~/.config/fractile`) don't depend on it.
- Code shape: keep Rust files under 800 non-test lines (1,500 hard cap, 1,000 for test files;
  `scripts/file-size.sh`) and move prefix families (`x.rs` + 2+ `x_*.rs`, or 3+ `x_*.rs`) into `x/`
  (`scripts/folder-shape.sh`).

## Architecture Map

- `agent/`, `src/app.rs`, `src/app/app_loop.rs`, `src/app/app_loop/`, `src/app/worker.rs`,
  `src/app/bridge.rs`: single-process Rune host (`fractile-agent`), owner-only capped logging, 25 ms
  `NSTimer` foreground pump, action queue, status, restore-before-exit coordination, in-process
  Settings window (`General`, `Layout`, `Rules`, `Workspaces`, `Shortcuts`, `Activity`, `Labs`), first-run onboarding
  tour (`agent/src/onboarding.rs`, `agent/src/onboarding/`), non-activating Workspace HUD overlay (`agent/src/hud.rs`),
  Playground (`⇧⌘D` / `--playground`), and the worker boundary.
- `agent/src/controls.rs`, `agent/src/controls/`, `agent/src/design_tokens.rs`: the Settings design
  system — layout tokens (`METRICS`, `WINDOW`, `MODAL`, `KEYS`), `Paint` type roles, cards and
  rows, `modal_panel`, shortcut keycaps, and the ratchet test that keeps hand-typed measurements
  out of the panes. Rules, tables and how to render scenes: [docs/design-system.md](docs/design-system.md).
- `src/app/runtime.rs`, `src/app/cycle.rs`, `src/app/cycle/affinity.rs`, `src/app/switching.rs`,
  `src/app/parking.rs`, `src/app/commands.rs`, `src/app/backend.rs`: the worker-thread
  `WorkspaceRuntime` — observation cycles and affinity (new windows bound for another workspace
  join it in `src/app/cycle/routing.rs` before any display reconciles), workspace switches, durable
  parking/restoration, focused-window commands, and the native/persistence/clock backend seam.
  `src/app/parking/table.rs` is the one record per parked window (recovery frame, its display,
  settle/restore/summon phase, display re-park epoch) plus parking exemptions; parking reads time
  only through `WorkspaceBackend::now`, which tests step. Tests live in `src/app/tests.rs` and
  `src/app/tests/`.
- `src/onboarding_plan.rs`, `src/app/arrange.rs`, `src/app/app_loop/setup.rs`: the setup tour's
  suggested arrangement — a pure, title-free classifier over the open windows (bundle ID, Launch
  Services category, size, resizability, display width), and the worker's one-shot move of those
  windows, its inventory, and its undo. It never writes a `route` rule.
- `src/window_memory.rs`, `src/app/memory.rs`: advisory per-bundle placement memory and relaunch
  recall (storing local `title` and `AXDocument` in mode-`0600` `window-memory.json`).
  `src/journal.rs`, `src/journal/doctor.rs`, `src/app/journal.rs`: bounded arrangement/anomaly
  journal and the `fractile doctor` summary.
- `src/app/input.rs`: bounded ordered user input and geometry-based gesture tracking, independent
  of preview-worker availability. Main-thread pointer sampling must not make synchronous AX calls.
- `src/status_item.rs`, `src/login_item.rs`, `src/termination.rs`: main-thread
  menu bar status item, login registration, and asynchronous AppKit termination
  coordination on winit's application delegate.
- `src/hotkeys.rs`, `src/keyboard_layout.rs`: Carbon global shortcut adapters.
- `src/macos.rs`, `src/macos/`: Accessibility/Core Graphics observation and writes, display
  discovery, native event monitor, and native handle cache.
- `src/observation.rs`, `src/model.rs`, `src/reconcile.rs`: immutable observations, stable logical slots,
  and conservative physical-window reconciliation.
- `src/workspace.rs`, `src/runtime.rs`, `src/layout_tree.rs`, `src/layout_tree/`, `src/navigation.rs`:
  ten default numbered workspaces, opt-in named workspaces A-Z, ownership, nested weighted layouts,
  directional operations, and persistence representation. `src/workspace/residency.rs` holds each
  untiled window's one `Residency` (floating with its cause and pins, or Scratchpad); tiles live in
  the engines, and the two never overlap.
- `src/config.rs`, `src/config/disposition.rs`, `src/defaults.rs`, `src/serde_map.rs`: strict version-1
  TOML parsing, window disposition as a left-biased `DispositionPatch` (first matching rule over
  built-in default over window traits), state-path selection, and deterministic persisted-map
  serialization.
- `src/control.rs`, `src/main.rs`: mode-`0600` local Unix control protocol, CLI, dry-run/live probes,
  fixture replay, and HUD.
- `src/motion.rs`, `src/macos/display_link.rs`, `src/macos/presenter.rs`: the motion engine's
  foundation, not yet wired in: a closed-form camera spring, gesture velocity history and
  per-display column bounds (pure, cross-platform); a paused-until-started main-thread
  `CADisplayLink`; and the interactive presenter, which writes position-only frames through
  per-app lanes that coalesce to the newest frame and hold `AXEnhancedUserInterface` off for a
  motion.
- `src/geometry.rs`, `src/live.rs`: frame math, off-screen target selection, live observation
  completion, placement filtering, and CLI probe helpers.
- `src/permission_guide.rs`, `native/permission_guide.m`: Accessibility onboarding adapter.
- `scripts/`: icon generation, signed bundle assembly/install, and destructive live acceptance
  harnesses.

## Product Invariants

- A logical slot is product identity; a physical `AXWindow` binding is replaceable evidence.
- Native-tab replacement preserves slot, order, weight, nested layout, workspace, Fullscreen Layout,
  and active-tile intent.
- Incomplete or timed-out observations never prove absence. Ambiguous native-tab evidence preserves
  bindings during its bounded settling window; only repeated complete observations may retire an
  unresolved binding under the tested reconciliation policy.
- One accepted observation produces one coherent model revision; native writes converge toward it.
- Rules are ordered and first-match. Named workspaces use the same workspace identity and ordered
  route schema as numbered workspaces; do not introduce an orthogonal app-space schema.
- Automatic tiling starts when Accessibility permission and hotkeys are available. Documentation must
  not claim an unconditional paused startup.

## Recovery And Persistence Invariants

- Persist a window's original frame before moving it off-screen.
- Remove parked metadata only after restoration succeeds and the removal is durably persisted. Roll
  metadata back in memory if persistence fails.
- Pause, quit, restart, hotkey failure, and worker failure must favor restoration. Never
  recommend deleting `parked-windows.json` as the first recovery step.
- `workspace-state.json` must validate before restoration. Invalid config/state may be ignored so
  parked recovery can proceed; malformed parked state must not be silently discarded.
- Persist workspace/layout mutations atomically. Keep backward migration only for actual persisted
  formats already emitted.
- Never change state paths or schemas without migration/recovery tests and synchronized docs.

## Privacy And Security Invariants

- Accessibility is used only for window observation, focus, movement, resizing, and required input/
  display context, plus pressing the focused app's own "Move Tab to New Window" menu item when the
  user runs `move-tab`. Never require SIP to be disabled.
- Window titles and `AXDocument` paths may be stored locally in mode-`0600` `window-memory.json`
  solely for matching multi-window apps across restarts, and must never enter logs, `fractile
  doctor`, debug output, HUD output, fixtures, serialized snapshots, or control responses.
- No telemetry, network access, or update checks without explicit product and security review.
- Keep `control.sock` and `control.lock` at mode `0600`, bound under the user's Application Support
  directory, with bounded requests and single-agent locking.
- Treat `_AXUIElementGetWindow` as private, optional, dynamically resolved compatibility risk. Do not
  expand private API usage casually or claim Mac App Store support.

## Thread And FFI Invariants

- AppKit/status UI and APIs requiring it stay on the main thread.
- Accessibility observer run-loop objects stay on the dedicated AX event thread.
- Window-management work and its thread-local AX handle cache stay on the worker thread. The one
  exception is the interactive presenter's lane threads (`src/macos/presenter.rs`): each lane opens,
  uses and drops its own apps' AX handles, and only window IDs and points cross into it.
- Native handles (`AXUIElement`, `AXObserver`, Core Foundation run-loop values, Objective-C objects)
  never enter logical models, persisted state, or cross-thread messages.
- Native callbacks only invalidate/request observation; they do not mutate the logical model.
- Keep unsafe FFI narrow, typed, dynamically checked where applicable, and covered by safe Rust
  boundaries. Do not relax crate-wide unsafe policy to simplify an adapter.

## Live-Test Safety

- Disable AeroSpace and every other window manager before directly launching a process that applies
  frames. The repository live harnesses instead require AeroSpace initially, capture its baseline, and
  own the disable/restore transition.
- Prefer fixture replay and dry-run commands. Use `watch-window` or disposable Ghostty processes for
  scoped live tests.
- Read live harnesses before running them. Confirm expected displays, preserve baseline workspace and
  focus, and verify cleanup. Never experiment on unrelated user windows.
- Live harness state must use the internal `FRACTILE_APPLICATION_SUPPORT` override and must never read
  or mutate the user's normal workspace or parked-window state.
- Preserve `~/Library/Application Support/Fractile/parked-windows.json` until pause/quit restoration is
  confirmed. If a test crashes, reopen the same app and recover before cleaning state.

## Verification

```sh
scripts/check            # fmt, file-size, folder-shape, metadata, clippy, tests, audit, CA roots, replay, config

```

A Settings UI change also renders its scenes before and after with `scripts/snap-set.sh` (see
[docs/design-system.md](docs/design-system.md#verification)); look at every image.

Run `./scripts/live-ghostty-tabs.sh` and `./scripts/live-workspaces.sh` only when the change reaches
their live behavior, AeroSpace is initially enabled, and the other safety conditions above are
satisfied.

`scripts/gauntlet.sh` (docs/development.md › Live Gauntlet) is the end-to-end check for changes to
tiling, switching, parking, focus, reflow, float, the Scratchpad, or recovery. Run it on a disposable
macOS VM or a test Mac with `--host`, never on a Mac someone is using, and quote its receipt summary
in the PR.

## Documentation Synchronization

Update README and linked docs in the same change when menu labels, shortcuts, CLI commands, config
schema, environment variables, state paths/schema, privacy output, supported windows/displays,
packaging, startup, restoration, or uninstall behavior changes. Verify every command and label against
source. Keep historical documents visibly marked and never rewrite them as current truth.

`docs/features/` maps user-visible capabilities to entry points, existing checks, and known gaps.
Consult it before changing user-facing behavior, and update the affected feature file when a shortcut,
CLI command, rule semantic, restart/recovery outcome, or covering test changes.

## Release Rules

- A Developer ID-signed source build is not a public release artifact.
- Do not claim release readiness without notarization/stapling, a downloadable checksummed artifact,
  architecture and minimum-OS validation, and clean-machine install/upgrade/recovery/uninstall tests.
- Build once from a clean reviewed revision, notarize and distribute that exact artifact, and verify
  signature, ticket, architecture, plist, archive contents, and checksum after packaging.
- Keep the real app icon and add a concise demo before calling the preview broadly release-ready.
- Never tell artifact users they need a Developer ID identity; that is a source-build requirement.
- Never ship instructions to disable SIP, erase recovery metadata before restoration, or imply Mac App
  Store support.
