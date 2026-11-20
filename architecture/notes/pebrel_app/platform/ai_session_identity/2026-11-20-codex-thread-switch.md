# Checkpointing Codex after an in-pane thread switch

## Status

Implemented for pane-owned native rollout probes.

## Context

Codex can change conversations without restarting its terminal client. A
confirmed tab or pane identifier stays valid throughout that change.

## Evidence

On Linux, packaged Pebrel 2.6.1 and Codex 0.160.0 with a standalone client
saved the first thread correctly. After `/new` the same client held two open
main/user rollouts. The new thread accepted two messages while the checkpoint
kept the old ID. After process termination, a new message went into the old
thread. On Windows x64 and ARM64, Codex 0.167.0 likewise changed the native ID
while the checkpoint retained the old one for 30 seconds. The neighboring
pane kept its own ID. The Windows TUI displayed an agent-navigation hint
before the shortcuts hint on the line below the native ID.
[Windows reproduction](https://github.com/Kuddev/pebrel/actions/runs/44707360005) · [Captured footer](https://github.com/Kuddev/pebrel/actions/runs/44713190737)

## Decision

Return distinct verified rollouts to the foreground pane. A single candidate
uses the existing attribution rules. For multiple candidates, the pane can
select a unique matching native ID displayed in the Codex status line under
its empty composer. With no match, discard an earlier probe-derived live
target and keep the Codex source for the resume picker. A failed lookup leaves
the prior checkpoint alone. Generation checks and pending cold-resume
confirmation still govern asynchronous results.

## Rejected alternatives

The last open descriptor, newest file and working directory cannot distinguish
a resumed older conversation from another open thread. An ID printed in
ordinary terminal output does not establish the active native thread.

## Consequences

Exact selection among several open rollouts requires the supported status-line
layout. A detached shared daemon still needs an independent pane binding.

## Validation

The native parser tests cover duplicate and ambiguous rollouts, a selected ID
and a foreign ID. The terminal regression switches the live checkpoint, falls
back to a source-only target on ambiguity and preserves a pending cold target.

## Supersedes

Extends the [pane ownership decision](2026-11-12-detached-codex-session-ownership.md).

## Revisit when

Codex exposes an explicit client-side notification that identifies the active
conversation to the terminal.
