# Historical: Fractile Rust + GPUI Plan

> [!IMPORTANT]
> This is the original implementation plan, preserved as design history. It includes superseded scope
> and architecture statements. See [README.md](README.md) and [AGENTS.md](AGENTS.md) for current
> behavior and engineering invariants.

## The First Version Proves Stable Logical Windows

Ghostty uses native macOS tabs. In the tested configuration, selecting a tab exposes a different
Accessibility window identity even though the user still sees one terminal window. AeroSpace equates
those physical identities with layout leaves, so opening or selecting a tab can allocate another tile
and halve the terminal.

Fractile will make a different concept authoritative: a **logical slot** owns one place in the layout,
while its current physical `AXWindow` binding is replaceable. A Ghostty tab transition changes that
binding when the native evidence is unambiguous. It does not insert another logical slot.

The MVP is a signed Rust menu-bar application. GPUI owns the AppKit event loop and one authoritative
`Entity<Fractile>`. A narrow native shell provides the status item and macOS notifications. Dedicated
per-application lanes own Accessibility handles and return immutable Rust observations. The GPUI
foreground owner reconciles those observations into logical slots and desired frames.

The MVP succeeds when ordinary windows tile on the main display and recorded Ghostty tab sequences
preserve one logical slot without freezing Fractile, corrupting layout state, or requiring SIP changes.

## Product Principles

1. **Logical slots outlive physical windows.** A physical window ID is an observation, not product
   identity.
2. **Observation invalidates; reconciliation decides.** Native callbacks request a fresh observation.
   They never mutate the logical model directly.
3. **Incomplete observations never prove absence.** A timed-out application keeps its logical slots.
4. **One accepted observation produces one model revision.** Native writes then converge windows
   toward that revision with idempotent, best-effort operations.
5. **Ambiguity is data.** Fractile preserves the affected application's prior slots and retries rather
   than guessing.
6. **Native handles stay native.** `AXUIElement`, `AXObserver`, Core Foundation run-loop values, and
   Objective-C objects never enter the logical model or GPUI entities.
7. **GPUI is the ownership and UI shell.** It does not become a second window-management model.
8. **No SIP compromise.** Fractile uses public Accessibility, AppKit, and Core Graphics interfaces
   plus one dynamically resolved private window-ID function.
9. **One package until reuse is real.** Ordinary Rust modules are enough for the MVP.

## MVP Scope

The MVP includes:

- A signed `LSUIElement` application installed at one stable path.
- A native `NSStatusItem` with permission, pause, retile, diagnostics, and quit actions.
- An optional GPUI diagnostics window; no window opens during normal startup.
- Accessibility onboarding and capability diagnostics.
- Discovery of eligible windows on the main display and current native Space.
- One flat workspace with equal horizontal or vertical slots.
- Automatic reconciliation after app, window, focus, mouse, display, Space, wake, and periodic events.
- One isolated AX lane per observed application.
- Conservative Ghostty native-tab replacement.
- A global-hotkey capability spike and, if reliable, focus-next/focus-previous commands.
- Structured diagnostics and sanitized recorded observations from the first native milestone.
- A signed and notarized release artifact after behavior is proven.

The MVP excludes:

- Virtual workspaces.
- Managing multiple displays.
- Nested layout containers.
- General floating or matching rules.
- TOML configuration.
- A daemon, Unix socket, or CLI.
- Persistence of layout state.
- Native fullscreen or minimized-window management.
- Mouse-driven rearrangement.
- Animations, borders, gaps, and decoration.
- App Store distribution.

Those omissions keep the first test surface centered on native observation, logical identity, and
frame convergence.

## Platform Choices

- Language: Rust 2024 edition on a pinned stable toolchain.
- UI/runtime: GPUI pinned to the same reviewed Zed revision used by `cutman-rs` initially,
  `b2a4fb47b1d489f16e8b2f58ed746a9ad883c517`.
