# Publish the initial desktop session to Runtime subscribers

## Status

Implemented on the candidate branch.

## Context

[#960](https://github.com/Kuddev/pebrel/issues/960) reports that the phone pairs
or reconnects after a desktop restart only after someone runs
`pebrel ctl snapshot`. The desktop listener starts before GPUI registers
its first workspace. A quiet startup previously relied on a later shell event
or an explicit Runtime read to populate the session hub.

## Evidence

A [Windows cold subscription comparison](https://github.com/Kuddev/pebrel/actions/runs/43155386039)
used the published 2.4.0 and 2.5.3 x64 packages. The first operation after
discovery was a subscription. All three acknowledgements reported revision zero.
2.4.0 sent a snapshot within 430 ms. Both 2.5.3 runs (fresh configuration and
the configuration reused from 2.4.0) sent no session during the six-second
observation. An explicit snapshot immediately delivered the session to the
same subscriber. Reading a snapshot as a readiness check hides this behavior.

## Decision

[First-window startup](../../../../../../pebrel_app/src/gpui_shell/workspace/windowing/startup.rs)
publishes the current Runtime session once after registering the initial
workspaces. This happens after the batch when restoring windows from an update.
Subscribers that connected during boot receive the publication; later
subscribers can receive the stored revision.

## Rejected alternatives

- Having Android issue an extra snapshot request would leave other cold
  subscribers dependent on incidental desktop events.
- Publishing before the windows are registered would send an empty session.
  The window registry already owns the projection of complete workspaces.

## Consequences

The initial publication no longer depends on terminal input, a background
notification or a CLI request. The existing snapshot format and live change
publication stay the same.

## Validation

The startup regression subscribes before opening the first workspace, then
asserts that the published session contains the actual startup pane. It never
requests a snapshot. The Windows comparison above establishes the behavior of
the two published packages.

## Supersedes

None.

## Revisit when

The desktop session projection becomes available before workspace registration,
or workspace registration itself owns publication for a restored batch.
