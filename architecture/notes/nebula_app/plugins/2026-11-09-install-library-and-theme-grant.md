# Plugin install library and the theme-library grant

## Status

Implemented: `pebrel plugin install/list/enable/disable/uninstall`, theme contributions, and
Settings → Plugins. `theme_library` is the only grant.

## Context

The user decided that plugins and themes connect in two ways: a plugin may ship theme packages,
and a plugin may generate themes. The command foundation only ran a package directory the user
named on each invocation; there was no installed state to hang a grant on, and
[BOUNDARIES §7](../../../../nebula_app/src/plugins/BOUNDARIES.md) recorded that no authorization
library existed. The user also asked for a Plugins menu in Settings.

## Evidence

- `Package::open` already reads only the manifest and the declared entry, with link and traversal
  checks; it never scans the package tree.
- The theme library already owns validated imports (`ThemeLibraryStore::import`), portable
  package checks (`CheckedPackage`) and a provenance receipt (`metadata.package.origin`) used by
  the community catalog to mark installs.
- `ThemeDocument::from_value` is the single validation path; unknown metadata fields survive it.
- Settings pages own background tasks with a sequence number (Agents, Mobile, theme community).

## Decision

- An install copies exactly the declared files into `<settings>/plugins/<id>/` through a staged
  directory, then swaps it in. The host writes `pebrel-install.json` (version, enabled, grants);
  the copy step never takes that name from the package. Uninstall and replacement move the old
  directory into a dot-prefixed temporary directory that the listing ignores.
- The manifest only *requests* grants, derived from what it declares (`themes`, or a command with
  `output = "theme"`). The user answers once at install time. A missing grant fails before the
  library changes; only requested grants are recorded. Disable and uninstall are the revocation.
- Bundled theme packages are checked with `CheckedPackage::open` before the swap and installed
  after it with an `Origin` whose `catalog` is `plugin:<id>`. Matching receipts make reinstalling
  the same version idempotent.
- Theme commands return a complete document or `{ base, ...overrides }`.
  `ThemeDocument::with_overrides` owns the merge, so plugins never receive a separate theme rule
  set. The host stamps `metadata.generator` and imports under a fresh ID and name. Without a grant
  (a package directory, or an uninstalled plugin) `run` returns the validated document only.
- Settings → Plugins calls the same library on the background executor, one task per click,
  reloading the listing in the same task. A newer sequence discards older results.

## Rejected alternatives

- Recording grants in the manifest or a package-supplied file: the package would grant itself.
- Copying the whole package directory: undeclared files would become installed state that no
  check covers, contrary to the declared-files rule.
- Letting plugin code write theme files or call the store: duplicates validation and bypasses the
  library's naming, locking and limits.
- Applying generated or bundled themes automatically: changes appearance as a side effect;
  every other theme source only adds to the library.
- A modal dialog for the grant: an inline card in the page keeps focus order simple and shows
  the package path, while the CLI uses an explicit `--grant` flag.
- A shared registry file for all plugins: one receipt per plugin avoids a cross-plugin write
  and keeps a broken entry removable on its own.

## Consequences

- Plugins still never run at startup or in render/input callbacks. The settings page runs the
  synchronous executor on a background task; a Runtime call from a theme command keeps the
  existing 3 s budget.
- Builds without the GPUI shell reject packages that request `theme_library`.
- Themes added by a plugin outlive it. There is no automatic cleanup by provenance yet.
- New grants need their own reviewed host boundary; this library stores them but grants nothing
  by itself.

## Validation

`plugins::install_tests` covers grant refusal, declared-file copying, unrequested grants,
enable/disable, replacement, broken entries, device names, bundled theme idempotence, invalid
packages, generated spec validation and the example generator. `settings_pane::plugins_tests`
drives the grant card, install, generation, disable and uninstall through rendered controls
against temporary roots.

## Supersedes

None. Updates the §7 implementation boundary in `BOUNDARIES.md`.

## Revisit when

A second grant is proposed, plugins gain resident or event-driven execution, or plugin-added
themes need cleanup on uninstall.
