# Stable release note verbs

## Status

Proposed for review on 2026-11-12.

## Context

Stable release notes follow the bilingual 1.x format. English entries begin with the verb in their section heading.

## Evidence

The existing preflight accepted `- A package set.` under `Added` and `- Added file imports.` under `Fixed` in otherwise valid notes.

## Decision

For versions 2.5.0 onward, validate top-level list entries in the English change categories during stable release preflight. Each used category has at least one entry and each entry starts with its category verb. The check leaves prefaces, nested details, translated text and historical notes under their existing rules.

## Rejected alternatives

Checking only at publication would catch the error after native builds. Applying the new check to archived notes would require rewriting old copy.

## Consequences

A malformed entry fails before packaging and reports its category and position in that category.

## Validation

The regression test accepts a bilingual note with nested details and rejects missing, mismatched and partial verbs. Historical notes continue through the existing tests.

## Supersedes

None.

## Revisit when

Stable notes adopt a different change list format.
