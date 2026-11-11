# Count synchronization in parser input bytes

## Status

Proposed; verified through a real Unix PTY and Windows product compilation.

## Context

VTE's synchronized byte count includes every input byte, including the ordered
private image marker. `pty_read` compared it with transport bytes alone. An
already committed prefix followed by an open image batch could consequently
lose its immediate Wakeup. The earlier newline injection had the same unit
mismatch, observable with multiple images.

## Evidence

A real PTY reproduced missing Wakeup for `A + BSU + image` and
`BSU + A + ESU + BSU + image`, although A was already in the grid. The image
remained buffered as intended; the problem was the notification predicate.

## Decision

`StreamProcessor::feed` returns the byte count actually passed to VTE. Each
`pty_read` accumulates that count and compares it with VTE's buffered tail.
Transport recording, read limits and the returned read count continue using
wire bytes. VTE remains the authority for commit, timeout and buffer lifetime.

## Rejected alternatives

- Subtract a fixed marker length: accepted markers and chunking vary.
- Maintain a second synchronized-marker counter: it duplicates VTE lifetime.
- Flush on image arrival or always notify: this breaks atomic synchronized
  updates or restores unnecessary redraws during an open batch.

## Consequences

The change adds a local count, with no persistent field or allocation. Ordinary
input returns its original byte length. Existing callers that do not decide PTY
Wakeup can ignore the returned count.

## Validation

Real PTY tests verify committed prefixes, completed frames before a new open
batch, multiple markers, every image split point, and separate reads through
ESU. Existing image tests retain timeout replay and disconnect cleanup coverage.
Timing comparisons use identical release harnesses and locks, with raw samples
and platform/build identities recorded alongside the local results. These
measurements are not a general throughput guarantee.

## Supersedes

None; refines synchronization accounting for ordered image replay.

## Revisit when

VTE exposes committed-input notifications or ordered extension OSC callbacks.
