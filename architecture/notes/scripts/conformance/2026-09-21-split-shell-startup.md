# Wait for split-shell startup before column probes

## Status

Proposed for review with the conformance regression fix.

## Context

The resize case opens a sibling pane and queries both shells for their native
column counts. Creating the pane returns before the new shell has initialized.
The existing boot case already waits for initial shell output before submitting
commands, but the resize fixture did not apply that precondition to its sibling.

## Evidence

[Windows job 106259683240](https://github.com/Kuddev/pebrel/actions/runs/35576551914/job/106259683240)
recorded the source pane's initial width as 38. The sibling produced a late
PowerShell prompt and only the leading `W` of its column probe. There was no
completed sibling measurement, so the case never reached `pane.resize`.

The focused regression simulates a sibling whose first output arrives after two
empty observations. Before the fix, its column command is submitted during
startup. A second regression requires a permanently silent sibling to fail and
be closed without performing the resize.

## Decision

Apply the existing boot fixture's bounded initial-output check before the new
sibling's first column command. Preserve the source pane's immediate initial
measurement and the existing startup timeout. This wait belongs to shell startup,
before the requested ratio change.

Both post-resize commands still run without additional polling for geometry.
Their output must satisfy the original growth, shrinkage and total-column bounds.
Initial output alone never counts as a successful column measurement.

## Rejected alternatives

- Retry the full resize case until it passes: that can hide a geometry defect.
- Delay or retry column measurements after changing the ratio: that weakens the
  immediate resize contract rather than addressing the uninitialized fixture.
- Increase measurement timeouts or accept missing probe output: neither makes an
  input command submitted during startup execute reliably.

## Consequences

Only the test fixture gains an initialization precondition. Product input rules,
the runtime API, geometry assertions and report fields retain their contracts.
The initial-output check follows the harness's managed shell setup; it is not a
general claim that every interactive profile is ready after its first output.

## Validation

Regression coverage includes delayed and silent startup, growth and shrinkage,
stale widths, total-column bounds, failure diagnostics and sibling cleanup. The
fixture rejects startup polling after the resize, preserving the timing boundary.
The real Windows conformance job must pass before the candidate is accepted.

## Supersedes

None.

## Revisit when

The runtime exposes an explicit shell-input readiness signal that the conformance
fixture can use in place of its existing initial-output precondition.
