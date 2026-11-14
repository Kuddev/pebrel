# Idle local tabs release their terminal session

## Status

Implemented; native acceptance in progress. See [#992](https://github.com/Kuddev/pebrel/issues/992).

## Context

Open tabs keep a terminal grid, a view and a shell process even while unused.
Users want to release those resources and reopen a tab at the same directory
with its recent output.

## Evidence

Descendants reveal shell jobs in separate processes, but miss PowerShell
thread jobs. Windows PowerShell integration reports readiness at the input
boundary only when its job table is empty. A partial input line can remain
in a native editor through a prompt redraw; a visible cursor alone does not
prove the editor is empty.

On a Linux aarch64 core fixture at 120 columns, 34 screen rows and 10,000
repeated output lines, live grid allocations were about 30.2 MiB. The
trimmed snapshot retained 4,262 lines in about 4.1 MiB. Across five runs,
copy took 4.9–5.1 ms and grid reconstruction took 9.3–10.2 ms, excluding
shell launch.

## Decision

Keep sleeping state in the workspace tab. Preserve its stable tab ID, shell
launch and directory with a compact, in-memory scrollback snapshot. The
sleep check requires a trusted, unedited prompt and no shell descendants.
Capture runs off the UI thread. Before replacing the pane, revalidate the
session identity, input generation and idle policy. The new pane receives
a new ID, and seeds its history before reading from the new PTY. Failed
startup leaves the snapshot available for retry.

Automatic sleep is opt-in and considers only inactive tabs. An input,
PTY report or visibility change renews the idle period. One tab per window
can be preparing to sleep at a time.

## Rejected alternatives

- Freezing the shell preserves its private state but leaves its processes
  and memory resident.
- Replaying ANSI or saving plain text breaks cell attributes, wide characters
  or wrap semantics. The history snapshot reuses the grid's cell model.
- Compressing repetitive plain text is fast, but that measurement omits
  cell attributes and reconstruction cost. Defer a compressed snapshot
  format until representative workloads justify it.

## Consequences

Shell variables, aliases and other process-local state are reset by startup.
The snapshot keeps recent terminal text and attributes, independent of inline
results and image resources. Large histories omit the oldest rows.
Snapshots are not written to disk. Ordinary workspace restoration still
stores the directory and launch under the existing format.

## Validation

Core tests cover the history budget, wide text and color, the fresh prompt,
and rejection of unsubmitted input. The PowerShell job check was also
exercised with a running and completed ThreadJob and after its removal.

## Supersedes

None.

## Revisit when

Representative workloads need longer retained history or shell integrations
can provide stronger input, job and working-directory evidence.
