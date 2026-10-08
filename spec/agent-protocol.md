# worldline 机器接口契约(agent 协议与 CLI JSON 模式)

**协议版本:** 1（语言 v1.10 entity 字段扩展）

0.15 的原生矢量预览、原子应用和安全SVG交换见 [scene-protocol.md](scene-protocol.md)，能力为 `authoring.vector_scene.v1`；语言默认版本与runtime不因此改变。

CAP-01A 的[就地建档组合意图](authoring-intents.md)由 core Project API、CLI 与 agent RPC
共同提供；既有单对象写入命令仍不代表组合事务能力。

时段包含扩展：`timeline.periods[]` 新增 `parent: string | null`，保存直接上级 ID。父子层级由 core 验证，未知上级及循环包含为 A219 编译诊断。CLI 与 RPC 同时返回该字段，不影响会话状态和运行指纹。

世界时间投影扩展（显式语言1.13）：`timeline` 新增 `order_scope` 与 `status`，
period/event/edge保留直接身份并附明确root/scope；event新增root_rank，旧rank仍限直接时段内部边。
全图编译错误为partial且root_rank为null，即使空图也不伪装complete。CLI/RPC使用相同core
DTO；`timeline --json` 编译失败仍返回 `type:"compile_failed"`、`ok:false`、诊断及
标为partial的timeline，退出码1；RPC compile失败结果同样附timeline而不创建story_id。
完整字段与不可比较边界见 [relations.md §7](relations.md)。

资料导航扩展：CLI `catalog --json` 与 agent `analyze.catalog` 的目录新增 `aliases`（target/name/file/line）和 `text_links`（source/target/label/file/line/column）数组；正文引用同时出现在 references。属于向后兼容的附加字段，旧消费者可忽略。未知别名／正文链接目标为 A218 编译诊断；格式错误为 P004；故事层仍返回 `ok:false`，不变成 JSON-RPC 协议错误。播放输出只包含链接的显示文字，不增加运行记录或另一份状态。
**生产者:** `wl`(JSON 模式)、`wl-agent`(`worldline-agent` crate)
**消费者:** 外部 agent 程序、CI、测试
**依据:** 本仓库语言规范与 core/runtime 的公开模型。

---

0.22 新增只读 `wl manuscript-review` / RPC `manuscript.review`，共享 core 全分支审稿 DTO、完整源快照校验及预算，详见 [manuscript-review.md](manuscript-review.md)。不改变旧阅读与发布权限。

## 0. 原则

1. **分层**:人类交互走 `wl` 人类模式与 worldedit;机器驱动走本文档定义的
   两条通道——一次性 CLI JSON 命令与 `wl-agent` 有状态协议。两者都只是
   core/runtime 的投影,不引入第二套解析或分析。
2. **真相唯一**:诊断、图、时间线、状态视图一律产自 `worldline-core` /
   `worldline-runtime`;接口层只做序列化与转发。
3. **契约先行**:方法表、字段、错误码的任何变更先改本文档,再改实现。
4. 本文档只定义**机器视图**;语言语义见 `syntax.md` / `semantics.md`,
   诊断编号见 `diagnostics.md`,图结构见 `relations.md`。

## 1. 总则

- 全部机器输出为 UTF-8;JSON 字段名 snake_case;`serde_json` 紧凑风格
  (无缩进)。
- **map 类字段(vars、visits、symbols 内的映射等)键序不定**,消费方不得
  依赖其顺序;有序信息一律使用 `*_order` 字段或数组。
- 退出码约定(`wl` 各子命令通用):
  - `0` 成功;
  - `1` 故事层失败(编译存在 error 诊断、运行期错误，或 `check` 发现工作区只读诊断);
  - `2` 用法 / IO 失败(文件无法读取、参数错误)。
- **故事层失败是正常结果**,不是协议错误:CLI 以退出码 + JSON 表达,
  `wl-agent` 以 `{"ok": false, ...}` 结果表达。协议层错误仅指消息本身
  不合法(见 §3.2)。

工作区诊断与故事编译诊断分域。`diagnostics[]` 只表示当前
`CompileResult` 的故事层诊断；读取 `.world/project.json`、注册展示文档及其
能力声明得到的作者工作区诊断放在 `workspace_diagnostics[]`，不会注入
`CompileResult.diagnostics`。`read_only` 在工作区存在这些诊断时为 `true`，表示
可以继续查询目录和资料，但不能通过作者编辑接口写盘。查询命令可以在
`read_only: true` 时保持 `ok: true`；`wl check` 则以退出码 1 和 `ok: false`
提示工作区不可安全写入。未知 `language_version` 或 `required_features` 都以
`WS003` 工作区诊断报告。

## 2. `wl` CLI JSON 模式

新增 `wl catalog <目录或入口> [--tag ID] [--recursive] [--kind 类型] [--json]`。
JSON 为 `{ok, catalog, matches, diagnostics, workspace_diagnostics, read_only}`;
其中 `diagnostics` 是故事编译诊断，`workspace_diagnostics` 是工作区只读诊断。
catalog 含 objects/tags/assets/states/anchors/marks/attachments/references,
每个对象有 target:{kind,id}、display、file、line;标签/素材与链接也保留声明位置。
无 --tag 时 matches 为所有对象,有 --tag 时为直接或递归命中,按 --kind 可进一步过滤。
未知标签视为用法失败;重复路径按对象 ID 去重。`analyze` 结果增加同样的 catalog 字段,
属于协议版本 1 的向后兼容扩展。详见 [catalog.md](catalog.md)。

人类模式输出保持不变;`--json` 切换机器输出。标志:`--load=<存档.json>`、
`--save=<存档.json>`、`--language-version=1.9|1.10|1.11|1.12|1.13`、`--json`。未指定
`--language-version` 时,目录或入口若位于带 `language_version` 的工程清单中,
由清单选择语言版本;没有清单的旧调用仍固定使用 1.9。直接 source API 和旧
CLI 调用不会因出现 `entity` 文本而隐式升级。

`wl catalog-query <目录或入口> --query '<JSON DTO>' [--offset N] [--page-size N]`
按 core 的 `CatalogQuery` DTO 查询资料；可选 `--max-candidates N` 设置候选预算。
续页通过 `--cursor '<JSON 游标>'` 传入上一页 `query.next`，使用游标时不能再传分页选项。
`--json` 结果的公共工作区字段与 `workspace check` 相同，`query` 字段包含 core 的
分页结果（`summary`、`snapshot`、`offset`、`total`、`items`、`next`、`diagnostics`）。
顶层 `diagnostics` 仍只属于故事编译域，`workspace_diagnostics` 保留工作区诊断。
查询错误以 `{ok:false,error:{code,message},query:null}` 返回；游标绑定查询和当前
Project 快照，变更后返回 `STALE_CURSOR`，调用方应从第一页重查。只读工作区仍可成功查询。
此命令与既有 `wl catalog` 并存，后者的标签目录输出保持原契约。

`wl authoring-intent preview|apply <目录或入口> --intent-json '<JSON DTO>' --json`
调用 core `Project::preview_authoring_intent` / `apply_authoring_intent`。DTO 必须带
`expected_baseline` 和 `target`，可带 `selection`、`placement`；JSON 形状见
[authoring-intents.md](authoring-intents.md)。`preview` 返回候选目标、引用影响、变更文件和
候选基线但不写入作者内容；`apply` 完整验证后通过 Project 的可恢复保存协议写入。
成功结果含 `{ok, operation, target, reference_impact, changed_files, baseline,
new_baseline, diagnostics, workspace_diagnostics, read_only}`；合法 DTO 的组合
失败返回 `{ok:false,error:{code,message},...}`，用法错误退出码为 2，组合或保存失败为 1。

`wl markdown import preview|apply <工程目录> --source <Markdown目录> --baseline <基线> [--json]`
调用 `Project::preview_markdown_import` / `apply_markdown_import`。应用另外要求 `--plan-digest`
以及显式损失确认 `--accept-losses`；从 1.9 工程导入还要求 `--allow-language-upgrade`。
确认只在 apply 阶段提供；未确认的 preview 仍返回完整损失和候选计划，`can_apply` 为 false。
`--id-map-json` 提供 source-relative-path 到稳定 ID 的显式映射，`--namespace` 选择输出命名空间。
preview 返回可审阅映射、冲突、损失和全部待写路径，不写文件；apply 重扫输入并校验预览摘要、内容基线与目标路径。
成功结果为 `{ok, operation, plan, changed_files, baseline, new_baseline, workspace_diagnostics, read_only}`；`baseline` 始终为应用前工程基线，`new_baseline` 在 apply 成功后为持久化的新基线（preview 时为 null），CLI 与 RPC 同义。preview 的 `changed_files` 为空。成功写入通过 Project 可恢复保存协议；失败返回 `{ok:false,error:{code,message},...}`，用法错误退出码为 2，预览/应用失败为 1。输入契约见 [markdown-import.md](markdown-import.md)。

`wl reader-export preview|apply <工程目录或入口> --selection-json '<JSON DTO>' [apply: --plan-digest 摘要 --out 新目录] [--json]` 调用 core `Project::preview_reader_export` / `export_reader_site`。preview 只读当前缓冲，返回 `plan`（作者可见的 `included` 与 `exclusions`、`content_baseline`、`plan_digest`），不写目标；apply 必须提供原选择、摘要和新目录。摘要绑定当前内容基线和被选附件字节；应用前重新验证，过期返回 `STALE_PLAN`，其他应用失败返回 `EXPORT_FAILED`。生成内容、显式允许范围、离线资源与完整备份差异见 [reader-export.md](reader-export.md)。

### 2.6 `wl entity` 作者资料编辑

`wl entity create <目录或入口> --id ID --kind 类型 --display 名称`
创建实体;`update` 使用相同参数修改显示名、分类、description 或 property;
`delete` 使用 `--id ID` 删除实体。`--description` 设置描述,
`--property name=value` 可重复,值为字符串、有限数值或 `true`/`false`。
此 CLI 参数暂只接受标量；core 的源码编辑 API 可用语言 1.10 的显式
`ref("kind", "id")` 设置对象引用，Catalog JSON 将其序列化为 `{"kind":"…","id":"…"}`。
编辑命令要求工程清单明确选择 1.10,并使用 `Project::edit` 与保存基线检查;
若同时提供 `--baseline` 且工作区内容基线不同,输出故事层失败而不写盘。
JSON 成功结果为 `{ok, operation, entity, catalog, language_version, baseline,
workspace_diagnostics, read_only}`；写入成功时 `read_only` 为 `false`。
失败为 `{ok:false,error:{code,message},diagnostics,workspace_diagnostics,read_only}`。
`code` 使用英文稳定标识,
`message` 使用中文。外部磁盘修改、保存事务或引用影响会以 `CONFLICT` 或
`EDIT_FAILED` 返回,不伪造成功。

### 2.7 工作区与地图资料查询

