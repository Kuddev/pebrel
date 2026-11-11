# Deliver detached Claude SessionEnd to its verified Linux terminal

## Status

Proposed for review with implementation.

## Context

Issue #434 reports a WSL zsh pane retaining its Claude identity after a double
Ctrl+C exit. Closing and reopening Pebrel then resumes an already exited CLI.
The shared pane lifecycle clears the saved identity on SessionEnd or OSC 133;D,
but either signal must first reach the pane.

## Evidence

A real zsh PTY running Claude Code 2.1.310 invoked both SessionStart and SessionEnd
hooks. Claude detached each hook from its controlling terminal. The bridge
returned authenticated OSC frames in terminalSequence JSON in both cases.
Claude relayed SessionStart to the terminal, but discarded SessionEnd during
both double Ctrl+C and /exit.

The shared zsh integration still emitted OSC 133;D. Starting the same zsh without
that integration demonstrated the missing second exit signal. These observations
come from Linux ARM64; the reported Windows 11 WSL desktop path remains to be
verified.

## Decision

For a Claude SessionEnd whose hook has no controlling terminal, recover the
verified ancestor CLI's Linux devpts terminal from its stdin descriptor. Require
the process epoch, foreground process group, controlling device, descriptor
identity and device ownership to match before writing. Open without following
symlinks or acquiring a controlling terminal.

Reuse the existing bounded OSC write and pane token. When verification fails,
retain the terminalSequence response. Existing controlling-terminal and tmux
routes keep their delivery rules. Other hook events keep their existing path.
The application continues to use its shared SessionEnd lifecycle and session-save
logic.

## Rejected alternatives

- Clear state on Ctrl+C: interruption can stop one turn while leaving Claude open.
- Rely only on a matching shell prompt: guest shell integration and prompt
  recognition can be unavailable.
- Scan every terminal or use a PID supplied in hook JSON: neither establishes the
  owning terminal.
- Return more SessionEnd JSON: the real CLI does not relay this response on exit.
- Add a background relay: the current short-lived hook can deliver the event.

## Consequences

The additional procfs reads and device open occur only on detached Claude exit.
There is no new persisted route, token, worker or setting. Linux devpts is the
supported recovery path; other hosts retain their existing transport.

## Validation

- The real-PTY regression fails before the change and passes after it. A detached
  hook reaches its foreground Claude ancestor, while redirected stdin and a
  background process group retain the JSON fallback.
- The remote-hook Python suite covers authenticated delivery, partial writes,
  detached JSON responses, native hooks and configuration preservation.
- Real Claude Code 2.1.310 delivers SessionStart and SessionEnd with the same
  process identity after the change: double Ctrl+C with and without zsh
  integration, and /exit with integration.
- The GPUI regression checks that a WSL SessionEnd without shell markers removes
  the foreground and saved recovery identities before window close.

## Supersedes

None. Extends the [WSL hook transport](../platform/wsl_hooks/2026-09-20-wsl-hook-transport.md).

## Revisit when

Claude relays terminalSequence during SessionEnd, or supplies an authenticated
terminal endpoint for detached hooks.
