# Native notification callback generations

## Status

Implemented for integration of PR #289. Bundled App acceptance passed on macOS 26
Apple Silicon and Intel for notification commit `6b253485` on 2026-11-17.

## Context

The macOS adapter uses a native request tag to find its application callback.
Delivered notifications can remain after the originating process exits. A
counter that restarts at one reuses those request identities in a new process.

## Evidence

The proposed adapter generated `pebrel-notification-1` after every start. The
locked GPUI service uses that tag as Apple's request identifier and returns it
with native response events. The callback lookup therefore needs a process
generation in addition to its sequence number.

The [production registry](../../../../../pebrel_app/src/platform/notifications/registry.rs)
has a stale-generation regression. A native Rust probe of that module passes
all three cases; replacing only the tag format with the previous counter-only
form makes the stale-generation case fail. The probe uses callback model types
and verifies registry behavior; native App delivery is a separate acceptance path.

## Decision

Create a fresh 128-bit random generation when the macOS adapter initializes.
Tags combine that generation with a checked monotonic counter. If identity
initialization fails, the native adapter remains unavailable and logs the error.
Counter exhaustion refuses another registration instead of reusing an identity.

One registry owns the 64 retained callback sets. A response takes its callback
once; an unknown, evicted or previous-generation tag has no target. Invalid action
ids never fall back to the default action. Eviction returns the native tag to
dismiss after the registry borrow is released. Activation also runs outside the
borrow so application/native delivery can re-enter safely.

## Rejected alternatives

- A process-local counter alone collides after restart.
- PID alone can be reused and does not name a process lifetime.
- Routing an unknown tag to the newest callback loses the original action owner.
- Calling native dismissal or activation while retaining the registry borrow
  makes re-entry depend on platform callback timing.

## Consequences

GPUI remains the owner of authorization, presentation and native delegates.
The application owns callback identity and bounded retention. No dependency or
persisted format is added. Old process notifications cannot invoke callbacks in
the new process; current requests still use their existing activation/choice path.

## Validation

Registry cases cover default and valid choices, malformed choices, consumption,
previous-generation responses, eviction and sequence exhaustion. Native acceptance
used the bundled App and the normal permission flow in
[run 43998356666](https://github.com/Kuddev/pebrel/actions/runs/43998356666). Both Mac
architectures granted the real system permission, displayed the foreground
notification in Notification Center, and returned from a different tab to the
originating pane after a native click. Package identities and screenshots were
retained with the run. The hosted image needed its standard Notification Center
agent started; application authorization used the normal native Allow action.

## Supersedes

None. Extends the [GPUI ownership decision](2026-10-01-gpui-macos-notification-ownership.md).

## Revisit when

Notification actions acquire a durable owner across process restarts or GPUI
provides an application callback registry with equivalent generation semantics.
