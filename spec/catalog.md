# 标签与创作素材

**版本：**1.9。此目录是作者的世界资料索引,独立于播放流程。

## 1. 标签定义与标记

```wl
tag coordinate as "坐标"
  description "世界中的已定义位置"
tag harbor_place as "雾港码头"
  property x = 120
  property y = 36

mark tag harbor_place with coordinate
mark event arrival with harbor_place
mark event farewell with harbor_place
mark character lin with harbor_place
```

tag 的语义类似指向对象的引用集合:存储稳定对象 ID,不复制内容,可通过其他 tag 间接引用。
`tag <ID> [as "名称"]` 是全局唯一声明,可带 description 与字面量 property。
`mark <类型> <对象ID或带引号路径> with <标签ID列表>` 是顶层标记,可放在任何被 include 的文件。
类型包括 anchor、state、event、scene、character、entity、relation、world、storyline、period、variable、tag、asset、file。

## 1.1 语言 1.10 实体

显式启用语言 1.10 后，`entity ID kind TYPE as "显示名"` 声明通用作者资料。
目录中的 `EntityInfo` 字段为 `id/entity_type/display/description/properties/file/line`，
身份固定为 `TargetRef { kind: "entity", id }`。`entity_type` 是可变分类，修改
它或显示名不修改 ID，也不改变地图标记的位置；同名的 character、tag 和 entity
保持独立。实体只进入 `catalog.entities` 和统一 `catalog.objects`，不进入事件
执行结构或运行指纹。description/property 的规则与 world 相同，属性值仍限于
字符串、有限数值和布尔值。

实体的正文链接写作 `[[entity:ID|显示文字]]`，并遵守正文链接的转义和目标存在
检查。旧 1.9 工程不自动启用该目标类型，也不把同名旧对象迁移为 entity。
同一对象可被多个标签引用,标签也可以被标签引用。重复标记合并为集合。
文件对象的路径相对于标记所在文件,必须指向本工程已引用的源码。

正文与属性属于所在完整对象,不创建独立可寻址片段。正文已有 `文字 #标签` 继续保留输出元数据语义,
目录将其归到所在完整事件或场景,保留正文行作为引用位置。旧正文标签允许不声明;目录显示为隐式标签。
对结构对象使用 mark 则必须引用已声明标签。
普通文本中的引用、条件和人物属性不能由标签自动改变。

索引区分直接命中和递归命中。查询 coordinate 的直接标签对象数量等于已标记的位置数量;
递归查询继续沿命中的标签查找关联事件、人物、素材等完整对象。访问集合去重,即使标签互相标记也会终止。
递归查询不向对象自动写入祖先标签;删除某个分类关系只改变索引路径。
所有查询保留来源文件和行号,可定位标记或原始声明。

## 1.2 语言 1.10 独立关系

`catalog.relation_types: BTreeMap<String, RelationTypeInfo>` 保存稳定类型 ID、
正向显示名、可选反向显示名、方向、端点约束和源位置；
`catalog.relations: BTreeMap<String, SemanticRelationInfo>` 保存关系 ID、类型、
`from_ref`、`to_ref`、说明、来源、scope、字面量 properties 与源位置。关系实例
同时以 `TargetRef { kind: "relation", id }` 出现在 `catalog.objects`，因此正文、
地图和删除影响可以按完整身份引用它；关系类型本身不是对象端点。

`Catalog::query_relations(target, options)` 是局部邻接查询的唯一分析入口。默认
`depth=1`，允许申请 `depth=2`，节点和边上限分别为 250 与 500。结果固定包含
`schema_version=1`、起点、节点深度、关系边身份、端点、方向、显示标签、
`truncated` 和继续查询信息。遍历保留同端点的多条关系，按关系 ID 稳定排序，
遇到循环只访问同一对象一次；截断不会删除目录中的关系，也不会把 A→B、B→C
推导成 A→C。读取反向端点时使用类型的 `inverse_display` 投影，不能保存反向副本。

继续查询使用 `continuation.offset` 和 `Catalog::continue_relations`，保持同一快照与
筛选；具体遍历及分页边界见 [relations.md](relations.md) §8。Rust 的完整 TargetRef
邻接映射由 core 序列化为 `[{target, relations}]` 数组，CLI/RPC 直接序列化 Catalog。

