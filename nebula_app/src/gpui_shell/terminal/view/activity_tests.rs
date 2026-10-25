use super::startup_tests::{feed, open};
use super::*;
use crate::ai_agents::{AgentStatus, AgentStatusSource};
use crate::ai_hook::AiHookEvent;
use gpui::TestAppContext;

fn hook(session: &str, name: &str, sequence: u64) -> AiHookEvent {
    let payload = serde_json::json!({
        "hook_event_name": name,
        "session_id": session,
        "bridge_sequence": sequence,
    });
    let wire = format!("nebula-hook/1 source=claude pane=42\n{payload}");
    crate::ai_hook::parse_remote_envelope(wire.as_bytes(), Some(42)).unwrap()
}

/// 与远端 bridge 真实信封同构：`process=<pid:epoch>` 进程身份（每次 CLI
/// 启动都换新值，各自的状态文件里 bridge_sequence 从 1 重新计数）。
fn remote_hook(session: &str, name: &str, process: &str, sequence: u64) -> AiHookEvent {
    let payload = serde_json::json!({
        "hook_event_name": name,
        "session_id": session,
        "bridge_sequence": sequence,
    });
    let wire = format!("nebula-hook/1 source=claude process={process}\n{payload}");
    crate::ai_hook::parse_remote_envelope(wire.as_bytes(), None).unwrap()
}

#[gpui::test]
fn pi_redraw_anchor_follows_accepted_hook_and_command_lifecycle(cx: &mut TestAppContext) {
    use nebula_terminal::term::test::TermSize;
    use nebula_terminal::vte::ansi::Processor;

    let (view, window, _) = open(cx);
    view.update(window, |view, cx| {
        let event = |kind: &str, seq: u64| {
            crate::ai_hook::parse_remote_envelope(
                format!("nebula-hook/1 source=pi pane=42\n{{\"kind\":\"{kind}\",\"session_id\":\"redraw-policy\",\"bridge_sequence\":{seq}}}").as_bytes(),
                Some(42),
            ).unwrap()
        };
        let check = |view: &mut TerminalView, expected: usize| {
            let mut term = view.session.as_ref().unwrap().term.lock();
            term.resize(TermSize::new(60, 5));
            let mut stream = Processor::<nebula_terminal::vte::ansi::StdSyncHandler>::default();
            let text = (0..12).map(|n| format!("ROW_{n:03}")).collect::<Vec<_>>().join("\r\n");
            let frame = format!("\x1b[?2026h\x1b[2J\x1b[H\x1b[3J{text}\x1b[?2026l");
            term.scroll_display(Scroll::Bottom);
            stream.advance(&mut *term, frame.as_bytes());
            term.scroll_display(Scroll::Top);
            term.scroll_display(Scroll::Delta(-2));
            stream.advance(&mut *term, frame.as_bytes());
            assert_eq!(term.grid().display_offset(), expected);
        };
        check(view, 0);
        assert!(view.handle_ai_hook(&event("session-start", 1), cx));
        check(view, 5);
        assert!(view.handle_ai_hook(&event("done", 2), cx));
        check(view, 5); // 回合结束不是 Pi 进程退出。
        assert!(view.handle_ai_hook(&event("session-end", 3), cx));
        check(view, 0);
        assert!(view.handle_ai_hook(&event("session-start", 4), cx));
        check(view, 5);
        view.finish_foreground_command(Some(0), cx);
        check(view, 0);
    });
}

fn screen(view: &mut TerminalView, text: &str) {
    feed(view, format!("\x1b[2J\x1b[H{}", text.replace('\n', "\r\n")).as_bytes());
}

