# worldline 共同契约：对象、展示文档与修改边界

版本0.2，设计草案（0.2修订：M1矢量画布——地图自有坐标系、栅格图层化、基础线面前移；阈值单源登记）。除明确标注为后续票据的部分外，本文的 M2 实体契约已由 worldline-core 实现；其余新增文件、类型、CLI和语法仍不能直接当成当前0.2.0的已支持能力。需求来源见[需求清单](../docs/design/shared/REQUIREMENTS.md)，事实依据编号见[研究记录](../docs/design/shared/RESEARCH.md)。

## 1. 不变量

1. 世界内容由作者编写；地图是内容的空间化展示，不是世界运行环境。
2. 内容、共享展示配置、个人浏览状态分别拥有唯一真源，不复制人物或地点正文。
3. 浏览命令不得写工作区；版式命令不得改内容、关系、世界事件、故事变量或存档。
4. 图形线、语义关系、正文链接、关键词命中必须有不同来源类型。
5. 删除标记不删对象，隐藏节点不删关系，地图导航不建立地理归属。
6. 无可执行事件的内容工程合法；仅启动故事演练时检查可运行入口。
7. 不添加sim模块、行动调度、世界时钟、资源结算、自主演化及其可选开关。

## 2. 身份与引用

### 2.1 内容身份

保留现有 `TargetRef { kind: String, id: String }`，不以显示名称、数组下标、源行号或地图位置作为身份。[R05]

M1支持现有kind；M2增加`entity`，实体的place/organization/culture等是`entity_type`，不是每种类型新增一个TargetRef.kind。因此改变实体分类不改变身份。既有character/tag不强制转entity；通过统一查询投影共存。

旧ID继续有效。新建对象优先由编辑器生成不依赖名称的ID，作者可另设显示名。显式重命名ID必须走引用迁移事务；不能将用户重命名显示文字当成ID修改。相同id在不同kind下是不同对象。新增地图/标记/关系的ID独立分配，文件移动不改变ID。

地图及图层是展示资源，不进入世界实体集合。导航目标采用明确区分的`NavigationTarget`：Object(TargetRef)、Map(MapId)、Placement(MapId,PlacementId)、Relation(RelationId)、Source(Location)。不得用一个字符串既指地图又指人物。

### 2.2 图与引用类型

`SemanticRelation`只记录明确的设定联系；`MentionReference`表示正文显式链接或关键词位置；`MapPlacement`是展示入口；`FlowEdge`仍表示旧剧情控制流。引用影响检查可以汇总它们，但返回`reference_class`，UI不能把它们渲染为语义等价的箭头。

## 3. 持久化布局

```text
作品/
  world.wl
  characters.wl
  lore/                         # 作者内容；默认仍递归索引全部wl
  stories/
  assets/maps/harbor.png         # 栅格图层素材只在素材库保留一份
  .world/
    project.json                # 格式能力与注册表；新建地图时显式创建
    maps/map_harbor.json         # 一张地图一份展示文档
    graph_views/view_lighthouse.json
    templates/place.json        # 栏目/提示，不是某个地点的资料
    comments/comment_x.json     # M4批注与提案，可一讨论一文件
    proposals/proposal_x.json
```

世界内容和明确语义关系继续存`.wl`。地图/网络版式JSON只保存显示配置和TargetRef。模板只保存栏目定义，不保存实体实例值。协作文件保存讨论及提案，不替代已接受的`.wl`正文。

个人镜头、临时筛选、打开标签页、解码缓存留在应用用户目录/浏览器本地缓存，不进入作品目录。只有“保存为共享视图”才把初始镜头或布局写入指定展示文档。

不需要一个集中式数据库或云服务才能读取工程。M1保留现有素材路径边界和完整工程保存，不增加其他平台内容格式。[R17]

### 3.1 清单提案

```json
{
  "schema_version": 1,
  "project_id": "project_harbor",
  "language_version": "1.9",
  "entry": "world.wl",
  "required_features": ["presentation.maps.v1"],
  "maps": {"map_harbor": ".world/maps/map_harbor.json"},
  "graph_views": {},
  "extensions": {}
}
```

