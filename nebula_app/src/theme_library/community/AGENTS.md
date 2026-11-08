# Community theme catalog

- 只在显式入口联网：主题选择器的「社区」页、重试按钮，以及 `pebrel theme catalog/install`。启动、设置页打开和后台任务都不拉取目录或预览图。
- `mod.rs` 校验目录行，规则与 `pebrel-community/themes` 的 `scripts/check_catalog.py` 对齐；单行无效只跳过该行。客户端额外只取主题自身仓库的预览图。
- 目录登记的大小和 SHA-256 是下载的固定值；校验通过后仍走 `package::CheckedPackage`，不另写解包或安装规则。安装只加入主题库，不应用设置。
- 来源回执写在 `metadata.package.origin`，只用于标记“已安装”，不是校验依据。
- 网络请求每次新建 `update_proxy::agent`；没有共享客户端、缓存或常驻服务。GUI 调用方拥有任务、代次与取消标志。
- 设计依据见 [按需社区目录](../../../../architecture/notes/nebula_app/theme_library/community/2026-11-08-on-demand-community-catalog.md)。
