# 本地化字符串交换

**版本：**1（`content.localization.v1`）  
**状态：**工具 0.33 在既有明确选择交换之上增加译文制作、状态巡检与只读 locale 展示快照。机器翻译、网络上传、引擎适配及独立可玩发布不在此契约内。

## 1. 身份与源码注记

启用清单能力 `content.localization.v1` 后，文本行及选择标签可在行尾声明 `#wl-localization:<id>`：

```wl
event harbor_arrival
  你好，{traveler} 👋，欢迎来到 [[event:harbor|雾港]]。 #wl-localization:welcome
  choice "مرحبا {traveler} 🌙" if greeted #wl-localization:reply
    -> END
```

ID 使用 `[A-Za-z_][A-Za-z0-9_-]*`，在活动源码中唯一。它是作者身份，不是正文哈希、文件/行位置或事件/场景显示名；移动或改名时 ID 随源语句保留。缺失 ID 的文本不会被自动提取或生成 ID。复制语句须由作者显式更换 ID，否则选中该 ID 时报告重复冲突。此注记不进入 runtime tags、输出文字或运行 fingerprint。未启用能力时不解释注记为本地化 ID；使用该语法的工程必须声明能力，以使不支持它的旧客户端按只读处理。

可导出的单位是有 ID 的正文文本行或选择标签。一个单位可含普通文字、插值和显式链接；事件摘要、资料描述、注释及其他目录字段不因本地化被隐式纳入。

## 2. 明确选择与交换格式

`LocalizationSelection` 的 `schema_version` 必须为 `1`，并包含 `source_locale`、`target_locale` 与 `string_ids`。locale 是区分大小写、不做规范化的 ASCII 标识符，采用与 ID 相同的字符范围；它不是 BCP 47 标签，也不做别名转换。source/target locale 按字节精确比较且必须不同；`string_ids` 非空且无重复。

导出严格按 `string_ids` 白名单提取，不包含其他 ID，不沿 include、正文链接或引用扩大范围。selection 不重复，比较时按 ID 字节序排序；package `string_ids` 与 `entries` 也按 ID 字节序排列。每项仅有 ID、相对源码文件/行号/kind、源文修订、源 parts 与可空的译文 parts；不含说话者、邻接文案、绝对路径、链接目标或插值表达式。相对路径只定位被选择的字符串。调用方在导入时再次提交相同 selection；core 要求 package 的来源 locale、目标 locale、ID 集合与 selection 完全一致，避免译者编辑 package 扩大授权范围。

UTF-8 JSON 交换文件版本为 `1`，未知字段和重复 JSON 对象键均拒绝。格式示例：

```json
{
  "schema_version": 1,
  "source_locale": "en",
  "target_locale": "zh-Hant",
  "source_baseline": "fnv1a64:0000000000000000",
  "string_ids": ["welcome"],
  "entries": [{
    "id": "welcome",
    "source_revision": "fnv1a64:0000000000000000",
    "source": {"file": "events/harbor.wl", "line": 2, "kind": "text"},
    "source_parts": [
      {"type": "text", "text": "你好，"},
      {"type": "placeholder", "token": "p0"},
      {"type": "text", "text": " 👋，欢迎来到 "},
      {"type": "link", "token": "l0", "label": "雾港"},
      {"type": "text", "text": "。\n下一行 مرحبا 🌙"}
    ],
    "translation_parts": null
  }]
}
```

`translation_parts` 初始为 `null`；译者将其替换为 typed part 数组。part 的 `type` 只有 `text {text}`、`placeholder {token}`、`link {token, label}`。每个源条目按 source 顺序为插值生成 `p0`、`p1`… token，为链接生成 `l0`、`l1`… token；token 只在该条目内唯一，目标可调整顺序但必须逐种类保留完整多重集。`text` 的内容及 `link.label` 可翻译、可合并或重排。`translation_parts:null` 表示未翻译，空数组表示译者明确给出的空文字（仅当源条目没有受保护 token 时合法）。链接 token 不包含 `TargetRef`；插值 token 不包含变量名/表达式。译文始终是数据，不会被解析为 `.wl`、表达式或可执行代码。JSON 往返保留多行、emoji、中文与 RTL 字符的 Unicode 内容；不承诺字体、RTL 双向排版或其他平台布局。

