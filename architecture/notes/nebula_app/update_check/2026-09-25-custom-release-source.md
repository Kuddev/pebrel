# Configurable GitHub Release update source

## Decision

Persist one optional `update_release_url`. Empty keeps the existing
`Kuddev/pebrel` updater. A custom value must be an HTTPS GitHub Releases URL;
`releases/tag/<tag>` selects that exact release, while `releases` uses GitHub's
latest-release API.

Only discovery and the trusted release-download repository become configurable.
The existing native asset names, size limits, SHA-256 requirement, streamed
verification and platform installation handoff remain unchanged. The official
source keeps its existing 403/429 public-page fallback; custom sources return the
GitHub API error instead of adding a second fallback protocol.

## Compatibility

Existing settings omit the key and keep current behavior. Clearing the field
restores the official source; changing it also prevents cached assets from a
different repository from passing download validation.
