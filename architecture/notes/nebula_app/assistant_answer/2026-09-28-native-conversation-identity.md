# 活体 Agent 对话投影的身份与异步读取

## Status

Implemented.

## Context

手机需要阅读 Agent 的消息、工具结果和确认选项，但切换到对话视图不应启动另一个
Agent，也不应把旧会话的输入交给同一 pane 中后来启动的进程。原生记录可能较大，
在 UI 回调中读取会让桌面 hover、输入和手机请求一起等待文件 I/O。

## Evidence

- [身份和请求合同](../../../../nebula_app/src/runtime_api/conversation.rs)将
  `kind`、`session_id`、`epoch` 作为同一会话的联合身份；首次读取可解析 epoch，写入必须携带它。
- [原生记录投影](../../../../nebula_app/src/assistant_answer/conversation.rs)核验
  Codex / Claude 的原生会话标识，按边界读取 JSONL，不将推理记录作为可见消息输出。
- 实测 Codex 0.155.1 的 `response_item/message/role=user` 同时包含环境注入和真正输入；
  可见用户消息另有 `event_msg/item_completed/UserMessage` 事件。助手完成事件的
  `AgentMessage` 内容使用 `Text`，与已有助手消息记录重复出现。
- [终端适配](../../../../nebula_app/src/gpui_shell/terminal/view/conversation.rs)已持有
  活体 Agent 身份、原生记录位置和桌面确认状态，另建一套屏幕识别会产生两个权威来源。

## Decision

- 对话仅是当前 CLI 的另一视图，读取原生消息；消息发送仍进入该活体终端的已有提交路径。
- Codex 用户消息以明确的 `UserMessage` 或旧式 `user_message` 事件为依据，不把
  原始上下文中的 `role=user` 全部提升为聊天消息。助手消息兼容 `AgentMessage/Text`
  并去重；保留用户主动发送的 XML、Markdown 和重复提问，不按正文关键词删除内容。
- 在 UI 线程捕获身份与执行上下文，在后台读取原生记录，回到 UI 线程再次核验身份后
  才发布结果。关闭 pane 或替换会话使旧请求失效，而非转发给当前焦点。
- 选项复用 `capture_confirmation()` / `answer_choice()` 的问题身份、一次性消费和
  按键语义；普通编号列表不提升为可操作授权选项。
- 手机端将变更请求串行化，并以变更版本丢弃发送前发起的迟到读取。发送后先清除旧
  选项和发送许可，再等待新的状态。网络结果不确定时不自动重放用户输入。
- Ctrl+C、Esc、Tab 和方向键走同一身份约束下的明确按键请求，不把正文当作按键脚本。

## Rejected alternatives

- 为手机新开一个 Agent：会话上下文、权限和任务生命周期都会分叉。
- 在 UI 回调中直接扫描原生记录：把磁盘或 guest I/O 延迟带入窗口交互。
- 为手机复制确认文本解析器：会与桌面的问题轮次、重复问题及单次消费规则漂移。
- 按 `AGENTS`、XML 标签或文件路径过滤用户正文：既会误删真实输入，也依赖注入文本的写法。
- 网络恢复后重试发送：成功回执丢失时会重复提交真实任务。

## Consequences

当前原生投影覆盖电脑本地 / 已有 guest 执行上下文中的 Codex 与 Claude。
桌面 SSH pane 的记录属于远端主机，不能套用本机记录路径；该路径明确返回未就绪。
手机缓存不保存可复用的 epoch、旧确认按钮或写入许可，重新进入时须重新协商。

## Validation

现有 Rust 测试覆盖原生身份不匹配、UTF-8 与分页边界、部分 JSONL 写入、工具结果去重、
省略推理记录、请求参数约束，以及同一确认问题跨轮次后的旧按钮失效。
Android `DesktopRecoveryTest` 覆盖迟到读取与变更互斥、身份绑定及未知结果不重放。
原生投影的 5 项定向测试通过。雷电模拟器与同一个原生 Codex 会话实际完成中文收发、
两轮选项提交、工具记录展开和收起、Markdown 代码复制以及终端/对话视图往返。
复制结果与原生代码块的 UTF-8 字节一致；阅读旧消息期间收到新回复，正文截图位置保持不变。
这些真实验收不覆盖其他提供者版本、桌面 SSH 远端记录或所有 Android WebView 版本。

## Supersedes

None.

## Revisit when

新增原生消息格式、远端 Agent 记录读取、实时增量事件或新的确认控件时，重新核对
记录所有权和输入语义；优先扩展现有提供者适配，而非在手机建立第二套 Agent 状态机。
