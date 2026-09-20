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

## 7. 时段与部分顺序(v1.6)

`Analysis.timeline` 为 `{periods, events, edges}`。periods 元素含 id、display、parent、file、line；parent 为上级时段 ID，无上级为 null。
events 元素含 event、period、rank;edges 元素含 before、after。
rank 是同一时段先后约束的拓扑层级(零起),无约束为 0;同级事件不意味着同时发生。
时间线消费者以时段分组、约束分列、同列并列,明确区分时间约束与执行跃迁。
时段里的事件展示不再暗示声明顺序就是发生顺序。
时间约束允许多前驱与跨故事线，但必须在同一时段内且无环（A213）；执行图的分支、汇合、回环不因此被禁止。正文概览按故事线与 seq 排列仅供阅读，不生成时间边。
## 8. 独立语义关系查询（语言 1.10）

`relation_type` 与 `relation_def` 是作者内容资料，和本文件前述的旧控制流
`RelationGraph` 分开。关系实例保存稳定 ID、类型、完整 `from_ref`/`to_ref`、
说明、来源、scope 与字面量 property；它不会生成事件、场景、选择或运行状态，
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
