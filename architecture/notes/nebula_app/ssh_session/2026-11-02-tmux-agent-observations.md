# SSH multiplexer Agent observations and Hook delivery

## Status

Implemented for the desktop candidate on 2026-11-02. Native UI acceptance is
separate from the real Linux PTY/multiplexer delivery exercise described below.

Partially superseded by [2026-11-05-unobserved-multiplexer-evidence.md](2026-11-05-unobserved-multiplexer-evidence.md)
for panes without a current observation.

## Context

The remote Hook bridge wrote OSC 777 to `/dev/tty`. Inside a multiplexer this is
the pane's inner PTY, so the server can filter the message before Pebrel receives
it. Persistent servers also retain missing or obsolete channel tokens after SSH
reconnects. The outer terminal's last screen rows may belong to an inactive split
pane, and its title need not contain the current Agent's native task title.

## Evidence

An isolated server running version 3.4 of the supported multiplexer, with
passthrough disabled, filtered an unwrapped OSC.
The product helper delivered a constructed Codex Hook to two real outer PTYs
with separate tokens, and rejected an inactive pane and a client that switched
sessions before the next observation. Reattachment refreshed the token without
restarting the synthetic Agent process. This is transport evidence, not proof of
a particular installed Codex version's Hook configuration or native UI behavior.

The existing [recovery monitor](../../../../nebula_app/src/ssh_session/resume.rs)
already resolves the client owning the SSH channel's nonce-scoped PTY. The
[shared lifecycle](../../../../nebula_app/src/ai_hook/lifecycle.rs) already owns
Hook arbitration and structurally constrained screen observations.

## Decision

Reuse the pane-owned recovery monitor and authenticated short-lived exec
channels. The installed Python helper reads the active pane's foreground job,
native title and at most 24 visible screen rows. Observations live in memory,
expire after eight seconds, and are consumed once per sample. They do not claim
a Hook session, submission, result or completion notification.

The helper registers ten-second routes in its private route directory.
Only connection tokens and routing metadata are written; no screen, payload or
transcript is persisted. Routes bind the outer PTY and process epoch, server
socket and epoch, session/window/pane, and foreground process epoch/arguments.
The directory is 0700, files are 0600, symbolic links are rejected, and route
count, subprocess output and execution time are bounded.

Before delivery, the bridge checks the current client/pane, process ancestry and
foreground group again. It writes to the verified outer PTY with that route's
current token, retaining the existing per-channel serialization and OSC parser.
No inherited token is used as a fallback. The authenticated envelope also carries
a context digest, so stale queued events cannot acquire another context's Hook
ownership or ordering state.

The view clears prior task/confirmation/answer ownership when the context changes,
then uses the existing Agent identity and lifecycle rules. Dynamic titles are
outside the context digest. A process observation alone cannot announce success.

## Rejected alternatives

- Enabling global passthrough or rewriting server hooks/options: affects other
  clients, and does not refresh an already running process's inherited token.
- Inferring the active Agent from the entire outer terminal: split panes can
  expose another Agent's footer and create incorrect task ownership.
- A second remote daemon or Agent state machine: existing channel ownership and
  lifecycle arbitration already provide the required cancellation and semantics.
- Replaying captured terminal escape sequences: observations remain bounded
  text; remote snapshots must not execute terminal commands or clipboard writes.

## Consequences

The installed Python integration and a positively resolved multiplexer client are
required. Missing capabilities/timeouts leave the shell usable. Routes expire
after connection loss, and process/TTY validation rejects recycled identities.
Events during initial discovery or a context transition may be dropped; current
process/title/screen observations identify an already running Agent afterward.
Nested multiplexer servers and non-POSIX hosts are not covered by this delivery path.

There are additional bounded exec/multiplexer queries at the existing two-second
poll interval. No work runs on the UI/render thread, no persistent control client
is added, and cancellation follows the SSH pane's existing channel lifetime.

## Validation

- Real isolated Linux PTYs and the supported server at version 3.4: OSC filtering, two matching clients,
  distinct-session isolation, inactive-pane rejection and reconnect token refresh.
- Foreground replacement and expired routes rejected; a fresh observation
  establishes a new lifecycle context. Route permissions and unchanged server
  passthrough setting checked directly.
- Rust formatting, architecture and diff checks performed for this change.
- No regression suite or full native Codex/UI acceptance was run.

## Supersedes

None. Extends the existing pane recovery and authenticated remote Hook boundaries.

## Revisit when

A supported multiplexer control transport can replace polling while retaining exact
client ownership, bounded output, cancellation and reconnect semantics, or the
remote integration can supply equivalent verified observations without Python.
