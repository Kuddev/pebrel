# Recover local OSC 8 file links corrupted by UTF-8 C1 byte collisions

## Status

Implemented; pending review.

## Context

CLI agents that format Markdown links as OSC 8 sequences (`\x1b]8;;file:///...\x1b\\label\x1b]8;;\x1b\\`)
sometimes emit raw, unencoded UTF-8 inside the URI and pass the rendered output through
byte-oriented ANSI layout/wrapping parsers. Multi-byte UTF-8 characters containing byte `0x9C`
(such as `最` `E6 9C 80` or `有` `E6 9C 89`) collide with the 8-bit C1 `ST` (`String Terminator`)
control byte. Depending on the upstream parser path, `0x9C` either terminates the OSC 8 URI
prematurely—leaving a trailing `U+FFFD` in the hyperlink target while spilling the remaining URI
bytes before the visible link label—or strips `0x9C` mid-segment, leaving `U+FFFD` replacement
characters inside the URI while the visible label remains intact.

## Evidence

Verified against live `agy` (`1.5.5`, `muesli/termenv` + `charmbracelet/x/ansi`) output on Windows
ConPTY (`1.24.260710001`, reported upstream as `google-antigravity/antigravity-cli#1513`; see also
`#1379`):

- ConPTY and `nebula_terminal`'s `vte` parser pass both raw UTF-8 and percent-encoded OSC 8 URIs
  intact; the corruption originates in the CLI emitter before PTY output.
- In premature-termination cases (`03_示例清单现有条目_42行_测试样本_v2.xlsx`), the cell's
  `Hyperlink` URI ends at `.../03_示例清单现\u{FFFD}` while the contiguous hyperlinked cells end
  with the intact label right before `\x1b]8;;\x1b\\`.
- In mid-segment cases (`示例报表_最终汇总结果_2026.xlsx`), the URI decodes with interior
  `U+FFFD` runs while the visible label (or parent directory on disk) holds the intact filename.

## Decision

Keep one recovery authority in `nebula_app/src/file_uri/osc8_repair.rs`, split strictly by thread
and I/O contract:

1. **In-memory hover preview vs raw match preservation (`repair_corrupted_file_uri_with_text`)**:
   gated by `has_utf8_replacement_corruption` so intact links pay zero allocations and zero grid
   reads. When a local `file:` URI contains `U+FFFD`, `display::hint` computes an in-memory repair
   candidate from the hyperlinked cell text (extending across single-row unlinked `' '` margin gaps
   for hard-wrapped TUI links only when both segments share the same explicit OSC 8 `id=` identity via
   `Hyperlink::explicit_id`, never across distinct explicit IDs, prose, or blank lines). `HintMatch::text` preserves the
   original raw `Hyperlink::uri()` so `HintInternalAction::Copy` and custom hint commands continue
   to receive the exact terminal match; `HintMatch::effective_hyperlink_uri` feeds zero-I/O hover
   previews, and `HintMatch::repaired_hyperlink_uri` passes a revalidated candidate to default-open
   dispatch.
2. **Background default-open resolution (`classify_osc8_link_target_with_cwd`)**:
   invoked only when `hint.hyperlink().is_some()` and `command == default_hint_command()`. Exact
   matching on the raw URI always runs first so literal filenames containing `U+FFFD` on disk retain
   unconditional priority. Only when the raw explicit `file:` target is missing and contains `U+FFFD`
   does classification consult the revalidated visible-text candidate (`repaired_uri`) or, when no
   visible filename candidate exists, bounded parent-directory recovery (`repair_corrupted_local_path_on_disk`),
   filtering directory candidates to those containing UTF-8 byte `0x9C` at every replaced/truncated
   span before uniqueness matching so unrelated non-C1 files remain `LocalMissing`, and leaving
   ordinary paths untouched.

## Rejected alternatives

- Scanning parent directories inside `hover_from_hint` or `draw_hyperlink_preview`. Rejected: UI
  hover and render paths must never perform synchronous filesystem I/O.
- Replacing `HintMatch.hyperlink` with a synthetic `Hyperlink`. Rejected: `should_highlight` and
  `HintMatch::text` revalidate equality against the terminal grid's stored `Hyperlink`.
- Picking the first directory match when multiple files share a truncated prefix (such as `.xlsx`
  and `_v2.xlsx`). Rejected: opening the wrong user file is worse than reporting `LocalMissing`.

## Consequences

Hover tooltips and `Ctrl+Click` / `Cmd+Click` open the intended local file for both C1-truncated
and mid-segment `U+FFFD` OSC 8 links, including hard-wrapped TUI lines and adjacent sibling links.
Uncorrupted links, remote UNC hosts, and non-local URI schemes (`https://`, etc.) are untouched.

## Validation

Unit and terminal-grid tests in `nebula_app/src/file_uri/osc8_repair.rs`,
`nebula_app/src/display/hint.rs`, and `nebula_app/src/gpui_shell/terminal/osc_links.rs` cover
mid-filename `U+FFFD`, premature C1 truncation with spilled URI tails, sibling `.xlsx` vs `_v2.xlsx`
links, hard-wrapped TUI margin gaps, non-local/remote rejection, and ambiguous directory refusal.

## Supersedes

None.

## Revisit when

If upstream CLI emitters universally percent-encode non-ASCII OSC 8 bytes and no supported tool
emits `U+FFFD`-corrupted local `file:` URIs, the fallback may be retired without changing public
configuration or persistence.
