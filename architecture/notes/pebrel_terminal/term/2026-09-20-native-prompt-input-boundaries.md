# Native prompt input boundaries

## Status

Implemented while integrating PR #215.

## Context

Native CMD does not provide a pre-execution callback like the managed
PowerShell integration. PR #215 uses a prompt marker and input epochs to keep
command activity independent of Clink, including queued PTY events and nested
shells. Its text-based submission fallback still required a conventional
`C:\path>` prompt, even though the injected PROMPT preserved arbitrary text.
Custom, multiline and empty prompts therefore failed to start builtin-command
activity. History recall also cannot be recovered from a keystroke mirror.

## Evidence

- The shared input snapshot rejected `[C:\work] pause`, `>pause` and `pause`
  when those were the actual rendered custom prompt and input.
- Attaching activity to receipt of a queued UI event can misattribute a prompt
  from before Enter to the command currently running.
- PTY parsing already handles semantic OSC 133 markers between parser slices,
  where the cursor position is exact; that is the appropriate coordinate owner.

## Decision

CMD's child-only PROMPT receives OSC 133 A/B around the user's prompt, followed
by its native prompt marker. Repeated preparation is idempotent, including an
inherited prefix-only marker from the initial PR implementation.

The terminal records the B input boundary in absolute grid coordinates. The
shared input adapter extracts echoed text from that boundary, including wrapped
lines, without depending on a prompt suffix or the keystroke mirror. Prompt
navigation and input-boundary methods share the `term/prompt.rs` responsibility.
Reset, reflow, command events and input that can submit/invalidate the prompt
discard the boundary. Alternate-screen content cannot expose a primary prompt.

The application's native input epoch remains the activity authority. A B marker
does not finish a command or emit a command result; completion still requires
the current native marker plus successful process evidence. Nested interactive
shells retain their outer runtime run. Paste captures activity before echo and
does not fabricate command history.

## Rejected alternatives

- Assuming every `>` or user-entered line is a shell prompt can misclassify REPLs.
- Reconstructing commands from keys fails after history recall and editing.
- Treating a prompt as unconditional command completion loses nested runs and
  allows stale queued output to finish a later submission.
- Keeping coordinates across reflow without a mapping can read unrelated text.

## Consequences

No dependency, background service or persistent format is added. One optional
coordinate accompanies existing terminal prompt state. The input adapter keeps
its bounded wrapped-line scan. User changes that remove all injected markers
fall back to existing conservative text detection; no arbitrary prompt grammar
is guessed. Resized prompts need fresh boundary evidence for exact extraction.

## Validation

Regressions exercise split OSC input, scrolling, reset and reflow, plus rendered
custom/multiline/empty/CJK/wrapped prompts, history recall, empty Enter, paste,
runtime submission and stale prompt epochs. PR #215's existing nested-shell,
process-failure and editing-key tests remain in the native suite. A filled last
cell is already part of echoed input even while the cursor is waiting to wrap.
The full-width regression failed with a 75-column prompt plus `pause` before
this correction; it also covers an empty full-width prompt at the bottom row.

## Supersedes

None.

## Revisit when

Grid reflow exposes stable anchor remapping, or a native CMD pre-execution hook
can supply authoritative input and exit codes without overriding user behavior.
