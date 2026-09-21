//! Shared AI-CLI identity, resumability and screen-state detection.
//!
//! Hooks remain the highest-confidence lifecycle source. This module fills the
//! gaps hooks cannot cover: wrapped process identity, agents without a hook
//! API, and interactive permission/question screens that a coarse "turn done"
//! callback cannot distinguish.
//!
//! Detection rules are declarative TOML. Bundled rules cover the popular
//! clients; `%APPDATA%\Nebula\agent-detection\<slug>.toml` (or the equivalent
//! [`nebula_settings::settings_dir`]) overrides one manifest. Overrides are
//! mtime-checked at most once every two seconds, so rules can be tuned while a
//! client is running without putting filesystem work on every frame.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{OnceLock, RwLock};
use std::time::{Duration, Instant, SystemTime};

use regex::Regex;
use serde::Deserialize;

mod screen_context;

/// AI clients Nebula can identify as a first-class terminal workload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AgentKind {
    Claude,
    Codex,
    Gemini,
    Aider,
    Amp,
    OpenCode,
    Copilot,
    Cursor,
    Goose,
    Droid,
    Pi,
    Auggie,
    Hermes,
    Vibe,
    Antigravity,
    Grok,
    Qwen,
    OhMyPi,
    Cline,
    Devin,
    Kimi,
    Kiro,
    Kilo,
    Qoder,
    Maki,
    Trae,
    CodeBuddy,
}

impl AgentKind {
    pub const ALL: [Self; 27] = [
        Self::Claude,
        Self::Codex,
        Self::Gemini,
        Self::Aider,
        Self::Amp,
        Self::OpenCode,
        Self::Copilot,
        Self::Cursor,
        Self::Goose,
        Self::Droid,
        Self::Pi,
        Self::Auggie,
        Self::Hermes,
        Self::Vibe,
        Self::Antigravity,
        Self::Grok,
        Self::Qwen,
        Self::OhMyPi,
        Self::Cline,
        Self::Devin,
        Self::Kimi,
        Self::Kiro,
        Self::Kilo,
        Self::Qoder,
        Self::Maki,
        Self::Trae,
        Self::CodeBuddy,
    ];

    pub fn slug(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Gemini => "gemini",
            Self::Aider => "aider",
            Self::Amp => "amp",
            Self::OpenCode => "opencode",
            Self::Copilot => "copilot",
            Self::Cursor => "cursor",
            Self::Goose => "goose",
            Self::Droid => "droid",
            Self::Pi => "pi",
            Self::Auggie => "auggie",
            Self::Hermes => "hermes",
            Self::Vibe => "vibe",
            Self::Antigravity => "antigravity",
            Self::Grok => "grok",
            Self::Qwen => "qwen",
            Self::OhMyPi => "omp",
            Self::Cline => "cline",
            Self::Devin => "devin",
            Self::Kimi => "kimi",
            Self::Kiro => "kiro",
            Self::Kilo => "kilo",
            Self::Qoder => "qodercli",
            Self::Maki => "maki",
            Self::Trae => "trae-cli",
            Self::CodeBuddy => "codebuddy",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Claude => "Claude Code",
            Self::Codex => "Codex",
            Self::Gemini => "Gemini",
            Self::Aider => "Aider",
            Self::Amp => "Amp",
            Self::OpenCode => "OpenCode",
            Self::Copilot => "GitHub Copilot",
            Self::Cursor => "Cursor Agent",
            Self::Goose => "Goose",
            Self::Droid => "Droid",
            Self::Pi => "Pi",
            Self::Auggie => "Auggie",
            Self::Hermes => "Hermes",
            Self::Vibe => "Vibe",
            Self::Antigravity => "Antigravity",
            Self::Grok => "Grok",
            Self::Qwen => "Qwen Code",
            Self::OhMyPi => "Oh My Pi",
            Self::Cline => "Cline",
            Self::Devin => "Devin",
            Self::Kimi => "Kimi Code",
            Self::Kiro => "Kiro",
            Self::Kilo => "Kilo Code",
            Self::Qoder => "Qoder",
            Self::Maki => "Maki",
            Self::Trae => "Trae CLI",
            Self::CodeBuddy => "CodeBuddy Code",
        }
    }

    pub fn label(self) -> &'static str {
        self.slug()
    }

    fn aliases(self) -> &'static [&'static str] {
        match self {
            Self::Claude => &["claude", "claude-code"],
            Self::Codex => &["codex", "codex-cli"],
            Self::Gemini => &["gemini", "gemini-cli"],
            Self::Aider => &["aider", "aider-chat"],
            Self::Amp => &["amp", "amp-local"],
            Self::OpenCode => &["opencode", "open-code"],
            Self::Copilot => &["copilot", "github-copilot", "ghcs"],
            // Cursor's current CLI binary is `agent`; keep the historical
            // labels so existing process snapshots still normalize correctly.
            Self::Cursor => &["agent", "cursor", "cursor-agent"],
            Self::Goose => &["goose"],
            Self::Droid => &["droid"],
            Self::Pi => &["pi"],
            Self::Auggie => &["auggie"],
            Self::Hermes => &["hermes", "hermes-agent"],
            Self::Vibe => &["vibe", "vibe-acp"],
            Self::Antigravity => &["agy", "antigravity", "antigravity-cli"],
            Self::Grok => &["grok", "grok-cli", "grok-build"],
            Self::Qwen => &["qwen", "qwen-code"],
            Self::OhMyPi => &["omp", "oh-my-pi"],
            Self::Cline => &["cline"],
            Self::Devin => &["devin", "devin-cli"],
            Self::Kimi => &["kimi", "kimi-code"],
            Self::Kiro => &["kiro", "kiro-cli"],
            Self::Kilo => &["kilo", "kilo-code"],
            Self::Qoder => &["qodercli", "qoderclicn", "qoder", "qodercn"],
            Self::Maki => &["maki"],
            // ByteDance's trae-agent declares this console entry point.
            Self::Trae => &["trae-cli"],
            // @tencent-ai/codebuddy-code 2.150.0's interactive bin entries.
            Self::CodeBuddy => &["codebuddy", "cbc", "codebuddy-code", "codebuddy-lowmem"],
        }
    }

    /// Resolve an executable/program label to a canonical client identity.
    pub fn parse(raw: &str) -> Option<Self> {
        let raw = raw
            .trim()
            .trim_matches(['"', '\''])
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(raw)
            .to_ascii_lowercase();
        let raw = [".exe", ".cmd", ".bat", ".ps1", ".com", ".js"]
            .into_iter()
            .find_map(|suffix| raw.strip_suffix(suffix))
            .unwrap_or(&raw);
        Self::ALL.into_iter().find(|agent| agent.aliases().contains(&raw))
    }

    /// Resolve an agent from a submitted shell command, including common
    /// interpreter and package-runner launch forms used inside WSL.
    pub fn parse_command(command: &str) -> Option<Self> {
        let mut tokens = command
            .split_whitespace()
            .map(|token| token.trim_matches(['"', '\'']))
            .filter(|token| !token.is_empty());
        let mut launcher = tokens.next()?;
        if launcher == "&" {
            launcher = tokens.next()?;
        }
        while is_env_assignment(launcher) {
            launcher = tokens.next()?;
        }

        if launcher_stem(launcher) == "env" {
            launcher = tokens
                .by_ref()
                .find(|token| !token.starts_with('-') && !is_env_assignment(token))?;
        }
        if let Some(agent) = Self::parse(launcher) {
            return Some(agent);
        }
        if !is_agent_interpreter(launcher) {
            return None;
        }

        tokens
            .filter(|token| !token.starts_with('-'))
            // The prewarm helper also lives inside the codebuddy-code package;
            // its parent directory alone must not identify it as a live CLI.
            .filter(|token| launcher_stem(token) != "cbc-prewarm")
            .find_map(|token| token.split(['/', '\\']).find_map(Self::parse))
    }

    /// Shell-safe resume command. Session ids are untrusted hook/file input,
    /// so an unsupported shape returns `None` instead of being quoted.
    pub fn resume_command(self, session_id: &str) -> Option<String> {
        valid_session_id(session_id)?;
        Some(match self {
            Self::Claude => format!("claude --resume {session_id}"),
            Self::Codex => format!("codex resume {session_id}"),
            Self::Gemini => format!("gemini --resume {session_id}"),
            Self::OpenCode => format!("opencode --session {session_id}"),
            Self::Amp => format!("amp threads continue {session_id}"),
            Self::Cursor => format!("agent --resume={session_id}"),
            Self::Copilot => format!("copilot --resume {session_id}"),
            Self::Grok => format!("grok --resume {session_id}"),
            Self::Pi => format!("pi --session {session_id}"),
            Self::OhMyPi => format!("omp --resume {session_id}"),
            Self::Aider
            | Self::Goose
            | Self::Droid
            | Self::Auggie
            | Self::Hermes
            | Self::Vibe
            | Self::Antigravity
            | Self::Qwen
            | Self::Cline
            | Self::Devin
            | Self::Kiro
            | Self::Kilo
            | Self::Qoder
            | Self::Maki
            | Self::Trae
            | Self::CodeBuddy => return None,
            Self::Kimi => format!("kimi --session {session_id}"),
        })
    }

    /// Cold-start commands whose interactive CLI spelling is verified against
    /// the provider's official CLI documentation. Unsupported clients must not
    /// be guessed from their detection slug.
    pub fn start_command(self) -> Option<String> {
        match self {
            Self::Claude => Some("claude".to_owned()),
            Self::Codex => Some("codex".to_owned()),
            Self::OpenCode => Some("opencode".to_owned()),
            Self::Cursor => Some("agent".to_owned()),
            Self::Pi => Some("pi".to_owned()),
            Self::OhMyPi => Some("omp".to_owned()),
            Self::Kimi => Some("kimi".to_owned()),
            _ => None,
        }
    }

    /// Shell-safe fork command for clients with a verified fork syntax.
    pub fn fork_command(self, session_id: &str) -> Option<String> {
        valid_session_id(session_id)?;
        Some(match self {
            Self::Claude => format!("claude --resume {session_id} --fork-session"),
            Self::Codex => format!("codex fork {session_id}"),
            Self::OpenCode => format!("opencode --session {session_id} --fork"),
            Self::Grok => format!("grok --resume {session_id} --fork-session"),
            Self::Pi => format!("pi --fork {session_id}"),
            Self::OhMyPi => format!("omp --fork {session_id}"),
            Self::Kimi => format!("kimi --fork {session_id}"),
            _ => return None,
        })
    }
}