#[gpui::test]
fn hook_lifecycle_cannot_be_rewritten_by_screen_words_or_idle_samples(cx: &mut TestAppContext) {
    let (view, window, _) = open(cx);
    view.update(window, |view, cx| {
        view.handle_ai_hook(&hook("authority", "UserPromptSubmit", 1), cx);
        for text in [
            "code: contains = [\"(y/n)\"]\n❯ \n? for shortcuts",
            "Do you want to proceed?\n❯ 1. Yes\n2. No\nEsc to cancel",
            "────────\n❯ \n────────\n? for shortcuts",
        ] {
            screen(view, text);
            for _ in 0..6 {
                view.refresh_agent_screen_state(cx);
            }
            assert_eq!(view.agent_activity.status(), AgentStatus::Working, "{text}");
            assert_eq!(view.agent_activity.source(), AgentStatusSource::Hook);
        }
        screen(view, "Do you want to proceed?\n❯ 1. Yes\n2. No\nEsc to cancel");
        view.handle_ai_hook(&hook("authority", "Stop", 2), cx);
        view.refresh_agent_screen_state(cx);
        assert_eq!(view.agent_activity.status(), AgentStatus::Done, "Stop is a terminal event");
        view.handle_ai_hook(&hook("authority", "PermissionRequest", 3), cx);
        assert_eq!(view.agent_activity.status(), AgentStatus::Blocked);
        screen(view, "────────\n❯ \n────────\n? for shortcuts");
        for _ in 0..6 {
            view.refresh_agent_screen_state(cx);
        }
        assert_eq!(
            view.agent_activity.status(),
            AgentStatus::Blocked,
            "only the owner can resolve its request"
        );
        view.handle_ai_hook(&hook("authority", "PostToolUse", 4), cx);
        assert_eq!(view.agent_activity.status(), AgentStatus::Working);
    });
}

#[gpui::test]
fn vanished_claude_form_restores_running_with_a_custom_status_footer(cx: &mut TestAppContext) {
    use crate::runtime_api::RuntimeTaskState;

    let (view, window, _) = open(cx);
    view.update(window, |view, cx| {
        view.running_program = Some("claude".into());
        view.agent_activity.begin_command(true);
        view.agent_activity.submitted();
        assert_eq!(view.runtime_task_state(), RuntimeTaskState::Running);

        screen(view, "Do you want to proceed?\n❯ 1. Yes\n2. No\nEsc to cancel · Tab to amend");
        for _ in 0..6 {
            view.refresh_agent_screen_state(cx);
            assert_eq!(view.agent_activity.status(), AgentStatus::Blocked);
            assert_eq!(view.agent_activity.source(), AgentStatusSource::Screen);
            assert_eq!(view.agent_activity.rule(), Some("live_form_blocked"));
            assert_eq!(view.sidebar_activity(), SidebarActivity::Attention);
            assert_eq!(view.runtime_task_state(), RuntimeTaskState::Attention);
        }

        // This running layout has no interrupt hint, and its four custom footer
        // lines push the input prompt outside prompt_idle's observation region.
        let running = "* Ideating… (1m 20s · 2.3k tokens)\n\n❯ \n\n状态 运行中\n模型 Claude Sonnet\n目录 /project\n上下文 12%";
        assert!(crate::ai_agents::detect("claude", running).is_none());
        screen(view, running);
        view.refresh_agent_screen_state(cx);
        assert_eq!(view.agent_activity.status(), AgentStatus::Blocked);
        assert_eq!(view.sidebar_activity(), SidebarActivity::Attention);
        assert_eq!(view.runtime_task_state(), RuntimeTaskState::Attention);

        for _ in 0..8 {
            view.refresh_agent_screen_state(cx);
            assert_eq!(view.agent_activity.status(), AgentStatus::Working);
            assert_eq!(view.agent_activity.source(), AgentStatusSource::Process);
            assert_eq!(view.agent_activity.rule(), None);
            assert_eq!(view.sidebar_activity(), SidebarActivity::Running);
            assert_eq!(view.runtime_task_state(), RuntimeTaskState::Running);
        }
    });
}

#[gpui::test]
fn nested_agent_hooks_do_not_finish_or_replace_the_primary_turn(cx: &mut TestAppContext) {
    let (view, window, _) = open(cx);
    view.update(window, |view, cx| {
        let mut main = hook("primary", "UserPromptSubmit", 1);
        main.agent_pid = Some(100);
        view.handle_ai_hook(&main, cx);
        for (index, name) in
            ["SessionStart", "Stop", "PermissionRequest", "SessionEnd"].iter().enumerate()
        {
            let mut nested = hook("nested", name, index as u64 + 1);
            nested.agent_pid = Some(200);
            view.handle_ai_hook(&nested, cx);
            assert_eq!(view.agent_activity.status(), AgentStatus::Working, "nested {name}");
            assert_eq!(view.ai_session.as_ref().unwrap().session_id, "primary");
        }
    });
}

