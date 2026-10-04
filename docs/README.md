# worldline 文档索引

当前产品版本为 0.20.0；默认语言 1.9、最高 1.13。先按任务选择入口，精确语义始终以 [spec](../spec/README.md) 为准。完整功能与实际验收边界见 [CHANGELOG](../CHANGELOG.md)。

## 作者入门

- [创作手册](handbook.md)：工作区、资料、叙事、状态、协作与交付
- [从资料到可信重放](author-route.md)：结合[栖雪山站示例](../examples/snowline-seeds/README.md)完成双路线与安全改稿
- [显式能力启用](explicit-capabilities.md)：预览已有语言/资料能力，核对变化后确认
- [工作区契约](../spec/workspace.md)：递归源码、引用边界、原字节、保存冲突与完整备份

## 真实路线对照

- [作者操作](route-comparison.md)：同稿验证两条实际路径、停止状态、动作来源与边界

## 当前稿里的工程问题

- [问题使用说明](problems.md)：CLI/RPC、来源角色、有界摘录、旧报告与导航守卫
- [诊断来源合同](../spec/diagnostic-sources.md)、[来源上下文合同](../spec/problem-source-context.md)：真实生产入口、schema 1 加法与完整传输预算
- [0.19 配对版本说明](../../worldedit/docs/releases/v0.19.0.md)：能力、语义兼容和最终验收边界

## 0.15 地图与 SVG

- [原生矢量场景](../spec/vector-scene.md)：编辑模型、曲线/文字/组、样式变换、迁移与事务
- [受限 SVG profile](../spec/vector-scene.md#4-支持的-svg-profile)：接受与拒绝的内容、根 viewport 裁剪及交换边界
- [场景机器接口](../spec/scene-protocol.md)：CLI/RPC 预检、预览、应用、导出和基线要求
- [展示与资源阈值](../spec/presentation.md)：地图/图层契约、预算和性能验收目标
- [配对编辑器操作](../../worldedit/docs/vector-authoring-0.15.md)：绘制、选择、节点、绑定、撤销与保存

## 静态读者站

- [完整世界站 v3](../spec/reader-site.md)：类型页面、别名搜索、稳定路由、profile、资源审计和预算
- [阅读包原有边界](../spec/reader-export.md)：v1/v2、逐字段授权、书稿、地图与附件
- [编辑器发布步骤](../../worldedit/docs/reader-publishing.md)：选择 → 核对 → 预览 → 确认；原生 ZIP 与 Web 下载

对象别名和允许的类型结构随 v3 对象选择公开，属性与附件仍分别授权。引用不会扩大范围；阅读站不执行故事、不提供下载后的访问权限，也不能替代完整工程备份。

## 语言、接入与维护

- [规范总表](../spec/README.md)：语法、运行、资料、诊断和共同格式
- [API 与工具接入](api.md)、[JSON-RPC/CLI 契约](../spec/agent-protocol.md)：以当前注册方法与 DTO 为准
- [语言版本](../spec/language-versions.md)、[确定性重放](../spec/replay.md)、[有界演练](../spec/bounded-execution.md)：兼容与真实执行范围
- [后台快照](../spec/workspace-snapshot.md)：精确当前稿、墓碑、只读与传输限制
- [发布构建](release.md)：配对、源码和包；历史版本记录不代表当前候选已完成验收

当前 0.20 的完整门禁、性能与真实平台覆盖以配对版本说明为准；旧版实测不能代替本版验证。规范中的性能数值是验收目标，不是已取得的测量结果。
