# Bound cached and running WSL guest probes together

## Status

Proposed during PR #409 review.

## Context

The guest-probe map retained every distribution/user pair for the process
lifetime. Each unseen target could also spawn another native worker.

## Evidence

The original `verified` path inserted entries without eviction or admission
limits. A source-extracted probe with synthetic completed workers retained all
40 requested targets; no WSL process or user configuration was accessed.

## Decision

Limit the sum of completed and running entries to 32. Evict the oldest successful
or expired-failure entry for a new target. Never evict a running owner or a failed
entry still in its five-minute cooldown. If no slot is eligible, return the
existing unknown verdict without spawning another worker. A later launch can try
again.

## Rejected alternatives

- Bounding only completed entries still permits unlimited simultaneous workers.
- Evicting running entries can duplicate probes of the same target.
- Evicting recent failures forgets the existing retry backoff under cache pressure.
- Blocking the caller until a slot is free would stall terminal creation.

## Consequences

The map and concurrent worker count are bounded together. Cache hits and the
existing failed-probe retry interval are preserved. Busy or evicted targets use
the existing non-invasive startup behavior until their verdict is available.

## Validation

Deterministic tests cover repeated completed targets, full running admission,
eligible eviction and preservation of failed-probe backoff. The source-extracted pre-fix probe fails
the 32-entry assertion. This does not claim native WSL startup timing.

## Supersedes

The unbounded process-lifetime cache in the proposed WSL zsh startup integration.

## Revisit when

Concurrent guest demand demonstrates that the fixed admission budget needs an
explicitly measured change or background queued admission.
