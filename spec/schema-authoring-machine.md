# 工具 0.35：资料约束填写机器接口

唯一对象投影、字段与绑定操作、来源完整性和候选事务来自
[schema 作者契约](schema-authoring.md)。协议仍为 1，语言默认 1.9、最高显式
1.13 不变，不新增作品能力或改变旧 schema 源码编辑入口。

协商能力为 `authoring.schema_authoring.v1`。

## 查询、预览与应用

- RPC `schema.authoring.query {project_id,target}` 查询已打开 Project 当前已应用快照
- RPC `schema.authoring.preview {project_id,request}` 返回同一 core 完整预览
- RPC `schema.authoring.apply {project_id,request,plan_digest}` 重建并校验计划后整笔修改内存
- CLI `wl schema-authoring query PROJECT --target-json JSON [--json]`
- CLI `wl schema-authoring preview PROJECT --request-json JSON [--json]`
- CLI `wl schema-authoring apply PROJECT --request-json JSON --plan-digest DIGEST [--save] [--json]`

RPC 不接受临时 `path`，不隐式刷新、恢复事务或保存磁盘；使用既有 `project.save`
显式保存。CLI 只读打开磁盘作品，不先恢复未决保存事务；apply 默认只修改本进程
内存，退出即丢弃，响应明确提示。只有 `--save` 通过 Project 既有保存事务持久化。
query/preview 不接受摘要或 `--save`。该加法入口不改变旧 schema-apply 的保存行为。

target 是完整 `{kind,id}`，只允许 world/character/entity/relation，不能靠显示名或
仅 id 替代。request 直接使用 core `SchemaAuthoringRequest`：schema_version、
expected_baseline、target、binding、fields。绑定和字段的显式意图及原始值格式见
core 契约；0、false、空字符串仍是实际有值，缺失不是默认值。

query 成功返回 `{ok:true,projection,baseline,applied:false,saved:false,error:null}`。
preview/apply 成功返回 `{ok:true,operation,plan,baseline,applied,saved,error:null}`。
`ok:true` 仅表示查询或事务成功，不表示没有实例约束错误；须检查 projection 或 plan
的诊断与完整性。plan 的 before_fingerprint / after_fingerprint 是规范十进制字符串，
runtime_changed 来自 core 实际比较；schema/绑定本身不改指纹，world/character 标量
属性沿用已有 hash 规则。消费者不得按对象种类猜测，也不得把旧存档失配称为已迁移。apply 的 `applied` 仅在确有变更时为 true；重复或过期请求不得
再次写入，任何未应用草稿也不会被本接口自动纳入。

## 严格输入、额度和失败

所有新入口拒绝未知参数、未知嵌套字段、重复 JSON key、重复 CLI 参数和非法形状。
请求 DTO 最多 256 KiB；RPC 规范化参数连同 project_id 等外壳最多 256 KiB + 8192。
core 投影/预览最多 8 MiB；完整成功响应（包括 RPC id、外壳与换行）最多 8 MiB + 4096。
接口层先流式计量完整成功响应，通过后才允许任何内存应用或保存。
RPC id 最多 3072 个实际 JSON 编码字节；超限返回 id:null 的 -32600，不回显超额 id。
无 id 通知执行同一检查但不输出响应。

结构/类型/未知字段错误为 RPC -32602、CLI 退出 2；全局原始 JSON 重复键/解析失败
继续为 RPC -32700。合法请求的不可用目标、源码不完整、只读、外改、过期基线或计划、
候选数量及 core 投影/计划构建预算超限返回
`ok:false,error:{code:"SCHEMA_AUTHORING_REJECTED",message}`，CLI 退出 1。
core 已返回完整结果、但接口层完整响应外壳超过额度时为 `OUTPUT_LIMIT`、CLI 退出 1。
两层都不伪造空字段或截断完整计划来授权写入；机器层不解析 core 的中文错误消息。
CLI 工程读取失败返回 `IO_ERROR`、退出 2。

保存失败返回 `SAVE_FAILED`、`stage:"save"`、`applied:true`、`saved:false`，保留真实
内存状态和可恢复保存事务；不声称磁盘零修改或已回滚。查询、预览和拒绝写入均明确
applied:false/saved:false。成功退出 0。错误消息为中文，code 为稳定英文标识。
