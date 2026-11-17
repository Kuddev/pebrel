# Rank command history by use count

## Status

Accepted for implementation.

## Context

History previously deduplicated commands and suggested the newest match. Repeating
a command did not promote it above a newer one, and consecutive submissions were
discarded entirely. The requested behavior is frequency first, with recency as the
tie breaker, consistently on desktop and Android.

## Evidence

`HistoryIndex` already owns scoped desktop history and the three suggestion paths.
The Android composer reads the live session's accepted-submission history. Both
can rank at those existing boundaries without changing terminal input or remote
shell history files. PowerShell's accepted-input report can confirm a submission
already observed at Enter; counting both would inflate frequency.

## Decision

Desktop indexes retain a saturating use count alongside each command's recency.
Inline, list, and fuzzy-filtered popup matches use frequency then recency. Every
nonempty submission is appended using the existing JSONL format. Reload derives
counts from those records; discarded repeats from older versions cannot be
reconstructed. Evicting a command also evicts its count. Host/distro isolation and
history deletion retain their existing authority.

The completion context matches one pending Enter with its native shell confirmation
so it counts once. Another accepted submission, including an identical command,
counts again. Recording also advances the history revision so cached suggestions
can observe a changed rank.

Android keeps the same ranking in the existing memory-only, session-scoped history.
The 40-command / 32,768-character bounds evict least-recently-used entries and their
counts together. Closing a terminal, desktop connection, or all sessions clears
the corresponding counts. This introduces no mobile history file or synchronization.

## Rejected alternatives

- Counting unique strings: this reproduces the recency-only defect.
- Treating consecutive identical submissions as duplicates: deliberate repeated
  commands must count. Suppress only the matching Enter/native confirmation pair.
- Persisting Android command text for parity: not needed for ranking and would
  broaden its existing storage/privacy contract.
- Keeping a permanent count for every evicted command: unbounded memory conflicts
  with the existing history limits.

## Consequences

Frequently used matching commands precede newer, rarely used commands. Counts
represent retained observations, not a complete audit of remote shell execution.
Existing desktop history files remain readable without migration; repeated commands
now contribute additional records. No terminal command is executed or modified by
the ranking operation.

## Validation

Rust tests cover all suggestion paths, scope isolation, repeat confirmation,
reload, eviction, cancellation, and clearing. Android unit tests cover frequency,
recency ties, repeated commands, independent histories, and memory bounds.

## Supersedes

None.

## Revisit when

History compaction or synchronized command counters are introduced; counts must
then survive compaction without double-counting replicated records.
