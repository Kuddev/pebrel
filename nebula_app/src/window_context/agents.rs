//! legacy 壳向托盘投影的 Agent 视图。

use super::*;

impl WindowContext {
    /// 托盘与侧栏读取同一 pane 事实源，不维护第二份 Agent 状态。
    pub fn tray_agents(&self) -> Vec<crate::tray::TrayAgent> {
        self.panes
            .iter()
            .filter_map(|pane| {
                let state = &pane.nebula_state;
                let program = state
                    .running_program
                    .as_deref()
                    .filter(|program| crate::ai_agents::AgentKind::parse(program).is_some())?;
                // 项目目录比 pane id 更适合作为托盘中的人工识别信息。
                let place = std::path::Path::new(state.cwd.trim())
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let label = if place.is_empty() {
                    program.to_owned()
                } else {
                    format!("{program} · {place}")
                };
                Some(crate::tray::TrayAgent {
                    window: self.display.window.id(),
                    pane: pane.id,
                    label,
                    needs_attention: state.needs_attention,
                })
            })
            .collect()
    }

    pub(super) fn mark_pane_bell(&mut self, pane_id: PaneId) {
        let active = self.active_tab;
        let mut marked = false;
        for (i, t) in self.tabs.iter_mut().enumerate() {
            let mut ids = Vec::new();
            t.layout.leaves(&mut ids);
            if ids.contains(&pane_id) {
                if i != active && !t.has_bell {
                    t.has_bell = true;
                    marked = true;
                }
                break;
            }
        }
        if marked {
            // A bell in a BACKGROUND tab is invisible even with the window
            // focused (claude/codex finishing a turn there) — deliver the
            // system notification here. The window-unfocused case is handled
            // at the per-pane Bell event, so the two paths never double-ring.
            // Use the REAL window focus: a background pane's cached
            // `terminal.is_focused` starts true and may never see a focus
            // event, which would double-ring against the per-pane path.
            if self.display.window.has_focus() {
                let program =
                    self.pane(pane_id).and_then(|p| p.nebula_state.running_program.clone());
                crate::notify::deliver(
                    &self.display.window,
                    &crate::notify::Notification::Bell { program },
                    Some(pane_id),
                );
            }
            self.dirty = true;
        }
    }

