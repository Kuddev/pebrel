# Refresh managed PowerShell bootstrap references at launch

## Status

Implemented for new process launches. Live-shell input behavior remains a separate acceptance item.

## Context

Session launch identities retain the resolved shell executable and arguments.
For integrated PowerShell, those arguments include a generated temporary script.
Restoring or copying that identity can retain a previous temporary directory and
an older script even while a newly created tab receives the current integration.

## Evidence

Native process command lines and saved session arguments identified two different
managed bootstrap locations. The older script lacked the editor-query binding
and capability report; the current script included both. An isolated PowerShell
process reported the capability absent after loading the older script and present
after loading the current script. This proves the integration mismatch, not every
reported unsolicited input change.

`TerminalView` freezes `snapshot_shell` into `LaunchSession::Shell`; the Windows
`cmdline` adapter previously passed explicit saved arguments through unchanged.

## Decision

At the Windows launch boundary, recognize only the generated PowerShell argument
suffix, supported flag-only prefixes, and the reserved bootstrap name inside
known managed directories. Resolve directory aliases before comparison. Generate
the current integration and replace the launch suffix, preserving executable and
original flags. Do not overwrite the location from the saved session.

Store the generated PowerShell script below `pebrel-shell/<version>-<sha256>/`.
The version is the terminal crate version; SHA-256 covers the embedded UTF-8
script source. Equal-version test builds with different scripts therefore do
not overwrite each other's file. Cache this generation once per process, not
per tab or frame. Existing flat references and previous generated directories
are both recognized; an old generation need not still exist to refresh it.

Default-shell preparation is lazy when an explicit shell is present, avoiding an
unused bootstrap write. Custom commands, other shells, arbitrary same-named files
and UNC references retain their existing arguments.

## Rejected alternatives

- Match the filename alone: risks replacing a custom user script.
- Write current contents to the saved path: gives persisted data write authority.
- Replace the whole launch with the current default shell: loses explicit identity.
- Restart or source code into running user shells without a separate action: exceeds launch repair.
- Use only the version number: different test builds may share that version.
- Delete prior generation files during launch: another process may still reference them.

## Consequences

Reuse the workspace's existing `sha2` implementation as a Windows-only dependency
instead of inventing a digest or relying on an unstable standard-library hash.
No new thread or persisted format. Work occurs at process launch,
not during input or rendering. Already-running shells retain their loaded code;
changing a script file does not retroactively update their functions.

## Validation

Focused Windows tests cover preserved flags and executable scope, replacement of
a managed stale reference, untouched custom commands, unrelated shells, unrelated
directories and UNC references. The isolated old/current script capability probe
uses a separate noninteractive PowerShell process. Product-level old-tab input
acceptance and packaging remain pending.

## Supersedes

None.

## Revisit when

Persisted launch identities gain an explicit bootstrap provenance field, or the
managed script storage contract changes.
