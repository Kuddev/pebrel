# Provider models, subagent defaults and compressed AI marks

## Status

Implemented locally; native acceptance remains separate from compile and unit checks.

## Context

The provider route existed but was hidden. Each provider held one model, so it
could not supply a model picker or a same-provider default for subagents. The
existing sidebar embedded large PNG exports even when SVG sources already existed.

## Evidence

- `nebula_app/src/ai_providers.rs` owns metadata and OS credential references.
- `nebula_app/src/codex_config.rs` owns explicit Codex configuration export.
- [Codex subagents](https://developers.openai.com/codex/subagents/) documents
  model/effort overrides and inheritance of the parent's provider.
- [Codex configuration reference](https://developers.openai.com/codex/config-reference/)
  documents `agents.default_subagent_model` and `model_reasoning_effort`.
- [Claude subagents](https://code.claude.com/docs/en/sub-agents) documents
  `CLAUDE_CODE_SUBAGENT_MODEL`; it remains a default, without forcing overrides.
- `extra/logo/README.md` records existing and added vector sources and regeneration.

## Decision

Keep provider metadata in the existing store, adding defaulted model-list,
raw effort, subagent model and agent-identity fields. The model picker reads only
its owning provider. Missing fields retain old single-model behavior. Preserve
multiple routes of the same protocol and user deletions across reloads.

The table and drawer adapt this model. Editing a row does not activate a provider.
CLI writes remain an explicit table action with a backup and confirmation because those
CLI files consume plaintext credentials. Codex and Claude use their documented
same-provider subagent defaults; no gateway or cross-provider routing is added.
Each CLI tracks its applied provider independently; the first table switch writes
the real CLI files before showing an active marker. Subsequent model/effort/default
edits on that route update its CLI configuration. Unsupported CLI adapters report
their boundary instead of presenting a metadata-only switch as a completed export.

Model discovery runs outside the UI thread, bounds time and response size, and
applies results only to the matching live editor generation. Keys stay outside
metadata serialization. The requested JSON editor keeps its credential only in
the current input draft, writes it through the OS credential store on Save, and
clears the draft when the drawer closes. Metadata writes use the existing atomic
replacement primitive. Editable TOML preserves additional configuration fields;
credential-bearing fields belong in auth.json rather than persisted TOML.
Credential access and exports run in owned background jobs. The compact JSON
credential has a 4096-byte limit shared with the Linux adapter; both credential
values are restored if a later save step fails. Codex and Claude exports acquire
the same per-file locks as Hook writers before reading and merging the latest
contents. Unix credential files and their backups use 0600 atomic replacement.

Native credential prompts are not invoked from GPUI callbacks: the Windows modal
message pump re-enters the application while its RefCell is borrowed. The reported
click on Set key produced a panic at GPUI AsyncApp::update; replacing that action
with the requested JSON input removes this nested modal call. The drawer uses
the existing deferred, occluding modal and focus-trap pattern to block background
clicks and scrolling. Column widths and compact agent selection follow the
available window width; the table keeps its header at every width and scrolls
inside its own bounds when necessary. Reasoning choices are ordered ultra, max,
xhigh, high, medium, low. The separate masked API-key input and auth JSON editor
share the same draft through value-checked subscriptions to avoid update loops.

Embed lossless SVGZ for AI icons, reusing the already-locked resvg 0.46.0 renderer.
Rasterization happens at the existing cache boundary and actual physical size.
The shared tint and optical-centering policy stays authoritative. SVG sources
remain reviewable, and PNG originals remain only in historical asset tests.

## Rejected alternatives

- A model-only cross-provider picker would select an identifier without routing
  its endpoint or credential; the requested scope is now same-provider selection.
- A new gateway would add a long-lived service outside the current scope.
- Copying prototype balance/expiration values would present invented account data.
- Embedding both large PNGs and SVGs would preserve the binary cost being removed.
- Rounding SVG path coordinates would make compression lossy.

## Consequences

The API provider entry is visible in Settings. Model and effort choices are saved
in Pebrel and exported through the existing explicit apply flow. Agent-specific
runtime support must be verified independently before adding another CLI adapter.
Source-byte savings do not claim a measured installer or executable-size reduction.

Download buttons start each CLI's official installation in a dedicated native
terminal, preserving progress, dependency prompts and cancellation through the
existing PTY lifetime. They never inherit an SSH/WSL pane. Installation commands
are queued once rather than persisted as shell startup arguments, so restoring
that tab does not reinstall. Node-dependent installers use a user-level Node 22
LTS download only when needed, verified against nodejs.org's SHA-256 list. Grok's
Windows binary uses its upstream checksum list. Discovery includes the official
standalone locations and refreshes when returning to settings; existing CLIs hide
their download button. Installer platform restrictions remain upstream's scope.

Installation sources were read on 2026-10-19: the official Codex/Claude setup
pages, google-gemini/gemini-cli README, opencode.ai/v2/install, superagent-ai/grok-cli
install.sh, pi.dev installers, NousResearch/hermes-agent installation docs, and
MoonshotAI/kimi-code installation docs. The archived kimi-cli installer is not used.

## Validation

Existing provider and Codex tests cover compatibility and export preservation;
focused cases cover duplicate routes, model ownership and Claude configuration.
Shared icon tests render both themes at physical sizes from 16 through 48 pixels.
SVGZ decompression is compared byte-for-byte with every source SVG. Compilation,
layout screenshots and credential-backed live requests are distinct evidence.
Windows provider/export/lock, UI input, font dropdown and vector tests passed locally.
Loopback HTTP tests cover authentication, URL construction, pagination and repeated
cursors for three protocols. No saved vendor key was available for authenticated
external account verification. macOS/Linux native credentials and live installer
execution still require platform acceptance; script syntax and launch-plan tests
are not recorded as successful product installation.

## Supersedes

None.

## Revisit when

An upstream CLI adds independent subagent provider/auth routing, a new provider
requires a different model-list contract, or native acceptance reveals an icon
or editor lifecycle regression.