    /// Apply a typed AI-CLI lifecycle event (claude/codex via the nebula-hook
    /// pipe) to its pane's turn state — the exact, edge-triggered version of
    /// what the BEL heuristics approximate. Returns `false` when the pane
    /// does not belong to this window so the processor can try the next one.
    pub fn handle_ai_hook(&mut self, ev: &crate::ai_hook::AiHookEvent) -> bool {
        // A missing pane id (env stripped by an intermediate layer) degrades
        // to the focused pane of the first window asked.
        let pane_id = ev.pane.unwrap_or_else(|| self.focused_pane_id());
        let Some(idx) = self.pane_index(pane_id) else { return false };
        // 路由第二因子：写管道那个进程必须真的跑在这个 pane 的进程树里。
        // `NEBULA_PANE_ID` 是环境变量，任何进程都能设成别的 pane；祖先链不能
        // 伪造。只有拿到明确反证时才拒绝，查不到证据（远端 OSC 通道、pane 还
        // 没有本地 shell、helper 已退出）一律放行。
        if let Some(client_pid) = ev.client_pid
            && ev.pane == Some(pane_id)
            && crate::process_tree::is_within_tree(client_pid, self.panes[idx].shell_pid)
                == Some(false)
        {
            log::warn!(
                "ai_hook: rejected event claiming pane {pane_id} from pid {client_pid} outside its \
                 process tree (source={} kind={:?})",
                ev.source,
                ev.kind
            );
            return true;
        }
        let verdict = crate::ai_hook::accept_for_pane(ev, pane_id);
        if !verdict.accepted() {
            log::debug!(
                "ai_hook: dropped event reason={verdict:?} source={} session={:?} pane={pane_id} \
                 kind={:?} bridge_seq={:?}",
                ev.source,
                ev.session_id,
                ev.kind,
                ev.bridge_sequence
            );
            return true;
        }

        // The hook names its client ("claude" / "codex") — ground truth for
        // the sidebar program icon, unlike the OSC 133 command-line sniffing
        // which misses wrapped launches and integration-less shells.
        {
            let state = &mut self.panes[idx].nebula_state;
            state.running_program = Some(ev.source.clone());
            state.agent_hook_seen = true;
            state.agent_status_source = crate::ai_agents::AgentStatusSource::Hook;
            state.agent_status_rule = None;
            if !matches!(ev.kind, crate::ai_hook::AiHookKind::SessionStart) {
                state.agent_runtime_submit_pending = false;
            }
            // 精确边沿抵达 = 屏幕检测的空闲计数作废（上一回合攒下的拍数
            // 不能把新回合的第一个空闲闪现立即降级）。
            state.idle_screen_streak = 0;
            // 会话身份跟着事件走：同一个 pane 里 /clear、重开会话都会带来
            // 新 id，最后一次上报永远是权威。133;D（CLI 退回提示符）清除。
            if let Some(id) = ev.session_id.as_deref() {
                state.ai_session = Some(crate::display::AiSessionIdentity {
                    source: ev.source.clone(),
                    session_id: id.to_owned(),
                });
            }
        }
        if let Some(id) = ev.session_id.as_deref() {
            let cwd = self.panes[idx].nebula_state.cwd.clone();
            if let Err(error) = crate::ai_sessions::record_hook_session(&ev.source, id, &cwd, None)
            {
                log::warn!("agent session index: could not record {} {id}: {error}", ev.source);
            }
        }

        match ev.kind {
            crate::ai_hook::AiHookKind::SessionStart => {
                let state = &mut self.panes[idx].nebula_state;
                state.agent_status = crate::ai_agents::AgentStatus::Idle;
                state.awaiting_input = true;
                state.needs_attention = false;
                state.command_started.get_or_insert_with(Instant::now);
            },
            crate::ai_hook::AiHookKind::PromptSubmit => {
                // A turn started: spinner resumes, stale dot is consumed.
                let state = &mut self.panes[idx].nebula_state;
                state.agent_status = crate::ai_agents::AgentStatus::Working;
                state.awaiting_input = false;
                state.needs_attention = false;
                state.finished_unseen = false;
                // No shell integration = no OSC 133;C ever ran: give the
                // spinner a start mark so the turn still animates.
                state.command_started.get_or_insert_with(std::time::Instant::now);
            },
            crate::ai_hook::AiHookKind::ToolComplete => {
                let state = &mut self.panes[idx].nebula_state;
                // 一个工具刚跑完 = agent 正在干活，这是无条件事实。此前只
                // 认 Blocked→Working，漏掉了「回合经不发 PromptSubmit 的路径
                // 继续（授权点头、队列消息）后状态还挂在 Done」的场景——
                // 也就是用户看到的「还在执行却已经显示完成蓝点」。
                state.agent_status = crate::ai_agents::AgentStatus::Working;
                state.awaiting_input = false;
                state.needs_attention = false;
                state.finished_unseen = false;
                state.command_started.get_or_insert_with(Instant::now);
            },
            crate::ai_hook::AiHookKind::TurnDone if ev.active_background_tasks() > 0 => {
                let active = ev.active_background_tasks();
                let state = &mut self.panes[idx].nebula_state;
                state.agent_status = crate::ai_agents::AgentStatus::Working;
                state.agent_status_rule = Some(format!("hook.background_tasks.active={active}"));
                state.awaiting_input = false;
                state.needs_attention = false;
                state.finished_unseen = false;
                state.command_started.get_or_insert_with(Instant::now);
            },
            crate::ai_hook::AiHookKind::TurnDone | crate::ai_hook::AiHookKind::NeedsAttention => {
                // codex 的 notify 只有"回合完成"一种事件：弹出交互式提问时
                // 它发的也是 turn-complete，事件流分不出"说完了"和"在等你
                // 回答"。回合结束的瞬间看一眼屏幕尾部——还挂着选择框或确认
                // 提示，就按「等你批准」处理（蓝点升级成手掌）。
                let screen_asks = ev.kind == crate::ai_hook::AiHookKind::TurnDone && {
                    // 与 GPUI 壳同判据：走 per-agent manifest 的 blocked 规则
                    // （带 region 锚定，只认当前活动框），不再拿裸关键词扫底部
                    // 15 行全文——正文里出现 (y/n)、do you want to proceed 之类
                    // 的字样（agent 打印的代码、上一轮没滚走的旧框）就会让正常
                    // 结束的回合挂上警告三角，而 Blocked 一旦点亮就再难落下。
                    let term = self.panes[idx].terminal.lock();
                    let lines = term.screen_lines();
                    lines > 0 && term.columns() > 0 && {
                        let start = Point::new(Line(0), Column(0));
                        let end = Point::new(
                            Line(lines as i32 - 1),
                            Column(term.columns().saturating_sub(1)),
                        );
                        let screen = term.bounds_to_string(start, end);
                        crate::ai_agents::detect(&ev.source, &screen).is_some_and(|detection| {
                            detection.status == crate::ai_agents::AgentStatus::Blocked
                        })
                    }
                };
                {
                    let state = &mut self.panes[idx].nebula_state;
                    state.agent_status =
                        if ev.kind == crate::ai_hook::AiHookKind::NeedsAttention || screen_asks {
                            crate::ai_agents::AgentStatus::Blocked
                        } else {
                            crate::ai_agents::AgentStatus::Done
                        };
                    state.awaiting_input = true;
                    state.finished_unseen = true;
                    // 「等你批准」是比「回合完成」更强的状态：它不是通知你
                    // 结果，是挡在半路要你点头。徽章上分成手掌与圆点两种
                    // 墨迹，此前两者共用一个点，界面上根本分不出来。
                    if ev.kind == crate::ai_hook::AiHookKind::NeedsAttention || screen_asks {
                        state.needs_attention = true;
                    }
                }
                // Tab dot when the pane sits in a background tab (same rule
                // as mark_pane_bell; the visible tab shows the pane itself).
                let mut background_tab = false;
                let active = self.active_tab;
                for (i, tab) in self.tabs.iter_mut().enumerate() {
                    let mut ids = Vec::new();
                    tab.layout.leaves(&mut ids);
                    if ids.contains(&pane_id) {
                        if i != active {
                            tab.has_bell = true;
                            background_tab = true;
                        }
                        break;
                    }
                }
                // Toast policy in one place: unfocused window, or focused
                // window with the pane hidden in a background tab. The global
                // toast throttle absorbs the BEL/OSC-9 double fire when
                // claude's notif channel is active as well.
                let attention =
                    ev.kind == crate::ai_hook::AiHookKind::NeedsAttention || screen_asks;
                if !self.display.window.has_focus() || background_tab {
                    let message = ev
                        .attention
                        .as_ref()
                        .map(|context| context.summary_for_pane(pane_id))
                        .or_else(|| ev.message.clone());
                    crate::notify::deliver(
                        &self.display.window,
                        &crate::notify::Notification::AiTurn {
                            program: ev.source.clone(),
                            message,
                            attention,
                        },
                        Some(pane_id),
                    );
                }
            },
            crate::ai_hook::AiHookKind::SessionEnd => {
                let state = &mut self.panes[idx].nebula_state;
                state.agent_status = crate::ai_agents::AgentStatus::Unknown;
                state.agent_status_source = crate::ai_agents::AgentStatusSource::Unknown;
                state.agent_status_rule = None;
                state.agent_hook_seen = false;
                state.agent_runtime_submit_pending = false;
                state.ai_session = None;
                state.running_program = None;
                state.pending_command_prompt = None;
                state.awaiting_input = false;
                state.needs_attention = false;
            },
        }

        self.dirty = true;
        self.display.window.request_redraw();
        true
    }

