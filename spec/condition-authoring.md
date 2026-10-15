# 工具 0.36：有类型条件的安全往返

本契约先于实现。公开模块为 `condition_authoring`；不新增 DSL，不更改默认 1.9、
显式 1.10–1.13、`and/or` 急切左到右求值、RNG、存档或 replay 合同。
投影和预览只解析与校验，不执行表达式。

## 身份、稿源与投影

`ConditionOwner { target: TargetRef, choice_line: Option<u32> }` 的 target 必须为完整
`event` 身份；event after 的 choice_line 必须为空，choice 两槽必须有大于零的行号。
Project 槽的 choice_line 是声明所在文件的物理行；未应用表单槽是 EventDraft.body
中的物理行。二者由不同 API 明确区分，不混用行号。
片段 fragment 中的 choice 不属于本版 owner 范围，明确拒绝；不宣传全语言 choice 的表单支持。
`ConditionSlot` 为 `choice_display/choice_enable/event_after`。

`Project::condition_authoring(&ConditionSelector)` 与其 `*_from_compile` 版本只读定位
已应用源码；后者核验同一次 CompileResult 的完整活动源码与能力，不重新编译。
选择器为 `{schema_version:1, owner, slot}`。投影携带 Project 内容基线、外部刷新代次、
完整选择器、精确 UTF-8 原文、来源文件/行/列/字节半开范围及支持状态。
来源须由正式 lexer/parser 提供，不按文字搜索、正则、显示名或第一个相同表达式定位。
缺少/重复声明、错 owner、错槽或不可信来源拒绝，不回落到第零项。

`Project::condition_draft_from_compile(compiled, input)` 消费
`ConditionDraftInput {schema_version:1, owner, slot, generation, source}`。
source 是该表单当前单行原文，generation 是调用方的输入代次；目录来自明确冻结的
CompileResult。投影同时绑定 Project 基线/刷新代次和输入代次；不是已应用源码。
该 API 可以处理已存在事件表单中的新增选择；不会把未应用正文解释为已应用作品。
未应用源码/WritingBuffer 应先按其既有快照编译，再使用匹配的 Project，不能拿旧目录
偷偷覆盖新稿。UI 保留输入，明确显示稿源；多个作者缓冲不能隐式合并。

## 有界支持子集

ConditionNode 是严格 tagged DTO，类型为 `bool/global_bool/has/compare/not/and/or`。
Bool 保存布尔值；global_bool 保存全局变量 ID；has 保存 state_id/tag_id；
compare 保存 variable、op(eq/neq/lt/le/gt/ge)、typed literal(num/str/bool)及 literal_left。
not 为单子节点，and/or 为有序 left/right 二叉节点。二叉关联与顺序完整保留；
不折叠、不交换、不短路、不展开 rule。节点身份只是本投影内从根起的 left/right/child
路径；不能持久化为作品 ID，也不能跨 generation 使用。

只支持已声明 state/标签、全局 bool 和全局 num/str/bool 与同型字面量的合法比较。
只有 num 允许大小比较；str/bool 只允许等于/不等于。负数是已有一元负数字面量。
state 候选包含其完整所属对象 TargetRef；变量/标签均用真实 ID，不能按显示名重绑。
所有引用与类型由 core 再校验。字符串转义由正式 parser 验证往返，不接受不能无损
表示的 CR、非有限数字或错误占位 AST。

状态区分 empty、supported、advanced、invalid、budget_exceeded、version_unsupported。
任意 rule、local、参数、算术、rnd、when 或其它调用使整式 advanced；未知引用、
非法类型/解析诊断使整式 invalid。无效、复杂、超限均保留完整 source，tree 为空；
不能部分降级、把 parser 的 Num(0) 占位当有效条件或允许表单覆盖其余表达式。
高级源码继续使用既有源码编辑入口。冗余括号原字节在无变化时保留；实际变更显式
输出完整括号，从而保持 AST 的真实关联。表达式内注释不能安全改写时整体高级保留。
enable 仅显式 1.12+，旧语言显示版本不支持且不自动升级。