`wl workspace check <目录> --json`、`wl maps list <目录> --json` 和
`wl relations <目录> --target KIND:ID [--offset N] [--depth 1|2]
[--direction outgoing|incoming|both] [--type TYPE] --json` 是只读机器查询。三条命令均在当前磁盘内容上打开工程，使用
`Project` 与 core 的统一分析、展示索引和关系索引；接口层不得重新解析源码或拼接
关系边。输出至少包含以下公共字段：

```json
{
  "ok": true,
  "schema_version": 1,
  "language_version": "1.10",
  "workspace_revision": "<content baseline>",
  "diagnostics": [],
  "workspace_diagnostics": [],
  "read_only": false,
  "truncated": false,
  "continuation": null
}
```

`workspace_revision` 是此次打开工作区的内容基线投影；它只用于标识本次查询快照，
不能替代保存冲突检查或运行 fingerprint。`diagnostics` 只放故事编译诊断，注册清单、
展示文档、未知能力和地图格式诊断放在 `workspace_diagnostics`。查询即使
`read_only: true` 仍可返回 `ok: true`；`workspace check` 在存在错误诊断或只读诊断时
返回退出码 1。

`workspace check` 另外返回 `stats`，与 `check --json` 使用同一 core 分析统计。
`maps list` 返回 `maps`（按稳定地图 ID 排序的 `MapDocument` 映射）和
`references`（按完整 `TargetRef` 排序的 `{target, placements}` 数组）。地图索引产生的
诊断归入 `workspace_diagnostics`；地图原始文档仍由 Project 保留，CLI 不写入展示文档。

`wl-agent` 的 `workspace.check` 与 `maps.list` 接受 `{path}` 或已打开工程的
`{project_id}`，每次先刷新当前 Project，再从同一快照返回上述公共字段、诊断、内容基线
以及 `stats` 或地图 `maps`/`references`。外部刷新冲突附在 `conflicts`，不阻止只读查询；
`project.analyze` 也返回同一快照的 `maps` 与 `references`。

`relations` 返回 core `RelationQueryResult` 的 `target/depth/nodes/edges`，并保留
`truncated` 与 `continuation`。`--offset` 默认 0，用于同一快照的续查；默认深度为 1，最大深度为 2；默认和最大节点/边上限
由 [presentation.md](presentation.md) §11 维护。反向读取只改变同一边的投影，不生成新的
关系 ID。TargetRef 字符串按第一个冒号分隔；当 `kind=file` 时，ID 余串中的冒号必须
保留，以支持 `file:C:/作品/章节/第一章.wl` 这样的 Windows 规范路径。未知 target、
类型或参数是用法失败（退出码 2）；故事编译错误仍是可解析的
故事层结果（退出码 1）。

关系资料的写入命令使用同一份 core `Project` 编辑 API。工程清单必须明确选择语言
1.10，并声明 `content.relations.v1`；读取和 compile API 不因缺少该能力而隐藏关系资料，
但结构写入会以故事层失败返回且保持零改动：

```text
wl relation-type create DIR --id TYPE --display 显示名 [--inverse-display 反向名]
wl relation-type update DIR --id TYPE [--display 显示名] [--direction directed|undirected]
  [--clear-inverse-display] [--clear-from-kind] [--clear-to-kind]
wl relation-type delete DIR --id TYPE
wl relation create DIR --id REL --type TYPE --from KIND:ID --to KIND:ID [--description 文案] [--scope KIND:ID] [--property name=value]
wl relation update DIR --id REL [--description 文案] [--source-note 来源] [--scope KIND:ID] [--property name=value]
  [--clear-source-note] [--clear-scope] [--clear-properties]
wl relation delete DIR --id REL
wl relations promote preview DIR --source character:A --target character:B --label 标签 --id REL --type TYPE [--scope KIND:ID] [--property name=value]
wl relations promote commit DIR --source character:A --target character:B --label 标签 --id REL --type TYPE [--scope KIND:ID] [--property name=value]
```

上述命令都接受 `--baseline` 与 `--json`。命令在写入前刷新源码、清单和已注册
展示文档；外部修改、基线过期、编译错误或工作区只读诊断都会返回 `ok:false`，并
且不把失败伪装成提交成功。成功结果包含 `operation`、`catalog`、`baseline`、
`diagnostics`、`workspace_diagnostics` 与 `read_only:false`；关系类型资料放在
`relation_type`，关系实例放在 `relation`。关系实例 `relation` 可附 `scope_refs` 与
`properties`，提升预览的 `draft` 必须完整保留这两项。`relation-type` 和 `relation` 的 ID 是
稳定身份，更新不得借此改名。删除仍由 core 做关系端点和地图引用影响检查。
CLI 的 `--clear-*` 只接受 `update`，同一字段不能同时使用设置参数和清空参数，
在 `create` 上会返回用法错误。JSON-RPC 更新使用 `null` 清空单个可选字符串，使用空
数组或空对象清空 `scope_refs` 或 `properties`；省略字段则保留原值。

旧人物关系只能通过 `relations promote preview` 先生成提升预览，再使用
`relations promote commit` 写入；`source`、`target`、`label` 与 `occurrence`
组成的 `LegacyRelationHandle` 是临时读取句柄，不是可持久化 ID。预览包含迁移前后
运行 fingerprint 差异和待写入资料，预览本身不修改源码。

### 2.9 `wl relations project` 查询时专题投影

```text
wl relations project <目录或入口> --target KIND:ID
  [--role-mapping-json '<JSON object>'] [--offset N] [--history-offset N]
  [--depth 1|2] [--direction outgoing|incoming|both] [--scope KIND:ID]*
  [--include-unscoped] [--include-period-children]
  [--max-nodes N] [--max-edges N] --json
```

`role-mapping-json` 可省略（等于 `{}`）；对象的 key 是当前工程中已有的稳定
`relation_type` ID，value 是本次查询的非空白角色标签。它只对本次请求生效。空映射
不猜测关系类型，因此 `relations.edges` 为空；character 的 `with` 历史仍可返回。
未知 target、scope、关系类型、角色标签或格式错误为用法失败（退出码 2）；源码有
故事编译 error 时按查询命令的公共规则返回诊断且不输出部分成功投影。

JSON 返回 `{ok, schema_version, language_version, workspace_revision, target,
relations, history, truncated, diagnostics, workspace_diagnostics, read_only}`。
`relations` 是 core `TopicProjectionResult.relations`，每条边除关系原字段外增加
`role` 与 `scope_refs`；`cycle_hint` 为当前页 directed 环或忽略方向的端点拓扑环，
相同端点的平行边本身不构成环；false 仅描述当前页，不能证明全图无环。
`history` 返回 `items/events/temporal_edges/parallel_groups/target_anchors/offset/truncated/next_offset`。总 `truncated` 为关系或历史任一部分截断。
`offset` 续关系页，`history-offset` 续历史关联页；调用方必须在相同
`workspace_revision`、target、mapping 与 filters 下续查，否则从 0 重查。预算截断但
offset 未推进时 continuation/`next_offset` 为 null，调用方应提高预算而非重复同一页；
地图/网络展示、源码、运行状态及 fingerprint 均不写入。

`max_nodes=0` 仍保留起点节点；`max_edges=0` 可返回空且 `truncated=true` 的关系/历史页。
若预算截断但 continuation/next_offset 不能前进，它们为 null，调用方应提高预算，不能
重复请求同一页。
`wl-agent` 的 `relation.project` 接受 `{story_id, target, role_mapping?, offset?,
history_offset?, depth?, direction?, scope_refs?, include_unscoped?,
include_period_children?, max_nodes?, max_edges?}`，或以 `project_id` 替换
`story_id`。`role_mapping` 是同一 JSON 对象；`scope_refs` 与分页语义沿用
`relation.query`。成功 result 除上述 `relations/history/truncated` 外保留 agent 查询
公共字段及同一快照诊断；参数类型、未知映射类型/target/scope 以 `-32602` 返回。
此方法只调用 core 查询，不修改已打开 Project。


### 2.1 `wl check <file> --json`

见 `diagnostics.md` §3,不在此重复:`{ok, stats, diagnostics[],
workspace_diagnostics[], read_only, language_version}`。工作区存在 `WS003`
等只读诊断时，故事 `diagnostics[]` 仍可为空，但 `ok` 为 `false` 且退出码为 1；
人类模式同时打印中文诊断和“工作区只读”提示。

### 2.2 `wl graph <file> --json`

```json
{ "graph": {
    "nodes":  [ { "name": "start", "is_event": true, "file": "s.wl", "line": 1,
                  "choice_count": 1, "word_count": 0, "storyline": "main",
                  "seq": 1, "summary": null, "characters": [], "perm": null },
                { "name": "hall", "is_event": true, "file": "s.wl", "line": 4,
                  "choice_count": 0, "word_count": 0, "storyline": "main",
                  "seq": 2, "summary": null, "characters": [], "perm": null } ],
    "edges":  [ { "from": 0, "to": 1, "kind": "choice", "label": "走",
                  "file": "s.wl", "line": 2,
                  "contexts": [{"conditions": [], "choices": ["走"]}],
                  "target_requirement": null },
                { "from": 0, "to": 1, "kind": "divert", "label": null,
                  "file": "s.wl", "line": 3,
                  "contexts": [{"conditions": [], "choices": ["走"]}],
                  "target_requirement": null } ],
    "ids": {"start": 0, "hall": 1},
    "entry": 0,
    "depth":  [0, 1],
    "storyline_order": [["main", "主线"]] } }
```

字段与 `relations.md` §1 完全一致;`kind` 序列化为
`"divert" | "choice" | "enter" | "drift"`。编译存在 error 时输出单行
`{"type":"compile_failed","diagnostics":[…]}`(同 §2.4),退出码 1。

每条 `GraphEdge` 新增 `contexts: [{conditions: string[], choices: string[]}]` 和 `target_requirement: string | null`。conditions 累计显式条件与前面分支的否定，choices 保留外到内选择文案；同一上下文中的条件共同约束该处，不同上下文表示不同收集路径。空数组表示未收集到上下文，不证明无条件可达。target_requirement 单独表示目标事件准入；同事件内场景跳转无需重复准入时为 null。两者都不执行表达式，不预测运行必然到达，详见 [relations.md](relations.md) §2.1。

图节点的旧 `perm` 字段仍保留；旧权限归一后通常为 null。消费者应读取准入表达式，不通过该字段是否为空推断是否受身份限制。

### 2.3 `wl timeline <file> --json`

```json
{ "stats": { "events": 12, "scenes": 5, "choices": 20, "words": 1834,
             "storylines": 2, "characters": 3 },
  "anchors": [ { "node": "start", "name": "开局", "note": null,
                 "file": "s.wl", "line": 4 } ],
  "timeline": { "periods": [], "events": [], "edges": [] },
  "graph": { "…同 §2.2,完整 RelationGraph 视图…": true } }
```