    /// 1 Hz 声明式屏幕检测。Hook 仍是精确边界；屏幕承担两类补位：
    ///
    /// 1. Gemini/Cursor/Copilot 等尚无 hook 桥接的客户端；
    /// 2. 可见的权限/问题框（比“turn complete”事件更能证明正在等人）。
    ///
    /// 只读底部 24 行，规则已预编译；普通 shell 或未知程序立即跳过。
    pub fn refresh_agent_screen_states(&mut self) {
        // Wakeup 不是所有 synchronized PTY 输出的必发事件；NebulaTick 在做
        // Agent 检测前先冲刷所有已看到 Grid 变化的 Runtime 提交 barrier。
        let pane_ids: Vec<_> = self.panes.iter().map(|pane| pane.id).collect();
        for pane_id in pane_ids {
            self.runtime_flush_pending_submit(Some(pane_id));
        }
        for pane in &mut self.panes {
            let (prompt_restored, screen) = {
                let term = pane.terminal.lock();
                let lines = term.screen_lines();
                if lines == 0 || term.columns() == 0 {
                    continue;
                }
                let prompt_restored =
                    pane.nebula_state.pending_command_prompt.as_deref().is_some_and(|expected| {
                        crate::display::nebula_shell_prompt_restored_from_raw_grid(
                            &term,
                            expected,
                            &pane.nebula_state.suggest_env,
                        )
                    });
                let take = lines.min(24);
                let start = Point::new(Line((lines - take) as i32), Column(0));
                let end =
                    Point::new(Line(lines as i32 - 1), Column(term.columns().saturating_sub(1)));
                (prompt_restored, term.bounds_to_string(start, end))
            };
            if prompt_restored
                && pane
                    .nebula_state
                    .running_program
                    .as_deref()
                    .and_then(crate::ai_agents::AgentKind::parse)
                    .is_some()
            {
                log::debug!(
                    "agent lifecycle: submitted shell prompt restored pane={} program={:?}",
                    pane.id,
                    pane.nebula_state.running_program
                );
                let state = &mut pane.nebula_state;
                if let Some(run) = state.active_run.take() {
                    state.last_run =
                        Some(crate::runtime_api::RuntimeRunOutcome::command_done(run, None));
                }
                state.running_program = None;
                state.ai_session = None;
                state.agent_status = crate::ai_agents::AgentStatus::Unknown;
                state.agent_status_source = crate::ai_agents::AgentStatusSource::Unknown;
                state.agent_status_rule = None;
                state.agent_hook_seen = false;
                state.idle_screen_streak = 0;
                state.agent_runtime_submit_pending = false;
                state.runtime_submit_barrier = None;
                state.command_started = None;
                state.pending_command_prompt = None;
                state.awaiting_input = false;
                state.finished_unseen = false;
                state.needs_attention = false;
                continue;
            }
            let program = match pane.nebula_state.running_program.clone() {
                Some(program) => program,
                None => {
                    let Some(agent) = crate::ai_agents::identify(&screen) else {
                        pane.nebula_state.idle_screen_streak = 0;
                        continue;
                    };
                    let program = agent.slug().to_owned();
                    log::debug!("agent identity from screen: pane={} program={program}", pane.id);
                    pane.nebula_state.running_program = Some(program.clone());
                    pane.nebula_state.agent_status_source =
                        crate::ai_agents::AgentStatusSource::Screen;
                    pane.nebula_state.agent_status_rule = None;
                    program
                },
            };
            if crate::ai_agents::AgentKind::parse(&program).is_none() {
                continue;
            }
            let Some(detection) = crate::ai_agents::detect(&program, &screen) else {
                continue;
            };

            let state = &mut pane.nebula_state;
            if detection.status == crate::ai_agents::AgentStatus::Idle
                && state.agent_runtime_submit_pending
            {
                state.idle_screen_streak = 0;
                continue;
            }
            if matches!(
                detection.status,
                crate::ai_agents::AgentStatus::Working | crate::ai_agents::AgentStatus::Blocked
            ) {
                state.agent_runtime_submit_pending = false;
            }
            // 空闲提示符降级要分三档（#「转圈不停」的根修）：
            // - hook 报过的 Done/Blocked 是精确终态，不被提示符降级；
            // - Working 可能是丢了 TurnDone 的僵尸态（打断的回合没有 Stop
            //   事件），但单拍空闲可能只是重绘间隙——连续两拍才收场；
            // - 其余状态照常应用。
            if detection.status == crate::ai_agents::AgentStatus::Idle {
                if state.agent_hook_seen
                    && matches!(
                        state.agent_status,
                        crate::ai_agents::AgentStatus::Done
                            | crate::ai_agents::AgentStatus::Blocked
                    )
                {
                    continue;
                }
                if state.agent_status == crate::ai_agents::AgentStatus::Working {
                    state.idle_screen_streak = state.idle_screen_streak.saturating_add(1);
                    if state.idle_screen_streak < 2 {
                        continue;
                    }
                }
            } else {
                state.idle_screen_streak = 0;
            }
            let previous = state.agent_status;
            if previous != detection.status
                || state.agent_status_rule.as_deref() != Some(&detection.rule_id)
            {
                log::debug!(
                    "agent screen state: pane={} program={} {:?}->{:?} rule={}",
                    pane.id,
                    program,
                    previous,
                    detection.status,
                    detection.rule_id
                );
            }
            state.agent_status = detection.status;
            state.agent_status_source = crate::ai_agents::AgentStatusSource::Screen;
            state.agent_status_rule = Some(detection.rule_id);

            match detection.status {
                crate::ai_agents::AgentStatus::Blocked => {
                    state.awaiting_input = true;
                    state.needs_attention = true;
                    state.finished_unseen = true;
                },
                crate::ai_agents::AgentStatus::Working => {
                    state.awaiting_input = false;
                    state.needs_attention = false;
                    state.finished_unseen = false;
                    state.command_started.get_or_insert_with(Instant::now);
                },
                crate::ai_agents::AgentStatus::Idle => {
                    state.awaiting_input = true;
                    state.needs_attention = false;
                    if previous == crate::ai_agents::AgentStatus::Working {
                        state.finished_unseen = true;
                        state.finished_at = Some(Instant::now());
                    }
                },
                crate::ai_agents::AgentStatus::Done | crate::ai_agents::AgentStatus::Unknown => {},
            }
        }
        // Chrome/tray read the established fields; rebuilding their compact
        // arrays here makes the detector visible in the same tick.
        self.sync_chrome_tabs();
    }