## 草稿与源码事务

`preview_condition_draft(compiled, input, replacement)` 返回 ConditionDraftPreview；
replacement 是 Option<ConditionNode>，None 明确移除条件，不表示缺失请求字段。
`apply_condition_draft(compiled, current_input, preview)` 重建并核验整个预览后只返回
新字符串，由 UI 写回同一个字段并形成一次既有表单撤销；不写 Project 或磁盘。
无语义树变化返回逐字节相同 source。取消与失败不修改任何作者输入。

Project `preview_condition_authoring(request)` / `apply_condition_authoring(request,digest)`
共用同一实现。ConditionAuthoringRequest 包含 schema_version、expected_baseline、
expected_generation、selector、replacement。仅修改一个已存在表达式槽；空槽只读显示，
新增或删除槽继续由现有 event/choice 表单负责（enable/disabled 成对）；机器接口明确
拒绝不能精确表达的槽结构改动。这样不存在遗留 disabled 或暗删作者说明。

Project 预览显示 exact patch(before/after/range)、changed 与前后运行指纹；apply 重建
全计划并核验 digest，仅原子提交 Project 内存，不保存。无变化 patch 为空且源码零字节
变化。实际修改仅替换正式 expression 范围；once、label、其它条件、disabled、去向、
本地化、注释、CRLF、正文与其它文件不重建。表单 source 可能与已应用槽不同，不能
把 draft preview 直接作为 Project 写入计划。

既有 EventDraft::write_choice / Project::write_event 在仅修改现存条件时采用正式来源局部 patch；
完整表单无变化零字节，不能经整块重序列化破坏其它 CRLF、注释、本地化或正文。
三槽表单原文也从正式表达式范围提取，保留范围内完整注释，不使用词法清洗后的中间串。
表达式含注释时 typed 投影整体为 advanced；作者仍可在既有高级字段显式修改完整原文，
包含仅改注释或主动删除注释。普通 raw 写入只替换该完整范围并校验完整候选；截断或
未闭合的注释边界仍拒绝，不能借 serializer 回退覆盖其它条件或吞掉来源错误。

所有写入和草稿确认绑定 root、source/内容基线、刷新代次、能力、owner/slot和当前输入；
检查工作区只读、活动源码、磁盘保存基线、未知新增/删除、链接、未解决事务和冲突。
预览期间变化、旧 plan/输入代次、只读或损坏声明一律整体拒绝，保留输入等待重预览。
保存本身不推进外部刷新代次；apply 不隐式保存。Project 快照沿用现有撤销/重做合同。

## 预算、候选与错误域

表达式原文至多 16 KiB 可进入表单；输出仍完整保留超限原文，超过请求/输出总预算时
明确失败，不截断伪称完整。表单树最多 128 节点、24 层（根为1）；正式解析额外最多
2048 token、64 层语法嵌套。请求最多 256 KiB；单源 4 MiB、活动源码总计32 MiB、
最多4096文档；总输出8 MiB。先预算后递归/复制，边界值允许。

候选查询 `condition_candidates_from_compile(compiled, query)` 按 ID 稳定排序，query
携带 kind(variable/state/tag)、text、offset、limit(1–100)；完整匹配数和 has_more 明确。
全局类型只返回可确定 num/str/bool；state/tag 必须正式声明且无声明歧义。UI按需分页，
不每帧重新编译、不把首100项冒充全部。

严格 JSON parser 拒绝重复键、未知字段/枚举、未知 schema_version、缺失 owner/slot、
不完整 TargetRef、非法预算。协议形状错误为 JSON-RPC INVALID_PARAMS；过期、来源、
不支持、只读、冲突等领域拒绝为 ok:false 与中文 message。CLI/RPC 共用 core DTO、
解析函数、投影和计划，不复制条件逻辑。
