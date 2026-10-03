# 安全源码生命周期（工具 0.16）

本契约不新增 DSL，不改变默认语言 1.9 或最高显式语言 1.13，不自动迁移能力。
所有命令由 core 拥有；编辑器、CLI 与 agent 只消费相同请求和逐处预览。

## 新建和引用

`Project::add_file(relative)` 创建活动 `.wl` 缓冲，并向工程入口添加 include。
旧工程维持既有递归源码行为；显式 `workspace.source_sets.v1` 工程在同一候选事务内
将新路径加入 `source_config.active`，保留 archived 和未知可选字段。任一检查失败均
不留下孤立新缓冲、半条 include 或清单修改。创建不会覆盖既有文件或删除墓碑。

`Project::include_file(path)` 只引用存在、非删除、已活动的工作区源码。显式工程的
归档或其他未活动源码返回明确错误，要求作者先另行授权启用，不隐式激活。
引用入口自身、越界、不存在、链接和只读工程均拒绝。旧工程仍可载入普通既有源码。
两操作不保存，不升级语言，不要求作者先修复与操作无关的未完成正文。
直接 Rust API 接受平台原生 `Path` 组件（包含 Windows `Path::join` 产生的目录分隔符），
写入 include 和 source_config 时统一为 `/`。只允许工作区内的普通相对源码组件，仍拒绝
上级跳转、控制字符、非 Windows 文件名中的反斜杠及保留事务/检查点目录。
机器生命周期请求中的路径始终使用 `/`，所有平台均拒绝请求内的反斜杠。
请求解析后的文件身份统一采用 compiler 的规范原生路径；缓冲、登记引用和语义加载
顺序证明使用同一表示，不把 Windows 分隔符差异当作顺序变化，也不忽略真实顺序变化。

## 单源码安全移动

只支持一个 Project 已跟踪且非删除、非工程入口的工作区内 `.wl`。目标是工作区内
不存在的相对 `.wl`，支持中文、空格与多级目录；目录批量移动、入口迁移和素材迁移
不在本版。源可为活动、归档或其他未活动源码，移动后保持完全相同的成员身份。
源与目标禁止相同身份、大小写碰撞、软链接、目录联接、越界、已有磁盘文件或已有
缓冲（包括墓碑）。目标父级不得穿过普通文件。工作区源码清单必须已完整载入。

### 正式源码字段清单

只在正式 lexer/parser 提供的原始 UTF-8 token 范围内修改路径：
- 所有源码的 include.path
- alias/mark/attach/state 的 file TargetRef；relation 的正式 scope_ref
- anchor_link 当前不支持 file kind，沿现行语法拒绝，不扩大目标种类
- 正文、choice/once 标签、say 的正式 file 链接
- 被移动文件的 asset 声明 path

入站路径由原声明位置解析；指向移动文件者改到新路径。出站 include、asset 和 file
引用按新声明位置重基，必须解析到相同对象/普通文件。普通正文、注释、description、
链接 label、未知字段内相同字符串保持原字节。未受支持的语法不得猜测改写。
全体未活动源码仍作词法路径检查，不进入活动编译，不因移动而公开或激活。
引用待删除源码不得回读旧磁盘补全；素材声明将待移动源码作为原始附件时，因不能
同时保证附件原始字节与源码路径重基，首版明确拒绝。

### 已登记 JSON 字段清单

file TargetRef 的 id 沿用 core 目录的既有绝对路径身份（不改协议表示）；源码集合、
批注正文来源与共享查询范围使用既有工作区相对路径。按原始 JSON 字符串 token 修改：
- maps：placements.*.target_ref/scope_refs；scene.nodes.*.target_ref/scope_refs
- graph_views：focus，positions 的 file:<path> 键
- comments：object anchor.target；text_range anchor.path（保留原 quote/hash；若引用行正文重基改变批注附着状态，拒绝移动，不伪造重新确认）
- presets：scope_refs
- saved_queries：query.filters 中 relation.values[].related，以及 author_scope.source_files
- reader_profiles：selection.objects/fields[].target（仍受既有选择格式校验；routes[].target 现有契约不支持 file，拒绝此类无效路由）
- manifest：source_config.active/archived

