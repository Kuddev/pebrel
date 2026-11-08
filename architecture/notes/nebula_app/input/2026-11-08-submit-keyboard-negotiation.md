# Deferred submit follows the current keyboard protocol

## Status

Implemented locally; pending maintainer review.

## Context

The non-Codex runtime input path waits for text echo before submitting Enter.
It previously encoded Enter while queuing the text. A cold CLI can negotiate
Kitty or return to legacy VT before the echo barrier opens, leaving that stored
sequence in the previous protocol. In a real WSL launch the initial Claude text
remained in its editor; a later deliberate Enter submitted it.

## Evidence

The view regressions queue a Claude prompt, change its keyboard protocol while
echoing the text, then flush the existing barrier. They cover both legacy-to-Kitty
and Kitty-to-legacy, plus a second wakeup that must not submit again.

## Decision

The shared barrier owns only its baseline screen. GPUI and legacy adapters keep
their existing echo condition and encode Enter with the shared input encoder at
flush time. Immediate empty submissions and Codex's ordered submission sequence
retain their existing behavior. Initial validation remains before text is written.

## Rejected alternatives

- Sleep and send another Enter: timing does not establish which protocol is active
  and automatic retries can submit a task twice.
- Always send CR: ignores negotiated Kitty and Windows input protocols.
- Store both old and new encodings: the terminal state is the current authority.

## Consequences

No new timer, prompt retry, readiness heuristic or persistent state. A failed
encoding leaves the barrier pending. The change fixes a protocol race; live cold
launch acceptance must separately verify that the CLI actually started its task.

## Validation

Run the view's runtime-submission and hidden-wakeup regressions, the GPUI product
build, and the legacy adapter compile check. Record live cold-launch results with
the repair deliverable instead of treating byte-level tests as model completion.

## Supersedes

None. Extends the non-Codex barrier described alongside
2026-09-21-runtime-codex-submission-boundary.md.

## Revisit when

The provider offers a stable input acknowledgement or changes keyboard negotiation.