    /// 后台修复请求（spec 001）的结果落地：pane 归属本窗口即认领（返回
    /// true，AiHook 同款路由契约）。写入前校验 seq——条子已被用户撤掉、或
    /// 新失败已顶掉旧请求时，迟到的响应直接丢弃。
    pub fn handle_ai_fix(
        &mut self,
        pane_id: u64,
        seq: u64,
        fix: &Option<crate::ai_assistant::AiFix>,
    ) -> bool {
        let Some(idx) = self.pane_index(pane_id) else { return false };
        let state = &mut self.panes[idx].nebula_state;
        if state.ai_fix.as_ref().is_some_and(|current| current.seq() == seq) {
            state.ai_fix =
                fix.clone().map(|fix| crate::ai_assistant::AiFixState::Ready { seq, fix });
            self.dirty = true;
            self.display.window.request_redraw();
        }
        true
    }

    /// 远程备份线程收尾：设置页状态行是第一现场，message bar 兜底通知
    /// 没开设置页的窗口。
    pub fn handle_backup_remote_done(&mut self, message: &str, error: bool) {
        self.display.backup_remote_done(message, error);
        let ty = if error {
            crate::message_bar::MessageType::Error
        } else {
            crate::message_bar::MessageType::Warning
        };
        self.message_buffer.push(crate::message_bar::Message::new(format!("备份：{message}"), ty));
        self.dirty = true;
        self.display.window.request_redraw();
    }

