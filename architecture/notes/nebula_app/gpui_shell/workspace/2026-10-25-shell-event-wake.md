# Wake the process shell-event pump when work arrives

## Status

Reviewed for PR #760.

## Context

Runtime commands, tray actions and connection prompts share the process shell-event
channel. The GPUI consumer previously drained it only after a 120 ms timer. Serial
request/response clients could pay that delay on every request, including remote
terminal input queued behind screen reads.

## Evidence

`start_shell_event_pump` placed its timer before `try_recv`. The contributor's
Windows release-build comparison in PR #760 reports text-only requests changing
from about 123 ms to 2 ms and screen requests from about 136 ms to 20 ms after
applying #759 first. These are measurements on that machine, not universal latency
or throughput guarantees and not a maintainer Android-device measurement.

## Decision

Preserve the std channel used by existing event producers. One named blocking
thread transfers events in order into the existing futures channel implementation.
The GPUI task waits for either an event or the periodic tick, then drains the queued
batch and calls the existing process-level window dispatcher.

The initial workspace transfers the single receiver to this pump. Its lifetime is
the application, not that first terminal pane or window: subsequent windows still
need runtime and tray dispatch after the first window closes. The receiver task is
owned by GPUI's application executor. The forwarding thread exits on producer EOF
or failed forwarding; it never joins or blocks on the UI thread. Application exit
ends the process-wide source services and pump.

After producer EOF, the GPUI task uses only the timer so a closed stream cannot
spin. Empty ticks retain periodic runtime snapshot publication. Events under load
can publish more snapshots per second; there is no new background polling loop.

## Rejected alternatives

- Shortening the periodic timer increases idle wakeups while still imposing a
  fixed request delay.
- Blocking on std `recv` in the GPUI task freezes the UI.
- Replacing every producer's channel interface is unnecessary for this adapter
  fix and would spread the lifecycle change across unrelated features.

## Consequences

One process-level thread is added; no dependency, wire format or setting changes.
Both channel implementations were already unbounded. Dispatch cost still depends
on queued requests and snapshot size. Performance evidence must separate increased
request throughput from CPU cost per request.

## Validation

The contributor reports 237 workspace/runtime tests and a Windows release build
passing. Code review checks event ordering, producer EOF, application ownership and
continued timer dispatch. Deterministic GPUI scheduling does not establish native
thread wake latency. Required CI and the combined native validation remain separate
from those reported measurements.

## Supersedes

None.

## Revisit when

All process event producers can share an async sender directly, or measured event
load requires a separate bounded admission/coalescing policy.
