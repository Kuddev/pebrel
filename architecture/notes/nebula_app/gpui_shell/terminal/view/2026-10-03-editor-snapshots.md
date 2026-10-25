# Native editor snapshots own completion edits

## Status

Implemented. Native acceptance evidence is recorded with the review.

## Context

A terminal grid can contain prediction text after the caret. It also cannot prove
the complete input while the caret is in the middle. A keystroke mirror is not an
authoritative substitute for native history recall and shell-managed editing.

## Evidence

The native fixture types through the product input handler, paints candidates,
accepts through the keyboard handler and checks actual command effects.
Its immediate-Tab case exposed a missing native query response that ordinary
end-of-line candidates had hidden. A real isolated ConPTY probe showed that xterm
F24 is not translated into the native PowerShell handler, while Ctrl+Shift+F12
works in both tested ConPTY creation modes.

Real Bash/zsh PTYs also showed that Bash's locale-aware cursor offset must be
converted to wire bytes, and that default zsh need not bind the Delete sequence.

## Decision

[`editor.rs`](../../../../../../nebula_app/src/gpui_shell/terminal/view/editor.rs)
owns a bounded, explicit query to an advertised native line editor. PowerShell
uses an unbound Ctrl+Shift+F12 chord; Bash/zsh use an unbound F24 sequence. Existing
bindings retain ownership. The capability advertisement distinguishes legacy
PSReadLine 2.0, whose native insertion loses surrogate pairs after editing. The
view declines non-BMP insertions for that editor and retains native Tab, rather
than accepting a changed name. Modern PSReadLine, Bash and zsh retain full Unicode. The application uses its
current keyboard encoder, including negotiated input modes, rather than a
second general encoder.

The reply carries the owning shell token, cursor unit and complete buffer. The
known current shell owns admission. UTF-16 positions are converted to valid UTF-8
boundaries; partial surrogate positions, oversized or invalid text are rejected.
Snapshots carry input epochs and revisions. Edits, settings changes and dismissal
cancel pending work; obsolete results cannot refill the view. Canceled replies
are consumed in request order. Missing replies time out to native Tab.

Prompt pixels can arrive before the capability event reaches the UI. An immediate
Tab can retain its epoch-bound intent until that event arrives; repeated readiness
for the same shell does not cancel a current request. Duplicate pending Tabs do
not issue another query. Remote directory misses retain the pending action until
its cache is filled; a failed read falls back only for the still-current action.
A completed candidate cache still delivers a later native action, so echoed
results cannot strand a manual Tab. Inline accepts a resulting edit; popup/hybrid
expose the list. Unsupported middle-of-line semantics return to native completion.

Shared literal spans describe the whole active word and following arguments.
Projection converts byte ranges into edits around the actual caret. Acceptance
advances through proven right-side text, backspaces the replaced range and writes
the escaped candidate, preserving following arguments. This avoids assuming that
Delete is bound in each native editor. RefSpec edits retain their explicit
destination and force marker. Accepting a candidate consumes Enter without
executing; a subsequent explicit Enter submits the command.

PowerShell reports raw accepted text separately from alias-expanded connection
metadata, so middle editing cannot put a guessed mirror into application history.
Single-line text and its known owner are validated before recording. Shell-native
history remains the authority for native recall.

## Rejected alternatives

- Read prediction text from the grid as input: includes characters never entered.
- Trust the append-only mirror after caret motion: can splice unrelated text.
- Add a fixed sleep before Tab: conceals the echo/protocol race.
- Disable PSReadLine prediction: changes a user's editor setting.
- Unconditionally take a private key binding: overrides user-owned behavior.
- Use xterm F24 for every reader: fails the tested native ConPTY translation path.

## Consequences

There is no source-selection rule in the view. The worker receives a verified line,
caret and syntax through the existing request boundary. No new dependency,
persistence format or global input automation is introduced. Unknown editors and
unsupported shell expressions retain native fallback. This is not a general shell
interpreter or a claim that every custom keymap has the same editing semantics.

## Validation

Core tests protect UTF-8 spans, later flags, quotes and refspec preservation.
Editor state tests cover canceled replies, same-owner readiness and UTF-16 bounds.
Real Bash/zsh PTYs check buffer contents and user-binding preservation. Native
Windows fixtures cover three modes, immediate Tab, quoted Unicode caret edits,
cancellation/settings transitions and PowerShell predictions. The test driver
uses a private desktop without switching it or sending system mouse input.

## Supersedes

Extends the verified-input contract in
`../../../completion/2026-09-30-partial-input-echo.md` and
`../../../completion/2026-09-30-literal-path-edits.md`; their echo guard and literal
escaping remain applicable.

## Revisit when

Another editor has a verified buffer interface, custom editing modes require a
distinct edit command, or accepted-text reporting is added to more shells.
