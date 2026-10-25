# Portable theme resources

## Status

Implemented and validated locally as a cold package foundation on Windows;
repository review and other-platform validation are pending.

## Context

Theme JSON can persist image settings but local paths do not share the image.
The community requires one ZIP artifact carrying identity and resources, with
strict video/archive budgets and bounded application memory.

## Evidence

The theme library already owns validated native documents and optimistic imports.
The application has serde, SHA-256, temporary files, and an atomic file replacer.
Existing text adapters retain at most 256 KiB and cannot carry large binary media.
A nested-ZIP fixture demonstrates that the general reader retries an earlier
embedded end record after an invalid outer directory. A plain header-size check
cannot ensure the library parses the same envelope that the application checked.

## Decision

Add a versioned manifest and ordinary ZIP envelope at the application boundary.
Reuse native theme JSON; package resource paths are relative and refer to declared
files. The installer resolves them to managed local paths before importing a new
library document. Import does not activate settings or overwrite a source ID.

Use the ZIP library with only stored/deflate support. Preflight the bounded end
record, every central-directory record, and extra fields before allocating entry
metadata. Reject Zip64 overrides, comments, embedded footer metadata, and mismatched
counts in this bounded pass. While constructing ZIP metadata, the file adapter
masks asset bytes so reader fallback cannot select an embedded archive; normal
streaming resumes after metadata construction. Limit entry
count, metadata, actual stream bytes, and SHA-256 with one 64 KiB copy buffer.
Archive, unpacked, per-video, and total-video limits are distinct.

Export uses a sibling temporary ZIP, verifies the completed artifact, and atomically
replaces the requested destination. Install extracts only declared regular files
into a unique directory; failure before library publication removes that directory.
Images are stored without recompression, and no media decoder or execution engine
is introduced. Rust package constants own actual install limits; callers reuse that authority.

## Rejected alternatives

- Encode media as JSON/base64: creates large retained buffers and expands file size.
- Extract a ZIP before validation: lets untrusted paths select output locations.
- Trust declared compressed sizes or resource kinds alone: permits size/type bypasses.
- Add ZIP parsing to the dependency-free settings crate: violates its domain boundary.
- Enable arbitrary ZIP methods and Zip64 for sub-64-MiB themes: unnecessary complexity.
- Fold package code into text format adapters: mixes streaming resources with palette conversion.

## Consequences

The package API owns export/check/install independently from CLI registration,
shell completion snapshots, GUI controls, and community download orchestration.
Static image themes include an optional preview image; declared future resource
kinds remain unactivated.
Installed resource cleanup and references must follow library ownership rather
than deleting directories merely named in untrusted metadata. This foundation
retains resource directories when a library JSON is deleted; automatic cleanup
and GUI uninstall are not implemented.

## Validation

Tests cover image round-trip/install, author metadata, optional values, duplicate
imports, video budgets, portable paths, undeclared entries, integrity mismatch,
archive preflight, export destination preservation, and extraction rollback.
The Windows GPUI product build passed. All 41 theme-library tests passed,
including 15 package regressions. Round-trip tests install background and preview resources after their source
images are removed. The package API never publishes runtime preferences.
The related rendered theme-studio suite passed 29 tests (one existing manual visual
review skipped). Architecture, formatting, file-budget, settings, internationalization,
and governance/name checks passed locally. No media playback or GUI ZIP acceptance
is claimed; other-platform package behavior remains subject to native CI.

## Supersedes

None.

## Revisit when

Editor package controls, shared resource cleanup, signed distribution, or animated
media requires a concrete extension to this format and ownership contract.
