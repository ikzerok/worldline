# worldline 关系图契约

**版本:** 1.9
**生产者:** `worldline-core::analysis`
**消费者:** `wl graph` / `wl timeline`(Mermaid 导出)、worldedit 关系图与时间线视图、测试

---

## 1. 结构

```text
GraphNode {
  name: String            // "event" 或 "event.scene"
  is_event: bool
  file: String, line: u32 // 源位置
  choice_count: u32, word_count: u32
  storyline: String       // 所属故事线 id(v1.5)
  seq: u32                // 故事线内序号,1 起(v1.5)
  summary: Option<String> // 事件简述(v1.5)
  characters: Vec<String> // with 引用的角色(v1.5)
  perm: Option<String>    // 旧兼容字段；旧权限归一后通常为 None，准入见边字段
}

GraphEdge {
  from: NodeId, to: NodeId
  kind: EdgeKind          // Divert | Choice | Enter | Drift
  label: Option<String>   // Choice: 选择标签;Drift: "漂流"
  contexts: Vec<TransitionContext> // 显式条件与选择路径
  target_requirement: Option<String> // 进入目标所属事件的准入表达式
  file: String, line: u32 // 边的源位置
}

TransitionContext {
  conditions: Vec<String> // 同一上下文中共同成立的显式条件
  choices: Vec<String>    // 外到内的选择原始文案（不求值内插）
}

RelationGraph {
  nodes: Vec<GraphNode>, edges: Vec<GraphEdge>
  ids: Map<String, NodeId> // 完整节点名到索引
  entry: NodeId, depth: Vec<u32>
  storyline_order: Vec<(String, String)>  // 故事线 (id, 显示名),声明序优先
}
```

分析产物另含 `anchors: Vec<AnchorDecl>`(源码中全部 `anchor` 语句,
含节点归属、名称、说明、源位置),供时间线视图打点。
独立锚点对象另在 `Analysis.catalog.anchors`，不与这个旧字段合并，见 [catalog.md](catalog.md) §4。

## 2. 边的语义

| kind | 含义 |
|---|---|
| Divert | `-> target` 语句产生的控制流转移(含选择体内部的跃迁) |
| Choice | 选择被执行时的控制流:取该选择体内预序首个跃迁的目标;无跃迁的选择(落穿汇聚)不产生边 |
| Enter | 结构进入:节点到其**每个直接子场景** |
| Drift | 漂流跃迁 `->>`:跨故事线控制流转移 + 主线切换(v1.5) |

约定:
- `-> END` 不产生边(END 不是节点);
- 隐式汇聚不产生边(它不转移节点,只是组内顺序流);
- 环允许(合法叙事),分析层的 A206 负责提示风险,图如实呈现。

### 2.1 条件上下文与准入

`contexts` 中每个元素是一条被收集到的显式条件/选择上下文；同一元素内的 conditions 共同约束该处，不同元素保留不同路径。嵌套 if 的条件会累计；else if/else 包含前面分支条件的否定。choice 的条件、`once` 的“此选择尚未选取”说明及原始选择文案也会保留。

`contexts: []` 只表示没有收集到上下文，不证明无条件可达；`[{conditions: [], choices: []}]` 表示已找到无显式条件或选择约束的源码处，仍不证明必然执行。隐式落穿、上游状态变化、完整路径可满足性与随机结果不在该字段的求解范围。

`target_requirement` 是目标准入要求的展示表达式，可包含归一后的 `has`；无要求时为 null。同事件内部场景跳转不重复准入，字段为 null；跨事件进入场景使用目标所属事件的要求。源上下文与目标准入分开保存，因为 exit 效果可能先改变状态；不能将它们当作同一时刻已求值的结果。

这两类字段由核心分析生成，支持解释图线与定位源码，不做自然语言冲突检查。表达式是展示字符串，不承诺保留原空白或括号写法，不能替代 AST 编辑接口。

## 3. 出口格式

- `wl graph file.wl` → Mermaid `flowchart` 文本:Divert 实线,
  Choice 虚线(带标签),Enter 点线,Drift 粗线(`==>` 标"漂流");
- `wl timeline file.wl` → Mermaid `flowchart LR`:每条故事线一个
  `subgraph`(泳道),事件按序号排布;线内 Divert 画箭头,
  Drift 以 `==>|漂流|` 跨泳道;Choice/Enter 不进入时间线投影;