- Native bindings: the coherent `objc2` 0.6 / `objc2-*` 0.3 family.
- Product state owner: one `Entity<Fractile>` updated on GPUI's foreground thread.
- Native status item: direct `objc2-app-kit`, because upstream GPUI has no `NSStatusItem` interface.
- Accessibility: `objc2-application-services` and `objc2-core-foundation`.
- Display/window server values: `objc2-core-graphics`.
- Physical ID bridge: dynamically resolve `_AXUIElementGetWindow` from ApplicationServices.
- Global hotkeys: capability spike Carbon registration under frontmost Ghostty/Zed/Electron; use a
  dedicated `CGEventTap` only if Carbon is empirically unreliable.
- License: MIT.
- Distribution: direct signed/notarized app and Homebrew cask.
- Initial deployment target: macOS 15. Ghostty behavior is fixture-qualified by version, beginning
  with Ghostty 1.3.1 on macOS 26; releases publish the exact tested matrix.

Fractile does not use Bevy, an ECS, a Redux-like store, or a trait per native operation.

## System Shape

```text
AppKit / NSWorkspace / display / power / hotkey callbacks
                         |
                         v
                    NativeShell
                   RuntimeInput values
                         |
                         v
              Entity<Fractile> in GPUI
         scheduling + authoritative logical model
                         |
               start observation cycle
                         v
                    MacWindows
          one AX lane and CFRunLoop per process
                         |
                NativeObservation DTOs
                         v
                    Reconciler
             pure next model + decisions
                         |
                         v
                       Layout
               pure desired placements
                         |
                         v
                    MacWindows
        frame/focus commands + readback reports
```

## Authoritative State

Only `Entity<Fractile>` owns product state:

```rust
struct Model {
    workspace: Workspace,
    continuity: HashMap<AppInstanceId, AppContinuity>,
    next_slot_id: u64,
}

struct Workspace {
    axis: Axis,
    slots: Vec<Slot>,
}

struct Slot {
    id: SlotId,
    weight: Weight,
    binding: BindingState,
}

struct AppInstanceId {
    pid: i32,
    launched_at: ProcessStart,
}

struct PhysicalWindowId {
    app: AppInstanceId,
    window_id: u32,
}

struct AppContinuity {
    previous_focused: Option<PhysicalWindowId>,
}

enum BindingState {
    Bound(PhysicalWindowId),
    Unresolved {
        previous: PhysicalWindowId,
        candidates: Vec<PhysicalWindowId>,
        since: CycleId,
    },
}
```

`ProcessStart` comes from `NSRunningApplication.launchDate`. It distinguishes process incarnations
because macOS may reuse a PID. If launch time is unavailable, discovery assigns a conservative local
incarnation and replaces it whenever lifecycle evidence becomes discontinuous. A `CGWindowID` is
valid only for the current user session and physical window lifetime.

The model does not contain:

- AX or AppKit handles.
- Current native frames as authoritative geometry.
- Individual native tabs.
- A tab-group ID that macOS does not reliably provide.
- Copies owned by menus, views, hotkeys, or AX workers.

The latest observation, desired frames, worker health, pending invalidations, and diagnostic history
are runtime evidence. They may be retained alongside the model, but they do not become another
authority.

## Deep Modules

### `MacWindows` Hides Native Inconsistency

`MacWindows` exposes three operations:

```rust
fn observe(&self, request: ObserveRequest);
fn apply(&self, request: ApplyRequest);
fn focus(&self, request: FocusRequest);
```

Results return through `RuntimeInput`. Callers never receive a native handle.

Its implementation owns:

- Running-application discovery.
- One serial AX lane and CFRunLoop per process.
- Application and per-window observers.
- `_AXUIElementGetWindow` lookup.
- Typed AX reads, writes, errors, and messaging timeouts.
- Display snapshots and AppKit-to-AX coordinate conversion.
- Eligibility evidence from AX plus the Core Graphics on-screen window list.
- Frame capability checks, writes, readback, and bounded retries.
- Worker health and process teardown.

The deletion test justifies this Module: removing it would spread unsafe ownership, run-loop rules,
thread affinity, retries, coordinate conversion, and partial-observation policy across the app.

Each lane creates and owns the target application's `AXUIElement`, `AXObserver`, observed window
handles, callback storage, and run-loop source. Those generated `objc2` types are deliberately
`!Send` and `!Sync`. Only owned Rust DTOs cross threads.

### `Reconciler` Owns Logical Continuity

`Reconciler` is one pure interface:

