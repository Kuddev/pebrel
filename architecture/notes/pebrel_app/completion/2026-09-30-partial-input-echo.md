# Completion must not target an intermediate input echo

## Status

Implemented with focused regression coverage.

## Context

File completion can finish before the shell echoes all of a text input event.
The request key correctly rejected obsolete calculations, but a fresh request
could still capture a partial echo after the entire input had already been sent.

## Evidence

The real window/PowerShell/PTY fixture sent `git checkout -- "qa in` and accepted
the first available path candidate. A query for the intermediate `..."qa i`
inserted `nline ...`, producing `..."qa innline ...`. Application calculation
tests passed because they supplied complete snapshots. Slower Git discovery
had previously hidden this gap in the same native fixture.

## Decision

The existing shared input-mirror module records the last echoed baseline and
the expected result of literal text/backspace/word deletion. Until a snapshot
reaches the expected result, prefixes along that edit remain ineligible for
completion. Replacement spans use the common prefix of the before/after text
so removal of a closing quote is also recognized as an intermediate echo.

GPUI and the shared legacy adapter use this reconciliation before starting
queries. It clears candidates and cancels pending view work during incomplete
echo. Different text is still treated as an actual shell rewrite; this does not
replace the shell's line editor. Clearing input or switching execution scopes
uses the same reset operation and drops the pending echo.

## Rejected alternatives

- Adding a sleep to the acceptance test conceals the product race.
- Slowing down file discovery only changes the chance of reproducing it.
- Using the keystroke mirror as authoritative command text breaks history
  recall, cursor editing and shell-managed completions.

## Consequences

The rule is shared across platforms and completion modes. It stores two short
prompt strings during an edit and does no I/O. Matching is linear in the
captured input length; shell-owned middle-of-line editing remains outside the
verified completion contract.

## Validation

The existing input-state tests exercise partial append/delete/replacement
echoes, Unicode boundaries, actual shell rewrites and resets. A GPUI fixture
delivers all input at once, feeds only a prefix through the terminal parser,
then verifies that the final acceptance writes the exact intended command.
The native fixture also asserts that any offered candidate corresponds to the
whole typed prefix; it does not wait an arbitrary amount of time before Tab.

## Supersedes

None.

## Revisit when

The terminal supports a verified shell-editor buffer or middle-of-line edits.
