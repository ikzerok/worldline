# worldline 文档索引

- [逐处审阅与安全替换](selective-replace.md)：精确命中、当前稿上下文、选择式事务与失败边界

当前产品候选版本为 0.28.0；默认语言 1.9、最高 1.13。先按任务选择入口，精确语义始终以 [spec](../spec/README.md) 为准。完整功能与实际验收边界见 [CHANGELOG](../CHANGELOG.md)。

## 作者依赖与试玩交接

- [静态依赖契约](https://github.com/ikzerok/worldline/blob/main/spec/executable-context.md)：规则/片段/变量使用处及CLI/RPC明确opt-in
- [试玩报告使用](https://github.com/ikzerok/worldline/blob/main/docs/playthrough-report.md)：当前稿重新验证后生成可读Markdown，明确私密内容范围

## 当前源码行列

- [使用与接入](https://github.com/ikzerok/worldline/blob/main/docs/source-coordinates.md)：当前光标、精确文本预览与跳转守卫
- [正式契约](https://github.com/ikzerok/worldline/blob/main/spec/source-coordinates.md)：物理行、Unicode字符列、边界与预算

## 当前源码结构

- [使用与接入](https://github.com/ikzerok/worldline/blob/main/docs/source-outline.md)：core只读投影、准确来源与过期守卫；没有新增CLI/RPC遥控入口
- [正式契约](https://github.com/ikzerok/worldline/blob/main/spec/source-outline.md)：声明类型、层级、当前源码版本与硬预算

## 作者入门

- [创作手册](handbook.md)：工作区、资料、叙事、状态、协作与交付
- [从资料到可信重放](author-route.md)：结合[栖雪山站示例](../examples/snowline-seeds/README.md)完成双路线与安全改稿
- [显式能力启用](explicit-capabilities.md)：预览已有语言/资料能力，核对变化后确认
- [工作区契约](../spec/workspace.md)：递归源码、引用边界、原字节、保存冲突与完整备份

## 全分支作者审稿

- [审稿操作](manuscript-review.md)：保留静态条件、选择和去向，查看同快照人物身份与逐段来源
- [正式契约](../spec/manuscript-review.md)：不执行、完整性、来源绑定和资源上限

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

当前 0.26 的完整门禁、性能与真实平台覆盖以配对版本说明为准；旧版实测不能代替本版验证。规范中的性能数值是验收目标，不是已取得的测量结果。

- [从变量终值找到实际写入](https://github.com/ikzerok/worldline/blob/main/docs/variable-write-evidence.md)：两路线的成功写入、初始化边界与机器接口
