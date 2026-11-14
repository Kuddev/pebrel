# UTF-8 control symbols in the formula parser

## Status

Pinned to the upstream fix on the 0.7.1 API.

## Context

The Markdown reader and terminal use the same formula compiler. An unsupported
control symbol should return a parse error so the source remains readable.

## Evidence

The published parser used one byte for every control symbol. When the next
character needed multiple UTF-8 bytes, it panicked in `str::split_at` before
either compiler entry point could return an error. This reproduces the first
panic message in #911; the original Windows stack trace is still needed to
confirm its caller.

## Decision

Use [07a0aacc](https://github.com/Carlosted/pulldown-latex/commit/07a0aacc2ad943e650ed235266585e7cd6d8fe9e)
from the upstream repository. Its parent is the published 0.7.1 source. The
only runtime change uses the first character's UTF-8 length when lexing a
control symbol.

## Rejected alternatives

A blanket check before the parser would also reject valid literal Unicode text
inside a text argument. Version 0.8.0 also has the fix, but changes the space
event and font variants consumed by the renderer.

## Consequences

Both compiler entry points keep the same parser API and return a parse error
for unsupported control symbols. The lockfile points to the reviewed upstream
revision.

## Validation

The compiler regression panics on the published 0.7.1 crate and passes on the
pinned revision. It exercises both entry points and checks that literal
Unicode text still renders.

## Supersedes

None.

## Revisit when

Remove the Git pin when upgrading to a published version that contains the
lexer fix and passes the application's formula regressions.
