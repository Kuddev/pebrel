# Windows GPUI main-thread stack reservation

## Status

Implemented on 2026-09-28 for the `pebrel` Windows binary.

## Context

Opening the Theme Studio picker and editor constructs a deep GPUI element tree on
the Windows UI thread. In an unoptimized MSVC build this exceeded the default
1 MiB executable stack reservation and terminated with
`STATUS_STACK_OVERFLOW` before Rust could emit a normal backtrace. That made the
development executable unsuitable for validating the UI workflow that triggered
the failure.

## Evidence

- The failure occurred in `target/debug/pebrel.exe` while opening the theme
  picker/editor and disappeared when the executable received a larger stack
  reservation.
- [The application build script](../../../../nebula_app/build.rs) owns the
  existing Windows-only executable linker configuration.
- The theme picker and editor are UI-thread GPUI overlays; moving only their
  persistent I/O to background tasks does not remove synchronous element-tree
  construction.

## Decision

Pass `/STACK:8388608` only when linking the Windows `pebrel` binary. This reserves
8 MiB of virtual address space for its main thread while Windows continues to
commit stack pages on demand. Other binaries and non-Windows targets keep their
existing linker behavior.

Keep this as a build-boundary setting rather than scattering large-stack helper
threads through individual settings views. Theme library and preference I/O
remain on the existing background executor.

## Rejected alternatives

- Leaving the 1 MiB default makes the supported debug validation path terminate
  before an error can be surfaced.
- Spawning a separate UI thread for one dialog would violate GPUI window
  ownership and duplicate application lifecycle state.
- A broad element-tree rewrite would mix an unmeasured renderer refactor into a
  focused Theme Studio lifecycle change without guaranteeing that other deep
  debug views remain safe.
- Applying the linker option to every workspace executable expands the change
  beyond the process that reproduced the failure.

## Consequences

The Windows process reserves an additional 7 MiB of virtual address range for
the main thread; this is not an assertion that 8 MiB is committed at startup or
that the theme workflow should routinely consume that stack depth. Recursive or
unbounded rendering remains a defect even with the larger guardrail.

The linker argument is MSVC-specific and remains inside the existing Windows
build-script branch.

## Validation

The user reopened and edited custom themes with the Windows debug executable
without the previous termination. The PR's Windows product build and rendered
Theme Studio tests remain the repeatable checks; the full required native matrix
must pass before merge.

## Supersedes

None.

## Revisit when

GPUI changes its Windows main-thread stack ownership; measurements show the
debug element tree stays safely below the default reservation; or the required
headroom exceeds this bounded executable setting.
