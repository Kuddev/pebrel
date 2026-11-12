# Community preview and apply behavior

## Status

Implemented in the picker and the existing theme application path.

## Context

Community cards showed an empty thumbnail while every preceding download completed.
Only the footer button on installed cards selected a draft. Applying a colors-only
community theme also retained the current wallpaper, so the confirmed result could
look different from the catalog preview.

## Evidence

The picker used one sequential preview loop. Its card clipped to a rectangle even
though the outline had rounded corners. The archive copier already counts bytes
against the validated catalog size. Custom theme preference updates treated an
absent wallpaper as inheritance, which is still useful for local themes.

## Decision

- Run at most three preview transfers and decodes concurrently. Keep the picker
  generation and cancellation flag as their lifetime boundary. Cap both thumbnail
  dimensions and keep the cache in the picker.
- Report bytes written by the existing verified archive transfer. Redraw at most
  every 100 ms when progress changes; show verification and installation after
  receiving the last byte. Only a successful library install marks completion.
- Let the entire installed card select a draft, through the same selection path
  used by the footer and keyboard. Clip the image to the inner card radius.
- Applying a catalog theme uses its wallpaper, or clears the active path if it
  ships without one. The common preference adapter uses the install receipt to
  distinguish this case from local theme inheritance. When switching away,
  clear the active path only if it still points to the prior community theme's
  wallpaper and the next theme has no explicit wallpaper. Preview and
  installation do not write settings or remove any wallpaper files.

## Rejected alternatives

- A request and decoded image for every card at once: network and decode cost
  would grow with the catalog.
- Persistent preview caching: this would need invalidation, a disk budget and a
  freshness contract.
- Timed progress or automatic application: neither indicates a verified result or
  lets the user confirm the visible changes.

## Consequences

Installation still uses the catalog integrity and existing package checks. The
CLI keeps its noninteractive install API. A picker that closes during transfer
drops pending previews and stops the next read; an in-flight request can wait
until its network timeout.

## Validation

The community transfer tests check reported bytes, integrity, cancellation and
thumbnail dimensions. The rendered picker test selects the installed card by
image hitbox and keyboard, verifies that preview does not write settings, and
checks that confirming a colors-only catalog theme clears the prior wallpaper.

## Supersedes

Updates the sequential preview and inherited wallpaper behavior in
[On-demand community theme catalog](2026-11-08-on-demand-community-catalog.md).

## Revisit when

The catalog grows enough that picker-wide thumbnail retention or fetching
previews below the visible rows has a measurable cost.
