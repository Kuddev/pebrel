# Mobile preview source baseline

## Status

Accepted for the local integration repair; publication remains separate.

## Context

A locally delivered Android preview contained recent interface improvements while
omitting existing main-branch SSH key authentication, session recovery, shortcuts
and file-transfer work. APK integrity and signature checks had passed; those checks
did not establish feature completeness.

## Evidence

- The withdrawn preview identified local commit `3adbdab5`, descended from the
  standalone source snapshot `d9090cf7`. That history has no merge base with main.
- Main commit `42ffd11f` includes SSH key authentication originating in `5258bbb3`.
  Its `HostForms`, `SshKeyFile`, JNI signature and native authentication path differ
  from the snapshot. This was a source-baseline mismatch, not deletion by the later
  conversation UI patch.
- The interface and reader changes are isolated in `50a12c37`, `566b5326` and
  `3adbdab5`. Current main already observes terminal display preferences; its newer
  keyboard implementation must not be overwritten by the snapshot.

## Decision

Build the repair on the complete main baseline and replay only the reviewed reader
and interface changes. Preserve current session, authentication, transfer, keyboard
and persistence implementations during conflict resolution. Keep the original
checkout and unrelated local edits intact.

Before handing out a preview, record its source commit, base commit, APK identity,
SHA256 and validation results. Compare the source baseline and feature regressions
as well as the archive signature. A timestamp or recent commit name is insufficient.

## Rejected alternatives

- Rebuilding the snapshot unchanged: produces a new APK with the same omissions.
- Copying individual key-login files into the snapshot: misses JNI, credential
  lifetime, transfer and session-recovery dependencies.
- Replacing the whole mobile tree with the snapshot: loses main-branch behavior.
- Replacing the local tree with main wholesale: loses approved interface changes.

## Consequences

The integration retains both sets of changes and remains independently reviewable.
Native build, component tests, device integration and public-download verification
are distinct checks; none can be reported as evidence for another.

## Validation

Use the main ancestor check and inspect the mobile diff against `42ffd11f`. Run the
reader browser suite, `CommandComposerTest` key-authentication cases, session and
transfer regressions, and the preview packaging checks. Device-only SSH tests must
be reported separately when an Android target is actually available.

## Supersedes

None.

## Revisit when

The project adopts a single preview builder that records and checks source ancestry
and feature baselines before producing a distributable artifact.