fn valid_session_id(id: &str) -> Option<()> {
    (!id.is_empty()
        && id.len() <= 64
        && id.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.')))
    .then_some(())
}

fn is_env_assignment(token: &str) -> bool {
    let Some((name, _)) = token.split_once('=') else { return false };
    let mut bytes = name.bytes();
    bytes.next().is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn launcher_stem(token: &str) -> String {
    let basename = token.trim_end_matches(['/', '\\']).rsplit(['/', '\\']).next().unwrap_or(token);
    let lower = basename.to_ascii_lowercase();
    [".exe", ".cmd", ".bat", ".ps1", ".com"]
        .into_iter()
        .find_map(|suffix| lower.strip_suffix(suffix).map(str::to_owned))
        .unwrap_or(lower)
}

fn is_agent_interpreter(token: &str) -> bool {
    matches!(
        launcher_stem(token).as_str(),
        "node"
            | "nodejs"
            | "bun"
            | "deno"
            | "npx"
            | "npm"
            | "pnpm"
            | "pnpx"
            | "yarn"
            | "bunx"
            | "python"
            | "python3"
            | "ruby"
            | "pipx"
            | "uv"
            | "uvx"
    )
}

/// Semantic Agent state shared by lifecycle hooks and fallback observations.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AgentStatus {
    Idle,
    Working,
    Blocked,
    Done,
    #[default]
    Unknown,
}

impl AgentStatus {
    /// 这个 pane 是否已经有权威判定。`Unknown` 表示 hook 与屏幕规则都没认领它
    /// （普通 shell，或还没识别出 agent）。
    ///
    /// BEL 的兜底资格看这一位。响铃的含义是「有事发生」，不是「停下来等你输
    /// 入」：CC / codex 在**回合结束**时同样响铃（终端自己的通知文案就写着
    /// 「任务完成，等待输入」，两个概念当初就被焊在一起）。一旦让响铃去表示等
    /// 待输入，agent 每做完一件事都会显示成「在问你」——所以有权威判定时响铃
    /// 不得改写它，只在没有判定时兜底。
    pub const fn is_decided(self) -> bool {
        !matches!(self, Self::Unknown)
    }
}

/// Why the current pane status has its value.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AgentStatusSource {
    Hook,
    Screen,
    Process,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detection {
    pub agent: AgentKind,
    pub status: AgentStatus,
    pub rule_id: String,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
struct Manifest {
    id: String,
    #[serde(default)]
    aliases: Vec<String>,
    #[serde(default)]
    identity: Vec<Gate>,
    /// Optional region wrapper for inherited rules; their original windows remain intact.
    #[serde(default)]
    shared_rule_region: Option<String>,
    rules: Vec<Rule>,
}

/// `_shared.toml`: rules merged into every manifest, with no agent identity.
#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
struct SharedManifest {
    rules: Vec<Rule>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
struct Rule {
    id: String,
    state: RuleState,
    #[serde(default)]
    priority: i32,
    #[serde(default = "whole_recent")]
    region: String,
    #[serde(flatten)]
    gate: Gate,
}

#[derive(Debug, Deserialize, Clone, Default)]
#[serde(deny_unknown_fields)]
struct Gate {
    #[serde(default)]
    contains: Vec<String>,
    #[serde(default)]
    regex: Vec<String>,
    #[serde(default)]
    line_regex: Vec<String>,
    #[serde(default)]
    all: Vec<Gate>,
    #[serde(default)]
    any: Vec<Gate>,
    #[serde(default, rename = "not")]
    not_gate: Vec<Gate>,
}

#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum RuleState {
    Idle,
    Working,
    Blocked,
}

impl From<RuleState> for AgentStatus {
    fn from(state: RuleState) -> Self {
        match state {
            RuleState::Idle => Self::Idle,
            RuleState::Working => Self::Working,
            RuleState::Blocked => Self::Blocked,
        }
    }
}

#[derive(Debug)]
struct CompiledManifest {
    manifest: Manifest,
    identity: Vec<CompiledGate>,
    rules: Vec<CompiledGate>,
    override_mtime: Option<SystemTime>,
}

#[derive(Debug)]
struct CompiledGate {
    contains: Vec<String>,
    regex: Vec<Regex>,
    line_regex: Vec<Regex>,
    all: Vec<CompiledGate>,
    any: Vec<CompiledGate>,
    not_gate: Vec<CompiledGate>,
}

#[derive(Debug)]
struct Cache {
    manifests: HashMap<AgentKind, CompiledManifest>,
    last_override_scan: Instant,
}

/// Skeleton rules merged into every manifest below (and into user overrides).
const SHARED: &str = include_str!("agent_detection/_shared.toml");

const BUNDLED: &[(AgentKind, &str)] = &[
    (AgentKind::Claude, include_str!("agent_detection/claude.toml")),
    (AgentKind::Codex, include_str!("agent_detection/codex.toml")),
    (AgentKind::Gemini, include_str!("agent_detection/gemini.toml")),
    (AgentKind::Cursor, include_str!("agent_detection/cursor.toml")),
    (AgentKind::OpenCode, include_str!("agent_detection/opencode.toml")),
    (AgentKind::Copilot, include_str!("agent_detection/copilot.toml")),
    (AgentKind::Grok, include_str!("agent_detection/grok.toml")),
    (AgentKind::Pi, include_str!("agent_detection/pi.toml")),
    (AgentKind::Amp, include_str!("agent_detection/amp.toml")),
    (AgentKind::Antigravity, include_str!("agent_detection/antigravity.toml")),
    (AgentKind::Cline, include_str!("agent_detection/cline.toml")),
    (AgentKind::Devin, include_str!("agent_detection/devin.toml")),
    (AgentKind::Droid, include_str!("agent_detection/droid.toml")),
    (AgentKind::Hermes, include_str!("agent_detection/hermes.toml")),
    (AgentKind::Kimi, include_str!("agent_detection/kimi.toml")),
    (AgentKind::Kiro, include_str!("agent_detection/kiro.toml")),
    (AgentKind::Kilo, include_str!("agent_detection/kilo.toml")),
    (AgentKind::Qoder, include_str!("agent_detection/qodercli.toml")),
    (AgentKind::Maki, include_str!("agent_detection/maki.toml")),
    (AgentKind::CodeBuddy, include_str!("agent_detection/codebuddy.toml")),
];

static CACHE: OnceLock<RwLock<Cache>> = OnceLock::new();

fn cache() -> &'static RwLock<Cache> {
    CACHE.get_or_init(|| {
        RwLock::new(Cache { manifests: build_cache(), last_override_scan: Instant::now() })
    })
}

fn build_cache() -> HashMap<AgentKind, CompiledManifest> {
    BUNDLED
        .iter()
        .map(|(agent, source)| {
            let path = override_path(*agent);
            let disk_mtime = modified(&path);
            let bundled = compile_manifest(source, disk_mtime).unwrap_or_else(|error| {
                panic!("bundled {} agent rules are invalid: {error}", agent.slug())
            });
            let loaded = std::fs::read_to_string(&path)
                .ok()
                .and_then(|text| {
                    compile_manifest(&text, disk_mtime)
                        .map_err(|error| {
                            log::warn!("agent detection: ignored {}: {error}", path.display());
                        })
                        .ok()
                })
                .filter(|loaded| {
                    let matches = manifest_matches(&loaded.manifest, *agent);
                    if !matches {
                        log::warn!(
                            "agent detection: ignored {} because id {} does not match {}",
                            path.display(),
                            loaded.manifest.id,
                            agent.slug()
                        );
                    }
                    matches
                })
                .unwrap_or(bundled);
            (*agent, loaded)
        })
        .collect()
}

fn override_path(agent: AgentKind) -> PathBuf {
    settings_dir().join("agent-detection").join(format!("{}.toml", agent.slug()))
}

/// Same path contract as nebula-settings, repeated here because that crate is
/// optional in non-GPUI builds while agent semantics serve both shells.
fn settings_dir() -> PathBuf {
    if let Some(path) = std::env::var_os("NEBULA_CONFIG_DIR").filter(|path| !path.is_empty()) {
        return PathBuf::from(path);
    }
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .unwrap_or_else(std::env::temp_dir)
        .join("Nebula")
}

fn modified(path: &std::path::Path) -> Option<SystemTime> {
    path.metadata().and_then(|meta| meta.modified()).ok()
}

fn refresh_overrides_if_needed() {
    let should_scan = cache()
        .read()
        .map(|guard| guard.last_override_scan.elapsed() >= Duration::from_secs(2))
        .unwrap_or(true);
    if !should_scan {
        return;
    }
    let Ok(mut guard) = cache().write() else { return };
    if guard.last_override_scan.elapsed() < Duration::from_secs(2) {
        return;
    }
    guard.last_override_scan = Instant::now();
    let changed = BUNDLED.iter().any(|(agent, _)| {
        let path = override_path(*agent);
        let disk_mtime = modified(&path);
        guard.manifests.get(agent).is_none_or(|loaded| loaded.override_mtime != disk_mtime)
    });
    if changed {
        guard.manifests = build_cache();
    }
}

/// Match one live screen snapshot. No match is `None`: callers retain their
/// higher-confidence hook/process state rather than fabricating idle.
pub fn detect(program: &str, screen: &str) -> Option<Detection> {
    let agent = AgentKind::parse(program)?;
    refresh_overrides_if_needed();
    let guard = cache().read().ok()?;
    let loaded = guard.manifests.get(&agent)?;
    let attention_screen = screen_context::attention_region(agent, screen);
    let live_input = screen_context::has_live_input_controls(attention_screen);
    let mut best: Option<(&Rule, &CompiledGate)> = None;
    for (rule, gate) in loaded.manifest.rules.iter().zip(&loaded.rules) {
        let blocked = matches!(rule.state, RuleState::Blocked);
        if blocked && !live_input {
            continue;
        }
        let text = region(if blocked { attention_screen } else { screen }, &rule.region);
        if !gate.matches(text) {
            continue;
        }
        // Equal priorities keep the first declared match (local rules precede shared rules).
        if best.is_none_or(|(previous, _)| rule.priority > previous.priority) {
            best = Some((rule, gate));
        }
    }
    best.map(|(rule, _)| Detection { agent, status: rule.state.into(), rule_id: rule.id.clone() })
}

/// Establish an agent identity from brand-specific terminal chrome.
///
/// This is deliberately separate from [`detect`]: generic status chrome such
/// as a prompt glyph or an interrupt hint can refine a known agent, but must
/// never invent one. The fallback matters most on Windows-hosted WSL panes,
/// where Toolhelp can see `wsl.exe` but not the Linux `codex` process behind it.
pub fn identify(screen: &str) -> Option<AgentKind> {
    refresh_overrides_if_needed();
    let guard = cache().read().ok()?;
    let mut found = None;
    for (agent, loaded) in &guard.manifests {
        if !loaded.identity.iter().any(|gate| gate.matches(screen)) {
            continue;
        }
        if found.is_some() {
            return None;
        }
        found = Some(*agent);
    }
    found
}

fn compile_manifest(
    source: &str,
    override_mtime: Option<SystemTime>,
) -> Result<CompiledManifest, String> {
    let mut manifest: Manifest = toml::from_str(source).map_err(|error| error.to_string())?;
    if manifest.rules.is_empty() || manifest.rules.len() > 64 {
        return Err("manifest must contain 1..=64 rules".to_owned());
    }
    // 共享骨架并入每一份 manifest——bundled 与用户 override 一视同仁，装了
    // 新 CLI 或改了本地规则都自动带上中断提示判据。理由见 _shared.toml 顶部：
    // per-agent 的 working 规则各写各的，同一个 AND 陷阱在多份文件里重复出现。
    if manifest.shared_rule_region.as_deref().is_some_and(|value| value != "after_last_prompt_row")
    {
        return Err("unsupported shared_rule_region wrapper".to_owned());
    }
    for mut rule in shared_rules().iter().cloned() {
        if let Some(wrapper) = &manifest.shared_rule_region {
            rule.region = format!("{wrapper}({})", rule.region);
        }
        manifest.rules.push(rule);
    }
    let rules = manifest
        .rules
        .iter()
        .map(|rule| {
            // Validate only the newly introduced wrapper; legacy regions retain
            // their existing loading behavior.
            if rule.region.starts_with("after_last_prompt_row") {
                let inner = rule
                    .region
                    .strip_prefix("after_last_prompt_row(")
                    .and_then(|value| value.strip_suffix(')'));
                if !inner.is_some_and(|value| {
                    value == "whole_recent"
                        || value == "after_last_horizontal_rule"
                        || region_count(value, "bottom_non_empty_lines").is_some_and(|n| n > 0)
                }) {
                    return Err(format!("invalid prompt region: {}", rule.region));
                }
            }
            compile_gate(&rule.gate)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let identity = manifest.identity.iter().map(compile_gate).collect::<Result<Vec<_>, _>>()?;
    Ok(CompiledManifest { manifest, identity, rules, override_mtime })
}

/// Rules every agent manifest inherits. Parsed once; see `_shared.toml`.
fn shared_rules() -> &'static Vec<Rule> {
    static SHARED_RULES: OnceLock<Vec<Rule>> = OnceLock::new();
    SHARED_RULES.get_or_init(|| {
        let parsed: SharedManifest = toml::from_str(SHARED)
            .unwrap_or_else(|error| panic!("bundled shared agent rules are invalid: {error}"));
        parsed.rules
    })
}

fn compile_gate(gate: &Gate) -> Result<CompiledGate, String> {
    let compile = |patterns: &[String]| {
        patterns
            .iter()
            .map(|pattern| Regex::new(pattern).map_err(|error| error.to_string()))
            .collect::<Result<Vec<_>, _>>()
    };
    Ok(CompiledGate {
        contains: gate.contains.iter().map(|value| value.to_lowercase()).collect(),
        regex: compile(&gate.regex)?,
        line_regex: compile(&gate.line_regex)?,
        all: gate.all.iter().map(compile_gate).collect::<Result<_, _>>()?,
        any: gate.any.iter().map(compile_gate).collect::<Result<_, _>>()?,
        not_gate: gate.not_gate.iter().map(compile_gate).collect::<Result<_, _>>()?,
    })
}

impl CompiledGate {
    fn matches(&self, text: &str) -> bool {
        let lower = text.to_lowercase();
        self.contains.iter().all(|needle| lower.contains(needle))
            && self.regex.iter().all(|regex| regex.is_match(text))
            && self.line_regex.iter().all(|regex| text.lines().any(|line| regex.is_match(line)))
            && self.all.iter().all(|gate| gate.matches(text))
            && (self.any.is_empty() || self.any.iter().any(|gate| gate.matches(text)))
            && !self.not_gate.iter().any(|gate| gate.matches(text))
    }
}

fn manifest_matches(manifest: &Manifest, agent: AgentKind) -> bool {
    manifest.id == agent.slug()
        || manifest.aliases.iter().any(|alias| agent.aliases().contains(&alias.as_str()))
}

/// Keep the latest prompt row and its footer, as captured in agy 1.1.27.
/// Empty prompts may have a box. Drafts require horizontal rules immediately
/// above and below; an arbitrary quoted or selected `> text` is not a boundary.
/// Without an identifiable boundary, preserve the supplied window so real approvals
/// are not silently discarded. This deliberately does not guess arbitrary input text.
fn from_last_empty_prompt(screen: &str) -> &str {
    let mut offset = 0;
    let mut start = 0;
    let mut lines = screen.split_inclusive('\n').peekable();
    let mut previous = "";
    while let Some(line) = lines.next() {
        let row = line.trim();
        let row = row.strip_prefix('│').and_then(|row| row.strip_suffix('│')).unwrap_or(row).trim();
        let draft = row.strip_prefix(['›', '❯', '>']).is_some_and(|rest| {
            rest.starts_with(char::is_whitespace)
                && !rest.trim_start().starts_with(|c: char| c.is_ascii_digit() || c == '[')
        });
        let horizontal_rule = |line: &str| {
            let row = line.trim();
            row.chars().count() >= 3 && row.chars().all(|c| c == '─')
        };
        if matches!(row, "›" | "❯" | ">")
            || (draft
                && horizontal_rule(previous)
                && lines.peek().is_some_and(|next| horizontal_rule(next)))
        {
            start = offset;
        }
        previous = line;
        offset += line.len();
    }
    &screen[start..]
}

fn whole_recent() -> String {
    "whole_recent".to_owned()
}

fn region<'a>(screen: &'a str, spec: &str) -> &'a str {
    if spec == "whole_recent" {
        return screen;
    }
    if let Some(inner) =
        spec.strip_prefix("after_last_prompt_row(").and_then(|s| s.strip_suffix(')'))
    {
        return from_last_empty_prompt(region(screen, inner));
    }
    if let Some(count) = region_count(spec, "bottom_non_empty_lines") {
        let lines: Vec<&str> = screen.lines().collect();
        let Some(start) = lines
            .iter()
            .enumerate()
            .rev()
            .filter(|(_, line)| !line.trim().is_empty())
            .take(count)
            .last()
            .map(|(index, _)| index)
        else {
            return "";
        };
        return slice_from_line(screen, &lines, start);
    }
    if spec == "after_last_horizontal_rule" {
        let mut offset = 0;
        let mut last = 0;
        for line in screen.lines() {
            offset = (offset + line.len() + 1).min(screen.len());
            let trimmed = line.trim();
            if trimmed.chars().filter(|character| *character == '─').count() >= 3 {
                last = offset;
            }
        }
        return &screen[last..];
    }
    ""
}