```rust
fn reconcile(
    model: &Model,
    observation: &NativeObservation,
    evidence: &ReconciliationEvidence,
    output: OutputArea,
    settings: &Settings,
) -> Reconciliation;
```

It hides:

- Per-application complete versus unavailable evidence.
- Insertion and removal.
- Native-tab replacement.
- Ambiguity handling.
- Hidden/minimized eligibility transitions.
- Stable slot ordering.
- Diagnostic decisions.

`ReconciliationEvidence` contains the previous accepted observation and placement snapshot with their
topology revisions. Native-tab frame corroboration uses that historical evidence; recomputing a prior
target from current display state would be incorrect after an axis or topology change.

Recorded fixtures and production call the same interface. No fake AX adapter is needed to test it.

### `Layout` Owns Target Geometry

`Layout` accepts logical slots and one output rectangle. It returns one target rectangle per active
slot. It knows nothing about macOS, GPUI, or physical IDs.

The first algorithm partitions integral logical points in stable order. Target rectangles cover the
output area without overlap. Applied application frames may differ because applications impose
minimum sizes or character-cell increments; readback reports that difference rather than rewriting
the target invariant.

### `Fractile` Owns Runtime Serialization

The authoritative GPUI entity dispatches semantic commands and consumes runtime inputs. It owns:

- Debounce and periodic scheduling.
- Revision, cycle, power, and display-topology generations.
- Logical model commits.
- Pause and suspension policy.
- Desired placements.
- Diagnostics projections.
- Optional GPUI window handles.

Views render projections from this entity. Native menu actions and callbacks send values into it over
a channel; they never re-enter GPUI synchronously.

## Observation Is Per Application

```rust
struct NativeObservation {
    cycle: CycleId,
    topology: DisplayTopology,
    applications: Vec<AppObservation>,
}

enum AppObservation {
    Complete {
        app: ObservedApplication,
        focused: Option<PhysicalWindowId>,
        windows: Vec<ObservedWindow>,
    },
    Unavailable {
        app: AppInstanceId,
        error: ObservationError,
    },
    Terminated {
        app: AppInstanceId,
    },
}
```

Only `Complete` permits negative inference. `Unavailable` preserves that application's slots and
schedules a retry. `Terminated`, corroborated by `NSWorkspace` or process liveness, removes its slots.

One failed application does not prevent complete applications from reconciling. This is necessary
for a window manager: a hung Ghostty must not freeze the menu or stop TextEdit from tiling.

## Runtime Protocol

```rust
enum RuntimeInput {
    Invalidate(Invalidation),
    DebounceElapsed { revision: Revision },
    ObservationReady {
        cycle: CycleId,
        revision: Revision,
        observation: NativeObservation,
    },
    ApplyFinished {
        cycle: CycleId,
        revision: Revision,
        power_epoch: PowerEpoch,
        topology_revision: TopologyRevision,
        report: ApplyReport,
    },
    Command(Command),
}
```

The lifecycle is:

1. An invalidation increments `Revision`, merges its cause and scope, and replaces the debounce task.
2. Debounce expiry captures the current revision and starts one `CycleId`.
3. `MacWindows` requests snapshots from per-app lanes.
4. A response whose revision, power epoch, or display topology is stale is ignored.
5. `Reconciler` produces one next model and diagnostic decision list.
6. The foreground owner commits the model and desired placements together.
7. Placements are grouped by application and submitted to `MacWindows`.
8. Apply failures create diagnostics and another invalidation; they do not roll back logical state.
9. An invalidation during a cycle marks the runtime dirty and schedules another cycle.
10. The periodic backstop starts only while otherwise idle.

Dropping a GPUI task can cancel a timer or discard a logical result. It cannot stop an AX call already
executing inside macOS. Each lane therefore uses a finite AX messaging timeout, coalesces queued work
to the newest cycle, and reports unavailable state instead of blocking global progress.

Every `ApplyRequest` carries revision, cycle, power epoch, and topology revision. A lane checks a
shared watermark before each queued native call. Superseded queued writes are skipped. A write already
executing in macOS cannot be recalled; its result is discarded as stale and a fresh post-wake or
post-topology reconciliation repairs any physical effect.

