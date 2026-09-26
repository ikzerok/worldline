# worldline 机器接口契约(agent 协议与 CLI JSON 模式)

**协议版本:** 1（语言 v1.10 entity 字段扩展）

CAP-01A 的[就地建档组合意图](authoring-intents.md)由 core Project API、CLI 与 agent RPC
共同提供；既有单对象写入命令仍不代表组合事务能力。

时段包含扩展：`timeline.periods[]` 新增 `parent: string | null`，保存直接上级 ID。父子层级由 core 验证，未知上级及循环包含为 A219 编译诊断。CLI 与 RPC 同时返回该字段，不影响会话状态和运行指纹。

资料导航扩展：CLI `catalog --json` 与 agent `analyze.catalog` 的目录新增 `aliases`（target/name/file/line）和 `text_links`（source/target/label/file/line/column）数组；正文引用同时出现在 references。属于向后兼容的附加字段，旧消费者可忽略。未知别名／正文链接目标为 A218 编译诊断；格式错误为 P004；故事层仍返回 `ok:false`，不变成 JSON-RPC 协议错误。播放输出只包含链接的显示文字，不增加运行记录或另一份状态。
**生产者:** `wl`(JSON 模式)、`wl-agent`(`worldline-agent` crate)
**消费者:** 外部 agent 程序、CI、测试
**依据:** 本仓库语言规范与 core/runtime 的公开模型。

---

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
`--save=<存档.json>`、`--language-version=1.10`、`--json`。未指定
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

## 3. `wl-agent` 协议(JSON-RPC 2.0 · stdio 行分帧)

`wl-agent` 是有状态的机器协议入口:外部 agent 程序 spawn 子进程,经
stdio 收发**行分帧 JSON-RPC 2.0**,驱动 编译 → 检查 → 试玩 → 选择 →
存读档 → 状态查询 全流程。

### 3.1 分帧与处理模型

- 每行一个 JSON 消息;空行忽略;以 EOF 或 `shutdown` 结束,退出码 0。
- **单线程顺序处理**:上一请求响应完成后才处理下一请求;无并发交错。
- `id` 必须回显;通知(无 id)不响应。
- stdout 上只写协议消息;日志一律走 stderr(当前实现不主动输出日志)。

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
| `initialize` | `{}` | `{protocol: 1, server: "wl-agent", version: "0.2.0"}` |
| `compile` | `{path, language_version?}` 或 `{source, file_name?, language_version?}` | `{ok, story_id, fingerprint, stats, diagnostics, workspace_diagnostics, read_only, language_version}`;故事编译失败时无 story_id；工程可编译但工作区只读时仍可返回 story_id，`read_only` 为 true |
| `analyze` | `{story_id}` | `{graph, anchors, symbols, stats, world, timeline, catalog, language_version}`(结构化,同 §2.2/§2.3 形状;symbols 为符号表全量) |
| `export` | `{story_id, format}`;format ∈ `graph_mermaid` \| `timeline_mermaid` | `{text}` |
| `session.open` | `{story_id, save?, seed?}`(save 为存档 JSON 字符串；seed 为新会话的非负整数随机种子，不能与 save 同用) | `{session_id, state}` |
| `session.trace` | `{session_id}` | `{trace}`；输出与 CLI 相同的 runtime ReplayTrace |
| `session.checkpoint` | `{session_id}` | `{checkpoint}`；仅用于相同 runtime/schema/fingerprint |
| `session.explain_choices` | `{session_id}` | `{choices}`；只读解释当前选择组的条件与阻断原因 |
| `trace.replay` | `{story_id, trace, max_steps?, time_budget_ms?}` | `{ok, replay}`；选择/观察不匹配和预算停止为结构化故事结果，不是 JSON-RPC 错误 |
| `session.continue` | `{session_id}` | `{outputs, choices, state, paused, ended}`;运行期错误 → `ok:false` |
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
| `catalog.query` | `{path, query, offset?, page_size?, max_candidates?}` 或 `{project_id, query, ...}`；续页使用 `{path|project_id, query, cursor}`，cursor 与分页选项互斥 | `{ok, schema_version, language_version, workspace_revision, query:{summary, snapshot, offset, total, items, next, diagnostics}, diagnostics, workspace_diagnostics, read_only, conflicts?}`；`query` 为 core `CatalogQuery` DTO，`items` 每项含 `TargetRef`、source 与 reasons；参数类型错误用 `-32602`，语义查询错误在 result 中以 `ok:false` 和稳定 `error.code` 返回 |
| `relation.query` | `{story_id, target, offset?, depth?, direction?, relation_type?, scope_refs?, include_unscoped?, include_period_children?}` 或同字段的 `project_id` 请求 | `{ok, schema_version, language_version, workspace_revision, target, depth, nodes, edges, truncated, continuation, diagnostics, workspace_diagnostics, read_only, conflicts?}`；`scope_refs` 为 TargetRef 数组，同维度 OR、跨维度 AND；未标范围仅在 `include_unscoped=true` 时包含；时期子树仅在 `include_period_children=true` 时显式展开；`offset` 为非负整数续查偏移，continuation 保留全部筛选；未知目标/范围/类型或深度参数使用 error `-32602` |
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
| `shutdown` | `{}` | `{bye: true}`(响应后进程退出,码 0) |

### 3.4 生命周期语义

v1.6 向后兼容扩展:`compile.path` 与所有 CLI 文件参数接受工程目录(解析 world.wl)。
`analyze.world` 和角色 properties / relations / events 字段见 relations.md §6。
协议版本仍为 1,消费者应忽略不认识的新增字段。`language_version` 是编译选项
的机器投影;实体对象只出现在 `catalog.entities` 和作者资料编辑结果,不会进入
运行时状态或 fingerprint。`project.open` 返回的 `baseline` 是
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
- `session.continue` 对应库层 `continue_story()`;`session.choose` 对应
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
