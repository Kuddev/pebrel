# Native views for ordinary shell output

## Status

Implemented on the 2.2.2 native-result branch; validation in progress.

## Context

The explicit `pebrel view` protocol rendered native content, but normal
`cat long.md` still displayed Markdown source. A terminal output feature needs
to enter through ordinary shell execution as well. The default document
component also used oversized headings and framed tables in terminal results.

## Evidence

The reference command-lens contract selects commands from shell-reported text
and preserves plain output for unmatched or interactive commands. Its terminal
Markdown styles inherit body typography, use heading scales 1.25/1.15/1.05,
and remove table outer frames. A generic Markdown component defaults to a 2.0
heading scale and a rounded full table frame; inheriting body size alone does
not implement this contract.

PowerShell integration already reports accepted, alias-resolved command text
through `pebrel_command` before OSC 133 C and reports completion via D. The
terminal processes those reports in wire order. App-side completion parsing
already distinguishes literal arguments from pipelines and shell expansions.

## Decision

The application classifies literal commands using that existing parser. A
default-empty EventListener method supplies renderer metadata to the terminal;
the terminal does not know command names or parse Markdown/JSON/Git formats.

The original command executes once, unchanged. Its output continues through
the VT parser. At successful completion, a bounded retained source range is
attached to the existing inline-content registry. The native view replaces its
visual extent while actual characters, terminal cursor and PTY geometry remain
intact. Per-row fingerprints prevent a later overwrite from retaining stale
rich content. Rejected, partial, evicted or interactive output stays plain.

The captured text is terminal display text, including soft-wrap reconstruction;
it is not advertised as byte-identical stdout. Exact source/stdout consumers
continue using the explicit view protocol. Unknown shell ownership, failed
commands and incomplete row boundaries do not create a native replacement.

## Rejected alternatives

- Rewrite `cat` aliases or replace the submitted command: changes shell meaning,
  pipeline values or history, and can accidentally run a command twice.
- Hide output before passing it through VT: ConPTY still advances its own grid,
  leaving the shell's absolute cursor operations in a different coordinate space.
- Treat every Markdown-looking line as a document: converts unrelated logs and
  interactive applications with no command boundary.

## Consequences

Automatic results appear when a recognized command completes. Streaming explicit
views retain their existing behavior. Host render snapshots must include the
source rows after compressed results, and suppress the replaced grid glyphs.
The same sparse projection continues to own scrolling and cursor placement.

## Validation

Targeted checks cover chunk boundaries, soft wraps, Unicode, preserved grid and
cursor, failed/full-screen commands and a long ordinary Markdown command whose
following prompt must remain visible. Live-window acceptance is still required.

## Supersedes

None. Extends the explicit transport and unified-scroll decisions.

## Revisit when

Additional shell integrations provide authoritative full command text, or
measured large-output retention/layout costs justify incremental host capture.
