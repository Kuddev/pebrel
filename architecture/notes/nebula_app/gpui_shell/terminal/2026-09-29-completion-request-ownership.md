# GPUI completion request ownership

## Status

Proposed; native macOS and Linux acceptance remains required.

## Context

Issue [#353](https://github.com/Kuddev/pebrel/issues/353) requests history and
current-directory suggestions accepted with Tab. The shared engine already
provided these sources, but GPUI only captured and refreshed completion input
on Windows. Its initial completion cwd was empty even when the PTY launch had
an explicit directory. Enabling the adapter on Unix also exposes the existing
synchronous filesystem scan in the paint callback.

## Evidence

- `nebula_app/src/gpui_shell/terminal/element.rs` captures grid input and the
  cursor in the same terminal lock.
- `nebula_app/src/display/input_state.rs` reconciles shell integration marks
  and echoed input, rejecting text after the cursor, including native shell
  suggestions. Keystroke reconstruction alone cannot see shell completion.
- `nebula_app/src/display/suggest_engine.rs` owns source ordering, matching,
  environment separation and candidate generation for both UI adapters.
- `nebula_app/src/gpui_shell/terminal/view/startup.rs` already resolves local
  PTY directories for directory history; completion previously discarded them.

## Decision

Use the existing grid reconciliation on every GPUI platform, including history
capture on submission. Native shell suggestions retain priority; alternate
screens and terminal vi mode do not supply completion input.

Run GPUI suggestion calculation on the existing background executor. Each view
owns its current task. Pass only the cwd, environment, input and style to the
worker, and return completion data without transferring terminal/view state.
The shared cache key includes the input, environment, cwd, style and source
generations. Input invalidation clears this key; cancellation and replacement
drop the owned task. Apply a result only while its key, environment and style
still match the view. This moves scans out of paint; it does not claim disk
operations already in progress can be interrupted or promise a latency bound.

Seed completion from the resolved local launch directory. A WSL launch's host
directory is not a guest cwd and remains unavailable until the guest reports it.
Keep remote directory queries on their existing WSL/SFTP channels. Apply the
directory-only rule to remote `cd` candidates as well as local candidates.

## Rejected alternatives

- Using the keystroke mirror on Unix: history recall and shell editing can make
  it disagree with the command actually submitted.
- Running the unchanged engine in paint on more platforms: local directories
  can reside on slow disks or network mounts.
- Porting fish's shell parser and command scripts into the terminal: the
  terminal does not own the shell's syntax or execution environment. This fix
  retains the existing engine rather than claiming complete fish equivalence.

## Consequences

Suggestions can arrive after the frame that requested them. An unchanged
query reuses its result, while a completed remote listing invalidates the query
through the existing source generation. No new dependency, persistent format
or global executor is introduced.

## Validation

Existing engine and GPUI test modules cover the issue's directory/file cases,
history from echoed input, Tab insertion without execution, remote filtering,
native shell suffixes, alternate-screen key delivery and obsolete requests.
Windows tests do not establish native macOS/Linux visual or shell acceptance.

## Supersedes

None.

## Revisit when

Shell-owned completion results become available through a verified adapter, or
measured directory/history contention requires changing shared-source ownership.