所有清单路径相对工作区根，不相对`.world`目录。`.wl`内include和asset仍遵守原规范的声明文件相对路径规则，两者不能混用。

无清单按旧工程读取，不主动写空文件。第一次创建地图才创建清单和地图JSON。清单未注册的普通JSON不参与分析；不扫描整个目录并把用户自己的JSON误认成系统元数据。

浏览器已有`worldedit-project.json`入口记录。[R09] 新清单不复用该文件名；两者同时存在且entry不同，报入口冲突而非静默选一个。未指定新entry时可读取旧入口；M1生产的新工程统一使用根`world.wl`。

必需能力按功能登记：M1使用`presentation.maps.v1`与`presentation.geometry.line_area.v1`（基础点线面随M1矢量画布一起交付）；M2语义实体/关系在语言1.10下声明`content.entities.v1`、`content.relations.v1`，共享网络视图声明`presentation.graph_views.v1`；M4展示预设与专题视图另立能力字符串。加入新几何类型时必须同步提高required_features，不能只在原地图里塞入旧客户端无法处理的polygon。Schema描述最终格式形状，客户端支持的能力集合另外检查。

版本策略：本设计schema_version=1为格式大版本；未知可选字段保留原值，unknown required_feature或更高大版本只读并提示。`extensions`为受控扩展区。未知字段不能默认在serde往返时被丢弃。此保护只对实现了本契约的新客户端成立，不能保证旧0.2.0理解未来格式；升级工程需明确提示旧工具不可安全编辑。

## 4. MapDocument

详细机器约束见`schemas/map.schema.json`；样例见`examples/map_harbor.json`。这些文件检查设计契约，并不意味着当前编辑器能够打开它们。

M1 的 `worldline-core` 从 Project 已注册的地图文档生成 `MapDocument` 与
`MapIndex`：地图 JSON 的格式、ID、坐标、图层、TargetRef、素材声明和导航
引用都在 core 校验。`TargetRef` 在 1.9 只接受现有 kinds；`entity` 等 1.10
能力由 Project 的显式语言版本启用。无效 JSON、重复键、越界/非有限坐标和
自交多边形产生 `MAP` 域诊断，原始字节继续由 Project 保留；缺失素材、未解析
对象和未注册导航目标作为可见的局部诊断，不阻止其他地图和源码读取。core 只
提供素材声明、相对路径和 `available` 状态，不解码图片。

`MapIndex` 是只读派生查询，不是地图文件的写入格式。每个 `MapDocument.source`
保留完整的原始 JSON 值，包含所有层级的未知可选字段与能力声明；查询中的几何、
引用和素材 DTO 只提供已理解的字段。后续结构编辑在 Project 原始文档上修改指定
字段，不能把派生 DTO 整体覆盖回文件。原始字节（含格式）仍由 Project 管理。

| 字段 | 类型与含义 |
|---|---|
| schema_version | 1 |
| id/title | 稳定地图ID、显示标题 |
| background | 兼容别名，见raster_layers；新文档一律使用raster_layers |
| raster_layers | 栅格图层数组；每项含稳定ID、image asset的TargetRef与地图空间放置矩形，按序绘制于标记图层之下；可为空数组 |
| canvas | 地图自身逻辑坐标系的宽高（正整数）、unit=normalized；坐标基准是地图extent，不绑定任何栅格图层 |
| layer_order | 图层ID数组，显式顺序 |
| layers | 以稳定ID为key的图层字典；title、visible_default、locked、style |
| placements | 以稳定ID为key的标记字典；layer_id、target_ref、geometry、annotation、用途等 |
| extensions | 可选扩展，保留 |

标记字段：`target_ref`可空；`geometry`必须存在；`annotation`是此处的展示说明，不是实体正文副本；`label_override`可空，空则从目标当前显示名读取；`role`说明“地点入口/出生地/故居/某章所在地/说明”；`navigation`可指子地图；`scope_refs`仅标记作者范围。

