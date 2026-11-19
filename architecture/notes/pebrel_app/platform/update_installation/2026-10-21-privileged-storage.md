# Administrator update state has a separate Windows trust boundary

## Status

Implemented for review. Native directory and installer-helper fixtures have been
run with both ordinary and elevated tokens; packaged product acceptance is separate.

## Context

The updater previously rejected elevated windows. Simply removing that check would
let an elevated PowerShell helper execute scripts and installation plans from the
ordinary settings directory. A checksum stored in that same writable plan cannot
authenticate its own expected value.

## Evidence

- `update_download::state_directory` is the common path boundary for downloads,
  cached metadata, installation transactions and scheduled snapshots.
- [SECURITY_INFORMATION](https://learn.microsoft.com/en-us/windows/win32/secauthz/security-information)
  distinguishes label access (`READ_CONTROL`/`WRITE_OWNER`) from full/protected
  SACL access (`ACCESS_SYSTEM_SECURITY`).
- [SetSecurityInfo](https://learn.microsoft.com/en-us/windows/win32/api/aclapi/nf-aclapi-setsecurityinfo)
  accepts `LABEL_SECURITY_INFORMATION` separately from auditing SACL information.
- The first elevated directory fixture returned Win32 1314 when creation supplied
  a full/protected SACL. Applying only the mandatory label after restricted DACL
  creation passed actual creation, readback and child-file inheritance checks.
- [DeleteFileW](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-deletefilew)
  documents both object DELETE and parent DELETE_CHILD as deletion/rename authority.

## Decision

Choose storage from the actual token; a failed token query is an error. Keep ordinary
updates under the existing settings directory. Administrator updates use the system
ProgramData known folder and an administrator/SYSTEM-only root. Partition it using
the token SID and unambiguously encoded canonical executable/configuration paths.
The hash identifies a scope; Windows permissions establish authority.

Create each application-managed directory with a protected administrator/SYSTEM
DACL and administrator owner. Pin the actual directory, verify that owner and DACL,
then set an inheritable high mandatory label denying lower-integrity reads/writes
through the label-only API. Ordinary processes never gain access during this step.
Read back the complete required policy. Reject pre-existing untrusted objects;
do not repair their ACLs and then trust their old contents.

Require a local volume with persistent ACLs. Pin canonical ancestors and the root
with real directory-read access and no delete sharing, reject reparse points, and
retain root pins for the process lifetime. Before acknowledging readiness, the
helper acquires its own ancestor pins while the caller still holds its pins.

Resolve PowerShell through GetSystemDirectoryW. Materialize its script inside the
protected transaction, and set helper TEMP/TMP there before launch. Downloads,
metadata and snapshots never migrate from ordinary writable caches into privileged
authority. Existing byte/size/hash, participant identity and commit checks remain.

## Rejected alternatives

- Removing only the elevation guard: leaves scripts, plans and recovery data writable
  by ordinary processes.
- Trusting the cached checksum alone: the writable record could replace both payload
  and expected checksum.
- Enabling the auditing privilege solely to create a protected SACL: unnecessary for
  a mandatory label and failed on a real elevated token without that privilege enabled.
- Relying only on the final directory DACL: ignores ancestor replacement and parent
  delete-child rights during the active transaction.

## Consequences

Administrator windows have their own update cache and may need to download a package
again. Other administrators and SYSTEM remain trusted by this policy. Unsupported
filesystems or pre-created untrusted directories produce visible preparation errors.
Ordinary installation paths, distribution ownership and released artifacts are unchanged.

## Validation

The native fixtures check policy rejection, real rename exclusion, ordinary-token
creation rejection, elevated creation/readback and inherited file labels. The existing
PowerShell harness passes 18 scenarios under each token, including privilege mismatch,
cancel, active processes, failed installers, repair and retained helper locks.
These fixtures do not claim real Inno installation or full application recovery.

## Supersedes

None. Extends the Windows installer adapter while retaining ordinary-session isolation.

## Revisit when

The installer becomes a service, packages gain a different authenticated distribution
channel, or Windows changes the supported integrity/known-folder contracts.
