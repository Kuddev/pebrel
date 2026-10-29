# Independent native prompt prediction settings

## Status

Implemented settings contract; native UI integration awaits product compilation.
Native adapter availability is independent of this UI.

## Context

Users need one prediction provider/model across supported AI CLI editors without
changing the model used for the CLI's main conversation. Existing shell completion
preferences do not authorize paid next-message prediction requests.

## Evidence

- `nebula_settings/src/prompt_suggestions.rs` owns opt-in, validation and key updates.
- `nebula_app/src/gpui_shell/settings_pane/providers/predictions.rs` edits a separate
  draft and reuses existing provider metadata without storing another credential.
- Provider compatibility is evaluated by the production prediction client's
  `supported` function; the picker also requires configured endpoint/key metadata.

## Decision

Persist three non-secret scalar preferences: enabled, existing provider ID and
prediction model ID. Default to disabled. Missing or invalid selections never
enable prediction. The app language remains authoritative, so there is no separate
language preference that can drift from the interface.

The settings UI owns a model input draft. Typing does not write to disk; explicit
Save or enabling commits the validated group through existing settings persistence.
Disabling remains available when the selected provider has been removed. Reset
clears prediction overrides while preserving provider credentials and CLI data.

## Rejected alternatives

- Reusing the CLI's main model field would silently change unrelated AI behavior.
- Copying credentials into prediction settings would introduce another secret store.
- Saving on every keystroke would add unnecessary I/O and repeated reconfiguration.

## Consequences

Provider metadata changes require the application service to replace its prepared
snapshot. The settings module does not read credentials or perform network checks.
The UI does not claim that every CLI has a native prediction adapter.

## Validation

Settings tests cover safe defaults, incomplete/invalid opt-ins, bounded identifiers,
round trips and reset preservation. Provider-picker tests check supported protocols
and configured credential metadata; the translation contract covers typed keys and
matching placeholders. Native UI layout remains a separate acceptance check.

## Supersedes

None.

## Revisit when

Provider identity or settings persistence gains an explicit migration contract.
