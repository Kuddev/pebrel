# Rule-based extra arguments for AI session resume and fork

## Status

Implemented; pending review.

## Context

Users resuming or forking AI coding agent sessions (`claude`, `codex`, `antigravity`, `gemini`, `opencode`, `cursor`, `kimi`, `pi`, `omp`, `copilot`, `grok`, `amp`) frequently need per-agent CLI flags such as `--yolo` or `--dangerously-skip-permissions`. These flags must be stored in `nebula_settings.txt`, manageable through a rule-based UI in `Settings → Coding Agents`, and quoted safely for the target pane's shell without introducing shell-command injection or coupling `nebula_settings` to UI/shell crates.

## Evidence

- `nebula_settings/src/agent_resume.rs`
- `nebula_app/src/agent_resume.rs`
- `nebula_app/src/platform/agent_resume.rs`
- `nebula_app/src/gpui_shell/terminal/view/startup_command.rs`
- `nebula_app/src/gpui_shell/settings_pane/agent_resume.rs`

## Decision

1. `nebula_settings::AgentResumeArgs` owns only the raw rule persistence (`agent_resume_args_<slug>` and `agent_resume_enabled_<slug>`) and canonical rule ordering, keeping `nebula_settings` zero-dependency.
2. `nebula_app::agent_resume` parses either natural CLI flag suffixes (rejecting unclosed quotes, control characters, and unquoted shell operators `;`, `|`, `&`, `` ` ``, `$`, `<`, `>`, `(`, `)`) or JSON string arrays (`["--yolo"]`), and delegates shell-specific literal argv quoting to `nebula_app::platform::agent_resume::quote_args`.
3. `TerminalView::configured_recovery_command` and `TerminalView::run_agent_session_command` apply active rules only at confirmed session Resume/Fork entrypoints (cold session recovery, `pi` session file resolution, `codex resume` fallback, `fork_ai_session`, and `WorkspacePaletteAction::RunAiSession`), leaving generic `new_pane` / `run_command` untouched and failing closed if a manually edited rule fails validation.
4. `Settings → Coding Agents` provides a compact rule builder (Agent selector + command suffix input + Save button) and an interactive rule list with per-rule enable/disable switch (`NebulaSwitch`) and delete button.

## Rejected alternatives

- Appending raw user strings directly to the resume/fork shell command: rejected because unquoted shell metacharacters would allow arbitrary command execution during session restore.
- Rendering 12 static text inputs in `Settings → Coding Agents`: rejected because most users configure rules for only 1–3 agents, and a static wall of inputs clutters the settings page.

## Consequences

- Active per-agent rules automatically apply to both Resume and Fork flows and are quoted for Posix shells, PowerShell (`powershell.exe` and `pwsh.exe`), and `cmd.exe`.
- Resetting settings clears both `agent_resume_args_<slug>` and `agent_resume_enabled_<slug>`.

## Validation

- `cargo test -p nebula-settings --locked`
- `cargo test -p nebula --bin pebrel --features gpui-shell,gpui-test-support agent_resume --locked`

## Supersedes

None

## Revisit when

Revisit if agent launch moves from shell command submission to direct argv process spawning without an intermediate shell.