### 2.4 `wl play <file> --json [--load=存档.json] [--save=存档.json]`

逐回合行协议:每回合向 stdout 输出**一行** JSON 事件;暂停时从 stdin 读
**一行**十进制整数作为选择(取值为上事件 `choices[].index`,**0 起**;
与人类模式的 1 起序号不同,机器模式以事件中的 index 字段为准)。

事件类型:

```json
{ "type": "turn", "outputs": [...], "choices": [{"index": 0, "label": "甲", "line": 5, "offset": 0}], "state": { ...状态视图... } }
{ "type": "ended", "state": { ... } }
{ "type": "eof", "state": { ... } }
{ "type": "run_error", "message": "...", "node": null, "line": 3 }
{ "type": "invalid_choice", "message": "(输入序号无效)" }
{ "type": "compile_failed", "diagnostics": [...同 check --json...] }
```

- `outputs` 元素为 Output 的机器视图:`{"type":"text","content":"…","new_line":true,"tags":["…"]}`、`{"type":"ended"}`(`semantics.md` §5)。
- 正文输出及 choices 可附带非空 `links: [{start, end, target: {kind, id}}]`。范围为求值后 content / label 的 UTF-8 字节偏移，左闭右开；省略表示没有显式链接。该信息用于 Wiki 导航，不影响选择索引或运行结果。
- 编译存在 error 时输出单行 `compile_failed` 后退出(码 1),不进入循环。
- stdin EOF(故事尚未结束时):输出单行 `eof` 事件后退出,退出码 0
  (与人类模式一致);故事自然结束则输出 `ended` 收束。
- `--save=<path>`:退出前(ended / eof / run_error)将 `Story::save()`
  的存档 JSON 写入该文件;暂停态语义同 `semantics.md` §7。
  人类模式下 `--save` 同样生效。

`wl play` 可选 `--seed N` 指定可复现随机流，`--trace-output=<文件>` 在退出时保存 runtime trace JSON。`wl replay <入口> --trace-json '<DTO>' [--max-steps N] [--time-budget-ms N] --json` 在当前编译产物上受控重放；回放不写 Project。参数/DTO 错误退出码为 2；故事失败、轨迹分歧、预算耗尽或取消退出码为 1，JSON 结果携带 `status`、当前位置、状态差异、覆盖和诊断信息。DTO/状态边界见 [replay.md](replay.md)。

### 2.5 状态视图(state)

由 `worldline_runtime::Story::state_view()` 统一产出,CLI 与 `wl-agent`
共用,字段:

```json
{ "turns": 3, "storyline": "main", "current_node": "start",
  "vars": { "gold": 5 }, "visits": { "start": 1 },
  "perms": [], "met": [], "anchors": [ "...AnchorRecord..." ],
  "states": {}, "state_history": [],
  "paused": true, "ended": false }
```

选项列表不进入状态视图,只出现在各协议事件/结果的 `choices` 字段中。
`perms` 是世界叙事身份状态的旧权限名称投影，不维护第二份集合；独立锚点在 `catalog.anchors`，这里的 `anchors` 仍是运行时记录。


### 2.10 本地化翻译交换

```text
wl localization export preview <工程目录> --selection-json '<DTO>' --json
wl localization export apply <工程目录> --selection-json '<DTO>' --plan-digest 摘要 --out 新包.json --json
wl localization import preview <工程目录> --selection-json '<DTO>' --package 交换包.json --json
wl localization import apply <工程目录> --selection-json '<DTO>' --package 交换包.json --plan-digest 摘要 --json
```

selection 使用 core `LocalizationSelection` DTO：`{schema_version:1, source_locale, target_locale, string_ids}`。export preview 调用 `Project::preview_localization_export`，只读返回 `{ok:true, operation:"preview", plan, baseline, workspace_diagnostics, read_only}`；plan 包含带选中源文与受保护 token 的 versioned UTF-8 `exchange`、`source_baseline`、`plan_digest`、diagnostics 与 `can_export`。export apply 重算计划并调用 `Project::export_localization`，只写入工作区外不存在的新 JSON 文件；返回 `{ok:true, operation:"apply", plan, baseline, output, workspace_diagnostics, read_only}`。

import 读取 `LocalizationExchange` JSON，并要求调用方重复提供导出时的相同 selection；不允许交换包扩大 ID 白名单。preview 调用 `Project::preview_localization_import`，只读返回 `{ok:true, operation:"preview", plan, baseline, workspace_diagnostics, read_only}`；plan 含目标 locale、受影响 ID、诊断、`plan_digest` 与 `can_apply`。即使 `can_apply:false`，preview 仍是成功的审阅结果，不写盘。apply 调用 `Project::apply_localization_import`，重验 package 版本/selection/source revision/source baseline、token 完整性、Project content baseline 与 digest，然后在单一可恢复事务中注册并更新 locale sidecar；成功返回 `{ok:true, operation:"apply", plan, changed_files, baseline, new_baseline, workspace_diagnostics, read_only}`。locale sidecar 不参与 runtime 输出。

apply 验证失败时 CLI 退出码为 1 并返回稳定 `error.code`、中文 `message` 及诊断；输出文件/包读取等 IO 错误退出码为 2。CLI/RPC 只转发 core DTO 和结果，不解析 `.wl`、不自行校验 token。JSON-RPC 方法的确切映射见 §3.3。

## 3. `wl-agent` 协议(JSON-RPC 2.0 · stdio 行分帧)

`wl-agent` 是有状态的机器协议入口:外部 agent 程序 spawn 子进程,经
stdio 收发**行分帧 JSON-RPC 2.0**,驱动 编译 → 检查 → 试玩 → 选择 →
存读档 → 状态查询 全流程。

### 3.1 分帧与处理模型

- 每行一个 UTF-8 JSON 消息；空行忽略；正常 EOF 或 `shutdown` 结束，退出码 0。
- 工具0.32起按原始字节读取完整行，再验证UTF-8。无效UTF-8单行返回
  `{"jsonrpc":"2.0","id":null,"error":{"code":-32700,…}}`，不解释该行、不执行其中
  任何方法、不推进既有Project或Story；随后继续消费下一合法行。错误响应不回显坏字节。
- 真正的底层读取IO错误不能当作EOF，终止并返回退出码2；Interrupted按原行继续重试。
  对端关闭输出导致writer写入失败时，沿用正常结束/退出码0的既有行为。
- 以上修复仅改变分帧错误处理；原有各方法DTO/结果预算保持，不新增或扩大stdio全局
  行大小预算承诺，也不把方法级预算冒充读取整行之前的配额。
- **单线程顺序处理**:上一请求响应完成后才处理下一请求;无并发交错。
- `id` 必须回显;通知(无 id)不响应。
- stdout 上只写协议消息;日志一律走 stderr(当前实现不主动输出日志)。
- 请求中任意 JSON object key 重复时按 parse error `-32700` 拒绝，不采用后者覆盖前者。

### 3.2 错误模型

| 场景 | 表达 |
|---|---|
| 行不是合法 JSON | error `-32700`(data: 原始错误摘要) |
| 不是合法请求对象(缺 method / jsonrpc 不符) | error `-32600` |
| 未知方法 | error `-32601` |
| 参数缺失 / 类型错误 / 未知 story_id / 未知 session_id / 选择越界 | error `-32602`(data: 原因) |
| 编译存在 error 诊断 | result `{"ok": false, "diagnostics": [...]}` |
| 运行期错误(准入失败、指纹不匹配等,即 `RunError`) | result `{"ok": false, "run_error": {message, node, line}}` |

### 3.3 方法表

