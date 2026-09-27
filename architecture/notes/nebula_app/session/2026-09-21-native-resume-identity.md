# Native conversation identity during cold restoration

## Status

Implemented; validation results are recorded with the change. Windows discovery
and exit behavior are extended by [the open-rollout decision](2026-09-21-windows-open-rollout-and-exit.md).

## Context

A snapshot can preserve the shell, directory and layout while failing to reopen
an Agent conversation. Resume needs the provider's persisted conversation ID;
a running Hook session/group ID is not sufficient evidence of a resumable target.

## Evidence

- Codex native Hook input includes both `session_id` and `transcript_path`.
  Codex resumes a rollout thread; its native session/group has a different
  lifetime. The official [Hook reference](https://developers.openai.com/codex/hooks/)
  documents nullable transcript paths and parent identities for subagents.
- The existing WSL/Linux probe reads the active rollout metadata, including its
  thread ID, and excludes explicit subagent/guardian sources. Native Windows
  has no equivalent procfs probe and depends on accepted Hook facts.
- Remote event reduction previously dropped native files and bridge lifetimes.
- Recovery required a saved file to be repeated in every acknowledgement, even
  when the provider and exact conversation ID matched.

## Decision

Normalize native Codex Hook identities using their reported rollout paths.
Retain that path in the existing optional snapshot field; the format version
stays unchanged. Older payloads without `transcript_path` keep their compatibility
behavior. Explicit null/invalid transcripts do not manufacture a resumable ID.

For a saved snapshot, a recognized rollout path takes precedence over its old
ID. An unrecognized filename retains the existing ID, which still passes through
the ordinary resume-command validator. Optional file metadata must not erase a
previously usable target or prevent an older snapshot from resuming.

Share rollout UUID parsing between Hook normalization, native-file discovery,
the active-process probe and saved-target normalization. Parse Windows and POSIX
paths lexically: a guest path must not be searched on the host filesystem.

Retain native identity fields when reducing large remote events. A matching
acknowledgement may omit the path but cannot conflict with a saved path or ID;
merging it retains the existing path. Incomplete targets remain available for
retry and emit the existing localized failure notification.

When a submitted Codex cold resume exits unsuccessfully and explicitly reports
that its exact saved ID does not exist, open the native `codex resume` chooser
once. The old target remains durable until the provider acknowledges the user's
choice. This intentional choice can replace the ID within the same provider;
ordinary recovery acknowledgements still reject conflicting identities. Other
errors do not open the chooser, and a failed chooser cannot start a retry loop.

## Rejected alternatives

- Pick the newest conversation in the same directory: several panes may share
  a project while running independent conversations.
- Reuse a runtime group when its transcript is explicitly absent: ephemeral
  sessions do not establish that any conversation can be resumed.
- Copy conversation content into the snapshot: providers own their transcripts.
- Add a persistent terminal daemon in this fix: reconnecting a live process is
  a separate lifecycle decision and does not fix cold restore identity.

## Consequences

No new dependencies, background services or persisted fields are introduced.
Already-invalid snapshots with neither a valid native file nor the correct ID
still need an explicit conversation choice; cwd similarity cannot repair them.
This change does not preserve arbitrary shell-local environment changes or all
optional Agent launch flags across process termination.

## Validation

Regressions cover distinct Hook/rollout IDs, Windows/guest paths, absent and old
transcripts, retained Claude paths, matching/conflicting acknowledgements, and
queued cold resume for ten CLI command forms. The POSIX bridge test delivers a
large native event over a real PTY and checks identity retention; Pi event
reduction is also covered. Provider login, model calls and platform UI acceptance
remain separate from these deterministic contracts.

## Supersedes

None.

## Revisit when

A provider changes its transcript identity/naming contract, the Hook protocol
adds a dedicated resumable thread field, or persistent terminals replace cold
Agent starts with reconnection to live processes.
