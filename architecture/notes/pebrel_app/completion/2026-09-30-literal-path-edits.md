# Literal path completion and shared edits

## Status

Implemented; validation evidence is recorded below.

## Context

Path completion split the input at whitespace and appended the matched filename.
This lost quoted spaces and bypassed the shell-aware byte spans already used by
command semantics. The file matcher also removed quote characters and expanded
home/n-dot syntax, even when its caller had already decoded a literal argument.

## Evidence

`cat "repo name/ch"` must replace the closing quote to accept a child. A filename
beginning with a quote must not match the otherwise identical unquoted filename.
`git -C` requires directories; `checkout --` requires paths; an unmarked first
checkout argument admits both branches and paths.

A native Windows probe also reproduced a separate argv boundary: both CMD and
Windows PowerShell 5.1 mispass `git -C "repo name\" rev-parse ...`, absorbing the
following arguments. The equivalent directory spelling `"repo name/"` succeeds.
This agrees with Microsoft's [native argument parsing rules](https://learn.microsoft.com/en-us/cpp/c-language/parsing-c-command-line-arguments):
a backslash immediately before a double quote has special meaning.

## Decision

The application path source owns local/remote routing and directory ranking.
It receives the request's shell syntax and reuses the existing traversal with
cooperative cancellation. A literal matching entry preserves quote characters,
home names and n-dots; the legacy completion API retains its interpretation.
Only a proven unquoted home target expands, and the resulting candidate is an
explicit path. Shared command context supplies quoting and replacement spans.

Shared presentation maps existing semantic File/Directory metadata to popup
types and uses the same edit for inline, popup and hybrid acceptance. It no
longer owns a second path scanner or slices paths at spaces. Whole-line history
priority remains; directory history still bypasses enumeration for inline hits.

Windows path spelling stays in the platform adapter. CMD/POSIX-facing native
paths use forward slashes; PowerShell paths containing spaces do likewise to
avoid the legacy native-argv ambiguity. Unknown remote syntax only accepts
common literal characters. Remote misses retain environment-scoped demand and
never trigger host filesystem discovery.

## Rejected alternatives

- Appending a raw suffix preserves the quoted-space and closed-quote defects.
- A second filesystem engine would duplicate matching, cancellation and ranking.
- General shell evaluation could run project code and cannot prove the current
  shell's functions, variables or interpreter state.

## Consequences

Quoted paths and mixed branch/path positions share the existing application
request boundary. There is no new dependency, persistence format or thread.
This does not implement arbitrary shell expansion, middle-of-line edits, remote
shell dialect discovery, or all command-specific pathspec semantics.

## Validation

Core semantic tests cover path roles, directory scope and literal quote
roundtrips. Application request tests cover three syntax families, all three
modes, real files, type metadata and remote routing. The native window/PTY
fixture includes quoted Unicode file restoration and checks that acceptance
does not execute. A native platform regression exercises CMD and PowerShell
passing the generated directory argument to Git.

## Supersedes

None. Extends the existing completion request boundary.

## Revisit when

Remote shell identity or richer pathspec rules become verified request facts,
or a supported shell needs a distinct path spelling policy.
