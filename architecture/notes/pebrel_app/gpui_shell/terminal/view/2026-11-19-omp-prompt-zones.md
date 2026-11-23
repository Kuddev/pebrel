# OMP message redraws and foreground session lifetime

## Status

Implemented for local session recovery (#1027) and hookless task titles (#1029).

## Context

OMP 18.22.2 marks each rendered user message as a terminal prompt zone. The end of that zone carries the same successful OSC 133 command-completion marker as PowerShell.

## Evidence

On Windows x64 a new OMP conversation wrote its user message and assistant reply to its native transcript. Pebrel accepted the session-start hook, then cleared its foreground identity in the command-completion handler while PowerShell and OMP were both still running. Autosave lost the session. The same sequence occurred while displaying a restored message. [Windows trace](https://github.com/Kuddev/pebrel/actions/runs/44487591689)

InstaBox reproduced #1029 on 2.6.0 and 2.6.1 with hooks disabled. The native OMP title still contained the task after a reply, while the workspace tab showed only the directory. The OMP message renderer appends consecutive OSC 133 B, C and D;0 markers.

## Decision

The terminal records whether a successful completion immediately followed the empty B/C/D sequence used by OMP. In a local OMP pane, that sequence preserves the active command and its title with or without hooks. A separate shell completion retains its exit code.

## Rejected alternatives

The presence of a JSONL file cannot establish that its process is still attached. Waiting longer for session metadata cannot recover an identity the completion handler explicitly cleared.

## Consequences

Redrawing a message can no longer discard the ID needed for autosave and restart. A local OMP process killed without a shutdown hook can remain visible until the next process-tree check.

## Validation

The view regression replays command start and successful completion after the OMP hook, then supplies process-exit evidence. A separate nonzero completion checks the shell failure path.

## Supersedes

None.

## Revisit when

OMP distinguishes its message boundaries from the outer shell’s command boundary.