manuscripts 的 entries[].target_ref 只允许 event/scene/entity/fragment；正式 POV 字段为
entries[].pov 且只允许 character，不存在 file 迁移，未知可选 perspective 保持原样。
templates 的 object_ref defaults 只允许现有 entity/relation/character，不存在 file
迁移；localization 的运行身份与本版允许的源码路径无关，不猜测改写。proposals 含
已捕获源码路径/摘要和基线，无法证明迁移等价时拒绝整个移动。任何已登记文档有未知
required feature、未知 schema、无效 JSON、只读或未受支持路径语义均拒绝。未知可选
字段与所有未改动字节保留；不承诺理解第三方在扩展字段中私藏的路径含义。

### 等价证明

移动前和候选都须无活动编译错误。除源路径替换外，比较完整语义加载顺序
`program.files`、默认事件入口、对象 kind/稳定 ID/显示名和声明顺序、全部正式目录
引用的 source/target/kind、素材 resolved_path/可读原始字节，以及运行 fingerprint。
正式 file 身份使用 old→new 的一一映射比较，其他身份不得变化；声明来源同步映射。
资源解析证据保存在预览。归档源码的出站资源同样检查存在性与边界。
仅编译成功不足以通过。未显式 include 导致路径排序/默认入口/合并顺序变化时拒绝，
不偷偷新增 include 或升级 source_sets。运行指纹不同（例如 file 所属状态的运行身份）
明确拒绝，不迁移 Story save、checkpoint 或 trace。

#### 确定性与顺序边界（工具 0.17）

`catalog.references` 是保留重复次数的正式引用多重集，数组次序不表示声明或执行
次序。core 按来源路径、行号、source.kind/id、target.kind/id、kind 完整键稳定排序；
同键重复项全部保留，即使两次正文链接处于同一行、具有相同目标和标签，也不得去重。
移动证明先对有类型的来源路径和 file TargetRef 作 old→new 映射，再按同一完整键
排序比较；新路径改变排序位置不能成为拒绝理由。对象索引按 TargetRef 排序，关系邻接
索引按 TargetRef 键映射后重建；所有条目、关系端点和重复关系 ID 均保留。

此规范化仅作用于上述派生索引。`program.files`、默认入口、声明和语句次序、正文
链接出现顺序、选择顺序、条件分支、效果与状态变更顺序均保持有序比较，不能递归排序
全部数组或把它们改成集合。显示名、说明、属性原值、关系 from/to/type/scope、
文件/行号及旧式关系的重复项序号都参与证明。只允许正式路径重基引起的文件默认显示名、
素材原始相对路径及正文链接列偏移变化；它们须分别由身份映射、资源字节与精确 token
改写保证，不忽略整项资料或任意源位置。

同一 Project 内容与同一请求的预览计划及摘要须跨重复编译和独立进程稳定；apply
重新计算也使用同一确定性证明。不得用重试、忽略错误、去重引用或关闭等价守卫来消除
随机误拒绝。回归至少包含最小两人物一事件和完整作者 fixture：同 Project 100 次
预览、100 个独立进程预览及同一成功计划在 100 个独立 Project 副本上应用。保留
真实引用改变、执行顺序改变、默认入口/加载顺序改变、过期、外改、入口移动和越界
的零修改拒绝，并验证 save/reopen、源字节/路径撤销与未引用普通文件保留。

## 事务和 DTO

`SourceLifecycleRequest` 使用 `operation` 标签：
- `{"operation":"create","path":"章节/新章.wl"}`
- `{"operation":"include","path":"章节/已有.wl"}`
- `{"operation":"move","from":"old.wl","to":"章节/新章.wl"}`

`Project::preview_source_lifecycle(&request)` 返回只读 `SourceLifecyclePlan`：request、
content_baseline、plan_digest、changes（path/after_path/kind/occurrences）、source_path、
destination_path、membership、runtime_fingerprint_before/after、entry_before/after、
load_order_before/after、resources。occurrences 复用 RefactorOccurrence 的逐处字节范围、
行号、字段、前后 token 和上下文。资源项有声明文件/字段、前后路径与解析目标、内容摘要。

