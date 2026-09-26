# Rendering resources follow visibility and completion

## Status

Implemented locally; pending review. This records resource ownership and the
bounded release path, not a whole-process memory guarantee.

## Context

Virtual lists bound mounted rows, but decoded images, native input entities,
background results and window atlas entries have separate lifetimes. Leaving a
document tab must release its presentation resources without losing a draft,
undo history, scroll position or caret.

Scientific rendering is shared with terminal views and other windows. A document
cannot clear that global cache as though it were the only consumer.

## Evidence

- [Document activity](../../../../nebula_app/src/gpui_shell/file_editor/activity.rs)
  separates the persistent editing session from visible presentation resources.
- [Image loading](../../../../nebula_app/src/gpui_shell/file_editor/image_cache.rs)
  has a document owner, bounded completed results, cancellable tasks and a
  generation check that rejects results from an earlier active period.
- GPUI's `App::drop_image` accepts the current window explicitly because an
  updating window is temporarily absent from the application's window table.
- The earlier scientific cache dropped CPU image references on eviction without
  removing the corresponding atlas entries. The regression
  `retirement_callback_removes_real_window_atlas_entries` checks a rendered image
  through the window's atlas query before and after the production callback.

## Decision

The workspace synchronizes document activity centrally. Inactive documents stop
pending preview work and selection scrolling, drop native preview/input entities
and release document images. Source text, structural metadata, edit history and
a lightweight caret description remain available for resumption.

Image release with a current window supplies that window to GPUI. Owner-release
callbacks without the window defer cleanup until the window is back in the app.

The existing [render cache](../../../../nebula_app/src/render_cache.rs) remains
the sole LRU/accounting implementation. Budget reduction and insertion notify
resource owners when they evict values.

The [scientific renderer](../../../../nebula_app/src/gpui_shell/scientific_render.rs)
holds evicted images until its existing application-task callback releases their
atlas entries. Retired pixels remain charged while the callback runs, including
when an already-running worker finishes concurrently. New workers wait until all
pending releases complete. An unreserved result that cannot coexist with retired
pixels is discarded and can be requested after cleanup. No additional thread or
unbounded retirement channel is introduced.

## Rejected alternatives

- Dropping an `Arc` alone does not remove an independently owned atlas entry.
- Moving evicted images to an unrestricted queue merely relocates their memory.
- Clearing the shared scientific cache whenever one document hides ignores its
  other consumers; visibility does not justify discarding their resources.
- Clearing undo on inactivity would destroy part of the editing session.

## Consequences

Resuming a document may reload images and reconstruct visible input/layout
entities. Under cache pressure an unreserved scientific result may be rebuilt.
The cache budget does not bound decoder intermediates, layout/compiler working
memory, external references, font resources, GPU page fragmentation or the entire
process. Atlas removal also does not promise immediate driver memory reclamation.

## Validation

Native GPUI regressions cover draft/undo/caret/scroll preservation, cancelled image
work, real atlas removal and reservations across concurrent release batches.
Render-cache tests cover owner notifications during budget reduction, replacement
and eviction. Native scrolling and process/GPU measurements remain separate from
these behavior tests; editing pressure requires verified input coverage.

## Supersedes

None.

## Revisit when

Scientific resources gain explicit consumer scopes, GPUI owns automatic atlas
eviction, or profiling identifies a different dominant retained allocation.
Reassess the budgets using representative native measurements rather than treating
cache byte limits as process-memory limits.