#[gpui::test]
fn shell_metadata_cannot_replace_a_hook_owned_agent_identity(cx: &mut TestAppContext) {
    let (view, window, _) = open(cx);
    view.update(window, |view, cx| {
        view.handle_ai_hook(&hook("hook-owned-identity", "UserPromptSubmit", 1), cx);
        view.suggest.last_committed = "cc --resume".into();
        for title in ["NEBULA|/project|main|node", "NEBULA|/project|main"] {
            view.process_event(TermEvent::Title(title.into()), cx);
            view.on_command_start(cx);
            assert_eq!(view.running_program.as_deref(), Some("claude"));
            assert_eq!(view.ai_session.as_ref().unwrap().session_id, "hook-owned-identity");
            assert_eq!(view.agent_activity.source(), AgentStatusSource::Hook);
            assert_eq!(view.agent_activity.status(), AgentStatus::Working);
        }
    });
}

#[gpui::test]
fn command_end_clears_progress_and_stale_agent_state(cx: &mut TestAppContext) {
    let (view, window, _) = open(cx);
    view.update(window, |view, cx| {
        for code in [0, 1] {
            view.suggest.last_committed = "pi update --extensions".into();
            view.on_command_start(cx);
            view.process_event(TermEvent::Progress { state: 3, value: None }, cx);
            assert_eq!(view.sidebar_activity(), SidebarActivity::Running);
            view.process_event(TermEvent::CommandDone { exit_code: Some(code) }, cx);
            screen(view, "◦ Working (1s • esc to interrupt)\n› Ask Codex to do anything");
            view.refresh_agent_screen_state(cx);
            assert_eq!(view.progress, crate::taskbar::TaskProgress::None);
            assert_eq!(view.agent_activity.status(), AgentStatus::Unknown);
            assert_eq!(
                view.sidebar_activity(),
                if code == 0 { SidebarActivity::Idle } else { SidebarActivity::CommandFailed }
            );
        }
    });
}

#[gpui::test]
fn failed_local_snapshot_does_not_finish_a_shell_builtin(cx: &mut TestAppContext) {
    let (view, window, _) = open(cx);
    view.update(window, |view, cx| {
        view.suggest.last_committed = "Start-Sleep -Seconds 30".into();
        view.suggest.suggest_env = crate::display::SuggestEnv::Local;
        view.on_command_start(cx);
        view.session.as_mut().unwrap().shell_pid = u32::MAX;
        view.command_started = Some(std::time::Instant::now() - std::time::Duration::from_secs(4));
        view.reconcile_shell_activity(cx);
        assert!(!view.command_running_disproved);
        assert_eq!(view.sidebar_activity(), SidebarActivity::Running);
    });
}

