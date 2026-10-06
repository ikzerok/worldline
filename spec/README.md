# worldline 语言规范

语言与共同文件格式的真源文档，实现（`worldline/core`、`worldline/runtime`）以其为准。产品 0.28.0不新增 DSL；默认语言仍为 1.9，最高 1.13。规范描述契约和验收要求，不表示当前候选已完成全部测试。使用路线见[文档索引](../docs/README.md)，变化与状态见[CHANGELOG](../CHANGELOG.md)。

| 文档 | 内容 |
|---|---|
| [source-coordinates.md](https://github.com/ikzerok/worldline/blob/main/spec/source-coordinates.md) | 当前精确源码的物理行、Unicode字符列、预览与过期定位守卫 |
| [source-outline.md](https://github.com/ikzerok/worldline/blob/main/spec/source-outline.md) | 当前单文件声明结构、精确源码范围、预算与过期定位保护 |
| [workspace.md](workspace.md) | 工作区目录边界、递归索引、外部刷新与完整导出 |
| [source-lifecycle.md](source-lifecycle.md) | 源码新建、引用、文件移动与统一事务 |
| [entity-source-move.md](entity-source-move.md) | 单实体声明精确移源、完整静态/运行证明与零修改守卫 |
| [terms.md](terms.md) | v1.9 术语表:事件 / 状态 / 身份标签 / 独立锚点 / 演练记录 |
| [syntax.md](syntax.md) | 词法、语法、语句、表达式、森林结构与准入、效果与锚点、与 Ink 的差异 |
| [semantics.md](semantics.md) | 执行模型、选择与汇聚、故事线/准入/效果/锚点、输出契约、存读档 |
| [diagnostics.md](diagnostics.md) | 诊断结构、编号表、机器接口(JSON) |
| [relations.md](relations.md) | 执行图的条件上下文与目标准入、时间无环偏序、导出格式 |
| [agent-protocol.md](agent-protocol.md) | 机器接口契约:CLI JSON 模式、`wl-agent` JSON-RPC 协议 |
| [states.md](states.md) | 状态替换/增加/移除、世界叙事身份迁移、变更出处与旧档兼容 |
| [replay.md](replay.md) | 确定性叙事重放、检查点、条件解释与访问覆盖 |
| [route-comparison.md](route-comparison.md) | 同稿双路线真实对照、继承覆盖与有界状态动作回源 |
| [catalog.md](catalog.md) | 通用标签、独立锚点与反查、文件引用及工程打包 |
| [presentation.md](presentation.md) | 地图、网络视图、展示文档、关系 DTO、命令边界与跨仓契约 |
| [vector-scene.md](vector-scene.md) | 原生矢量模型、编辑事务、受限 SVG、viewport 裁剪、迁移与公开白名单 |
| [scene-protocol.md](scene-protocol.md) | scene CLI/JSON-RPC、核心计划摘要、修订与失败边界 |
| [reader-export.md](reader-export.md) | 静态阅读包 v1/v2、字段/章节/地图/附件的明确授权 |
| [reader-site.md](reader-site.md) | v3 世界站、typed 页面、别名搜索、稳定路由、profile 与发布预算 |
| [workspace-snapshot.md](workspace-snapshot.md) | 后台当前稿快照、只读/墓碑保真、有界传输与过期结果保护 |
| [language-versions.md](language-versions.md) | 支持版本、显式能力预览/启用及兼容边界 |
| [manuscript.md](manuscript.md) | 书稿章节、来源引用与正文边界 |
| [manuscript-review.md](manuscript-review.md) | 可信全分支作者审稿、真实来源与预算 |
| [templates.md](templates.md) | 内容模板与注册文档 |
| [markdown-import.md](markdown-import.md) | 外部 Markdown 的预览、损失核对和应用 |
| [localization.md](localization.md) | 显式本地化交换与原稿保护 |

地图与展示文档以 `presentation.md`、`vector-scene.md` 及其 [schemas](schemas/README.md) 为共同契约；[矢量场景 Schema](schemas/vector-scene.schema.json) 的形状检查不能替代引用、锁定、资源或事务验证。
结构以各机器 Schema 为准，行为回归位于源码测试中；字段模板的产品单源为 [templates.catalog.json](templates.catalog.json)，其中没有故事实例值。

规范随工具发行包提供，无需在线文档；作品导出只保留工作区自身内容，不自动插入规范。0.29 起发布归档不附带仓库历史演示工程或规范样例；集成测试及必要历史兼容夹具位于 `runtime/tests/`。

[v1.9 创作模型补全](authoring-v19.md)：权限统一为状态、独立锚点、条件关系与正文概览。

- [显式语言1.11](language-1.11.md)：规则、片段、类型化集合、角色台词以及兼容契约。

- [持续资料约束](schemas.md)：显式1.12的schema、field身份、bind、统一校验与影响计划
- [可见禁用选择](choices.md)：显式1.12 enable/disabled、presentation协商和兼容
- [当前稿查找替换](search-replace.md)：作用域、保护token、基线和原子事务

- [静态人物强引用](character-refs.md)：显式1.13双能力保护、typed property/schema/template、身份改写与披露边界

- [bounded-execution.md](bounded-execution.md)：普通演练的预算、可恢复outcome、CLI与JSON-RPC协商。

- [真实变量写入证据](https://github.com/ikzerok/worldline/blob/main/spec/variable-write-evidence.md)：路线验证区间、实际前后值、共享预算与可信来源

- [静态可执行依赖](https://github.com/ikzerok/worldline/blob/main/spec/executable-context.md)：调用与全局读写的明确语境、同快照索引与显式消费
- [可读试玩审阅](https://github.com/ikzerok/worldline/blob/main/spec/playthrough-report.md)：重新验证的单路线、实际输出来源、私密交接与预算
