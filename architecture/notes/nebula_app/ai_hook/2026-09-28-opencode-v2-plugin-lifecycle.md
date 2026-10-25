# OpenCode V2 notification lifecycle

## Status

Proposed in PR #298; maintainer review pending.

## Context

The generated OpenCode hook must keep the existing V1 server entrypoint while
supporting V2's default plugin definition and async event subscription. Both paths
must feed the same Pebrel notification reducer and helper protocol. A V2 plugin
may be unloaded while its event stream or helper delivery is still pending.

## Evidence

- OpenCode v2.0.18 validates a default plugin definition with an `id` and an
  `effect` or `setup` entrypoint: [loader](https://github.com/anomalyco/opencode/blob/v2.0.18/packages/core/src/plugin/module.ts).
- V2's Promise plugin event domain reuses the generated client Event API. In
  v2.0.18, `event.subscribe` takes one optional request-options argument; it has
  no query argument: [plugin event type](https://github.com/anomalyco/opencode/blob/v2.0.18/packages/plugin/src/promise/event.ts),
  [generated event client](https://github.com/anomalyco/opencode/blob/v2.0.18/packages/client/src/promise/generated/client.ts#L1660).
- The bridge's regression suite exercises the installed ESM export, V1 and V2
  event mapping, cleanup, and helper argument delivery.

## Decision

Keep one default object with `id`, the V1 `server` entrypoint, and the V2 `setup`
entrypoint. `setup` subscribes with
`ctx.event.subscribe({ signal: controller.signal })`, translates only the required
V2 lifecycle events, and forwards them through the existing reducer. Its cleanup
aborts the subscription and helper process and waits for queued delivery to end.

## Rejected alternatives

- Separate V1 and V2 hook files would duplicate lifecycle mapping and risk two
  active bridge instances.
- Passing a query and request options separately does not match the pinned V2
  event API; other generated client operations use that two-argument shape, but
  `event.subscribe` does not.
- Replacing the reducer with V2-specific notification state would create a
  second authority for session lifecycle and ordering.

## Consequences

V1 and V2 remain adapters over the same notification payload and process helper.
V2 cleanup owns both the stream and any pending helper work. When OpenCode changes
its plugin export or event schema, the generated hook and focused bridge workflow
must be checked against that release before widening the supported version range.

## Validation

The Node/Bun workflow runs the focused bridge suite on Linux, Windows, and macOS.
The suite covers the installed module and cleanup contract; it does not claim a
full OpenCode UI or SSH end-to-end run.

## Supersedes

None.

## Revisit when

Revisit if V1 support is dropped, the pinned V2 event API changes, or the Pebrel
helper protocol no longer owns notification delivery and ordering.
