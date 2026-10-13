# Completion roles include the active caret and following literal arguments

## Status

Implemented in the shared grammar. Application source admission remains owned by
its request boundary and is enabled alongside each metadata source.

## Context

A caret in the middle of a word has both a typed prefix and text to replace on
its right. Options after that word can change its role: `git switch topic --detach`
requests a revision, while a later `--no-track` disables branch guessing.

## Evidence

[`command_context.rs`](../../../nebula-completions/src/command_context.rs) retains
the full active word, its byte span and following literal arguments.
[`semantic/tests.rs`](../../../nebula-completions/src/semantic/tests.rs) exercises
UTF-8 boundaries, later options, workspace selection and refspec destinations.

## Decision

Keep caret positions and whole-word byte spans in the shared command context.
Following options may select the role but later positionals do not consume the
position being completed. Unsupported expansions, incomplete following literals
and invalid UTF-8 positions decline semantic completion.

Git and package-manager roles live in cohesive grammar modules, with no process
or filesystem access. Their results describe sources, directory selection and
literal edits. Workspace selectors stay separate from the script-name prefix.
A refspec edit before its colon retains the supplied destination and force marker.

The application admits dynamic sources only when their I/O owner is available.
Grammar extraction alone cannot use a root manifest for a selected child project
or replace remote metadata with local filesystem reads.

## Rejected alternatives

- Treat the caret as the end of the command: removes following text and misses
  options that change the argument role.
- Duplicate argument rules in a terminal view: produces different behavior for
  inline suggestions and candidate lists.
- Evaluate shell expressions to identify a role: turns discovery into execution.

## Consequences

No new dependency or persisted format is introduced. The grammar is a bounded
literal parser, with native completion retained for unsupported shell expressions.
Application and UI adapters still own metadata access and native edit projection.

## Validation

The completion crate's 28 tests pass across literal shell syntaxes. Regression
fixtures preserve quoted Unicode words, later options and explicit refspec tails.
Dynamic source tests accompany their application owners.

## Supersedes

Extends the end-of-line span contract in
`../nebula_app/completion/2026-09-30-literal-path-edits.md`.

## Revisit when

Another argument grammar needs a verified caret-aware role or a concrete shell
expression requires a separate, explicitly owned completion adapter.
