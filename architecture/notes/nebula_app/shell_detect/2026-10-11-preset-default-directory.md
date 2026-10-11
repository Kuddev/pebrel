# Saved shell presets and the WSL default directory

## Status

Proposed.

## Context

Issue [#198](https://github.com/Kuddev/pebrel/issues/198) requests editable arguments based on detected PowerShell and WSL recipes, and guest home when no starting directory is configured. The local profile editor already owns argument editing, stable ids and compare-and-swap persistence. Its saved recipes must stay independent from installed-shell discovery.

A host working-directory fallback cannot identify the home directory of a selected WSL distribution and user. Representing guest home as a host `PathBuf` would mix two path domains and can silently select a directory from the host.

## Evidence

- Microsoft [WSL basic commands](https://learn.microsoft.com/en-us/windows/wsl/basic-commands#change-directory-to-home) documents guest home through `wsl ~`; the [distribution/user command](https://learn.microsoft.com/en-us/windows/wsl/basic-commands#run-a-specific-linux-distribution-from-powershell-or-cmd) selects the distribution and existing user. Checked 2026-10-11; document updated 2025-12-01.
- Microsoft's [WslClient.cpp directory parser](https://github.com/microsoft/WSL/blob/master/src/windows/common/WslClient.cpp#L208-L225) treats `~` and `/` as Linux directory inputs. Its [distribution and directory options](https://github.com/microsoft/WSL/blob/master/src/windows/common/WslClient.cpp#L1590-L1615), [user option](https://github.com/microsoft/WSL/blob/master/src/windows/common/WslClient.cpp#L1631), and [launch call](https://github.com/microsoft/WSL/blob/master/src/windows/common/WslClient.cpp#L696-L697) carry these selections separately. Checked 2026-10-11; file blob `48b5c4fe89337f2b30897e8cdee52f865f6f354d`.
- [#351](https://github.com/Kuddev/pebrel/pull/351) established `shell_detect::wsl_launch`, own-option boundaries, guest cwd injection and spawn-time distribution snapshots. [#409](https://github.com/Kuddev/pebrel/pull/409) concerns guest zsh reporting, rather than recipe editing or default startup location.
- `shell_detect::tests::wsl_default_home_retains_distribution_user_and_guest_command_arguments` and `platform::shell::tests::wsl_startup_options_keep_guest_home_separate_from_explicit_host_directory` specify argv and path-domain boundaries using synthetic Windows recipes.

## Decision

Copy the detected shell's raw recipe into a new draft in the existing editor. Give each saved copy its own profile id, retain the detected semantic shell id, and persist edits through the existing profile store. Discovery runs in the editor's background task and does not mutate installed presets.

Use `wsl_args_with_default_home` to add `--cd ~` only when neither an explicit host startup directory nor WSL's own directory option exists. Reuse the existing WSL option parser and host/guest directory transformations. Distribution, user and arguments after the guest command boundary remain untouched; WSL resolves the selected user's home.

Encode the default through ordinary WSL argv. Saved profile JSON retains the raw editable recipe. Durable launch snapshots contain the resolved argv; cold profile reload re-applies the home policy after loading the local recipe. No new persistent marker or host directory value is required.

At GPUI and legacy new-launch callers, preserve whether a directory is explicit or a focused/process fallback. An own WSL directory suppresses the latter. Explicit host requests are passed through the existing own-directory removal before PTY spawn. Both adapters use the platform PTY preparation boundary.

## Rejected alternatives

- A separate PowerShell/WSL editor or preset store would introduce a second authority for validation, ids and stale edits.
- Host HOME, a guessed `/home/<user>`, or the current Mac directory cannot determine a WSL account's configured home.
- A new profile marker would require migration and adapter-specific interpretation, despite WSL already providing a directory argument.
- Replacing WSL's guest default shell with `--exec bash` would violate the existing guest shell contract.

## Consequences

Editable recipes remain portable and preserve the same executable/arguments model used by local profiles. Directory policy is visible in durable launch argv while the editor keeps the original editable recipe. Host defaults remain host defaults for ordinary programs.

The launch-layer dependency conservatively retains a snapshot's own WSL directory only for explicitly known matching distribution and user; unknown or changed identities use the local recipe. The home policy is reapplied after that resolution. Running snapshots retain their frozen arguments.

## Validation

New synthetic argv, profile persistence, GPUI control and launch-mapping regressions accompany the change. Native Windows discovery, WSL account/home resolution and PTY execution require Windows acceptance; Mac synthetic tests cannot establish those behaviors. Cargo and product compilation are recorded separately by the serial verifier.

## Supersedes

None. Extends the unset-directory policy using the WSL authority from #351 and the local profile editor.

## Revisit when

WSL changes its directory argument interpretation, the profile schema gains explicit guest identities, or the application adds a unified explicit/fallback directory request type across both adapters.
