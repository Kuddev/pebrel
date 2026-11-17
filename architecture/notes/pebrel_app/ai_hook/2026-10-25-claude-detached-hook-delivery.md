# Deliver detached Claude hooks through its terminal sequence output

## Status

Implemented for the notification-delivery defect in issue #540.

## Context

Interactive Claude can start a hook without a controlling terminal. Opening
`/dev/tty` then fails with ENXIO and the fail-open bridge discards the event.
A sequence file advancing does not prove delivery to the owning pane.

## Evidence

The real detached-process regression starts the production bridge with a new
session and receives empty stdout on the previous implementation. Claude's
[official changelog](https://github.com/anthropics/claude-code/blob/main/CHANGELOG.md),
version 2.1.141, documents `terminalSequence` for hooks without a controlling
terminal. PR [#542](https://github.com/Kuddev/pebrel/pull/542) supplied the fallback
design and reported an interactive Claude/WSL observation.

## Decision

Keep authenticated OSC framing, process ownership, bounded payloads, ordering and
terminal writes in the shared remote bridge. Only Claude, only ENXIO from opening
the terminal, and only before any terminal bytes are written may return the same
OSC inside one `terminalSequence` JSON object. Other providers and other errors
retain their existing behavior. A partial terminal write is cancelled, never
duplicated through stdout.

## Rejected alternatives

- Consider sequence-file updates successful delivery: the file precedes output.
- Fall back after partial writes: this can duplicate or interleave an event.
- Return the field for every provider: their stdout contracts differ.
- Bundle automatic `continue`: that is the independent feature in issue #541;
  notification repair does not require injecting another command.

## Consequences

No dependency, setting, local credential, hook installation or retry policy
changes. Interactive clients supporting this output field can restore delivery.
This does not promise delivery in non-interactive CLI/SDK modes or old clients
that ignore the field, and it does not solve other bridge transport failures.

## Validation

The real detached-process regression covers permission/failure events, identity
envelopes and invalid-token rejection. Tests preserve provider/error isolation,
normal TTY single-delivery and partial-frame cancellation. Existing real PTY and
transaction tests remain. A physical WSL/Claude acceptance run is not claimed by
these Linux process tests.

## Supersedes

None; isolates the delivery portion proposed in #542 from automatic continuation.

## Revisit when

The provider offers an authenticated direct hook transport, or retires the
documented interactive terminal sequence contract.
