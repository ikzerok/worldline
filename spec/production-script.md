# 同稿角色制作台本（工具 0.34）

本契约只读消费当前作者稿，不新增语言语法、运行行为、读者发布授权或第二份正文。
默认语言仍为 1.9，正式 `say` 仍需作者显式启用既有 1.11。运行不参与台本生成。

## 范围与身份

`ProductionScriptRequest` schema_version=1：

- `scope` 是带 `kind` 的严格对象：`current_target { target }`、
  `manuscript { query: ManuscriptQueryRequest, chapter_ids: Option<Vec<String>>,
  expected_query_key: Option<String> }`、`project`
- `include_fragments` 默认 true，表示静态 call 闭包；false 显著标记直接范围、未纳入被调用片段
- `speaker: Option<TargetRef>` 只接受现存 `character` 的完整身份；不使用 POV、with、显示名或冒号猜测
- `include_narration`、`include_choices` 默认 false；打开后分别纳入 text、choice，且不会因此纳入其他角色 say
- `target_locale: Option<String>`，None 为源文；`locale_policy` 为 `strict`（默认）或 `source_fallback`
- `statuses: Vec<ProductionStatus>`（默认空，表示全状态）、`search: String`（默认空）在完整范围内过滤
- `limits: ProductionLimits`、`expected_snapshot_key: Option<String>`；请求无分页字段，分页作用于已建立的不可变快照

所有输入 DTO 拒绝未知字段；严格 JSON 入口拒绝重复键。TargetRef 只允许 kind/id。
书稿 scope 沿用同一完整书稿查询，忽略页面偏移、折叠和当前视图；chapter_ids=None 表示全部匹配章，
Some([]) 表示明确空选择；重复、歧义、缺失或被查询排除的所选章失败。当前目标支持 event/scene/fragment/entity；
entity 没有 Text/Say/Choice 单元，结果为零，不把描述伪造为可译台词。project 含全部活动事件与片段定义；
该范围的 include_fragments=false 也不删除活动片段，闭包开关仅控制额外调用范围扩张。

按当前每文件唯一 WritingBuffer 建立同一编译快照；书稿草稿使用既有 ManuscriptQueryDraft。
重复 WritingBuffer 路径（即使相同）、过期草稿、无效编译或无法确认完整的工程观察均失败，不回退旧结果。
快照身份绑定完整 Project 内容基线、全部草稿原文与代次、编排文档/查询、完整源码（含角色显示名、speaker、direction）、
选定 locale 文档与范围及过滤参数。source_revision 只表示既有翻译源版本，不代替台本身份。

## 定义、出现与使用关系

选定目标与闭包片段按真实来源去重。event 与其 scene 重叠时，同一物理正式语句只保留一次。
共享片段沿静态 call 目标遍历，每个定义最多一次；菱形调用不展开树，不执行实参、条件、once、状态或 RNG。

制作台本内部来源身份使用同一编译快照中的原始规范 `PathBuf`、物理行及正式
语句 kind；调用点另保留列。展示用相对路径不作为关联、去重或 row_key 的唯一键。
本地化条目与正式语句按该内部身份一一关联，重复或缺失身份为 `INVALID_SOURCE`，
不得以后写覆盖或角色筛选隐藏歧义。事件与 scene 的合法重叠仅合并同一物理语句。

在编译前先检查活动 Project/缓冲的原始路径（拒绝非 UTF-8 身份），并在
scope、speaker、locale、status 和文本筛选前检查完整编译来源路径能否从公开
相对 file 无损还原到同一物理 `PathBuf`；有损展示、冲突或不可逆路径均为
`INVALID_SOURCE`，不生成快照、分页或任何格式材料。原生 Unix 的字面 `a\b.wl`
不能被当作目录路径 `a/b.wl`；即使不存在后者也不能返回错误导航位置。该门仅限
制作台本，不更改 Project 加载、源码救援或其他既有文件编辑能力。可信 source_hit
仍须验证当前完整快照并恢复唯一真实来源。私有来源索引及 raw 路径副本完整计入
既有来源/结果元数据预算，不因展示字段较短而绕过上限。
输出按工作区相对文件、行、列、kind 的确定顺序，不声称为运行顺序。

