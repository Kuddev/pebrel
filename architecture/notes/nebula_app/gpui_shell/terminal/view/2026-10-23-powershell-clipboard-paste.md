# PowerShell clipboard paste belongs to the native editor

## Status

Implemented; maintainer review pending.

## Context

Without negotiated bracketed paste, `paste_now_impl` converts CRLF and LF to CR.
PSReadLine receives these characters as Enter, so a multiline clipboard action
can execute commands before the user reviews the whole input. A confirmation
dialog permits that delivery but does not turn it into a multiline editor change.
See [issue #567](https://github.com/Kuddev/pebrel/issues/567).

## Evidence

An isolated Windows ConPTY probe with PowerShell 7.6.8 / PSReadLine 2.4.5 reproduced
execution of three pasted commands without a separate Enter. Removing Pebrel's
shell integration did not remove the behavior. Sending LF instead of CR, with
or without an unnegotiated paste wrapper, kept input unsubmitted but reversed the
lines. Native Shift+Enter and PSReadLine Paste retained their order.

PSReadLine's [native Paste implementation](https://github.com/PowerShell/PSReadLine/blob/v2.4.5/PSReadLine/KillYank.cs#L669)
inserts or replaces an editor region as one edit. It removes CR and expands tabs
to four spaces. This is native editor normalization, including inside strings;
the clipboard contents are not promised to survive byte for byte. PSReadLine's
[bracketed-paste support request](https://github.com/PowerShell/PSReadLine/issues/1471)
explains why a terminal must not infer protocol support from the shell name.

## Decision

[`clipboard.ps1`](../../../../../../nebula_terminal/src/tty/clipboard.ps1) installs
a delegate to PSReadLine's built-in Paste on Ctrl+Shift+F11 only when that chord
is unbound. The handler's name carries the shell instance token: PSReadLine lets
user scripts use the name `Paste`, so that display name alone cannot establish
ownership. The public `SetKeyHandler` API receives a native `Action` delegate to
`PSConsoleReadLine.Paste`, without a PowerShell pipeline, another clipboard
reader or a second insertion algorithm.
Every prompt rechecks the owned binding and reports `pebrel_clipboard_ready`
with the current `pebrel_shell` token, or an empty value to revoke capability.
Vi mode is excluded because its active keymap can change without another prompt.
Existing user bindings retain ownership.

[`native_clipboard.rs`](../../../../../../nebula_app/src/gpui_shell/terminal/view/native_clipboard.rs)
owns the UI admission state. A new shell report, including the same token at the
next prompt, clears the previous capability. Command start also clears it.
The platform capability table owns host support, leaving platform compilation
branches out of the view. Admission requires the current shell owner, a local
Windows environment and an active prompt without a foreground Agent, executing command, alternate screen,
terminal Vi mode, bracketed paste or Kitty keyboard protocol.

The view delegates only when the current OS clipboard text still matches the
text captured for the action. It sends one key through the existing encoder and
input lifecycle, preserving selection clearing, scrollback following and editor
snapshot invalidation. It emits the original text as the existing broadcast
event; receivers independently check their own editor and never emit again.
No command is recorded as submitted merely because the paste key was sent.
The legacy VT encoding of the same paste chord also preserves the active prompt;
otherwise a second paste would lose admission despite the first remaining editable.

Unsupported targets retain the existing byte delivery and confirmation rules.
Frozen confirmation text, dropped paths and Runtime API payloads retain their
original delivery paths. In particular, native clipboard capability does not
satisfy the Runtime API's bracketed-paste requirement.

## Rejected alternatives

- Send raw LF: the native reader treats it as an editing key rather than an
  inserted newline; the tested buffer order is wrong.
- Force bracketed-paste delimiters: an unnegotiated reader does not gain support.
- Wrap every payload in `& { ... }`: changes PowerShell syntax and scope and
  would require separate rules for other shells and foreground programs.
- Simulate each newline with editing keys: depends on per-character keymaps,
  invokes other bindings for tabs/control characters and splits native Undo.
- Replace PSReadLine Paste with another clipboard reader/insertion handler:
  duplicates the editor's selection, normalization and Undo semantics.
- Invoke Paste through a PowerShell ScriptBlock: the tested sessions retained
  Unicode in the editor buffer but rendered emoji and the continuation prompt
  as question marks. Direct native delegate registration preserves rendering.

## Consequences

There is no new dependency, persisted setting, worker or general input encoder.
The existing paste gesture produces an editable multiline buffer at supported
prompts, so it does not need the raw-input execution warning there.

PSReadLine reads the clipboard when it handles the queued key. Matching text at
dispatch does not freeze the OS clipboard against a later external change.
Custom keymaps must expose the checked native binding; per-prompt advertisement
does not promise detection of a binding changed midway through editing.
Other shells, remote sessions and foreground applications keep their contracts.

## Validation

The [GPUI regressions](../../../../../../nebula_app/src/gpui_shell/terminal/view/native_clipboard_tests.rs)
exercise the clipboard action, consecutive native/VT broadcast delivery, current owner, capability
revocation, command start, changed clipboard, remote context and negotiated modes.
They also verify rejection and original-text fallback on unsupported hosts.
The [binding test](../../../../../../nebula_terminal/tests/native_clipboard.rs) runs
real PSReadLine keymaps and checks installation, reload, preserved user handlers,
revocation, Vi exclusion and Emacs support. A same-name user script regression
fails with the original `Function == Paste` check and remains unclaimed after
reload with the owned handler marker, as does a user-assigned built-in Paste.

The opt-in [Windows acceptance fixture](../../../../../../nebula_app/src/gpui_shell/terminal/view/native_clipboard_tests/acceptance.rs)
uses a fresh configuration, the real clipboard, GPUI and ConPTY. Its Windows
shell bootstrap belongs to the existing platform shell adapter. It checks the
native buffer, no execution before Enter, one Undo/Redo, explicit execution,
here-strings, Unicode in both the editor buffer and visible grid, tab normalization,
cancellation and insertion at the caret.
It drives the product input callbacks and can capture its native window; this
does not claim physical keyboard hardware or every custom editor was tested.
Clipboard cleanup restores the previous contents only while the fixture still
owns the current text.

Native acceptance was run on Windows x64 with Windows PowerShell 5.1 / PSReadLine
2.0.0 and PowerShell 7.6.8 / PSReadLine 2.4.5. Both versions were checked with
in-box ConPTY and the repository-pinned ConPTY runtime; PowerShell 7 also used
translated VT key delivery. Owned native windows were inspected at 192 DPI.
Other operating systems,
display scales and physical keyboard hardware are outside this evidence.

## Supersedes

None. Extends the current-owner boundary in
[`2026-10-03-editor-snapshots.md`](2026-10-03-editor-snapshots.md).

## Revisit when

PSReadLine negotiates bracketed paste, a verified editor exposes a better
snapshot-based paste API, or another native clipboard normalization needs an
explicit product choice.