/// 无集成的嵌套交互 shell（Cygwin fish 等）没有 133;D 完成边；OSC 7 随它
/// 每次提示符渲染到达，必须充当「命令结束、回到提示符」的边，否则进入
/// fish 后每个 Enter 的弱推断标记都永远等不到完成（用户 10-10 报障）。
#[gpui::test]
fn cwd_report_finishes_a_command_inside_an_unintegrated_nested_shell(cx: &mut TestAppContext) {
    let (view, window, _) = open(cx);
    view.update(window, |view, cx| {
        // fish 提示符上的提交：commit_line 的弱推断路径（无 133;C 参与）。
        view.suggest.last_committed = "cd /cygdrive/d/project".into();
        view.mark_command_running();
        assert_eq!(view.sidebar_activity(), SidebarActivity::Running);
        // fish 画下一个提示符时的 OSC 7 上报：cwd 信号本身不是完成证据，
        // 只排程进程树探测，运行态当场必须维持。
        view.process_event(TermEvent::CwdReport("/cygdrive/d/project".into()), cx);
        assert!(view.command_running, "OSC 7 alone is a cwd signal, not completion");
        assert_eq!(view.cwd, "/cygdrive/d/project", "the report still updates the cwd");
        // 进程树只剩交互 shell（bash 根 + 接管的 fish）→ 回到提示符，清运行态。
        let at_prompt = vec![
            crate::process_tree::ProcessEntry {
                pid: 1,
                parent_pid: 0,
                depth: 0,
                executable: "bash.exe".into(),
            },
            crate::process_tree::ProcessEntry {
                pid: 2,
                parent_pid: 1,
                depth: 1,
                executable: "fish.exe".into(),
            },
        ];
        let started = view.command_started;
        view.apply_osc7_prompt_evidence(started, Ok(at_prompt), cx);
        assert!(!view.command_running);
        assert!(view.command_started.is_none());
        assert_eq!(view.sidebar_activity(), SidebarActivity::Idle);
        // 已被进程反证/完成的命令不受影响；hook 拥有的 agent 回合不走这条弱边。
        view.handle_ai_hook(&hook("osc7-not-mine", "UserPromptSubmit", 1), cx);
        view.mark_command_running();
        assert_eq!(view.sidebar_activity(), SidebarActivity::Running);
        view.process_event(TermEvent::CwdReport("/cygdrive/d/other".into()), cx);
        let started = view.command_started;
        view.apply_osc7_prompt_evidence(
            started,
            Ok(vec![crate::process_tree::ProcessEntry {
                pid: 1,
                parent_pid: 0,
                depth: 0,
                executable: "bash.exe".into(),
            }]),
            cx,
        );
        assert_eq!(
            view.sidebar_activity(),
            SidebarActivity::Running,
            "a hook-owned turn ignores the weak OSC 7 edge"
        );
    });
}

/// 运行中的命令自己发出的 OSC 7 不是完成证据（评审反例：
/// `bash -c 'printf "]7;file://localhost/tmp"; sleep 60'`）——
/// 进程树里 sleep 还在，弱推断的运行态必须保持。
#[gpui::test]
fn a_running_command_emitting_osc7_does_not_finish_itself(cx: &mut TestAppContext) {
    let (view, window, _) = open(cx);
    view.update(window, |view, cx| {
        view.suggest.last_committed = "bash -c 'printf osc7; sleep 60'".into();
        view.mark_command_running();
        assert_eq!(view.sidebar_activity(), SidebarActivity::Running);
        // 命令运行中上报了一个目录。
        view.process_event(TermEvent::CwdReport("/tmp".into()), cx);
        assert!(view.command_running, "the cwd report alone changes nothing");
        // 进程树：根 shell → bash -c → sleep —— 存在非 shell 子进程，否决。
        let command_running_tree = vec![
            crate::process_tree::ProcessEntry {
                pid: 1,
                parent_pid: 0,
                depth: 0,
                executable: "bash.exe".into(),
            },
            crate::process_tree::ProcessEntry {
                pid: 2,
                parent_pid: 1,
                depth: 1,
                executable: "bash.exe".into(),
            },
            crate::process_tree::ProcessEntry {
                pid: 3,
                parent_pid: 2,
                depth: 2,
                executable: "sleep.exe".into(),
            },
        ];
        let started = view.command_started;
        view.apply_osc7_prompt_evidence(started, Ok(command_running_tree), cx);
        assert!(view.command_running, "a live command child vetoes the prompt inference");
        assert_eq!(view.sidebar_activity(), SidebarActivity::Running);
        // sleep 退出后同样的上报才构成回到提示符的证据。
        let at_prompt = vec![
            crate::process_tree::ProcessEntry {
                pid: 1,
                parent_pid: 0,
                depth: 0,
                executable: "bash.exe".into(),
            },
            crate::process_tree::ProcessEntry {
                pid: 2,
                parent_pid: 1,
                depth: 1,
                executable: "fish.exe".into(),
            },
        ];
        let started = view.command_started;
        view.apply_osc7_prompt_evidence(started, Ok(at_prompt), cx);
        assert!(!view.command_running);
        assert_eq!(view.sidebar_activity(), SidebarActivity::Idle);
    });
}