`ProductionScopeSummary` 分开提供 selected_chapter_occurrences、root_targets、definition_count、
added_fragment_definitions、call_sites、source_files、matching_rows、source_only、includes_fragment_closure。
章节出现保存 manuscript_id/chapter_id/target；定义保存正式 target、相对 source、is_root（否则为闭包/内部子声明）；调用点保存 caller/callee、
相对 source、内部 control_ancestry。重复章不放大定义行或 call_sites；项目 scope 的章节出现数为零。

`ProductionRow` 字段固定为 row_key、kind、declaration、speaker（正式 target/display 或 null）、
source（相对 file/line/column）、stable_line_id（无则 null）、source_revision、source_parts、
selected_parts、status、target_locale、used_source_fallback、control_ancestry、external_call_uses、
可选 direction。row_key 仅为当前快照锚，不是持久行 ID。

parts 使用既有 LocalizationPart typed tokens：literal/text、placeholder、link。表达式保留未求值 token；
不提供猜测读音或求值结果。内部控制祖先包括原始 if/else 条件、choice 条件/enable/once 与 scene 身份，均标记未求值；
choice 祖先不含选项正文或 disabled_reason；选项文案自身也带该 choice 的 condition/enable/once。
直接选中 scene 仍保留所属事件 after 入口约束，和从事件进入同一 scene 的上下文一致。外部调用使用关系与定义内部祖先分开，不合并为“实际发生条件”。
外部关系为所选图中直接调用所属定义的 call 点；上游关系可通过完整调用表追溯，不枚举所有路径。
默认绝不携带其他角色相邻正文；本版本不提供相邻正文选项。

## Locale 与完整性

`ProductionStatus` 为 source、translated、missing、stale、invalid。source-only 不要求稳定 ID。
目标 locale 下无稳定 ID/无译文为 missing；源版本变更为 stale；typed token/链接/占位符不匹配为 invalid。
core 复用现有本地化来源与 sidecar 校验，绝不调用 runtime Strict 或自行宽松替代校验。
未知 locale、坏注册/sidecar、不支持版本、全工程重复稳定 ID 等完整性失败在范围过滤前阻断。
其他未选中单元缺译/过期/译文 token 错误不阻断所选单元。

query 可检查所有状态。strict 下 export 要求全部最终过滤选中单元 translated；失败是 LOCALE_INCOMPLETE，
不返回部分成品。source_fallback 由作者显式选择，对 missing/stale/invalid 使用源 parts，
逐行 used_source_fallback=true 并保留实际 status；交付头明确回退策略及数量。
speaker 名称与 direction 始终标记为源语言作者元数据，不称已翻译。

## 不可变结果、分页与预算

Public API（错误统一 `ProductionError { code: String, message: String }`，消息中文）：

- `Project::production_script_snapshot(&[WritingBuffer], &[ManuscriptQueryDraft], &ProductionScriptRequest) -> Result<ProductionScriptSnapshot, ProductionError>`
- `ProductionScriptSnapshot::key() -> &str`
- `ProductionScriptSnapshot::summary() -> &ProductionScopeSummary`
- `ProductionScriptSnapshot::definitions() -> &[ProductionDefinition]`
- `ProductionScriptSnapshot::chapter_occurrences() -> &[ProductionChapterOccurrence]`
- `ProductionScriptSnapshot::call_sites() -> &[ProductionCallUse]`
- `ProductionScriptSnapshot::author_direction(row_key: &str) -> Option<&str>`
- `Project::production_script_source_hit(&[WritingBuffer], &[ManuscriptQueryDraft], &ProductionScriptSnapshot, row_key: &str) -> Result<SearchMatch, ProductionError>`
- `ProductionScriptSnapshot::page(offset: usize, limit: usize) -> Result<ProductionScriptPage, ProductionError>`
- `ProductionScriptSnapshot::export(&ProductionExportOptions) -> Result<ProductionArtifact, ProductionError>`
- `Project::validate_production_script(&[WritingBuffer], &[ManuscriptQueryDraft], &ProductionScriptSnapshot) -> Result<(), ProductionError>`
- `parse_production_script_request(&str) -> Result<ProductionScriptRequest, String>`
- `parse_production_export_options(&str) -> Result<ProductionExportOptions, String>`
- native `write_production_script_new(workspace: &Path, destination: &Path, artifact: &ProductionArtifact, before_publish: &mut dyn FnMut() -> Result<(), String>) -> Result<(), String>`

