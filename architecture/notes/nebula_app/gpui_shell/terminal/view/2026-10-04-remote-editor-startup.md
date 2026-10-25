# Advertise native editing from every integrated startup route

## Status

Implemented. Route acceptance evidence accompanies the review.

## Context

Completion accepts authoritative editor snapshots only after the owning shell
advertises the query binding. Installing a widget without its readiness report
leaves the view on native fallback. Backend metadata and remote command tests do
not exercise that handshake.

An audit found that the default WSL Bash prompt and external SSH Bash/zsh prompts
installed the adapter without advertising readiness. The authenticated SSH hook
installer also composed its shell assets from raw rc files without the shared
connection/editor adapter. Local shell acceptance did not reveal these routes.

## Evidence

Three real PTY regressions executing the actual WSL and external SSH startup
constants failed before the repair because the initial prompt lacked readiness.
After the repair they verify the initial and next prompt, the same snapshot owner,
UTF-8 caret position, a quoted Unicode middle edit and actual command output.

The remote installation regression checks the actual planned Bash/zsh assets.
Existing repeat-install, upgrade, removal and foreign-edit tests retain ownership
protection. The native remote fixture uses the ordinary WSL launch and an actual
authenticated SSH connection, rather than supplying synthetic readiness events.

## Decision

Keep one readiness reporter in the shared terminal shell adapter. Prompt owners
call it after reporting their shell token; it emits only when installation
established the private query binding and a token exists. Local, WSL and external
SSH prompts use that same reporter. The authenticated SSH installer appends the
same connection/editor adapter to its owned shell assets and updates their existing
hash receipts through its normal upgrade policy.

Window acceptance waits for the real readiness event, queries the actual buffer,
accepts through product input/keyboard handlers and paints through the product
renderer. It verifies that acceptance leaves Git HEAD unchanged and that a later
explicit Enter executes the completed command. Probe queries reuse the product's
existing encoding and classic VT fallback, including the private F24 sequence.

## Rejected alternatives

- Infer editor readiness from prompt pixels: a prompt does not prove a binding.
- Treat remote metadata tests as window acceptance: they omit the editor handshake.
- Duplicate an adapter in each startup script: separates binding ownership and
  protocol behavior across routes.
- Force a supported shell or overwrite a user binding: changes startup semantics.
- Enable installation after an explicit integration opt-out: changes the user's
  installation policy. Ordinary-shell fallback remains available.

## Consequences

Repeated same-owner readiness keeps an immediate Tab request alive under the
existing editor state contract. User-owned F24 bindings still prevent adapter
installation. Unsupported editors, custom startup commands and failed integration
retain existing fallback behavior; this does not promise every shell is supported.
Bash before version 4 lacks the required native buffer/caret interface and does
not install or advertise the query widget. Its startup regression requires native
command execution without a readiness claim.

The QA driver keeps settings and SSH authentication profiles in a fresh config,
uses an owned known-host file and loopback key, and launches a desktop that is never
switched to. WSL QA disables hook installation into the guest home and redirects
shell history into the owned fixture. Authenticated SSH QA uses an isolated remote
HOME while exercising the normal installer and authenticated connection pool.

## Validation

[`test_completion_editor.py`](../../../../../../scripts/tests/test_completion_editor.py)
tests real Bash/zsh buffers and startup constants. The native
[`remote.rs`](../../../../../../nebula_app/src/gpui_shell/terminal/view/completion_native_tests/remote.rs)
fixture tests inline, popup and hybrid modes, immediate Tab, Chinese/emoji middle
editing with following flags, acceptance without execution, execution and Escape
cancellation. CI still compiles the shared test driver on selected native hosts;
an ignored graphical fixture is executed explicitly and is not counted as CI GUI
coverage. Actual native route results and limitations accompany the review.

## Supersedes

None. Extends [`2026-10-03-editor-snapshots.md`](2026-10-03-editor-snapshots.md).

## Revisit when

Additional shell editors expose a verified buffer API, or startup integration can
advertise capabilities independently of the existing owned shell assets.