#[gpui::test]
fn ssh_shell_prompt_return_clears_a_command_without_a_done_marker(cx: &mut TestAppContext) {
    let (view, window, _) = open(cx);
    view.update(window, |view, cx| {
        view.suggest.suggest_env =
            crate::display::SuggestEnv::Ssh { destination: "user@example.test".into() };
        view.suggest.last_committed = "sleep 30".into();
        view.suggest.pending_command_prompt = Some("user@host:~$".into());
        view.on_command_start(cx);
        screen(view, "quiet remote work");
        for _ in 0..6 {
            view.refresh_agent_screen_state(cx);
        }
        assert_eq!(view.sidebar_activity(), SidebarActivity::Running, "silence is not completion");
        screen(view, "user@host:~$ next command");
        view.refresh_agent_screen_state(cx);
        assert_eq!(
            view.sidebar_activity(),
            SidebarActivity::Running,
            "nonempty draft is not an empty prompt"
        );
        screen(view, "user@host:~$ ");
        view.refresh_agent_screen_state(cx);
        assert_eq!(view.sidebar_activity(), SidebarActivity::Idle);
        assert!(view.running_program.is_none());
        assert!(view.command_started.is_none());
    });
}

/// SSH pane 里 agent 的两个会话周期：退出后再次启动，新 CLI 进程的
/// SessionStart 必须重新建立身份（图标/状态/摘要的输入），无论上一个会话的
/// 收尾事件是否完整到达（SessionEnd 在 bridge 通道上有真实丢失路径）。
#[gpui::test]
fn ssh_agent_second_launch_reestablishes_identity(cx: &mut TestAppContext) {
    let (view, window, _) = open(cx);
    view.update(window, |view, cx| {
        view.ssh_destination = Some("build@example.test".into());
        view.suggest.suggest_env =
            crate::display::SuggestEnv::Ssh { destination: "build@example.test".into() };

        // ---- 会话 1：用户键入 claude 回车（133;C + bridge SessionStart）----
        view.suggest.last_committed = "claude".into();
        view.process_event(TermEvent::CommandStart, cx);
        assert!(view.handle_ai_hook(&remote_hook("s1", "SessionStart", "100:1", 1), cx));
        assert!(view.handle_ai_hook(&remote_hook("s1", "UserPromptSubmit", "100:1", 2), cx));
        assert_eq!(view.running_program.as_deref(), Some("claude"));
        view.process_event(TermEvent::Title("✳ 修复登录跳转".into()), cx);
        assert_eq!(view.title, "✳ 修复登录跳转", "first session's title is live");
        assert_eq!(view.sidebar_activity(), SidebarActivity::Running);

        // ---- 退出：SessionEnd 丢失（bridge 忙/超时），只剩 133;D 收尾 ----
        view.process_event(TermEvent::CommandDone { exit_code: Some(0) }, cx);
        assert_eq!(view.running_program, None, "command boundary clears identity");

        // ---- 会话 2：新 CLI 进程（process 换新、序号从 1 重来）----
        view.suggest.last_committed = "claude".into();
        view.process_event(TermEvent::CommandStart, cx);
        assert!(
            view.handle_ai_hook(&remote_hook("s2", "SessionStart", "200:9", 1), cx),
            "the second session's SessionStart must be accepted"
        );
        assert_eq!(view.running_program.as_deref(), Some("claude"));
        view.process_event(TermEvent::Title("✳ 继续修分页".into()), cx);
        assert_eq!(view.title, "✳ 继续修分页", "second session's title is live");
        assert!(
            view.handle_ai_hook(&remote_hook("s2", "UserPromptSubmit", "200:9", 2), cx),
            "the second session's turn hooks must be accepted"
        );
        assert_eq!(view.sidebar_activity(), SidebarActivity::Running);

        // ---- 变体：SessionEnd 完整到达的收尾同样不得卡住下一次进入 ----
        assert!(view.handle_ai_hook(&remote_hook("s2", "SessionEnd", "200:9", 3), cx));
        view.suggest.last_committed = "claude".into();
        view.process_event(TermEvent::CommandStart, cx);
        assert!(view.handle_ai_hook(&remote_hook("s3", "SessionStart", "300:4", 1), cx));
        assert_eq!(view.running_program.as_deref(), Some("claude"));
    });
}