每项缺少译文、package 少了选中 ID、含未知/重复 ID、source parts 被改、source revision 失配或 token 校验失败时，preview 给出稳定诊断，不生成可应用的部分结果。导出文件仅写到工作区外的一个新 `.json` 文件，不覆盖已有目标。

## 3. 修订、预览与原子导入

- `source_baseline` 是活动 `.wl` 源集合、入口及语言版本/编译能力的确定性快照，按工作区相对路径排序，覆盖路径与原始 UTF-8 字节；排除 manifest 展示字段与 localization sidecar，使不同 locale 的已导出包可依次导入。前缀为 `fnv1a64:` 的 16 位小写十六进制值只检测作者源码快照变化，不是密码学签名。
- `source_revision` 是单个注记语句的确定性 AST 内容修订，覆盖 kind、可见文字片段、插值表达式 AST、显式链接目标/显示文字及影响输出的粘接属性；不含路径、行号、ID、事件/场景名或显示名。源码移动/改名保留 ID，但会改变 `source_baseline`，旧包须重新导出。source revision 与 `Project::content_baseline()` 分工不同；其 FNV 修订标记同样非安全签名。
- Export preview 返回 core `LocalizationExportPlan`（完整当前 content baseline、source baseline、选中条目、diagnostics、`plan_digest`、`can_export`），只读当前 Project 缓冲。Apply 按相同 selection 重建计划；计划摘要或任一基线不匹配时拒绝生成文件。
- Import preview 返回 `LocalizationImportPlan`，包含 `affected_ids`，比较当前源码 ID、逐条 source revision、selection、sidecar 版本及 Project `content_baseline()`。源文变化的 ID 产生带该 ID 的 `STALE_SOURCE` 诊断（调用方将其标记为需复核）；任何 stale、缺译、未知/重复 ID、token 错误、版本/选择不兼容、只读工作区或未保存的 Project buffer 都会使 `can_apply=false`。
- Import apply 要求 preview 的 `plan_digest` 且 Project 没有未保存 buffer；在 clean Project 副本刷新并重跑全部验证。任一计划摘要、Project content baseline、source baseline、版本、selection 或输入 package 不匹配时，整批零写入。成功时在 Project 候选副本中更新 manifest 注册及一个 locale sidecar，通过 Project 可恢复保存事务持久化，事务成功后才替换当前 Project 缓冲；失败保留原缓冲与磁盘内容。文件事务沿用 workspace 既有保证，不宣称多文件文件系统原子性。
- 导入可更新现有 locale 中同 ID 的译文并保留其他 ID；不会自动删除 package 未包含的旧译文。不同 source locale 不能写入同一个 target locale sidecar。sidecar 更新保留 manifest/sidecar 中未知 JSON 字段。
- `plan_digest`、source revision 与 baseline 是稳定性/陈旧检测标记，不是安全签名或防篡改机制。

## 4. 工程注册与持久模型

清单可选 `localizations` 对象，将 target locale ID 映射到工作区内 JSON 路径，例如：

```json
{
  "schema_version": 1,
  "language_version": "1.10",
  "required_features": ["content.localization.v1"],
  "localizations": {"zh-Hant": ".world/localization/zh-Hant.json"}
}
```

非空 `localizations` 必须声明 `content.localization.v1`；路径须工作区内、`.json` 扩展名，且不得与清单或其他注册文档共用。首次导入使用 `.world/localization/<target_locale>.json`；已登记 locale 沿用清单原路径。sidecar schema 为 `1`，包含 `required_features`、`source_locale`、`target_locale` 及按 ID 索引的 `entries`；每项持久化 `source_revision` 与 `translation_parts`。它是作者文件，进入 Project content baseline 与工程备份，但不参与运行 fingerprint。runtime 不直接读取 sidecar；工具 0.33 只接收 core 从当前已应用 Project 缓冲验证并冻结的展示快照（§7）。未知格式/能力遵守 Project 注册文档的只读保护。

