# Legacy PSReadLine pointer eligibility

## Status

Implemented; real PSReadLine 2.0 and 2.4.5 Unix PTY checks passed. Windows
ConPTY and desktop acceptance of this change remain unverified.

## Context

The Windows default shell is Windows PowerShell. Its in-box PSReadLine can be
older than 2.1. The initial pointer adapter excluded the entire editor because
older PSReadLine can lose surrogate pairs after native editing. Consequently
even clicking inside an ASCII command such as `ls` did nothing.

## Evidence

The public `GetBufferState`, `SetCursorPosition` and undoable `Delete` APIs exist
in [PSReadLine 1.2](https://github.com/PowerShell/PSReadLine/blob/v1.2/PSReadLine/PublicAPI.cs)
and [2.0](https://github.com/PowerShell/PSReadLine/blob/v2.0.0/PSReadLine/PublicAPI.cs).
The existing adapter already checks owned key chords without depending on the
newer `Get-PSReadLineKeyHandler -Chord` parameter.

Loading the actual 2.0 module into a real Unix PTY verifies native ASCII/CJK
cursor movement, insertion, range deletion and undo in Windows and Emacs modes.
A buffer containing a surrogate pair can be queried and rejected without
mutating it. These are Unix editor checks, not proof of Windows input transport.

## Decision

Allow the owned bridge on PSReadLine 1.2 or newer. For versions before 2.1,
reject pointer snapshots containing any surrogate code unit. Preserve the
strict UTF-8, binding ownership, native snapshot, nonce and stale-buffer checks.
The native editor remains the only input buffer and edit authority.

## Rejected alternatives

- Requiring users to install a new shell or module for ASCII clicks.
- Removing the Unicode boundary and risking silent loss of existing emoji.
- Sending approximate arrow counts or replacing the editor buffer from a mirror.

## Consequences

Legacy editors can advertise pointer support for BMP input. Emoji-containing
buffers retain normal terminal selection without native pointer mutation.
Modern editors retain their existing Unicode behavior. No network access,
module installation, persistence format or per-keystroke I/O is added.

## Validation

`scripts.tests.test_powershell_pointer` runs the actual 2.0 module when
`NEBULA_TEST_PSREADLINE_LEGACY` names its manifest; an absent module is an explicit
skip. It also checks the modern editor, races, ownership, redraw and re-source.
`nebula_terminal/tests/native_pointer.rs` checks the in-box Windows module's
actual bindings. That Windows test has not been executed on this Linux host.

## Supersedes

Only the blanket 2.1 minimum in
[the original pointer decision](2026-10-31-powershell-native-pointer.md).
Its transport and lifecycle contracts remain unchanged.

## Revisit when

Native Windows acceptance finds an additional legacy limitation, or the
supported Windows baseline guarantees a newer PSReadLine.