同一内容可以在不同地图甚至同一地图出现多次，placement_id不同。对同一对象的名称修改自动反映到未覆盖名称的标记；作者设定的label_override保持原值，UI说明“自定义标签”，不把它误称同步失败。

图层是展示集合。地图可以多入口、可以返回已看过的地图，因此导航不是强制树。循环导航不执行递归加载；只在点击时进入，访问历史长度有上限。不能因为城市图链接寺院图就生成“寺院隶属城市”的关系。

### 4.1 坐标

坐标系属于地图文档自身：原点在canvas extent左上，x向右，y向下。持久坐标为归一化二维值`[u,v]`，均相对地图extent取[0,1]。标记、折线、多边形与未来矢量图元共用这一坐标系；更换、裁剪或删除栅格图层不改变任何已保存坐标。未知位置不用[0,0]占位，而是不建立标记。

栅格图层（PNG/JPEG等位图素材）是放置在该坐标系中的展示图层：每层声明asset引用与放置矩形`[u0,v0,u1,v1]`，默认整幅铺满extent。首次导入位图时extent默认取该图像素尺寸、放置矩形为[0,0,1,1]，作者随后可显式调整extent或各层放置。矢量图元（点标记、折线、多边形）是格式的原生成员；外部SVG等矢量文件导入不是本期目标，未来作为新图层类型经required_features演进。

栅格放置矩形必须满足 `u0 < u1` 且 `v0 < v1`，拒绝零宽、零高和倒置矩形。

显示变换：`screen = viewport_origin + pan + zoom * (u*width, v*height)`。zoom与pan是个人镜头，不写内容。点击和拖拽通过逆变换定位。屏幕坐标使用显示逻辑点（与渲染框架无关的UI单位），由编辑器负责与设备像素的正确换算，不得重复乘系统DPI。图标大小与点击热区按屏幕逻辑点计算，避免缩小后无法点选。

M1提供point、polyline与简单polygon的创建与编辑；M4增加展示预设与专题模板。折线至少两点，多边形至少三点，最后一点不重复首点；不支持自交和洞时明确拒绝，不静默修形。非有限数值、维度错误、范围越界和层引用不存在是错误。JSON数字不能利用NaN/Infinity绕过限制。

### 4.2 语义无副作用

地图数据不能包含StoryState、库存、自动目标、时钟或执行指令。图形线即使链接某条关系，也只引用RelationId，不负责创建该关系。几何区域只表示作者画出的展示范围，不自动确定区域内对象的文化、归属或管理者。

## 5. SemanticRelation与通用实体

M2新增内容声明，保留旧人物关系兼容读取。`entity`、`relation_type` 与
`relation_def` 在语言 1.10 下启用；1.9 默认入口仍不识别这些关键字。关系类型、
关系实例和受限局部查询由 `worldline-core` 生成，地图只可引用已存在的关系 ID，
不得因图形线或导航自动创建关系。

```text
entity lighthouse kind place as "雾港灯塔"
  description "由作者编写的地点资料。"
  property appearance = "白石塔身，临海。"

entity keepers kind organization as "守灯会"
  description "守护灯塔的团体。"

relation_type maintains as "维护"
  inverse "由其维护"
  direction directed

relation_def rel_keepers_lighthouse type maintains from entity keepers to entity lighthouse
  description "守灯会负责灯塔的日常维护。"
  source_note "共同设定记录第3项"
```

实体和其他作者资料继续复用字符串、数值、布尔属性和多行 description，不引入通用可执行属性系统。语言 1.10 增加显式 `ref("kind", "id")` 属性值以支持模板对象引用；当前 kind 为 `entity` 或 `relation`，作为稳定 `TargetRef` 参与目录、重命名及删除保护，不求值且不进入运行指纹。普通字符串不因模板字段或内容同名而变成强引用；详见 [syntax.md](syntax.md) 与 [templates.md](templates.md)。

