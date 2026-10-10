# 正式对白创作（工具 0.34）

本契约不新增 DSL。语言默认 1.9，最高既有 1.13；`say` 仍要求显式 1.11。
正文、台词与源码共用按文件唯一 `WritingBuffer`。临时表单不是第二份持久正文。

## 编辑器的显式同稿子模式

默认正文入口保留既有 Text 直接编辑缓冲、原选区及导航返回行为；目标中存在 Say
不能自动接管这些入口。作者明确选择“逐句对白”，或发起插入/续句，才进入同稿
Text/Say 的 typed 逐句界面；模式开关必须反映真实显示状态。逐句模式里的 Text
仍通过下述 core 计划完整预览、纳入缓冲、应用及保存，不是只读摘要。

模式切换不改变语言、作品内容或保存状态，也不隐式提交或丢弃未纳入字段。当前
输入接收者先处理已到达的文本/IME；切换不能将输入送入另一来源或重放被拒动作。
退出、返回、取消和试演沿用现有草稿守卫。真实来源选区只能由既有正文/源码映射
恢复；typed 解码字段的光标不能被冒充为源码字节偏移。

## 正式投影与 typed 文本

公开入口在 `worldline_core::manuscript`：

- `Project::project_dialogue_buffer(buffer, target) -> Result<DialogueProjection, DialogueError>`
- `Project::preview_dialogue_edit(buffer, request) -> Result<DialogueEditPlan, DialogueError>`
- `Project::stage_dialogue_edit(buffer_mut, plan) -> Result<(), DialogueError>`
- `Project::apply_dialogue_edit(buffer, plan) -> Result<(), DialogueError>`
- `Project::dialogue_continuation(buffer_after, plan) -> Result<DialogueInsertionAnchor, DialogueError>`

投影只读编译所选文件的完整当前缓冲，使用正式 AST、lexer 和 provenance；
无效草稿返回错误且保留完整源码救援入口，不能借用旧投影冒充当前稿。
目标支持 event、scene、fragment；entity description 不是执行 Text，不提供转换。
跨文件来源仅在其对应文件的缓冲中编辑，不猜来源或展平调用。

`DialogueProjection` 字段为 schema_version=1、target、baseline、generation、
snapshot、statements、anchors、rows、speakers、complete=true。
`rows` 为按正式 ReviewProjection 顺序展平的 `DialogueRow`：
`{depth, label, statement_id?, source?, kind}`，kind 沿用 ReviewKind；控制行保留
原条件（附于 label 且明确未求值），不把显示分组当作可编辑语句。
`speakers` 为正式 `ReviewSpeaker {target,display}`，同名人物按完整 TargetRef 区分。

`DialogueStatement` 含 id、kind、source（ReviewSource）、draft、parts、glue、tags、
localization_id、after_anchor_id（可空，正式下一句动作仅用此锚）。id 为绑定当前快照的不可持久化标识；定位长期身份只能用已有稳定
localization_id。完整 WritingBlock 合并的多条正文不能成为一条 statement。
同一 Project 基线、完整当前文件、generation 与观察代次生成相同 ID；机器入口
每次以当前 Project 新开 generation=0 缓冲，不远控 GUI 未应用稿。

`DialogueDraft` 为 `{kind,speaker?,direction?,parts}`；kind 为 text 或 say。
say 必须有 `speaker:{kind:"character",id}`；text 的 speaker 和 direction 必须为空。
`DialoguePart` 是按 kind 标签区分的严格枚举：

- `{kind:"literal",text}`：已解码的字面文字
- `{kind:"expression",source}`：未求值的正式表达式源码，不含外层 `{}`
- `{kind:"link",target:{kind,id},label}`：强链接，不从字面字符串推断

表达式新增/修改是明确 typed 动作；core 使用正式解析器及整个候选编译验证。
literal 内出现 `{}`、引号、反斜杠或链接外形仍是字面文字，由 core 编码。
Enter 为同一 literal 的换行，编码为语言既有转义；创建下一语句须独立动作。
当前 Text 无法无损表示的特殊字面值明确拒绝，不暗换 kind、不求值或清洗正文。
相邻 literal 和空 literal 在输入比较时规范为一个 literal/空 parts；不会由此改稿。

`DialogueMappedPart` 含 part 与 `source_range:Option<Range<usize>>`，范围是当前
文件 UTF-8 原始字节。表达式和链接来自正式 provenance；未证明的 literal 选区
映射为 None。界面不得把解码后字符位置当源偏移，也不得在 None 时启用选区关联。
只编辑其他字段不会丢失已有表达式、链接或稳定本地化 ID。

`DialogueInsertionAnchor` 含 id、line、byte_offset、label；只提供 core 证明安全的
正式语句前/后兄弟插入位置。结构头之后、选择组中间和无法证明的注释跨度不提供。
空 event 的终止语句之前提供插入点。锚采用当前语句缩进和局部 LF/CRLF；不接受
客户端任意 offset。EOF 没有末尾换行时只加入插入所必需的分隔符。

## 请求、计划与应用

`DialogueEditRequest` 严格字段：schema_version=1、expected_baseline、target、
generation、operation、enable_language_1_11=false。
`DialogueOperation` 严格 kind 标签枚举：

- update：statement_id、draft；draft.kind 必须与原句一致
- insert：anchor_id、draft
- delete：statement_id
- convert：statement_id、to（text/say）、speaker?、allow_direction_loss=false

