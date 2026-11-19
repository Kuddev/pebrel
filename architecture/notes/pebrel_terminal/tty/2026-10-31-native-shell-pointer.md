# Native shell pointer edits

## Status

Native Bash/Readline and zsh/ZLE adapter implemented; GUI routing is a separate adapter.

## Context

Terminal copy selections are not editable shell ranges. Synthesizing one arrow
per screen cell fails for wide and combining characters and bypasses the actual
line editor. The existing completion bridge already reads native buffers and
cursor positions through an owned query key.

## Evidence

Real PTY tests exercise `READLINE_LINE`/`READLINE_POINT` and `BUFFER`/`CURSOR`.
Changing either the text or the native cursor after a query must reject a later
operation, including between the range-start and deletion callbacks. A host-side
input epoch alone cannot detect every native editor widget.

Readline accepts numeric arguments only up to 1,000,000. Larger packed ranges can
reset the argument and reinterpret following digits. The protocol therefore uses
separate bounded endpoint operations, each at most 12291.
Numbered query arguments from 20000 through 999999 also preserve the native
buffer in real PTYs; the response echoes that nonce. A lost reply can therefore
be replaced without matching a late response to a different pointer gesture.

## Decision

Reuse the existing owned query key and native numeric argument. An ordinary key
without an argument preserves the completion query contract. Argument zero reads
the native pointer snapshot. Other arguments move the cursor, stage one endpoint,
or delete through the second endpoint. Only deletion mutates the native buffer,
once, so the native undo stack remains authoritative.

Every operation rechecks native text, cursor, shell token and binding ownership.
An executed command/new prompt invalidates the snapshot. The adapter advertises
only for the verified Emacs keymap with native digit-argument bindings; user
bindings and vi modes retain ownership. No stdin packet reader, file polling,
process-wide editing service or second editable draft is added.

The host must independently verify an active OSC 133 input boundary, match the
snapshot against actual terminal cells and decline ambiguous coordinates. It
must not invoke this adapter for application mouse reporting or alternate-screen
applications. The native guard does not make unverified screen mapping safe.

## Rejected alternatives

- Per-cell arrow counts do not represent Unicode editor positions.
- Reading a payload directly from stdin in a callback can consume user input.
- Replacing the line with a host mirror loses native edits and undo ownership.
- Advertising an untested PowerShell key encoding would confuse API availability
  with verified transport. PSReadLine remains outside this adapter's capability.

## Consequences

Operations use a bounded temporary native snapshot and the existing OSC user-var
transport. Shell-side binding inspection/base64 work happens only on prompt
advertisement and explicit pointer operations, not on ordinary typing.
Multi-line or visually ambiguous input may be rejected by the GUI mapping.

## Validation

`scripts.tests.test_shell_pointer` runs real Bash/zsh PTYs for native positioning,
Unicode range deletion, reverse ranges, one-step undo, stale text/cursor rejection,
replay rejection, bounds, user bindings and vi mode. The existing completion-query
PTY tests protect compatibility. These tests do not establish native desktop,
Windows ConPTY or macOS acceptance.

## Supersedes

None.

## Revisit when

A line editor exposes a stable direct control protocol, or a separately verified
adapter can support additional keymaps/platforms without guessing input state.
