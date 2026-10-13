# Script names belong to selected project targets

## Status

Implemented; real package-manager acceptance is recorded with the change.

## Context

The nearest package manifest is sufficient for `npm run`, but gives the wrong
scripts for an explicit workspace. Package managers also differ when a selected
package does not define a requested script.

## Evidence

The pure [`scripts` grammar](../../../../nebula-completions/src/semantic/scripts.rs)
records directory and workspace selection separately from script-name matching.
The bounded [`workspace` reader](../../../../nebula_app/src/completion/project_scripts/workspace.rs)
selects catalog entries and then combines their script names.

Owned npm and pnpm fixtures show that npm includes the root with
`--include-workspace-root`, while pnpm's explicit child filter keeps the child
selection. pnpm recursive execution skips missing scripts. Without a pnpm YAML
workspace, its recursive/filter behavior includes the local project and discovered
descendants; package.json's npm workspaces field is not its declaration authority.

## Decision

Keep argument roles and project selection in the completion crate. The application
owns bounded manifest discovery, catalog selection and cache lifetime. Local and
guest catalogs use the same target-selection rules and script-name projection.

npm and Yarn read their workspace declaration from package.json. pnpm searches
for its YAML workspace root before using its nearest-project fallback. Directory
selectors anchor discovery. Catalog traversal omits symlink directories and common
generated/dependency trees, with byte, package, directory and cooperative time
budgets. A broken manifest cannot borrow scripts from a parent project.

npm combines selected script names by intersection, or union with `--if-present`.
pnpm combines by union, matching its missing-script behavior. Yarn's explicit
workspace name is literal. Names and paths are matched against discovered targets;
an ambiguous pnpm unscoped literal is not guessed. Changed-since selectors require
a Git-owned query and are not treated as all packages.

No package manager is run to enumerate candidates. Reading script bodies cannot
be a route to executing them. Remote probes trim bodies before returning metadata.

## Rejected alternatives

- Reuse the nearest manifest regardless of selectors: suggests root scripts for
  child workspaces.
- Use one missing-script policy for all managers: npm and pnpm have different
  actual execution behavior.
- Invoke `run` or a package-manager plugin to list names: can execute project code
  during discovery.
- Add workspace rules to a view: would fork source behavior across UI modes.

## Consequences

All three completion modes share target resolution. Cache identity includes the
manager, selectors, explicit directory and source kind. This is bounded declarative
selection, not a package installation, lockfile resolver or general script runner.
Large or unsupported catalogs can fall back to the shell's completion path.

## Validation

Core tests cover flags before/after the active script, attached selectors and safe
literal edits. Application fixtures cover declared packages, exclusions, root
selection, pnpm defaults and ambiguous names. Real WSL pnpm/Yarn tests execute
accepted candidates in three modes and check distinct markers before/after.
Native npm workspace scenarios use real windows, shell buffers and command effects.

## Supersedes

Supersedes the nearest-manifest-only target selection in
`2026-09-30-semantic-arguments-and-scripts.md`.

## Revisit when

Version-aware dependency selection, lockfile information, changed-since filters
or another manager has a concrete validated completion requirement.
