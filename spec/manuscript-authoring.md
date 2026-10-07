# 新章与正式正文来源（工具 0.30）

语言仍默认 1.9、最高 1.13；不新增 DSL、书稿格式版本或持久能力标识。
仅显式加入既有 `presentation.manuscripts.v1` 能力。空白作品仍只有
`event start` / `-> END`。只有作者明确创建时才增加章、清单和必要源码。
旧 `ManuscriptCommand` 及 `preview_manuscript` / `apply_manuscript` 仍只改编排，
不创建源码、不改变运行指纹。本契约是独立的显式组合事务。

## 请求与身份

`ManuscriptChapterCreateRequest` 的 `schema_version` 为 1，含
`expected_baseline`、`expected_revision`（既有 Revision 三个 generation 字段）、
`book`、`chapter`、`source`。所有 DTO 与嵌套对象拒绝未知字段；JSON 重复键拒绝。

- book：`{kind:"existing",id}` 或 `{kind:"new",id,title}`
- chapter：`{id,title,parent_section_id?,after_sibling_id?}`；null/省略表示无
- source：`{kind:"existing",target:{kind,id}}` 或
  `{kind:"new_event",id,destination,storyline}`
- destination：`{kind:"existing_active_source",relative_path}` 或
  `{kind:"new_active_source",relative_path}`

书稿/章节 ID 沿用书稿规则；event 与 storyline ID 沿用正式作者 writer 的规则。
章与 event 使用独立稳定 ID，不由标题猜身份，apply 不自动追加后缀。
parent 必须是已有 section；after 必须是同父兄弟；无 after 追加到该父末尾。
现存来源只接受能解析的 event/scene/entity/fragment，仍受既有语言门控。
新来源仅支持 1.9 已有 event，正文仅为 `-> END`，没有样例句、人物、时间关系、
自动前驱或自动剧情连线。首章可明确复用 `event:start`，不默选任何候选。

## 预览、事务与结果

Rust `preview_manuscript_chapter_create(revision, &request)` 与
`apply_manuscript_chapter_create(&mut revision, &request, plan_digest)` 共用 core
准备路径。预览零缓冲/磁盘写；apply 重新生成计划，核对完整摘要和最后保存基线，
只一次替换外部 Project，不保存。调用方只压一个撤销快照。

计划含 schema_version、workspace（根目录身份）、baseline、revision、book_id、
chapter_id、target、source_path、new_source、manuscript_path、changed_files、changes、
diagnostics、entry_before/after、runtime_fingerprint_before/after、can_apply 和
plan_digest。路径均为相对工作区路径（workspace 除外）；changes 给实际 before/after
UTF-8 原文，摘要绑定完整候选字节和请求、全库存、保存基线、刷新代次，不能用截断
摘录或客户端修改的范围授权。入口必须保持。纯复用来源必须保持运行指纹；新增事件
显示真实指纹变化，既有 save/checkpoint 不直接兼容，需按当前作品重新开始。
工具升至 0.30 后，0.29 的 ReplayTrace/ReplayCheckpoint 仍按既有 runtime_version
严格相等规则拒绝，即使没有新建 event、源码指纹保持也不例外；同 runtime 的入口
trace 才可对新指纹受控重放。普通 Story Save 不额外检查工具版本，沿用语言能力、
运行指纹和状态完整性规则。本功能不迁移或放宽任何运行存档守卫。

结果含 plan、book_id、chapter_id、target、source_path、changed_files、new_baseline、
new_revision。新 event 同时增加 content_generation 和 presentation_generation；
仅编排分支只增加 presentation_generation。workspace_generation 不变。

全工程库存/缓冲受 4096 项、源码和注册文档各 64 MiB、保存/库存校验总计 256 MiB
的现有生命周期预算保护；请求 JSON 上限 64 KiB，ID/标题/路径字符串分别上限
256/4096/1024 UTF-8 字节，预览完整变更原文合计上限 8 MiB。超限明确拒绝。

