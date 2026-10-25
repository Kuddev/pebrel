# Completion query transport and visible-menu ownership

## Status

Implemented; focused state tests and real Debian WSL native acceptance pass. The user's running installed build is unchanged until a replacement package is installed.

## Context

A WSL session could lose its completion list after typing a command prefix, while Tab and Enter were delayed or ineffective. The private editor query and background candidate refresh had separate defects.

## Evidence

The existing real WSL acceptance failed with an advertised Bash editor, no query reply and WIN32_INPUT_MODE. The console's ordinary VT function-key conversion does not preserve physical F24 as the reserved POSIX query sequence. Sending the agreed bytes directly restored the seven existing native scenarios.

An added c-prefix scenario still failed: the input remained c, the cursor anchor and popup mode were unchanged, but the list became empty while another calculation was active. A directory-cache generation update re-entered begin_completion_query, which cleared already usable results before the asynchronous replacement arrived.

## Decision

- POSIX editor queries use the advertised private byte sequence. PowerShell retains its native modifier chord. Do not change ordinary user-key encoding.
- The UI-owned pending calculation retains a query context: environment, directory, input, caret, presentation mode, syntax and editor revision. Allocate it only when a new calculation is required, not on cache-hit paints.
- A source-generation refresh for the same context keeps the visible candidates. A changed input or context still clears them immediately.
- Match the selected candidate by value when refreshed results arrive; an index alone can select a different command after reordering.
- Usable command candidates resolve the list action without waiting for a supplementary remote directory listing. Empty results still follow the existing pending-source behavior.
- Keep cancellation, exact query/environment checks and revision rejection for stale asynchronous results.

## Rejected alternatives

- Increasing the private-query timeout: it does not make an unsupported function-key translation deliver a reply and prolongs input interception.
- Disabling WSL completion or dropping native editor verification: removes the requested feature or risks editing a guessed caret position.
- Keeping every old popup: would preserve candidates from a different directory, connection or input.
- Adding polling threads or synchronous directory scans: unnecessary and contrary to the input-latency boundary.

## Consequences

One owned input context is retained for the pending/latest calculation. Existing results can remain visible during same-input source refresh, rather than flashing empty. No new watcher, background polling loop, process launch or cache lifetime is introduced.

## Validation

Seven focused editor state tests pass, including Win32-mode POSIX query bytes, unchanged PowerShell chord encoding, same-input directory generation refresh, and changed-input invalidation. Nine real WSL scenarios pass: popup/hybrid c-prefix menus remain available through 1.5 seconds of rendering and accept into the real Bash buffer; the prior three-mode immediate Tab, middle Unicode edit and Escape cases remain covered. Tests use an isolated desktop and shell-history file; no keys are injected into the user's existing pane.

## Supersedes

None.

## Revisit when

The shell query protocol gains an explicit out-of-band transport, or candidate ownership requires additional execution identity beyond the existing editor revision and environment.
