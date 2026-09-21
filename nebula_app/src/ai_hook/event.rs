//! Normalized provider facts. No terminal scanning, I/O or pane mutation.

use super::payload::{MESSAGE_MAX_CHARS, truncate};

/// What a lifecycle event means for the pane's turn state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AiHookKind {
    /// Agent process/session became live; usually the earliest session-id edge.
    SessionStart,
    /// The user submitted a prompt: a turn is running.
    PromptSubmit,
    /// A tool completed; clears a stale permission/question wait.
    ToolComplete,
    /// The turn finished; the CLI waits for the next instruction.
    TurnDone,
    /// An explicit permission request or input question is blocking the CLI.
    NeedsAttention,
    /// Agent session shut down and no longer owns the pane.
    SessionEnd,
}

/// A stopped turn is not necessarily a successful answer. Only explicit
/// provider result metadata may classify it; never scan assistant prose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiTurnOutcome {
    /// Older Pi bridges omit result metadata; preserve their activity edge.
    Unspecified,
    Succeeded,
    Failed,
    Cancelled,
    Incomplete,
    Unknown,
}

/// Provider 的 Hook 能力并不对称。这里描述 Nebula 当前实际安装的桥接能力，
/// 避免上层把“有生命周期 Hook”误当成“也有权限上下文或事件顺序保证”。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AiHookCapabilities {
    /// The installed bridge reports turn starts as well as completions.
    pub lifecycle: bool,
    /// The bridge also reports explicit waiting/resumption. This is separate
    /// from turn boundaries: Pi's extension has no permission callback.
    pub attention_events: bool,
    pub attention_context: bool,
    pub background_tasks: bool,
    /// Nebula 自己的 bridge 是否为事件盖了单调序号。**没有任何 provider 提供
    /// 原生顺序字段**：opencode/pi 的序号由我们注入的 plugin/extension 生成
    /// （启动纪元 × 1e6 + 自增），claude/codex 的 hook 完全没有顺序信息，只能
    /// 依赖本地到达顺序。名字里是 bridge 而不是 provider，正是这个原因。
    pub bridge_sequence: bool,
    pub serialized_delivery: bool,
}

/// Versioned contract of the installed Codex command hooks. Legacy notify has
/// no start/permission authority; it remains available until native hooks run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CodexHookMode {
    Turns,
    Full,
}

pub fn capabilities_for(source: &str) -> AiHookCapabilities {
    match source {
        "claude" => AiHookCapabilities {
            lifecycle: true,
            attention_events: true,
            attention_context: true,
            background_tasks: true,
            bridge_sequence: false,
            serialized_delivery: false,
        },
        "opencode" => AiHookCapabilities {
            lifecycle: true,
            attention_events: true,
            attention_context: true,
            background_tasks: false,
            bridge_sequence: true,
            serialized_delivery: true,
        },
        "pi" => AiHookCapabilities {
            lifecycle: true,
            attention_events: false,
            attention_context: false,
            background_tasks: false,
            bridge_sequence: true,
            serialized_delivery: false,
        },
        // Codex notify 当前只给 turn-complete；没有 permission payload，也没有
        // 可验证的 provider sequence。接收顺序只能代表本机实际到达顺序。
        "codex" => AiHookCapabilities {
            lifecycle: false,
            attention_events: false,
            attention_context: false,
            background_tasks: false,
            bridge_sequence: false,
            serialized_delivery: false,
        },
        "kimi" => AiHookCapabilities {
            lifecycle: true,
            attention_events: true,
            attention_context: true,
            background_tasks: false,
            bridge_sequence: false,
            serialized_delivery: false,
        },
        _ => AiHookCapabilities {
            lifecycle: false,
            attention_events: false,
            attention_context: false,
            background_tasks: false,
            bridge_sequence: false,
            serialized_delivery: false,
        },
    }
}

/// Agent 当前的权限档位。**这不是「正在等你批准」**，两者必须分开：
///
/// * `BypassPermissions` 是一个持续状态——用户用 `--dangerously-skip-permissions`
///   起的会话根本不会来问，把它当成 awaiting 会让徽标永远误亮；
/// * `NeedsAttention` 是一次瞬时事件，只有真的卡住等人时才发。
///
/// 合起来才能正确回答「这个 pane 现在是不是在无人监督地改我的仓库」。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AiPermissionMode {
    /// 每次动作都会问。
    Default,
    /// 自动同意文件编辑，其余仍会问。
    AcceptEdits,
    /// 全部跳过：不会有任何权限请求抵达。
    BypassPermissions,
    /// 只读规划，不落盘。
    Plan,
}

