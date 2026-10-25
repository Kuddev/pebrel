# Ordered terminal effect sources

## Status

Implemented. Settings/compiler contracts, native multi-file pixel comparisons,
list/activation controls, file selection and live-edit reload have evidence.
Foreground performance and other native backends remain separate scopes.

## Context

Independent WGSL effects need an editable execution order without merging their
source text. The existing line-based preferences already store a single source
in `terminal_effect_path`; selected sources must not implicitly start execution.

## Evidence

[`RawSettings`](../../../../nebula_settings/src/lib.rs) trims values and treats an
empty value as absent. The terminal compiler produces an ordered list of native
passes, while the renderer limits a chain to eight passes and two intermediate
textures. Each file can declare more than one fragment entry point.

## Decision

- Keep the first persisted key and add numbered slots 2 through 8. Read occupied
  slots in numerical order; write all slots together to clear removed entries.
- Own serialization, path validation and the slot limit in `TerminalEffects`.
  Reject paths that cannot round-trip through the existing line-based format.
- Compile each source separately and concatenate native passes in list order.
  Identical entry names in different files do not collide. Enforce eight passes
  across the whole chain; a failed later source rejects the entire new chain.
- Add files through the existing picker and require explicit re-enabling after
  addition. Reordering/removal preserves activation of remaining sources;
  removing the last source disables it. Cancellation, including an empty picker
  result, leaves the current list and activation unchanged.
- Keep ordering and removal in the settings adapter, guarded by its captured
  source list. Show persistence failures without publishing an in-memory list
  that was not written. Reload reuses the existing revision/cancellation path.

## Rejected alternatives

- Join filenames with a delimiter: ordinary filenames could be misparsed.
- Concatenate WGSL text: independently valid files may reuse global/entry names.
- Publish successfully compiled prefixes: a failed later file would silently
  change the meaning of an explicitly ordered effect chain.
- Allocate intermediate textures per file: ordering does not require extra
  surfaces beyond the renderer's existing ping-pong pair.

## Consequences

Older single-source preferences remain valid. An older application sees only the
first source; this is not a promise of backwards multi-file support. The reset
registry removes all eight slots. Disk reads/compilation remain background work,
and the native renderer receives the same bounded pass descriptor as before.

## Validation

The shared settings suite covers ordered round-trip, deletion/stale-slot clearing,
activation independence, invalid paths, limits and reset. Compiler tests cover
same-name independent sources, reversed order, a failed/missing later file and
the aggregate pass limit. Product compilation, native window pixels and settings
control interaction are distinct checks; unit results do not establish them.

The Windows product build was run in four private-window scenarios: disabled,
paths without activation, invert then halve-red, and the reverse file order.
The inspected terminal text/background pixels match the corresponding ordered
transforms within 0.5 of an 8-bit channel value. Inspected sidebar, titlebar and
terminal padding are unchanged. Both source files deliberately use the same
entry name. Commands complete normally in every run; these are not foreground
performance measurements or real settings-control interaction tests.

Subsequent native settings runs exercise search, reorder, removal, explicit
confirmation, disable and removal of the final source. Persisted source slots and
activation are checked after real clicks. Returning to the terminal shows the
expected transformed pixels after confirmation and original pixels after disable
or removing all sources. File selection and reload of changed source contents
are not covered by these runs. Native layout inspection also caught source names
being squeezed into the select-control column; the list now uses content width.

Separate native file-dialog runs select two files containing Chinese characters
and a space in their filenames. The files are appended in the returned order,
existing paths are retained, and activation is revoked. Cancelling the native
dialog leaves settings bytes unchanged. A same-path source edit keeps the old
program until explicit reload; terminal pixels then match the changed program.
Invalid-source reload keeps original rendering, and restoring/reloading that file
recovers the effect without changing settings. Resize checks observe the expected
pixels at smaller, larger and restored surface extents.

Rendered GPUI control tests cover keyboard selection, explicit confirmation and
cancellation, restored focus, reload and removing the final source. Cancelled or
empty picker responses preserve enabled settings; delayed picker results and
confirmation of a replaced source do not overwrite/authorize newer preferences.
These tests use the existing shared-settings fixture and serial group. The
component's declared dialog transition duration is advanced on the deterministic
test clock before checking restored keyboard focus. App-native unit tests also
check frame integer flags/cursor history and the shader search aliases.

## Supersedes

Extends the single-source persistence described in
[`terminal WGSL effects`](../../nebula_app/gpui_shell/terminal/effects/2026-10-06-wgsl-terminal-effects.md).
The frame ABI, ownership and background independence are unchanged.

## Revisit when

A demonstrated workflow needs more stages, another persisted source type or
portable relative paths that require a separately reviewed format migration.
