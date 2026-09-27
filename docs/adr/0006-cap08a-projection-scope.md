# ADR-0006：CAP-08A 家族/组织与人物/地点历史投影范围

- 状态：已决定（用户于 2026-09-27 选择查询时显式映射）。
- 关联：CAP-08A / [worldline#22](https://github.com/ikzerok/worldline/issues/22)。
- 范围：只读查询投影；不涉及持久映射、模拟或引擎适配器。

## 背景

core 已有稳定 `relation_type` / `relation_def`、显式关系查询、人物 `with` 参与事件、独立锚点与 `period` / `follows` 偏序。缺口是把用户选定的关系类型映射到本次查询角色，并把明确的人物/地点历史来源与现有时间资料组合返回。共享持久化映射不是本票目标。

## 决定

采用查询本地映射，不新建 `.wl` 声明、展示文档或映射注册表。公开 core seam 为 `Analysis::query_topic_projection(&TargetRef, TopicProjectionOptions)`；CLI 为 `wl relations project`，agent RPC 为 `relation.project`。所有分析仍从当前 core `Analysis` 得出，CLI/RPC 只解析参数和序列化结果。

`role_mapping` 是 `relation_type` 稳定 ID 到调用方显示角色的映射。只包含本次显式提供的类型；空映射不返回语义关系边，不根据 ID、显示名、端点 kind 或 `entity_type` 猜测。未知类型 ID 或空白角色名报查询错误。多个映射项即使端点相同也保留每个原始关系 ID、类型、方向、来源和 scope，不合并边；关系反向读取沿用现有显示投影，不存储反向副本。

人物历史只以事件 `with` 角色引用建立成员关系。地点历史只以映射中关系类型的显式 `event → entity(place)` 端点建立成员关系。正文链接、锚点、地图标记、旧人物关系和普通提及不会制造事件成员关系；显式锚点只作为可导航的引用返回。每条人物/地点历史关联保留事件身份与来源；地点的平行语义关系不合并。

时间投影只返回现有 `Timeline` 信息：明确 period、现有 rank 与直接 `before → after` 约束。无 period 的事件标为 `unknown`，不赋日期或位置；同一直接 period/rank 的事件可列入 `parallel_groups`，这里只表示相同拓扑层级，**不表示同时发生**。不同 rank 不推出先后，结果不按 chronology 排序。冲突或模糊的作者关系不解析、裁定或折叠；查询仅保留被显式映射的原始边，原有 `follows` 诊断不改变。

查询复用现有关系语义与边界：默认深度 1、最大深度 2；关系节点最多 250、关系边最多 500；`scope_refs`、`include_unscoped` 与显式时期子树扩展沿用 `Catalog::query_relations` 规则。`cycle_hint` 报告当前页 directed 环或忽略方向的端点拓扑环；相同端点的平行边不单独构成环，`false` 不证明全图无环。关系与历史关联分别给出 offset/truncated；预算截断但 offset 不前进时不提供 continuation/next_offset，要求调用方提高预算。

## 取舍

- 查询时映射不需要新格式、能力协商、迁移或写入生命周期，但调用方必须为每次查询提供映射；共享复用映射不在本票实现。
- 返回原始关系、明确 `with`、既有偏序与 unknown/parallel 提示，保留多义与资料缺口；不提供“家谱真相”、历史裁定或总时间线。
- 不新增 UI 布局写入。任何未来持久映射需独立决策展示文档 schema、未知字段、能力与删除影响。

## 验收与实际证据

在隔离工作树的 CAP-08A 代码中，新增公共 core 测试覆盖显式/空/未知映射、亲生/养亲角色、同端点多边与 directed/undirected `cycle_hint`、同显示名跨 kind、period/version scope、正向截断与零预算不产生停滞 continuation、人物 `with`、地点显式事件→地点关系、文本提及排除、锚点引用、undated、parallel 非同时语义、partial-order edges 与分离的争议 period 关系；并断言查询前后 fingerprint 相同。CLI 集成测试覆盖 role JSON、scoped 输出、cycle_hint、unknown type 错误及源码字节不变；RPC 测试覆盖 `relation.project` 的 history、cycle_hint、unknown type 与 `-32602`。

已实际启动 `wl` 并运行 `wl relations project .scratch/cap08a-smoke --target entity:harbor --role-mapping-json '{"happens_at":"地点"}' --scope period:era --scope entity:version_one --depth 2 --json`：输出 `ok:true`、`cycle_hint:false`、保留 `arrival_harbor` 的 role/source/scope、列出 period/rank，`truncated:false`。实际 `wl-agent` 的 `compile` → `relation.project` → `shutdown` 行协议 smoke 同样成功，且 `cycle_hint:false`、保留同一 scope。临时 smoke 工程已清理。以上为聚焦检查；项目全量验证由 integration owner 运行。
