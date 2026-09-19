# Build, packaging and release rules

- 处理版本号、构建、打包、自动更新、GitHub Release、Changelog 或 Release Note 前，完整阅读本地 `.agents/memory.md` 的发布章节和 `docs/release-notes/AGENTS.md`。
- `.agents/memory.md` 是经真实发布核验的本地记忆，不是临时推理草稿。只有代码、脚本、GitHub 元数据或真实运行结果已核实后才能更新；过时结论直接修正并注明核验日期。
- 所有发布相关命令使用 UTF-8。工作区可能包含用户的未跟踪探针、构建目录和截图，只能显式暂存本次文件，不得清理、全量暂存或重置它们。
- 正式发布不得使用 `-SkipBuild` 或 `-AllowStale` 绕过新鲜度检查，也不得把 workspace 默认构建产生的 legacy shell 当作 GPUI 产品包。
- 构建和输出目录必须位于用户明确允许的路径。
- GitHub 推送使用普通非强制推送。发布后二进制版本标签不可因说明文档修订而移动。
- 结束前核验 Release 标题、标签、正文、真实资产文件名、空资产标签、大小、SHA256、分支与版本标签指向；创建命令成功不等于发布核验完成。
- 构建图、资产集合、更新协议或发布事务的非平凡变化写入 `architecture/notes/packaging/`。