relation_def至少具有稳定ID、关系类型、from/to和可选说明。关系类型定义方向、显示名、反向显示名及可选端点约束。不要求所有创作关系具有数值强度。关系实例可附作者明确的范围、来源、相关事件和任意非执行资料。`from` 与 `to` 端点使用完整 `TargetRef`，关系 ID 本身可作为 `TargetRef(kind="relation")` 被展示文档引用。

新关键字必须在显式language_version=1.10下启用；旧compile_source/compile_sources默认仍按1.9兼容语义，新入口提供CompileOptions。新声明不进入event执行体，不变成选择或效果。词法/解析/目录/导航/标记/删除重命名/语法高亮/CLI样例必须一起更新。

复杂盟约等多方关系先建立独立entity，再用关系标注各参与者角色，不把所有多方语义硬塞成一条多端点边。`Catalog::query_relations` 默认深度1、最多深度2，最多250节点/500边；结果带稳定顺序、关系 ID、端点、显示方向与 `truncated`/continuation，不做反向或传递推断。

### 5.0.1 M2 实体最小实现

本节的 `entity`、`relation_type` 与 `relation_def` 声明在语言 1.10 定稿。核心公开 `LanguageVersion::{V1_9,V1_10}` 与
`CompileOptions`，旧 `compile_source`/`compile_sources` 默认使用 1.9。Project
按 `.world/project.json` 的 `language_version` 选择编译选项，无清单或版本为
1.9 的工程不会隐式升级。实体查询统一从 `Analysis.catalog` 派生：
`catalog.entities` 是 ID 到 `EntityInfo` 的映射，同时在 `catalog.objects` 暴露
`TargetRef(kind="entity", id)`。

`EntityInfo` 只含稳定 ID、可变 `entity_type`、显示名、description、字面量
properties 与源位置。创建、修改、改名（显示名或分类）和删除都通过 Project
的源码编辑事务完成；删除沿用 `DeletionImpact` 的内容/地图引用检查。实体声明
不加入事件执行 Program 的事件体，也不改变运行指纹。1.10 的正文链接、Wiki
搜索、导航和 CLI catalog 读取同一 catalog，不在各层复制解析器。

### 5.1 旧人物关系

旧CharacterRelation没有独立ID。[R16] 为显示可生成临时LegacyRelationHandle，含来源对象、目标、标签及区分重复项的信息；它不是稳定可持久化身份，不能被批注或地图永久引用。核心公开 `Catalog::legacy_relation_handles` 与关系提升预览/提交接口。

需要持久引用时，显式“提升为独立关系”：预览新relation_def与旧项移除，整批提交，保留作者说明。预览携带 `Project::content_baseline()` 和完整草稿，包含 scope 与 properties；提交按内容基线拒绝陈旧请求并保持零写入。不能以源行号伪装永久ID。旧人物关系原本参与指纹规则；提升可能影响旧档，必须提示和测试，不能未经证明宣称等价迁移。

### 5.1.1 可选内容模板（M3）

内置模板目录的机器单源为 `spec/examples/templates.catalog.json`，schema 为
`spec/schemas/templates.schema.json`。core 公开只读模板目录与按
`kind/entity_type` 匹配接口；首版共 16 类（world 1、character 1、entity 14）。

模板只包含可选字段、创作问题和关系建议，不保存对象实例值。切换或删除模板不会
清除 description、已有 property、别名或未知自定义字段；空栏目不自动写成
false/0。关系建议只打开显式 relation 草稿，仍须作者选择类型与两端后提交。
来源、陈述性质与创作状态作为彼此独立的作者资料字段展示，不因“已接受”推断真假。

### 5.2 范围不是模拟状态

M4的范围引用可以指作者定义的period、作品条目或版本条目。按维度内部OR、跨维度AND筛选；是否包含未标范围项由`include_unscoped`明确控制。默认不展开时期子树，作者可显式选择“含子时期”。

