# Release Note rules

1. 只写已实现且有代码、构建或真实运行证据的用户可感知变化。调试探针、测试用强制弹窗、内部重构和未经验证的推断不得写成已交付功能。
2. 沿用 `v1.3.0` 的双语结构：English 的 `Added / Fixed / Improved`、中文的 `新增 / 修复 / 改进`、`Contributors` 和最终资产 `SHA256`。中英文表达同一事实。
3. 同一发布的 `CHANGELOG.md`、`docs/release-notes/vX.Y.Z.md` 和 GitHub Release 正文保持同步。正式构建前完成 Changelog，因为 ZIP 和安装器会把它打进产物。
4. 引用 Issue 前读取其标题、正文、状态和必要评论，再挂到最准确的一条说明上。不得按编号猜测、复用旧版本链接或把宽泛需求改写成逐项 Bug 报告。
5. 英文使用 `Addresses [#N](https://github.com/Kuddev/nebula/issues/N).`，中文使用 `对应 [#N](https://github.com/Kuddev/nebula/issues/N)。`。同一 Issue 涵盖紧密相关修复时，优先只在代表性条目引用一次。
6. 文案描述可核验的用户结果，不用内部类型名、辅助函数或提交标题代替行为说明。仍开放或只部分覆盖的问题必须准确标明范围，不能扩大完成度。
7. Contributors 规则和已发布版本的例外以本地 `.agents/memory.md` 中经过核验的最新发布结论为准，不凭空补头像或贡献者。
