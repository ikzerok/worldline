# 可信全分支作者审稿（0.22）

这是只读的作者材料，不是某次运行、可达性证明或读者发布授权。旧 `ReadingProjection.lines` 与 reader-export 的独立权限/DTO 不变；默认语言仍为 1.9、最高显式 1.13，无新 DSL，不改变 Program 语义指纹、Save 或 Trace。

## Rust 接口

`manuscript::review_projection(&CompileResult, &TargetRef) -> Result<ReviewProjection, ReviewError>` 支持 event、scene、fragment、entity。`Project::compile_writing_drafts` 的当前未应用缓冲编译结果可直接输入。编译有错误、目标缺失、来源不确定或超预算均整体失败，不返回貌似完整的残稿。

ReviewProjection：schema_version=1、target、snapshot（所有参与编译的完整源文件路径/原文字节及 CompileOptions 的版本标记）、complete=true、node_count、nodes。内部另保留完整源快照与编译选项用于精确比较，标记不是安全签名，也不使用运行指纹。

ReviewNode：kind（text/say/if/branch/choice_group/choice/scene/call/return/divert/structure/description）、label、parts、children、source、speaker、condition、enable、once、disabled_reason、target、glue、end_label。parts 为 `{text,target,dynamic}`；动态表达式明确标记而不求值；链接保留显示文字及完整 TargetRef；speaker 为 `{display,target}`，同快照目录提供显示名，稳定人物身份始终保留。

if 节点的直接 children 只包含 branch（依次 if/else if/else）；各分支 children 为本分支正文。正式 parser 记录每个分支头真实文件和位置（含空分支），不按首正文或上一行猜测。当前语言不允许空 if/else 块时，保留原有诊断并整体拒绝。相邻 choice 依正式 AST 序列组成 choice_group；空 choice 保留。choice 的 if 是可见条件，enable 是可选条件，once 是一次性约束，disabled_reason 是原文说明；均不假定实际真假。

scene 保留边界；call 只显示目标及参数原文，不展开；return、divert、状态/变量/效果等保留结构说明及原文。glue 保留原始粘接标记，不跨分支拼接。if/choice_group/scene 的 end_label 明示组块结束与“仅控制流继续时汇合”，不宣称跳转/返回分支必定继续。事件已被 AST 提取的 effect 仍按真实来源顺序显示。

## 来源与导航

ReviewSource：target（本次审稿根目标）、file、line（1-based）、column（1-based Unicode scalar）、byte_start/byte_end（完整文件 UTF-8 半开区间）、excerpt（该语句/声明原文）。声明级来源不声称定位到描述中的具体字。合成 choice_group 无来源，其选择头各有来源。原文来自正式 lexer/provenance 的物理跨度，不能重新猜测 DSL。

`validate_review_source(&CompileResult, &ReviewProjection, &ReviewSource) -> Result<(), ReviewError>`：当前结果无错误、完整源集合及逐字原文/CompileOptions 一致、位置确实属于该投影且原文片段一致才通过。注释移行、其他文件人物更名、当前未应用输入改变、坏稿均拒绝旧导航。UI 必须先重新编译所有当前 WritingBuffer，再校验；失败保持输入并禁用旧来源跳转。API 只验证，不应用、不保存、不自动恢复。

## 完整性与预算

固定上限：参与编译源码合计 16 MiB、单投影 10,000 节点、最大树深度 64、序列化 DTO 1 MiB（包含 JSON 转义后实际 UTF-8 字节）。构造前检查输入，构造中检查节点/深度/累计文字预算，最终有界流式计量 JSON；超限整体返回 `review_limit`，无截断成功、无假全稿。调用方可按已编排章节分别请求，整书必须逐章明确显示失败章节，不跳过。

## CLI / RPC

`wl manuscript-review <目录或入口> --target-json '{"kind":"event","id":"start"}' [--json]` 使用 `Project::open_read_only` 和 `compile_read_only`，不恢复事务、不保存；输出 `{ok,review,error}`，review 为同一 core DTO。成功退出 0，故事/审稿失败 1，参数/IO 失败 2。人类模式也输出明确标为静态全分支的 JSON，避免再造含义不同的排版协议。

RPC `manuscript.review`：params 为 `{project_id,target}`，对已打开 Project 使用 `compile_read_only`，返回同一 `{ok,review,error}`。合法请求的编译/目标/预算失败为 ok:false；协议参数错误仍用 JSON-RPC error。响应业务 DTO 最大 1 MiB，外壳另留 4 KiB，超长请求标识不得突破响应上限。

ReviewError 为 `{code,message}`，中文 message。失败没有可导航投影；成功完整性只涵盖所选目标的静态原文，不包含被调用片段展开、全局资料或实际运行结果。
