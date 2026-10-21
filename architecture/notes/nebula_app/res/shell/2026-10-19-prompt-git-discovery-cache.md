# Cache prompt repository discovery, refresh HEAD directly

## Status

Proposed for review. This corrective change does not establish maintainer
approval for the deferred visual prompt feature.

## Context

The WSL Bash and SSH Bash/Zsh prompt builders ran `git rev-parse --abbrev-ref
HEAD` for every refresh. Git startup and repository discovery were measurable
costs, especially for a Windows checkout accessed through WSL's mounted drive.

## Evidence

The regression in `nebula_app/tests/shell/prompt_git.sh` counts actual discovery
invocations, including a deliberately slow Git wrapper. Repeated refreshes in
one context require no further Git invocation while branch changes remain
visible. Timing evidence compares the previous and current production scripts
on the same ext4 repository and Windows-mounted checkout; these local shell
measurements exclude SSH transport and terminal painting.

## Decision

Use one shared Bash/Zsh asset, `nebula_app/res/shell/prompt_git.sh`, assembled
into each existing installation path and the WSL prompt payload. Git resolves
the absolute Git directory when the cwd, discovery environment, nearest
repository marker or gitfile contents change. A bounded set of shell variables
caches that discovery result, including failures. No cache files or workers
are created.

Each refresh checks repository markers and reads the cached directory's HEAD
using shell builtins. Branch text continues through the existing literal-safe
Bash expansion or bounded Zsh psvar slot. Linked worktrees and submodules use
the Git-resolved directory, never an assumed `.git/HEAD` path. Detached HEAD
displays `HEAD`; a symbolic unborn branch can display its name.

## Rejected alternatives

- Caching only the branch with a timer would delay checkout changes and could
  show another directory's branch.
- Parsing gitfile targets instead of asking Git would duplicate repository
  discovery rules and platform-specific path handling.
- An asynchronous worker would require additional process, cancellation,
  output and shell-lifetime ownership for a deferred visual feature. It remains
  an option if first-discovery latency needs to be eliminated.

## Consequences

Ordinary refreshes do not launch Git. Filesystem metadata checks and HEAD reads
remain synchronous, so slow mounts can still impose a cost. First discovery and
context invalidation still perform a synchronous Git query. Failed discovery is
retried on context changes, not on every refresh; editing global Git policy
alone may therefore require leaving and re-entering the directory.

## Validation

The maintained regression exercises branch switches, detached/unborn HEAD,
worktrees, submodules, bare repositories, relative gitfiles, nested repositories,
logical symlink directories where supported, environment changes, corrupt HEAD,
failed discovery and slow Git. Existing prompt rendering, literal branch text,
WSL status/cwd reporting and bounded psvar tests remain required. Real remote
SSH transport and GUI behavior need separate validation.

## Supersedes

None.

## Revisit when

Maintainer direction establishes the visual prompt's scope, first-discovery
latency is unacceptable, or a supported Git layout needs more metadata than HEAD.