`Project::apply_source_lifecycle(&request, plan_digest)` 重新生成全部计划，比对摘要并
在一次内存替换前检查完整内容基线、全部保存基线、源码集合、目的缺失与资源指纹。
`Project::apply_source_lifecycle_plan(&plan)` 另外严格比较完整 DTO，拒绝篡改任何预览。
可取消版本在计划阶段和提交前检查取消回调；取消后零修改。同步提交不提供部分选择。
成功返回实际计划。调用者在 apply 前保存一个 Project snapshot，用 restore 实现一次
undo/redo；core 不偷偷保存。保存复用既有 recoverable journal 的旧路径删除与新路径
写入，保存失败保留可恢复事务，不宣称磁盘跨文件物理原子。

### 有类型的失败回执（工具 0.17）

Rust 的兼容新增入口 `preview_source_lifecycle_classified`、
`apply_source_lifecycle_classified`、`apply_source_lifecycle_plan_classified` 及前两者的
`_cancellable_classified` 形式返回 `Result<_, SourceLifecycleFailure>`。
`SourceLifecycleFailure { kind, message }` 保留原始中文详情；`kind` 为
`SourceLifecycleFailureKind`，按 snake_case 序列化：

- `SourceChanged`：内容基线/完整预览不符、外部保存基线变化、新增未载入源码、提交前资源变化或未解决保存事务；提示保留稿件、检查外改并重新预览
- `IllegalPath`：相对路径/边界不符、目的碰撞、入口移动或不允许写入的路径；提示选择合法的非入口工作区路径
- `SemanticChange`：明确检出的加载/默认入口、运行指纹、正式引用或资料、资源字节、成员身份或批注附着变化；提示修正造成变化的内容或保留原路径
- `UnableToProve`：坏稿、未知能力/注册语义、预算、取消、内部投影或读取错误等无法完成证明的情形；保留详情，不冒称已经证明语义改变

类别必须在 core 已知守卫产生错误时指定，不解析中文消息猜类别；未分类的旧底层
错误仅归入 `UnableToProve`。旧 String API 包装同一实现并仅取 message，既有 CLI/agent
错误协议不变。分类只服务解释和下一步提示，不授权重试、不自动刷新/保存，不削弱
任何拒绝或零修改保护；编辑器直接消费 enum，技术详情可展开。

CLI：`wl source-lifecycle preview|apply <workspace> --request-json JSON [--plan-digest DIGEST] --json`。
agent：`project.source_lifecycle_preview` / `project.source_lifecycle_apply`，params 仅指定
path 或 project_id 之一、request；apply 必须提供 plan_digest。机器 apply 成功后保存，
源码错误为 `ok:false` / `SOURCE_LIFECYCLE_REJECTED`，协议形状错误才用 JSON-RPC error。

## 必须的回归

explicit add/include 假成功；legacy 创建；中文多级/空格；注释与普通值原字节；入站与
出站 include/file/asset；同名不同 kind；所有已登记 JSON 字段；活动/归档/未活动成员；
目标碰撞、大小写、路径/链接、缺资源；过期/外改/取消/篡改/未知能力零改；无 include
排序拒绝；状态 file 身份指纹拒绝；一次 restore 往返；save/reopen 和恢复日志沿用。

### 有界检查

单次组织最多检查 4096 个工作区文件/目录条目与 4096 个已跟踪缓冲；源码当前缓冲
和已登记 JSON 当前缓冲分别最多 64 MiB，均在候选克隆前检查。最多 16384 处正式路径；
每份资源和磁盘保存基线最多 64 MiB，去重资源总量及保存基线总量分别最多 256 MiB。
源码生命周期使用独立有界保存基线检查，不改变其它检查点 API。扫描和读取在超限前
停止，不先全量复制再拒绝。普通文件以非链接、只读句柄有界读取；Unix 非阻塞打开
避免 FIFO 替换造成阻塞，Windows 使用 reparse-point 句柄并拒绝链接/非普通文件。
Windows 保留全体祖先目录句柄，允许 READ/WRITE 共享但禁止 DELETE 共享，避免干扰
独立子目录的创建与发布，同时阻止祖先删除/改名。目录写共享不承担重定向隔离：逐级
检查非 reparse 目录，打开叶文件后在任何内容读取前核对句柄的完整最终解析路径；
中途 junction 重定向导致路径不符时拒绝。叶句柄仍仅共享 READ，禁止写入/删除替换。
超限显式拒绝且不截断证明。提交前重新检查目的缺失、完整保存基线和资源摘要；
取消回调返回后也执行这一步。
