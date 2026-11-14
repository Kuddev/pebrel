# WSL completion ownership and detached native hook delivery

## Status

Implemented locally; pending maintainer review.

## Context

WSL/SSH OSC hooks updated TerminalView, while only native hook dispatch called
the delegation completion handler. A worker could finish with a recognized hook
and still leave its delegation pending. The remote parser also received no local
pane identity. Claude Stop may carry a structured answer without a plain message.

An independent transport boundary remained for native Linux CLIs whose hooks run
without a controlling terminal. In the tested Claude 2.1.319 session, hook stdout
was consumed by the provider; returning JSON alone did not put OSC on the PTY.

## Evidence

- The original native delegation selection failed six of eleven cases, including
  WSL completion, queued delivery and Claude's structured final answer.
- After shared completion routing, real WSL Claude-to-Codex direct delivery,
  Claude-to-Codex queued delivery and Codex-to-Claude queued delivery completed
  without manually relaying a worker result. The queue cases observed a finished
  worker while its originating agent was still running.
- The detached-process regression invokes production send() with hook stdout
  captured separately. The original bridge returns no bytes to the private PTY.
- Codex's prior user-installed Linux fallback already verified a pinned ancestor,
  process birth, UID, installed executable inode and controlling terminal.

## Decision

Emit a boxed AiTurnDone view event only after lifecycle and ordering acceptance.
Use the receiving PTY's canonical pane id. Workspace defers shared completion
until view state is updated, publishes the runtime snapshot, consumes delegations
and attempts callbacks. Native and OSC ingress share that accepted completion;
ignored or replayed lifecycle events must not publish it. Prefer the structured
answer source over the optional message when creating the bounded worker result.

Retain the prior Codex fallback and extend the same pinned-owner checks to native
Claude. On Linux ENXIO, open an existing owner fd through its pinned proc directory
and revalidate ancestry, birth, UID, executable and PTY device before returning it.
Never reopen an unverified /dev/pts pathname. Explicit SSH routes, other errors,
other providers and partial writes retain their previous behavior. If a verified
native owner is unavailable, Claude's existing terminalSequence JSON fallback
remains available for clients that consume it.

## Rejected alternatives

- Polling agent.read: retrieves text but leaves automatic delegation broken.
- Completing every received hook before acceptance: replays and foreign processes
  could consume another task or publish an unowned result.
- Trusting an envelope's pane claim: the receiving authenticated PTY owns routing.
- Reopening the owner's terminal pathname: does not pin the already verified fd.
- Replacing user shell-environment policy: unrelated to callback transport; the
  reverse live test supplied its known isolated runtime identity explicitly.

## Consequences

No new dependency, timer, persistent format or automatic prompt retry. The native
Linux fallback recognizes the explicitly supported user installation anchors;
other layouts fail closed. Existing custom hook configuration remains preserved.

## Validation

Native GPUI tests cover real OSC ingress, both directions, busy-origin queues,
replays, foreign process rejection and native controls. Linux process/PTY tests
exercise delivery and owner-identity rejection. Actual test counts and remaining
unrelated failures are recorded with the repair deliverable.

## Supersedes

Extends the native Linux delivery portion of
2026-10-25-claude-detached-hook-delivery.md; keeps its JSON compatibility path.

## Revisit when

A provider offers an authenticated direct hook transport, native installation
anchors change, or upstream owns a verified replacement for these adapters.