convert 只改变正式单语句 kind，内容、稳定 ID 与注释保持；不同结构、多语句或
任意选区没有转换入口。Text glue=true 或 tags 非空时拒绝转换为 Say，因为当前
Say 固定 glue=false、tags=[]。Say 转为 Text 丢弃非空/空串 direction 都须显式
allow_direction_loss=true；该标志代表产品用户已确认预览所列备注损失，机器调用者
不能将未获授权的请求包装成已确认。尚未确认的预览仍返回计划并 can_apply=false，
列出 metadata_losses；应用拒绝。speaker 移除是转换的明确固有变化，计划同时显示。

`DialogueEditPlan` 含 schema_version、request、baseline、generation、snapshot、
source_path（工作区相对路径）、range、before、after、old_speaker、new_speaker、
metadata_losses、migration?（既有 CapabilityEnablePlan）、changes（完整逐文件
WritingAuthoringChange）、includes_unapplied_draft、runtime_fingerprint_before、
runtime_fingerprint_after、fingerprint_comparison_reliable、can_apply、no_change、plan_digest。公开字段是预览，
应用必须重建计划并比较，不能信任客户端修改的 range/before/after。

非迁移 stage 只变同一 WritingBuffer，Project 与磁盘不变；之后显式应用正文。
apply 将当前文件完整未应用草稿与所列编辑一次提交到 Project，绝不保存。
仅在需要 Say 且明确 enable_language_1_11 时预览升级；当前语言更高则保持。
迁移计划只允许 apply：清单变化、完整当前稿与台词修改在一个候选内编译，一次
替换 Project、一次撤销；stage 不留下没有语言能力的孤立台词或隐式清单草稿。
取消与 preview 零写入。no_change 返回原字节、原代次且 apply 也不提交无关草稿。

保护绑定完整 Project 内容基线、完整输入文件及 original、generation、完整目标
片段、工作区根与观察代次；重验当前活动来源、文件保存基线、普通文件库存、只读
能力、恢复冲突及目标角色。预览后任何相关变化要求重新预览，不自动套用旧计划。
无效/过期/预算失败/不可表示/失效目标均零 Project/缓冲/磁盘写入；表单输入保留。
请求 JSON 上限 64 KiB；完整变更 before+after 上限 8 MiB；投影继承正式 review
的 16 MiB 源码、10000 节点、64 层、1 MiB JSON 上限，超限不返回截断成功。
附加语句索引使用显式遍历并只保留所选 review 的来源键，沿用正式 provenance
的所属声明和跨文件身份；不为每条台词重新扫描整份注释，也不逐条线性查重插入锚。
这些索引不扩大投影范围、不改变来源或锚的顺序，不能绕过上述完整性预算。

更改只重写正式目标语句范围；独立注释、行尾注释、其他声明、结构、缩进、换行
保持。删除保留注释与行分隔，不把注释误删。句内嵌块注释等不能局部无损重建的
情况明确拒绝，完整源码入口继续可用。

## 本地化与语义

Text↔Say 改变 unit kind，source_revision 依既有规则变化，稳定 ID 与旧译文保留
并标 stale。只改 speaker 或 direction 不改变当前 unit source_revision，可能仍为
Translated；但完整稿快照与角色台本归属必须失效，不能仅用翻译 revision 或运行
fingerprint 作为身份。speaker 的运行含义及 direction 作者元数据地位保持既有规范。

错误结构 `DialogueError {code,message}`，message 中文；code 为 INVALID_REQUEST、
STALE_BASELINE、STALE_DRAFT、SOURCE_UNAVAILABLE、INVALID_DRAFT、INVALID_SPEAKER、
UNSUPPORTED_CONVERSION、CONFIRMATION_REQUIRED、MIGRATION_REQUIRED、READ_ONLY、
EXTERNAL_CONFLICT、BUDGET_EXCEEDED。输入 DTO 拒绝未知字段，机器层还须拒绝重复键。
UI/CLI/RPC 共用上述计划；机器 apply 仅修改自己 Project 内存，保存仍是另一动作。

core 提供 `parse_dialogue_edit_request(&str)`（64 KiB）与
`parse_dialogue_target(&str)`（4 KiB）；使用唯一键 JSON 解析且所有嵌套
TargetRef 采用本入口局部严格反序列化，不修改全局旧 TargetRef 兼容规则。

下一句续写只接受计划的私有完整结果见证：stage 后使用同一缓冲，atomic apply 后
使用重新打开的当前缓冲。core 验证完整结果、原文、基线、代次、工作区观察及目标，
重新定位刚修改的正式语句后返回其 after-anchor；不接受客户端推算字节范围。
删除、未确认或没有安全 after-anchor 的计划不提供续写。
`WritingBuffer::identity()` 包含路径原始编码字节、完整原稿/当前稿、基线及代次，
供 UI 缓存键使用；值在创建、替换与安全 rebase 时刷新，读取只返回缓存值。
撤销分叉回相同代次不能复用旧页。缓存键不是修改授权。
计划还绑定全部普通文件库存及保存基线（单资源 64 MiB、总校验 256 MiB），
附件改动即使未改变正文或语言也会使旧编辑计划失效。

新增或修改的 typed expression 使用正式词法后的资源门：最多 256 个表达式 token、
最多 64 层括号/一元运算嵌套，超限为 BUDGET_EXCEEDED，不进入递归解析。该门仅保护
新作者动作，不降低语言本身的既有能力；已经完整编译且源码未变的复杂表达式保持
原 token 源文，编辑其他字面内容、角色或 direction 不会重写或暗中丢弃该表达式。
完整源码入口仍遵循原编译能力。
下一句私有见证还包含精确源文件路径以及应用前/后完整库存守卫；stage 后附件变化
或 apply 后保存基线变化都要求重新取正式锚，不能以同文不同文件满足旧见证。
