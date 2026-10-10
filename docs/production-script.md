# 同一当前稿的角色制作台本

角色台本把正式 `say`、来源与所选 locale 放在同一份静态材料中，适合核对、录制和校对准备。
它不会运行故事、选择路线、求值参数，也不会复制一套独立台词库。详细契约见
[production-script.md](../spec/production-script.md)。

## 先确定这次交付什么

1. 选择当前 event / scene / fragment / entity、一本书稿查询中的所选章，或全工程活动定义
2. 用正式 `character` 的完整 ID 选择说话者；同名人物仍是不同角色
3. 确认是否纳入静态调用闭包、是否另纳入旁白 / 选项文案
4. 选择源文或一个已注册 locale，核对缺译、过期与无效状态
5. 查看范围计数、新增片段定义、调用使用关系及来源，再选择格式和导出选项

正文工作台可使用当前每文件唯一的 WritingBuffer。未应用草稿仍是当前稿，只读查询不会自动应用或保存。
无效 / 过期输入、真实外改或不完整来源会阻止生成，不能用上一份结果冒充当前稿。
未提交的角色表单、台词保留输入或其他尚未进入 WritingBuffer 的输入不属于已生成台本。

“说话者”来自正式 `say character_id`。章 POV、事件 with、文字里提到的人物、普通“姓名：”前缀均不能替代它。
勾选旁白或选项文案会加入独立 kind；不会因此加入其他角色的 say。
entity 描述不是 Text/Say/Choice 单元，单独选择它可能得到明确的零结果。

### 定义、根章出现和调用点分别计数

默认纳入静态调用闭包：每个被调用片段定义只列一次，多次调用、两章共用一个事件或菱形调用不会复制录制台词。
事件内部的场景声明仍保留正式归属。根定义用 is_root 标识，额外片段数另列。

- selected_chapter_occurrences：这次所选的书稿章出现，重复来源的两章仍是两章
- root_targets：所选章 / 当前目标去重后的根来源数
- definition_count：纳入的唯一来源声明，包括内部场景与共享片段
- added_fragment_definitions：通过闭包额外纳入的片段定义数
- call_sites：所选来源里的静态 call 语句数，不是运行调用次数
- matching_rows：全部过滤完成后的精确行数，之后才分页

关闭闭包后，当前目标 / 选章材料标为“直接范围，被调用片段未纳入”，不能称为全角色台本。
全工程范围本来就包括全部活动事件和片段，关闭额外闭包不会移除这些活动片段。
归档或未激活 `.wl` 不会被偷偷编译进来。

内部条件、选择、场景上下文与外部调用使用关系分别呈现。所有条件均未求值，
动态 placeholder 显示为未求值标记；不提供猜测读音、实际参数值、执行顺序或 shared-once 运行次数。
默认不夹带其他角色的相邻对白。来源行锚只对生成时的快照有效，不能当成长期稳定 ID；
没有显式 localization ID 的行会标明“无持久行 ID”。

## Locale：严格检查这次实际选中的单元

源文模式不要求每行已有稳定 ID。选择 locale 后，行状态为 translated、missing、stale、invalid；
无稳定 ID 或无译文均为 missing。改动说话者 / 名称 / direction 会使旧台本快照失效，
但这些元数据并不自动改变既有翻译 source_revision。Text ↔ Say 改变 kind，会按既有规则使旧译文过期。

默认 strict 可在查询中查看各种状态，但导出要求最终过滤选中的每一行都是 translated。
另一章或另一个角色的缺译不会阻止这次已完整翻译的选定角色。
未知 locale、损坏的注册 / sidecar、未知必需能力、全工程重复稳定 ID 等完整性问题仍会在过滤前阻断。

只有明确选择 source_fallback 才能用源文替代缺失、过期或无效译文。
交付头说明回退策略与数量，每行保留原状态并标记 used_source_fallback；不会把回退文字认证为新译文。
正式角色显示名与 direction 始终是源语言作者元数据。

## 三种格式来自同一份结果

- JSON：所选生产字段的精确值，适合机器使用；不包含整份内部来源结构
- Markdown：经过 HTML / Markdown / 链接 / 控制字符转义的人读审阅材料
- CSV：表格阅读格式。每个单元格（包括表头、空值及数字显示）加一个单引号前缀，再双引号包裹；
  内部双引号加倍，UTF-8，记录分隔为 CRLF。材料含范围元数据、定义、章出现、调用点和 row 记录

CSV 前缀属于输出字节，不改变原稿。CSV 不是无损回导格式，精确原值请使用 JSON。
它不是对所有表格软件及再次另存后的通用安全保证，也不声称未经实测的 Excel / LibreOffice 行为。

演出备注 direction 默认排除。此排除同时覆盖 raw say 行、excerpt、隐藏 JSON 字段和 CSV 附列等旁路，
不是仅在界面上隐藏一个字段。只有明确 include_direction=true 才加入所选行备注。
其他 locale、源文件全文、绝对本机路径、附件、私密资料和无关角色对白不会随材料导出。
该材料仍是作者私密材料；导出不会自动发送给第三方、建立公开网页或扩大 reader profile 权限。

原生文件导出只创建工作区外的新文件，不覆盖已有目标。
目标父目录必须已经存在且没有链接 / 联接；发布前重新检查当前稿和外部观察。
取消、过期或失败清理暂存，不留下冒似成功的成品。浏览器只请求下载相同字节，不证明已存入磁盘。

## CLI

