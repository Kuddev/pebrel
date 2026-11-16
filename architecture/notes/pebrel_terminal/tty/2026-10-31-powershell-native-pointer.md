# Native PSReadLine pointer edits

## Status

Implemented for PSReadLine 2.1 or newer in Emacs and Windows editing modes.
Linux PTY and desktop acceptance is separate from Windows ConPTY acceptance.

## Context

The shell pointer bridge needs the actual native buffer and cursor, plus a native
range deletion that retains undo. PSReadLine exposes these APIs, but the Bash/ZLE
numeric-argument transport is not portable to .NET Console.ReadKey on Unix.

## Evidence

On Unix, Escape followed by a digit reaches PSReadLine as two keys rather than
an Alt-digit argument. PSReadLine's public two-key chord registration works on the
same real PTY. Ctrl+Shift+F11 already belongs to the native clipboard bridge and
cannot also be a pointer prefix.

PSReadLine calls prompt again on ClearScreen without entering
PSConsoleHostReadLine. Advertising a capability and OSC 133 input boundary only
at ReadLine entry loses both after Ctrl+L. Re-importing PSReadLine during a
re-source also replaces its original ReadLine function.

The GUI's generic input gate treats Ctrl+X and Ctrl+Z conservatively. PSReadLine
uses these for its Emacs undo prefix and Windows Undo. Losing the input boundary
on these keys prevents a later pointer gesture despite an unchanged native editor.

## Decision

Use owned Ctrl+Shift+F10,digit chords to collect a bounded numeric operation;
Ctrl+Shift+F12 completes it. A bare F12 chord keeps the existing completion query.
The native callback accumulates at most six digits for at most one second and
rejects changes to the native buffer/cursor between fragments. The host uses its
negotiated keyboard encoder rather than hard-coding Windows input records.

All operations recheck binding ownership, native text/cursor and shell token.
Queries return UTF-8 byte positions; edits convert scalar endpoints to native
UTF-16 offsets. A range uses one PSReadLine Delete invocation, retaining one undo
record. Vi modes, conflicting bindings, invalid Unicode and oversized buffers
are not advertised or are rejected. No stdin parser or host draft is introduced.

The Unix bootstrap wraps the original prompt and PSConsoleHostReadLine, retains
profiles and status, and marks the returned prompt on redraw. Re-source preserves
the original functions. Only a bare pwsh invocation is adapted; custom arguments
are unchanged. Generated scripts are rewritten only when content changes.

The GUI preserves the current PSReadLine input boundary for native Ctrl+X/Ctrl+Z
only while that shell's pointer capability is active. Ordinary shells, submitted
commands, alternate screens and application mouse reporting retain their gates.
AI CLI fullscreen input is unaffected.

## Rejected alternatives

- Escape-digit arguments fail the real Unix Console.ReadKey path.
- Ctrl+Shift+F11 conflicts with native clipboard paste.
- A raw stdin reader can consume normal user input and requires another parser.
- Replacing the input with a mirrored draft loses native edits and undo history.
- Guessing the input region from prompt text bypasses the existing exact native
  snapshot/cell verification contract.

## Consequences

The PowerShell adapter shares the existing snapshot and coordinate protocol with
Bash/ZLE. Binding checks and bounded temporary state occur on prompt or explicit
pointer operations, not ordinary typing. Explicit multiline, hidden/offscreen
input and other ambiguous cell layouts remain excluded by the shared mapper.

## Validation

`scripts.tests.test_powershell_pointer` exercises real PTYs with Unicode moves,
range deletion/undo, stale text/cursor and replay rejection, fragment limits,
clipboard-key coexistence, custom bindings, vi modes, prompt redraw and re-source.
The mounted `shell_pointer` tests cover routing, scoped undo keys and protocol
replies. Linux desktop tests exercise English/Chinese click insertion, range
Backspace/Delete, wrapped lines and native undo. Windows ConPTY desktop acceptance
has not been performed; Linux does not establish that result.

## Supersedes

The PowerShell exclusion in
[the initial bridge decision](2026-10-31-native-shell-pointer.md), for the verified
Linux native transport. The Bash/ZLE protocol and other boundaries remain intact.

## Revisit when

PSReadLine exposes a stable out-of-process editor protocol, its public key chord
contract changes, or Windows ConPTY acceptance demonstrates a platform difference.
