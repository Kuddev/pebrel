## 开始前先确认范围

贡献不一定从写代码开始。清晰的复现步骤、文档改进、翻译、可访问性反馈和针对性的测试同样有价值。

准备修改代码时，先阅读仓库的[贡献指南](https://github.com/Kuddev/pebrel/blob/main/CONTRIBUTING.md)、[架构说明](https://github.com/Kuddev/pebrel/blob/main/docs/architecture.md)和[项目约束](https://github.com/Kuddev/pebrel/blob/main/docs/project-constraints.md)。涉及 UI 文档时，还需要遵循仓库的国际化规则。

<div class="contribution-flow">
<div><strong>准备</strong><span>Fork 仓库，从目标分支创建独立工作分支。</span></div>
<div><strong>修改</strong><span>一次 PR 只解决一个清晰问题，复用已有职责和状态来源。</span></div>
<div><strong>验证</strong><span>运行与改动对应的测试、架构检查、格式检查和真实产品检查。</span></div>
<div><strong>提交</strong><span>写清用户问题、实际改动、验证结果和仍未验证的范围。</span></div>
</div>

## 准备开发环境

1. Fork `Kuddev/pebrel`，从 PR 目标分支创建自己的工作分支。
2. 按仓库的 [INSTALL.md](https://github.com/Kuddev/pebrel/blob/main/INSTALL.md) 安装当前平台需要的依赖。
3. 使用 `rust-toolchain.toml` 固定的 Rust 工具链，不要随意切换工具链版本。
4. 构建实际的 GPUI 产品界面：

```sh
cargo build --locked -p nebula --bin pebrel --features gpui-shell
```

如果只是修文档，应优先运行文档自己的构建与测试，不需要为了文字改动制造无关的产品代码变化。

## 保持改动容易审查

- 一个 PR 只承担一个概念上的改动；不要顺便重构无关文件。
- 先说明用户可观察到的问题，以及哪些既有行为必须保持。
- 共享规则应继续由现有权威模块负责，UI 只适配，不复制第二套状态或持久化逻辑。
- 有自然测试入口时，为问题增加最小、聚焦的回归验证。
- 不要通过放宽预算、移除测试、增加跳过参数或扩大忽略范围来让 CI 变绿。
- 不提交构建输出、本地探针、包含隐私/凭据的截图或临时调查文件。

仓库的 PR size 检查会限制过大的源码改动；文档、锁文件和资源与源码预算的计算规则不同。需要大范围重构时，先拆分成可独立审查的步骤。

## 本地快速检查

架构检查需要 Python 3.11+。把 `<PR-base-commit>` 替换成实际目标分支的基准提交：

```sh
python3 scripts/check_architecture.py --base <PR-base-commit>
python3 -m unittest scripts.tests.test_architecture_budgets scripts.tests.test_architecture_dependencies scripts.tests.test_architecture_governance
cargo test --manifest-path tools/i18n-contract/Cargo.toml --locked
cargo test -p nebula-settings
cargo test -p nebula --test file_line_budget
cargo fmt --all -- --check
```

涉及正式应用行为时，再运行：

```sh
cargo check -p nebula --bin pebrel --features gpui-shell --tests --locked
```

还应运行与你修改区域直接相关的行为测试。无法在当前机器上验证的平台能力，需要在 PR 描述里明确写出，而不是把“能编译”描述成“UI 已验证”。

## 修改文档网站

文档站源文件位于 `docs/site/`。本地构建：

```sh
python -m pip install -r docs/site/requirements.txt
python docs/site/build.py
python -m unittest discover -s docs/site -p test_site.py
```

新增公开文档源文件时，还要把具体路径加入 `.gitignore` 中的精确 allowlist。不要直接放开整个 `docs/` 目录。

文档应从用户任务出发：先告诉用户去哪里、执行什么、看到什么结果，再补限制和背景。不要把源码结构说明写成用户指南。

## 提交 Pull Request

PR 描述至少应包含：

| 内容 | 应写什么 |
| --- | --- |
| 问题 | 用户实际遇到了什么，或为什么需要这个变化 |
| 改动 | 哪些行为发生变化；哪些关键行为保持不变 |
| 验证 | 实际运行过的命令、测试和平台结果 |
| 缺口 | 没有运行的 UI、平台或打包验证 |
| 风险 | 持久化格式、依赐、线程、公共接口等是否受到影响 |

早期需要设计反馈时可以先开 Draft；准备合并前再切换 Ready。维护者根据实际 diff、测试结果、平台验证和代码审查决定是否可以合并。

## CI 会检查什么

Ready PR 会运行完整的原生平台矩阵；Draft 会省略部分较慢的平台任务。核心检查包括架构合同、格式与矩阵规划、源码规模，以及 Linux、Windows 和 macOS 的相应测试/编译任务。

来自 fork 的首次贡献可能在 GitHub Actions 中显示等待维护者批准。这是 GitHub 的安全边界，不应通过提高 token 权限或改用不安全触发方式绕过。

## 提交前自检

- 工作分支基于正确的目标分支。
- Diff 只包含当前功能需要的文件。
- 没有提交凭据、个人路径、私密日志或无关截图。
- 相关测试真实运行过，PR 中没有夸大验证范围。
- CI 失败时修原因，而不是关闭门禁。
- 如果改动引入新的架构责任或跨层依赖，已经按仓库要求记录和讨论。

完成这些步骤后，PR 会更容易复现、审查和维护。
