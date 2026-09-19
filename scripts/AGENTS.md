# Maintained scripts and governance rules

- 维护脚本是产品工程合同的一部分：默认离线、UTF-8、确定性，并对缺失输入、解析失败、读取失败和不完整结果 fail closed。
- 新门禁必须说明不变量，提供一个合法通过样例、一个违规失败样例和已知误报的反例测试；先修门禁缺陷，再把它称为强制规则。
- 不通过提高预算、扩大排除、自动加例外、删除测试或 `continue-on-error` 消除失败。规则错误时提交窄范围政策修订和回归。
- 依赖和行数策略的单一权威仍在 `architecture/`；脚本读取同一来源，不复制第二份常量清单。
- 发布脚本还必须遵循 [`../packaging/AGENTS.md`](../packaging/AGENTS.md)。
- 门禁语义、扫描边界或治理流程的非平凡变化写入 `architecture/notes/scripts/<area>/`。
