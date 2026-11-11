# On-demand community theme catalog

## Status

Implemented. Linux tests cover catalog parsing, transfer checks and rendered picker
interaction. Native Windows/macOS CI and a live download against GitHub are recorded
in the PR, not assumed here.

## Context

`pebrel-community/themes` publishes `catalog/index.json`. Each row records the
release ZIP URL, its SHA-256 and size. The community repository only checks that
metadata, without downloading any archive. Before this change a user had to find a ZIP,
download it in a browser and import it by path. The app never read the catalog.
The user chose that the catalog should be fetched only when they open the
community page or run a command, never at startup or in the background.

## Evidence

- `theme_library::package::CheckedPackage` already bounds and verifies ZIP
  envelopes, resource digests and installation, and never applies settings.
- `update_proxy::agent` already resolves the Network setting, the Windows system
  proxy and environment proxies, enforces HTTPS, and re-evaluates exclusions on
  redirects. GitHub release downloads redirect to a CDN host.
- `scripts/check_catalog.py` in the community repository is the publication rule
  for rows: GitHub source, release in the same repository, a versioned tag, a
  lowercase digest, and byte limits matching the package constants.

## Decision

- Add `theme_library::community`. It parses the catalog and validates each row
  with the community script's rules. An invalid row is skipped and counted, so it
  does not hide the rest of the catalog. An unknown `schema_version` rejects the whole document.
- Treat the catalog's size and SHA-256 as fixed values for the download. Stream
  the archive into a temporary file and compare its length and digest before
  any ZIP parsing. Then install through `CheckedPackage::install_from`, which is
  the existing installer with a provenance receipt added.
- Record `metadata.package.origin` (catalog, id, version, sha256). It is used
  only to show "installed". It is never used to validate anything.
  Export already strips `metadata.package`, so the receipt does not travel.
- Fetch previews only from `raw.githubusercontent.com/<owner>/<repo>/` of the
  row's own source repository. This keeps the page from contacting arbitrary
  hosts listed in third-party metadata. Previews are decoded with dimension and
  allocation limits, and reduced to 480 px wide in the background.
- Each request creates its own agent through `update_proxy`. The failure
  classification moved there, so updater and catalog share one classifier
  but keep separate messages.
- GUI: a fifth "Community" filter in the theme picker. The picker owns the state.
  A generation check discards late results. Closing the picker sets a cancel
  flag that stops downloads at the next chunk. A completed install is added to
  the picker's snapshot and becomes the previewed draft. Applying the theme
  still requires the existing "Use this theme" button.
- CLI: `pebrel theme catalog` and `pebrel theme install <id>`.

## Rejected alternatives

- Fetch the catalog at startup or on a timer: rejected by the user. It would also
  add network traffic and a failure surface that most launches don't need.
- Trust the catalog row and extract without the package checks: the digest pins
  bytes, not format safety; the ZIP checks remain the safety boundary.
- Cache the catalog on disk: freshness and invalidation rules would be new
  persistent state. The picker-lifetime state is enough for browsing.
- Allow previews from any HTTPS host listed in the row: lets one catalog entry
  make every viewer contact a third-party host.
- Replace an older installed version in place: the store never overwrites an ID.
  A newer catalog version is installed as a new library theme instead, and the
  card offers "Install <version>".

## Consequences

The theme picker can now issue network requests, but only after the user selects
the Community filter. Library documents installed from the catalog carry a
receipt that later features (updates, removal, plugin-provided themes) can read.
Resource directories still survive library deletion, as before.

## Validation

`theme_library::community::tests` covers row validation and skipping, preview
host rules, schema rejection, exact size/digest copying and cancellation, receipt
round-trip and installed-state detection, proxied catalog requests with HTTP and
size failures, and bounded preview decoding. The rendered tests in
`settings_pane/theme_community_tests.rs` click the Community filter and the
install buttons. They assert in-place failure, that the installed theme becomes
the draft without touching settings, that no duplicate is created, and that
"Use this theme" applies it. They also check that reopening the picker starts
from fresh state. On Linux (Xvfb), the 2026-11-08 live catalog listed 8 themes. Both
`pebrel theme install` and the picker's Install button downloaded GitHub release
archives through the CDN redirect, verified them and added them to the library.
The installed state was marked, and settings were unchanged until confirmation.

## Supersedes

None.

## Revisit when

The community catalog changes schema or hosting, a signed index (the TUF
direction in the plugin boundary) replaces plain HTTPS + pinned digests, or
plugin packages start contributing themes through the same receipt.
