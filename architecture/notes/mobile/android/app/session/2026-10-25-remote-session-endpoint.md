# Preserve the actual multiplexer endpoint across SSH recovery

## Status

Accepted for implementation.

## Context

A shell-mode SSH tab can enter multiplexer manually. Background network recovery must
restore that tab's session, window, and pane. If the saved target has disappeared,
the new PTY should open the login shell without creating a replacement session.

## Evidence

`TmuxSessionControl` previously queried the PATH-selected default executable, which only addresses the
default socket. A manually launched client using a named socket and another multiplexer
version was invisible. Successful empty client lists could also clear the last
checkpoint while a network failure was removing the PTY.

An isolated OpenSSH fixture with two PTYs and multiplexer 3.8 on a named socket reproduces
the missing target with the old discovery path. The fixed controller identifies
both clients and restores the selected window/pane after replacing one transport.

## Decision

Carry executable/socket metadata with the typed remote session. Discover candidate
endpoints from this user's multiplexer process metadata and listening sockets. Linux uses
`/proc` fd inodes; macOS uses `lsof`. No command arguments, environment values, or
pane contents are collected. Probe bounded candidates and accept only one matching
the independently identified SSH PTY; never choose the first session/server.

Checkpoint, scroll, cancel-copy-mode, and attach use the same endpoint. Session
creation time and server PID continue to guard against ID reuse. A missing client
does not clear the checkpoint unless process discovery still identifies this SSH
PTY and confirms there is no multiplexer client on it. Late callbacks cannot update a
non-ready transport generation. Browsing inventory is not attachment evidence.

Attachment guards and window/pane selection fall back to the login shell when the
saved target no longer exists. Configured multiplexer retries use exact session names and
never `new-session`. A new user-created connection retains existing creation policy.
The attach command runs inside the recovery wrapper so a target disappearing after
validation, another attach failure, or intentional detach can still enter the shell.

## Rejected alternatives

- A socket name alone: a different PATH-selected binary may not speak the server's
  protocol, and `-S` sockets need not follow default naming/location rules.
- Parsing the user's command line: aliases, scripts, and wrappers hide the actual
  endpoint; command arguments can contain unrelated sensitive input.
- Treating an empty client list as a detach: transport failure can produce the
  same observation. Preserve the last confirmed target when ownership is unknown.
- Attaching by session name after identity mismatch: names and IDs can be reused
  by another task. The requested fallback is a normal shell.

## Consequences

The common active-client path uses one metadata query, and native wheel gestures
still issue none. Endpoint discovery is needed only when the cached endpoint no
longer finds this PTY. Checkpoints remain memory-only and belong to the live tab;
this change does not restore tabs after Android kills the whole application.
Unsupported/restricted process introspection preserves ordinary shell behavior.

## Validation

Unit tests cover named endpoints, quoting, ambiguous ownership, disappearing PTYs,
transport replacement, and stale callbacks. Real isolated multiplexer tests cover missing
sessions/panes and reused identity falling back without session creation. A real
failed-attach case also verifies fallback after successful target validation. A real
OpenSSH + multiplexer 3.8 fixture checks two independent clients, copy mode, intentional
detach, window changes, and saved-target restoration. macOS remote discovery and
physical Android background behavior still require native acceptance.

## Supersedes

None; extends the existing SSH resume and inline recovery decisions.

## Revisit when

Remote servers expose a portable authenticated channel-to-PTY identity API, or
application process-death restoration becomes part of session persistence.
