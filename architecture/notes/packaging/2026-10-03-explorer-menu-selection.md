# Preserve Explorer menu choices across installer runs

## Status

Implemented; pending review and remote validation.

## Context

Issue [#218](https://github.com/Kuddev/pebrel/issues/218) requests individual
Explorer entries and a master switch, available during installation or in
settings. The current installer owns both the ordinary entry and the existing
WSL cascade; it registers every discovered distribution on each installation.

## Evidence

[`installer.iss`](../../../scripts/installer.iss) previously wrote the ordinary
entry unconditionally. [`installer-migration.iss`](../../../scripts/installer-migration.iss)
already discovers distributions through Lxss, excludes Docker plumbing, and
protects WSL registration with executable ownership checks. The application
does not have an Explorer-registration settings adapter.

## Decision

Use one native installer checklist for the master switch, ordinary entry and
discovered WSL distributions. Re-running the installer edits the selection.
Keep the existing WSL cascade and commands. Persist DWORD choices under
`HKCU\Software\Pebrel\ExplorerMenu`, independently of application preferences.
The ordinary entry continues to launch the configured default shell.

`Enabled` controls integration, `Default` controls the ordinary entry, and
`wsl:<distribution name>` records each distribution independently of enumeration
order. Absent choices inherit the existing all-enabled behavior until `Saved`
exists. Subsequently new distributions default to unchecked. Retain absent
distribution values so reinstalling a distribution restores its choice. Silent
upgrades read the same values. Turning the master off preserves individual values.

Remove unconditional ordinary registry writes. Reconcile both Explorer roots
using exact ordinary command ownership and the existing WSL owner validation.
Keep foreign commands and unknown child subtrees; conflicting registration fails
visibly. Save the selection after the registrations succeed.

## Rejected alternatives

- A second registry adapter in the application duplicates installer ownership
  and requires settings/startup coordination without being necessary for the
  issue's installation option.
- Index-based preference names would attach choices to a different distribution
  after enumeration changes.
- A new menu hierarchy is unnecessary: the existing WSL cascade is retained.
- Unconditionally writing menu keys during upgrades loses the user's selection.

## Consequences

These choices apply to the Inno installation channel, and take effect when the
installer completes. Changing them requires re-running the installer. This does
not add individual local-shell launch verbs, nor MSIX/Scoop integration.
Preferences survive uninstall, matching the retained user-configuration policy.
Legacy Inno uninstall logs may still contain the former ordinary entry's
`uninsdeletekey` action; this change does not rewrite historical uninstall logs.

## Validation

The maintained Inno fixture uses only `HKCU\Software\PebrelTestFixtures`.
It covers both cwd arguments, add/remove, master disable/restore, persistence,
enumeration changes, new distributions and foreign/edited keys. The Windows
input fixture drives the production checklist with mouse and keyboard events,
checks registered results and captures its states. Remote execution and visual
review are required; fixture compilation is not Explorer launch acceptance.

## Supersedes

The all-distributions registration policy in
[`2026-09-23-wsl-context-menu-cascade.md`](2026-09-23-wsl-context-menu-cascade.md).
The cascade layout and ownership policy remain.

## Revisit when

Users need to change integration without running the installer, or another
installation channel requires the same registration ownership.
