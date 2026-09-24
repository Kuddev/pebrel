# Check the platform budget before allocating native runners

## Status

Implemented for review.

## Context

The platform-condition budget ran only during packaging. A Windows-only WSL
regression introduced one additional test-level condition, leaving the source
count at 485 against a budget of 484. All ten PR checks passed because none ran
that budget checker. Preview preparation then failed before any package build.

## Evidence

- Preview run 35882296442 failed in `Check platform cfg budget` on main commit
  `1d26c2dc9860a6cfb61c14b630c12d36dbf95e43`.
- The added condition guarded the WSL truecolor regression, not a new production
  platform branch. Five related WSL forwarding tests repeated the same Windows
  compilation condition.
- Sharing that condition at the WSL test-module boundary preserves all five test
  bodies and lowers the repository count to 481 without moving source outside
  the scanner's declared roots or adding an exclusion.

## Decision

- Keep the WSL forwarding regressions together under one Windows test module in
  their existing owning source file. Other identity and PATH tests retain their
  existing boundaries. Production environment behavior is unchanged.
- Tighten the existing budget from 484 to the measured count of 481.
- Execute the same authoritative platform checker in required lint before matrix
  planning. Keep the packaging check as an independent release precondition.
- Run the checker's contract tests with the existing lightweight CI contracts.

## Rejected alternatives

Raising the budget, deleting the new regression, excluding test files from the
scanner, or ignoring the packaging failure would hide the mismatch. None is
needed. This change also does not turn the historical text-count heuristic into
a claim that architectural coupling has been proven absent.

## Consequences

The five WSL tests share one platform boundary and remain Windows-only. Budget
failures are visible before native runner allocation. Legitimate future scanner
policy changes still need their own reproducer, tests and review.

## Validation

The platform checker reports 481 against 481. Validation also compares the test
function inventory, runs the Windows identity/WSL regression group, and checks
that lint executes the gate before planning without automatic budget updates or
failure suppression.

## Supersedes

None.

## Revisit when

The text-based platform counter is replaced with a reviewed semantic contract,
or native runner planning is moved to another shared preflight owner.