Each lane drains a command mailbox from its owning `CFRunLoop`. A narrow `LaneWake` value can cross
threads only to signal a retained run-loop source and call `CFRunLoopWakeUp`; it never exposes AX or
run-loop objects. This is one explicitly audited unsafe `Send` seam. Observation aggregation captures
the expected app set and a cycle deadline. Lane death or deadline expiry synthesizes `Unavailable`,
so one missing reply cannot prevent `ObservationReady`.

## Native Tabs Are Conservative Physical Replacements

The MVP enables native-tab reconciliation only for Ghostty:

```text
com.mitchellh.ghostty -> FocusReplacement
all other apps        -> Disabled
```

Fork may be added after it has its own recorded fixtures. An allowlist is necessary because native
tab exposure varies by application and macOS release.

For Ghostty, the active physical set is the union of normalized `AXWindows` and `AXFocusedWindow`.
The focused handle is registered before stale handles are classified because those attributes can
temporarily disagree.

The safe first replacement requires:

- One complete Ghostty application observation.
- A newly observed focused physical ID.
- Exactly one new physical ID.
- Exactly one bound stale physical ID.
- The stale physical ID was the application's previous focused ID.
- The previous slot target and newly observed full frame match within tolerance.
- The left mouse button is not down.

If every condition holds, the new physical ID replaces the stale binding in the same slot. Slot ID,
order, weight, and target remain unchanged.

If stale/new candidates are ambiguous, affected slots enter `BindingState::Unresolved` and
reconciliation commits no insertion or removal for that application. Unresolved slots preserve their
logical position but receive no focus or frame writes. Other complete applications may still commit.
Fractile emits a sanitized `native_tab_ambiguous` decision and retries after settling or periodic
reconciliation.

A later unambiguous complete observation resolves the binding. If the same ambiguity remains after a
bounded settling window, Fractile stops rapid retries, keeps the application unresolved, and exposes
`ResyncApplication`. That explicit recovery rebuilds only the selected application's slots from its
current complete active windows and appends them deterministically; unrelated slots remain intact.
This favors visible, user-authorized recovery over a permanent stale binding or an automatic guess.

This policy cannot prove every native tab transition. A close-plus-create operation occurring wholly
between snapshots can be observationally indistinguishable from replacement. The MVP claims
correctness for recorded Ghostty transition sequences and conservative behavior elsewhere, not a
perfect tab oracle.

Required fixture cases include:

- Opening, selecting, and closing tabs in one group.
- Closing the current tab.
- Closing the complete tab group.
- Two independent Ghostty groups in one process.
- A real Ghostty window opening with no stale representative.
- A close and create inside one debounce interval.
- A tab detached into a real window.
- A missed notification followed by periodic reconciliation.
- An unavailable Ghostty observation.

Geometry is corroborating evidence, never general identity. Real top-level windows may overlap.

## Mouse And Drag Policy

Fractile does not fight user drags.

- Before committing topology changes or frame writes, query current left-button state through Core
  Graphics.
- While the button is down, continue recording invalidations but do not reconcile topology or apply
  frames.
- A global left-mouse-up observation triggers a settling debounce and complete observation.
- If mouse-up is missed, periodic reconciliation checks button state and recovers.

This one policy covers ordinary window dragging and native tab detachment. Mouse-driven layout edits
remain outside the MVP; an ordinary dragged window returns to its desired frame after release.

## Window Eligibility

The MVP manages a physical window only when all of these hold:

- It belongs to an application other than Fractile.
- The application is not hidden.
- AX role is `AXWindow` and subrole is `AXStandardWindow`.
- Position and size are readable and settable.
- It is not minimized or native fullscreen.
- Core Graphics reports the physical ID on-screen in the current native Space.
- Its center lies inside the main display's visible frame.

MVP behavior by state:

| Native state | Behavior |
| --- | --- |
| Eligible standard window | Managed |
| Minimized window | Excluded from active layout |
| Hidden application | Excluded from active layout |
| Native fullscreen | Unmanaged |
| Dialog, sheet, panel, or popup | Unmanaged |
| Fixed-size or immovable window | Unmanaged with diagnostic |
| Fractile-owned window | Always excluded |
| Observation unavailable | Preserve prior slots |
| Process terminated | Remove its slots |