范围只选择已有记录，不推出某条关系何时开始、自动终止或生成中间历史。不同作者版本使用明确ID和范围；本期不实现继承覆盖求值或自动合并事实。

## 6. GraphViewDocument

只保存专题标题、中心对象、筛选、已展开节点、固定位置、隐藏显示项和可选初始镜头。中心对象使用完整 `TargetRef`，也可指向 `relation` 对象；边来自内容关系查询，不在视图文件重存一份语义。

隐藏节点/边只修改视图；删除关系必须显式执行DeleteRelation并显示影响。自动布局是无语义副作用的计算，其结果先在个人工作状态；作者确认“保存布局”后才进入共享文件。

局部查询默认depth=1，可申请depth=2；首版最多显示250节点、500边（数值以第11节阈值登记为准），超过时返回`truncated=true`与继续展开入口。原始关系数据不能因为显示上限被删掉。查询顺序确定、双向读取不生成反向副本。

### 6.1 M2 网络布局读写契约

`graph_views::build_graph_view_index` 从清单 `graph_views` 的安全路径构建唯一展示索引。
GraphViewDocument v1 的必填字段为 `schema_version/id/title/focus/filters/positions/hidden_relation_ids`。
`positions` 键为 `kind:id`（只分割首个冒号），值为两个有限坐标；这是网络逻辑坐标，
不表示地理位置。`filters` 含 depth（1–2）、relation_types（空表示全部）、direction
（both/outgoing/incoming，默认 both）、max_nodes（1–250）与 max_edges（1–500）。
同维关系类型取 OR，方向与类型取 AND；分页必须沿用同一快照及筛选，替换当前页而非无限累积。

读取不改工程。损坏 JSON、重复键、结构或数值错误报 `GRAPH001`；明确引用缺失报
`GRAPH002`；源码有错误导致引用暂时无法解析时报告 `GRAPH004`，不能视为已删除。
未知必需能力或版本报 `GRAPH003` 并只读保留原字节。展示诊断不混入故事
编译结果。标题、focus、positions 和 hidden_relation_ids 均是展示资料，不产生关系。
正文提及、关键词匹配与旧人物关系兼容投影不自动写成网络中的独立关系。

`GraphViewCommand` 捕获进程 Revision 与完整内容基线；`apply_with_content` 校验内容
快照、基线、文档格式与引用后，在 Project 副本中注册/写入，再整批提交。首次保存默认
路径为 `.world/graph-views/<id>.json`，不得接管既有普通文件；清单显式登记
`presentation.graph_views.v1`。修改只更新已知字段，保留原文档、focus 和 filters 中
未知可选字段。命令只提高展示代次，源码与运行指纹不变，Project 快照可一次撤销。
坏内容导致引用无法确认时拒绝新增引用；已存在且未改变的引用允许纯布局修复。
保存时仍遵守 Project 的磁盘冲突检查，内容基线不是磁盘锁或密码学凭证。

删除影响增加 `graph_views` 列表，含 view_id、路径与引用字段；中心、固定位置和隐藏
关系均作为明确引用。损坏或不支持的注册视图使全工程删除检查不完整；删除命令不能
将其当作无引用。临时隐藏边不解除这些共享引用，删除关系须先明确修改引用它的视图。
`DeleteGraphViewCommand` 只删除共享布局文档并解除注册，需同样的修订与内容基线；
一次撤销可恢复注册与原字节。未知必需能力、路径共享或只读文档拒绝删除，绝不级联删除实体或关系。
同一物理文件不得兼作多个地图/网络注册项；索引、保存与删除均拒绝歧义路径。
关系类型被共享视图的 relation_types 筛选引用时，必须先显式修改这些筛选才能删除类型。

### 6.2 M4 展示预设契约

展示预设只保存作者明确选择的地图、网络视图、图层显隐、范围筛选，以及对既有折线/
多边形的“路径说明”或“分布区”引用。格式见
`spec/schemas/presentation_preset.schema.json`；工作区清单以 `presets` 注册并声明
`presentation.presets.v1`。切换预设只改变展示与筛选，不执行 event、不改运行指纹，
也不会因为多边形覆盖某对象就推断领土、文化或组织归属。

