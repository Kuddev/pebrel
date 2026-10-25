# Local branch completion context and request ownership

## Status

Implemented; native desktop acceptance and automated validation are recorded
separately below.

## Context

History and final-token filesystem matching cannot discover a branch the user
has never typed. `git switch` needs a branch argument, while `git switch -c`
first expects a new name. Quoted arguments also require a replacement span;
appending a suffix after a closing quote is not a faithful edit.

## Evidence

- `nebula-completions/src/command_context.rs` owns literal argument parsing,
  branch-position recognition, quoting and byte ranges.
- Git's `Documentation/git-switch.adoc` distinguishes branch, new-branch and
  start-point positions. `Documentation/git-for-each-ref.adoc` defines
  `worktreepath`; these definitions were checked against upstream documentation
  and the installed Git help before implementation.
- `gpui_shell/terminal/suggest.rs::calculate` already runs in the pane's owned
  background request and has cooperative cancellation and stale-result checks.
- `runtime_exec::build_command` owns frozen launch-environment overrides and
  direct argv construction. The terminal shell's later environment mutations
  are not observable through this interface.

## Decision

Keep parsing and edits independent of rendering. Recognize literal `git switch`
arguments, repeated `git -C`, a bounded set of switch options, and the start point
after `-c`/`-C`. Unsupported expressions, shell expansion and middle-of-line
edits do not invoke this source. Shell syntax comes from the launch executable;
unknown shells accept only the common unquoted subset. CMD expansion characters
do not have a universally safe quoting form and are not synthesized.

The current GPUI product queries local Git from its existing completion worker.
It invokes `for-each-ref` with fixed formatting and `refs/heads/` through direct
argv. No shell completion script, network request, repository file parser,
new dependency or new executor is introduced. SSH, WSL and nested remote
contexts retain their existing sources and cannot query the host repository.
The optional legacy renderer retains its existing sources.

Each pane owns one snapshot keyed by cwd and literal `-C` arguments, with a
two-second reuse window. Failure/absence is cached too. A command submission
invalidates the snapshot generation. Parsing, matching and process work occur
outside the cache lock; obsolete work cannot repopulate an invalidated generation.
The Git read has a 750 ms polling deadline and a 1 MiB output limit. Cancellation
terminates and reaps the owned child using the existing platform process-group
adapter. Process creation and an OS operation already in progress are not
interruptible by the polling flag.

The shared presentation adapter converts semantic byte spans to edits immediately
before the cursor. Inline and popup acceptance use the same edit method. Acceptance
does not send Enter. Recognized branch positions with no candidate remain empty
instead of showing unrelated files or stale command-history arguments.

## Rejected alternatives

- Run Git in paint or the input handler: subprocess/disk latency would block UI.
- Start Git for every prefix: reuse one bounded repository snapshot instead.
- Read `.git/refs/heads` directly: packed refs and linked worktrees differ.
- Pass entered text to a shell: parsing must not execute user input or expansions.
- Put subprocesses or view lifetimes in the completion library: this would reverse
  the application's dependency direction.
- Add a general provider registry, new pool or full shell parser for one source:
  the existing request boundary and a literal-context type serve this feature.

## Consequences

This slice discovers existing local branches; it is not full Git grammar,
remote-tracking discovery, alias expansion or arbitrary shell interpretation.
The cache does not watch external repository edits. Such changes appear on a
later query after expiry; submissions in this pane invalidate immediately.
The current worktree's branch and branches in other worktrees are normally
omitted; explicit detach/start-point/ignore-other-worktrees modes allow them.

## Validation

Existing completion-library tests cover argument positions, UTF-8 spans,
incomplete/closed quotes, literal escaping and expression rejection. Application
tests use real Git repositories for packed refs, linked worktrees, invalidation,
non-repositories and cancellation. GPUI tests exercise grid capture, real Git
discovery, all three modes, PTY-bound bytes, quoting and environment changes.

`git_completion_native_shell_end_to_end` is a separately invoked native desktop
test: real window, shell, PTY, painted suggestions, input-handler/keyboard routes,
and a real branch switch only after the subsequent Enter. It requires a fresh
isolated config directory and reports exact input, accepted text and branch.
An ignored native test or a successful build alone is not desktop acceptance.

## Supersedes

Extends `gpui_shell/terminal/2026-09-30-completion-lock-and-cancellation.md` with
the first semantic source. Existing history and remote-path ownership is unchanged.

## Revisit when

A second semantic command requires shared syntax structure, a verified remote
execution channel is added, or measured request cost justifies different cache
invalidation or task admission.

The child deadline is superseded by
`2026-09-30-background-query-deadline.md`; other source semantics remain in force.
