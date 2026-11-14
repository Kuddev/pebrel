# Managed aliases belong to exact native conversations

## Status

Implemented locally; pending maintainer review.

## Context

The native CLI conversation could resume after a restart, while its Runtime
name disappeared because the managed registry existed only in memory. Reused
windows also accept local commands that open menus or replace conversations.
Treating those commands as model work leaves an unfinishable submission barrier;
letting a name silently follow a new conversation would retarget old work.

## Evidence

The previous local build restored the three verified native conversations but
reported the formerly managed worker without its alias. The shared lifecycle
had a local-command exemption only for exact Codex /plan. Runtime Agent prompt
dispatch did not recheck the native foreground identity as delegation did.

Private live testing reproduced a Codex /new worktree chooser followed by an
empty conversation whose native id was temporarily unavailable. Waiting for a
different id alone leaves a transition interval with old cached authority.

## Decision

The existing per-pane AgentSession stores an optional managed_name beside its
provider, native session id and native file. The saved launch environment owns
the resume path. No global session-id search, focused-pane fallback, conversation
text, pending work or worktree creation ownership is persisted with the name.

The RuntimeHub owns bounded restore reservations. A reservation becomes a live
managed Agent only after the adapter reports the same provider and native id.
The recovery target alone cannot prove a live identity. Missing early window
snapshots keep reservations; terminal closure, identity mismatch or name conflict
discards them. Names cannot be stolen from another active pane.

Every RuntimeHub uses an OS-random namespace for generations and runtime ids.
Restored names receive fresh runtime identities, so cached requests from an old
process cannot match merely because a PID or a first generation was reused.
The serialized v4 format remains backward compatible through an optional field.

Explicit agent.attach lets a controller name an existing idle foreground CLI
after verifying its native id. The UI rechecks readiness and identity before
registering it. Repeating the same binding is idempotent. An explicit attach
after a conversation change invalidates the old binding and establishes a fresh
one; it neither starts a CLI nor sends a prompt.

The shared local-command classifier is deliberately narrow and provider aware.
Known local commands waive only the model-completion barrier. Their input receipt
does not claim that a menu selection has completed. They cannot create a
delegation waiting for a result that the provider will never send. Agent input
dispatch rechecks the foreground native identity before writing into the PTY.

Accepted reset commands retire the old managed binding immediately, including
commands submitted through pane input. Their receipt distinguishes a requested
reset from a completed native identity change. A cancelled chooser requires a
fresh explicit attach too. An empty native conversation without a verifiable id
cannot be attached; its controller must confirm the pane and send the first new
task there before naming the resulting native conversation. Menu state may still
be reported as idle by a provider, so an idle receipt cannot confirm a selection.

## Rejected alternatives

- Save runtime ids, generations or pending callbacks: this resurrects old routing
  authority and can replay work against a restored or replacement conversation.
- Follow names automatically across /new or /clear: native conversation ownership
  changes while previously cached selectors still refer to the old task.
- Recreate a named CLI to repair its alias: this can replace the user's open CLI
  and spend model context when only local registration is required.
- Treat every slash command as local: /compact and custom commands can call a model.
- Key persistence by cwd or pane number alone: both can be reused for other work.

## Consequences

Restored layout leaves preserve aliases only when AI resume is requested and the
same native conversation is confirmed. A changed conversation requires a fresh
list and explicit attach. Existing worktrees remain on disk without importing
stale creation ownership. Local command menus remain a read/key interaction.
No new dependency, model call, transcript rewrite or persisted answer cache is
introduced; OS randomness uses the existing getrandom dependency.

## Validation

Focused tests cover optional-format compatibility, exact native matching,
provisional recovery, window migration, name conflicts, fresh identity after a
restart, idempotent attach, stale selectors and once-only failure of old tasks.
The GPUI recovery path exercises alias retention before provider confirmation.
Local-command regressions preserve model-turn ownership for /compact and unknown
commands. The private live acceptance uses a separate configuration and CLI
conversation; user conversations are not used for reset/model tests.

## Supersedes

None. Extends the native identity and submission-ownership decisions.

## Revisit when

Providers expose an authoritative local-command acknowledgement and explicit
conversation-reset event, or the Runtime gains user-visible alias management
independent of restored native conversations.