`scope_refs` 只引用作者已有对象；`include_unscoped` 与
`include_period_children` 必须显式保存。路径用途只接受 polyline，分布区用途只接受
polygon；用途不匹配时拒绝保存，不能把几何形状转换成语义关系。预设的地图或网络视图
不存在、图层不存在、范围对象缺失时报告 `PRESET001` 并保留原始字节。

`PresetCommand` 与网络布局一样携带 Revision 和内容基线，在 Project 副本中完成
清单注册和文档写入后一次提交；首次保存默认使用 `.world/presets/<id>.json`。
预设写入只提高展示代次，不改 .wl、地图几何或关系定义。未知可选字段保留；未知必需
能力由工作区能力协商按只读处理。

## 7. 命令、修订和错误

建议接口形状，不是当前Rust API：

```text
WorkspaceSnapshot { revision, content_index, relation_index, maps, diagnostics }
Revision { workspace_generation, content_generation, presentation_generation }
CommandEnvelope { expected_revision, expected_documents, command }
Command = CreateMap | UpdatePlacement | DeletePlacement | SetLayer
        | CreateEntity | UpdateEntity | CreateRelation | UpdateRelation
        | RenameObjectId | PlanDeleteObject | ApplyDeletePlan
        | SaveGraphView | AddComment | ApplyProposal
CommandResult { new_revision, changed_files, affected_refs, undo_record, diagnostics }
```

`expected_revision`是当前进程的乐观并发令牌，不是Git提交；`expected_documents`包含文档内容hash，用于保存、外部修改和提案检查。Git提交只能作为已保存协作基线的附加信息。

撤销也是新的用户意图：调用方从当前历史栈选取逆操作记录，并在发起时捕获 `expected_revision`，不得在延迟执行时补填最新修订。逆操作记录描述要恢复的字节，不自动授予当前写入权限；执行须同时检查意图修订和记录的修改后字节，防止后续编辑改回相同字节时旧意图误生效。连续撤销逐次捕获新基线，每次仍推进展示修订。

内容/版式命令共用事务边界，但使用不同验证集。展示命令不必通过全工程可运行检查；已损坏声明导致目标身份无法确认时，可以移动现有图形位置，但不能据此新建不确定引用。原始编辑模式允许保存未完成文本，结构化命令不写出新结构错误。

错误至少区分：InvalidSchema、InvalidGeometry、MissingReference、UnresolvedReference、StaleRevision、ExternalConflict、ReadOnlyFeature、AssetLimit、StorageFailure。Missing是确认不存在，Unresolved是来源尚未解析成功；二者不能混为删除事实。

新WorkspaceDiagnostic使用`location={source_span|json_pointer|workspace}`及domain，旧Diagnostic编号和序列化保留。MAP/REL/WS前缀是本设计草案编号，不抢占既有A/P编号；正式实现前登记规范。

## 8. 保存、撤销与外部协作

Project保持一套文档会话，可暂时保留原documents(.wl)，新增authoring_documents(JSON)和统一遍历接口，降低迁移成本。新增文件、删除墓碑、保存基线、mark_saved、restore、dirty、refresh、portable清单都必须覆盖两类文档。二进制素材共享句柄或按hash缓存，不能被每次Project克隆复制。

一次拖动：pointer_down记录对象与旧值 → pointer_move仅更新UI预览 → pointer_up校验版本并提交一个命令 → 一次Undo恢复旧位置。取消和只点击不形成历史。

桌面保存沿用全部目标预检与逐文件临时替换，但跨文件引用更新需要恢复日志。建议`.world/.transactions/<id>/`存prepared/applying/committed状态、目标路径、前后hash和待写内容；该目录是明确注册的暂存区域，不属于作者内容。正常完成后清理；发现未完成事务时先恢复或保留冲突副本，禁止静默继续。

