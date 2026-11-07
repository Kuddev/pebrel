# Kitty protocol: placements own the grid, the cell size comes from the pane

## Status

Proposed; implemented and validated locally on Windows x64 MSVC, Rust 1.97.1,
against yazi 26.9.1.

## Context

Terminals that implement the Kitty protocol must serve two clients with
different dialects. Neovim transmits an image and then asks the terminal to
place it at the cursor with an explicit `c`/`r` cell count. yazi (and other
"KGP-old" clients) instead asks for one `1x1` placement per cell: it moves the
cursor to every cell of the preview rectangle and sends
`a=p,i=ID,p=INDEX,x=..,y=..,w=..,h=..,c=1,r=1` each time, with the source pixel
rectangle already computed on its side. The image is the union of the cells the
cursor visited, and no command carries the total size.

The same clients ask the terminal for its cell geometry with `CSI 16 t` before
choosing a preview size. `vte 0.15` has no handler for that sequence, and ConPTY
reports a zero pixel dimension, so neither the parser nor the platform PTY can
answer. A client that never learns the cell size falls back to its configured
maximum pixel box and stretches that box across the whole preview rectangle.

The repository already owns one lifetime model for grid images: cells carry
`ImageCell`, and a renderer keeps only weak references
([grid-owned images](../../term/2026-09-27-grid-owned-inline-images.md)). The
Kitty protocol has to join that model, not run beside it.

## Evidence

Measured on yazi 26.9.1 previewing a 1179x1279 JPEG in a 46x20 cell preview
pane, 14x36 device pixels per cell (8.00x20.57 logical, scale 1.75).

- Without `CSI 16 t` the client decoded to 1200x900 (its configured maximum),
  asked for a 56x36 cell placement, and stretched that bitmap across the
  preview: aspect 0.759 of the source, i.e. visibly squashed.
- Answering `CSI 16 t` with `14`/`36` made the client decode to 644x698 and
  place 46x20 cells; the painted aspect became 0.9226 against the source 0.9218.
- A 1600x400 image placed as 46x5 cells painted 644x161, aspect 4.0000 against
  the source 4.0.
- A first implementation that reported pixel widths instead of cells, and that
  published one event per placement, produced 3680 events for one preview and a
  box larger than the pane.

## Decision

`nebula_terminal::kitty_graphics::KittyGraphics` parses the APC commands
(`a=t` transfer with chunked base64, `a=p` placement, `a=d` erase, `a=q` probe,
`a=T` transfer-and-place) and translates each dialect into the existing grid
model:

- A placement union-tracked per image id becomes one `ImagePlacement`, anchored
  at the first cell the cursor visited. `a=p` writes the tile into the grid at
  the cursor and never moves the text cursor (`C=1`), so prompt state survives.
- The bitmap is published once per transfer, not once per placement: one
  `Event::KittyImage` carrying raw pixels, the channel count and the placement.
  Decoding happens on the app's background executor through the existing
  `InlineImageStore` queue.
- `a=d` clears the cells owning that image id, drops its pixels and geometry,
  and lets the next transfer with the same id start from a clean slate. yazi
  reuses one id for the process lifetime, so a surviving geometry would decode
  the next picture at the previous stride.
- `CSI 16 t` is answered from the raw byte stream, because the parser cannot see
  it, with `\x1b[6;height;width t` in device pixels.
- `Msg::CellMetrics` carries cell metrics without resizing the pseudo console.
  Panes whose born geometry equals their target geometry never send `Resize`, so
  without this message the stream would never learn the cell size.
- Shared-memory transfers (`t=s`) answer `ENOTSUP` so clients fall back to
  base64; the transfer path allocates no per-chunk temporaries and writes no
  files.

## Rejected alternatives

- Reporting pixel dimensions in the event: the terminal would have to divide by
  a cell size it may not know, which is exactly the bug above.
- One event per placement: a single preview emits thousands of placements.
- Keeping the previous geometry when a reused id starts a new transfer: decodes
  the new payload at the old stride.
- Accepting `t=s` probes: the payload is then a shared-memory name, not pixels,
  and the terminal has no portable way to map it.
- Asking for `a=q` support by replying `OK` to everything, including shared
  memory: the client would then never send base64 transfers.
- A second image store beside `InlineImageStore`: the grid already owns image
  lifetime, and two stores disagree after scroll, reflow or alt-screen swaps.
- Painting inside a cell-sized box and stretching the bitmap to fill it: a
  client that rounds its pixel area up to whole cells paints a slightly
  non-square picture; the placement therefore keeps the bitmap's own aspect.

## Consequences

The terminal core reports cells and pixels, never a division of the two. The
app keeps one cache for all image protocols, bounded by count and bytes, and
drops entries when their grid cells disappear.

Placed images are erased by rescanning the screen once per image id, not once per
erased cell; a client that erases single placements still costs one screen scan
per erase command.

The bitmap is held in the stream until the grid cells that own it go away, so a
pane that has shown images retains one bitmap per live placement.

## Validation

`cargo test -p nebula_terminal` passes 353 tests with two pre-existing
PowerShell-proxy failures (the host has no `pwsh`), including the new coverage
for chunk reassembly, placement unions, erase plus id reuse, probe answers and
the `CSI 16 t` reply. `cargo test -p nebula --bin pebrel --features gpui-shell`
passes the image-store tests, including raw-pixel repacking for both channel
counts. `scripts/check_architecture.py` reports no errors.

On a real Windows pane, yazi 26.9.1 paints the 1179x1279 JPEG at 784x850 device
pixels (aspect 0.9224) and a 1600x400 PNG at 644x161 (aspect 4.0000), both
inside the preview rectangle; a 870 KiB JPEG preview reaches the screen within
about 150 ms of the keypress, where the previous build stalled for about a
second while it appended a log line per transfer chunk.

## Supersedes

None.

## Revisit when

A client needs the `t=s` shared-memory path, protocol image ids stop being reused
per process, or the grid gains an exact image-owner registry that makes the
per-erase screen scan unnecessary.
