# Host icons follow the system a server reports

## Status

Accepted by the maintainer on 2026-11-06.

## Context

A saved host without a chosen icon was meant to follow its server's system, but
nothing recorded that system, so the generic terminal icon stayed.

## Evidence

`detected_os` tests cover distribution and family mapping and the stored map. The
desktop flow was not exercised against live servers in this change.

## Decision

After an SSH shell is ready, one exec channel on the authenticated connection reads
`uname -s` and `/etc/os-release`. The mapped catalog id is stored in
`ssh_detected_os.json` beside the profiles, not in `ssh_profiles.json`, so a
background connection never conflicts with an open host editor. `icons()` uses it
only for hosts whose icon is unset or `auto`; a chosen icon is never replaced.
Android applies the same mapping and replaces only the default or a system icon,
never a role icon such as container or rack.

## Rejected alternatives

- Write the detected id into the profile: it races the editor's
  compare-and-swap save and turns a hint into a user choice.

## Consequences

Views that cache icons show a newly detected system the next time they load
profiles.

## Validation

Unit tests on both platforms.

## Supersedes

None.

## Revisit when

Hosts need per-connection icons, for example one address reaching several systems.