| 方法 | 参数 | 结果(result) |
|---|---|---|
| `initialize` | `{}` | `{protocol: 1, server: "wl-agent", version, capabilities}` |
| `compile` | `{path, language_version?}` 或 `{source, file_name?, language_version?}` | `{ok, story_id, fingerprint, stats, diagnostics, workspace_diagnostics, read_only, language_version}`;故事编译失败时无 story_id；工程可编译但工作区只读时仍可返回 story_id，`read_only` 为 true |
| `analyze` | `{story_id}` | `{graph, anchors, symbols, stats, world, timeline, catalog, language_version}`(结构化,同 §2.2/§2.3 形状;symbols 为符号表全量) |
| `export` | `{story_id, format}`;format ∈ `graph_mermaid` \| `timeline_mermaid` | `{text}` |
| `session.open` | `{story_id, save?, seed?, capabilities?, max_steps?, time_budget_ms?}`(预算须协商，详见 bounded-execution.md；save 为存档 JSON 字符串；seed 为新会话的非负整数随机种子，不能与 save 同用) | `{session_id, state}` |
| `session.trace` | `{session_id}` | `{trace}`；输出与 CLI 相同的 runtime ReplayTrace |
| `session.checkpoint` | `{session_id}` | `{checkpoint}`；仅用于相同 runtime/schema/fingerprint |
| `session.explain_choices` | `{session_id,include_evidence?:bool}` | `{choices}`；默认旧形状只读解释；显式 true 只返回已实际执行的暂停/失败组证据，没有缓存则空数组；证据契约见 replay.md |
| `trace.replay` | `{story_id, trace, max_steps?, time_budget_ms?}` | `{ok, replay}`；选择/观察不匹配和预算停止为结构化故事结果，不是 JSON-RPC 错误 |
| `session.continue` | `{session_id, max_steps?, time_budget_ms?}`（预算覆盖须先协商） | `{outputs, choices, state, paused, ended}`；协商后附 outcome/executed_steps，运行失败或预算/取消暂停 → `ok:false` |
| `session.cancel` | `{session_id}`（须协商 runtime.bounded_continue.v1） | `{cancel_pending:true,state}`；取消下一次推进，顺序传输不抢占已开始请求 |
| `session.choose` | `{session_id, index}`(**0 起**) | `{state, paused, ended, choices}`;越界 → error `-32602` |
| `session.state` | `{session_id}` | `{state}` |
| `session.save` | `{session_id}` | `{save}`(存档 JSON **字符串**) |
| `session.restart` | `{session_id}` | `{state}` |
| `session.close` | `{session_id}` | `{closed: true}` |
| `project.open` | `{path}` | `{ok, project_id, language_version, baseline, catalog, diagnostics, workspace_diagnostics, read_only}`;故事可读但有故事诊断或工作区只读诊断时仍返回 `project_id`，后者不伪装成可写 |
| `project.analyze` | `{project_id}` | `{ok, language_version, baseline, catalog, maps, references, diagnostics, workspace_diagnostics, read_only, conflicts?}` |
| `workspace.check` | `{path}` 或 `{project_id}` | `{ok, schema_version, language_version, workspace_revision, stats, diagnostics, workspace_diagnostics, read_only, truncated, continuation, conflicts?}`；工程可读但有只读诊断时仍返回 `ok:true` |
| `maps.list` | `{path}` 或 `{project_id}` | `{ok, schema_version, language_version, workspace_revision, maps, references, diagnostics, workspace_diagnostics, read_only, truncated, continuation, conflicts?}` |
| `authoring.intent.preview` | `{path, intent}` 或 `{project_id, intent}` | `{ok, operation:"preview", target, reference_impact, changed_files, baseline, new_baseline, diagnostics, workspace_diagnostics, read_only}`；`intent` 是带显式 `expected_baseline` 的 core `AuthoringIntent` DTO |
| `authoring.intent.apply` | 同 `authoring.intent.preview` | 同上，`operation:"apply"`；成功后通过可恢复保存协议持久化全部变更文件，RPC `project_id` 缓冲同步更新 |
| `markdown.import.preview` | `{path, source, baseline, id_overrides?, namespace?}` 或以 `project_id` 替代 `path` | `{ok, operation:"preview", plan, baseline, workspace_diagnostics, read_only}`；`plan` 含稳定映射、冲突、损失、待写路径与 `plan_digest`，不写文件；只允许且必须提供 `path` 或 `project_id` 之一 |
| `markdown.import.apply` | 同 preview，并含 `plan_digest`, `accept_losses:boolean`, `allow_language_upgrade:boolean` | `{ok, operation:"apply", plan, changed_files, baseline, new_baseline, workspace_diagnostics, read_only}`；重新验证来源/目标基线，成功后经可恢复保存协议持久化；Project 会话仅在保存成功后更新 |
| `catalog.query` | `{path, query, offset?, page_size?, max_candidates?}` 或 `{project_id, query, ...}`；续页使用 `{path|project_id, query, cursor}`，cursor 与分页选项互斥 | `{ok, schema_version, language_version, workspace_revision, query:{summary, snapshot, offset, total, items, next, diagnostics}, diagnostics, workspace_diagnostics, read_only, conflicts?}`；`query` 为 core `CatalogQuery` DTO，`items` 每项含 `TargetRef`、source 与 reasons；参数类型错误用 `-32602`，语义查询错误在 result 中以 `ok:false` 和稳定 `error.code` 返回 |
| `reader.export.preview` | `{path, selection}` 或 `{project_id, selection}`；`selection` 是 core `ReaderExportSelection` DTO | `{ok, operation:"preview", plan, baseline, workspace_diagnostics, read_only}`；只读当前 Project 缓冲，`plan` 含作者专用排除报告与 `plan_digest` |
| `reader.export.apply` | `{path, selection, plan_digest, output}` 或 `{project_id, selection, plan_digest, output}` | `{ok, operation:"apply", plan, baseline, output, workspace_diagnostics, read_only}`；摘要过期或发布失败返回 `ok:false` 与 `STALE_PLAN` / `EXPORT_FAILED`，不覆盖已有目标；`reader.preview` 和 `reader.export` 是对应的短方法别名 |
| `relation.query` | `{story_id, target, offset?, depth?, direction?, relation_type?, scope_refs?, include_unscoped?, include_period_children?}` 或同字段的 `project_id` 请求 | `{ok, schema_version, language_version, workspace_revision, target, depth, nodes, edges, truncated, continuation, diagnostics, workspace_diagnostics, read_only, conflicts?}`；`scope_refs` 为 TargetRef 数组，同维度 OR、跨维度 AND；未标范围仅在 `include_unscoped=true` 时包含；时期子树仅在 `include_period_children=true` 时显式展开；`offset` 为非负整数续查偏移，continuation 保留全部筛选；未知目标/范围/类型或深度参数使用 error `-32602` |
| `relation.project` | `{story_id, target, role_mapping?, offset?, history_offset?, depth?, direction?, scope_refs?, include_unscoped?, include_period_children?, max_nodes?, max_edges?}` 或以 `project_id` 替代 `story_id` | `{ok, schema_version, language_version, workspace_revision, target, relations, history, truncated, diagnostics, workspace_diagnostics, read_only, conflicts?}`；`role_mapping` 是稳定 `relation_type` ID 到非空白角色标签的本次映射；空映射不产生语义边；人物 history 仅取 `with`、地点 history 仅取映射类型的显式 `event→place` 关系；history 的 period/rank/parallel 仅为现有偏序投影，不暗示同时或总序；关系 `offset` 与 `history_offset` 分页，过期快照须从 0 重查 |
| `relation.type.create` | `{project_id, relation_type, baseline?}` 或 `{path, relation_type, baseline?}` | `{ok, operation, relation_type, catalog, language_version, baseline, workspace_diagnostics, read_only}` |
| `relation.type.update` | `{project_id, relation_type, baseline?}` 或 `{path, relation_type, baseline?}` | 同 `relation.type.create`;关系类型 ID 保持稳定 |
| `relation.type.delete` | `{project_id, id, baseline?}` 或 `{path, id, baseline?}` | `{ok, operation, relation_type:null, catalog, language_version, baseline, workspace_diagnostics, read_only}` |
| `relation.create` | `{project_id, relation, baseline?}` 或 `{path, relation, baseline?}` | `{ok, operation, relation, catalog, language_version, baseline, workspace_diagnostics, read_only}` |
| `relation.update` | `{project_id, relation, baseline?}` 或 `{path, relation, baseline?}` | 同 `relation.create`;关系 ID 保持稳定 |
| `relation.delete` | `{project_id, id, baseline?}` 或 `{path, id, baseline?}` | `{ok, operation, relation:null, catalog, language_version, baseline, workspace_diagnostics, read_only}` |
| `relation.promote.preview` | `{project_id, legacy, relation, baseline?}` 或 `{path, legacy, relation, baseline?}` | `{ok, operation:"preview", preview, catalog, language_version, baseline, workspace_diagnostics, read_only}` |
| `relation.promote.commit` | `{project_id, preview, baseline?}` 或 `{path, preview, baseline?}`；也可用 `legacy` + `relation` 让服务端先生成预览 | `{ok, operation:"commit", preview, relation, catalog, language_version, baseline, workspace_diagnostics, read_only}` |
| `entity.create` | `{project_id, entity, baseline?}` 或 `{path, entity, baseline?}` | `{ok, operation, entity, catalog, language_version, baseline, workspace_diagnostics, read_only}` |
| `entity.update` | `{project_id, entity, baseline?}` 或 `{path, entity, baseline?}` | 同 `entity.create`;实体 ID 为稳定身份 |
| `entity.delete` | `{project_id, id, baseline?}` 或 `{path, id, baseline?}` | `{ok, operation, entity:null, catalog, language_version, baseline, workspace_diagnostics, read_only}` |
| `localization.export.preview` | `{path|project_id, selection}` | `{ok, operation:"preview", plan, baseline, workspace_diagnostics, read_only}`；`plan` 带 versioned exchange、source baseline、diagnostics、plan digest 与 `can_export` |
| `localization.export.apply` | `{path|project_id, selection, plan_digest, output}` | `{ok, operation:"apply", plan, baseline, output, workspace_diagnostics, read_only}`；重算 export 计划，拒绝过期摘要、已有文件或工程内输出路径 |
| `localization.import.preview` | `{path|project_id, selection, exchange}` | `{ok, operation:"preview", plan, baseline, workspace_diagnostics, read_only}`；有效 DTO 的内容错误留在 plan diagnostics / `can_apply:false` 且不写盘；结构无效的 package 返回 result `INVALID_PACKAGE`，raw JSON 重复键按 §3.1 拒绝 |
| `localization.import.apply` | `{path|project_id, selection, exchange, plan_digest}` | `{ok, operation:"apply", plan, changed_files, baseline, new_baseline, workspace_diagnostics, read_only}`；任何版本、选择、源文、token、Project 基线或摘要冲突均整批零写入 |

| `shutdown` | `{}` | `{bye: true}`(响应后进程退出,码 0) |

### 3.4 生命周期语义

v1.6 向后兼容扩展:`compile.path` 与所有 CLI 文件参数接受工程目录(解析 world.wl)。
`analyze.world` 和角色 properties / relations / events 字段见 relations.md §6。
协议版本仍为 1,消费者应忽略不认识的新增字段。`language_version` 是编译选项
的机器投影;实体定义出现在 `catalog.entities` 和作者资料编辑结果，定义本身不进入
运行时状态或 fingerprint；但 state 所属实体的稳定 ID 仍参与运行指纹。`project.open` 返回的 `baseline` 是
`Project::content_baseline()` 计算的内容基线,覆盖源码、工程清单和已注册展示文档的
相对路径、原始字节与删除状态,不使用 runtime fingerprint。后续编辑请求可回传它
保护陈旧请求。已有 `project_id` 的 `project.analyze` 与 `entity.*` 每次先刷新磁盘源码及
清单注册的展示文档，再返回刷新后的 `baseline`；`project.analyze` 遇刷新冲突仍返回可读
目录并附 `conflicts`，刷新错误返回 `ok:false` 的 `IO_ERROR`，实体编辑的刷新冲突返回
`ok:false` 的 `CONFLICT`。调用方应解决冲突后用返回的新基线重试。省略时仍由 Project
保存前的磁盘基线检查保护外部修改。编辑失败始终是 result 中的 `ok:false`;只有未知方法、
参数类型错误、未知工程 ID 等协议违规才使用 JSON-RPC error。
`project.open` 与 `project.analyze` 的 `diagnostics` 仍只属于故事编译域；清单版本、
未知必需能力及注册文档问题只进入 `workspace_diagnostics`。只要该数组非空就返回
`read_only: true`；工程仍可返回 `project_id`、`catalog` 和最新 `baseline`，但
`entity.*` 必须返回故事层 `READ_ONLY` 结果而不写盘。已有 `project_id` 的查询和编辑
请求都先刷新工作区，再计算这些分域诊断，因此外部新增未知能力或展示文档变化也会
立即反映在 `project.analyze`、`workspace.check`、`maps.list`、`catalog.query`、关系查询与后续编辑结果中。
`authoring.intent.preview/apply` 接受 `{path|project_id,intent}`，`intent.expected_baseline`
必须与刷新后的当前基线一致；过期基线、刷新冲突、编译失败或只读诊断均拒绝写入。
preview 只在 core 候选 Project 中验证并返回候选变更，不保存；apply 通过 Project 的可恢复
保存协议写入，若保存失败恢复原 Project 缓冲并返回 `ok:false`。候选任一步失败不会留下部分源码或地图更新。
`catalog.query` 只读当前 Project 缓冲；`cursor` 绑定 core 生成的查询指纹与内容基线，查询 DTO
或快照变化时返回 result `ok:false`、`error.code: "STALE_CURSOR"`，不得按旧 offset 猜测续页。
未知只读能力不会阻止查询，但故事编译错误、无效 DTO/选项和候选预算超限都返回
`ok:false` 的稳定错误码与中文 message；只有参数类型错误或未知 `project_id` 使用 JSON-RPC error。
core 的 cancellable 查询 API 可返回 `CANCELLED`；当前 CLI/RPC 方法没有请求中断机制，因此不承诺传输层取消。
`reader.export.preview/apply` 同样复用传入 Project 当前缓冲，不先刷新或保存作者文件；apply 只写一个新的站点目录，不更新 `project_id` 的内容、保存基线或运行状态。站点包仅按 selection 白名单生成，作者预览中的排除项和工作区路径不进入包。
关系查询的 `offset` 必须与同一 `target`、`depth`、方向和类型筛选及未变化的
`workspace_revision` 一起使用；基线变化后应从 `offset: 0` 重新查询。`relation.*` 和
`relation.promote.*` 与 `entity.*` 一样只接受语言 1.10 的可写工作区；关系写入
失败始终是 result 中的故事层 `ok:false`。旧关系提升的 commit 会重新验证预览句柄、
`content_baseline` 与完整 `draft`（包括 `scope_refs` 和 `properties`）；运行 fingerprint
只用于兼容预览差异检查，源码在预览后变化时返回 `EDIT_FAILED` 并不写入。
`analyze.timeline` 与 `wl timeline --json` 的新增 `timeline` 字段输出时段及先后约束,
结构见 relations.md §7;原 graph 保留执行关系,二者不能互换。

