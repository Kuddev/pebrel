# Owned file browser acceptance foundation

## Status

Proposed as the first explicit dependency of issue #219.

## Context

A second file browser must own a unique terminal source, model, inputs, scroll
and asynchronous tasks. Its regression tests also need real file and SFTP
interactions without resolving personal SSH profiles or reading credentials.

## Evidence

The existing SidePanel and path resolver already own local/WSL traversal and
manual directory validation. The document regression peer already implements
the real SFTP publication protocol. Reusing these authorities avoids a separate
browser or upload simulation with different semantics.

## Decision

Provide a LocalFilesDrawer bound to one weak terminal entity, with independent
SidePanel, path/search inputs and scroll. Visibility controls its owned polling
and validation tasks and releases input focus when hidden. Path validation stays
on the background executor and checks its original source/directory identity.

The staged drawer module is compiled only with test and gpui-test-support until
the dependent pairing change imports it in release. Its permanent module path
avoids deleting and recreating the component in that next change. No optional
pairing control or default product layout is introduced by this foundation.

Tests use synthetic terminal sessions and ephemeral loopback SSH/SFTP endpoints.
Fixture authentication asserts none-auth succeeds and cannot use credentials.
The registered sentinel fails closed after removal, rather than entering SSH
profile resolution. Production SFTP channel opening remains shared.

The opt-in native acceptance entry is test-only, before normal startup workers,
and requires an explicit existing artifact directory. It creates its own
configuration, empty SSH config, source files, server keys and known-host files.
It never repurposes HOME or changes ordinary release startup.

## Rejected alternatives

- Copying local traversal or upload protocol behavior into a test renderer.
- A default Auto-auth fixture that could fall back to Keychain after failure.
- Broad release dependency changes solely to build a native acceptance window.

## Consequences

The dependent dual-tree change must remove only the staging module cfg and add
explicit owner selection/layout. It must separately pass ordinary product
compilation. This foundation does not claim the feature or native drag visual
acceptance is complete. The neutral focused local/WSL directory helpers move
to drawer_follow to respect the existing workspace line ratchet.

## Validation

Independent TestWindow controls exercise the actual component. Real owned
loopback controller tests cover listings, exact publication bytes, conflicts,
cancellation and closed sentinel behavior. Native and ordinary product builds
remain separate gates; command outcomes are reported from actual logs.

## Supersedes

None.

## Revisit when

Another production caller needs this component without explicit pane ownership,
or owned acceptance endpoints require more protocol behavior than the shared
document peer provides.