## 5. Project / CLI / RPC seams

Core sole source of extraction and validation. Public Project operations are:

- `preview_localization_export(selection)`：只读导出预览；
- `export_localization(selection, plan_digest, destination)`：重新预览并写入新交换文件；
- `preview_localization_import(selection, exchange)`：只读导入预览；
- `apply_localization_import(selection, exchange, plan_digest)`：重新验证并将一个 locale sidecar 应用到 Project。

CLI 与 JSON-RPC 只解码、传入同一 core DTO、序列化 core plan/result，不重新解析 `.wl` 或自行校验 placeholders。机器失败返回稳定错误码和中文 message；不把翻译错误伪装成 JSON-RPC 参数错误。精确机器参数和结果见 [agent-protocol.md](agent-protocol.md)。

## 语言 1.11 台词与片段

稳定字符串白名单也覆盖所有已声明 fragment 内正文/选项和 say 正文，不沿 call 展开而重复提取。
台词 source.kind 为 say，翻译只含 spoken text 及原有占位/链接 token；speaker 身份、显示名和 direction
不会自动进入交换包。正文修订与占位表达式绑定；演出备注不属于翻译内容。导入仍复用完整源码基线、
计划重建、只读保护和 locale sidecar 原子应用，旧稳定 ID 不随正文移动自动改变。

静态人物引用的 `content.character_refs.v1` 能力开启时纳入本地化 source_baseline，能力变更使旧交换包过期；缺少该新能力时保留原baseline字节编码，不为旧工程添加恒false字段。该检查不改变runtime fingerprint。

## 6. 工具 0.33 译文制作与状态巡检

### 6.1 目录、状态与有界查询

`Project::query_localization_catalog` 只读当前已应用的活动源码缓冲和选定 locale 的
注册文档；不读取未应用的正文 WritingBuffer，不保存、不刷新、不沿引用扩大范围。
尚未启用本地化的合法作品仍可查看可翻译单元，写入须先经既有能力预览显式启用
`content.localization.v1`；不自动创建清单、提升语言版本或生成 ID。

目录包含正文、say 正文、choice 标签及 fragment 中同类声明，每个 AST 声明只计一次。
沿用工作区同一物理文件只合并一次的 include 规则，重复 include 不生成新的译文单位或 ID patch；
fragment call 也不展开。不同物理文件中相同文字仍是不同来源，不按文字内容合并。
来源从正式 parser 的 statement provenance 取得，不能拿 event/fragment 声明文件猜
include 片段实际来源；同一根声明下同位置/种类跨 include 来源有歧义时整次返回
`INVALID_SOURCE`，不提供伪精确导航或 ID 修改。
每项含相对来源 `file/line/kind`、临时定位键、可空稳定 ID、源修订、源 typed parts、
可空译文 typed parts、目标 sidecar 路径/JSON Pointer 与派生状态。临时定位键只在当前
源码快照有效，不是持久字符串身份。source parts 继续隐藏表达式及链接目标；此目录
是作者查询，不会自动扩大译者交换的明确 ID 白名单。

状态由 core 唯一计算，按下列优先级归类：`missing_id`、`duplicate_id`、
`invalid_translation`（含坏 locale 文档/坏条目结构或受保护 token 失配）、
`stale_source`、`missing_translation`、`translated`。选定 locale 中没有任何当前源
ID 的旧条目另列 `orphan_translation`；不会自动删除或猜测新身份。源修订不匹配
不能通过人工“完成”标志消除。只读属性与状态正交，未知格式按原字节保留。
目录 `read_only` 表示此 locale 的 typed 操作是否可写；坏 JSON/重复键或无效文档头也
将它设为 true，不能安全合并译文。Project 既有原始字节修复入口不因此被锁死。

查询接受可选 locale、相对来源前缀、kind、精确 ID 白名单、大小写保留的文字查询、至多 7 项且不重复的状态集合及 offset/limit。
core 按来源路径、行、kind、ID 稳定排序，孤立译文在源条目后按 ID 排序；返回匹配
范围内准确 `total`、全目录 `all_total` 和状态计数、当前 `content_baseline/source_baseline`、
当前页与下一 offset。第一页以后必须提交前一页 source/content 基线，任一失配拒绝
续页，不把不同快照拼成一个列表。只读查询不把不完整结果标成精确总数。

