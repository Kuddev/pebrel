# Resident endpoint lifetime

## Status

Implemented; see the validation scope below.

## Context

[Issue #688](https://github.com/Kuddev/pebrel/issues/688) reports that a retained
terminal cannot control a restarted resident even though the current discovery
file is valid. A shell's inherited environment can outlive the server endpoint.
Private administrator instances must continue to address only their own server.

## Evidence

[`runtime_api/server.rs`](../../../../nebula_app/src/runtime_api/server.rs) previously exported the current port/token to every PTY.
`read_endpoint` preferred that inherited value over `runtime.port` indefinitely.
Removing the environment value restored ordinary discovery in the report.
The same explicit-address contract also protects private instances, as described
in [ADR-0010](../../../../docs/architecture-decisions.md#adr-0010--administrator-shells-hosted-by-the-gpui-product).

## Decision

Track whether the hosted endpoint publishes shared discovery. Shared instances
export the explicit `resident` discovery mode through the existing
`PEBREL_RUNTIME_ENDPOINT` variable; private instances retain their port/token.
The existing discovery entry point reads the file for every new resident-mode
request, before request serialization or transmission. In-process callers still
use their own hosted endpoint. CLI calls, subscriptions, bounded plugin requests
and ordinary launch handover share this entry point.

## Rejected alternatives

- Retrying arbitrary failed requests can repeat an input or mutation whose
  response was lost. Endpoint discovery does not replay transport operations.
- Falling back from every explicit port/token to the shared file can cross an
  administrator/private-instance boundary. Invalid and legacy explicit values
  retain their existing semantics.
- Updating a running shell's environment cannot update already-running child
  agents. A stable discovery mode survives inheritance without process injection.
- Omitting the variable alone can expose an outer instance's inherited endpoint
  when a resident is launched from another terminal. An explicit mode replaces it.

## Consequences

No new service, dependency, credential file or transport retry is introduced.
Resident CLI calls perform one discovery-file read; private calls stay pinned.
Existing WSL passthrough carries the mode without another environment variable.
A missing or malformed resident discovery record fails rather than choosing an
unverified endpoint. Runtime target identities and subscription lifetimes remain
separate from endpoint discovery.

Older CLI binaries cannot parse the new mode; PTYs already export their host's
CLI path. Legacy retained shells with raw endpoint values require one explicit
migration after confirming that they belong to the ordinary resident. Raw values
cannot safely be classified as private or shared after the original server exits.

## Validation

Regression tests in `runtime_api/server.rs` cover discovery rotation, missing and
malformed records, explicit private isolation, legacy values, and unset discovery.
The hosted-server test exercises both discovery scopes and real authenticated
loopback transport. The Windows runtime and agent-environment tests pass. The
built Windows CLI was also exercised with isolated loopback fixtures: a retained
environment follows replacement discovery, explicit pins keep their own target,
and a lost input response does not replay after the discovery file changes.
This does not claim a desktop restart, UAC prompt or WSL session acceptance.
No terminal rendering or request-submission behavior is changed.

## Supersedes

None. Refines ordinary discovery while retaining ADR-0010's private ownership.

## Revisit when

Private instances gain an explicitly authenticated reconnect/discovery contract,
or retained runtime target identities acquire a separately reviewed migration.