fn region_count(spec: &str, name: &str) -> Option<usize> {
    spec.strip_prefix(name)?.strip_prefix('(')?.strip_suffix(')')?.parse().ok()
}

fn slice_from_line<'a>(screen: &'a str, lines: &[&str], index: usize) -> &'a str {
    let offset = lines[..index.min(lines.len())]
        .iter()
        .map(|line| line.len() + 1)
        .sum::<usize>()
        .min(screen.len());
    &screen[offset..]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aliases_resolve_to_canonical_agents() {
        assert_eq!(AgentKind::parse(r"C:\tools\CLAUDE.EXE"), Some(AgentKind::Claude));
        assert_eq!(AgentKind::parse("cursor-agent"), Some(AgentKind::Cursor));
        assert_eq!(AgentKind::parse("grok-cli.cmd"), Some(AgentKind::Grok));
        assert_eq!(AgentKind::parse("cargo"), None);
    }

    #[test]
    fn trae_cli_identity_does_not_guess_session_commands() {
        for command in ["trae-cli", r"C:\tools\TRAE-CLI.EXE", "/usr/bin/trae-cli"] {
            assert_eq!(AgentKind::parse(command), Some(AgentKind::Trae));
        }
        assert_eq!(AgentKind::parse_command("uv run trae-cli --help"), Some(AgentKind::Trae));
        assert_eq!(AgentKind::parse("trae-cli-helper"), None);
        assert_eq!(AgentKind::Trae.start_command(), None);
        assert_eq!(AgentKind::Trae.resume_command("session-1"), None);
        assert_eq!(AgentKind::Trae.fork_command("session-1"), None);
    }

    #[test]
    fn codebuddy_entry_points_identify_the_cli_without_claiming_its_helper() {
        for command in [
            "codebuddy",
            "cbc",
            "codebuddy-code",
            "codebuddy-lowmem",
            r"C:\tools\CODEBUDDY.CMD",
            "/usr/bin/cbc",
        ] {
            assert_eq!(AgentKind::parse(command), Some(AgentKind::CodeBuddy), "{command}");
        }
        for command in [
            "npx --yes @tencent-ai/codebuddy-code",
            "node /opt/node_modules/@tencent-ai/codebuddy-code/dist/codebuddy.js",
            "env DEBUG=1 codebuddy --help",
        ] {
            assert_eq!(AgentKind::parse_command(command), Some(AgentKind::CodeBuddy), "{command}");
        }
        for command in [
            "cbc-prewarm",
            "codebuddy-helper",
            "cat codebuddy.md",
            "node /opt/node_modules/@tencent-ai/codebuddy-code/bin/cbc-prewarm",
        ] {
            assert_eq!(AgentKind::parse_command(command), None, "{command}");
        }
        assert_eq!(AgentKind::CodeBuddy.start_command(), None);
        assert_eq!(AgentKind::CodeBuddy.resume_command("session-1"), None);
        assert_eq!(AgentKind::CodeBuddy.fork_command("session-1"), None);
    }

    #[test]
    fn codebuddy_screen_identity_requires_brand_and_live_footer() {
        // Transcribed from the user's 2.150.0 Windows/WSL screenshot, not a
        // runtime capture. Only the observed prompt/footer establishes idle.
        let screen = "╭─ CodeBuddy Code v2.150.0 ─╮\nTips for getting started\n\
                      ────────────────\n> \n────────────────\n\
                      /agent-mode to switch · ? for shortcuts ← for agents";
        assert_eq!(identify(screen), Some(AgentKind::CodeBuddy));
        let idle = detect("codebuddy", screen).unwrap();
        assert_eq!(idle.status, AgentStatus::Idle);
        for text in [
            "CodeBuddy Code is a CLI.",
            "CodeBuddy Code v2.150.0\nuser@host:~$ ",
            "/agent-mode to switch · ? for shortcuts ← for agents",
            "> generic prompt\n? for shortcuts ← for agents",
            &format!("{screen}\nuser@host:~$ "),
        ] {
            assert_eq!(identify(text), None, "{text}");
        }
        assert!(detect("codebuddy", &format!("{screen}\nuser@host:~$ ")).is_none());
    }

    #[test]
    fn submitted_commands_resolve_agents_across_wsl_launch_forms() {
        for agent in AgentKind::ALL {
            assert_eq!(AgentKind::parse_command(agent.slug()), Some(agent), "{}", agent.slug());
        }
        assert_eq!(
            AgentKind::parse_command("FOO=1 npx --yes @openai/codex --model o3"),
            Some(AgentKind::Codex)
        );
        assert_eq!(
            AgentKind::parse_command("env DEBUG=1 node /opt/@anthropic-ai/claude-code/cli.js"),
            Some(AgentKind::Claude)
        );
        assert_eq!(
            AgentKind::parse_command("npm exec @google/gemini-cli"),
            Some(AgentKind::Gemini)
        );
        assert_eq!(AgentKind::parse_command("uvx aider-chat"), Some(AgentKind::Aider));
        assert_eq!(AgentKind::parse_command("cat codex.md"), None);
        assert_eq!(AgentKind::parse_command("node server.js"), None);
    }

    #[test]
    fn commands_are_exact_and_injection_safe() {
        assert_eq!(AgentKind::Claude.start_command().as_deref(), Some("claude"));
        assert_eq!(AgentKind::Codex.start_command().as_deref(), Some("codex"));
        assert_eq!(AgentKind::OpenCode.start_command().as_deref(), Some("opencode"));
        assert_eq!(AgentKind::Cursor.start_command().as_deref(), Some("agent"));
        assert_eq!(AgentKind::Pi.start_command().as_deref(), Some("pi"));
        assert_eq!(AgentKind::OhMyPi.start_command().as_deref(), Some("omp"));
        assert_eq!(AgentKind::Kimi.start_command().as_deref(), Some("kimi"));
        assert_eq!(AgentKind::Gemini.start_command(), None);
        assert_eq!(
            AgentKind::Claude.resume_command("abc-123").as_deref(),
            Some("claude --resume abc-123")
        );
        assert_eq!(AgentKind::Codex.fork_command("abc-123").as_deref(), Some("codex fork abc-123"));
        assert_eq!(
            AgentKind::OpenCode.fork_command("abc-123").as_deref(),
            Some("opencode --session abc-123 --fork")
        );
        assert_eq!(
            AgentKind::Cursor.resume_command("abc-123").as_deref(),
            Some("agent --resume=abc-123")
        );
        assert_eq!(AgentKind::Pi.fork_command("abc-123").as_deref(), Some("pi --fork abc-123"));
        assert_eq!(
            AgentKind::OhMyPi.resume_command("abc-123").as_deref(),
            Some("omp --resume abc-123")
        );
        assert_eq!(
            AgentKind::Kimi.resume_command("abc-123").as_deref(),
            Some("kimi --session abc-123")
        );
        assert_eq!(AgentKind::Kimi.fork_command("abc-123").as_deref(), Some("kimi --fork abc-123"));
        assert_eq!(AgentKind::Claude.resume_command("x; calc"), None);
        assert_eq!(AgentKind::Aider.resume_command("abc"), None);
    }

    /// 响铃只能在没人认领这个 pane 时兜底。CC / codex 在回合结束时也响铃，所以
    /// 一旦让它压过 hook / 屏幕规则的判定，「完成」会在 tab 上显示成「在问你」
    /// ——这就是终端徽章那条「结束了却画手掌」的病根。
    #[test]
    fn bell_may_only_speak_for_panes_no_rule_has_claimed() {
        for decided in
            [AgentStatus::Idle, AgentStatus::Working, AgentStatus::Blocked, AgentStatus::Done]
        {
            assert!(decided.is_decided(), "{decided:?} 是权威判定，响铃不得改写");
        }
        assert!(!AgentStatus::Unknown.is_decided(), "没有判定时才轮到响铃兜底");
    }

    #[test]
    fn screen_rules_distinguish_working_blocked_and_idle() {
        let blocked =
            detect("claude", "Do you want to proceed?\n❯ 1. Yes\n  2. No\nEsc to cancel").unwrap();
        assert_eq!(blocked.status, AgentStatus::Blocked);
        let working = detect("codex", "• Working (12s • esc to interrupt)").unwrap();
        assert_eq!(working.status, AgentStatus::Working);
        let idle = detect("claude", "────────────────\n❯ ").unwrap();
        assert_eq!(idle.status, AgentStatus::Idle);
    }

    #[test]
    fn review_regression_restored_codex_identity_requires_live_prompt_and_footer() {
        let live = "› Ask Codex to do anything\n\n  gpt-6-astra max · /mnt/d/temp_build/project · Saved conversation";
        assert_eq!(identify(live), Some(AgentKind::Codex));
        for text in [
            "› generic shell prompt",
            "The CLI says Ask Codex to do anything.",
            "› Ask Codex to do anything\nuser@host:~$ ",
            "› Ask Codex to do anything\ngpt-6-astra max · /project\nuser@host:~$ ",
        ] {
            assert_eq!(identify(text), None, "{text}");
        }
    }

    #[test]
    fn branded_screen_chrome_identifies_codex_without_a_visible_host_process() {
        let screen = "OpenAI Codex (v0.42.0)\n\n› Ask Codex to do anything";
        assert_eq!(identify(screen), Some(AgentKind::Codex));
        assert_eq!(identify("› generic shell prompt"), None);
    }

    /// 下面四段都是 2026-08-22 用 runtime `pane.read` 从真实窗口抓的原文，
    /// 也就是判错的现场。规则改动必须对着它们回归，不能对着记忆里的格式写。
    #[test]
    fn real_claude_working_chrome_reads_working() {
        // 正在干活的 pane。spinner 行用 ✶（U+2736）而不是盲文点阵，且与底部
        // 中断提示相隔 6 行——旧规则要求「盲文行 AND 中断词」且 region 只有
        // 6 行，两个条件同时落空，回合进行中因此被判成 idle，再连续两拍降级
        // 成「完成」蓝点。
        let screen = "  ⎿  Running…\n\
                      ✶ Moseying… (8m 42s · ↓ 18.3k tokens)\n\
                      \x20 ⎿  Tip: You haven't used the ui-ux-pro-max plugin in a while.\n\
                      \x20    with /plugin\n\
                      ────────────────────────────────────────\n\
                      ❯ \n\
                      ────────────────────────────────────────\n\
                      \x20 ⏵⏵ bypass permissions on (shift+tab to cycle) · esc to interrupt · ← for agents";
        let detection = detect("claude", screen).unwrap();
        assert_eq!(detection.status, AgentStatus::Working, "rule={}", detection.rule_id);
    }

    #[test]
    fn real_claude_idle_chrome_reads_idle() {
        // 回合已结束：没有中断提示，输入框空着。输入框可见本身不是判据——
        // 上面那段工作中的屏幕里 ❯ 同样在。
        let screen = "✻ Baked for 15m 24s\n\
                      ────────────────────────────────────────\n\
                      ❯ \n\
                      ────────────────────────────────────────\n\
                      \x20 ⏸ manual mode on · ? for shortcuts · ← for agents";
        let detection = detect("claude", screen).unwrap();
        assert_eq!(detection.status, AgentStatus::Idle, "rule={}", detection.rule_id);
    }

    #[test]
    fn real_claude_permission_form_reads_blocked() {
        // 真的停在权限框上。这段同时含「Esc to cancel」，共享 working 规则
        // 必须被它的 not 段拦住，否则「等你点头」会被当成「正在干活」。
        let screen = " List dist artifacts and query GitHub release assets\n\
                      \x20This command requires approval\n\
                      \x20Do you want to proceed?\n\
                      \x20❯ 1. Yes\n\
                      \x20  2. Yes, and don't ask again for: gh release *\n\
                      \x20  3. No\n\
                      \x20Esc to cancel · Tab to amend · ctrl+e to explain";
        let detection = detect("claude", screen).unwrap();
        assert_eq!(detection.status, AgentStatus::Blocked, "rule={}", detection.rule_id);
    }

    #[test]
    fn codex_queue_and_compaction_are_live_work_only_in_the_chrome_tail() {
        for chrome in [
            "• Working (12s · esc to interrupt)\n• Messages to be submitted after next tool call (press esc to interrupt and send immediately)\n  ↳ Continue the task",
            "• Compacting context (1m 41s · esc to interrupt)\n  └ Making room to continue.",
        ] {
            let frame = format!("{chrome}\n› Ask Codex to do anything\ngpt-6 max · /project");
            assert_eq!(detect("codex", &frame).unwrap().status, AgentStatus::Working);
            let stale = format!(
                "{chrome}\n{}› Ask Codex to do anything\ngpt-6 max · /project",
                "completed output\n".repeat(20)
            );
            assert_eq!(detect("codex", &stale).unwrap().status, AgentStatus::Idle);
        }
        let quoted = "The log said Compacting context; Messages to be submitted after next tool call\n› Ask Codex to do anything\ngpt-6 max · /project";
        assert_eq!(detect("codex", quoted).unwrap().status, AgentStatus::Idle);
    }

    #[test]
    fn real_codex_idle_chrome_reads_idle() {
        // codex 早已答完，屏幕上就是空闲输入框。这块屏幕当初根本没人去匹:
        // running_program 是 None，1 Hz 看门狗在入口就早退了，于是转圈长挂。
        let screen = "  代码级修复和带 gpui-shell 的编译、针对性测试已经通过。\n\
                      › Improve documentation in @filename\n\
                      \x20 gpt-5.6-sol xhigh · D:\\temp_build\\nebula";
        let detection = detect("codex", screen).unwrap();
        assert_eq!(detection.status, AgentStatus::Idle, "rule={}", detection.rule_id);
    }

    /// 下面六段是 2026-09-17 用 pebrel 1.8.1 的 runtime `pane.read` 从 kimi
    /// 0.43.1 真实窗口抓的原文(tmp/pane11*.json)的精简转写,也就是判错的
    /// 现场。规则改动必须对着它们回归,不能对着记忆里的格式写。
    #[test]
    fn real_kimi_working_chrome_reads_working() {
        // 回合进行中:输入框正上方挂月相 spinner 行(U+1F311–U+1F318 逐帧轮转,
        // pane11-w2–w8),回合结束该行被重绘消失。输入框与状态行此时同样在
        // 场,idle 规则必须输给 working。
        let screen = "✨ 写一首关于秋天的四行短诗,直接输出诗,不要解释\n\
                      \x20 🌕 · Tip: /tasks to check progress and status for background tasks\n\
                      \x20╭─────────────────────────────────────╮\n\
                      \x20│ >                                    │\n\
                      \x20╰─────────────────────────────────────╯\n\
                      \x20K3 thinking: high  D:\\pebrel\\tmp  feat/kimi-hooks-20260917\n\
                      \x20                                       context: 4% (33.4k/1M)";
        let detection = detect("kimi", screen).unwrap();
        assert_eq!(detection.status, AgentStatus::Working, "rule={}", detection.rule_id);
        assert_eq!(detection.rule_id, "moon_spinner_working");
    }

    #[test]
    fn real_kimi_idle_chrome_reads_idle() {
        // 回合已结束(pane11-idle.json):月相行消失,只剩历史回答、空输入框
        // 和状态行。状态行的「K3 thinking: high」是模型名 + 思考档位,
        // 「context: N%」是用量条,都不是 working 证据。
        let screen = "● 叶落知秋深,\n\
                      \x20  风凉染层林。\n\
                      \x20  雁过长天远,\n\
                      \x20  霜轻月色沉。\n\
                      \x20╭─────────────────────────────────────╮\n\
                      \x20│ >                                    │\n\
                      \x20╰─────────────────────────────────────╯\n\
                      \x20K3 thinking: high  D:\\pebrel\\tmp  feat/kimi-hooks-20260917\n\
                      \x20                                       context: 4% (33.6k/1M)";
        let detection = detect("kimi", screen).unwrap();
        assert_eq!(detection.status, AgentStatus::Idle, "rule={}", detection.rule_id);
        assert_eq!(detection.rule_id, "prompt_idle");
    }

    #[test]
    fn real_kimi_approval_panel_reads_blocked() {
        // 真实审批面板(pane11-blocked.json):面板取代输入框固定在底部,上下
        // 各一条 ─ 横线;问句是大写 R 的「▶ Run this command?」,底行是
        // 「↑/↓ select · 1/2/3/4 choose · ↵ confirm」——旧规则写的小写
        // 问句、无斜杠 ↑↓ 和 esc cancel 都对不上这块面板。
        let screen = "● Running a command · $ echo hello-kimi-probe\n\
                      \x20  Press Ctrl+B to run in background\n\
                      \x20──────────────────────────────────────\n\
                      \x20  ▶ Run this command?\n\
                      \n\
                      \x20  cwd: D:/Documents/HTA/My Projects/pebrel/tmp\n\
                      \x20  $ echo hello-kimi-probe\n\
                      \n\
                      \x20  ▶ 1. Approve once\n\
                      \x20    2. Approve for this session\n\
                      \x20    3. Reject\n\
                      \x20    4. Reject with feedback\n\
                      \n\
                      \x20  ↑/↓ select · 1/2/3/4 choose · ↵ confirm\n\
                      \x20──────────────────────────────────────\n\
                      \x20K3 thinking: high  D:\\pebrel\\tmp  feat/kimi-hooks-20260917  ctrl+o expand\n\
                      \x20                                       context: 4% (33.7k/1M)";
        let detection = detect("kimi", screen).unwrap();
        assert_eq!(detection.status, AgentStatus::Blocked, "rule={}", detection.rule_id);
        assert_eq!(detection.rule_id, "approval_panel_blocked");
    }

    #[test]
    fn real_kimi_thinking_preview_reads_working() {
        // 回合内的另一种 working 画面(pane11-x5.json):没有月相行,盲文
        // spinner + 流式思考预览悬在输入框正上方。盲文行锚定底部后才是有效
        // 证据,这条把它钉住。
        let screen = "    /exit\n\
                      \n\
                      \x20 ⠙ thinking…\n\
                      \x20  User input weird: \"D:/Git/exit\n\
                      \x20  /exit\". Likely trying to exit\n\
                      \x20╭─────────────────────────────────────╮\n\
                      \x20│ >                                    │\n\
                      \x20╰─────────────────────────────────────╯\n\
                      \x20K3 thinking: high  D:\\pebrel\\tmp  feat/kimi-hooks-20260917\n\
                      \x20                                       context: 4% (33.8k/1M)";
        let detection = detect("kimi", screen).unwrap();
        assert_eq!(detection.status, AgentStatus::Working, "rule={}", detection.rule_id);
        assert_eq!(detection.rule_id, "braille_thinking_working");
    }

    #[test]
    fn kimi_stale_braille_thinking_line_does_not_read_working() {
        // 回合结束后,「⠙ thinking…」预览被最终回答替换,只可能留在滚屏高处
        // (pane11.json 里旧思考内容距底部 10+ 非空行)。陈旧盲文行没有月相行
        // 陪同、又不在底部窗口内时,不得把屏幕拖回 Working。
        let screen = " ⠙ thinking…\n\
                      \x20  User input weird: \"D:/Git/exit\n\
                      \x20  /exit\". Likely trying to exit\n\
                      \n\
                      \x20● Bash 工具没有运行成功:用户拒绝了执行批准请求,没有产生输出。\n\
                      \n\
                      \x20╭─────────────────────────────────────╮\n\
                      \x20│ >                                    │\n\
                      \x20╰─────────────────────────────────────╯\n\
                      \x20K3 thinking: high  D:\\pebrel\\tmp  feat/kimi-hooks-20260917\n\
                      \x20                                       context: 4% (33.8k/1M)";
        let detection = detect("kimi", screen).unwrap();
        assert_eq!(detection.status, AgentStatus::Idle, "rule={}", detection.rule_id);
        assert_eq!(detection.rule_id, "prompt_idle");
    }

    #[test]
    fn kimi_welcome_box_identifies_the_cli() {
        // 「Welcome to Kimi Code!」与「Send /help for help information.」两条
        // 顶行文字在 kimi.exe 0.43.1 二进制内核验;30 行 capture 窗口
        // (pane11.json)只留下框尾和下面的 Web UI 推广行,后者同样品牌独有。
        let welcome = "╭─────────────────────────────────────╮\n\
                       \x20│   Welcome to Kimi Code!                │\n\
                       \x20│   Send /help for help information.     │\n\
                       \x20╰─────────────────────────────────────╯";
        assert_eq!(identify(welcome), Some(AgentKind::Kimi));
        let tail = " │  Version:   0.43.1\n\
                    │  MCP:       1 connected\n\
                    ╰──────────────────────────────────────────────╯\n\
                    ✦ Try Kimi Code Web UI - clearer task progress and settings management";
        assert_eq!(identify(tail), Some(AgentKind::Kimi));
        assert_eq!(identify("the docs say welcome to kimi code, warmly"), None);
        assert_eq!(identify("user@host:~$ \n> generic prompt"), None);
    }

    #[test]
    fn shared_rules_light_up_agents_without_their_own_working_rule() {
        // 归一化的意义：中断提示对每个注册的 CLI 都点亮 working，新接的 CLI
        // 不必从零再写一遍 spinner 正则（cline 至今就没有 working 规则）。
        let screen = "some output\n────────\n> \n  esc to interrupt · ? for shortcuts";
        for agent in ["grok", "pi", "opencode", "gemini", "cline", "kilo", "kimi"] {
            let found =
                detect(agent, screen).unwrap_or_else(|| panic!("{agent} produced no detection"));
            assert_eq!(found.status, AgentStatus::Working, "{agent} rule={}", found.rule_id);
        }
    }

    #[test]
    fn shared_blocked_rules_outrank_shared_working_on_confirmation_forms() {
        // 确认框里几乎总有中断词；共享层必须自己把这一对张力解开，不能指望
        // 每个 CLI 都记得写 blocked 规则。
        let screen = "Run rm -rf /tmp/x ?\n  Do you want to proceed?\n  1. Yes\n  2. No\n\
                      esc to cancel · enter to confirm";
        for agent in ["grok", "pi", "cline", "kilo", "kimi"] {
            let found =
                detect(agent, screen).unwrap_or_else(|| panic!("{agent} produced no detection"));
            assert_eq!(found.status, AgentStatus::Blocked, "{agent} rule={}", found.rule_id);
        }
    }

    #[test]
    fn real_antigravity_working_chrome_reads_working() {
        let screen = "  ⠋ Thinking... (8s)\n\
                      ────────────────────────────────────────\n\
                      › \n\
                      ────────────────────────────────────────\n\
                      \x20 Plan mode: research & plan only (shift+tab to cycle) · Press esc to interrupt generation";
        let detection = detect("antigravity", screen).unwrap();
        assert_eq!(detection.status, AgentStatus::Working, "rule={}", detection.rule_id);
    }

    #[test]
    fn real_antigravity_idle_chrome_reads_idle() {
        let screen = "  I have completed the task and updated the configuration.\n\
                      ────────────────────────────────────────\n\
                      › \n\
                      ────────────────────────────────────────\n\
                      \x20 Plan mode: research & plan only (shift+tab to cycle) · ? for shortcuts";
        let detection = detect("antigravity", screen).unwrap();
        assert_eq!(detection.status, AgentStatus::Idle, "rule={}", detection.rule_id);
    }

    #[test]
    fn antigravity_old_scrollback_spinner_does_not_prevent_idle() {
        // 历史滚屏中留有上一轮的思考转圈符号；region 限制在底部后不能把已结束的回合误判为 Working。
        let screen = "  ⠋ Thinking... (from previous turn 5m ago)\n\
                      \x20 Result: success\n\
                      \x20 Line 1\n\
                      \x20 Line 2\n\
                      \x20 Line 3\n\
                      \x20 Line 4\n\
                      \x20 Line 5\n\
                      \x20 Line 6\n\
                      \x20 Line 7\n\
                      \x20 Line 8\n\
                      \x20 Line 9\n\
                      \x20 Line 10\n\
                      ────────────────────────────────────────\n\
                      › \n\
                      ────────────────────────────────────────\n\
                      \x20 Accept-edits mode: file edits auto-approved (shift+tab to cycle)";
        let detection = detect("antigravity", screen).unwrap();
        assert_eq!(detection.status, AgentStatus::Idle, "rule={}", detection.rule_id);
    }

    #[test]
    fn real_antigravity_permission_form_reads_blocked() {
        let screen = " requesting permission for:\n\
                      \x20 Run command: cargo test\n\
                      \x20 Do you want to proceed?\n\
                      \x20 › 1. Yes, allow\n\
                      \x20   2. No, deny access\n\
                      \x20 esc to cancel · tab amend";
        let detection = detect("antigravity", screen).unwrap();
        assert_eq!(detection.status, AgentStatus::Blocked, "rule={}", detection.rule_id);
    }

    #[test]
    fn antigravity_previous_confirmation_does_not_override_current_prompt() {
        for phrase in [
            "The tool requires approval before running (y/n).",
            "Use enter to confirm or tab to amend the command.",
            "Do you want to proceed?",
            "waiting for permission",
            "requesting permission for: cargo test\n[y] Yes\nesc to cancel · enter to confirm",
        ] {
            for (activity, footer, expected) in [
                ("", "? for shortcuts", AgentStatus::Idle),
                ("", "esc to interrupt", AgentStatus::Working),
                ("⠋ Thinking... (8s)\n", "esc to interrupt", AgentStatus::Working),
            ] {
                let screen = format!("{phrase}\n{activity}────────\n› \n────────\n{footer}");
                let detection = detect("antigravity-cli", &screen).unwrap();
                assert_eq!(detection.status, expected, "{screen}: rule={}", detection.rule_id);
            }
        }
    }

    #[test]
    fn antigravity_working_footer_excludes_quoted_form_hints() {
        for footer in
            ["↑/↓ Navigate · enter Select · esc Skip", "↑/↓ Navigate · tab Amend · f full diff"]
        {
            let working = format!(
                "The footer reads:\n{footer}\nand that means a choice is pending.\n⠋ Thinking... (2s)\nesc to interrupt"
            );
            assert_eq!(detect("agy", &working).unwrap().status, AgentStatus::Working);
            let approval = format!("Previous output: esc to interrupt\n{footer}\nesc to cancel");
            assert_eq!(detect("agy", &approval).unwrap().status, AgentStatus::Blocked);
        }
    }

    #[test]
    fn prompt_drafts_need_input_box_context() {
        for row in ["> fix the tests please", "│ › fix the tests │", "› Ask anything…"] {
            let screen =
                format!("Do you want to proceed?\n────────\n{row}\n────────\n? for shortcuts");
            assert_eq!(detect("agy", &screen).unwrap().status, AgentStatus::Idle);
        }
        for row in
            ["> Yes, I trust this folder", "> Allow once", "> [ ] 中文", "> 1) Yes", "> 1. Yes"]
        {
            let screen = format!("Do you want to proceed?\n{row}\nesc to cancel");
            assert_eq!(from_last_empty_prompt(&screen), screen);
        }
    }

    #[test]
    fn malformed_prompt_regions_are_rejected_at_load() {
        for region in [
            "after_last_prompt_row()",
            "after_last_prompt_row(",
            "after_last_prompt_row(unknown)",
            "after_last_prompt_row(bottom_non_empty_lines(0))",
        ] {
            let source = format!(
                "id = 'custom'\n[[rules]]\nid = 'test'\nstate = 'idle'\nregion = '{region}'\ncontains = ['ready']"
            );
            assert!(compile_manifest(&source, None).unwrap_err().contains("invalid prompt region"));
        }
    }

    #[test]
    fn prompt_regions_are_explicit_and_preserve_unknown_input() {
        let screen = "Do you want to proceed?\n│ ›   │\n? for shortcuts";
        assert_eq!(
            region(screen, "after_last_prompt_row(bottom_non_empty_lines(12))"),
            "│ ›   │\n? for shortcuts"
        );
        for row in ["› Ask anything…", "> 1. Yes", "› unfinished draft"] {
            let screen = format!("Do you want to proceed?\n{row}");
            assert_eq!(from_last_empty_prompt(&screen), screen);
        }
        let source = "id = 'custom'\nshared_rule_region = 'after_last_prompt_row'\n\
            [[rules]]\nid = 'shared_custom'\nstate = 'blocked'\n\
            region = 'whole_recent'\ncontains = ['approval']";
        let compiled = compile_manifest(source, None).unwrap();
        assert_eq!(compiled.manifest.rules[0].region, "whole_recent");
        assert_eq!(
            compiled.manifest.rules[1].region,
            "after_last_prompt_row(bottom_non_empty_lines(1))"
        );
        assert!(
            compile_manifest(&source.replace("after_last_prompt_row", "unknown"), None).is_err()
        );
    }

    #[test]
    fn antigravity_long_forms_and_extended_footers_still_block() {
        for (footer, rule) in [
            ("↑/↓ Navigate · enter Select · esc Skip", "question_selection"),
            ("↑/↓ Navigate · tab Amend · f full diff", "file_review"),
        ] {
            let screen = format!(
                "Question 1/1: Choose\n{}\n{footer} · ctrl+g expand\nesc to cancel",
                "long wrapped option\n".repeat(20)
            );
            let found = detect("agy", &screen).unwrap();
            assert_eq!(found.status, AgentStatus::Blocked);
            assert_eq!(found.rule_id, rule);
            // Even an exact standalone footer quoted in an answer is history
            // once the current input prompt is visible.
            for prompt in [">", "│ ›   │"] {
                let answered = format!("{screen}\n{prompt}\n? for shortcuts");
                assert_eq!(detect("agy", &answered).unwrap().status, AgentStatus::Idle);
            }
        }
    }

    #[test]
    fn antigravity_real_command_permission_requires_attention() {
        let screen = "Command\n────────\nRequesting permission for:\n\
            cmd /c echo agy-command-probe\nDo you want to proceed?\n\
            > 1. Yes\n\
            2. Yes, and always allow in this conversation for commands that start with 'cmd'\n\
            3. Yes, and always allow for commands that start with 'cmd' (Persist to settings.json)\n\
            4. No\n↑/↓ Navigate · tab Amend · ctrl+g edit/expand command\n\
            esc to cancel                         Gemini 3.8 Flash · high";
        assert_eq!(detect("agy", screen).unwrap().status, AgentStatus::Blocked);
    }

    #[test]
    fn antigravity_real_file_creation_review_requires_attention() {
        let review = "Create file\n────────\nagy-permission-check.txt +1\n\
            1 + permission probe\nAllow creation of this file?\n\
            > 1. Yes, allow creation\n  2. No, deny creation\n\
            ↑/↓ Navigate · tab Amend · f full diff\n\
            esc to cancel                         Gemini 3.8 Flash · high";
        let detection = detect("agy", review).unwrap();
        assert_eq!(detection.status, AgentStatus::Blocked);
        assert_eq!(detection.rule_id, "file_review");
        let edit = review
            .replace("Allow creation of this file?", "Accept this file edit?")
            .replace("Yes, allow creation", "Yes, accept this change")
            .replace("No, deny creation", "No, reject this change");
        assert_eq!(detect("agy", &edit).unwrap().status, AgentStatus::Blocked);
        let accepted = format!("{review}\nCreated file.\n────────\n>\n────────\n? for shortcuts");
        assert_eq!(detect("agy", &accepted).unwrap().status, AgentStatus::Idle);
        let automatic = "Created agy-permission-check.txt\n────────\n>\n────────\n? for shortcuts";
        assert_eq!(detect("agy", automatic).unwrap().status, AgentStatus::Idle);
    }

    #[test]
    fn antigravity_real_ask_question_overrides_cancel_working_hint() {
        // Captured from agy 1.1.27 in a Windows ConPTY, before choosing Chinese.
        let question = "Question\n────────\n\
            Question 1/1: 请选择接下来交流使用的语言 / Please select the language to use:\n\
            > 1. 中文\n  2. English\n  3. Write-in...\n\
            ↑/↓ Navigate · enter Select · esc Skip\n\
            esc to cancel                         Gemini 3.8 Flash · high";
        let detection = detect("agy", question).unwrap();
        assert_eq!(detection.status, AgentStatus::Blocked);
        assert_eq!(detection.rule_id, "question_selection");

        let answered = format!("{question}\n已选择中文。\n────────\n>\n────────\n? for shortcuts");
        assert_eq!(detect("agy", &answered).unwrap().status, AgentStatus::Idle);
        let prose = "The shortcuts are ↑/↓ Navigate · enter Select · esc Skip\n>\n? for shortcuts";
        assert_eq!(detect("agy", prose).unwrap().status, AgentStatus::Idle);
    }

    #[test]
    fn antigravity_active_confirmation_after_prompt_still_blocks() {
        let screen = "› \nrequesting permission for: cargo test\n› 1. Yes, allow\n\
                      2. No\nesc to cancel · enter to confirm";
        let detection = detect("antigravity-cli", screen).unwrap();
        assert_eq!(detection.status, AgentStatus::Blocked, "rule={}", detection.rule_id);
    }

    #[test]
    fn antigravity_prose_mentioning_approval_while_idle_reads_idle() {
        // 回答正文中提及 "needs approval for ..."，但此时已完成并显示空闲提示符，
        // 不得误判为 Blocked。
        for phrase in
            ["needs approval for", "requesting approval for", "requesting permission for:"]
        {
            let screen = format!(
                "│ The tool {phrase} operations outside the workspace.\n\
                 │ Please confirm your settings if you plan to enable this.\n\
                 ────────────────────────────────────────\n\
                 › \n\
                 ────────────────────────────────────────\n\
                 \x20 Plan mode: research & plan only (shift+tab to cycle) · ? for shortcuts"
            );
            let detection = detect("antigravity", &screen).unwrap();
            assert_eq!(detection.status, AgentStatus::Idle, "{phrase}: rule={}", detection.rule_id);
        }
    }
}