/// CLI 2.x 底部 chrome 长到 5 个非空行的 idle 屏（ai_agents.rs 同源 fixture）：
/// ❯ 在自底向上第 6 行，prompt_idle 命中；❯ 后是 NBSP。
const CLAUDE_IDLE_SCREEN: &str = "╭─── Claude Code v2.1.286 ───╮\n\
                                   ────────────────────────────────────────────────────\n\
                                   ❯\u{a0}\n\
                                   ────────────────────────────────────────────────────\n\
                                   \x20 ⚠ Transcript saving is off — inherited session\n\
                                   \x20 🧠glm-5.3 | 📁Fleet | 🌿feature/left_tree*\n\
                                   \x20 ⏳5h 🟢 20% 2h7m | 📅MCP 13/1000\n\
                                   \x20 ⏵⏵ auto mode on (shift+tab to cycle)";

/// 同款加宽 chrome 的工作中屏幕：转圈 + esc to interrupt 同屏，spinner_working
/// 判 Working——停滞对账不得在干活时收边。
const CLAUDE_WORKING_SCREEN: &str = "  ⎿  Running…\n\
                                     ✶ Moseying… (8m 42s · ↓ 18.3k tokens)\n\
                                     ────────────────────────────────────────\n\
                                     ❯ \n\
                                     ────────────────────────────────────────\n\
                                     \x20 ⏵⏵ bypass permissions on (shift+tab to cycle)\n\
                                     \x20 ↳ queued\n\
                                     \x20 ⏸ esc to interrupt · ? for shortcuts · ← for agents";

/// 远端 Stop hook 丢失（bridge 通道锁/超时/超限都会丢）时，hook 权威把回合钉在
/// Working；远端没有进程树这第二重证据，唯一的对账源是屏幕。停滞超阈值 +
/// 连续两拍 agent 自家 idle 屏 → 收敛为 Done；身份保留给命令边界。
#[gpui::test]
fn remote_lost_stop_is_reconciled_by_stalled_idle_screen(cx: &mut TestAppContext) {
    let (view, window, _) = open(cx);
    view.update(window, |view, cx| {
        view.ssh_destination = Some("build@example.test".into());
        view.suggest.suggest_env =
            crate::display::SuggestEnv::Ssh { destination: "build@example.test".into() };
        // fixture 先对着 detect() 钉一遍，防止规则演进让这组断言静默失效。
        assert_eq!(
            crate::ai_agents::detect("claude", CLAUDE_IDLE_SCREEN).unwrap().status,
            AgentStatus::Idle
        );
        assert_eq!(
            crate::ai_agents::detect("claude", CLAUDE_WORKING_SCREEN).unwrap().status,
            AgentStatus::Working
        );
        // 回合起跑；Stop 在远端丢失。
        view.handle_ai_hook(&hook("lost-stop", "UserPromptSubmit", 1), cx);
        assert_eq!(view.agent_activity.status(), AgentStatus::Working);
        assert_eq!(view.sidebar_activity(), SidebarActivity::Running);
        // agent 已答完：屏幕是 idle 提示符，但 hook 静默未超阈值——不得收边。
        screen(view, CLAUDE_IDLE_SCREEN);
        for _ in 0..4 {
            view.refresh_agent_screen_state(cx);
        }
        assert_eq!(
            view.agent_activity.status(),
            AgentStatus::Working,
            "a live hook timeline outranks the idle screen"
        );
        // 停滞超阈值：连续两拍 idle 屏收敛为 Done；CLI 还活着，身份不动。
        view.agent_activity.backdate_activity_for_test(
            super::REMOTE_TURN_STALL + std::time::Duration::from_secs(1),
        );
        view.refresh_agent_screen_state(cx);
        assert_eq!(view.agent_activity.status(), AgentStatus::Working, "one sample is not enough");
        view.refresh_agent_screen_state(cx);
        assert_eq!(view.agent_activity.status(), AgentStatus::Done);
        assert_eq!(view.runtime_task_state(), crate::runtime_api::RuntimeTaskState::Finished);
        assert_eq!(view.running_program.as_deref(), Some("claude"), "identity survives the turn");
    });
}

