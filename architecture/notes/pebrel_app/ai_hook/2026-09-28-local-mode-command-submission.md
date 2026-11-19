# 本地模式命令与真实任务提交的生命周期区别

## Status

Implemented; targeted tests and the live mobile conversation path verified.

## Context

手机对话页发送 Codex `/plan` 后，CLI 已切换模式并回到输入提示，但对话页仍显示运行中，
后续消息的发送入口持续禁用。该命令只改变本地交互模式，没有任务完成 hook。

## Evidence

- `AgentActivity::submitted()` 会建立 `pending_submit` 屏障，防止旧输入提示在任务刚提交时
  被误判为完成。这是普通任务需要保留的行为。
- 实际 Codex 0.155.1 接收 `/plan` 后显示 Plan mode 和空输入提示；Runtime 仍保持
  `running / state_source=process`，等待的任务事件并不存在。
- `gpui_shell/terminal/view/runtime.rs` 的文本和粘贴入口原先都无差别建立该屏障。

## Decision

由共享生命周期的 `submitted_text(program, text)` 区分这条已经核实的本地命令。
仅 Codex 的完整输入去除首尾空白后等于 `/plan` 时调用已有 `input_sent()`，允许后续
屏幕观察，但不制造新任务，也不改变正在运行的任务状态。

Runtime 普通输入和粘贴入口共用这一规则。其他命令、其他提供者和包含额外任务正文的
多行输入继续使用 `submitted()`；手机视图不建立第二套命令或任务状态机。

## Rejected alternatives

- 移除全部提交屏障：旧屏幕中的输入提示可能提前结束真实任务。
- 把所有斜杠命令当作本地模式操作：`/review` 等命令本身会启动工作。
- 手机发送 `/plan` 后强制显示完成：掩盖共享状态错误，也可能错误结束已有任务。
- 通过超时解除运行状态：时间流逝不能证明任务已经结束。

## Consequences

命令语义留在共享 Agent 生命周期，终端和手机入口保持一致。本次不推断未核实的其他
斜杠命令，不把传输成功当作任务完成。

## Validation

13 项生命周期测试通过。相关回归覆盖原生 hook 已接入和未接入的空闲会话、普通任务
提交屏障、运行中模式切换、其他提供者以及带任务正文的输入。

真实手机对话页发送 `/plan` 后保持可继续输入；随后在同一原生会话提交中文问题，
收到可点击选项，选择后收到对应助手回答。正常任务仍经过 running、attention 和
finished 的真实状态变化。

## Supersedes

None. Extends the shared ownership established in `2026-09-19-agent-activity-authority.md`.

## Revisit when

提供者改变模式命令的事件语义，或出现另一条已复现的本地命令时，重新核对真实 CLI
事件与输入路径；保持精确分类，不扩展为未经验证的通用斜杠命令规则。
