# OMP session files during local workspace recovery

## Status

Candidate for #1027.

## Context

OMP 18.22.2 accepts a session ID or a JSONL path after `--resume`. It resolves an ID within its session catalog. A saved transcript can live outside that catalog, for example in a custom session directory.

## Evidence

The OMP release writes a 256-byte title record before the session header. Pebrel's hook previously inspected the first line alone. When the title was present, it retained the running session ID and lost the file path. Local workspace recovery also generated its command from the ID alone.

[OMP serializer](https://github.com/can1357/oh-my-pi/blob/v18.22.2/packages/coding-agent/src/session/session-title-slot.ts) · [OMP resume handling](https://github.com/can1357/oh-my-pi/blob/v18.22.2/packages/coding-agent/src/main.ts)

## Decision

The OMP hook recognizes the version-one title slot and reads the following header within its existing 16 KiB limit. On recovery, the local background worker opens the regular file and verifies that its header still identifies the saved session. The terminal quotes the path for its active shell. Older ID-only records continue to use OMP's ID lookup.

## Rejected alternatives

Selecting the newest file by directory can open a different conversation. Passing a previously observed filename without rechecking its header can follow a file that has since been replaced.

## Consequences

The saved workspace uses its existing optional file field. A missing, unreadable or mismatched file falls back to the saved ID. Guest panes keep their existing lookup in their owning environment.

## Validation

In an isolated Linux ARM64 profile, the unmodified OMP 18.22.2 executable rejected a saved ID outside its catalog. Resuming the absolute path returned that same ID and its one stored message through RPC. The bridge regression exercises title-prefixed and legacy records, mismatched IDs and unrelated first records. The workspace regression checks the command queued for a matching file and for a replacement file.

## Supersedes

None.

## Revisit when

OMP changes its metadata layout, or a guest environment supplies a validated session file for path-based restore.
