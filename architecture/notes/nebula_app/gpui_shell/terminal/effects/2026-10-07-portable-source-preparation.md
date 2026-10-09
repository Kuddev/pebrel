# Portable source preparation for terminal effects

## Status

Application migration implemented; complete product validation pending.
This step retains the current platform availability and does not claim that
terminal controllers outside Windows or Metal postprocessing are enabled.

## Context

The terminal controller previously received Windows bytecode from its application
compiler. The qualified WGPU entry instead consumes WGSL modules and ordered
entry names. Keeping target compilation in the view's application module would
require a second preparation flow when connecting that backend.

## Evidence

The renderer's Windows backend now implements the same portable source factory
and retains its old bytecode interface. Complete renderer source checks and real
Windows shader compilation/source-reuse tests passed at renderer revision
`d34e803beb6bfdf77451f052fae84dca9e606755` in
[run 37567979636](https://github.com/Kuddev/zed/actions/runs/37567979636).
Earlier physical Vulkan executor and scene tests are recorded with their exact
revisions in the renderer's postprocessing note, not relabeled as product tests.

## Decision

- Keep file limits, explicit enable/reload behavior, file ordering, total pass
  limits, product binding names, and the existing frame ABI.
- Validate the shared fragment/frame/surface contract through the renderer's
  optional WGSL interface. The product still owns its source files and binding
  names; the common validator is the authority for the transport ABI.
- Keep one shared source allocation per file and retain its entry order in the
  program. Surface resize clones the source references rather than recompiling
  target bytecode in the controller.
- Prepare through `prepare_postprocess_wgsl`. Native compilation, device-specific
  programs, cancellation, adoption, and retirement stay in the backend.
- Pin the renderer and all component renderer references to the same exact
  revision. Validate the consumer's top-level lockfile with normal locked CI.

## Rejected alternatives

- Treating source text as compiled bytecode.
- Branching the terminal view into separate compiler/adoption lifecycles.
- Duplicating the fragment ABI validator in each platform or application module.
- Enabling an unqualified platform simply because it can parse WGSL.

## Consequences

The application no longer performs target-specific native compilation for
terminal effects. Existing settings, per-pane state, GPU budgets, frame encoding,
and error/reload behavior remain in their current owners. Background shader
compilation is separate and is not silently migrated by this change.

## Validation

Existing compiler tests now check portable source/entry ordering and shared source
identity; native bytecode compilation/reuse is checked in the backend. Product
architecture, platform-conditional budget, formatting, and naming checks are run
locally. Full native product CI and real product use remain separate evidence.

## Supersedes

None. Refines the target-compilation boundary without changing the effect ABI.

## Revisit when

Non-Windows window activity and backend capability adapters connect the same
controller, retaining this source and ownership contract.
