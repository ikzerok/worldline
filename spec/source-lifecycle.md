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
超限显式拒绝且不截断证明。提交前重新检查目的缺失、完整保存基线和资源摘要；
取消回调返回后也执行这一步。