/// 工作中的屏幕（转圈/中断提示）反证「回合已结束」：连续 idle 计数归零，
/// 之后即使停滞依旧，也要重新攒满两拍。
#[gpui::test]
fn a_working_screen_resets_the_stall_reconciliation_samples(cx: &mut TestAppContext) {
    let (view, window, _) = open(cx);
    view.update(window, |view, cx| {
        view.ssh_destination = Some("build@example.test".into());
        view.suggest.suggest_env =
            crate::display::SuggestEnv::Ssh { destination: "build@example.test".into() };
        view.handle_ai_hook(&hook("working-screen", "UserPromptSubmit", 1), cx);
        view.agent_activity.backdate_activity_for_test(
            super::REMOTE_TURN_STALL + std::time::Duration::from_secs(1),
        );
        screen(view, CLAUDE_IDLE_SCREEN);
        view.refresh_agent_screen_state(cx);
        // 攒到第 2 拍之前屏幕翻回转圈：计数归零，回合继续。
        screen(view, CLAUDE_WORKING_SCREEN);
        view.refresh_agent_screen_state(cx);
        assert_eq!(view.agent_activity.status(), AgentStatus::Working);
        screen(view, CLAUDE_IDLE_SCREEN);
        view.refresh_agent_screen_state(cx);
        assert_eq!(
            view.agent_activity.status(),
            AgentStatus::Working,
            "the counter restarted from the working frame"
        );
        view.refresh_agent_screen_state(cx);
        assert_eq!(view.agent_activity.status(), AgentStatus::Done);
    });
}

/// 停滞对账只属于远端：本地 pane 有进程树这第二重证据，hook 权威不被屏幕
/// 侵蚀（等价加固 `hook_lifecycle_cannot_be_rewritten_by_screen_words_or_idle_samples`）。
#[gpui::test]
fn local_hook_authority_is_never_eroded_by_stall_reconciliation(cx: &mut TestAppContext) {
    let (view, window, _) = open(cx);
    view.update(window, |view, cx| {
        view.suggest.suggest_env = crate::display::SuggestEnv::Local;
        view.handle_ai_hook(&hook("local-authority", "UserPromptSubmit", 1), cx);
        view.agent_activity.backdate_activity_for_test(super::REMOTE_TURN_STALL * 2);
        screen(view, CLAUDE_IDLE_SCREEN);
        for _ in 0..4 {
            view.refresh_agent_screen_state(cx);
        }
        assert_eq!(
            view.agent_activity.status(),
            AgentStatus::Working,
            "local panes keep the process tree as their second witness"
        );
    });
}

/// 远端 UserPromptSubmit 丢失（bridge 通道锁 busy/超时/超限都会丢）时，上一
/// 回合的 Stop 把回合钉在 Done（侧栏 ready）；停滞超阈值 + 连续两拍转圈屏
/// 重新点火 Running；身份保留给命令边界。
#[gpui::test]
fn remote_lost_prompt_submit_is_reignited_by_stalled_working_screen(cx: &mut TestAppContext) {
    let (view, window, _) = open(cx);
    view.update(window, |view, cx| {
        view.ssh_destination = Some("build@example.test".into());
        view.suggest.suggest_env =
            crate::display::SuggestEnv::Ssh { destination: "build@example.test".into() };
        // 第一回合完整：UserPromptSubmit → Stop（Done，侧栏读作 ready）。
        view.handle_ai_hook(&hook("lost-prompt", "UserPromptSubmit", 1), cx);
        view.handle_ai_hook(&hook("lost-prompt", "Stop", 2), cx);
        assert_eq!(view.agent_activity.status(), AgentStatus::Done);
        assert_eq!(view.sidebar_activity(), SidebarActivity::Done);
        // 第二回合的 UserPromptSubmit 在远端丢失；屏幕转圈但 hook 静默未超
        // 阈值——hook 时间线仍然在场，屏幕不得点火。
        screen(view, CLAUDE_WORKING_SCREEN);
        for _ in 0..4 {
            view.refresh_agent_screen_state(cx);
        }
        assert_eq!(
            view.agent_activity.status(),
            AgentStatus::Done,
            "a live hook timeline outranks the working screen"
        );
        // 停滞超阈值：连续两拍转圈重新点火 Running；CLI 还活着，身份不动。
        view.agent_activity.backdate_activity_for_test(
            super::REMOTE_TURN_STALL + std::time::Duration::from_secs(1),
        );
        view.refresh_agent_screen_state(cx);
        assert_eq!(view.agent_activity.status(), AgentStatus::Done, "one sample is not enough");
        view.refresh_agent_screen_state(cx);
        assert_eq!(view.agent_activity.status(), AgentStatus::Working);
        assert_eq!(view.sidebar_activity(), SidebarActivity::Running);
        assert_eq!(view.running_program.as_deref(), Some("claude"), "identity survives the turn");
    });
}

