# Configurable GitHub Release update source

## Status

Accepted for this PR.

## Context

Pebrel currently checks the official GitHub Releases feed only. Fork builds and preview releases need one optional source override without duplicating the updater pipeline.

## Evidence

The existing updater already owns asset selection, size limits, SHA-256 verification, streamed downloads, and platform install handoff. Only release discovery and trusted repository validation need to vary.

## Decision

Persist one optional `update_release_url`. Empty keeps `Kuddev/pebrel`. A custom value must be an HTTPS GitHub Releases URL. `releases` or `releases/latest` selects the repository's latest release; `releases/tag/<tag>` pins an exact release.

The official source keeps its existing 403/429 public-page fallback. Custom sources use the GitHub API directly and do not add another fallback protocol.

## Rejected alternatives

A second updater implementation per source was rejected because it would duplicate asset selection, verification, and install logic. Arbitrary download URLs were rejected because they would weaken repository and asset validation.

## Consequences

Existing settings remain unchanged when the field is absent or empty. Changing the source invalidates assets from another repository during download validation. Custom sources must keep Pebrel's existing native package naming and SHA-256 metadata contract.

## Validation

CI covers the architecture note contract, Rust formatting, native tests, and updater tests. The parser also tests latest-release normalization, explicit prerelease tags, and rejection of non-GitHub release URLs.

## Supersedes

None.

## Revisit when

Revisit if Pebrel supports a non-GitHub release backend or if package identity and verification rules become source-specific.
