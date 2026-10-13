# Completion package identity

## Status

Maintainer-requested incremental naming migration, 2026-09-30.

## Context

The product already uses Pebrel, but completion consumers still imported the old
Rust package name. Renaming persisted identifiers together with internal symbols
would mix data compatibility with a compile-time change.

## Evidence

The root workspace has one completion package. Its consumers are the application's
completion, command search, directory ranking and history modules. The package
has no configuration directory or wire-protocol identity. The architecture budget
currently rejects removed source roots, including a physical directory rename.

## Decision

Rename the package to `pebrel-completions` and migrate all Rust consumers to
`pebrel_completions` in the same change. Keep one implementation, the existing
features, dependency versions and source directory. The manifest's path continues
to point at that directory; source budgets and packaging scans retain coverage.

Directory migration is a separate change that must preserve scan coverage and
history-relative budgets, with legitimate-rename and forbidden-removal tests.
Other internal names can follow the same module-sized migration. Persisted
configuration, credential and protocol compatibility readers retain their current
contracts; historical release names and upstream attribution retain their meaning.

## Rejected alternatives

- Global text replacement: internal imports and persisted compatibility inputs
  have different contracts.
- A second forwarding crate: every current consumer can migrate atomically, so a
  second package would add an unnecessary permanent dependency edge.
- Removing the old scan root without replacing its contract: directory naming
  must not reduce coverage or reset legacy file budgets.

## Consequences

Cargo callers use `-p pebrel-completions`. The unchanged source path is temporary
and explicitly documented. No runtime allocation, executor, storage format or
optional-feature behavior changes.

## Validation

Validate the renamed package with default color support, both application feature
configurations, the existing completion regressions and native acceptance flow.
Check the lockfile for a local-package rename only and run architecture/name gates.
Platform CI remains required before merge; compilation is not GUI acceptance.

## Supersedes

The internal-library-name stability boundary in
[ADR-0003](../../../docs/architecture-decisions.md#adr-0003---pebrel-16-identity-migration)
for this package only. Its runtime and historical compatibility rules remain.

## Revisit when

The source-root migration is validated, or a supported external consumer requires
an explicit compatibility policy for a package import.
