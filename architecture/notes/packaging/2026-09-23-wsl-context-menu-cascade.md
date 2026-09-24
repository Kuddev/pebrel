# Group WSL launch entries under one Explorer submenu

## Status

Implemented; pending review.

## Context

One top-level context item per WSL distribution crowds Explorer menus. The user
requested one ordinary Pebrel entry and one expandable entry for distribution
choices, for both selected directories and directory backgrounds.

## Evidence

The previous installer registered `PebrelWsl<index>` directly under each Explorer
shell root. Those entries remain registered after an upgrade unless explicitly
migrated. WSL distribution enumeration already excludes Docker plumbing entries.

## Decision

Keep the ordinary entry. Register a static `PebrelWslMenu` cascade with an empty
`SubCommands` value and its distribution verbs under `shell`. Record its executable
owner and validate child commands before replacing or removing the subtree.
Migrate only flat entries whose command invokes the exact owned executable.

Preserve `%1` for selected directories, `%V` for backgrounds, and the existing
`--shell` before `--working-directory` order. No distributions means no empty
submenu. Conflicting/edited subtrees and other installations remain untouched.

## Rejected alternatives

- Removing distribution choices reduces existing launch functionality.
- Deleting every key with the product prefix can remove another installation or
  user customization.
- Installing a new Explorer extension adds deployment and lifetime requirements
  unnecessary for this static cascade.

## Consequences

Registration still reflects distributions present at installation time. Existing
menus migrate when the new installer runs; changing this source does not modify
the user's live registry. Conflicts fail visibly rather than overwrite unknown keys.

## Validation

The real Inno fixture uses an isolated HKCU test subtree and passes 117 migration
checks in total. New coverage includes both roots, zero/one/three distributions,
Unicode/space-containing names, repeat registration, stale-entry removal, edited
subtrees and foreign ownership. The complete installer compiles without executing
an installation. Explorer presentation/click acceptance remains separate.

## Supersedes

The installer's flat WSL verb layout; the ordinary Pebrel verb is retained.

## Revisit when

Distribution registration must update outside installation, or Explorer requires
a different supported menu registration mechanism.
