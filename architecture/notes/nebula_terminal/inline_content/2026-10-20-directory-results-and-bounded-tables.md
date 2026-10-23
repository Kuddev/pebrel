# Directory output and bounded native tables

## Status

The ordinary directory-table presentation is superseded by
`architecture/notes/nebula_app/gpui_shell/terminal/native_results/2026-10-20-directory-folding.md`.
The JSON materialization limits and source-validation decisions remain applicable.

Implemented locally. Parser, ordinary-command and native viewport regressions
have run; release and cross-platform acceptance are recorded separately.

## Context

Ordinary Markdown/JSON output already enters the native-result registry through
accepted shell command text. Directory output must use the same viewport and
preserve only fields the command actually printed. Converting every space into a
filename boundary or inspecting the current local filesystem would misrepresent
quoted names, remote directories and historical results.

Large JSON arrays also need a bound during parsing, before the complete array is
materialized. A row count alone does not bound a sparse union of many columns.

## Evidence

- `command_lenses.rs` owns matching; `inline_content/command.rs` retains display
  text and source-row fingerprints without executing a command again.
- `native_result_model/directory.rs` covers POSIX long records, the existing
  PowerShell directory wrapper and PowerShell's actual table header/ruler.
- `tty/command.sh` reports full accepted commands. An interactive Bash probe
  confirmed that duplicate/ignored history can keep the same history index;
  using `history 1` without checking that index would reuse an unrelated command.
- The directory viewport regression scrolls 300 real source rows from the last
  result to the first, checks offscreen row omission and keeps the original prompt.

## Decision

Reuse native tables and their background parsing/filtering. Recognized long
listings keep reported mode, link count, owner, group, size, timestamp and name;
the PowerShell wrapper does not gain owner/group fields it never emitted.
File icons are derived only from reported mode. Ambiguous names-only output
keeps complete logical lines in original order, with no invented metadata.
Unknown layouts retain their original text. Copying continues to use the captured
display source, not reconstructed rows or a second filesystem query.

Bash 4.4+ reports through a prefix to its existing PS0; the parent shell records
the observed history generation using a zero-length parameter expansion. A
missing/ignored history entry does not borrow the previous command. Zsh uses its
accepted preexec argument. Neither path evaluates the report or overrides input
bindings. PowerShell continues using its existing accepted-line report.

JSON arrays stop while deserializing beyond 20,000 records. The existing 64-column
limit is joined by a 262,144-cell materialization limit. Original payload limits
still apply. Exceeded table limits retain full captured source for raw viewing;
they do not truncate a successful table silently. These bounds describe specific
data structures, not the total process or GPU memory ceiling.

Ordinary scrolling skips only history that was already validated at the previous
sync. Rows that were live at that sync must still be checked even if they became
historical: a batch of VT input can overwrite a row and scroll it away before the
next draw. The saved history boundary covers both marker loss and changed text
with an intact marker. Reflow/reset revalidate the complete source. Offscreen table
rows are omitted; the terminal owns scrolling.

## Rejected alternatives

- Replace `ls`/`Get-ChildItem` execution or query the directory again: changes
  command semantics and can inspect the wrong host or a later filesystem state.
- Split arbitrary whitespace into names: destroys valid filenames and ordering.
- Parse an unrestricted JSON array before checking its row count: transient
  allocations escape the intended materialization bound.
- Recheck every historical cell on each wheel event: repeats work on immutable
  history and makes scroll cost depend on the complete output length.

## Consequences

Unsupported/ignored command reports and unrecognized formats keep literal output.
Only a verified successful command on the main screen can be replaced. Large
result parsing and ordering can take longer than one frame, so they remain owned
background tasks; an old query cannot replace a newer filter/sort result.

## Validation

Model regressions cover Unicode/spaced names, symlinks, missing fields, native
PowerShell columns, original order, row/cell limits and formatted JSON objects.
The native regressions check expanded height, body font, viewport row selection,
PTY isolation and same-batch overwrite/scroll invalidation. Existing copy/hover,
long Markdown, retention and prompt
projection checks remain applicable. Timing samples are informational and do not
assert that a debug synthetic window equals hardware presentation performance.

## Supersedes

Extends `2026-10-19-command-output-lenses.md` and the table materialization bound
in `2026-10-18-native-result-surfaces.md`; transport and retention ownership stay.

## Revisit when

Real directory formats need additional fields, shell integrations expose accepted
text without history, or measured workloads invalidate the current source/layout
costs. Extend from captured output and concrete use cases, not inferred filenames.
