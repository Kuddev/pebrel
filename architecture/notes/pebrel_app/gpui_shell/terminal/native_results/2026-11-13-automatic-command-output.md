# Automatic command output views

## Status

Implemented for the native desktop terminal. The existing presentation remains
the default.

## Context

Issue #900 reports that automatic command views remove the original terminal
colors and introduce a visual change when a command completes. A result's Raw
tool displays retained text; it cannot recover the original cell colors.

## Evidence

Automatic results retain verified VT source rows. Manually requested results
arrive through the inline-content protocol and have no corresponding VT text.
Both use the same viewport projection, which can reveal the original rows
without rerunning the shell command or copying its text back into the grid.

## Decision

Store the preference with the existing runtime settings. Publish it to terminal
sessions through one app-owned atomic flag, observed from the same in-memory
settings snapshot as other desktop controls. The VT adapter checks it before
classifying a command and before capturing completed output; it does not read
settings files or acquire a settings lock on the I/O thread. The projection
omits already captured automatic results while the setting is off. The original
grid and manually requested results remain available.

## Rejected alternatives

- Change the Raw tool: retained plain text does not have the grid's color or
  cursor state.
- Disable the entire inline-content protocol: explicit results have no VT
  source to reveal.
- Apply the preference at session creation only: an open terminal would need
  to restart to reveal its original output.

## Consequences

The setting applies to open and new terminal panes. Re-enabling it can show
retained automatic results again; commands that were not captured remain as
original terminal rows. The preference does not change shell execution.

## Validation

The VT regression feeds ANSI-colored command output and toggles the preference
before or during capture. The desktop regression toggles the setting in a cached
terminal view and checks its projection alongside an explicit result. Settings
tests cover search, saving and restoring the default.

## Supersedes

None.

## Revisit when

Automatic result rendering no longer replaces verified source rows.
