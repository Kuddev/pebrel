# Modified Enter compatibility at the terminal input boundary

## Status

Implemented on 2026-09-19. Native transport probes and input regressions support
the decision; they do not establish acceptance with every client version.

## Context

Claude clients without an enhanced keyboard protocol did not receive the expected
Shift+Enter newline. GPUI, legacy input and Runtime API could also choose different
encodings for the same terminal client.

## Evidence

The Windows ConPTY probe receives CR from Shift+VK_RETURN even when UnicodeChar
is LF, while native ReadKey retains Shift. Direct LF compatibility and negotiated
CSI-u survive that transport. The
[shared input implementation and regressions](../../../../nebula_app/src/input/terminal_input.rs)
and [GPUI keymap tests](../../../../nebula_app/src/gpui_shell/terminal/keymap.rs)
cover the byte/key boundary.

## Decision

The shared terminal input adapter owns the compatibility decision for all callers.
A negotiated keyboard protocol takes precedence. An unnegotiated Claude client
receives LF for Shift+Enter. Native Windows clients retain native key identity and
modifiers. Modified Enter never submits a shell-history entry.

## Rejected alternatives

- A separate special case in each UI would let the encodings drift again.
- Replacing every modified Enter with LF would discard native key identity and
  bypass an explicitly negotiated keyboard protocol.
- Assuming UnicodeChar alone determines ConPTY output contradicts the native
  input-record probe.

## Consequences

The fallback is scoped to the client and protocol state that need it. It does not
change persisted shortcuts or require a new input thread, dependency or terminal
protocol. UI adapters remain responsible for forwarding their actual key facts.

## Validation

Regressions verify unnegotiated newline bytes, negotiated protocol precedence,
native Windows modifiers and the command-history boundary. The GPUI tests exercise
the key-to-PTY route; the shared adapter is compiled for both shell configurations.

## Supersedes

None. This is the initial module-scoped record for the compatibility decision.

## Revisit when

Supported Claude clients negotiate the keyboard protocol consistently here. Remove
the narrow fallback when transport and client evidence make it unnecessary.