    /// 同步线程收尾（spec 003）：消息进 message bar；拉到新历史时热加载
    /// （ghost 补全立即吃到另一台机器的命令）。settings 变化不用管——
    /// mtime 监视在下一帧自动 reload。
    pub fn handle_sync_done(&mut self, message: &str, error: bool, history_changed: bool) {
        // 设置页的状态行（按钮下方）是第一现场；message_bar 兜底通知
        // 没开设置页的窗口。
        self.display.sync_action_done(message, error);
        let ty = if error {
            crate::message_bar::MessageType::Error
        } else {
            crate::message_bar::MessageType::Warning
        };
        self.message_buffer.push(crate::message_bar::Message::new(format!("同步：{message}"), ty));
        if history_changed {
            self.display.reload_nebula_history();
        }
        self.dirty = true;
        self.display.window.request_redraw();
    }

    /// Toast click landed: bring this window to the foreground and, when the
    /// toast named a pane, surface its tab and focus that split.
    pub fn focus_from_toast(&mut self, pane: Option<u64>) {
        if let Some(pane_id) = pane {
            let index = self.tabs.iter().position(|tab| {
                let mut ids = Vec::new();
                tab.layout.leaves(&mut ids);
                ids.contains(&pane_id)
            });
            if let Some(index) = index {
                if index != self.active_tab {
                    self.select_tab(index);
                }
                if let Some(tab) = self.tabs.get_mut(index) {
                    tab.active_pane = pane_id;
                }
            }
        }
        // Best-effort: Windows may downgrade a background process's focus
        // request to a taskbar flash; the click usually grants it.
        self.display.window.focus_window();
        self.dirty = true;
    }

    /// Confirm a typed-`ssh` login as connected and save its destination to
    /// the sidebar, once the session shows PTY activity beyond the fast-
    /// failure window. Called on Wakeup: the old flow only saved when the
    /// session ENDED (`SAVE_MIN_SESSION` at CommandDone), which left the
    /// sidebar empty exactly while the user was connected and looking at it.
    pub fn confirm_ssh_on_activity(&mut self, pane_id: Option<u64>) {
        let Some(index) = pane_id.and_then(|id| self.pane_index(id)) else { return };
        let state = &mut self.panes[index].nebula_state;
        if state.pending_ssh_host.is_some()
            && state
                .command_started
                .is_some_and(|started| started.elapsed() >= crate::ssh::SAVE_CONNECTED_AFTER)
        {
            if let Some(host) = state.pending_ssh_host.take() {
                self.display.nebula_save_ssh_host(&host);
                self.dirty = true;
            }
        }
    }
}