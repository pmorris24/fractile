# Historical: Research Behind The Rust + GPUI Plan

> [!IMPORTANT]
> This July 2026 research record informed the original plan. It is preserved as historical evidence,
> not current product documentation. See [README](../README.md) and
> [Compatibility](compatibility.md) for current claims.

## The Ghostty Failure Comes From Physical Identity Churn

Ghostty uses AppKit native tabs. In Ghostty 1.3.1 with AeroSpace 0.20.3-Beta on macOS 26, creating or
selecting a tab can expose a separate Accessibility window identity. A tiler that gives every physical
identity a layout leaf allocates space for hidden tabs.

Primary reports:

- [Ghostty: macOS Tiling Window Managers](https://ghostty.org/docs/help/macos-tiling-wms)
- [AeroSpace issue #68: Native macOS tabs are considered windows](https://github.com/nikitabobko/AeroSpace/issues/68)
- [Ghostty issue #3170: Creating a new tab splits the screen](https://github.com/ghostty-org/ghostty/issues/3170)
- [AeroSpace discussion #2071: Ghostty 1.3.1 report](https://github.com/nikitabobko/AeroSpace/discussions/2071)

This behavior motivates physical replacement, but native evidence does not create a perfect tab-group
identifier. Close-plus-create and tab replacement can be observationally identical if both happen
between snapshots. Fractile must record fixtures, match conservatively, and preserve ambiguous state.

## AeroSpace PR #2206 Proposes Slot-Preserving Replacement

[AeroSpace PR #2206](https://github.com/nikitabobko/AeroSpace/pull/2206), reviewed at commit
`a2795bc752a8c2d7cf2694c5bb5c43498454bbd4`, proposes reconciling Ghostty and Fork against each
application's current `AXWindows` list. A newly focused physical window replaces a stale one while
preserving parent, index, adaptive weight, cached geometry, and fullscreen state.

Candidate rules worth validating:

- Enable replacement only for known bundle IDs.
- Register the focused physical handle before pruning stale handles.
- Prefer the application's previous focused physical ID.
- Replace only an unambiguous candidate.
- Change the tree in one synchronous model operation.
- Defer native-tab reconciliation while the mouse button is down.

The PR was open and unmerged when researched on July 29, 2026. It is evidence for a promising
algorithm, not proof of complete native-tab identity.

## Current Rust Window Managers Prove The Native Path

### Paneru

[Paneru](https://github.com/karinushka/paneru), reviewed around revision
`500563b93db47046c4c34f8873d9a576b01e33a4`, is an MIT Rust window manager using `objc2` and native
macOS interfaces. Relevant implementation evidence includes:

- `_AXUIElementGetWindow` wrapping in `src/manager/windows.rs`.
- Observer and callback lifetime handling in `src/manager/app.rs`.
- Run-loop source setup in `src/util.rs`.
- Event-tap callback pinning and timeout re-enable behavior in `src/platform/input.rs`.
- Same-app full-frame matching as a native-tab signal in its layout tests.

Paneru proves Rust viability, but Fractile should not copy its Bevy/ECS architecture, relaxed AX
thread traits, or broad private WindowServer surface. Its geometric tab grouping is corroborating
heuristic evidence, not universal identity.

### Rift And Glide

[Rift](https://github.com/acsandmann/rift) demonstrates modern `objc2-*` Accessibility wrappers,
event-tap ownership, and direct `_AXUIElementGetWindow` use.

[Glide](https://github.com/glide-wm/glide) demonstrates the stronger production concurrency model:
one isolated AX actor/run-loop lane per target application. A blocked or unresponsive app does not
freeze the manager or other apps.

Fractile combines the coherent modern binding family with per-application isolation from its first
native observation milestone.

Cross-thread lane commands require an explicit wake mechanism. The narrow implementation sends plain
Rust commands through a mailbox, signals a retained Core Foundation source, and wakes the owning run
loop. Any unsafe cross-thread wrapper exposes only signal/wakeup behavior, never AX elements or direct
run-loop mutation. Observation cycles also need an expected-app set and deadline so a dead lane becomes
an unavailable result instead of blocking aggregation forever.

## Use One `objc2` Binding Family

The initial native dependency family is:

- `objc2` 0.6.
- `objc2-application-services` 0.3 for AX elements, observers, values, errors, and trust APIs.
- `objc2-core-foundation` 0.3 for retained ownership, strings, collections, and run loops.
- `objc2-core-graphics` 0.3 for window IDs, display values, on-screen window data, and event input.
- `objc2-app-kit` 0.3 for application lifecycle, screens, workspace notifications, and status UI.
- `objc2-foundation` 0.3 for Foundation values and notification ownership.

Generated AX and Core Foundation wrappers preserve conservative thread traits. Retained AX elements
and observers are not casually `Send` or `Sync`; the owning per-app lane exchanges only plain Rust
DTOs with GPUI.

Mixing old `objc`, `cocoa`, or Servo Core Foundation types into this layer would create duplicate
ownership models and unnecessary conversions.

## `_AXUIElementGetWindow` Is One Isolated Capability

Apple exposes no public AX-to-`CGWindowID` bridge. Mature window managers use
`_AXUIElementGetWindow`, exported by ApplicationServices/HIServices but absent from public headers.

Fractile dynamically resolves that symbol, validates both the returned status and nonzero ID, and
isolates the call in one native module. If unavailable, Fractile reports an unsupported capability and
does not guess identity from title or geometry.

The private symbol rules out Mac App Store distribution. It does not require disabling SIP.

## GPUI Should Own The Event Loop, Not Native Window State

`cutman-rs` pins GPUI and `gpui_platform` to Zed revision
`b2a4fb47b1d489f16e8b2f58ed746a9ad883c517`. That revision provides:

- The AppKit application loop.
- Foreground `Entity` ownership and async command handling.
- Optional normal, dialog, and floating GPUI windows.
- System wake callbacks.
- Application-local actions and key bindings.

It does not provide:

- `NSStatusItem`.
- General `NSWorkspace` app lifecycle observation.
- AX client permission or observers.
- Reliable global hotkeys.
- Agent activation-policy configuration.

The pinned macOS platform forces regular activation policy during launch. Fractile resets
`NSApplication` to accessory policy inside GPUI's launch callback and proves the behavior before
window management begins. A minimal vendored platform patch remains the fallback if that reset causes
Dock flicker or future regressions.

The native status item and menu use `objc2-app-kit`. Their callbacks enqueue commands into GPUI rather
than re-entering the foreground app synchronously.

Relevant GPUI source evidence:

- `gpui_platform::application()` constructs the macOS platform.
- `Application::run` owns the main AppKit event loop.
- `App::open_window` can lazily create diagnostics.
- `ApplicationHandle` warns against unsafe synchronous re-entry.
- Upstream [Zed PR #43822](https://github.com/zed-industries/zed/pull/43822) proposed activation-policy
  support but was closed without merge.

## Global Hotkeys Need A Capability Spike

GPUI key bindings work only while a GPUI window receives input. Fractile needs OS-level hotkeys.

Carbon `RegisterEventHotKey` is simple and avoids Input Monitoring permission, but reports indicate
that some self-drawn frontmost applications consume combinations before Carbon dispatch. A dedicated
`CGEventTap` is more controllable but adds Input Monitoring, callback timeout, and teardown concerns.

Milestone 0 tests Carbon under the exact foreground applications Fractile users care about: Ghostty,
Zed, Electron, and ordinary AppKit apps. If it fails, Fractile adopts a dedicated event-tap lane with
explicit permission diagnostics and timeout re-enable handling. The plan does not choose by folklore.

## AX Observers Need Run-Loop Ownership

Each application requires its own `AXObserver`. Notifications arrive only after its source is attached
to a running `CFRunLoop`. Callback context and retained elements must outlive queued callbacks.

The production topology is:

| Owner | Native responsibility |
| --- | --- |
| GPUI/AppKit main thread | `NSApplication`, status menu, screens, workspace/display/power notifications |
| One lane per target app | App/window AX handles, AX observer, run-loop source, reads, writes, retries |
| Optional input lane | `CGEventTap` and its run-loop source if Carbon fails |

Callbacks remain short, catch or prevent panic across FFI, and enqueue invalidation data. Removing a
notification does not immediately free callback context because a callback may already be queued.

## Partial Native State Must Stay Partial

AX calls can time out or return `kAXErrorCannotComplete`. Therefore a global snapshot is a collection
of per-application outcomes, not an atomic desktop truth.

Only a complete application observation may prove that a physical window disappeared. An unavailable
observation preserves that application's logical slots. Process termination is corroborated through
`NSWorkspace` or process liveness before removal.

`NSRunningApplication.launchDate` supplies process-start evidence alongside PID. This prevents a
reused PID from inheriting the prior process incarnation's logical continuity. If launch date is
unavailable, Fractile must conservatively create a new local incarnation after discontinuous lifecycle
evidence rather than assuming PID continuity.

This rule prevents one hung application from collapsing its tiles or contaminating other complete
applications.

## Accessibility, AppKit, Core Graphics, And One Private Function Cover The MVP

Required native sources include:

- AX application/window attributes and notifications.
- `NSWorkspace` lifecycle notifications.
- `NSScreen` display geometry.
- Core Graphics on-screen window data and input state.
- `_AXUIElementGetWindow` physical identity.

Important AX values include:

- `AXWindows`, `AXFocusedWindow`, `AXPosition`, `AXSize`, `AXRole`, `AXSubrole`, `AXMinimized`, and
  native fullscreen state.
- Application-level window-created and focused-window-changed notifications.
- Window-level destroyed, moved, resized, minimized, restored, and title notifications.

Unsupported notification registration is a degraded capability, not fatal discovery. Periodic full
observation is the correctness backstop.

## Frame Writes Are Best-Effort Convergence

Accessibility exposes position and size as separate writes, and applications may clamp or partially
apply frames. Mature tools use a defensive starting sequence:

1. Set size.
2. Set position.
3. Set size again.
4. Read back actual position and size.
5. Retry only after readback proves a mismatch.

Fractile applies only changed frames and reports constrained results. AX move/resize notifications do
not carry Fractile transaction IDs, so the MVP converges idempotently instead of pretending those
events can be tagged.

[Rectangle](https://github.com/rxhanson/Rectangle) and AeroSpace provide relevant frame-write prior
art. Glide adds bounded readback/retry and per-app isolation.

## Native Spaces Remain Outside The MVP

macOS has no supported public interface for creating, enumerating, or moving windows between Mission
Control Spaces. Yabai gains deeper control through private APIs and a scripting addition that can
require reduced SIP protections.

Fractile manages only on-screen eligible windows on the main display in the MVP. A native Space
change invalidates current observations; it does not imply that Fractile owns Space membership.

Post-MVP virtual workspaces require a separate design. Off-screen parking is one candidate, not an
assumed consequence.

References:

- [AeroSpace guide](https://nikitabobko.github.io/AeroSpace/guide)
- [yabai](https://github.com/koekeishiya/yabai)

## `cutman-rs` Supplies A Proven Local App Workflow

Fractile can reuse the shape of Cutman's scripts without copying media-specific behavior:

- Build a complete app bundle.
- Compile an asset catalog.
- Install at one fixed path.
- Sign with a stable identity.
- Verify a stable designated requirement.
- Terminate the prior installed binary before replacement.

Fractile adds `LSUIElement` validation and removes document, microphone, screen-capture, and Swift
runtime assumptions. Public release work adds Developer ID signing, hardened runtime, notarization,
and stapling.

## Comparable Projects And Licenses

| Project | Language | License | Relevant lesson |
| --- | --- | --- | --- |
| [AeroSpace](https://github.com/nikitabobko/AeroSpace) | Swift | MIT | Snapshot reconciliation and virtual workspace precedent |
| [Paneru](https://github.com/karinushka/paneru) | Rust | MIT | Rust native integration and tab heuristics |
| [Rift](https://github.com/acsandmann/rift) | Rust | MIT | Modern `objc2` wrappers and event-tap ownership |
| [Glide](https://github.com/glide-wm/glide) | Rust | Apache-2.0 OR MIT | Per-application AX isolation and frame convergence |
| [yabai](https://github.com/koekeishiya/yabai) | C | MIT | Deep native control with much higher OS/SIP maintenance |
| [Amethyst](https://github.com/ianyh/Amethyst) | Swift/Objective-C | MIT | Mature automatic tiling vocabulary |
| [Rectangle](https://github.com/rxhanson/Rectangle) | Swift | MIT | Narrow AX seam and reliable frame writes |
| [Loop](https://github.com/MrKai77/Loop) | Swift | GPL-3.0 | Polished manual movement but incompatible reuse license |

## Research Conclusion

The smallest credible Fractile is one Rust package with GPUI as foreground ownership and optional UI.
`MacWindows` turns inconsistent native state into immutable observations and idempotent commands.
`Reconciler` converts observations into stable logical slots and decisions. `Layout` partitions the
main display. Direct AppKit adapters provide only the status item and native callbacks GPUI lacks.

The architecture is prepared for a larger manager because native inconsistency and logical authority
have distinct seams. The MVP stays small because it implements one display, one flat workspace, one
Ghostty policy, and no generalized adapters before a second use exists.