CLI 查询读取它自己打开的 Project；不会遥控一个已经打开的 GUI。独立进程也不会接管 GUI 未应用草稿。
以下 `doctor` / `start` / `en` 只是参数示例，应替换为工程内的正式身份。

查询当前事件及其静态片段闭包，按正式角色过滤，返回前 50 行：

```sh
wl production-script query /path/to/project \
  --request-json '{"schema_version":1,"scope":{"kind":"current_target","target":{"kind":"event","id":"start"}},"speaker":{"kind":"character","id":"doctor"}}' \
  --offset 0 --limit 50 --json
```

查询返回 page.snapshot_key。后续请求可把它放进 request.expected_snapshot_key，
保持完全相同的范围和过滤，再改 CLI 的 --offset / --limit；当前稿变化会拒绝旧快照。
每页为 1–100 行，total 是完整过滤结果数，不是当前页数。

导出严格 en locale 的新 JSON 文件：

```sh
wl production-script export /path/to/project \
  --request-json '{"schema_version":1,"scope":{"kind":"current_target","target":{"kind":"event","id":"start"}},"speaker":{"kind":"character","id":"doctor"},"target_locale":"en","locale_policy":"strict"}' \
  --options-json '{"schema_version":1,"format":"json","include_direction":false}' \
  --output /existing/output/doctor-en.json --json
```

省略 --output 时只返回 artifact.text、format、snapshot_key 与 byte_count，不写文件。
选择 markdown / csv 时，原生输出扩展名须对应 `.md` / `.csv`。
确需源文回退时将 locale_policy 明确改为 source_fallback，并核对回退行数。

选定一本书稿查询中的两章：

```json
{
  "schema_version": 1,
  "scope": {
    "kind": "manuscript",
    "query": {"schema_version": 1, "manuscript_id": "book", "limit": 100},
    "chapter_ids": ["opening", "arrival"]
  },
  "include_fragments": true,
  "speaker": {"kind": "character", "id": "doctor"}
}
```

该范围复用完整书稿查询，而不是当前可见页 / 折叠树。chapter_ids 省略时选择全部匹配章，
空数组是明确的零选择；重复、歧义、不存在或被查询过滤掉的选择会拒绝，不扩张范围。
CLI 的 --drafts-json 只接受既有 ManuscriptQueryDraft 编排草稿，不是任意写入源码的通道。

## JSON-RPC

在自己的 project_id 会话中，production.script.query 使用同一 request DTO：

```json
{
  "jsonrpc": "2.0",
  "id": 10,
  "method": "production.script.query",
  "params": {
    "project_id": "p1",
    "request": {"schema_version": 1, "scope": {"kind": "project"}, "speaker": {"kind": "character", "id": "doctor"}},
    "offset": 0,
    "limit": 50
  }
}
```

production.script.export 保留同一 request 的完整范围、角色和筛选，并加入 options；
不接受文件路径、不发送外部消息。承接上述查询时，将 page.snapshot_key 的实际返回值
填入 expected_snapshot_key；下例的占位字符串必须替换，不能原样使用。若稿件或条件
变化，旧身份会拒绝交付，应重新查询并核对材料，而不是去掉范围或身份继续导出。

```json
{
  "jsonrpc": "2.0",
  "id": 11,
  "method": "production.script.export",
  "params": {
    "project_id": "p1",
    "request": {"schema_version": 1, "scope": {"kind": "project"}, "speaker": {"kind": "character", "id": "doctor"}, "expected_snapshot_key": "替换为查询返回的 page.snapshot_key"},
    "options": {"schema_version": 1, "format": "markdown", "include_direction": false}
  }
}
```

返回 artifact.text 经 JSON 解码后的 UTF-8 内容，就是 core 生成的材料字节。
RPC 对自己 Project 的内存编辑与磁盘保存分开，query / export 均不自动应用或保存。
协议对象未知字段、重复键、错误类型与业务拒绝分开报告；机器入口不绕过草稿 / 预算 / locale 保护。

## 明确的规模边界

默认上限也是硬上限：4096 章、4096 定义、20000 调用点、50000 源单元、4096 来源文件、
16 MiB 活动源码、16 MiB 完整结果、32 MiB 单份导出字节；请求最多 4 MiB。
完整编译后的 AST 还有 200000 节点（含分支头）和 64 层嵌套上限。
范围身份、角色名称、控制 / 调用关系与来源元数据同样计量。

此外，选择范围前仍核对全工程本地化完整性：最多 50000 个源单元，每单元最多
4096 个 parts / 64 KiB，清单与每份 sidecar 最多 8 MiB，来源记录与目录元数据
各最多 64 MiB。已加载 Project 的活动源码另经 200000 物理行门；未应用
WritingBuffer 叠加后的当前编译稿仍受 16 MiB、AST 及源单元门约束，该物理行
检查不代表对叠加稿再检查一次。全部跟踪文档（含 inactive、墓碑）
最多 4096 份，current/saved/path 合计最多 256 MiB，工作区诊断最多 200 条。
书稿范围另有 4 MiB 身份 / 路径元数据上限。完整约束见
[台本规范](../spec/production-script.md)及[本地化预算](../spec/localization.md)。

超限会明确失败，没有“成功但截断”的台本。所选结果超限时，可明确缩小章节或角色
范围后重新生成；若失败来自全局源码、AST、来源或注册资料，筛选不能绕过它，须先
处理相应问题。分页只是浏览方式，这些数字也不是保证同时可达到的作品容量。
