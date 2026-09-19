# Layered agent rules and causal notes

- Date: 2026-09-19
- Scope: repository governance and agent context
- Owner: maintainer review

## Status

Accepted by direct maintainer instruction; implementation is included with this note.

## Context

The repository had one root `AGENTS.md`. Release-note formatting, release safety,
architecture gates, UI behavior and terminal-core constraints were all loaded for
every task. Verified facts, a consolidated ADR log, plans, temporary work logs and
hard lessons also shared the broad `docs/` area despite different lifecycles.

## Evidence

- Only the root `AGENTS.md` existed before this change.
- `docs/architecture-decisions.md` had grown into one consolidated decision file.
- Local planning, handoff and hard-lesson files used `docs/` despite having causal
  or temporary lifecycles.
- Existing local-only agent context already followed a separate lifecycle outside
  the reviewed engineering documents.
- Existing architecture contracts already required non-trivial decisions,
  alternatives, consequences, validation and revisit conditions.

## Decision

Keep the root guide limited to repository-wide invariants and task entry points.
Place responsibility-specific rules in the nearest owning directory. Split release
build safety from release-note wording. Store sanitized, reviewable causal records
under `architecture/notes/`, mirroring the owning code path and without a global
index. Local-only operational context keeps its existing lifecycle.

Current factual documents continue to update in place. Accepted decision notes keep
their original rationale; a later design creates a new note and declares the
supersession relationship. Ordinary fixes and styling changes do not create notes.

The existing consolidated ADR and hard-lesson files remain historical material.
They are not bulk-rewritten or mechanically split; relevant history migrates only
when a future non-trivial change needs a new note.

## Rejected alternatives

- Keep adding every rule to root `AGENTS.md`: unrelated tasks continue paying the
  context cost and module-specific instructions remain harder to discover.
- Rewrite all historical ADRs immediately: this creates a large conflict-prone
  migration and risks changing the meaning of accepted decisions.
- Keep every causal note local and ignored: collaborators and clean clones lose the
  reviewed reasoning behind shared code.
- Maintain one global notes index: parallel branches must edit the same file for
  unrelated decisions, creating predictable merge conflicts.
- Put every plan and small fix in notes: the directory becomes a chronological
  activity log instead of a durable causal record.

## Consequences

Agents must read the root guide plus the nearest module guide. New modules add an
`AGENTS.md` only when they have distinct contracts. Notes contain only information
intended for repository review. The old ADR log stays readable but is no longer the
required destination for new decisions.

## Validation

Governance tests verify that layered guides and the note policy are tracked,
decision notes use dated module paths, contain all required sections, stay within
the scoped line limit and do not introduce a global index. Positive and negative
note fixtures lock the checker behavior.

## Supersedes

Supersedes the storage process in the `Process` section of the consolidated
`docs/architecture-decisions.md`; it does not rewrite or invalidate ADR-0001 onward.

## Revisit when

Revisit the 200-line note limit only with a reproducible legitimate note that cannot
remain clear through links or a scoped evidence file. Revisit directory ownership
when a module boundary changes in `docs/architecture.md`.