- `compile` 产出一个 **story 单元**(Program + Analysis 驻留服务进程),
  `story_id` 自 `"s1"` 起递增;产物驻留至进程退出(设计面向短生命周期
  agent 进程,不做回收)。
- `session.open` 基于 story_id 新建会话(多会话可共享同一 story);带
  `save` 时经 `Story::load` 恢复,指纹不匹配 → `ok:false`。
  `session_id` 自 `"c1"` 起递增。
- `session.continue` 对应库层有界推进;`session.choose` 对应
  `choose()`——**只消费选择、不推进**(与库语义一致),推进靠随后的
  `session.continue`。
- 会话结束(ended)后 `session.continue` 仍可安全调用:返回
  `ended: true`,`outputs` 仅含 `{"type":"ended"}` 收束事件;
  `session.restart` 复位到开头。

`session.open` 可选 `seed`，否则使用 runtime 默认种子。`session.trace`、`session.checkpoint`、`session.explain_choices` 和 `trace.replay` 都复用 runtime 公共接口；重放时不得按旧选择索引回退。`trace.replay` 的 `max_steps` 与 `time_budget_ms` 是非负整数；结构无效或版本/fingerprint 不兼容的 DTO 属于 `-32602`，可执行轨迹的分歧、预算耗尽和故事运行错误作为 `replay.status` 返回。调试 schema、选择身份、失败位置和预算语义见 [replay.md](replay.md)。

### 3.5 会话示例

```text
→ {"jsonrpc":"2.0","id":1,"method":"compile","params":{"path":"story.wl"}}
← {"jsonrpc":"2.0","id":1,"result":{"ok":true,"story_id":"s1","fingerprint":9812,…}}
→ {"jsonrpc":"2.0","id":2,"method":"session.open","params":{"story_id":"s1"}}
← {"jsonrpc":"2.0","id":2,"result":{"session_id":"c1","state":{…}}}
→ {"jsonrpc":"2.0","id":3,"method":"session.continue","params":{"session_id":"c1"}}
← {"jsonrpc":"2.0","id":3,"result":{"outputs":[{"type":"text","content":"…"}],
   "choices":[{"index":0,"label":"甲"}],"paused":true,"ended":false,"state":{…}}}
→ {"jsonrpc":"2.0","id":4,"method":"session.choose","params":{"session_id":"c1","index":0}}
← {"jsonrpc":"2.0","id":4,"result":{"state":{…},"paused":false,"ended":false}}
→ {"jsonrpc":"2.0","id":5,"method":"session.continue","params":{"session_id":"c1"}}
← …
→ {"jsonrpc":"2.0","id":9,"method":"shutdown"}
← {"jsonrpc":"2.0","id":9,"result":{"bye":true}}
```

## 4. 状态目录与演练历史（语言 v1.9）

分析结果 catalog.states 提供全工程状态声明与按 ID 聚合的变更索引；状态语义见 states.md。运行时状态与真实变更历史随存档持久化。

状态视图 `states` 为状态 ID 到标签 ID 数组的映射；`state_history` 为有序记录数组，每条含 `kind/state/before/after/event/node/note/turn`。`event` / `node` / `note` 可以为 null；before/after 是实际操作前后的完整集合。旧历史缺少 kind 时按 Become 读取。

编译目录 `catalog.states` 为 ID 到声明信息的映射，信息含 `id/display/target/tags/file/line/changes`。每条 `StateChangeSite` 形如：

```json
{"kind":"AddTags","event":"arrival","node":"arrival","timing":"enter",
 "tags":["alert"],"note":"读到来信","contexts":[],"file":"events/harbor.wl","line":5}
```

| 字段 | 类型与含义 |
|---|---|
| `kind` | `"Become"` / `"AddTags"` / `"RemoveTags"`，与 Rust ChangeKind 的序列化大小写一致；分别对应 with/add/remove |
| `event`, `node` | 字符串，所属事件与节点 ID |
| `timing` | `"enter"` / `"exit"` / `"done"` / `"during"` |
| `tags` | 字符串数组，本次操作参数；add/remove 时不是操作后的完整集合 |
| `note` | 字符串或 null |
| `contexts` | 字符串数组，源码中的条件、选择等说明；与 GraphEdge 的对象数组形状不同 |
| `file`, `line` | 源文件与从 1 开始的行号，用于定位，不是永久动作 ID |

索引顺序不是执行顺序；不要把不同分支的出处合并为唯一状态结果。实际历史的 `kind` 使用相同枚举值，同值操作也保留记录。

## 5. 独立锚点目录

`catalog.anchors` 是 ID 到 `AnchorInfo` 的映射，字段为 `id/display/description/file/line/links`。links 的每个元素为 `{anchor, target: {kind, id}, file, line}`，其中 kind 仅为 character/event/state/anchor。顶层 objects 与 references 同时暴露锚点对象和引用来源；可用 `wl catalog <工程> --kind anchor --json` 查询。

该映射不含复制的 states 或 changes 内容。Rust API `Catalog::anchors_for(&TargetRef)` 反查直接关联的锚点；`Catalog::anchor_changes(id)` 返回关联状态与关联事件的共同变化出处借用。JSON 消费者通过 links 找到状态 ID 与事件 ID，再从 `catalog.states[id].changes` 按 event 过滤，得到相同集合。上述 Rust 方法不是新增 JSON-RPC 方法。

`analyze.anchors` / `wl timeline --json` 的顶层 anchors 仍是正文手动语句位置，`state.anchors` 仍是实际演练记录；二者不改名、不自动转为独立目录对象。独立锚点资料及链接不改变 fingerprint。

## 6. 旧权限与协议兼容

协议版本仍为 1；消费者应容忍新增字段。`compile` 接受旧 grant/revoke/perm 输入，核心将其归一到世界叙事身份状态；analyze 返回归一后的状态声明、动作和准入表达式。所有旧权限都使用同一状态模型，生成 ID 不保证固定拼写。

`perms` 兼容数组与旧 Grant/Revoke 演练锚点从身份状态及其动作派生；没有可独立写入的权限集合。旧存档仅在记录的旧指纹/归一后指纹对匹配时转换；无映射权限、缺失状态、冲突来源或其他指纹变化返回故事层 `ok:false/run_error`，不变成 JSON-RPC 协议错误。存档详细规则见 [states.md](states.md)。


## 4. 工作区与编辑器控制边界

传入目录时，`compile.path` 与所有 CLI 分析命令读取根目录 `world.wl` 并递归载入所有 `.wl`。传入文件时维持入口及 include 的语言工具模式；只允许访问入口父目录范围。目录模式和编辑器分析结果一致。引用越界为 A109 故事诊断。目录读取失败仍按原 IO 契约处理。

`wl-agent` 的 `export` 只导出 Mermaid 文本，不是工程目录导出。现有协议没有连接运行中 worldedit 的通道，也没有修改缓冲、切换视图、撤销重做、窗口控制、图布局、系统文件对话框的方法。AI 可以修改磁盘工作区文件并用 CLI 校验；桌面自动刷新后显示修改。不能声称 CLI 已完整控制编辑器。

### 资料查询排序版本

`catalog.query` 与 `wl catalog-query` 共用 CatalogQuery：v1 为既有默认顺序，v2 必须包含 `sort:{field:"name"|"kind",direction:"ascending"|"descending"}`。排序语义、缺值与并列规则见 catalog.md §7.1。page/cursor 版本保持 1；排序改变后旧游标返回 STALE_CURSOR。命中添加同快照 display 字段，接口不得在分页后自行重排。未知 sort 字段或方向属于无效 DTO；版本与有效 sort 不匹配属于 INVALID_QUERY。


## 显式语言 1.11

CLI 的 `--language-version=1.11`、工程清单 `language_version:"1.11"` 和 RPC compile 的同名参数选择同一 core 编译契约。缺省仍为1.9；未知版本拒绝，不按源码猜测升级。新规则、片段、集合和强角色台词的语义见 [language-1.11.md](language-1.11.md)。check/compile 报同一定位诊断，catalog/analyze 投影规则与片段身份；不增加CLI私有解析器。

say 仍输出 `type:"text"`，content只含台词正文，附可选 `speaker:{kind:"character",id}`。普通正文省略speaker。CLI纯文本模式只显示content，JSON和RPC保留结构化speaker；作者演出备注不出现在运行输出或本地化交换中。session.save/load完整携带片段帧及类型值，未知required_features或损坏帧返回既有运行失败载荷，不跳过运行指纹。

读者选择DTO新增schema_version 2及`reader.fields.v1`明确能力，字段、speaker的公开白名单与旧schema1兼容见 [reader-export.md](reader-export.md)。CLI reader-export与RPC reader.export仍原样把同一core预览/摘要验证结果交给调用方。

### 完整源码草稿事务

语言1.11规则/片段等结构可通过通用完整源码接口创作，使用与编辑器相同的core WritingBuffer，不把新节点降成jump或丢弃未知正文。

`wl source-edit preview|apply <工程目录> --request-json '<DTO>' --json`，apply另需`--plan-digest`。
RPC `source.edit.preview`接受`{path|project_id,request}`；`source.edit.apply`另需`plan_digest`。
request为严格DTO：`{schema_version:1,path:"world.wl",expected_baseline:"当前内容基线",source:"完整目标源码"}`。path只能指向已加载工作区内的相对.wl路径，不新建目录、不接受越界或不明文件。以当前workspace check/project.analyze取得基线。

preview只生成`{schema_version,path,changed,plan_digest,diagnostics}`，不改缓冲或磁盘。apply重建候选并比对摘要及原工程/磁盘基线，然后整体应用、使用既有可恢复保存；失败不静默覆盖。该接口明确允许有编译诊断的源码草稿，诊断不等于文件丢失。正常正文/结构表单仍要求候选通过编译。调用方必须传完整源文而非正文片段，未提及的源内容会按明确请求替换。

