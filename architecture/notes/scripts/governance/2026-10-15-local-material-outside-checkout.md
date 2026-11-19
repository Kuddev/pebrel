# Keep local material outside the source checkout

## Status

Accepted for the release preparation under the maintainer's explicit request.

## Context

Public ignore rules previously listed machine-specific workspace directories.
The maintainer moved external inspection material outside the checkout and
requested generic public exclusions. Local Git exclusions retain existing
machine-only paths, while a fresh clone should expose misplaced material.

## Evidence

The existing governance fixture expected external directory names to remain
in the public ignore policy. Removing those rules correctly made them visible
in a fresh clone but failed that older assertion. Separately, opening entire
release-note and screenshot subtrees allowed arbitrary private documents
through unless filename-specific exclusions were repeated.

## Decision

Keep machine paths in local Git metadata. Do not publish specific local
directory rules. Material accidentally copied back into a fresh checkout
remains visible to review rather than being silently hidden.

Public release notes permit versioned Markdown and their agent guide;
screenshots permit their shot list and image formats. Other documentation
continues to require its existing explicit allowlist. Technical source,
dependencies, fixtures and diagnostic tools remain visible.

## Rejected alternatives

- Removing private-document checks leaves genuine allowlist holes.
- Adding disguised filename patterns hides the same local paths indirectly.
- Ignoring every new root directory can conceal maintained source and tests.

## Consequences

The public ignore file contains generic build/local-file rules. Each machine
owns additional workspace exclusions. A fresh-clone regression verifies
external directory examples are visible, while existing positive/negative
fixtures verify private documentation stays excluded and public assets remain
visible. Existing Git-tracked external trees are rejected.

## Validation

Run the existing governance suite with the new fresh-repository case. The
original private-document fixtures stay in place; no dependency or test-root
exclusion is introduced. Architecture and naming checks remain unchanged.

## Supersedes

The public-directory-name assumption in the prior private-material fixture.

## Revisit when

The project deliberately vendors a new external dependency, which needs an
explicit source/licensing review rather than a machine-workspace exception.