Restored or unhidden windows enter at the end in the MVP. Preserving historical order requires
persistence and is deferred.

## Native Event Strategy

Callbacks are invalidations, not an event journal.

Main-thread native shell inputs:

- Application launch, termination, activation, hide, and unhide.
- Active native Space change.
- Display topology change.
- System sleep and wake.
- Global hotkey or mouse-up input.
- Permission changes and status-menu commands.

Per-app AX lane inputs:

- Application element: window-created and focused-window-changed where supported.
- Window elements: destroyed, moved, resized, minimized, restored, and title-changed where supported.

`kAXErrorNotificationUnsupported` degrades that application's observation capability and appears in
diagnostics. It does not reject the application. Full and periodic snapshots reconcile subscriptions
against current physical handles.

## Frame Application Converges Instead Of Pretending To Be Atomic

For each target whose observed frame differs, and which is not currently accepted as constrained for
that same target, the owning AX lane:

1. Confirms size and position remain settable.
2. Applies `size -> position -> size`.
3. Reads back the actual frame.
4. Retries a bounded number of times only after readback proves a mismatch.
5. Returns one result:

```rust
enum ApplyOutcome {
    Applied,
    Constrained { actual: Rect },
    Unavailable,
    Gone,
}
```

AX success means the message was accepted, not that the application honored the exact geometry.

The runtime keeps bounded apply evidence per physical window:

```rust
struct AppliedFrameState {
    target: Rect,
    actual: Rect,
    outcome: AppliedFrameOutcome,
    cycle: CycleId,
}
```

When an app constrains a target and later observations still match the accepted constrained `actual`,
Fractile does not retry on every periodic cycle. The constraint clears when the target changes, the
user requests `Retile`, the observed frame departs from the constrained actual, or the binding changes.
Therefore a user drag snaps back after release while a stable app-enforced minimum size does not loop.

Move/resize callbacks do not carry Fractile generations. Every callback still invalidates. Readback
and the bounded apply evidence decide whether another write is needed; recent timing alone never hides
a mismatching user move or application constraint.

## GPUI And Native Shell

GPUI owns `NSApplication` and the main AppKit event loop. Fractile must not start another main
`CFRunLoop`.

The pinned GPUI macOS platform currently forces regular activation policy during launch. Fractile's
launch callback resets `NSApplication` to accessory policy before opening any window. Milestone 0
must prove that this produces no persistent Dock icon. If it flickers or regresses, Fractile will
vendor a minimal `gpui_macos` activation-policy patch rather than spreading the workaround.

Upstream GPUI has no status-item interface. `NativeShell` directly owns and retains:

- `NSStatusItem`, `NSMenu`, and Objective-C action target.
- `NSWorkspace` and display-notification tokens.
- Global hotkey or event-tap registration.
- Native callback-to-Rust channel sender.

Native selectors only enqueue `RuntimeInput`. They never synchronously call into GPUI because native
notifications can arrive during an existing GPUI borrow.

The optional diagnostics/settings UI is a normal GPUI window. Closing it clears its handle but does
not quit the process or tear down native observation.

## Semantic Commands

All adapters dispatch the same commands:

```rust
enum Command {
    Retile,
    SetPaused(bool),
    Focus(Direction),
    SetAxis(Axis),
    ResyncApplication(AppInstanceId),
    OpenDiagnostics,
    Quit,
}
```

Setters are preferred over transport-level toggles. The status menu can translate a user toggle into
the current desired setter.

Cross-application focus is a hypothesis until the native spike proves a public sequence using
application activation, focused/main AX attributes, and `AXRaise`. If public focus is unreliable,
focus commands leave the MVP instead of silently adding more private APIs.

## Milestones

### Milestone 0: Prove The Rust + GPUI Native Shell

Deliver:

- One Cargo package and pinned toolchain/GPUI revision.
- Signed app bundle with `LSUIElement = true` and bundle ID `app.fractile`.
- GPUI event loop running with zero windows.
- Accessory activation policy with no persistent Dock icon.
- Native status item and menu callbacks reaching `Entity<Fractile>`.
- Accessibility trust check, prompt, and status refresh.
- Dynamic `_AXUIElementGetWindow` capability check and explicit unsupported state.
- Global-hotkey spike under frontmost Ghostty, Zed, Electron, and ordinary AppKit apps.
- Public cross-application focus spike.
- Build, check, and fixed-path install scripts derived from `cutman-rs`.

