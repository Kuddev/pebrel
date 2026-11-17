# Preserve leading-space history opt-out through command capture

## Status

Implemented while integrating PR #990.

## Context

Command history receives submissions from both the terminal grid and native
editor reports. Trimming at either capture boundary removes the evidence needed
to omit a command whose first input character is an ASCII space.

## Evidence

The original Enter paths trimmed before calling the shared history writer.
CMD prompts without an OSC 133 boundary additionally leave a space after `>`
whose origin may be either the prompt or user input.

## Decision

Pass the original command to `NebulaHistory::record`. That shared method rejects
an initial ASCII space before normalization, so both memory and disk history
use the same rule. Program/activity bookkeeping still receives trimmed text.

Without a CMD prompt boundary, remove one separator space only when the
remaining grid text exactly matches the nonempty current keystroke mirror.
Otherwise preserve the space and omit the ambiguous line from history. Native
prompt boundaries continue to identify the input directly.

## Rejected alternatives

- Trim before recording: destroys the opt-out evidence.
- Always remove one CMD separator: confuses a typed space with prompt text.
- Guess the origin of recalled or edited text without input evidence: may
  persist a command the user intended to omit.

## Consequences

Leading-space commands still execute normally and update activity state but do
not enter Pebrel's command history. A separator-prefixed CMD recall without a
current mirror can also be omitted. Trailing-space normalization is unchanged.

## Validation

The retained regressions cover the shared history gate, Enter handling, native
editor confirmation, plain and separator-suffixed CMD prompts, and recall
without a current mirror. The integration updates their terminal crate imports
to the current workspace name and runs the native suite before merge.

## Supersedes

None.

## Revisit when

Every supported command editor provides an authoritative prompt/input boundary,
or history gains an explicit submission-level privacy field.
