# Bounded inline result surfaces

## Status

Height, scrolling and view retention are superseded by
`2026-10-19-unified-result-scroll.md`; ordinary command capture is extended by
`2026-10-19-command-output-lenses.md`.

Implemented locally; targeted product and Windows runtime checks passed. This note does not
claim arbitrary stdout conversion or a cross-platform performance result.

## Context

The product needs native Markdown, JSON tables and Git progress at the location of
the command result. Ordinary PTY output remains a VT stream; the existing grid is
display state, not lossless stdout. Its absolute line numbers change during reflow.
Reusing an image's old absolute anchor for an interactive result can cover later
output and route pointer input to the wrong terminal cell.

## Evidence

- `inline_content.rs` defines MIME payloads, frame limits and retention.
- `term/cell.rs` already uses optional shared `CellExtra` for uncommon attributes.
- `event_loop.rs::StreamProcessor::feed` preserves OSC/VT wire order.
- `gpui_shell/terminal/native_results` consumes only visible row markers and owns
  all renderers, filtering, sorting, theme and pointer behavior.
- `native_view_cli.rs` supplies an explicit entry from stdin/file or a Git child.
- The pinned component's `TextViewState::push_str` parses streaming appends
  asynchronously; `set_text` performs a synchronous full replacement.

## Decision

1. Use the private versioned `OSC 777;pebrel-view;1` extension with Begin/Place/Append/End
   frames. This is not advertised as a universal terminal standard or TSP support.
   Payload chunks are UTF-8 encoded through Base64. Terminal code owns framing,
   storage and placement, not Markdown/table/Git interpretation.
2. The sender writes real CRLFs between Place frames, so ConPTY and the frontend
   advance together. The first Windows live trial showed that receiver-only line
   insertion let the next command overwrite an earlier result. Native frames
   commit pending synchronized VT output before applying placement. Attach a shared content marker only
   to each reserved row's first cell, through optional `CellExtra`. Ordinary Cell
   size remains unchanged. Markers are nonempty for reflow and line-length purposes.
   The registry keeps weak references, so clear/overwrite releases the last marker's
   source immediately. Alternate-screen views do not render
   primary-screen results. A partially overwritten visible slot is not overpainted.
3. Begin declares a maximum height of 2–40 terminal rows; it initially owns one
   row. Place grows the slot sequentially at real column-zero positions, bounded
   by that cap. Short Git failures occupy two rows; each visible phase or log
   line adds one row plus the command status. The CLI and GUI share the Git model,
   including EOF handling. Static tables use their record count; Markdown uses
   a bounded source-line estimate, not a claim of exact measured document height.
   Slots only grow while producing output and never delete existing terminal
   rows. Content exceeding the slot scrolls internally; width follows the pane.
4. Bound a frame at 48 KiB, a source at 4 MiB/4096 nonempty chunks, and retained
   sources at 32/16 MiB per stream. Eviction releases raw chunks and becomes an
   explicit state on remaining markers. These numbers bound source storage only;
   UI strings, parser trees, rows, font/GPU state are additional measured costs.
5. Chunks share references across snapshot boundaries. Source revision controls
   incremental consumption. UI entities exist only for currently visible results;
   offscreen layout caches are released. Reopening a result can therefore parse
   its bounded source again and reset local filter/scroll state.
6. Table parsing and ordering run in background tasks. Query generations reject
   obsolete ordering results. Rows are virtualized. Invalid or oversized tables
   retain source and present an explicit raw view. Markdown extensions remain
   stable across frames to avoid triggering reparsing on every render.
7. Native child controls consume pointer events before the ancestor terminal;
   keyboard input stays in the relevant focus owner, with Escape returning to the
   terminal. Copy/View/Raw float over the content only while hovered, keyboard
   focused or showing clipboard success. Mouse-acquired focus alone cannot pin
   the toolbar. Hover does not change the occupied rows. Clipboard success uses
   the existing owned feedback lifecycle.
8. `pebrel view --type markdown|table|git` is opt-in. A new local product pane
   advertises `PEBREL_NATIVE_RESULTS=1`; other terminals and redirected stdout
   use text. `--native` explicitly requests a compatible remote endpoint.
9. `--type git -- git ...` captures stdout/stderr with bounded reader queues.
   Stream identity is retained in frames, parser line buffers and raw-view selectors;
   arbitrary reads from different pipes are never treated as one original byte stream.
   The child uses the C locale for known Git phase parsing. Its exit code supplies
   completion status; a phase's 100% does not mean the command succeeded. Shell
   command completion closes a still-receiving surface as interrupted.

## Rejected alternatives

- Guess every arbitrary output as JSON/objects: PTY control traffic, partial data
  and mixed stderr make that unreliable.
- Store GUI types or arbitrary action scripts in the terminal core: violates the
  dependency and trust boundaries.
- Rebuild the entire VT/history model first: the current slice needs bounded
  attachments, not a replacement terminal engine.
- Add metadata to every Cell or every historical line: unnecessary permanent cost.
- Reuse absolute image coordinates without reflow handling: stale anchors can
  hide unrelated terminal output.
- Treat a browser prototype as evidence of native performance or OS file drag:
  those are separate runtime/platform capabilities.

## Consequences

Traditional output is not automatically transformed. Initial inputs are UTF-8
Markdown, JSON object arrays/NDJSON and explicit Git capture. Tables currently
accept at most 20,000 rows/64 columns as native data; larger inputs retain a raw
view within the source budget. Result source is not persisted across app restarts.
Reserved rows remain after source eviction with an explicit unavailable notice.
Native source is not reconstructed by ordinary grid text capture; source copying
belongs to the result view. No automatic remote image or HTML loading is added.

## Validation

Core split-frame/reflow/reset and incremental placement regressions passed locally,
including rejection of gaps, duplicates and non-column-zero placement. The GPUI
regression exercises the real first-row controls: hover, copying, feedback expiry,
raw-view height, mouse/keyboard focus and PTY isolation. It exposed overlap with
the terminal search entry; reserving its hit area keeps the toolbar reachable.
The filtered application suite passed 120 tests with 8 existing ignored tests.
Windows runtime checks passed on the compact revision: consecutive Markdown and
JSON results, a real local Git clone (exit 0), a missing-repository failure (exit
128), and native idle/hover screenshots. The existing Cell-size regression remains
applicable. Visual acceptance by the user and release performance measurements
remain separate from these debug-build checks.

## Supersedes

None. Existing bounded command reading still returns guarded display text and
does not become a raw-byte result API.

## Revisit when

The slice requires exact pixel-measured document height, structured remote file actions,
unbounded data/spooling, persistence, a standard protocol, or measured costs that
invalidate the present budgets. Expand from verified workloads, not from format
names alone.
