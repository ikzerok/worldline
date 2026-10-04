# 世界资料 CSV 批量导入与修订（DTO v1）

本契约是作者资料入口，不新增语言版本。默认 1.9、最高 1.13 不变。CSV 是纯数据，不执行公式、HTML、链接或脚本，不改原 CSV。只允许 character/entity 的完整 kind + 稳定 ID 新建或更新；显示名不参与匹配。重复身份行（即使内容相同）整批阻断。不修改身份、不删除对象或属性、不移动声明，不写 world/tag/relation/schema/bind、附件、别名、标签集合或导出 CSV。

## 输入与预算

UTF-8 逗号 CSV，允许起始 BOM、LF/CRLF、双引号包围、双写引号及引号内逗号和换行。不猜编码、分隔符或类型。不允许空/重复表头、非矩形记录、未闭合引号、引号外多余字符或孤立 CR。逻辑行号包含表头（表头=1），column 为 1 起CSV字段列序号（不是物理字符列）；数据记录/类型诊断的 line 为该CSV记录起始物理行号（多行单元格之后仍指记录起点）；CSV语法诊断的 line 为发现错误的物理行号，空/重复表头使用该字段起始行，非矩形记录使用记录末行。引号内 CRLF 明确规范化为 LF，并在预览列出 normalization_count；原工程未触及字节和原有文件换行不变。空白长度必须为零才是空单元格，空格不 trim。

固定上限：原 CSV 2 MiB、500 数据行、64 列、每格解码 UTF-8 64 KiB；CSV结构错误只报告首个阻断，不宣称已可靠解释后续记录；结构有效后独立字段/候选错误最多返回100项，但 error_count 统计全部，任何错误均阻断。完整逐行字段预览最多4 MiB，超限拒绝，绝不截断后允许应用。core工程检查复用源码组织既有4096文件/目录扫描、4096受跟踪文档、源码总量64 MiB、已登记展示文档总量64 MiB及保存基线预算。CLI为保证加载阶段不恢复磁盘，采用既有只读快照加载器：完整工作区的普通作者文件（包括附件和未登记文件）额外限制4096文件/合计64 MiB，排除内部事务和检查点；此为宿主读取限额，不改变语言或DTO语义。

映射DTO最多64项，property key最多256 UTF-8字节，destination最多1024 UTF-8字节；core在工程扫描、编译和逐行展开前拒绝超限请求，避免小CSV被长映射键放大。

每列必须恰好显式映射或 Ignore；恰好一个 Kind 和 Id，重复目标字段拒绝。未知结构字段、类型、DTO版本/JSON字段拒绝。所有 JSON 入口拒绝重复键。字段支持 Display、EntityType、Description、Property{key,value_type}；后两内置字段仅 entity，character 非空输入报错，空且 Keep 可跳过。Property 类型为 Text、Number、Bool、Ref{target_kind}。number 使用 JSON 十进制数字语法、有限 f64，整数不得超过可精确整数区间±9007199254740991，非零十进制下溢为0时拒绝（不静默丢精度为0）；bool 只认 true/false。ref 单元格仅稳定目标 ID，目标 kind 由列映射明确指定。

BlankPolicy 为 Error（默认）、Keep、EmptyText。Keep 空单元格跳过该字段，保留旧值；新对象跳过的可选字段不创建。EmptyText 只准 Display、Description 或 text property，空单元格设置空字符串；identity/entity_type 不可清空。0、false、字符串 null/~ 都不是空。未映射资料严格保持原文字节，不做隐式删除。新对象必须有明确映射的非空 display；新 entity 必须有 entity_type。

## 唯一 API 与 JSON DTO

core::catalog_import 导出以下类型（Rust enum JSON 使用 snake_case）：