下列预算只约束工具 0.33 新增目录、ID、typed 编辑、候选导入及展示快照 API；既有
导出/立即持久化导入接口保持兼容。所有阈值按 UTF-8 字节/实际条目数检查，native
与 WASM 相同：

- 单份交换 JSON、工程清单或 locale sidecar 至多 8 MiB，解析前检查；重复 JSON 对象键拒绝
- 新准备/候选及后台复制前，全部已跟踪文档（含 inactive、墓碑）至多 4,096 份；current、saved 与路径字节合计至多 256 MiB，工作区诊断至多 200 条
- 活动源码总计至多 32 MiB、200,000 物理行，编译前先检查；可翻译 AST 单元及 sidecar 条目各至多 50,000
  物理行按 LF 分界，CRLF 只算一行，末尾 LF 不产生额外空行，空文件为零行
- 单个源或译文 typed parts 的内容总计至多 64 KiB、parts 至多 4,096
- 单次候选导入、typed 编辑或 ID 分配至多 1,000 个明确选中项
- 单页 limit 为 1–200；core 目录/计划 DTO 序列化输出至多 2 MiB（不含协议外层）
- 来源记录及完整目录条目各至多 64 MiB 序列化 payload，逐项借用计数后才能复制路径、parts 与元数据
- 只读运行展示快照的整个公开 DTO 至多 64 MiB，包括来源路径、sidecar 路径与 JSON Pointer；诊断至多 200 条
- 搜索字符串至多 1,024 字节，ID/locale 至多 128 字节

越界返回 `BUDGET_EXCEEDED` 错误，整次失败且零修改；不静默截断源文、译文或
诊断后仍宣称可应用。预算在复制/解析大型输入前先检查，已有更小的工作区保护仍生效。
`Project::check_localization_budget` 是 UI/native worker 复制前的同源廉价预检，只借用
当前/保存字节长度与路径长度，不编译、解析 JSON、复制内容或访问存储。此复制边界
只约束新 A1/A2 准备/候选和新后台宿主，不改变普通 Project.clone 或旧接口行为。
新译文候选在 preview 中还以同一借用计数器投影 manifest/sidecar 的最终 current、saved、
路径与文档数，超过边界即拒绝，不先发出可应用计划或复制完整候选；apply 提交前再次检查最终缓冲。
这些是序列化/内容 payload 上限，不承诺进程总堆内存等于字节上限；解析 AST、受限输入及
候选事务可能同时存在。展示快照复用已排序 entry 做二分定位，不建立逐条路径副本
索引；条目数另受 50,000 上限保护。来源记录构造前先预算新路径的序列化字节，目录
复制前计入完整条目，最终完整公开快照再核预算，不能只计算不含位置的 digest 元组。

### 6.2 显式稳定 ID 计划

`LocalizationIdDraft` 带 schema 1、当前 source baseline 和一组明确来源、源修订、
预期旧 ID（缺 ID 为 null）、作者提供的新 ID。`preview_localization_ids` 检查来源
仍是同一活动 AST 单元，新 ID 合法且最终全工程唯一；重复现有 ID 必须由作者明确
选中要改的声明，不自动选择保留项。预览列出精确前后源码、受影响文件、diagnostics、
content/source baseline 与 plan digest。注记仅改选定语句行尾，保留其它源码字节，
再次编译证明运行 fingerprint、正文修订与受保护内容未变。

`apply_localization_ids` 重建计划并验证 digest、当前基线和外部保存基线，在私有
Project 候选中一次提交全部源码修改。失败保持原缓冲与磁盘；成功仍为未保存。
ID 随语句持久保存，位置和文本哈希不成为 ID。新分配/更换 ID 改变 source baseline，
所有旧交换包须重新导出；更换 ID 不迁移、改写或删除旧 ID 的译文。

### 6.3 Typed 编辑与跨平台候选导入

