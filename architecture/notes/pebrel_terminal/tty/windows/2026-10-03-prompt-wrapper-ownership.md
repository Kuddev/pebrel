# PowerShell prompt wrapper ownership

## Status
Proposed for the confirmed venv deactivation regression; references Issue #278.

## Context
Python venv saves the current prompt, wraps it while active, and restores it on
deactivation. Pebrel also wraps replaced prompts before the next ReadLine call.

## Evidence
The Windows fork probe on production main 9dd3d649 preserved Conda and active
venv prefixes but failed after real Python 3.13 venv deactivation with a stale
empty environment prefix. Run: https://github.com/WilliamWang1721/pebrel/actions/runs/37104065702.
The saved Pebrel function still read the latest mutable global previous-prompt
reference, which had been replaced with the venv hook whose backup was now gone.

## Decision
Use PowerShell GetNewClosure when installing a wrapper. Its previous prompt,
visual owner flag and recursion guard live in that closure's script scope.
Restoring an old wrapper therefore restores its original owner. Re-sourcing
reads that installed closure's owner rather than stale installation metadata.
Existing global fields retain compatibility metadata; execution does not use
them to choose the previous prompt. Shared rendering depth lets only the outer
wrapper publish OSC and the completed-command snapshot; inner wrappers contribute
visual output and owner callbacks. Release depth before restoring caller status.

## Rejected alternatives
Inferring environment names in the host does not repair prompt lifetime. A
venv-specific deactivate override would interfere with Python's own cleanup.
A shared global guard would suppress an older wrapper's real user prompt.

## Consequences
Conda and venv retain their own prefix generation and disable preferences.
No environment reporting protocol, persistent format or dependency is added.

## Validation
The same real venv fixture covers default/custom prompts, activate/deactivate,
repeat bootstrap, disabled customization and native failure status. Existing
PowerShell tests and the Conda hook controls run through fork Actions only.

## Supersedes
None.

## Revisit when
PowerShell closure or environment activation contracts change.
