# Classify URI references before checking repository paths

## Status

Approved by the maintainer on 2026-09-30 after the #386 CI failure was reported.

## Context

The governance test recognized external links only by `://`. A legitimate
`mailto:` contact newly added to CONTRIBUTING therefore failed as a missing file.

## Evidence

- [The failing architecture run](https://github.com/Kuddev/pebrel/actions/runs/36664004246)
  identifies `mailto:fickleheartedkeys@163.com` as the nonexistent relative target.
- Python's [URL parsing documentation](https://docs.python.org/3/library/urllib.parse.html#urllib.parse.urlsplit)
  exposes scheme, network location, path, query and fragment separately.
- The maintainer approved URI/path classification and targeted positive/negative
  regression coverage; no broader gate relaxation was approved.

## Decision

Use the standard library URL parser in the existing governance test. Check the
decoded relative path only for references without a scheme or network location.
Continue rejecting missing local targets; do not fetch external destinations.

## Rejected alternatives

- Treat every target without `://` as local: `mailto:` is a counterexample.
- Remove the contact link or exclude CONTRIBUTING: hides the faulty classifier.
- Silence the whole test or assume all links are external: loses local-link checks.

## Consequences

The check remains offline, dependency-free and scoped to the same public guides.
It proves local target existence, not remote availability or fragment validity.
Relative fragments/queries are not part of a filesystem name.

## Validation

The existing test checks all public guides. The new temporary-directory fixture
accepts mailto, HTTPS, network-path and fragment references plus an existing
percent-encoded Unicode filename, then verifies a missing relative file is reported.
Run the existing architecture/governance/prohibited-name unittest suite and CI.

## Supersedes

None.

## Revisit when

The project adopts a fuller Markdown parser or a separately scoped link checker.
