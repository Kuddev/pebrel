# SSH configuration discovery follows the captured WSL user

## Status

Implemented for discovery and completion in an owned WSL pane. Host-library
selection and guest SSH launch have separate integration boundaries.

## Context

An alias in Windows and an alias in a WSL distribution can name different hosts.
Reading Windows config for a guest command, or merging plain alias strings before
choosing an execution environment, loses the identity needed to use guest keys.

## Evidence

- [OpenSSH configuration](https://man.openbsd.org/ssh_config) defines quoted
  Include arguments, lexical glob order and paths relative to the user's `.ssh`
  directory. `Match exec` makes `ssh -G` unsuitable for passive discovery.
- [WSL commands](https://learn.microsoft.com/windows/wsl/basic-commands) provide
  `--list --running --quiet` without selecting a distribution to execute in.
- Microsoft's [WSL client encoding initialization](https://github.com/microsoft/WSL/blob/99e41dfda9f707d5aede29b68fda3e4b3c289ddd/src/windows/common/WslClient.cpp#L1882)
  selects UTF-16 unless `WSL_UTF8` is exactly `1`. The host adapter explicitly
  retains the default encoding and validates the complete bounded response.
- Existing completion requests already own cancellation, captured execution
  identity and stale-result rejection. Guest metadata already uses isolated
  Python 3 and bounded process output.

## Decision

Keep one Rust Host/Include walker. Native and guest filesystem adapters provide
canonical identity, bounded text and ordered glob expansion. Guest paths retain
POSIX semantics independently of the Windows host. Quoted Unicode paths, Include
position and canonical cycle detection therefore use the same traversal policy.

The fixed guest reader runs with `python3 -I -S`. It reads filesystem facts only;
it does not parse SSH syntax or execute configuration content. Reads verify the
opened regular file, and directory enumeration stops at its entry budget.
Requests share a ten-second deadline, 1 MiB source budget, 64-file/eight-level
limits, 4096 examined directory entries and 1024 aliases. The existing host
adapter retains its 250 ms cooperative deadline.

Discovery declines a distribution absent from the current running-state snapshot.
WSL exposes no atomic run-only command boundary here: a concurrent external stop
between enumeration and execution remains an OS race, not a guarantee provided by
this check. Cancellation and guest-side deadlines still bound owned requests.

The reader resolves the actual guest user/home first and pins subsequent reads to
that user. Completion admits only a matching captured WSL pane; a title or nested
shell label alone is insufficient. The per-pane two-second cache includes the
distribution, requested user, config selection, home-expansion intent and cwd.
Failures are unavailable results rather than successful empty snapshots. Explicit
`-F none` remains empty. Host discovery and WSL distribution-name completion keep
their existing host scope.

## Rejected alternatives

- Host UNC filesystem reads: can block independently of guest process ownership
  and apply the wrong path/case semantics.
- Executing `ssh -G` or a second shell SSH parser: either executes Match commands
  during typing or duplicates the traversal authority.
- Aliases without a distribution/user identity: equal spelling is not the same
  connection and must not redirect a native host after a guest cache warms.
- Guest config fallback to Windows: unavailable data must not select another
  machine's credentials or hosts.

## Consequences

Guest discovery requires Python 3. No keys are copied, no SSH connection is opened
while completing, and no additional long-lived worker or dependency is introduced.
Discovery lists literal aliases; it does not claim to resolve all OpenSSH options.

## Validation

Existing scanner regression covers nested quoted Unicode Includes, source order,
cycles, invalid input, byte limits and cancellation. Scope tests separate users,
distributions, cwd and explicit config selection. The opt-in WSL request fixture
exercises the production completion entry in all three presentation modes, login
prefixes, jump hosts, `-F none` and cancellation against isolated guest files.
Management-output decoding covers Unicode names and malformed UTF-16. Native
execution results and UI acceptance limits accompany the PR.

## Supersedes

Extends the local-only SSH-config admission in
`../completion/2026-09-30-common-command-sources.md`. It retains the ownership and
failure semantics in `../completion/2026-10-03-scoped-metadata.md`.

## Revisit when

WSL provides an atomic existing-instance execution API, or unavailable Python 3
justifies another bounded filesystem adapter without duplicating SSH parsing.