- CatalogImportRequest {schema_version:1, expected_baseline:String, csv:String, destination:PathBuf, columns:Vec<CatalogColumnMapping>}
- CatalogColumnMapping {column:usize（0起）, field:CatalogImportField, blank:CatalogBlankPolicy}
- CatalogImportField：带 kind 标签的 Kind/Id/Display/EntityType/Description/Ignore/Property{key:String,value_type:CatalogImportType}
- CatalogImportType：带 kind 标签的 Text/Number/Bool/Ref{target_kind:String}
- CatalogBlankPolicy：error/keep/empty_text
- parse_catalog_csv(&str) -> Result<CatalogCsvTable,CatalogImportDiagnostic>；表含 headers、rows（每行 cells、line）、normalization_count
- Project::preview_catalog_import(&CatalogImportRequest) -> Result<CatalogImportPlan,String>
- Project::apply_catalog_import(&CatalogImportRequest, plan_digest:&str) -> Result<CatalogImportResult,String>
- CatalogImportPlan {schema_version, baseline, input_digest, plan_digest, destination, ignored_columns, normalization_count, rows, diagnostics, error_count, can_apply, changed_files, runtime_fingerprint_before, runtime_fingerprint_after}
- CatalogImportRow {row,line,target:Option<TargetRef>,operation:String（create/update/unchanged/blocked）,source:Option<PathBuf>,fields:Vec<CatalogImportFieldChange>}
- CatalogImportFieldChange {field:String,value_type:String,before:Option<PropertyValue>,after:Option<PropertyValue>}
- CatalogImportDiagnostic {code:String,row:Option<usize>,column:Option<usize>,line:Option<u32>,message:String}
- CatalogImportResult {plan, changed_files:Vec<PathBuf>, new_baseline:String}

运行指纹字段为 u64 / Option<u64>；无有效候选时 after 为 null。诊断与字段解释只由 core 生成，UI 不解析 CSV、WL 或类型。

例列映射：{"column":2,"field":{"kind":"property","key":"age","value_type":{"kind":"number"}},"blank":"keep"}。schema_version、完整源CSV、列映射（含忽略/空策略）、目标文件、完整Project内容基线、工作区身份、全部差异与真实候选指纹都参与 plan_digest。摘要是确定性意外变更检测，不是签名。

## 预览、候选与一次事务

新增对象只进入作者选择的既有活动 .wl；已有对象始终修改原声明文件。预览为只读；允许已提交内存缓冲尚未保存，禁止无效基线工程、未知能力/只读、未解决保存事务或外部磁盘冲突。缺 entity/ref/character-ref 所需语言或 capability 时明确阻断，不修改清单或暗中升级。

core 正式 lexer 与其 token 位置决定精确字段字节 patch；不调用 write_character/write_entity 整块重建。不变语义值是零字节改动。旧属性順序、未映射字段、关系、注释、引号与混合换行保持不变。若一个值内部嵌有块注释而不能证明只改值不丢注释，则安全阻断。新增字段追加到合法对象块内，沿用该声明行换行及已有块缩进。

先收集全部身份和补丁，再对最终候选统一编译/schema校验，允许合法前向/环/自引用。任一错误整批不可应用。预览列出所有数据行、忽略列、真实字段前后值、来源文件、规范化计数、全局阻断和真实 before/after fingerprint。人物显示名/标量资料可能改变运行指纹，旧 Story save/checkpoint 可能不兼容；UI 应展示并确认这一批实际差异。Ref-only不意外重排原标量。旧载入守卫与存档不改、不自动迁移。

apply 从相同快照请求重新生成整个计划，比对摘要，最后重新检查磁盘基线/源码库存、只读/链接、恢复事务。失败零部分内存改动；成功一次替换 Project。调用方只压一次 Project 撤销快照；unchanged 不压历史、不置脏、不重排。core apply 不保存；后续显式 save 复用可恢复 journal，不宣称跨文件磁盘原子。保存后整批撤销仍遵循 Project::restore；外部刷新使旧历史过期。同CSV与映射重导入幂等。

## CLI 与 RPC

CLI：wl catalog-import preview|apply <project> --request-json <DTO> [--csv <file>] [--plan-digest <digest>] [--save] [--json]。--csv 在 CLI 有界读取并验证 UTF-8后替换 DTO 的 csv；core不访问外部CSV路径。apply默认只报告内存候选，结果明确 saved:false 和进程退出将丢弃候选的 notice；短命CLI应显式--save完成持久化；--save仅apply可用，保存使用既有事务。建议 CLI 使用preview后相同CSV快照与摘要进行apply --save。

CLI加载阶段拒绝未完成journal，不运行自动恢复；apply只有显式--save才写盘。

RPC另提供 project.save {project_id,expected_baseline} 显式保存当前缓冲，失败保留内存并复用保存事务恢复。

RPC：catalog.import.preview / catalog.import.apply，参数 {project_id, request}，apply另含plan_digest；均只修改已打开Project内存，后续project.save显式保存。机器能力为 catalog_import_v1。结果成功 {ok:true,plan,...}；数据、候选、陈旧、冲突或能力失败为 {ok:false,error:{code,message}}，协议结构错误/未知project_id才用JSON-RPC error。所有端共用上述core DTO及结果，协议版本仍1。
