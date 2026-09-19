# worldline 设计文档

这里维护 worldline 仓库自己的产品与架构主文档，以及后续实施所需的需求追踪资料。

| 文档 | 用途 |
|---|---|
| [产品需求文档](PRD.md) | worldline 负责的内容、展示和验收目标 |
| [系统架构文档](ARCHITECTURE.md) | 核心、运行时、工作区与展示文档的落地边界 |
| [需求清单](shared/REQUIREMENTS.md) | 需求 ID、依赖、阶段和验收映射 |
| [机器可读需求](shared/requirements.json) | 需求、工作包和设计文档的机器索引 |
| [需求追踪](shared/TRACEABILITY.md) | 需求、工作包和计划测试之间的映射 |
| [实施计划](shared/IMPLEMENTATION.md) | 工作包拆分、依赖和迁移顺序 |
| [测试与验收计划](shared/TEST_PLAN.md) | 产品落地后的行为、安全和性能验收 |
| [内容模板目录](shared/CONTENT_TEMPLATES.md) | 可选创作栏目和提示 |
| [研究记录](shared/RESEARCH.md) | 设计依据、源码证据和未验证事项 |
| [PR 描述模板](tools/PR_TEMPLATE.md) | 实现变更的范围与验证记录 |

地图、关系、展示文档和跨仓库共同文件格式只在 [worldline/spec/presentation.md](../../spec/presentation.md) 及其 Schema 中维护。这里的设计文档引用该规范，不复制契约内容。

这些文档描述设计目标和验收口径；文档、Schema 或样例存在不代表对应 Rust、桌面或 WASM 功能已经实现。
