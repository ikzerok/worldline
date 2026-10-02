# 安全源码组织

源码新建、引用与路径移动使用同一份 core 计划，CLI 和 JSON-RPC 不另建路径解析器。
默认语言仍为 1.9，最高显式语言仍为 1.13；本操作不升级作品语言或自动启用资料能力。
完整契约见 [source-lifecycle.md](../spec/source-lifecycle.md)。

## 先确认操作

- create：在工作区内新建不存在的 `.wl`，同时添加入口 include。显式 source_sets
  工程在同一事务中把新文件加入 active，不覆盖已有文件或删除墓碑
- include：引用已经存在且活动的 `.wl`。归档或未活动文件会被明确拒绝，需要作者先
  另行授权启用；不能引用入口自身
- move：移动一个已跟踪、非入口的 `.wl` 到不存在的新相对路径。保持 active、archived
  或未活动身份，支持中文、空格和多级目录；不支持目录批量移动、入口迁移或附件移动

新建与引用不要求先修完无关的正文草稿。移动要求当前和候选活动源码均编译通过，
并证明对象身份与顺序、正式引用、默认入口、加载顺序、附件解析及运行指纹等价。
例如缺少显式 include 的文件移动改变默认加载顺序，或 file 所属 state 的运行身份
随路径改变，都会拒绝。旧 Story 存档、检查点和 trace 不在此功能中迁移。

## CLI：预览、核对、应用

例如新建一章：

```powershell
wl source-lifecycle preview "D:/作品/我的世界" --request-json '{"operation":"create","path":"章节/新 章.wl"}' --json
```

核对返回 plan 的 changes / occurrences，并复制 plan.plan_digest。保持 request 完全
相同，用实际摘要替换下面的占位内容：

```powershell
wl source-lifecycle apply "D:/作品/我的世界" --request-json '{"operation":"create","path":"章节/新 章.wl"}' --plan-digest '<预览返回的摘要>' --json
```

其他操作仅替换 request，仍需各自先预览：

```json
{"operation":"include","path":"人物/已有.wl"}
{"operation":"move","from":"旧章.wl","to":"章节/新章.wl"}
```

preview 返回 `applied:false,saved:false`，不应用计划。apply 重新检查完整请求、内容
基线、保存基线、目标缺失及资源指纹，通过后应用并保存；成功返回
`applied:true,saved:true`。两者的 plan 都是 core 的完整逐处计划。
请求过期时重新预览，不能靠替换摘要绕过变化。用法失败退出码 2，业务或保存失败
退出码 1，成功为 0；不加 `--json` 时显示中文摘要。

## JSON-RPC

在同一 `wl-agent` 进程中使用以下方法；`path` 可替换为 `project.open` 返回的
`project_id`，两者只能提供一个：

```json
{"jsonrpc":"2.0","id":1,"method":"project.source_lifecycle_preview","params":{"path":"D:/作品/我的世界","request":{"operation":"move","from":"旧章.wl","to":"章节/新章.wl"}}}
{"jsonrpc":"2.0","id":2,"method":"project.source_lifecycle_apply","params":{"path":"D:/作品/我的世界","request":{"operation":"move","from":"旧章.wl","to":"章节/新章.wl"},"plan_digest":"<预览返回的摘要>"}}
```

不接受未知参数、未知 request 字段或重复 JSON key。参数类型与形状错误走 JSON-RPC
error；合法请求因工作区状态被拒绝时，result 为 `ok:false`，错误码为
`SOURCE_LIFECYCLE_REJECTED`。完整结果字段见 [机器接口契约](../spec/agent-protocol.md)。

## 逐处审阅和保存失败

计划列出每个变更文件、移动后的路径、逐处原始 UTF-8 字节范围、行号、正式字段、
前后 token 与上下文，并提供资源解析证据。只修改正式支持的路径字段；普通正文、
注释、标签、未知可选字段与其他未改动字节保持原样。不能勾掉部分引用后继续移动。
链接、越界、目标碰撞、缺资源、未知必需能力、无效登记文档和无法证明的提案迁移
都会整体拒绝。

若返回 `error.stage:"save"`，说明内存操作已经应用，但保存尚未确认完成。
`applied:true,saved:false` 不表示磁盘零修改；某些文件可能已替换。保留当前缓冲和
`.world/.transactions/` 恢复日志，重新打开或刷新工程，让既有保存恢复流程继续。
RPC 的 project_id 会话也保留应用后的稿件。不要删除日志、盲目重发旧计划或将旧
快照强行覆盖回磁盘；存在恢复冲突时按[工作区恢复契约](../spec/workspace.md)处理。
跨文件保存采用可恢复 journal，不宣称物理原子写入。
