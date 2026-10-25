# Semantic arguments and project script discovery

## Status

Implemented; native desktop acceptance is recorded with the change.

## Context

The application request boundary existed, but its only semantic consumer was a
parser specific to `git switch`. Adding package scripts there would duplicate
literal parsing, quoting, matching and source admission inside each I/O adapter.
The same text can also represent a new branch name, an existing branch or an
option value; completing the last whitespace token cannot distinguish them.

## Evidence

- `nebula-completions::command_context::CommandContext` owns literal arguments,
  UTF-8 replacement spans and shell-aware quoting without evaluating shell code.
- `nebula-completions::semantic::Context` selects subcommands, options, enumerated
  values, existing branches or project scripts. New names and free text have an
  explicit empty source. Checkout path positions retain the existing path source.
- `completion::Session` admits host filesystem and Git access only for a local
  input with a host execution context. Foreign and nested inputs cannot query the
  host project, even when their cwd happens to name an existing host directory.
- npm documents scripts in `package.json` and execution from the package folder:
  <https://docs.npmjs.com/cli/v11/commands/npm-run>.
  pnpm and Yarn also expose explicit script-name positions:
  <https://pnpm.io/cli/run>, <https://yarnpkg.com/cli/run>.

## Decision

Keep command semantics in the pure completion crate and I/O in the application.
Use concrete source variants and small command option tables; the second consumer
justifies sharing literal context and safe edits, not a plugin registry.

Reuse the existing matcher, prefix-first ordering and scoped history. History
may promote an existing candidate; it cannot restore a deleted branch or script.
The view retains its task, cancellation, stale-result checks and all three modes.

Read script names directly from the nearest package manifest, bounded to 1 MiB
and 64 ancestors. A found but invalid manifest stops discovery. Explicit directory
selectors anchor the read; workspace/filter selectors are not guessed. Never run
a package manager, hook, install operation or script to obtain candidates.

Each pane owns a two-second script snapshot, invalidated on submission. Locks do
not span I/O. Generation checks prevent old work from repopulating invalidated
state. This follows the existing Git cache lifetime; no new worker or polling
service is introduced.

## Rejected alternatives

- Invoke shell completion or `npm run` to discover scripts: introduces shell or
  executable behavior for information available as bounded declarative data.
- Add source selection to GPUI: would duplicate admission rules per frontend.
- Use host paths for remote inputs: produces candidates from the wrong machine.
- Treat every argument as a branch/path: corrupts new-name and free-text roles.
- Replace the matcher and history implementation: their existing contracts and
  indexes already provide the needed behavior.

## Consequences

Windows, macOS and Linux share these rules and local source implementations.
Remote literal subcommands can use static data, but remote project/branch I/O is
still unavailable through this source. Unknown syntax and unsupported command
forms retain their prior fallback; this is not a full shell interpreter.

No dependency, persistent format, color feature or UI preference changes.
The nearest-manifest source does not resolve monorepo workspace selectors or Yarn
cross-workspace script names. Those need explicit target resolution.

## Validation

Semantic tests cover argument roles, option values, fuzzy matching, UTF-8 edits,
quotes and shell-expression rejection. Application tests cover current-project
history promotion, cache invalidation, damaged manifests, cancellation and foreign
filesystem isolation. Native acceptance extends the existing real GPUI/PTY path
with subcommand and option completion plus npm scripts in all three modes; marker
files prove project code runs only after explicit submission. Native compilation
and automated tests are distinct from manual desktop verification on other hosts.

## Supersedes

Extends `2026-09-30-request-boundary.md`; source admission and request ownership
remain there. Replaces the single-command parser described in
`../git_completion/2026-09-30-local-branch-context.md` with shared argument semantics.

## Revisit when

A new source requires richer argument grammar, a verified remote execution channel
is available, or workspace selectors require project target resolution. Extend
the existing ownership boundary with those concrete use cases.