#[gpui::test]
fn output_rule_source_does_not_turn_a_working_codex_into_attention(cx: &mut TestAppContext) {
    let (view, window, _) = open(cx);
    view.update(window, |view, cx| {
        view.running_program = Some("codex".into());
        screen(view, "  { contains = [\"(y/n)\"] },\n  { contains = [\"[y/n]\"] },\n]\n\n◦ Working (12m 36s • esc to interrupt)\n\n› Ask Codex to do anything");
        view.refresh_agent_screen_state(cx);
        assert_eq!(view.agent_activity.status(), AgentStatus::Working);
        assert_eq!(view.sidebar_activity(), SidebarActivity::Running);
    });
}

#[gpui::test]
fn ignored_idle_notification_does_not_reopen_the_hook_ordering_gate(cx: &mut TestAppContext) {
    let (view, window, _) = open(cx);
    view.update(window, |view, cx| {
        for name in ["UserPromptSubmit", "Stop", "Notification", "PostToolUse"] {
            let wire = format!("nebula-hook/1 source=claude\n{{\"hook_event_name\":\"{name}\",\"session_id\":\"idle-notification-order\"}}");
            let event = crate::ai_hook::parse_remote_envelope(wire.as_bytes(), Some(42)).unwrap();
            view.handle_ai_hook(&event, cx);
            if name != "UserPromptSubmit" {
                assert_eq!(view.agent_activity.status(), AgentStatus::Done, "{name}");
            }
        }
    });
}

#[gpui::test]
fn ssh_bell_does_not_report_a_blocked_or_finished_command(cx: &mut TestAppContext) {
    let (view, window, _) = open(cx);
    view.update(window, |view, cx| {
        view.suggest.suggest_env =
            crate::display::SuggestEnv::Ssh { destination: "user@example.test".into() };
        view.suggest.last_committed = "sleep 30".into();
        view.on_command_start(cx);
        view.on_bell(cx);
        assert_eq!(view.sidebar_activity(), SidebarActivity::Running);
        assert!(!view.awaiting_input);
    });
}

#[gpui::test]
fn claude_shift_enter_reaches_pty_without_committing_shell_history(cx: &mut TestAppContext) {
    let (view, window, receiver) = open(cx);
    window.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.running_program = Some("claude".into());
            view.suggest.last_committed = "claude".into();
            view.suggest.line_buf = "a multiline prompt".into();
            feed(view, b"\x1b[?9001h");
            view.on_key_down(
                &KeyDownEvent {
                    keystroke: gpui::Keystroke::parse("shift-enter").unwrap(),
                    is_held: false,
                    prefer_character_input: false,
                },
                window,
                cx,
            );
            assert_eq!(view.suggest.last_committed, "claude");
            assert!(
                view.suggest.line_buf.is_empty(),
                "multiline invalidates the single-line shadow"
            );
        });
    });
    let bytes: Vec<u8> = receiver
        .try_iter()
        .filter_map(|message| match message {
            Msg::Input(bytes) => Some(bytes.into_owned()),
            _ => None,
        })
        .flatten()
        .collect();
    assert_eq!(bytes, b"\n");
}
