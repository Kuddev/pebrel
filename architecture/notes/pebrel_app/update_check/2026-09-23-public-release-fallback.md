# Public release discovery when GitHub REST access is limited

## Status

Proposed for review; implemented and verified on macOS arm64.

## Context

A macOS user received HTTP 403 when checking for updates. Startup and manual
checks use the same unauthenticated GitHub REST endpoint, so shared egress IP
quota exhaustion can disable both even when the release website is reachable.

## Evidence

On 2026-09-23, the public REST endpoint returned HTTP 403 with `API rate limit
exceeded`. The official `/releases/latest` website returned HTTP 200 after
redirecting to `/Kuddev/pebrel/releases/tag/v1.9.0`. No account credentials were
needed for the website request. The official macOS 1.9.0 DMG contains a signed
`Pebrel.app` with bundle identifier `io.github.kuddev.pebrel` and version 1.9.0.

## Decision

Only REST HTTP 403/429 invokes a second, bounded request to the official latest
release page. Resolve its proxy independently for github.com. The HTTP client's
final URI must name this repository, use HTTPS, and contain an exact numeric
major.minor.patch tag (optional v/V prefix). Do not parse website markup.

After version discovery, fetch the official `SHA256SUMS` release asset with a
separate ten-second deadline and a 64 KiB limit. Only one valid checksum for an
exact native package name grants download metadata; absence or failure retains
version discovery with manual download. Package names are shared with download
validation. Normal API responses retain digest/release-body checksum handling.
Both automatic and manual checks use this path. Local update rehearsals never
fall back to the public network.

## Rejected alternatives

- Embedding a GitHub token: introduces credentials and distribution risk for a
  public read-only operation.
- Treating an API failure as up to date: hides real failures and misses upgrades.
- Guessing package URLs or checksums from a tag: discovery does not authorize an
  installer. Scraping HTML would add a fragile metadata parser.

## Consequences

A limited API may add two ten-second requests; the website lookup permits at
most five redirects. If version discovery fails, manual checking reports an
error. Without a valid manifest entry, a newer fallback version offers manual
download only. No unverified package can be auto-executed. Unsupported future
tag formats fail explicitly rather than producing a guessed version.

## Validation

Update-check and proxy regressions passed on macOS. Coverage
includes 403/429, ordinary API success, malformed responses, fallback failure,
redirect transport and disallowed hosts/tags. An explicit live test successfully
queried the production checker and the public fallback, both returning 1.9.0.
Exact architecture/name preference and checksum manifest rejection tests passed.
Independent i18n contracts passed, including zero-allocation lookup. A native
macOS test window completed the Settings check from checking to up to date
(GitHub v1.9.0), without the previous HTTP 403. The separate
[macOS installation decision](../update_download/2026-09-23-macos-bundle-handoff.md)
records installation evidence. Windows/Linux runtime acceptance is not claimed.

## Supersedes

None.

## Revisit when

GitHub changes the public redirect contract, release tags gain another stable
format, or a first-party metadata endpoint supplies authenticated package hashes
without requiring client credentials.