- Rust API 直接暴露 `RelationGraph` 与 `anchors`,编辑器画布消费;
- 机器可读的结构化导出(`wl graph/timeline --json`、`wl-agent analyze`)
  见 [agent-protocol.md](agent-protocol.md) §2.2/§2.3/§3.3,字段与本文 §1 一致;
- 导出格式只是投影,**图的结构真源只在 core**,CLI/编辑器不得反向推断。
- 当前 Mermaid 导出保留边类型与选择标签，未完整呈现 contexts/target_requirement；需要这两类信息时读取 JSON 或 Rust API。

## 4. 布局提示(非契约)

图中附 `depth: u32`(从 entry 沿边最短路径),供编辑器做分层布局;
非连通分量 depth = u32::MAX,编辑器单独成列。

## 5. 故事线过程流投影

- 泳道 = 故事线(按 `storyline_order` 纵向排列);
- 事件按 `seq` 横向排布于所属泳道;
- `seq` 优先取事件的显式 `at` 序号,否则取故事线内声明序;相同时按事件 ID 排序;
- 线内 Divert 为泳道内箭头;Drift 为跨泳道弧线(标注"漂流");
- `analysis.anchors` 提供正文手动锚点的位置，消费者可附加打点；该数据不是独立锚点目录;
- 准入条件可由关系边的 `target_requirement` 解释；不得只依赖旧 `perm` 字段识别身份要求。

## 6. 世界与角色视图(v1.6)

`Analysis.world` 为唯一世界观或 null,含 id、display、description、properties、file、line。
`symbols.characters[id]` 在显示名与源位置之外增加 `properties`(键到字面量值)、
`relations`(target、label、file、line)、`events`(去重后的关联事件 ID)。
人物关系图与反向事件列表直接消费此数据,不得在 UI 层另行解析。

## 7. 时段与部分顺序（1.6；1.13 显式跨时段扩展）

`Analysis.timeline` 为 `{periods, events, edges, order_scope, status}`。时间线由 core
唯一分析；CLI JSON、RPC、Mermaid 与原生编辑器消费同一结果，不另推导排序。

- `periods[]` 保留 id、display、parent、file、line，新增 `root: string | null`。
  root 是沿显式 `within` 链到达的、已声明且没有 parent 的顶层时段；缺失上级、
  循环包含或重复时段造成根身份不明确时为 null。parent 始终是直接上级。
- `events[]` 保留 event、period、rank。period 始终是作者写的直接 `during`，
  rank 仍仅计算同一直接时段内显式边的零起拓扑层级，绝不偷偷改成根内层级。
  新增 root、order_scope（均为时段 ID 或 null）、root_rank（整数或 null）、status。
- `order_scope` 顶层为 `direct_period`（1.9—1.12）或 `root_period`（显式1.13）。
  事件的 order_scope 是该版本实际比较范围的 ID；旧版为直接时段，1.13 为 root。
  1.13 的 root_rank 由同一根内全部合法显式边计算；旧版为 null。
- `edges[]` 保留 before、after，新增 root（可为空）、order_scope（时段 ID）、
  file、line，源码位置指向书写 `follows` 的后继事件头；边不会因传递性被自动补齐。
- 顶层与事件 `status` 为 `complete` 或 `partial`。它表示分析完整性，不表示全序。
  任一编译 error（包括解析错误造成的空/缺失节点）使整体和所有事件为 partial，
  所有 root_rank 为 null。保留的旧 rank 此时仅是尽力投影，不得作为可信层级展示。
  合法空时间线可以 complete；空数组本身从不证明无错误。调用方仍须展示编译诊断。

默认1.9与显式1.10/1.11/1.12继续要求每条 `follows` 两端位于同一直接时段，否则
A213。显式1.13允许两端在不同直接时段，但必须共享唯一明确顶层 root；根自身内事件
和任意深度后代均可参与。独立根、未声明时段、没有时段、缺失前驱、自环与任何跨层
循环均不可合法排序（A213；无效父关系仍为A219）。诊断指向后继事件头，已知前驱
作为关联位置。父关系编辑立即重算整图并拒绝破坏已有约束的事务。