`LocalizationEditDraft` 带 schema 1、source/target locale、当前 source baseline 与
明确的 `id/source_revision/translation_parts` 列表。core 从同一快照补齐受保护 source
parts 后复用交换导入的选择、修订和 token 多重集校验。译文始终是 typed 数据，不把
文字重新解析成 DSL。空 parts 仅对无受保护 token 的源文有效。

`preview_localization_edit` 与 `apply_localization_edit` 用 plan digest 绑定完整请求
和 content/source baseline；不以旧译文内容推断作者已复核新源文。只应用明确选择的
ID，保留未选译文及 manifest/sidecar/条目未知可选字段。

`preview_localization_import_candidate` 与 `apply_localization_import_candidate` 提供
同样的一次内存事务给桌面和 WASM：允许已经应用到 Project 的未保存源码/展示缓冲，
拒绝外部冲突、未知必需能力、未知格式、坏 JSON、过期计划、缺译、重复 ID 或 token
错误。所有受影响文档在候选全部验证后一次替换；调用方以一个 Project 快照使用既有
`restore` 做一次撤销/重做。预览和应用均不保存、不建立目录、不写 sidecar、不修改
WASM 已挂载快照；保存/下载属于另外的显式动作。重复提交旧 digest 必须拒绝。

既有 `preview_localization_import` 继续拒绝 dirty Project；既有 native
`apply_localization_import` 继续刷新、重新验证并立即通过可恢复保存事务持久化，
成功才替换 Project。旧 CLI/RPC 不被候选接口静默改成只改内存。
旧交换提取/导入同时纠正 include 正文来源，使用同一 parser provenance。旧包若仍带
错误的声明文件位置，报告 `SOURCE_MISMATCH`、真实来源及“重新导出交换包”修复提示，
整批零写并保留输入；不静默重定位到另一个文件，也不保留既有错误来源以伪装兼容。

## 7. 工具 0.33 只读 locale 展示快照

`Project::prepare_localization_presentation` 从当前已应用 Project 缓冲构造验证后的
不可变 `LocalizationPresentationSnapshot`，不执行表达式、不消费 RNG、不更新源码、
译文或保存基线。快照含目标 locale、显式策略、程序 fingerprint、source baseline、
独立 presentation digest（内容元组排序后计算，不因来源遍历顺序变化而改变）
与每个正文/say/choice AST 单元的来源、稳定 ID、源修订、
源 parts、合法译文 parts、状态及 sidecar 精确 JSON Pointer。不得由 UI 重写校验规则。

`strict` 策略要求全部可翻译单元有唯一 ID、有效当前译文，任一缺译、过期、重复 ID
或 token 无效即拒绝；`source_fallback` 必须显式选择，对每个失败单元保留真实状态
并使用源文，不静默宣称已译。不存在 locale 与不支持的 sidecar 格式整体拒绝；
没有可翻译单元的合法作品可以准备空快照。快照绑定整个源程序、来源和译文内容，
不能与另一源码程序混用。未知可选字段不会成为运行指令。

runtime 只消费此只读快照；表达式仍按原 source parts 顺序各求值一次，再按译文
token 顺序重组已物化值，并生成真实 UTF-8 链接范围。choice 身份、执行状态、随机流、
访问记录、glue 与暂停规则由原程序决定。人物名、禁用理由和其它无 ID 展示字段仍
为源文。presentation digest 与运行 fingerprint 分离，locale/译文变化的会话重放
匹配和检查点规则由 runtime 契约明确；本地化不改变源运行语义。


草稿试演的 locale 准备使用 `Project::prepare_draft_localization_presentation`，先核对
原工程、已应用 content baseline、刷新代次及编译能力，再从该不可变草稿实际编译
源码建立私有候选。任何未应用草稿都不被提交或保存；源文变化正常派生需复核状态。
`DraftRehearsalSnapshot::localization_source` 只定位实际草稿 AST 中唯一的正文/say/choice
声明，返回精确 UTF-8 语句/头部范围及 draft 标记；不包含缩进和尾部注释，不声称
表达式级精度。单次来源片段至多 64 KiB，超限拒绝；回源时仍需已有当前稿/导航守卫。
