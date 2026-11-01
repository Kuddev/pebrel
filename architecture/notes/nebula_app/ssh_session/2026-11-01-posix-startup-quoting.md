# Probe the SSH exec quoting contract before POSIX startup

## Status

Accepted for the fix to [#810](https://github.com/Kuddev/pebrel/issues/810).

## Context

Remote shell startup injects a quoted POSIX script for integration and resumable
sessions. Windows OpenSSH may find `sh` while its outer command interpreter passes
nested quoting differently. A successful SSH exec request only accepts the command;
the shell can subsequently exit with a syntax error.

## Evidence

The report shows an immediate disconnect and an unmatched-quote error in the
startup script. The old probe, `sh -c 'printf PEBREL_POSIX_1'`, exercises neither
embedded quotes nor expansion. A prepared integration or resume target also
previously bypassed this probe.

## Decision

[`resume::supported`](../../../../nebula_app/src/ssh_session/resume.rs) now runs a
probe through the same POSIX quoting helpers as startup. It verifies an exact
sentinel containing quotes, a dollar sign, a backslash and a space after variable
assignment and expansion. Probe support before preparing integration or executing
any POSIX startup. An unsupported host receives an ordinary SSH shell request.

## Rejected alternatives

- Detect Windows from an SSH banner: the banner does not identify the configured
  command interpreter or its quoting rules.
- Treat exec acceptance as successful startup: the reported syntax failure occurs
  after acceptance.
- Disable integration for every Windows host: compatible servers should retain it.

## Consequences

Supported hosts retain integration, cwd and resume startup. Unsupported hosts
remain usable with the server's ordinary shell, without POSIX-only integration.
The bounded probe runs once per shell opening, including when integration or a
resume target was available; there is no new polling or persistent state.

## Validation

The loopback regression accepts the old trivial probe and rejects nested startup,
matching the failure boundary. Restoring the old production logic makes the test
fail because POSIX startup still runs. With the fix, the InstaBox Linux product
test binary passes 98 SSH tests, with 2 existing tests ignored. A real `sh` also
round-trips the exact probe. This is not a claim of manual Windows SSH-server
acceptance.

## Supersedes

None.

## Revisit when

SSH exposes a negotiated shell/exec contract, or a supported server fails this
probe despite correctly handling the actual startup script.