Acceptance:

- The installed signed app stays alive without a GPUI window.
- Opening and closing diagnostics does not stop the status item.
- Repeated builds at the same path and designated requirement retain Accessibility approval in the
  tested development environment.
- Missing permission or private symbol produces a visible diagnostic instead of partial operation.
- The selected hotkey mechanism works while Ghostty and Zed are frontmost, or navigation is explicitly
  deferred.

### Milestone 1: Produce Bounded Native Observations

Deliver:

- `MacWindows` with one AX lane and run loop per process.
- Finite AX messaging timeouts and worker health.
- Running-app, display, current-Space, and on-screen-window discovery.
- Typed attribute reads and physical window ID extraction.
- Application and per-window observer capability handling.
- Sanitized structured diagnostics and recorded raw observations.
- Complete, unavailable, and terminated application outcomes.

Acceptance:

- Opening, focusing, hiding, minimizing, restoring, and closing fixture windows produces coherent
  observations without moving them.
- A deliberately hung fixture application becomes unavailable while Fractile's menu and other app
  observations remain responsive.
- Ghostty tab transitions and two tab groups are captured as versioned fixtures.
- Secondary-display and native-Space windows are excluded without being moved.

### Milestone 2: Reconcile And Lay Out Pure Data

Deliver:

- Rust model, `Reconciler`, and `Layout`.
- Stable slot insertion/removal and process-launch identities.
- Ghostty physical-binding replacement and ambiguity decisions.
- Main-display rectangle partitioning in canonical AX coordinates.
- Fixture replay tests for every known tab sequence.

Acceptance:

- Ten tabs in one recorded Ghostty group retain one slot.
- Two recorded Ghostty groups retain two slots.
- A real new Ghostty window with no stale candidate inserts a slot.
- Ambiguous transitions preserve logical slots as unresolved and emit a retry decision.
- Persistent ambiguity stops rapid retries and can recover through explicit application resync.
- Unavailable applications never lose slots.
- Pure targets partition the selected visible frame without overlap.

### Milestone 3: Apply One-Display Frames

Deliver:

- Window eligibility and capability checks.
- Defensive writes, readback, bounded retry, and apply reports.
- Mouse-down deferral and settling reconciliation.
- Bounded apply evidence and idempotent move/resize feedback convergence.
- Pause and retile menu commands.

Acceptance:

- Two movable fixture windows divide the main display within one logical point.
- Opening and closing a real window converges without a persistent gap.
- Creating, switching, and closing Ghostty tabs does not change slot count or target frame for the
  recorded supported sequences.
- A constrained window reports its actual frame and does not create an unbounded write loop.
- A user drag whose observed frame departs from accepted constrained evidence retries the desired
  frame after release.
- Dragging never causes Fractile to fight the pointer before release.

### Milestone 4: Add Navigation If The Spike Passed

Deliver:

- Focus-next and focus-previous.
- Axis selection through status menu and global hotkeys.
- Hotkey conflict and permission diagnostics.

Acceptance:

- Navigation follows logical slot order rather than stale physical IDs.
- A Ghostty tab replacement preserves focus order.
- Unsupported focus behavior remains disabled rather than silently failing.

### Milestone 5: Harden And Publish

Deliver:

- Sleep/wake and display-topology recovery.
- Bounded in-memory diagnostics and sanitized export.
- Developer ID signing, hardened runtime, notarization, and stapling.
- Release artifact validation and Homebrew cask.
- Clean-machine onboarding and issue template.
- Published macOS/Ghostty acceptance matrix with fixture and integration-test versions.

Acceptance:

- Sleep during an observation or write cannot commit stale logical intent after wake.
- Display disconnect skips queued old-topology writes; an unavoidable in-flight native call is treated
  as stale and repaired by post-topology reconciliation.
- A clean machine installs, grants permission, and completes the Ghostty acceptance flow.
- Release validation covers the minimum supported macOS and current macOS on available hardware or
  dedicated CI hosts; untested combinations are not presented as guaranteed.
