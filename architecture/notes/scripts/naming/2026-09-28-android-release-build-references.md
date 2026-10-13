# Android release cache references

## Status

Implemented with targeted positive and negative checks.

## Context

The Android release workflow caches the existing native dependency build directory
and keys that cache by its pinned upstream manifest. These paths are build inputs,
not product comparisons, but the naming checker recognized them only in the
already-established Gradle and Python build entry points.

## Evidence

The staged check rejected both the cache path and its `hashFiles` input in
`.github/workflows/android-release.yml`. The exact reference forms already exist
in `MOBILE_BUILD_REFERENCE_PATTERNS`. The naming tests now include both rejected
workflow lines as reproducible cases.

## Decision

Apply those same exact-reference patterns to the Android release workflow.
Continue checking the rest of each line, including appended descriptions and
comments. No file, directory or arbitrary quoted string is exempted.

## Rejected alternatives

- Remove the cache to satisfy a text checker: discards useful build reuse.
- Rename a dependency directory just for the checker: unrelated source churn.
- Exempt all workflow text: permits unrelated comparisons in public CI metadata.

## Consequences

Only the established native build references gain another legitimate consumer.
Ordinary mentions and the same references in unrelated documentation still fail.

## Validation

The existing naming suite covers both workflow references, appended comparison
text, unrelated metadata and references outside their owning paths.

## Supersedes

Extends the consumer scope of `2026-09-28-mobile-integration-references.md`.

## Revisit when

Another real build consumer needs these paths; require a concrete failing example
and retain the checks on all remaining text.
