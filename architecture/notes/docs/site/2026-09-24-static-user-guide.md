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
