# Preserve CodeBuddy's newline binding through ConPTY

## Status

Accepted for [#839](https://github.com/Kuddev/pebrel/issues/839).

## Context

CodeBuddy's byte-oriented input reader receives CR when ConPTY translates a native
Shift+Return event. CR submits a prompt instead of inserting a newline. ConPTY's
DECSET 9001 announcement describes the host transport; it does not prove that a
child byte reader understands native key records.

## Evidence

The report compares the runtime API's 36-byte native record with the child's
single CR byte. The published
[`@tencent-ai/codebuddy-code@2.179.0`](https://registry.npmjs.org/@tencent-ai/codebuddy-code/-/codebuddy-code-2.179.0.tgz)
contains `setupWindowsTerminal`, which binds Shift+Enter to `ESC CR`. Its modified
Enter normalization uses the same sequence. Only the binding was examined; no
upstream implementation is copied into the product.

## Decision

Extend the existing program-aware multiline compatibility rule in
[`terminal_input`](../../../../nebula_app/src/input/terminal_input.rs) to return
the appropriate sequence: LF for Claude, ESC CR for registered CodeBuddy aliases.
Physical GPUI keys and runtime keys share this rule. A negotiated extended keyboard
protocol takes precedence; native readers such as Codex and PSReadLine retain
their existing Win32 records. Other chords and ordinary Enter are unchanged.

## Rejected alternatives

- Disable Win32 input globally: native readers require its modifier information.
- Guess input boxes or screen contents: the existing agent registry identifies the
  relevant application without per-keystroke scanning.
- Send LF to every agent: CodeBuddy publishes a different terminal setup binding.
- Require a user-installed key mapping: the app already owns this compatibility
  boundary for physical and runtime input.

## Consequences

The compatibility lookup has no new I/O, persistent state, dependencies or tasks.
It only applies when the running program is recognized and no extended keyboard
protocol supersedes it. Arbitrary wrappers still depend on existing program
identification. This does not add a general SendString binding action.

## Validation

Encoder tests cover aliases, repeat counts, native/legacy modes, negotiated keys
and unaffected chords. A real GPUI key event reaches the PTY channel without
committing the shell history shadow. A Windows fixture drives the product encoder
through actual ConPTY and reads native records, CodeBuddy/Claude bytes and the
negotiated sequence in isolated child processes. Native execution results belong
to the PR checks; Linux tests cannot establish Windows runtime acceptance.

## Supersedes

None. Extends the existing Claude newline compatibility boundary.

## Revisit when

CodeBuddy negotiates an extended keyboard protocol or changes its published
newline binding, or the application gains a reliable child-reader capability
contract independent of ConPTY's host mode.
