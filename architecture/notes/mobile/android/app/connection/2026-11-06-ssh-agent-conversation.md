# Conversation view for Agents in a direct SSH terminal

## Status

Accepted by the maintainer on 2026-11-06 for the mobile preview.

## Context

The chat view only worked for panes of a paired computer, whose runtime reads the
native transcript. A phone that connects to a server over SSH had only the terminal,
although the same Agents (Claude Code, Codex, Pi, Antigravity) run there.

## Evidence

On an Android 14 emulator, an SSH tab running a stand-in Agent that writes Claude's
JSONL format showed the conversation switch within one detection round. A sent
message, the tool call, the selection menu and the chosen answer appeared in the
chat view. Projections were also checked against real Claude Code and Codex
transcripts and against Pi and Antigravity records built from their published
formats. Real Pi and Antigravity CLIs were not run.

## Decision

A bundled Python reader runs on the server through a separate exec channel, never
through the interactive shell. It finds the Agent in the foreground process group of
this connection's PTY, or of the multiplexer pane the app reports, locates its
transcript and returns one bounded page in the existing conversation message shape.
The reader is installed once under `~/.cache/pebrel`, named by its hash, because one
exec command is limited to 8 KiB. Output stays under the 64 KiB query budget.

Input goes to the terminal: a message is pasted and submitted with Enter, and a
numbered menu drawn on screen becomes a prompt card answered with arrow keys. The
menu is read from the terminal engine, so it is current while the terminal is hidden.

## Rejected alternatives

- Parse the screen into messages: wrapped and redrawn output cannot be told apart
  from content.
- Require the desktop app on the server: direct SSH is the case being served.
- Send the reader with every poll: it exceeds the exec command size.

## Consequences

The server needs `python3`; without it the view explains why. Agents whose
transcript cannot be found show an empty conversation, and the terminal still
works. Transcript formats are not public contracts and may change with releases.

## Validation

`TerminalPromptTest`, `RemoteOsIconTest` and the debug unit suite pass; the emulator
flow above was run with screenshots.

## Supersedes

None.

## Revisit when

An Agent publishes a stable session API, or a transcript format change breaks
the projection.
