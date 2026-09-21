# Codex runtime 提交边界与不确定回执

## Status

Implemented locally; pending maintainer review.

## Context

运行时 CLI 需要可靠地向 Codex 提交文本，并区分确定拒绝与不确定投递。
Codex 会把快速输入收集为 paste burst，过早到达的 Enter 可能成为正文换行。
原有 runtime prompt/paste 在首次屏幕变化后补 Enter；屏幕变化不能证明输入已处理完。
此外，部分 CLI 在投递后才校验 wait timeout，旧 ctl 缺少基线时会降级成无基线等待。

## Evidence

- [`terminal_input.rs`](../../../../nebula_app/src/input/terminal_input.rs) 的 runtime 文本路径
  保留 ConPTY 原生字符输入；非字符按键通过协商的 VT/扩展键盘/Win32 编码器发送。
- [`runtime.rs`](../../../../nebula_app/src/gpui_shell/terminal/view/runtime.rs) 原有屏幕回显
  屏障只比较前后屏幕，无法建立 Codex paste-burst 的输入边界。
- Codex 的输入编辑器会先缓冲快速输入；非字符输入会形成提交前的输入边界。
- [`AgentActivity`](../../../../nebula_app/src/ai_hook/lifecycle.rs) 已有 pending-submit
  守卫：未观察到 Working/Blocked 或完成 hook 时，旧 idle 不能制造新回合完成。

## Decision

仅对已识别 Codex 的提交使用共享 `build_runtime_codex_submission`：协商的粘贴文本、
Right、Enter 形成同一次有序输入。非空文本才附加 Right；空输入只发 Enter。
GPUI 和 legacy adapter 复用同一实现；普通 shell 仍使用原有回显屏障。
多行 paste 仍须通过已有校验与 bracketed-paste/目标边界，不开放新的字节注入入口。

等待参数在输入写入前校验。所有提交后等待路径要求有效非零基线。
CLI 将输入请求的传输/解析错误标记为 `submission_outcome_unknown`，提示先读取目标；
服务端明确返回的拒绝仍保留原错误。没有增加自动重发或新的任务状态机。

## Rejected alternatives

- 固定睡眠后按 Enter：ConPTY 分块和程序调度仍使延时成为猜测。
- 只加 bracketed paste：Windows 原生输入读取不一定生成 Paste 事件。
- 所有 shell 一律加 Right：会影响 PSReadLine、历史输入和原有交互语义。
- 投递失败后再自动补一次 Enter/重发文本：回执丢失时可能重复触发任务。
- 重写 Agent 生命周期：已存在相应 pending-submit 守卫，应保留唯一权威。

## Consequences

Codex 提交不再依赖终端重绘，也没有新增 timer/线程/持久化依赖。
这是输入顺序与边界保证，不承诺用户同时编辑草稿时的原子隔离、模型服务可用性或任务成功。
新错误码是 CLI 层的保守分类；依赖旧泛化传输错误的调用者可改为先读后判断。
通用 tab 任务回执与异步回传仍需独立的任务关联、超时和投递生命周期设计。

## Validation

回归覆盖 VT/application-cursor/Win32 输入顺序、Unicode/大块/多行文本、空输入、
GPUI 无重绘提交、后续 Wakeup 不重复提交、no-submit、无效等待参数先于粘贴源读取、
缺失/零基线拒绝，以及输入传输失败与只读错误的区分。
原生 CI 验证产品和输入适配器；回合提交与 CLI 接受输入分开报告，不把本地
输入单测解释为真实模型服务调用成功。

## Supersedes

None.

## Revisit when

Codex 提供可确认提交的稳定 API，或其输入 burst 语义变化时，复核并替换对应边界。
Runtime 提供绑定会话与任务的回执后，CLI 可直接等待任务状态，减少基线适配。
