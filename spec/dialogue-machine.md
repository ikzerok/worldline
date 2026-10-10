# 0.34 对白与私密制作台本机器入口

语言与内容唯一遵循 [dialogue-authoring.md](dialogue-authoring.md) 和
[production-script.md](production-script.md)。协议仍为 1；不远控编辑器、不隐式刷新、
不自动保存、没有网络发送。initialize 新增 `authoring.dialogue.v1` 与
`authoring.production_script.v1`。

## 正式对白

```text
wl dialogue query PROJECT --target-json TARGET [--json]
wl dialogue preview PROJECT --request-json REQUEST [--json]
wl dialogue apply PROJECT --request-json REQUEST --plan-digest DIGEST [--save] [--json]
```

TARGET 是严格的 `{kind,id}`，上限 4 KiB；REQUEST 为 core `DialogueEditRequest`，
上限 64 KiB。query 不接受 request/digest/save；preview 不接受 target/digest/save；
apply 不接受 target，必须 digest。未知、重复、空值及多余位置参数拒绝。

每次命令通过 `Project::open_read_only` 打开，绝不恢复待处理磁盘事务。query 或编辑
请求使用当前 Project 正式目标新开唯一 WritingBuffer，generation 为 0；因此相同
Project 的来源身份与计划稳定。机器不保存单独键入状态，不接受任意 offset，也不
读取另一个编辑器进程的未应用输入。另一个进程的修改必须重新打开并重新查询。

RPC：

| 方法 | 严格参数 |
|---|---|
| `dialogue.query` | `{project_id,target}` |
| `dialogue.edit.preview` | `{project_id,request}` |
| `dialogue.edit.apply` | `{project_id,request,plan_digest}` |

RPC 从已打开 Project 的当前已应用内存稿新建临时 WritingBuffer，generation 同为 0。
core 再次验证来源、观察与保存基线；不会为了满足 request 自动刷新或降低守卫。

query 返回 `{ok,projection,baseline,applied:false,saved:false,error:null}`。
preview 返回 `{ok,operation:"preview",plan,baseline,applied:false,saved:false,error:null}`；
计划 can_apply=false 仍是可读预览，不表示已提交。apply 先重建同一 core 计划并核验
digest，再调用 core 原子 apply。返回 plan、最新 baseline 与 applied/saved；no_change
返回 applied=false，不提交其他内容。启用 1.11 与完整目标稿/台词处于同一 core 事务。
Say→Text 转换的 direction 损失只能经明确请求中的 allow_direction_loss=true 确认；
更改选项后必须取得新计划摘要，旧 digest 不授权新决定。显式 update 删除备注或
delete 删除整句按各自操作的损失预览确认，不接受 convert 专属字段。

CLI apply 默认仅改短命内存并明确“进程退出将丢弃候选”；只有显式 --save 才另行
调用 Project 保存。保存失败准确返回 `ok:false,applied:true,saved:false,stage:"save"`，
error.code=SAVE_FAILED，不声称磁盘零修改。RPC apply 永远 saved=false，随后使用
既有 `project.save {project_id,expected_baseline}`。未执行、旧计划、非法 DTO 和失败
不能冒充 applied。成功 no-op 即使请求 --save 也不写文件，saved=false。

## 同稿制作台本

```text
wl production-script query PROJECT --request-json REQUEST [--drafts-json DRAFTS] [--offset N] [--limit N] [--json]
wl production-script export PROJECT --request-json REQUEST --options-json OPTIONS [--drafts-json DRAFTS] [--output ABSOLUTE_NEW_FILE] [--json]
```

REQUEST/OPTIONS 是 core 的 `ProductionScriptRequest` / `ProductionExportOptions`，
DRAFTS 为既有严格 `ManuscriptQueryDraft[]`。它们只读构造候选，不应用或保存编排。
本机器接口不接收 GUI WritingBuffer；同一有状态 RPC Project 可先显式应用对白/源码
再查询台本。query 默认 offset=0、limit=50，limit 1–100；完整过滤后分页。
export 禁止 offset/limit；query 禁止 options/output。CLI 子命令全部参数的原始
UTF-8 字节长度合计上限 4 MiB（包括操作、路径、选项名与 JSON 文本）。

RPC `production.script.query {project_id,request,drafts?,offset?,limit?}` 与
`production.script.export {project_id,request,drafts?,options}` 返回同一 core 语义。
未知参数、重复 JSON 键以及非整数 offset/limit 不可默默修正。分页/导出使用
request.expected_snapshot_key 明确绑定先前快照；过期必须失败，不能从新稿继续
旧材料。RPC 不缓存无限历史快照；每次重建并核对身份。所有分页和导出均完整验证，
不以已经看到的一页当作完整角色范围。

query 返回 `{ok,page,baseline,applied:false,saved:false,delivered:false,error:null}`。
export 返回 `{ok,artifact:{format,snapshot_key,text,byte_count},baseline,
applied:false,saved:false,delivered,error:null}`。text 是完整输出 UTF-8 字符串，
byte_count 为其 UTF-8 字节数；JSON 包装反转义后与 native/WASM 文件字节完全一致。
JSON、Markdown、CSV 都由 core immutable snapshot 产生；机器不自行重排/格式化材料。

默认 direction 在产物所有字段中不存在；显式 include_direction 才纳入。CSV 显示
前缀会改变交换字节，精确原值使用 JSON；不作所有表格软件安全的保证。不存在给
第三方发送的步骤。RPC 只返回材料，delivered=false；CLI 只有显式 --output 才将
同一完整字节写入工作区外全新文件，不覆盖。输出前与原子发布前重验快照，失败清理
暂存。JSON 包装响应须在创建文件前通过预算验证，超限不留下交付成功文件。

## 错误、预算与兼容

CLI 成功退出 0、合法请求的业务失败 1、用法/读取失败 2；有无 --json 均给完整
结构化结果，--help 为中文说明。JSON 重复键、未知字段、类型错误为用法失败。
RPC 消息重复键为 -32700，未知 project_id/字段/类型为 -32602，无效 jsonrpc 为
-32600；合法请求的旧身份、编译/locale/能力/范围/预算失败为 `{ok:false,error}`。
RPC 在解析消息后，按 `params` 紧凑 JSON 序列化的 UTF-8 字节数计预算：对白
为 64 KiB + 8192 B，台本为 4 MiB；其中 target / request 仍分别受对应 core
解析器预算约束。参数及字段长度超限属于参数边界（-32602）；core 的工程库存、
完整范围和产物预算属于业务边界。id 预算属于 envelope 边界（-32600）。
这些是解析后的参数预算，不是 stdio 原始消息行或 JSON 解析前的内存上限；原始
空白、外层字段不计入 params 预算。本版本不改变既有 agent 的逐行读取契约，
客户端不能据此假设任意大的原始输入会在解析前被拒绝。
错误不携带作者源全文或假成功产物。

对白与台本专用完整响应上限均为 64 MiB（含 JSON 包装），不走截断成功路径。
RPC id 序列化上限 3072 B。适配器在 apply 或文件交付前核验可返回完整结果；超限
明确 BUDGET_EXCEEDED，保留原 Project 和磁盘。业务响应保留准确 applied/saved/
delivered 状态，不用终端输出失败掩盖已发生的保存或交付。

既有 manuscript.review/manuscript.delivery、localization catalog/edit、source.edit、
runtime locale 和 reader profile 保持原契约。本功能既不是替代全分支审稿，也不是
翻译回导、完整备份、录音管理或可执行公开发布。
