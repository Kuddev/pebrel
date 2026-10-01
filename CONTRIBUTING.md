# Contributing to Pebrel / 贡献指南

Pebrel 1.6 uses the `pebrel` command, Pebrel installation directories, and
`PEBREL_*` environment variables. Legacy configuration and integration identifiers
remain compatibility inputs; follow the [identity migration decision](docs/architecture-decisions.md#adr-0003---pebrel-16-identity-migration)
when changing them. Internal Rust names migrate one module at a time. The completion package now uses
`pebrel-completions`; its source directory remains `nebula-completions/` until the
scan-root migration is validated. See the [package identity decision](architecture/notes/nebula-completions/2026-09-30-package-identity.md).

## Required reading

- [Architecture and module ownership](docs/architecture.md)
- [Enforced contracts and review rules](docs/project-constraints.md)
- UI changes must also satisfy the **UI 设计约束 / UI design constraints** chapter
  in that document: interaction states, result feedback, hit targets and visual acceptance.
- [Causal note policy](architecture/notes/AGENTS.md) for new non-trivial decisions
- [Legacy decision archive](docs/architecture-decisions.md) for accepted historical context
- [Evidence behind the rules](docs/engineering-evidence.md)
- [Internationalization](docs/internationalization.md) when changing UI text

These rules apply equally to maintainers, outside contributors and coding agents.
They constrain responsibilities and behavior, not personal taste. A demonstrated
problem in a rule is a reason to review the rule, not to conceal or ignore a failure.

## Ways to contribute

- **Report a bug:** use the existing issue form. Include the version/build, OS,
  display scale, relevant settings, minimal steps, expected result and actual result.
  Search existing issues first. Remove tokens, credentials, private paths and command
  history from logs or recordings before sharing them.
- **Propose a feature:** describe the workflow and user need before prescribing an
  implementation. For broad changes, agree on scope before writing a large PR.
- **Translate or improve docs:** follow the language registry/catalog contract;
  preserve named placeholders and review terminology, clipping and fallback. Do not
  mark a language complete merely because English placeholders were copied into it.
- **Fix or test code:** start with a focused regression and an existing module's
  tests. Add fixtures or improve coverage when that is the actual contribution.

No programming contribution is required to be useful. Clear reproduction steps,
native-language review and accessibility feedback also help the project.

## Commercial promotion policy

PRs, Issues, and their comments must not be used for advertising, commercial
promotion, or traffic solicitation. This includes promotional provider presets,
referral links, signup incentives, and unrelated brand placement disguised as
feature contributions or bug reports. Close any PR or Issue containing unauthorized
promotion directly; no further maintainer confirmation is required. Apply this
rule once the promotional content and lack of authorization are established.

For commercial cooperation, sponsorship, or other business requests, email
[fickleheartedkeys@163.com](mailto:fickleheartedkeys@163.com). Do not negotiate
commercial arrangements through PRs or Issues. Sponsor content explicitly approved
by the maintainer is handled within that approval's scope; it does not authorize
other promotional submissions.

Review the actual user need and changed content. Necessary compatibility fixes,
Agent integrations, and upstream attribution are not advertising merely because
they name a service or product. A small diff, passing CI, or first-time contributor
status does not establish a need for a provider addition or override this policy.

Review the submission body, actual diff, attachments, and linked destinations,
not just the title. Relevant provider integrations still require a demonstrated
project need; a working endpoint or a vendor's own description is not approval.
Verify any claimed sponsorship with a traceable maintainer decision. Record only
the approval's scope in public, not private email or commercial terms.

Once an unauthorized promotional submission is verified, close it with a brief
policy link and email contact, without repeating promotional links or copy.
Withdraw any mistaken approval and remove that change from pending integrations.
If an unrelated account posts an advertisement under an otherwise legitimate PR
or Issue, moderate that comment rather than closing the legitimate contribution.
This is a content-review rule, not an automatic keyword/domain blacklist.

严禁利用 PR、Issue 及其评论进行广告宣传、商业推广或引流，包括以功能贡献或
问题反馈为名植入服务商推广、返利链接、注册优惠和无关品牌内容。核实属于未经授权的
推广后，直接关闭对应 PR 或 Issue，无需再次向维护者确认。
商业合作、赞助及其他商业需求须通过上述项目邮箱沟通，不得通过 PR 或 Issue 商业洽谈。
经维护者明确批准的赞助内容按批准范围处理，不构成对其他推广提交的授权。
审阅应核实真实用户需求及改动内容；必要的兼容修复、Agent 集成和上游署名不因包含
品牌名称就被视为广告。改动小、CI 通过或首次贡献者身份均不能代替需求审查。

审阅须检查正文、实际差异、附件及链接去向，而非只看标题；新增服务商即使接口可用，
仍须有真实项目需求，服务商自述不等于维护者批准。声称已获赞助授权时，应核对可追溯的
维护者决定；公开记录只说明批准范围，不披露私人邮件或商业条款。
核实为未授权推广提交后直接关闭，简短引用规则及商业邮箱，不重复传播推广链接或文案。
若此前误批，应撤销认可并从待合并集成中排除。无关账号在正常 PR 或 Issue 下发布广告时，
处理该广告评论，不因此关闭正常贡献。本规则依赖内容审查，不使用关键词或域名一刀切。

This section is the authoritative policy. Agent instructions, review guidance,
and submission templates link here and summarize it for their audience.
When changing the policy or contact address, update those summaries together.
READMEs introduce the product and do not duplicate this policy. / 本节为规则的
唯一权威说明；规则或联系邮箱变化时，同步检查代理指引、审阅约束和提交模板中的
摘要与链接。README 用于项目介绍，不重复收录本规则。

## First code contribution

1. Fork the existing `Kuddev/pebrel` repository and create your work branch from the
   PR target branch. See [INSTALL.md](INSTALL.md) for environment prerequisites.
2. Use the pinned Rust toolchain in `rust-toolchain.toml`. Build the actual GPUI
   product, not an accidentally substituted legacy executable:

   ```sh
   cargo build --locked -p nebula --bin pebrel --features gpui-shell
   ```

3. Make the focused change, run the checks below, and open a PR against the agreed
   target. Draft PRs are useful for early design feedback; describe remaining work.
4. Respond to review and rerun affected checks after changes. Maintainers decide
   readiness from the actual evidence, not from checked boxes alone.

Do not include build outputs, local probes, screenshots containing secrets or
unrelated generated files. Preserve third-party license/attribution notices and
identify the source of any externally copied code or assets for license review.

Keep one-off test scripts and probe outputs under `tmp/`; external project checkouts
and investigation notes belong in the reserved `research/`, `reference-projects/`
or `external-probes/` local directories. These are not vendored build dependencies:
do not blanket-ignore `third_party/` or maintained test/diagnostic sources. `docs/`
is private by default; public documentation needs an exact reviewed allowlist entry
in `.gitignore`. Do not publish competitor studies or HTML prototypes by opening a
whole documentation subtree. Ignore rules do not untrack files already in Git;
raise any existing tracked private artifact for an explicit maintainer decision.

## Small, reviewable changes

1. State the user-visible problem and the behavior to preserve. Discuss a new
   cross-layer dependency, persisted format, core abstraction or threading model
   before implementing it. Record significant decisions under the owning path in
   `architecture/notes/`; ordinary fixes do not create a note.
2. Keep one conceptual change per PR. A necessary extraction and its behavior
   tests may accompany the feature; unrelated rewrites and formatting may not.
   The `pr-size` check fails above 1500 changed source lines (docs, lockfiles
   and assets excluded); split instead of asking for an exemption.
3. Put shared rules in their existing authority. UI modules adapt those rules;
   they must not fork persistence, state transitions or domain behavior.
4. Add regression tests that fail for the defect, and test error/cancellation
   paths where relevant. Explain what was actually run and what was not.
5. Fill in the PR template. A green build is necessary, not sufficient: a
   maintainer must also review cohesion, public interfaces, compatibility and cost.

## Local checks

The fast checker requires Python 3.11+ and no third-party Python packages. Use
`python` instead of `python3` on Windows if that is your configured interpreter.
Use the actual target branch commit, not the feature branch's own HEAD, for PR
ratcheting. Run without `--base` for current-tree checks during development.

```sh
python3 scripts/check_architecture.py --base <PR-base-commit>
python3 -m unittest scripts.tests.test_architecture_budgets scripts.tests.test_architecture_dependencies scripts.tests.test_architecture_governance
cargo test --manifest-path tools/i18n-contract/Cargo.toml --locked
cargo test -p nebula-settings
cargo test -p nebula --test file_line_budget
cargo fmt --all -- --check
```

Run affected behavior tests and the appropriate real product checks as well:

```sh
cargo check -p nebula --bin pebrel --features gpui-shell --tests --locked
```

`Full native tests` plans validation on every PR. Ordinary shared-code changes
run the complete Linux suite; OS-specific paths and Rust conditional compilation
add the relevant native architectures. Documentation-only PRs retain repository
contracts without a native application build. Dependencies, toolchains, CI,
packaging, shared host boundaries and unknown code roots select all five platforms.
Main pushes, daily scheduled runs, merge groups and manual/reusable calls retain
the complete matrix. The authoritative selector is [`scripts/ci_plan.py`](scripts/ci_plan.py).
New commits cancel obsolete PR runs. Pull requests run tests and
compile checks without building distribution packages. Package validation runs
after matching changes reach `main`, or by explicit manual dispatch; a package
job does not replace the native test suite. These triggers do not configure
required status checks.

GitHub may show **Waiting for approval** for a first-time fork contributor. A
maintainer must inspect the submitted changes and approve that workflow run from
the Actions page before tests start. Pushing more commits does not remove this
GitHub approval requirement; do not run fork code through `pull_request_target`
or give it a write token to work around the wait.

The isolated i18n test compiles production files, not copied implementations. It
does not test real window layout, every OS integration, or the full application.
Existing platform build/package checks still apply. Never turn a metadata-only
compile check into a claim that UI tests or a packaged application were run.

## Review and enforcement

`architecture-contracts`, `lint`, `pr-size`, the five
`Tests (<os>)` jobs and both `Release workspace (<os>)` jobs are required checks
on `main`, together with Code Owner approval; see the
[activation checklist](docs/project-constraints.md#server-side-activation).
Draft and ready PRs use the same path policy. The required lint job validates the
complete Git merge-base diff before requesting native runners. Renames include
both old and new paths; Rust OS/architecture conditions are inspected in both
file revisions. Missing commits or unreadable source fail planning.

Actual compilation and tests appear in `Native tests (<os>)` jobs. The existing
five `Tests (<os>)` and two macOS `Release workspace (<os>)` contexts are lightweight
Ubuntu policy reports. Their summaries explicitly distinguish **executed** from
**not run**. Every selected native job must succeed; failure, cancellation or an
unexpected skip prevents passing reports. No conclusion is copied from another run.
Mac release compilation still runs on the selected native Mac, sharing its checkout
and toolchain while keeping the existing separate compiled caches.

Shared-code regressions specific to another host may now be found after merge.
Full main/daily validation and existing release packaging/conformance remain;
dispatch the complete native workflow on the intended release commit when needed.
See the [policy decision](architecture/notes/scripts/ci/2026-09-29-path-based-native-validation.md).
Every required check must succeed before merging. Marking a draft ready without changing its commits does
not repeat the matrix; source updates and reopen events run it again. Native CI
uses pinned `cargo-nextest` for unit and
integration tests, followed by `cargo test --doc` with the same workspace features;
the production feature graph and release workspace are still checked separately.
Native-test PR jobs restore Cargo caches without uploading merge-ref snapshots. The default branch
publishes reusable snapshots per dependency/toolchain configuration; a cache hit
never skips current-commit tests. Downloads are shared between architectures of
the same OS, while compiled targets remain isolated by architecture and SDK.

Auxiliary UI screenshots must not add a second automatic upstream PR build matrix.
Keep contributor-owned screenshot builds and publication in the source fork;
upstream capture needs an explicitly requested run and read-only target-cache
restoration. Screenshot evidence must identify its source commit and does not
replace native regression tests. The registered upstream screenshot workflow is
disabled as recorded in the [cache ownership decision](architecture/notes/scripts/ci/2026-09-28-upstream-screenshot-cache-ownership.md).

Local hooks are convenient, but bypassable; they are not the enforcement boundary.
Submitting a workflow or `CODEOWNERS` file does not configure server-side rules.

Do not silence a failing contract by increasing a budget, removing test coverage,
compressing code, adding `continue-on-error`, or broadening an exclusion. If the
contract is wrong, submit a focused policy fix with a reproducer and both positive
and negative tests. Review a justified policy change separately from unrelated
feature work; there is no routine `--skip-architecture` option.

## 中文摘要

- 先读架构图、工程合同和决策记录；按职责拆分，不按行号切片。
- 一个 PR 只做一件事；改动超过 1500 行源码（不计文档、lockfile、资源）`pr-size` 会失败，请拆分。
- Draft 和 Ready PR 共用路径策略：文档不构建原生应用，普通共享代码跑完整 Linux 测试，平台路径及条件编译追加相应原生架构，依赖/CI/打包等改动跑全量。main、每日定时和手动验证保留五平台。十项必需检查名称不变，结果汇总明确注明未运行的平台，已选择的测试失败仍阻止合并；原生套件和 doctest 不删减。
- 附加截图留在贡献者 fork 中生成；上游按需执行，不重复自动跑整套截图构建，也不上传 PR 专属的大型编译缓存。截图与原生回归测试不能互相替代。
- 2000 行是现有仓库的防灾上限，800 行只提示审查，不是“大厂标准”。
- 普通功能 PR 不得增加存量债务；有问题的规则可以修订，但要有反例、测试和维护者审批。
- 新增核心抽象、依赖方向、持久化或线程模型改变要先说明设计，不强迫每个小修复写 ADR。
- 修改热路径要给出成本证据；不能为了“可扩展”增加没有实际用途的框架。
- PR 要附实际测试结果；本地钩子和勾选框不代替服务端必需检查与人工评审。