- Diagnostics explain every preserved ambiguity and unavailable application without exposing window
  titles by default.

## Test Strategy

The interface is the test surface.

Pure tests cover:

- Slot insertion, removal, and replacement.
- Process launch identity and PID reuse.
- Per-application unavailable evidence.
- Ambiguous stale/new transitions.
- Unresolved binding recovery and explicit application resync.
- Stale revision and topology rejection.
- Constrained apply evidence clearing after target or observed-frame changes.
- Weighted logical-point partitioning.
- Hidden/minimized eligibility changes.

Recorded observation tests cover:

- One Ghostty group with tab creation, selection, and closure.
- Two independent Ghostty groups.
- Real window creation during tab activity.
- Tab detachment.
- Missed callbacks followed by periodic reconciliation.
- Partial application failure.
- Displays with negative origins and mixed scale factors.

Opt-in macOS integration tests cover:

- Permission denial and grant.
- Private-symbol unavailability through an injected resolver.
- Observer registration degradation.
- Ordinary application insertion/removal.
- AX timeout isolation.
- Observation aggregation when a lane dies or misses the cycle deadline.
- Frame constraints and readback.
- Ghostty when installed.

## Initial Dependency Direction

```text
GPUI / AppKit callbacks
          |
          v
Fractile runtime + pure model
          |
          v
macOS native implementation
```

Likely dependencies:

- `gpui` and `gpui_platform` at the pinned Zed revision.
- `objc2`.
- `objc2-app-kit`.
- `objc2-application-services`.
- `objc2-core-foundation`.
- `objc2-core-graphics`.
- `objc2-foundation`.
- `flume` for native/runtime channels.
- `tracing` and one macOS-appropriate subscriber.
- `serde` only when observation fixtures first require serialization.

Use explicit native feature lists. Do not mix legacy `objc`, `cocoa`, or Servo Core Foundation types
into the native implementation.

Workspace lint policy denies unsafe code globally. Small native modules locally allow unsafe only for
FFI declarations, callback recovery, typed AX value creation, and Objective-C interop that generated
bindings cannot express. The audited `LaneWake` signal/wakeup handle is the only planned unsafe
cross-thread wrapper; it exposes no AX or run-loop access.

## Packaging And Permission Stability

Development follows the proven `cutman-rs` workflow:

- Build a complete `.app` bundle.
- Install it at `~/Applications/Fractile.app`.
- Use a stable bundle ID and signing identity.
- Verify the designated requirement and reject ad-hoc signing.
- Request Accessibility only from the installed bundle, never `cargo run`.
- Validate `LSUIElement` in the bundle checker.

Local development may use Apple Development signing. Public releases use Developer ID Application,
hardened runtime, notarization, and stapling. TCC retention is verified behavior for one stable path
and designated requirement, not a universal guarantee.

## Diagnostics Are Part Of Correctness

Use structured events from Milestone 1 with fields such as:

- `revision`, `cycle`, power epoch, and topology revision.
- Process start identity, PID, bundle ID, and physical window ID.
- Observation and apply duration.
- AX error, timeout, and observer capability.
- Decisions including `window_inserted`, `window_removed`, `native_tab_replaced`,
  `native_tab_ambiguous`, `observation_unavailable`, `stale_cycle_dropped`, and `apply_constrained`.

Window titles are omitted or hashed by default. A bounded ring supports the GPUI diagnostics window
and issue exports.

## Post-MVP Direction

Only after the native-tab MVP is stable:

1. Manage multiple displays while retaining one flat workspace per display.
2. Wrap workspaces in a `Desktop` and add virtual workspace projection.
3. Add nested weighted containers.
4. Add explicit floating rules.
5. Add versioned TOML configuration with last-known-good replacement.
6. Add a versioned, user-private Unix socket and thin CLI using the same semantic commands.
7. Add visual borders and optional layout feedback.

Inactive virtual workspaces may eventually park windows off-screen, but that behavior requires a
separate design for focus, displays, recovery, and visible slivers. It is not assumed by the MVP.

The load-bearing invariant must survive every later feature: native physical windows are observations;
logical slots are Fractile's product state.
