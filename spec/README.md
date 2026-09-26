# worldline 语言规范

语言真源文档,实现(`worldline/core`、`worldline/runtime`)以其为准。

| 文档 | 内容 |
|---|---|
| [workspace.md](workspace.md) | 工作区目录边界、递归索引、外部刷新与完整导出 |
| [terms.md](terms.md) | v1.9 术语表:事件 / 状态 / 身份标签 / 独立锚点 / 演练记录 |
| [syntax.md](syntax.md) | 词法、语法、语句、表达式、森林结构与准入、效果与锚点、与 Ink 的差异 |
| [semantics.md](semantics.md) | 执行模型、选择与汇聚、故事线/准入/效果/锚点、输出契约、存读档 |
| [diagnostics.md](diagnostics.md) | 诊断结构、编号表、机器接口(JSON) |
| [relations.md](relations.md) | 执行图的条件上下文与目标准入、时间无环偏序、导出格式 |
| [agent-protocol.md](agent-protocol.md) | 机器接口契约:CLI JSON 模式、`wl-agent` JSON-RPC 协议 |
| [states.md](states.md) | 状态替换/增加/移除、世界叙事身份迁移、变更出处与旧档兼容 |
| [replay.md](replay.md) | 确定性叙事重放、检查点、条件解释与访问覆盖 |
| [catalog.md](catalog.md) | 通用标签、独立锚点与反查、文件引用及工程打包 |
| [presentation.md](presentation.md) | 地图、网络视图、展示文档、关系 DTO、命令边界与跨仓契约 |

`presentation.md` 及其 [schemas](schemas/README.md) 是地图、展示文档和共同文件格式的唯一规范真源。
验证夹具位于 [examples](examples/README.md)，仅用于设计级 Schema/引用检查，不是可运行工程或产品资源。

规范随工具发行包提供，无需在线文档；作品导出只保留工作区自身内容，不自动插入规范。语言运行示例位于仓库根 `examples/`，共同契约验证夹具位于本目录 `examples/`，集成测试位于 `runtime/tests/`。

[v1.9 创作模型补全](authoring-v19.md)：权限统一为状态、独立锚点、条件关系与正文概览。