父子、兄弟声明顺序、显示名、数字形似日期、`at`、正文及控制流都不产生时间边。
同根不同 rank 也不直接证明两事件有先后关系；须沿显式边可达才有该约束。同 rank
不代表同时发生，独立根的 rank 不可比较。原生图按直接时段分组，1.13 在同根范围
按 root_rank 分列，注明范围和不完整状态，保留连线及源导航。Mermaid 保留层级和
显式边并标出比较范围，不把源码声明顺序当时间。详见 [language-1.13.md](language-1.13.md)。

执行图的分支、汇合、回环不因此被禁止；时间线不调度事件，不改变运行 fingerprint
或存档兼容性。不提供日期、时长计算、日历转换或自动事实推理。

## 8. 独立语义关系查询（语言 1.10）

`relation_type` 与 `relation_def` 是作者内容资料，和本文件前述的旧控制流
`RelationGraph` 分开。关系实例保存稳定 ID、类型、完整 `from_ref`/`to_ref`、
说明、来源、scope 与非执行 property（包括显式 `ref` 对象引用）；它不会生成事件、场景、选择或运行状态，
也不进入运行指纹。旧 `CharacterRelation` 继续按 1.9 规则参与指纹，只在
`Catalog::legacy_relation_handles` 产生临时兼容投影。

`Catalog::query_relations(target, RelationQueryOptions)` 由 core 唯一实现邻接
查询。默认 `depth=1`，调用方最多请求 `depth=2`；每个结果最多 250 节点、500
条关系边。节点按 (深度, kind, id) 排序，边按稳定关系 ID 排序；同端点的多条
关系全部保留。循环只再次遇到已访问对象时停止，A→B、B→C 不推导 A→C，也不
因为 directed 关系生成反向副本。沿 `to` 端读取时，边仍携带原始端点与 ID，
但 `label` 使用类型的 `inverse_display` 投影。

结果带 `schema_version=1`、起点、应用的深度上限、节点/边、`truncated` 和 continuation。
达到上限时只截断当前查询结果，原始目录关系不变。`continuation` 带起点、筛选、
深度、实际边界 `frontier` 和下一页 `offset`；`Catalog::continue_relations` 消费它，
或调用方将 offset 传给相同目标/筛选的 `query_relations`。offset 是确定性广度遍历
中已经返回的独立边数，不是源码行号；遍历每个对象的邻接关系按稳定 ID 排序，
各页输出仍按边 ID 排序。跳过已返回边时仍遍历它们，保证第二层不会因翻页断开。
每页保留起点并包含全部返回边的两端，仍遵守 250/500 上限；同端点超过 500 条
关系可以逐页读完。自定义低于一条边所需节点数的上限或 max_edges=0 不能前进，
继续入口使用默认展示上限。offset 仅适用于同一目录快照及相同筛选；工作区修订
变化后调用方必须从 0 重新查询，不得混合两次快照。查询不修改 Project、源码或展示文档。

## 9. 查询时专题投影（CAP-08A）

`Analysis::query_topic_projection(&TargetRef, TopicProjectionOptions)` 是 family/organization
关系与 character/place 历史的只读组合查询。映射只属于一次调用，不写 `.wl`、
展示文档或 Project。其输入字段为：

```text
TopicProjectionOptions {
  role_mapping: BTreeMap<relation_type_id, role_label>
  offset, history_offset: usize
  depth: u8
  direction: RelationQueryDirection
  scope_refs: Vec<TargetRef>
  include_unscoped, include_period_children: bool
  max_nodes, max_edges: usize
}
```

`role_mapping` 的 key 必须是当前目录已声明的稳定 `relation_type` ID，value
必须是非空白调用方标签。未知类型或空白标签返回查询错误；空映射合法，但语义
关系结果只有起点节点、没有边。映射中列出的类型是唯一关系筛选；不得按类型
ID/显示名、端点 kind、`entity_type` 或旧 `CharacterRelation` 标签猜测映射。

core 错误使用稳定 code：`UNKNOWN_TARGET`、`UNKNOWN_SCOPE`、
`UNKNOWN_RELATION_TYPE` 与 `EMPTY_ROLE_LABEL`。
输出边在既有关系字段上增加 `role`，仍保留关系 ID、原始类型、端点、方向、来源、
scope 与作者标签。相同端点的多边不合并；读取反向端只使用既有 `inverse_display`
投影，不写反向副本。