DTO格式/参数错属于协议错误；只读、过期、路径、保存冲突等业务拒绝返回`ok:false,error.code:"SOURCE_EDIT_REJECTED"`。清单未知必需能力保持只读，不因完整源码入口而绕过。

读者preview v2的`content`按实际页面顺序返回`{title,output_path,text,empty_content}`，其text与生成search-index的正文一致；schema1不新增此字段。`fields`仅选择已有property键，引用目标不因字段选择而自动公开。站点本身manifest格式仍为1，不能拿作者预览DTO冒充发布内容。

### 1.11类型值与执行位置

运行Value继续使用既有外部标记枚举JSON；新增`{"Tag":"id"}`、`{"TagSet":["id",...]}`、`{"StateRef":"id"}`。TagSet按ID排序并去重，无普通字符串隐式身份转换。只在实际采用新增能力的作品中，state视图增加calls，逐层提供`fragment,statement,line,file,caller,call_statement,locals`。这些是作者/调试接口，不自动进入读者包。

规则真实求值EvidenceNode可选file/line，其他节点省略，不把未执行分支编造为已满足。状态变更索引的StateChangeSite可选source完整TargetRef及state_expression/tags_expression；片段定义的event为空、node为fragment:ID。Catalog.dynamic_state_changes列出参数化动作，空时省略；只有运行state_history是已发生记录，静态参数目标不会被猜成某个具体state。

## 显式语言 1.12：schema 与锁定选择

`--language-version=1.12`、清单同名版本与 RPC compile 选择同一 core。默认1.9和显式1.10/1.11不升级。持续约束见 [schemas.md](schemas.md)，锁定选择语法与降级规则见 [choices.md](choices.md)。

`wl schema-index DIR --json` / RPC `schema.index {path|project_id}` 返回 `ok/index/baseline/workspace_diagnostics`，index直接序列化core的SchemaIndex，含声明、绑定、实例和定位诊断，不另行推断约束。`wl schema-preview DIR --request-json '<SourceEditRequest>' --json` / RPC `schema.edit.preview {path|project_id,request}` 返回同一core SchemaEditPreview。`wl schema-apply` / RPC `schema.edit.apply` 另需`plan_digest`，重验并整笔应用后使用既有保存事务；source-edit的严格DTO/基线/路径/只读规则不变。预览提供field_changes、instance_impacts及完整性标记，不自动迁移、改名、强转或补值。业务拒绝用 `ok:false,error.code:"SCHEMA_EDIT_REJECTED"`；DTO错误仍是协议错误。允许保留无效草稿，不代表可以绕过发布检查。

运行旧 `choices` 数组始终只包含可选项，index零起。CLI `play --json --choice-presentation` 显式添加 `choice_presentation`，其 `index` 指向旧choices数组，禁用项为null；stdin仍读取旧可选索引。纯文本菜单用无编号锁定行呈现说明。

RPC initialize响应capabilities包含`runtime.choice_presentation.v1`；session.open以`capabilities:["runtime.choice_presentation.v1"]`申请，响应返回已协商能力数组。只有协商成功的session.continue/session.choose附带choice_presentation；缺省旧消费者得到仅可选choices。session.choose的`index`、`presentation_index`、`choice_id`必须三选一；后两者要求已协商能力。禁用或失效身份是`ok:false/run_error`且零推进；参数格式或多选择器属于JSON-RPC error。capabilities未知项不被接受，不扩大其它权限。

投影字段为`id,label,links?,line,offset,enabled,index,disabled_reason`；不含条件表达式、变量或调试证据。全锁组落穿，不因禁用项创建暂停。作者静态禁用说明未纳入本地化交换白名单，交换成功不等于该说明已翻译。

## 普通有界演练扩展

`runtime.bounded_continue.v1` 通过 initialize/session.open 显式协商。普通继续始终
有默认保护，旧消费者正常字段不变，超限是 `ok:false` 故事结果。新增预算、
取消方法、CLI 标志与输出完整契约见 [bounded-execution.md](bounded-execution.md)。

## 稳定 ID 重构计划兼容投影

Rust `Project::plan_rename_target -> Result<RenamePlan, String>` 与既有序列化字段保持兼容，不新增 JSON-RPC 方法。`RenamePlan` 额外暴露 `runtime_fingerprint_before` / `runtime_fingerprint_after`（u64）；每个 `RefactorChange` 额外暴露 `occurrences`，逐处含 `line`、可选 `field`、`before_range` / `after_range`（`{start,end}`，文档 UTF-8 字节半开范围）、`before_token` / `after_token`、`before_context` / `after_context`。语境是计划真实前后字节的完整行，JSON 的 field 是 JSON Pointer；未知新增字段按既有协议规则忽略。原始 before/after 文件字节仍不序列化；机器消费者不能编辑逐处清单授权部分重构。完整基线、范围、语境与候选在应用前重新校验，篡改或过期整批拒绝。

entity/relation 改名仅在运行指纹不变时成功。尤其 state 所属 entity 的稳定 ID 改名安全拒绝，返回中文错误，包含受影响 state ID/文件/行、旧/候选 fingerprint、旧 Story save/检查点的指纹不匹配原因、入口 replay 需按当前稿重新受控验证的边界，以及仅修改实体显示名的路径。实体定义本身排除于运行指纹不意味着对它的所有运行引用也排除。

旧计数字段仍表示实际身份引用数，不要求与 occurrences 项数相等。旧类型的字符串语法重编码可将整条实际变更行投影为 `field:"source.syntax"`，明确不是单个身份 token；entity/relation 计划始终每个身份 token 一项。

## 安全源码生命周期（工具 0.16）

`project.source_lifecycle_preview` / `project.source_lifecycle_apply` 与 CLI
`wl source-lifecycle preview|apply` 使用相同 core SourceLifecycleRequest 和
SourceLifecyclePlan；apply 必须回传预览摘要，成功后保存。请求、逐处预览、活动源码
成员语义、明确拒绝和零修改边界见 [source-lifecycle.md](source-lifecycle.md)。

CLI 完整形式为 `wl source-lifecycle preview|apply <目录或入口> --request-json
'<SourceLifecycleRequest>' [apply: --plan-digest 摘要] [--json]`。RPC params 必须是对象，
仅含 `path` 或 `project_id` 之一、`request`；apply 另外必须含非空字符串
`plan_digest`，preview 不接受该字段。未知字段、重复 JSON key、无效 operation 或
字段类型均拒绝，不猜测请求意图。CLI 重复或不适用参数为用法失败（退出码 2）；
RPC 参数形状错误为 `-32602`，重复 JSON key 按既有消息解析规则为 `-32700`。

两接口成功结果为 `{ok:true,operation,plan,baseline,applied,saved}`，`plan` 原样
序列化 core DTO，`baseline` 是操作后当前 Project 内容基线。preview 不改缓冲或
磁盘，`applied:false,saved:false`；非空 apply 成功为 `applied:true,saved:true`。
`move_entity` 使用同一 DTO；同源空计划 apply 不保存，返回 `applied:false,saved:false`。
业务失败为 `{ok:false,operation,plan,baseline,applied:false,saved:false,
error:{code:"SOURCE_LIFECYCLE_REJECTED",message,stage}}`，其中 `stage` 为 preview 或
apply，`plan` 为 null。打开失败沿用 `IO_ERROR`、`stage:"open"`，baseline 为 null，
CLI 退出码 2；其他业务失败退出码 1。

内存应用后若保存失败，返回 `ok:false`、`stage:"save"`、`applied:true,saved:false`，
保留实际 `plan`、应用后基线和当前 Project 缓冲，不恢复旧快照或删除 journal。
`saved:false` 仅表示未确认保存完成，不表示磁盘零修改；磁盘可能已部分写入，须通过
既有重新打开/刷新恢复事务。RPC 的 project_id 会话继续保留应用后的内容。只有
`saved:true` 才能声明保存成功；本接口不承诺跨文件物理原子性。

## 世界对象焦点与时间解释（工具 0.17）

initialize.capabilities 新增 `authoring.world_context.v1` 与 `authoring.temporal_explanations.v1`。
协议仍为 1；消费者先检查能力后显式调用新方法即选择该能力，不改变旧方法或旧会话字段。
世界上下文完整 DTO、精度与预算见 [world-context.md](world-context.md)。

CLI `wl world-context DIR --target KIND:ID [--options-json '<WorldContextOptions>'] --json`
以及 `wl world-object DIR --target KIND:ID --json` 使用同快照 core API；前者输出 context，
后者输出 object。RPC `world.context` / `world.object` 接受 path 或 project_id（二选一），
target 为 `{kind,id}`；world.context 可传 options。新方法拒绝未知字段、未知 options 字段、
重复 key 及非法字段类型；RPC形状错误 -32602，重复key沿用 -32700，CLI用法错误退出2。
业务错误 UNKNOWN_TARGET/STALE_SNAPSHOT/CANCELLED 为 `{ok:false,error:{code,message}}`；
坏稿返回 ok:false 和包含 invalid_source 的 context，CLI退出1。截断本身不表示编译失败，
ok:true 仍须检查 complete/truncated/reasons；不会以空表表示预算失败。

两接口均带 schema_version、language_version、workspace_revision、diagnostics、
workspace_diagnostics、read_only；project_id 请求先 refresh 并报告 conflicts。只读工作区
允许查询。content_baseline 与 workspace_revision 相同，source snapshot 属于实际编译产物。
world-object 还返回 snapshot，精确 lookup 不受无关目录总数量影响。

CLI `wl timeline compare DIR --left ID --right ID [--expected-baseline BASELINE] --json`
与 RPC `temporal.compare`（path 或 project_id、left/right、可选 expected_baseline）返回
comparison 和上述工作区公共字段。比较结果完全来自 core Timeline::compare；before/after
的 evidence 只含同快照真实 follows 边，完整语义见 [temporal-explanations.md](temporal-explanations.md)。
expected_baseline 不匹配返回业务错误 STALE_BASELINE，不展示旧证据为当前事实。未知事件为
comparison.relation=unknown，业务 ok:false；invalid 同样 ok:false。协议形状错误仍与业务分开。

时间比较在 refresh 报告未解决外部冲突或恢复事务冲突时返回 CONFLICT、comparison:null，
即使保留的本地缓冲基线未变化也不能将旧比较解释为当前磁盘一致证据。Project 只读包装不
隐式刷新，基线绑定当前缓冲；发现已存在的交叉修改冲突会拒绝比较。

`world.context` 在 refresh 已知未决外部/恢复冲突时保留同一缓冲 context，附原 conflicts，
并设置 context.complete=false、reasons 包含 source_conflict；冲突本身不置 truncated，
不改变 read_only，也不清空或覆盖任何一侧。源码快照摘要不能证明当前磁盘最新版。

## 工程问题报告（工具 0.18）