旧 `character` 块中的 `relation` 仍在 `catalog.legacy_relations` 只读投影；
`LegacyRelationHandle` 带来源、目标、标签、源码位置和重复项序号，但没有持久
关系 ID。需长期引用时必须由 Project 生成提升预览，再以一个事务新增
`relation_def` 并移除旧行；预览同时保存 `content_baseline` 与完整 `RelationDraft`
(包括 `scope_refs`、`properties`)，并明确给出旧指纹与新指纹差异。提交按内容基线
拒绝陈旧预览，失败保持全部源码不变。旧人物关系仍按 1.9 规则参与运行指纹，独立
关系资料不参与。

Project 的关系类型/实例结构写入及提升要求清单显式使用 1.10，并在
required_features 声明 content.relations.v1；缺失时拒绝且不改文档。只读源码分析
仍由 CompileOptions 选择语言版本，不要求内存源码调用方提供工程清单。

## 2. 工作区图片与音频

```wl
asset harbor_art image "assets/harbor.png" as "码头概念图"
asset lin_voice audio "assets/lin.wav" as "林舟声音参考"
attach event arrival with harbor_art
attach character lin with lin_voice
mark asset harbor_art with harbor_place
```

`asset <ID> <image|audio|file> "路径" [as "名称"]` 声明全局唯一素材。file 可引用其他格式的设定资料。
`attach <对象类型> <对象ID或路径> with <素材ID列表>` 将素材关联到任意上述对象。
路径相对于 asset 声明文件解析，必须位于工作区内，禁止绝对路径与远程网址。外部素材先复制进工作区再关联。点击打开时交给系统关联应用；浏览器下载已导入的原始素材。

打开工程允许缺失素材继续编辑，core 产生 A215；越界引用产生 A109。导出要求声明的素材存在且可读取，完整复制工作区全部文件（含未引用素材），保留名称与目录，源码采用当前缓冲。另存遵守目录边界，允许未完成源码。不会自动覆盖 README 或生成 spec 目录。详见 [workspace.md](workspace.md)。

## 3. 校验与兼容

- 标签/素材重复 ID: A104;属性重复: A212。
- 标记/附件引用未知对象、标签或素材: A214,错误阻止完整导出。
- 不存在、不可读取或类型与扩展名不符的素材: A215 提醒,完整导出会拒绝。
- tag/asset/mark/attach/anchor_def/anchor_link 不直接改变运行时、存档指纹、事件先后关系。
- 结构检查覆盖 ID、引用、时间约束及属性定义的一致性;不会自动判定任意自然语言设定是否互相矛盾。

CLI 的 `catalog` 与 agent 的 `analyze.catalog` 提供对象、标签、素材、标记与附件及源位置。
`catalog --tag ID --recursive --json` 返回去重后的对象索引,`--kind tag` 可统计分类下的位置等标签对象。

状态作为完整对象，可被标签标记、关联文件，状态的内容标签与分类标记相互独立。状态引用的标签必须显式声明；详见 [states.md](states.md)。

## 4. 独立叙事锚点

```wl
anchor_def turning_point as "读信后的决定"
  description "林舟开始把港口的命运看作自己的责任。"
anchor_link turning_point character lin
anchor_link turning_point event arrival
anchor_link turning_point state mood
mark anchor turning_point with harbor_place
attach anchor turning_point with harbor_art
```

`anchor_def ID as "名称"` 是顶层声明；可选缩进块只允许一个 `description "叙事意义"`，不接受 property。锚点 ID 在全工程的锚点命名空间唯一，与名称分离。`anchor_link ID KIND TARGET` 可放在任意引用文件，1.9 关联完整 character/event/state/anchor 对象，1.10 另外允许 entity；两端均须存在，重复锚点与未知关联对象报 A217。标记/附件本身的未知引用继续使用 A214。

`catalog.anchors: BTreeMap<String, AnchorInfo>` 保存 `id/display/description/file/line/links`；每条 `AnchorLink` 含 `anchor/target/file/line`。通用 objects、references、标签查询与附件查询均包括锚点。`Catalog::anchors_for(&TargetRef)` 反查直接关联某角色、事件、状态或锚点的独立锚点；不隐式沿角色的全部事件扩散关联。