`TopicProjectionResult` 含 `relations`、`history` 与总 `truncated`。`relations` 复用
`Catalog::query_relations` 的深度、方向、范围、节点/边限制、确定顺序与 continuation；
continuation 另携本次 `role_mapping`。`cycle_hint` 报告当前页 directed 环或忽略方向的
端点拓扑环；相同端点的平行边不单独构成环，原始边仍分别保留。它只描述当前页，
因此 `cycle_hint=false` 不证明全图无环。

机器 DTO 的形状为：

```json
{
  "schema_version": 1,
  "target": {"kind": "character", "id": "lin"},
  "relations": {
    "depth": 1,
    "nodes": [{"ref": {"kind": "character", "id": "lin"}, "depth": 0}],
    "edges": [{"id": "edge_1", "relation_type": "parent", "role": "生亲",
      "from_ref": {"kind": "character", "id": "lin"},
      "to_ref": {"kind": "character", "id": "mei"},
      "scope_refs": [],
      "label": "父母", "direction": "directed", "source_note": null,
      "file": "world.wl", "line": 1}],
    "cycle_hint": false,
    "truncated": false,
    "continuation": null
  },
  "history": {
    "items": [{"event": {"kind": "event", "id": "arrival"},
      "source": {"kind": "with"}, "file": "world.wl", "line": 2}],
    "events": [{"target": {"kind": "event", "id": "arrival"},
      "file": "world.wl", "line": 2, "time_status": "period_ranked",
      "period": {"kind": "period", "id": "era"}, "rank": 0, "anchors": []}],
    "temporal_edges": [],
    "parallel_groups": [],
    "target_anchors": [],
    "offset": 0,
    "truncated": false,
    "next_offset": null
  },
  "truncated": false
}
```

`time_status` 为 `unknown` 时 `period` 与 `rank` 均为 null；映射关系源的
`history.items.source` 为 `{kind:"relation", id, relation_type, role, scope_refs}`。顶层
`truncated` 是关系页与历史页截断状态的逻辑 OR，continuation/next_offset 仍分属两页。


历史成员资格严格按来源区分：

- `character` 只列出显式 `with` 该角色的事件，关联来源标为 `with`。未标 scope 的
  `with` 关联在请求 scope 时只在 `include_unscoped=true` 下返回；不从旧关系、文本提及、
  锚点或事件控制流推导参与关系。
- `entity` 且 `entity_type == "place"` 只列出映射类型中 `from_ref.kind == "event"`、
  `to_ref == target` 的显式语义关系。每条边分别输出关系 ID、映射 role 与源码来源；
  正文链接、地图标记、其它端点方向不建立地点历史。
- 历史事件及其直接锚点返回完整 `TargetRef`；锚点是单独引用，不制造事件成员关系。

`history.items` 按事件 ID、再按关系 ID 稳定排序；多条地点关系指向同一事件时事件
对象可去重，但每条关联项保留。`history_offset` 按关联项分页，`next_offset` 与
`history.truncated` 明示局部历史页不完整。`max_nodes` 限制历史页中的不同事件数，
`max_edges` 限制关联项数；关系图与历史各自遵守同一已登记上限。

若关联/边预算达到上限但 offset 没有前进，则 `truncated=true`、continuation/`next_offset`
为 null；调用方须提高预算或重新查询，不得循环请求同一页。`max_nodes=0` 沿用关系
查询的根节点最小值；`max_edges=0` 可产生空且 truncated 的关系/历史页。

历史时间仅投影现有 Timeline：事件有显式 period 时返回 `period` 与现有 `rank`，
新增 `root`、`order_scope`（完整 period TargetRef）、`root_rank` 与 `status`。
不完整投影的 `time_status="partial"`，rank/root_rank 均为 null；无时段仍为
`time_status="unknown"` 且不补日期。历史顶层 `timeline_status` 保留全图完整性，
即使当前历史页为空也不掩盖编译错误。`temporal_edges` 只返回当前页事件之间已有
的直接 `before → after` 约束。`parallel_groups` 只分组相同实际 order_scope/有效显示 rank 的事件；旧版本为直接
period/rank，1.13 为 root/root_rank，不完整事件不分组；
parallel 只表示同一拓扑层级、不表示同时发生。不同 rank 不推出先后，数组按稳定
对象 ID 排序，不生成全序、日期或冲突裁定。工作区诊断仍由调用方单独返回。

每页只读当前 Analysis 快照，不能跨 `workspace_revision` 混用 offset。结果和查询
不修改 Project、源码、Timeline、运行状态或 fingerprint。