新增 `authoring.problems.v1`、只读 `project.problems` 与 `wl problems`。完整 DTO、分页/游标、coverage、位置精度、预算和退出码见 [problems.md](problems.md)。旧 `wl check` 的检查范围与成功语义不变；报告列出 sidecar error 不改变任何运行、发布或只读门禁。

工具 0.19 的 `initialize.capabilities` 新增 `authoring.problem_source_context.v1`。
`project.problems` 与 `wl problems` 保持 schema1 与旧请求形状；响应位置增加可选
context（version=1），旧 report 仅只读，导航必须刷新。DTO、窗口与精度、字节/字符
坐标和兼容边界见 [problem-source-context.md](problem-source-context.md)。该工具能力
不进入作品 required_features 或 Story save。

`project.problems` 的响应限制包含 JSON-RPC 外壳、原 id 的实际 JSON 编码与行末 LF。
§3.1 的 id 回显仅有此方法的巨大标识例外：若连紧凑错误外壳都不能在 1MiB 内回显，
方法不执行，返回 -32600 / id:null / data:"request_id_exceeds_response_budget"；
不截断 id。其余可回显 id、协议错误 code、通知行为与其他方法均不变，详情超预算
采用有界摘要。精确策略见 [problem-source-context.md](problem-source-context.md)。

## 0.20 真实路线对照机器接口

`initialize.capabilities` 新增 `authoring.route_comparison.v1`；比较 DTO 使用
`schema_version:1`。完整语义与 runtime 唯一实现见 [route-comparison.md](route-comparison.md)。

- CLI：`wl route-compare PROJECT --left-trace-json '<ReplayTrace>' --right-trace-json '<ReplayTrace>' [--max-steps N] [--time-budget-ms N] [--json]`
- RPC：`project.compare_routes {project_id, left_trace, right_trace, max_steps?, time_budget_ms?}`

CLI 只接受一个工程目录或入口；RPC 的非空 `project_id` 必须是当前进程已经打开的
工程，不接受 `story_id`、`session_id` 或 `path`。缺失、重复 CLI 参数、未知参数、
错误类型、负数以及超过硬限额的预算均为调用错误；RPC 对新方法严格拒绝未知字段。
Trace 的既有未知可选字段仍沿原兼容规则读取。CLI 在解析每个 trace JSON 前检查其
UTF-8 原始字节；RPC 在反序列化 trace 前检查所提供 JSON 值的序列化字节（含未知
字段）。每侧上限为 4194304 bytes；runtime 再验证已解析 DTO 的字节与 4096 步上限。

`max_steps` 默认及最大值为 100000，`time_budget_ms` 默认及最大值为 30000；它们是
两侧合计预算。0 为零额度，不等于无限制。其它证据、运行输出和结果额度使用
runtime 默认硬上限。CLI/RPC 都是同步有界调用；后续 RPC 不能中断正在处理的比较，
不会把 `session.cancel` 宣称为路线比较取消接口。

CLI 使用 core `Project::open_read_only`，两端使用 `Project::compile_read_only`，
在同一当前已应用编译快照上调用一次共享 `compare_routes`。RPC 不刷新、保存、应用
或恢复工程；CLI 不通过普通 `open/refresh` 自动恢复事务或迁移权限。存在未解决
事务时拒绝比较并保留文件原字节。接口不重算状态差异、覆盖、对齐或动作来源。

正常结果为 `{ok, comparison}`；`comparison` 是未改写的 runtime DTO，
`ok` 仅在两侧 `status:"replayed"` 时为 true。部分区段验证通过仍保留各自
`complete:false` 与 `ended:false`，不代表完整结局。单侧分歧、故事运行失败、
取消或资源停止保留两侧独立实际结果，不能替换为原 trace 观察。
CLI 两侧 replayed 时退出 0，其它结构化侧结果退出 1。

参数/结构/兼容性错误（`invalid_options`、`input_limit`、`invalid_trace`）为 CLI
退出 2 / RPC `-32602`。有效请求的编译失败、不可用当前快照、结果不能装入输出额度
等返回 `{ok:false,comparison:null,error:{code,message},...}`，CLI 退出 1；编译错误
附 `diagnostics`，工作区诊断独立放在 `workspace_diagnostics`。CLI 工程读取失败
为 `IO_ERROR`、退出 2。错误 message 中文；不把业务失败转换成 JSON-RPC error。

runtime 比较 DTO 最多 1048576 bytes；CLI 完整响应及 RPC 完整响应行（包括外壳、
原 id 的实际 JSON 编码和末尾 LF）最多 1052672 bytes，即 1 MiB + 4096 bytes。
若正常结果或错误详情超出该额度，返回有界 `output_limit` 业务错误，不截断成空差异。
RPC 在执行方法前预检 id：连最小业务或协议错误都无法在额度内完整回显时，返回
`-32600`、`id:null`、`data:"request_id_exceeds_response_budget"`，不执行方法、不截断
id。可回显 id 的超长协议错误保留原协议 code，以有界消息替代细节。通知遵循既有
无响应规则。此例外只扩展到本方法，不改变其它方法的旧响应限额。

比较不增加 source-resolve RPC：只返回 core 在同一快照中验证的来源，不能以 trace
里的旧文件/行或同名状态猜测位置。0.20 继续拒绝 0.19 trace/checkpoint；普通 Story
Save 保留原有兼容规则，不新增同样的 runtime_version 拒绝条件。

## 0.21 世界资料批量导入

`initialize.capabilities` 新增 `catalog_import_v1`。协议版本仍为1；默认语言1.9、最高1.13不变。唯一版本化请求、CSV预算、映射/空值、字段预览、真实指纹及事务边界见 [catalog-import.md](catalog-import.md)。不创建额外作品能力或导入sidecar。

- `catalog.import.preview {project_id,request}`：只读生成完整core计划
- `catalog.import.apply {project_id,request,plan_digest}`：重新验证并一次应用当前Project内存；`saved:false`
- `project.save {project_id,expected_baseline}`：显式保存当前缓冲，成功才` saved:true`；失败保留缓冲、既有保存基线与恢复事务

三方法均拒绝未知顶层参数；导入request及嵌套映射拒绝未知字段、未知enum；既有JSON入口拒绝重复键。非空project_id须已通过project.open建立，不支持path临时工程，不暗中刷新、恢复或保存。preview正常返回`{ok:plan.can_apply,operation:"preview",plan,saved:false}`，apply成功返回`{ok:true,operation:"apply",plan,changed_files,new_baseline,saved:false}`。CSV/映射/类型/schema数据错误留在plan.diagnostics并令ok:false，同时保留error:{code:"CATALOG_IMPORT_BLOCKED",message}；读取/过期/冲突/能力等失败返回`{ok:false,error:{code:"CATALOG_IMPORT_REJECTED",message},saved:false}`。参数类型/结构/未知ID才走JSON-RPC error。保存陈旧返回STALE_BASELINE，保存故障SAVE_FAILED。

`wl catalog-import preview|apply PROJECT --request-json '<DTO>' [--csv UTF8_FILE] [--plan-digest DIGEST] [--save] [--json]`使用同一core。CLI先按Project::open_read_only加载并拒绝待恢复事务，完整工作区限4096文件/64MiB；--csv先有界读取为快照，core不接受外部CSV路径；不指定时使用request.csv。apply需要摘要，preview禁止摘要/--save。apply默认只改短命内存，响应明确saved:false和退出丢弃提示；显式--save后调用既有保存事务。命令成功退出0，业务阻断及CSV输入失败1，参数格式错误2，--json失败仍返回结构化ok:false。CSV输入失败返回stage:"input"/CSV_INPUT_REJECTED；保存失败返回applied:true、stage:"save"、plan、saved:false、SAVE_FAILED和可能存在待恢复磁盘事务提示，不假称零磁盘写入。源CSV改变导致重建摘要不匹配，不能复用旧授权。