`Catalog::anchor_changes(id)` 返回 `Vec<(&StateInfo, &StateChangeSite)>`，只取关联状态中属于关联事件的动作，包括嵌套条件、场景和效果；源码文件与行号用于定位，持久身份仍是对象 ID。无状态或无事件关联时返回空。不会因链接到另一个锚点而自动复制其变化。

结构编辑使用 `AnchorDraft { id, display, description, targets: Vec<TargetRef> }`，前三项为 String。`Project::write_anchor(original, &draft)` 创建或编辑资料及关联，`Project::set_anchor_links(id, &targets)` 替换全工程直接关联；调用方用 `Project::edit` 校验并提交交易。已有 ID 不可由表单改名，注释保留，引用失败整批回滚；显示名称可以修改。

独立锚点描述作者定义的意义，不执行动作，不制造时间顺序，不改变存档指纹。正文旧 `anchor "名称"` 继续生成手动演练记录；`analysis.anchors` 与运行时 `state.anchors` 不自动转为独立对象。

## 5. 别名与正文对象链接

顶层 `alias character lin as "阿舟"` 为完整对象增加查找名称。目标类型与 mark 相同，文件目标使用带引号的工程源码路径，相对于声明文件解析。可跨文件声明；相同对象的重复别名合并显示，不同对象允许同名，搜索返回全部候选。别名不能为空白或包含换行，不产生新对象或运行状态；目标不存在报 A218，格式错误报 P004。

正文及选择文案支持 `[[character:lin|阿舟]]`，也可链接 tag、event 等完整对象。类型与目标 ID 用冒号分隔，竖线后必须明确写出显示文字。scene 使用限定 ID，file 使用相对于所在源文件的已引用源码路径。显示文字不包含方括号、竖线、花括号、反斜杠、双引号、#、~、换行及注释分隔符；格式错误报 P004，未知目标报 A218。写 `\[\[` 可输出普通双左方括号；选择引号内按字符串规则写双反斜杠。属性和说明中的相同文字不自动建立正文链接。

播放只输出显示文字；链接不执行目标、不改变时间、准入、人物在场或状态。修改目标显示名、别名或链接目标身份不改变既有文字；新增／删除链接但保留相同显示文字也不改变存档指纹。显示文字本身变化仍改变指纹。人物 ID 的结构改名同步别名目标和正文／选择显式链接，保留显示文字、普通同名提及与注释。

`catalog.aliases` 保存 target/name/file/line；`catalog.text_links` 保存 source/target/label/file/line/column，source 是所属完整事件或场景。正文链接同时进入 references（kind 为“正文链接”）；多个位置可分别定位，搜索对象按类型与 ID 去重。核心提供按名称、ID、别名筛选的对象查询；同名候选不自动合并。

编辑器提供人物常用属性栏目（外貌、经历、性格、动机、底线、口吻及例句）和字符串多行编辑，均为可选静态资料；既有字段不覆盖、不强制填写、不自动变为状态或事件。长文本沿用字符串的换行转义，不另存一份人物卡。

对象资料阅读页从当前工程快照汇总说明、属性、别名、关系、事件、状态初始定义及源码变化、锚点、引用与素材路径。源事件的变化分开列出条件与出处，不计算唯一当前事实；文件继续交系统应用打开。正文中的对象链接可点击阅读目标，资料页不是第二份可编辑真源。

## 6. Wiki 关键词与注释索引

Wiki 是已有完整对象的阅读索引。人物、世界、事件、场景、故事线、时段、状态、锚点、显式标签与素材的显示名称及别名自动成为关键词；文件名、变量和未声明正文标签不自动成为关键词。ID 仍可用于搜索。独立释义复用 `tag ID as "关键词"`、`description "释义"` 与 `alias tag ID as "别称"`，不新增语言关键字或第二份资料文件。

关键词由 core 的 `wiki::KeywordIndex` 生成，编辑器统一用于正文概览、资料阅读和试玩显示。匹配从左至右、同一位置优先最长名称，保留每次重复出现；中文可连续匹配，ASCII 字母忽略大小写，ASCII 字母、数字及下划线遵守完整词边界。关键词不能为空白或包含换行。同名对象保留全部候选，点击后由读者选择；显式正文链接优先，其显示文字不再自动拆分或改换目标。

