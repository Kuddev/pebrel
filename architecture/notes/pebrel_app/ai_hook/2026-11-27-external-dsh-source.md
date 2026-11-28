# External DSH hook source

## Status
Implemented for 2.6.2; release validation pending.

## Context
Issue #1105 requests lifecycle and complete assistant answers from a DSH plugin using Claude-style JSON.

## Evidence
The source was absent from the helper stdin whitelist, protocol classifier, answer extractor and answer inbox. DSH CLI identity was separately implemented on main.

## Decision
Reuse the existing helper transport and pane/session ownership checks. DSH is opt-in through ai_hooks_dsh. Its external plugin owns installation; Pebrel does not create or heal a fictitious DSH hooks file. Stop answers retain the existing 128 KiB complete-answer boundary.

## Rejected alternatives
A new generic provider framework adds no required capability. Treating an external plugin as an installed settings file would incorrectly claim ownership of user configuration.

## Consequences
The plugin must call pebrel-hook dsh with JSON on stdin in the originating pane. Existing Agents and transport bounds remain in force.

## Validation
Existing answer tests include DSH UTF-8, oversized responses, sequence ordering and cross-session rejection. Helper source tests include stdin draining.

## Supersedes
None.

## Revisit when
DSH publishes a native hook installation contract.
