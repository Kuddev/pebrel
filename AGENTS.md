# Nebula 项目代理指引

## 全局规则

- 修改前阅读 `CONTRIBUTING.md`、`docs/architecture.md` 和 `docs/project-constraints.md`；新 UI 文案同时遵循 `docs/internationalization.md`。
- 规则按目录分层：先遵守本文件，再读取目标文件路径上最近的 `AGENTS.md`。模块细节留在模块目录，不回填到根规则。
- 行数预算与依赖方向由 `architecture/` 声明，使用 `python3 scripts/check_architecture.py --base <PR-base-commit>` 验证；800 行仅提示，2000 行是既有防灾上限。
- 禁止靠抬高预算、删测试、压缩格式、机械分片或静默排除目录让门禁通过。规范存在缺陷时，先给可复现反例，补正反测试并记录经维护者审查的规则修订。
- 核心规则保持单一权威实现；按职责和生命周期拆分，不因文件夹名称或历史模块别名推断耦合。新增长期依赖、跨层接口、持久化和线程模型变更须给出设计依据。
- 普通翻译查询维持已测零分配合同；参数格式化和冷路径另行评估。不能把单机微基准写成普遍性能保证。
- 不得声称仅加入 workflow 或 `CODEOWNERS` 就已启用 GitHub 强制保护；服务端设置及负例 PR 验证须另行获准并核验。
- `docs/` 记录经过核验的当前事实；非平凡改动的决策、被否定方案和事故因果写入与代码职责同路径的 `architecture/notes/`。遵循 [note 规则](architecture/notes/AGENTS.md)，普通样式修改和单文件修复不写 note。

## 分层入口

- 应用、窗口和 UI：遵循 [`nebula_app/AGENTS.md`](nebula_app/AGENTS.md)。
- 终端核心、PTY、VT 和输入：遵循 [`nebula_terminal/AGENTS.md`](nebula_terminal/AGENTS.md)。
- 设置、持久化和语言注册：遵循 [`nebula_settings/AGENTS.md`](nebula_settings/AGENTS.md)。
- 检查器、门禁和治理脚本：遵循 [`scripts/AGENTS.md`](scripts/AGENTS.md)。
- 文档事实与操作手册：遵循 [`docs/AGENTS.md`](docs/AGENTS.md)。
- 版本号、构建、打包、自动更新、GitHub Release、Changelog 或 Release Note：先完整阅读 [`packaging/AGENTS.md`](packaging/AGENTS.md)、[`docs/release-notes/AGENTS.md`](docs/release-notes/AGENTS.md) 和本地 `.agents/memory.md` 的发布章节。
