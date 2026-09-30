# Completion modes own their keyboard interaction

## Status

Implemented; native desktop acceptance remains separate from automated tests.

## Context

The requested choices are Inline (Tab accepts ghost text), List (Tab inserts the
selected candidate), and Hybrid (Right accepts ghost text; Tab opens a list).
An independently editable accept key can contradict each mode's description.
The application has GPUI and explicitly selected legacy presentation adapters.

## Evidence

- `nebula_settings/src/lib.rs`: `CompletionStyleName` is the persisted mode.
- `nebula_app/src/display/state.rs`: `NebulaPaneState` owns completion candidates,
  selection and the lifetime of a manually requested list.
- `nebula_app/src/gpui_shell/terminal/view/completion.rs`: asynchronous results
  are checked against the query, environment and current mode before application.
- `nebula_app/src/input/keyboard.rs`: the legacy input adapter previously used
  the independent accept-key setting and always cycled on Tab in a popup.

## Decision

Keep one three-value mode in the settings crate and reuse it in both adapters.
The existing `completion_style` key accepts `inline`, `popup` and `hybrid`;
existing aliases and the Inline default remain valid. Preserve the old `accept`
key when reading or saving configuration, but hide its control and let the mode
determine completion interaction. Reset returns the mode to Inline.

Keep candidate generation in the existing engine. Hybrid normally requests
inline candidates; Tab changes the effective presentation to a list. The shared
pane state owns opening, navigation, acceptance and dismissal. Typing refines an
opened list. Acceptance, cancellation, prompt termination and settings changes
end its lifetime. Cancelling a manually opened list restores inline suggestions.

GPUI retains ownership of its background task. Cancellation invalidates its query;
settings changes also invalidate the mode. Stale work cannot reopen the list.
Plain terminal input is required: IME composition, modifiers and alternate-screen
or vi-mode input do not pass through the completion handler.

## Rejected alternatives

- A separate accept-key picker: it exposes combinations that contradict the
  three user-approved modes and makes settings descriptions unreliable.
- A second completion engine for Hybrid: only presentation changes; duplicating
  candidate generation would diverge in history, local, WSL and SSH handling.
- Duplicated popup state transitions per renderer: cancellation and selection
  must have the same behavior across the existing presentation adapters.

## Consequences

Users who previously set `accept=right` use the chosen mode's keys after upgrading.
Their saved value is retained for compatibility with older versions. No new
dependency, configuration file, thread or background service is introduced.
These modes change interaction, not the semantic depth of generated suggestions.

## Validation

Existing settings tests cover mode round trips, aliases, invalid values and reset.
GPUI startup tests cover file/history completion, direct Tab insertion, Hybrid
opening, filtering, cancellation, settings changes, IME and alternate-screen input.
The existing i18n and architecture checks validate shared contracts separately.
Automated tests do not assert real desktop appearance or native shell acceptance.

## Supersedes

None.

## Revisit when

A supported interaction needs independent key customization. Define how its
description, persistence and migration relate to the three modes before exposing
another control.