不能声称文件系统具备全工程原子替换。恢复时仅对当前hash等于事务前/后值的文件进行自动处理；外部第三方改动需人工决定。备份/打包遇未解决事务应先提示，临时日志不混入普通项目包；除这一新定义暂存区外，原有普通文件包括未引用素材、README和.agent仍保留。

浏览器没有持续磁盘同步。[R09,R17] 打包当前缓冲为完整快照，维护`last_export_revision`、`local_snapshot_revision`和脏状态；发起下载不证明用户文件已经写入磁盘。本地快照失败保留脏标记。现有包体限制不能因新增地图悄悄移除。

M4文件协作先实现按稳定ID的三方diff/merge。不同标记不同字段可在验证后合并；同字段、删除与修改、layer_order并行重排必须显示冲突。没有服务端身份认证时，作者署名与已接受状态是团队记录，不是不可伪造的权限控制。

### 8.1 M4 批注与提案契约

批注由清单 `comments` 注册，必需能力为 `collaboration.comments.v1`。`CommentDocument v1` 含稳定 `id`、作者、正文、解决状态与一个锚点；锚点只能是完整 `TargetRef` 对象/关系、地图 `map_id + placement_id`，或带相对路径、行范围、原文与片段 hash 的正文范围。对象改 ID 时跨视图重构同步更新对象锚点；正文或标记不再精确匹配时只标记 `detached`，不得猜测相邻段落或同名对象。批注本身不进入运行指纹；对象删除影响计划必须列出直接对象批注引用。

提案由清单 `proposals` 注册，必需能力为 `collaboration.proposals.v1`。`ProposalDocument v1` 记录稳定 ID、作者、理由、状态及逐文件 `{path, domain, base, proposed}`；`domain` 明确区分 `content` 与 `presentation`。提案保存不采纳修改，采纳前再次读取当前缓冲做三方预览：当前等于 base 时可应用 proposed；JSON 对象的不同稳定键可递归合并；同字段并发修改、删除/修改、数组并发修改及正文并发改写均产生冲突并整批零写入。成功采纳后才把提案标为 `accepted`，且内容/版式实际修改与状态更新在同一候选 Project 中提交。

审阅预览为只读 DTO，包含当前 Project 内容基线、逐文件三方原文与差异、冲突、引用影响及截断标记。展示 JSON 用 JSON Pointer 定位字段，正文以空行分隔的段落及 UTF-8 字节范围定位；无法无歧义对齐的正文段落须设置 `alignment_uncertain` 并回退到三方原文，不伪造段落对应或字节范围。JSON 字段未能无歧义映射到词法位置时，源范围保守指向整份原文，供审阅者回看，不伪造精确行号。无法结构化的展示文档回退为三方原文。内容文件的引用影响基于 core 对当前 Project 与应用整份提案后的候选各编译一次，列出受改动文件内对象在两套目录中的入站引用；编译失败或超过引用上限时标记 `reference_impact_complete = false`，不得把部分结果显示成“没有影响”。审阅差异、原文、受影响对象及每对象入站引用的上限见§11；超限显式标记截断/不完整，预览截断不改变合并或采纳规则。机器可序列化此 DTO。采纳命令必须同时携带预览的内容基线与修订，并重新校验实际当前内容；过期预览不能作为覆盖授权。

`changed` 表示候选文件字节会变化，`semantic_changed` 表示结构化值变化；只调整 JSON 空白或键顺序不能列作作者语义差异。

批注/提案命令携带进程内 `Revision` 和内容基线，陈旧表单或过期提案必须重新审阅。未知必需能力只读保留，未知可选字段往返保留。作者署名和 `accepted` 仅是协作记录，不构成身份认证或权限边界。

## 9. 删除与指纹规则

删除标记仅删placement；删图层要求显式选择移动或删除其标记；删地图只删其展示配置，并提示指向它的导航。删除asset若仍作底图必须先修复引用。删除对象返回影响计划，允许取消、重新绑定或明确解除引用，不默认删除其他正文。