impl AiPermissionMode {
    /// Claude 的 hook 载荷直接带 `permission_mode`，这比读 agent 进程的 argv
    /// 可靠得多——会话中途切档、包装脚本启动、`npx claude` 都不会反映在命令行
    /// 里。Windows 上读别的进程命令行还要 WMI 或 PEB 遍历，代价与收益完全不成
    /// 比例，所以这里只认 provider 自己声明的值，读不到就是 `None`。
    pub(super) fn parse(raw: &str) -> Option<Self> {
        match raw.trim() {
            "default" => Some(Self::Default),
            "acceptEdits" | "accept_edits" => Some(Self::AcceptEdits),
            "bypassPermissions" | "bypass_permissions" => Some(Self::BypassPermissions),
            "plan" => Some(Self::Plan),
            _ => None,
        }
    }

    /// 这一档会不会产生权限请求。用来判断一个没有明确类型的通知该不该被解释
    /// 成「等你批准」。
    pub fn can_ask_for_permission(self) -> bool {
        !matches!(self, Self::BypassPermissions)
    }
}

/// Claude `Stop` 会携带本回合的后台 Task 列表。主回合停止不等于后台
/// subagent 已经停止；至少一个 task 仍 running 时，Pane 必须保持 Working。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AiBackgroundTasks {
    pub active: u32,
    pub total: u32,
}

/// 权限/等待输入事件的可行动上下文。`raw_context` 是经过字段脱敏、深度和
/// 体积限制的副本；它绝不能等同于 provider 的原始 stdin，也不能进 Debug 日志。
#[derive(Clone)]
pub struct AttentionContext {
    pub source: String,
    pub pane_id: Option<u64>,
    pub session_id: Option<String>,
    pub event_kind: AiHookKind,
    pub event_id: Option<String>,
    pub bridge_sequence: Option<u64>,
    /// Provider 声明的 Unix 时间戳（毫秒）；没有可靠字段时保持 None。
    pub occurred_at_ms: Option<u64>,
    /// Nebula 完成 envelope 解析的 Unix 时间戳（毫秒）。
    pub received_at_ms: u64,
    pub cwd: Option<String>,
    pub project: Option<String>,
    pub git_branch: Option<String>,
    pub permission_or_tool: Option<String>,
    /// 事件抵达时 agent 声明的权限档位。`BypassPermissions` 时这条 attention
    /// 一定不是「等你批准」（那种会话不会来问），只可能是等你输入——UI 的文案
    /// 必须据此区分，否则用户会以为有个批准按钮在等他。
    pub permission_mode: Option<AiPermissionMode>,
    pub message: Option<String>,
    pub selection: Option<String>,
    pub raw_context: Option<String>,
}

impl std::fmt::Debug for AttentionContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AttentionContext")
            .field("source", &self.source)
            .field("pane_id", &self.pane_id)
            .field("session_id", &self.session_id)
            .field("event_kind", &self.event_kind)
            .field("event_id", &self.event_id)
            .field("bridge_sequence", &self.bridge_sequence)
            .field("occurred_at_ms", &self.occurred_at_ms)
            .field("received_at_ms", &self.received_at_ms)
            .field("cwd", &self.cwd)
            .field("project", &self.project)
            .field("git_branch", &self.git_branch)
            .field("permission_or_tool", &self.permission_or_tool)
            .field("permission_mode", &self.permission_mode)
            .field("message", &self.message)
            .field("selection_chars", &self.selection.as_ref().map(|s| s.chars().count()))
            .field("raw_context_bytes", &self.raw_context.as_ref().map(String::len))
            .finish()
    }
}

impl AttentionContext {
    /// 通知正文只放定位和请求摘要；选区正文、raw context 从不进入系统通知。
    pub fn summary_for_pane(&self, pane_id: u64) -> String {
        let mut parts = Vec::with_capacity(4);
        if let Some(project) = self.project.as_deref().or(self.cwd.as_deref()) {
            parts.push(truncate(project, 120));
        }
        parts.push(format!("Pane {pane_id}"));
        if let Some(request) = self.permission_or_tool.as_deref() {
            parts.push(truncate(request, 100));
        }
        if let Some(message) = self.message.as_deref() {
            let message = truncate(message, MESSAGE_MAX_CHARS);
            if !parts.iter().any(|part| part == &message) {
                parts.push(message);
            }
        }
        if self.selection.is_some() {
            parts.push("selection context".to_owned());
        }
        parts.join(" · ")
    }
}