词条的“出现位置”从当前全部源码缓冲查询，包含声明、说明、属性、正文与选择，保留文件、原始行号、按 Unicode 字符计数的列号和原始行预览；源码注释不计入。显式正文链接计入它指定的目标，内部 ID 和显示文字不再额外匹配其他词条。此索引表示文字出现，不自动生成 mark、对象语义引用或执行关系。插值生成的运行文字可以点击，但运行时生成的拼接词没有虚构的静态出现位置。

`Project::write_wiki_entry(original, &WorldDraft, aliases)` 复用标签及别名编辑，调用方须在 `Project::edit` 事务中提交。词条随普通源码保存、撤销、外部刷新和完整导出；更新释义或别名不改写正文，也不影响运行状态或存档指纹。

## 7. 组合查询与待办投影

`CatalogQuery` 是只读查询 DTO，`schema_version` 必须为 `1`。查询对象是当前 Project 缓冲编译出的完整 `TargetRef`，不会读取或刷新磁盘。查询条件按维度组成：`kind`、`name`、`tag`、`property`、`relation`、`author_scope`、`missing`。同一维度的 `values` 是 OR，存在的不同维度是 AND；每个维度可用 `negate: true` 否定整个 OR 集合。空 OR 集合恒为假，否定后的空集合恒为真；没有维度的查询匹配全部对象。name 对显示名、ID 和别名作不区分 ASCII 大小写的子串匹配；tag 可明确请求递归解引用，循环去重。

property 只匹配目录可取得的字面量属性（character、entity、relation、tag 与 world）；比较要求字符串、有限数值或布尔类型完全相同。缺失属性条件如 `{"kind":"property","key":"source"}` 表示对象没有该字面量属性；不判断自然语言真假、不生成属性或事实。未知属性键是合法查询但匹配为空；未知属性值类型、查询版本或维度会产生查询错误。relation 仅检查已声明的直接有向边；不作传递、对称或路径推断，方向与类型可筛选。因而关系环不会触发递归遍历。

`author_scope` 是调用方传入的 `.wl` 工作区相对路径集合，匹配对象主要声明所在源码文件；它不是个人作者身份，也不读取个人资料。路径必须在工作区内，禁止绝对路径和 `..`。缺少的“来源”应表达为 `missing` 的 `property` 条件（例如 `source`），只表示资料字段未填写。

每个命中返回 `target`、对象声明的 `source {file,line}` 和逐维度可读 `reasons`。命中按 `TargetRef.kind`、`TargetRef.id`、路径和行号升序，不按显示名称合并。summary 使用可见维度和值的确定性中文摘要。每个维度的 OR 候选最多 100 项；分页大小为 1–100，候选对象预算默认 10,000 且上限 100,000；超预算返回错误且不返回部分页。可取消查询每 64 个候选检查一次取消回调，取消时不返回部分页；编译当前缓冲阶段不可取消。

游标含版本、offset、page size、`max_candidates`、查询指纹和 Project `content_baseline`。续页沿用游标中的页大小与候选预算；必须仍使用相同 DTO 与未变化缓冲，任一不符返回 `StaleCursor`，调用方从第一页重查。结果、查询与游标不推进或刷新 Project 生命周期。查询包含语言诊断，避免把不完整语法的部分目录误报为无条件完整结果。

已保存查询是共享展示文档，不含用户身份、收藏或 TODO 状态。清单 `saved_queries` 将稳定 ID 映射到 `.json` 路径；非空注册须声明 `catalog.saved_queries.v1`。其定义可用于查询当前缓冲，不进入故事运行指纹。文档 schema 版本或必需能力未知时只读保留原始字节；更新支持版本时保留未知 JSON 字段。

`Project::todo_projection()` 是非持久、只读的当前缓冲投影：缺失正文链接逐位置成为断链项，同一缺失 `TargetRef` 另聚合成一个待建条目；未解决且失锚的批注、状态为 `open` 的提案分别成为待办项。待办 ID 从类别、来源路径/位置与目标（或批注/提案 ID）确定性派生，不是长期身份；来源文件仍是原文档的唯一所有者。已解决批注和已接受提案不投影。诊断、不完整或未知版本文档保持其原有可见性与只读规则，不因投影被修复或改写。