M1新增展示/模板/批注文档不进入Story.program、不改旧指纹。新增entity/relation声明是作者资料，也不参与运行指纹；但旧character/world字段原有指纹行为保留，不因新架构悄悄放宽。[R13] 内容改稿对存档的影响按旧规则提示。

### 9.1 跨视图稳定 ID 重命名（M2）

首版 `Project::plan_rename_target` / `apply_rename_plan` 对 `entity` 与 `relation`
稳定身份提供两阶段重构。预览从当前内容、地图和网络视图索引确认引用完整性，
列出实际会改变的源码与展示文档；显示名修改仍走普通资料编辑，不触发 ID 重构。

提交必须匹配预览时的 `content_baseline` 与每个目标文件原始字节，任何 ID 冲突、
损坏/只读展示文档、陈旧基线或重写后诊断都会使整批零写入失败。源码中的声明、
结构引用与显式正文链接，以及地图/网络 JSON 的 TargetRef、位置键和隐藏关系 ID
在同一候选 Project 中更新并统一验证。entity/relation ID 重命名不得改变运行指纹。

## 10. CLI与仓库间契约

保留`wl check/graph/timeline/catalog/play`含义。新增提案命令：`wl workspace check DIR --json`、`wl maps list DIR --json`、`wl relations DIR --target KIND:ID --depth 1 --json`。新命令先在M2实现；当前0.2.0不可直接调用。

旧`wl graph`继续是剧情图。新增命令返回版本、工作区修订、数据、诊断和截断状态。Rust API是主共享入口；不为地图建立HTTP服务、浏览器远控或新的AI上下文系统。现有agent继续服务原分析/演练，本PRD不依赖它写编辑器缓冲。[R18]

跨仓库采用兼容成对提交：worldline合并契约与实现后，worldedit在CI checkout固定SHA验证；两边发布记录列出匹配版本。暂不改同级path依赖，避免与产品功能混合迁移。[R03,R12,E07]

## 11. 阈值登记（单源）

以下数字只在本节维护；其它文档与schema引用本节，不在别处另立数值。修改任何阈值须与本节同一次修订更新，并同步TEST_PLAN的负载与判定。所有数值是验收建议初始值，按G1/G4/G6实测修订；修订只改本表。

| 项 | 值 | 适用范围 | 首次引入 |
|---|---|---|---|
| 局部关系展示上限 | 250节点/500边 | 图视图查询 | M2 |
| 关系查询深度 | 默认1，最大2 | 关系查询 | M2 |
| 导航访问历史 | 最近64步 | 地图导航 | M1 |
| 栅格图层尺寸·桌面 | ≤4096×4096且≤16,777,216像素 | 解码预检 | M1 |
| 栅格图层尺寸·WASM | 默认≤2048×2048；更大图须显式确认后降级或拒绝 | 解码预检 | M1 |
| 纹理缓存 | 桌面128MiB / WASM 64MiB；CPU解码缓存独立预算 | 渲染 | M1 |
| 浏览器整包 | 维持现有64MiB包体限制 | 工程导出 | M1 |
| 交互绘制帧率 | P95≤33ms；M1记录不阻断，M2起为发布门 | 渲染 | M1记录/M2门 |
| 本地对象查询 | P95≤150ms | 已加载工程 | M2 |
| 暖态资料切换 | P95≤200ms | 编辑器 | M2 |
| 提案审阅差异 | 256项/文件 | 审阅预览 | CAP-04A |
| 提案审阅文本 | 16 KiB/每份展示文本 | 审阅预览 | CAP-04A |
| 提案审阅受影响对象 | 256个/内容文件 | 引用影响 DTO | CAP-04A |
| 提案审阅入站引用 | 256条/对象/当前或候选 | 引用影响 DTO | CAP-04A |
| M1验收负载 | 1,000对象、1张2048×2048栅格图层、500标记、3,000米级图元 | 测试夹具 | M1 |
