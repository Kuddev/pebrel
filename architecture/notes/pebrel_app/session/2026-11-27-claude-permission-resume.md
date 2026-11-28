# Preserve the observed Claude resume mode

## Status
Implemented for 2.6.2; release validation pending.

## Context
Issue #1108 reports that an explicitly selected bypass mode disappears after restart.

## Evidence
Claude hooks report permission_mode, but AgentSession dropped it. Claude's session documentation says terminal resumes do not automatically restore bypassPermissions.
https://code.claude.com/docs/en/sessions#permission-mode-on-resume

## Decision
Save the provider-reported mode with the pane's existing session identity. Missing fields in later hooks retain the prior mode only for the same source and session. Resume appends only a fixed whitelist of Claude modes; other agents ignore this field. Old snapshots omit it and preserve their existing command.

## Rejected alternatives
Saving raw argv loses in-session mode changes and persists unrelated arguments. Enabling bypass by default changes the user's selected mode.

## Consequences
The optional field is compatible with existing snapshots. Without a provider report the previous behavior remains. It does not infer permission from attention state.

## Validation
The existing GPUI startup fixture exercises hook → snapshot serialization → resume for bypass, absent fields, plan and default. Release-native tests cover the final candidate.

## Supersedes
None.

## Revisit when
Claude changes the permission-mode resume contract.
