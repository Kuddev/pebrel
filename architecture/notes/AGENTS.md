# Causal note rules

## Purpose

- `docs/` describes verified current behavior, architecture and operating procedures.
- `architecture/notes/` records why a non-trivial decision exists: context, rejected alternatives, consequences, evidence and replacement conditions.
- Plans, task lists and temporary work logs do not belong here.

## When to write

Write a note for dependency direction, core ownership, persistent formats, protocols, threading/lifetime, important performance contracts, governance changes or a cross-layer incident whose cause is likely to be forgotten. Do not write one for styling, ordinary single-file fixes, mechanical refactors or routine dependency updates.

## Location and history

- Mirror the owning code path, for example `architecture/notes/nebula_terminal/input/2026-09-19-example.md`.
- Use `YYYY-MM-DD-kebab-case.md`. Keep one decision or incident per file.
- Do not create a global `INDEX.md`. The nearest module `AGENTS.md` points agents to the relevant directory.
- Once accepted, do not rewrite the old rationale into a new conclusion. Create a new note with `Supersedes`; the old note may receive only a short `Superseded by` pointer.
- Notes are reviewed repository content and contain only rationale and evidence intended for repository review.

## Required structure

Each decision note stays at or below 200 physical lines and contains:

1. `Status`
2. `Context`
3. `Evidence`
4. `Decision`
5. `Rejected alternatives`
6. `Consequences`
7. `Validation`
8. `Supersedes`
9. `Revisit when`

State `None` where a relation does not exist. Link authoritative code, tests or factual docs instead of copying API inventories. Tests protect current behavior; the note preserves the reason and the discarded paths.
