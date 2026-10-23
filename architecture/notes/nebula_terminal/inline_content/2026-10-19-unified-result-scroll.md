# Native results share the terminal viewport

## Status

Integrated onto Pebrel 2.2.2 (`f0441e6d`, tree identical to `v2.2.2`).
Core, terminal UI and migration checks passed on this baseline. Earlier 1.9.1
experimental builds are superseded. Live visual acceptance remains pending.

## Context

The initial native result implementation reserved a bounded set of terminal rows
and put independently scrollable Markdown, table, raw and Git views inside them.
User validation found inconsistent font sizes, missing hover boundaries, broken
scrolling and severe scroll latency. The explicit requirement is complete inline
output with one terminal scroll path and no per-result vertical scroller.

## Evidence

The retained Tern protocol documentation, `protocol-surfaces.md`, describes a
single grid anchor, natural-height surface layout, shared input/cursor geometry,
content-based scroll anchoring and deferred offscreen layout. Extracted
`surface-lens.css` confirms an absolute hover toolbar and a thin left boundary;
`surface-content.css` confirms inherited body typography. The probe contains no
complete termview Rust implementation. These are reference contracts and styles,
not recovered implementation code copied into Pebrel.

The previous `native_results/mod.rs` destroyed each entity when it left the
viewport and used separate scrolling containers. Re-entering could recreate
Markdown parsing and table ordering. Existing ordinary grid coordinates do not
encode natural document height.

## Decision

The CLI now emits one marker row and one real CRLF. The receiver continues to
accept existing sequential Place frames, but natural height never requires
inserting/deleting VT rows or changing PTY dimensions.

The application owns a sparse source-row/visual-row projection. Its scroll
offset, painting, pointer conversion, decorations and cursor anchors use the
same projection. The terminal exposes a presentation-only viewport origin so a
natural-height result can place the visible start inside the live grid; VT
processing retains source coordinates. Ordinary explicit scrolling clears that
presentation override, allowing search/navigation to choose a new source anchor.

Actual content layout reports height. Following output pins the content tail;
reading a result preserves its identity and offset when heights change. Source
identity retains existing renderer state across scrolling, while erased/expired
markers release it. JSON/raw visible rows are projected into the outer viewport,
without an independent scroll offset. Body and code typography derive from the
terminal; hover decorations do not participate in layout.

## Rejected alternatives

- Hide the scrollbar while retaining the old fixed-height viewport: still clips
  output and leaves independent scroll ownership.
- Reserve ever more physical terminal rows: changes source coordinates and
  cannot represent later wrapping or view changes reliably.
- Reparse and rebuild each time a result becomes visible: repeats cold work
  during scrolling.
- Claim full TSP or recovered Tern layout algorithms: neither is implemented or
  established by the retained sources.

## Consequences

The projection adds a presentation boundary without changing shell input or
the terminal's grid storage. Result-source limits remain in force. Layout and
parsed-state retention have additional cost and must be measured separately.
Existing multi-row frames keep their source extent; new CLI output uses one
anchor. A visible component must still disappear when its source is overwritten.

## Validation

On the 2.2.2 baseline, core tests passed 331 with two existing manual samples
ignored. The terminal UI suite passed 313 with seven existing platform/manual
conditions ignored, including native Markdown/table height, hover/copy controls,
wheel and scrollbar routing, prompt hit mapping, resize, alternate-screen return,
offscreen eviction and the 2.2.2 smooth-cursor projection. Four result-model and
two CLI tests also passed. Isolated internationalization passed 24 with one
existing manual sample ignored. Git Bash and PowerShell 7 were selected using
the tests' existing executable-path overrides; no production changes were made
to accommodate local PATH differences.

History-relative architecture and changed-file formatting checks passed. The
Windows build-selection and package-architecture fixtures passed; migration
checks passed 117 assertions, and installer launch checks passed 22 assertions
plus their real Run-entry fixture. Full installer syntax compiled but was not
installed. Release ZIP/installer generation remains untested in this dev build.

The 80-paragraph, 40-scroll synthetic-window sample on this baseline recorded
a median 9.661 ms and maximum 13.303 ms, including executor drain and explicit
window redraw. This is not a desktop GPU frame-time or release comparison.
Live visual acceptance, cross-platform runtime and release performance remain
separate. Version/help, PE resources and native CLI payloads are rechecked on
the final preview artifacts; the old 1.9.1 artifacts are not used as evidence.

## Supersedes

The per-result scrolling and height-cap decisions in
`2026-10-18-native-result-surfaces.md`. Other transport, ownership and limits stay.

## Revisit when

Measured large-document layout or retained-state costs need a more granular
viewport cache; use stable content identities rather than per-scroll rebuilding.
