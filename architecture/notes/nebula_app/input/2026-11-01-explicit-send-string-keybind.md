# Explicit terminal strings in custom keybindings

## Status

Implemented locally; pending maintainer review.

## Context

Issue #839 reports an unnegotiated byte-reading client receiving plain CR from
ConPTY when the user presses Shift+Enter. It proposes either changing the default
encoding or allowing an explicit string binding. The existing modified-Enter
compatibility boundary preserves native Windows readers and negotiated protocols.

## Evidence

The shared keybind parser accepts unit action names only, although the legacy
input action already supports `Action::Esc(String)`. The GPUI keybinding adapter
also lacks that action. Preferences already preserve each action as the string
after the first colon in a `keybind=` line.

See [the shared model](../../../../nebula_app/src/display/keymap.rs),
[GPUI bindings](../../../../nebula_app/src/gpui_shell/workspace/keyboard_bindings.rs)
and the existing modified-Enter compatibility decision in this directory.

## Decision

Accept `SendString:<JSON string>` as an explicit spelling of the existing escape
action. JSON keeps control characters, quotes, backslashes and Unicode on one
physical settings line, and supplies a single existing parser and serializer.
No new configuration action variant or dependency is needed.

The GPUI adapter scopes the action to a focused terminal. Its literal-text input
path owns PTY writes, prompt-mirror invalidation and optional broadcast delivery.
Strings bypass paste wrapping, newline normalization and key-protocol encoding.
They do not execute in the application or through a local shell subprocess.

Normal keyboard encoding and Runtime API named keys keep their current behavior.
The settings action picker remains limited to its existing fixed actions; this
parameterized action is configured in the preferences file.

## Rejected alternatives

- Replacing Shift+Enter globally with ESC+CR would change native console readers
  and override negotiated keyboard protocols.
- Adding another program-name exception requires client-specific evidence and
  would not provide the requested configurable workaround.
- Routing through clipboard paste would normalize CR/LF and add paste markers.
- Defining a second custom escape grammar would duplicate JSON's validation.

## Consequences

Existing action names and later-line precedence remain compatible. Invalid JSON
is ignored like other malformed bindings. An empty string consumes the key.
Explicit bindings may intentionally send control characters; only focused,
usable terminal panes handle them, not settings inputs or answer readers.
No thread, transport, dependency or default binding changes.

## Validation

Parser regressions cover exact control/Unicode round trips through preferences
and reject malformed or non-string JSON. GPUI regressions exercise real key
dispatch to the terminal receiver, mode independence, repeated presses, focused
inputs, empty strings and removal. Native Windows/CodeBuddy acceptance is not
established by the Linux tests.

## Supersedes

None. The existing modified-Enter compatibility decision still applies to
ordinary keys.

## Revisit when

The keybinding editor supports parameterized actions. Reuse this grammar and
input path when exposing a string editor.