/// A typed AI-CLI lifecycle event, parsed from one pipe connection.
#[derive(Debug, Clone)]
pub struct AiHookEvent {
    /// Older Claude notifications omit their type. They may signal attention
    /// during work, but cannot reopen an idle/completed turn.
    pub(super) legacy_attention: bool,
    pub(crate) codex_hooks: Option<CodexHookMode>,
    /// Identity supplied by our bridge on an authenticated SSH channel. Kept
    /// separate from local kernel PIDs; never used for host process queries.
    pub(crate) remote_process: Option<String>,
    pub(crate) turn_id: Option<String>,
    /// Compaction starts hooks inside an existing turn; it is not an idle edge.
    pub(crate) session_compacted: bool,
    pub answer: Option<crate::assistant_answer::AssistantAnswer>,
    pub answer_cwd: Option<std::path::PathBuf>,
    /// Pane hosting the CLI (from `NEBULA_PANE_ID`); `None` falls back to the
    /// focused pane (only happens when the env was stripped along the way).
    pub pane: Option<u64>,
    /// AI CLI identity, used as the toast title.
    pub source: String,
    pub kind: AiHookKind,
    pub turn_outcome: AiTurnOutcome,
    /// Human text when the event carries one (claude's notification message,
    /// codex's last assistant message).
    pub message: Option<String>,
    /// CLI 自己的会话身份：claude hook 载荷的 `session_id`、codex notify 的
    /// `thread-id`（即 rollout 文件名尾部的 uuid，`codex resume` 认它）。
    /// 冷恢复接续对话的唯一事实源——文件系统扫描只能靠 mtime 猜。
    pub session_id: Option<String>,
    /// Exact native file and owner lifetime used by durable session recovery.
    pub session_file: Option<String>,
    pub bridge_instance: Option<String>,
    /// Provider 给出的幂等身份；只在同一 source/session/pane 内去重。
    pub event_id: Option<String>,
    /// Nebula bridge 盖的单调序号（不是 provider 原生顺序）。没有时只承诺
    /// 本地接收顺序。
    pub bridge_sequence: Option<u64>,
    pub occurred_at_ms: Option<u64>,
    pub received_at_ms: u64,
    /// 进程内严格单调的接收序号，用于稳定批处理；不冒充 provider 顺序。
    pub received_sequence: u64,
    /// 写入命名管道的那个进程的真实 pid，由内核回答
    /// （`GetNamedPipeClientProcessId`），不是载荷里自报的。路由的第二因子：
    /// 校验它是否真的跑在声明的 pane 进程树内。远端 SSH 走 OSC 通道，没有本地
    /// 客户端进程，因此恒为 `None`。
    pub client_pid: Option<u32>,
    /// 沿 [`Self::client_pid`] 祖先链找到的最近 AI CLI 进程的 pid。这是 agent 的
    /// **进程身份**，用来区分嵌套子代理：`claude -p` 起的子代理有自己的 pid，
    /// 它的 session id 却是短命的——一旦被当成 pane 的会话身份，就会把真正活着
    /// 的那个顶掉。远端 SSH 没有本地进程，恒为 `None`。
    pub agent_pid: Option<u32>,
    /// Agent 自己声明的权限档位。与 [`AiHookKind::NeedsAttention`] 是两件事：
    /// 前者是持续状态，后者是瞬时事件。
    pub permission_mode: Option<AiPermissionMode>,
    pub background_tasks: Option<AiBackgroundTasks>,
    pub attention: Option<AttentionContext>,
}

impl AiHookEvent {
    pub fn capabilities(&self) -> AiHookCapabilities {
        let mut capabilities = capabilities_for(&self.source);
        if let Some(mode) = self.codex_hooks {
            capabilities.lifecycle = true;
            capabilities.attention_events = mode == CodexHookMode::Full;
            capabilities.attention_context = mode == CodexHookMode::Full;
        }
        capabilities
    }

    pub fn active_background_tasks(&self) -> u32 {
        self.background_tasks.map_or(0, |tasks| tasks.active)
    }
}
