# New Window Latency

## Goal

Reduce user-visible latency from `AXWindowCreated` event arrival to the first application of the
correct Fractile frame without weakening reconciliation, cancellation, fallback recovery, or idle
efficiency.

## Benchmark

```sh
cargo test --release benchmark_new_window_event_to_correct_frame -- --ignored --nocapture
```

Primary metric: `new_window_correct_frame_ms`, the median event-arrival-to-correct-frame latency
over nine measured event-loop phases after one warmup. Spread, best, and worst are also emitted.
The deterministic harness exercises the production refresh predicate and real reconciliation/apply
path with a fake backend, including an initially misplaced new Ghostty window.

## Results

| Experiment | Median ms | Spread ms | Decision |
| --- | ---: | ---: | --- |
| Baseline: all refreshes wait for activation settling | 63.008 | 23.993 | Baseline |
| `AXWindowCreated` bypasses activation settling | 13.006 | 23.995 | Keep: 79.4% lower median |
| Final verification rerun | 13.008 | 23.996 | Stable |

## Notes

- The AX callback coalesces notifications into an atomic dirty bit.
- The foreground loop polls every 25 ms and retains dirtiness while the worker or a gesture is busy.
- Application activation settles for 50 ms before activation-only refreshes.
- A missed event is recovered by the one-second fallback refresh.
- Production snapshot and AX frame-write cost are not represented by the fake backend.

## Experiments

### Window creation bypasses activation settling

Hypothesis: a concrete `AXWindowCreated` notification is sufficient evidence to reconcile now; the
50 ms settling delay is needed for activation-only refreshes, not creation events.

The observer now retains a separate coalesced creation bit alongside general dirtiness. Creation
dispatches on the next foreground tick, while all other AX events, activation invalidation, busy
worker handling, gesture exclusion, and fallback behavior retain their previous gates.

Decision: keep. Median fell from 63.008 ms to 13.006 ms with equivalent spread.

## Path Inspection

- The AX observer run loop returns as soon as a source is handled; its 100 ms run-loop duration is
  not notification coalescing latency. The foreground tick remains the measured scheduling bound.
- Worker-busy and gesture guards run before either invalidation bit is consumed, so creation remains
  pending and coalesced until dispatch is safe. Existing action cancellation/latest-intent behavior
  is unchanged.
- Each cycle still performs the broad Core Graphics snapshot and per-application AX observation.
  The existing steady-state benchmark puts in-memory reconciliation below 0.1 ms, but the fake
  latency backend cannot quantify production snapshot and write cost.
- Failed observer registration or a creation event that precedes registration still relies on
  activation invalidation when the new app becomes foreground, then on the one-second fallback.
  Registration refresh remains once per second and idle scan frequency is unchanged.

## Dead Ends And Limits

- Lowering the 25 ms foreground tick could reduce the remaining 13 ms median, but it would increase
  continuous frontmost-application and mouse polling while idle. Discarded without implementation
  because it violates the event-driven idle-efficiency constraint.
- Waking the GPUI loop directly from the AX callback could remove the remaining tick phase, but that
  requires a new cross-thread wakeup architecture rather than another local tuning change.
- The fake backend verifies scheduling, reconciliation, workspace ownership, and the first correct
  applied frame. It excludes production Core Graphics snapshot and Accessibility write cost, OS AX
  delivery jitter, and the case where a just-created window is not yet snapshot-visible.