应用检查工作区可写、恢复事务、所有跟踪文件保存基线、完整普通文件库存、活动
集合及新增路径缺失。新 .wl 复用 source lifecycle 路径、大小写别名、成员与 include
规则；归档/非活动/墓碑不能暗启用，新目标不能接管普通文件或注册展示路径。
书稿按原 JSON 合并，保留未知顶层/节点/引用字段；不保证 JSON 空白或键顺序。
失败全部保留原工程与输入，不留下章、source、include 或能力声明。

稳定失败 code 为 STALE_BASELINE、ID_CONFLICT、INVALID_DESTINATION、INVALID_CHAPTER、
READ_ONLY、SOURCE_UNAVAILABLE、EXTERNAL_CONFLICT、BUDGET_EXCEEDED；message 中文。
同一固定请求成功后重试必须过期/冲突，不能创建第二个对象。预览摘要不是安全签名。
UI 的未应用稿不属于 Project：创建前保守阻止未提交作者表单/同书稿编排/目标文件
WritingBuffer，保留输入并返回处理。能力或基线改变后只重新预览，不能自动提交。

## 安全空正文插入点

`WritingProjection.empty_prose_slot: Option<WritingProseInsertion>` 为独立投影，
不是伪造的 WritingBlock。仅当正式 AST 和 lexer 证明目标 event 执行体只有终止
`-> END`（注释/空白除外）时提供，复杂 event、scene/entity/fragment 不猜位置。
位置由正式 token 行及实际字节定位，位于终止语句行前；继承该行缩进和局部换行，
事件外字节、独立注释、其他声明和 EOF 无末尾换行保持原样。

`Project::insert_writing_prose(&mut buffer, &slot, text)` 重验完整 TargetRef、文件、
Project 基线/语言、generation、精确完整源文签名和当前正式空槽。槽字段不可由调用方
构造/修改；接口不接受任意 offset。外变、只读、未知能力、墓碑/非活动、陈旧和
无法证明结构均拒绝。空字符串为 no-op；真正输入才写唯一 WritingBuffer，并增加
代次；不应用、不保存。槽的只读 offset()/text() 提供下一正文块的稳定字符起点及
当前真实的语义空白文本，纯空格/换行输入和清空仍复用原槽范围；没有输入时不预写
任何字节。替换为空只在槽原值也为空时为 no-op。新输入沿既有 replace_prose 的源文语义，不转义/清洗作者文本，
无效草稿留在缓冲并可切源码修复。第一段变普通 Prose；core 将连续同缩进前导空行并入紧随的正文块，保持输入控件起点，
不吸收注释或结构。清空后可重新投影空槽。

## 机器接口

能力 `authoring.manuscript_chapter.v1`，协议仍为 1。
RPC `manuscript.chapter.preview {project_id,request}` 与
`manuscript.chapter.apply {project_id,request,plan_digest}` 仅用已打开工程，不刷新。
正常返回 `{ok,operation,plan,result?,applied,saved:false}`；结果直接序列化 core。
Project 会话的 revision 与预览/应用响应一起公开；初始为全零，变化后用返回值。
业务失败为 `{ok:false,error:{code,message},applied:false,saved:false}`；类型/未知字段/
未知 project_id 为 -32602，重复键沿消息解析规则 -32700。应用之后由
`project.save {project_id,expected_baseline}` 显式保存。

CLI `wl manuscript-chapter preview|apply PROJECT --request-json DTO
[--plan-digest DIGEST] [--save] [--json]` 只读打开工程（不恢复事务），初始 Revision 全零。
preview 禁止 digest/--save；apply 必须 digest。默认应用只改短命内存，明确
`saved:false` 和退出丢弃提示，显式 --save 才沿既有 journal 保存。成功退出 0、业务
失败 1、用法/读取失败 2；--json 失败仍结构化输出。保存失败返回 applied:true、
saved:false、SAVE_FAILED，保留候选与恢复事务，不声称磁盘零修改。

空槽仅为 Rust/UI 正文投影；机器后续输入使用既有 source.edit，不新增远程键入协议。