快照不可从 JSON 构造、字段私有，不实现 Serialize；行分页只克隆请求页。
source_hit 先验证最新完整输入和观察，再由保存在快照的正式 AST 来源生成当前 SearchMatch；
含原文的可信导航凭证仅用于作者界面，永不进入交付 DTO。
页包含 schema_version/snapshot_key/summary/offset/limit/total/next_offset/rows。
完整过滤后分页，limit 1–100，零结果合法，越过末尾为空，精确 total。

`ProductionLimits` 默认且硬上限：chapters=4096、definitions=4096、call_sites=20000、rows=50000、
source_files=4096、source_bytes=16777216、result_bytes=16777216、export_bytes=33554432；
所有限制必须为正且不得超过硬上限。完整编译后、任何本地化收集前，core 先用迭代遍历验证全部 AST 的 200000 节点（含分支头）与 64 嵌套深度上限；
只有经此验证的不可变快照可供 source_hit 再次读取。
范围/控制/来源元数据也计入序列化预算；超预算 BUDGET_EXCEEDED，没有截断成功。
严格输入最大 4 MiB、search 4096 UTF-8 字节，选择和身份字段受同样硬预算约束。

上述预算不是可独立承诺的工程容量。core 在选择角色、章节或行状态之前，仍执行
[本地化完整性预算](localization.md)：全工程至多 50000 个可翻译源单元，每单元
至多 4096 parts / 64 KiB，清单及每份 sidecar 至多 8 MiB，来源记录及目录元数据
各至多 64 MiB；已加载 Project 的活动源码另经既有 200000 物理行门。未应用
WritingBuffer 叠加后的当前编译稿仍受 16 MiB、AST 及源单元预算约束；物理行门
不应被描述为对此叠加稿的另一次全量检查。全部跟踪文档（含 inactive、
墓碑）至多 4096 份，current/saved/path 总计至多 256 MiB，工作区诊断至多 200 条。
活动源码字节同时受本节更紧的 16 MiB 上限限制。书稿范围还继承
[完整交付范围](manuscript-delivery.md)的 4 MiB 身份/路径元数据预算。
缩小角色或章节只能降低所选结果的部分规模，不能绕过这些全局来源、AST、注册资料
及工作区完整性门；未选中内容超限也可使整个请求失败，不以局部成功隐藏错误。

## 私密输出边界

`ProductionExportOptions { schema_version: 1, format: json|markdown|csv, include_direction: bool=false }`。
`ProductionArtifact` 私有不可伪造，提供 `bytes()`, `format()`, `snapshot_key()`。
所有格式只读同一不可变 snapshot；页面不得自行排序、合并、换文或生成另一套材料。

导出 DTO 为 `ProductionDocument { schema_version, snapshot_key, summary, scope_kind, speaker, target_locale, locale_policy,
metadata_language: "source", direction_included, definitions, chapter_occurrences, call_sites, rows }`。
它仅包含选定生产字段；JSON 为精确值。没有源文件全文、绝对路径、ReviewSource、raw say、excerpt、
内部导航凭证、未选择 locale、相邻其他角色正文或附件。include_direction=false 时，所有层级连 direction 字段都不输出。
查询页默认也不包含 direction；作者可通过 `author_direction(row_key)` 显式读取所选行备注。
启用导出 direction 时才在行字段加入它，不通过 raw source 实现。

CSV 是表格阅读格式：每一个单元格（包括表头、数字/布尔的显示文本和空值）统一加一个 ASCII 单引号，
再以双引号包裹，内部双引号加倍，UTF-8、记录分隔符 CRLF；源文换行保留在引号内。
安全前缀属于输出字节，不改变原稿。CSV 不是无损回导，精确值请用 JSON；
不保证所有表格软件或再次另存后的通用安全，不声称未经实测的 Excel/LibreOffice 行为。
嵌套控制/调用/parts 字段用精确 JSON 文本作为单元格内容，同样转义。

Markdown 对所有作者字符串进行 HTML/Markdown/链接字符转义，控制字符显示为可见转义；
不生成可执行 HTML、图片或外链。显示未求值标记、范围、locale 状态与回退标记。

native 复用已有 core 暂存字节、二次 before_publish 校验、无覆盖 rename_new 发布契约，
只允许工作区外已有真实目录中的新绝对路径文件（扩展名与格式一致）；取消/过期/失败清理暂存，
既有目标不覆盖。WASM 只交付完全相同 bytes 下载请求，不能声称已存入用户磁盘。
材料不修改 Project、不保存作者稿，也不扩大 reader profile 白名单。
