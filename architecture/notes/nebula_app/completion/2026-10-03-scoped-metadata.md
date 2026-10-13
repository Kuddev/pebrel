# Completion metadata follows a captured execution scope

## Status

Implemented. Native and backend acceptance evidence accompanies the review.

## Context

Local branch and package-script sources could not inspect a WSL or SSH project.
Using host files for that input would suggest names from another machine. Opening
a new SSH connection for each prefix would also introduce authentication and
profile resolution into an otherwise read-only completion request.

## Evidence

[`metadata.rs`](../../../../nebula_app/src/completion/metadata.rs) captures process
or authenticated SSH execution. WSL keeps the selected distribution and captured
user. SSH connection generations participate in the cache key.
[`completion.rs`](../../../../nebula_app/src/ssh_session/completion.rs) rejects
absent or ambiguous authenticated routes and retains the selected connection
through all reads in a request. Its tests exercise cancellation while that shared
connection remains usable.

## Decision

The application worker prepares one execution scope before consulting source
caches. Source admission remains in the application completion entry; views own
input, cancellation and result application. SSH preparation captures an existing
authenticated connection, without looking up profiles or starting authentication.
Independent exec channels own their output, timeout and close lifecycle.

Each pane retains the existing two-second snapshots. Keys include cwd, explicit
directory selection, source role and execution identity. An SSH reconnect receives
a new generation even when the destination spelling is unchanged. Invalidation
generations prevent older work from repopulating a cleared cache. Failed reads
are unavailable results, not successful empty snapshots.

Git reads configured fetch mappings and refs, without remote discovery. Prompt,
optional-lock, lazy-fetch and allowed-protocol controls are applied inside the
guest as well as on host processes. Configuration discovery excludes remote URLs
and credentials. Checkout guess/path conflicts are checked on the same machine.

Guest script discovery runs the owned Python program with `-I -S`. It reads
bounded declarative metadata and returns script names, not bodies. It does not
invoke a package manager, hook, install command or project script. Root and catalog
reads share a 4 MiB input budget and a request deadline. Cancellation closes the
owned query, without closing the interactive SSH connection.

An unavailable foreign source retains only its environment-scoped history.
Available empty metadata cannot revive deleted names through that fallback.
Host filesystem discovery is never a replacement for unavailable guest metadata.

## Rejected alternatives

- Host filesystem fallback: equal path spelling does not identify the same data.
- New SSH authentication per request: changes connection ownership and can prompt
  during typing.
- Commands in the interactive PTY: would alter its buffer, output and program state.
- Caching failures as empty data: hides reconnection and source recovery.
- Looking up a connection separately for each read: can combine different sessions
  into one snapshot after a reconnect.

## Consequences

No new executor, dependency or persisted format is introduced. Local projects use
the existing Rust manifest reader. Guest project discovery requires Python 3 and
an absolute Unix cwd. Typed nested connections without an owned metadata route
retain scoped history and native completion. This does not evaluate shell
functions, runtime expansions or arbitrary remote login-shell semantics.

## Validation

The application tests exercise scoped history, successful-empty behavior,
invalidation and host isolation. The authenticated loopback transport fixture
checks missing/ambiguous routes, captured connection generations and channel
cancellation. Ignored real SSH and WSL tests use the production request path and
verify actual Git/script effects after applying accepted candidates.

## Supersedes

Extends `2026-09-30-request-boundary.md`. Supersedes the local-only project/ref
admission described in `2026-09-30-semantic-arguments-and-scripts.md`; its shared
grammar and presentation boundaries remain applicable.

## Revisit when

Another transport has an authenticated metadata boundary, guest Python becomes
unavailable often enough to justify another owned declarative reader, or a pane
needs an explicit connection handle instead of destination-based admission.
