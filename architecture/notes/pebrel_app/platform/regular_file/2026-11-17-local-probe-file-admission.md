# Local probe file admission

## Status

Implemented for review as the platform prerequisite of PR #966.

## Context

Local hook metadata identifies transcript and registration files. A probe must
distinguish an absent record from an unreadable or unsuitable filesystem object,
and must not wait for a writer on a named pipe. The hook classifier owns session
identity; the platform adapter owns native file opening and file-type checks.

## Evidence

PR #966 introduced this adapter for Antigravity transcript probes. Its consumer
has a bounded listener wait and a separate worker. The adapter is reviewed here
with direct filesystem cases so native file admission can be verified separately
from conversation ordering and worker saturation.

## Decision

Require an absolute existing root, reject linked roots and ancestors during
validation, and reject relative traversal components. Unix descent below the
opened root uses directory descriptors, no-follow flags and a nonblocking leaf
open. Windows validates path segments and checks the opened disk handle. The
opened object must be a regular file. An absent relative record is `NotFound`;
an invalid root or unsuitable object is `InvalidOrError`.

These checks do not make the preliminary ancestor observations atomic with the
root open. The adapter does not claim to contain a hostile concurrent ancestor
rename. Native path observations and the hook worker's time limit have separate
responsibilities; a regular disk file can still be slow to read.

## Rejected alternatives

- Treating every error as absence could admit an unclassified conversation.
- Opening a path with blocking default flags before checking its type can wait
  indefinitely on a named pipe without a writer.
- Placing native flags and filesystem policy in the hook parser would duplicate
  the adapter boundary and make file admission harder to test independently.

## Consequences

The platform module exposes file admission and lexical path comparison, with no
new dependency, thread, persistent state or UI. PR #966 uses this contract for
its own transcript layout and lifecycle decisions.

## Validation

Direct tests cover a nested Unicode file, missing relative paths, invalid roots,
directory leaves, traversal components, linked leaves/directories/ancestors and
a Unix FIFO without a writer. The consumer retains its complete parser and
conversation regressions. Windows link fixtures require symbolic-link privilege
and report its absence separately from ordinary filesystem cases.

## Supersedes

None.

## Revisit when

A caller requires protection against concurrent ancestor replacement or adds
a filesystem object type beyond local regular files.