工具0.27的`route-compare`与`project.compare_routes`加法返回每侧`variable_writes`，
仍是同一schema1只读结果，不改变ReplayTrace/ReplayCheckpoint协议。旧结果缺字段
表示未提供证据，不表示没有写入。语义与预算见
[变量写入证据](https://github.com/ikzerok/worldline/blob/main/spec/variable-write-evidence.md)。

工具0.27的schema预览/应用结果加法提供`incomplete_reasons`，并在源码加载缺失、
越界或解析/身份不完整时准确标记complete=false；已知实例违规不冒充来源缺失。
CLI/RPC共享同一core投影，错误草稿保存规则不变，详见
[schema影响完整性](https://github.com/ikzerok/worldline/blob/main/spec/schemas.md)。
`incomplete_reasons`固定顺序去重，值为`source_loading`、`syntax`、
`ambiguous_declaration`、`schema_definition`；完整时序列化为空数组。

## 0.28 可执行依赖查询

initialize.capabilities 新增 `authoring.executable_context.v1`；协议仍为1，作品
required_features 不变。既有 `wl world-context` 的 `--options-json` 与 RPC
`world.context.options` 接受 `include_executable:true`，或显式选择
`rule_call` / `fragment_call` / `global_read` / `global_write` kind；这构成选择新能力。
默认 false 保留旧结果。响应加法提供 executable_context 能力字符串说明此服务支持，
并原样返回 core 的 typed provenance、同快照来源与 complete/reasons；严格参数检查、
业务错误、退出码、冲突和来源失效规则不变。细节见
[executable-context.md](executable-context.md)。

## 0.28 已验证试玩报告接口

能力 `authoring.playthrough_report.v1`；`wl playthrough-report <目录或入口> --trace-json DTO
[--max-steps N] [--time-budget-ms N] [--json]` 与 RPC `project.playthrough_report
{project_id,trace,max_steps?,time_budget_ms?}` 共用 runtime 的单一路径验证及Markdown生产者。
输入、信任边界、来源、时间、完整性和预算见 [playthrough-report.md](playthrough-report.md)。

CLI默认写出同一Markdown，JSON与RPC结果为 `{ok,report}`；调用错误/编译错误为
`{ok:false,report:null,error:{code,message}}`，编译错误附当前诊断。`ok` 仅在
`report.status=replayed` 为真；即使此时 `complete=false`，也只表示已验证部分区段。
完整结果还必须 `complete && ended`。其余真实停止状态保留报告但 `ok:false`，不得把状态
伪造成参数错误。CLI验证通过退出0、故事/报告失败1、参数/输入/IO失败2。RPC协议参数错误
用 `-32602`，合法请求的故事失败仍是正常结果。只接受上述参数，重复CLI参数拒绝。

完整JSON响应（含id、外壳及换行）小于1MiB+4096字节；输入trace含JSON转义的计量先于克隆。
巨大id不能保证有界响应时返回null id的 `-32600`，不反射超额id。诊断及错误文本先流式
计量再投影；超额返回小型 `output_limit`。通知不输出响应。CLI只读打开当前磁盘稿，RPC
只读编译已打开工程的已应用缓冲，不刷新磁盘、不恢复事务、不修改live session。


资料查询 `catalog.query` 的显式 query v3 支持既有对象引用属性的精确条件 `equals:{"type":"reference","value":{"kind":"entity","id":"harbor"}}`，并沿用原分页协议；v1/v2不接受此值。合法缺失目标返回零命中，非法类型/身份返回INVALID_QUERY。默认不升级，详细兼容与保存文档能力见 [catalog.md](catalog.md) §7.3。

## 0.30 原子新章与正式来源

新增 `authoring.manuscript_chapter.v1` 能力与 `manuscript.chapter.preview/apply`、
`wl manuscript-chapter preview|apply`。版本化请求、修订、完整候选摘要、业务失败、
内存应用/显式保存和严格 JSON 边界见 [manuscript-authoring.md](manuscript-authoring.md)。
协议仍 1，默认语言 1.9、最高 1.13 不变，旧书稿纯编排接口不生成源码。

## 0.30 统一只读对象候选页

能力 `authoring.object_search.v1`，`world.objects.search` 接受已打开的 `project_id`、
`query` 字符串（可空）、可选 `filter`、`options`、`expected_baseline`；不接受 path。
CLI `wl object-search PROJECT --query TEXT [--filter-json DTO] [--options-json DTO]
[--expected-baseline BASELINE] [--json]` 只读打开磁盘作品，不恢复事务。
filter/options 直接使用 [object-search.md](object-search.md) 的 core DTO，不复制匹配算法。

两端只读编译当前已应用快照，不刷新/应用/保存，返回 `schema_version:1`、`baseline`、
`language_version`、`snapshot:"applied"`、`diagnostics`、`workspace_diagnostics`、
`read_only` 和原样 `page`。页只描述所给目录，坏稿为 ok:false / INVALID_SOURCE，仍
保留实际页并明确不完整。只读工作区允许查询，不能把快照称作实时磁盘数据。

未知参数/嵌套字段、错误类型、重复 key 拒绝；RPC 类型错误 -32602，重复 key -32700，
CLI 用法错误退出2。参数合计上限64KiB。陈旧基线及 core 数值/页/预算拒绝为业务
ok:false、page:null，code 为 STALE_BASELINE、INVALID_LIMIT、INVALID_CANDIDATE_BUDGET、
CANDIDATE_BUDGET_EXCEEDED、INVALID_OFFSET；不假造空页或总数。CLI 成功0，业务失败1，
读取失败2。未知kind/entity_type合法零命中，分页顺序与全部身份由core唯一提供。

## 工具 0.31：模板设计、书稿检索与真实状态检查

新增协商能力 `authoring.template_designer.v1`、`authoring.manuscript_query.v1`、
`runtime.state_inspection.v1`。protocol 仍为1，语言默认1.9、最高显式1.13；新增入口
不改变旧方法返回形状，也不把界面操作提升为另一套语言解释器。

### 模板草稿及预览/应用

- `template.draft`：参数 `project_id`、`request`。request含 `schema_version:1`、
  可选 `expected_baseline` 与 `action`。action是 `open {source}`、`inspect {draft}`
  或 `edit {draft,edit}`。source是 `new`、`copy {id}`、`existing {id}`、`json {bytes}`。
  draft保留 `source_bytes`、可空 `existing_id`，以及 `reserved_field_ids` / `reserved_keys`
  两组草稿期保留身份；后两者缺省为空以接受旧DTO，删除字段后不得复用身份使未知扩展复活。
  这些保留信息不写进作品模板schema。结构edit由core定义，见templates.md。
  返回baseline和projection（原始字节、typed模板、diagnostics、read_only、editable）；
  调用成功只表示投影完成，不表示文档可应用。不开启、应用或保存任何作者文件
- `template.preview` / `template.apply`：参数 `project_id`、`request`；apply另需
  `plan_digest`。request含 `schema_version:1`、完整 `expected_revision`、
  `expected_baseline`、`intent`。intent是 `upsert {draft}`、明确的
  `repair_invalid {draft}` 或 `delete {id}`。upsert保留Existing身份；修改JSON ID
  不能替换原模板，按新身份导入必须显式移除draft的existing_id绑定
- preview复用core完整模板事务，返回import/replace/repair_invalid/delete、真实ID、
  字段变化、实例影响、诊断、改动文件、complete、incomplete_reason、can_apply及摘要。
  current_template / proposed_template明确两侧标题和适用范围，实例逐侧标注applicable；
  字段变化携带两侧父组及索引。源码不完整时保留已知影响但complete=false、can_apply=false。
  apply用同一request重建
  完整计划并校验摘要，再通过既有修订/内容/磁盘基线守卫整批提交内存；不自动保存
- 摘要绑定请求原字节、身份、完整基线/修订和全部可见计划；它用于检出过期或改动，
  不是密码学签名或授权令牌。repair_invalid必须明确选择并满足templates.md的受限
  修复条件，普通replace不能绕过原来的只读保护

业务request DTO最多4MiB，模板机器预览/投影最多8MiB。机器影响报告还限制真正适用的
实例最多10,000个、实例字段值最多100,000个；core在实例/字段值物化前检查数量，
在克隆值之前流式计量输出，不先分配巨大JSON再判断额度。最终计划计入16字符摘要后
再次核8MiB。超限返回PLAN_LIMIT，不截断已声明完整的实例影响；旧UI/高级JSON/core
无预算入口继续保留全量能力。机器DTO拒绝未知参数字段，raw模板JSON的未知可选扩展仍保留。
已成功解析JSON后的参数/DTO错误用-32602；全局原始JSON解析与重复key仍遵循既有
-32700。过期、core拒绝、只读或结果预算失败是 `ok:false` 的故事层结果。

CLI对应 `wl template draft|preview|apply <目录或入口> --request-json JSON`，apply须
附 `--plan-digest`，仅显式 `--save` 才在应用后保存；不带--save为本进程内存修改，
进程结束即丢失。draft/preview不接受--save。JSON模式单行返回明确applied/saved，
保存失败不伪装回滚成功，沿既有可恢复事务处理。所有路径以工作区为边界。

### 书稿查询

`manuscript.query`参数为 `project_id`、`query`、可选`drafts`，使用
[manuscript-query.md](manuscript-query.md)的同一DTO与不可变快照查询。query必须明确
schema_version=1和manuscript_id；drafts每项绑定expected_baseline。进程内WritingBuffer
可以由core调用方显式叠加，机器入口不自动获取编辑器未提交输入，也不引入另一份正文。
CLI为 `wl manuscript-query <目录或入口> --query-json JSON [--drafts-json JSON] --json`。

业务参数载荷最多4MiB，查询结果最多4MiB；超限结构化失败。查询错误与不完整范围
返回ok:false并保留可提供的诚实页面/诊断；无匹配不等于没有问题。已生成不可变快照
上的筛选/分页零IO；快照生成仍复用core只读路径、注册同一性与附件metadata检查，
禁止缺失源码磁盘回退和保存预检。不会创建、应用、保存或刷新作品。

### 真实运行状态

`session.inspect`参数仅为 `session_id` 与可选 `query`，业务参数最多64KiB。
复用 [state-inspection.md](state-inspection.md) 的有类型query/page；默认每页50、
最大100，不为查看隐式创建或推进Story。返回 `ok:true,inspection:page`；过期stamp
或结果预算错误返回ok:false，参数结构/额度错误为-32602。原session.state保持原样。

新inspection stamp的 `run_id`、`compiled_snapshot`、`fingerprint`、`trace_generation`、
`revision` 在JSON中均为规范十进制字符串，内部仍为u64。仅接受`"0"`或非零ASCII数字
开头的无符号整数串且不超过u64范围；数字类型、前导零/符号、空白、小数、指数、
缺失或未知字段拒绝。客户端原样回传同会话的stamp，普通JavaScript JSON往返不会
舍入64位身份。本轮新能力尚未发布，没有数值stamp旧schema兼容承诺；既有trace、
checkpoint、Story Save、state_view及旧方法的fingerprint编码不变。

CLI普通或JSON试玩在等待选择输入时接收 `inspect` 或 `inspect {query JSON}`，只输出
状态检查结果并继续等待同一选择，不执行continue/choose。首次/上一基线仅来自真实
已记录观测，缺失和省略不能补0/false/空值，声明定位不能冒称最后写入的原因。

## 工具 0.32：同一稿的协调、试演、审稿与巡检

协议仍为1；新增以下能力和入口，不改变旧方法形状，不自动升级作品语言或schema。

| 能力 | RPC | CLI | 真源契约 |
|---|---|---|---|
| `authoring.workspace_reconciliation.v1` | `reconciliation.capture/preview/apply` | `wl reconciliation capture/preview/apply/save` | [普通外改](workspace-reconciliation.md) |
| `authoring.draft_rehearsal.v1` | `project.draft_rehearsal` | `wl draft-rehearsal` | [隔离草稿试演](draft-rehearsal.md) |
| `authoring.manuscript_delivery.v1` | `manuscript.delivery` | `wl manuscript-delivery` | [同范围作者审稿本](manuscript-delivery.md) |
| `catalog.scope.v1` | `catalog.scope` | `wl catalog-scope` | [查询范围巡检](catalog-scope.md) |

外改 RPC 只操作已打开 Project，capture/preview 不采纳，apply 只采纳内存；下一次明确
`project.save` 仍独立校验外改。CLI需要明确真实旧基线和本地字节材料；apply的短命内存
不能当作编辑器已改，独立save重建同一计划并核摘要。`null`缺失与空字节数组不同。
候选完整序列化预算在采纳前检查，不能在写入之后才发现响应过大并伪称零修改。

草稿试演使用明确的正文覆盖、完整当前基线、文件代次和真实选择ID；不会从另一进程
提取编辑器输入。CLI支持有界请求文件或内联JSON二选一。结果中的实际输出、状态和
条件来自独立真实运行，不产生正式轨迹、存档或检查点，亦不修改普通运行会话。

书稿交付输入包含同一书稿查询及可选稳定章ID；保留全部匹配出现和重复正文、错误及
不完整边界。RPC只返回报告和同一core Markdown；CLI仅显式`--output`导出工作区外的
全新`.md`。这份作者材料不经过读者白名单，不应作为公开阅读包。

查询巡检的RPC接受`path`与`project_id`二选一；只读路径不恢复事务。完整typed范围、
当前页和可选正式关系都来自一次不可变快照；外部消费者须核来源身份，不得与新稿拼接。

四个入口拒绝未知字段、重复键和非法形状。参数错误走JSON-RPC error，合法请求的
过期、预算、保护和编译失败走`ok:false`。各能力的输入、结果、完整外壳与换行预算
不同，以链接契约为准；新适配器的编码预算不声称改变既有stdio首次读行/JSON解析器
的全局资源边界。CLI还受宿主命令行长度限制，大正文试演宜使用`--request`文件。
