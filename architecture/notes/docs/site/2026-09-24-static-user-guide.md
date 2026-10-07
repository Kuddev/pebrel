# Static user guide build boundary

## Status

Proposed with the documentation-site pull request.

## Context

The desktop application needs a navigable user manual that can be reviewed in a
fork, published through repository Pages, and read without a running service.
Desktop builds must not acquire a web-toolchain dependency.

## Evidence

- [Builder](../../../../docs/site/build.py) converts reviewed Markdown into HTML.
- [Workflow](../../../../.github/workflows/docs-site.yml) builds independently of Cargo.
- [Checks](../../../../docs/site/test_site.py) cover references and search coverage.
- [Operations](../../../../docs/site/README.md) describes Pages activation.

## Decision

Keep the site under `docs/site`, with build-only pinned Markdown and highlighting
libraries. Emit independent HTML pages, relative asset links and a local search
index. Use system fonts and repository-owned application imagery. Production
Pages deployment requires explicit repository opt-in and only runs from `main`.

## Rejected alternatives

A client-only application would make basic reading depend on JavaScript. A
hosted search service would add a credential and network dependency. A second
large frontend framework is unnecessary for Markdown, navigation and search.

## Consequences

Markdown and navigation remain separately reviewable. Rich HTML is trusted
repository content, not an input format for arbitrary visitors. The build has no
server-side user input and does not affect the desktop runtime. Captured app
images require a separate native environment; browser screenshots verify only
the guide's own interface.

## Validation

Local structural tests and the independent browser workflow exercise generated
links, search, copying, theme persistence, mobile navigation and no-script reading.
CI results, rather than the presence of workflow files, establish run status.

## Supersedes

None.

## Revisit when

Multiple maintained languages or versioned manuals make the current navigation
manifest insufficient, or the small local search index no longer meets users'
needs. Re-evaluate on measured behavior rather than adding services in advance.

## Colour themes (2026-10-07)

The guide takes its colours from the application's built-in themes instead of a
site-specific palette, so the documentation reads as part of the product and
readers can choose the theme they already use. `docs/site/palettes.py` parses the
catalog from `nebula_settings/src/themes.rs` at build time; the build fails when
that source changes shape. A hand-copied colour table was rejected because it would
drift from the application. A fixed light/dark pair was rejected because the
application ships sixteen themes and a toggle cannot express them. Where a theme's
accent or muted colour is too faint for text, the site darkens or lightens it toward
the theme foreground until it reaches 4.5:1; the application's own colours are
unchanged. The choice is stored in browser storage only.

## Release baseline (2026-10-07)

The base pages were re-audited against the `v2.1.1` tag and the manifest now records
that version and commit for every page; the earlier page-level `main` overrides were
removed. The tag, not `main`, is the baseline because the guide describes what readers
can install: `main` carries features that are not in any release. Pages whose behaviour
changed between 1.9.1 and 2.1.1 were updated from the code at the tag (platform
integrations on macOS and Linux, Windows ARM64 installer, release asset names, update
channels, settings navigation) and from the release notes for 2.0.0, 2.1.0 and 2.1.1.
Differences that are only translation refactors were not treated as behaviour changes.

