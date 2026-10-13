# Native Agent configuration and transport

## Status

Implemented; desktop integration remains subject to native runtime acceptance.

## Context

Agent configuration policy lived under `ai_hook/win`, while Unix entry points
returned without installing hooks. Sharing the UI alone could not enable Agents
on macOS or Linux. Unix packages also omitted the helper used by those hooks.

## Evidence

- `nebula_app/src/ai_hook/local.rs` owns the common installation and removal rules.
- `nebula_app/src/ai_hook/integrations.rs` detects installed providers.
- `nebula_app/src/ai_hook/unix.rs` and `windows.rs` own native transports.
- `nebula_hook/tests/lifetime.rs` exercises the Unix socket acknowledgement.

## Decision

Keep one set of provider installers and ownership checks under `ai_hook/local`.
Use native quoting only where the provider invokes a shell. Do not convert Unix
backslashes into path separators. Require an executable helper before installation.

The Unix server owns a private short socket directory and a joinable worker.
Only same-user kernel peer identities are accepted; shared parsing and Agent
ancestry rules remain authoritative. Payload size, read time and queued events
are bounded. The helper stays alive until acknowledgement or its existing watchdog.
Each PTY receives the endpoint explicitly, avoiding process-wide environment writes
after worker threads start. Shutdown stops the listener and removes its directory.

The configuration guard now has an owner outside the application event loop.
Dropping it cancels waits and joins its worker. Notifications coalesce rather than
queueing arbitrary filesystem event batches. Process probes share bounded output
and wait handling under `platform/process_output`.

## Rejected alternatives

- Copying all installers for Unix would let authorization and removal rules drift.
- Enabling capability flags while retaining empty Unix entry points would advertise
  integrations that cannot emit events.
- A detached permanent watcher would leave no explicit shutdown owner.
- Global environment mutation would race running threads and nested instances.

## Consequences

Local provider schemas and ownership checks are shared across platforms. Only
transport and command invocation differ. Queue saturation drops events with a log;
it is not an authoritative event journal. Existing terminal lifecycle observations
remain necessary. A missing helper prevents installation without overwriting foreign
hooks. macOS session lookup uses open descriptors from live pane descendants rather
than assuming that `/proc` exists or selecting a session by working directory.

## Validation

Existing installer, quoting, ownership and lifecycle tests remain in their owning
modules. Unix socket and helper tests exercise real local connections. Native
compilation checks cover both Windows and Linux; macOS checks use the repository's
native CI matrix. None of these checks substitutes for desktop visual acceptance.

## Supersedes

None.

## Revisit when

A provider changes its hook invocation contract, or native process identity APIs
replace the bounded process-table probe.
