# 工程问题报告（工具 0.18）

## 1. 真源与边界

本合同增加 core 只读工程问题快照，不增加 DSL、语言版本或全局阻断策略。旧
Diagnostic JSON 字段和诊断含义保持兼容，旧 `wl check` 仍仅检查活动内容与工作区
注册。报告有 error 不等于不能试玩；无可见问题不等于可以导出、发布或覆盖文件。
运行、只读、读者白名单和所有既有保存/发布门禁保持不变。

报告基于 Project 已应用缓冲，不刷新、不保存、不改变 dirty、原始字节或运行指纹。
不包含未应用表单、编辑器组合输入及外部编辑器尚未导入的稿件。
`content_baseline` 绑定源码、清单和已载入注册文档；不是磁盘签名。外部资源只接受
各既有静态验证器实际进行的受限读取，不能宣称实时磁盘一致或完整资源审计。

覆盖域固定为 content、workspace、maps、graph_views、presets、comments、proposals、
templates、saved_queries、manuscripts、reader_profiles、localizations。content 只消费
活动源码；未注册普通 JSON 不解析、不接管。每份注册文档失败独立记录，不能阻止
后续文档检查。墓碑、缺失、坏 UTF-8/JSON、重复键、未知版本和必需能力保留原文。
`READER001` 表示已注册读者配置的静态读取/结构错误，`LOC001` 表示已注册 locale sidecar 的静态读取/结构错误；均为error及document或unavailable位置，不替换既有操作级结果code。
reader_profiles 只检查配置结构、注册身份和路由结构，不执行发布、授权闭包或资源
可交付性检查。本地化只检查已注册 sidecar 的既有结构（版本、能力、locale、entries
及已存译文结构）；缺失译文、选中字符串的陈旧性、源保护 token 一致性是 import/
export 操作级检查，明确不在此报告范围内。

## 2. DTO 与 Rust API（schema_version = 1）

所有以下公共 DTO 均实现 Clone/Debug/Serialize/Deserialize，拥有字符串而不保存
`&'static str`，适于原生和 WASM 后台 worker 传输。枚举 JSON 使用 snake_case。

- `ProblemsReport { schema_version:u32, report_version:String, content_baseline:String, source_observation:String,
  language_version:String, content_has_errors:bool, read_only:bool, complete:bool,
  truncated:bool, reasons:Vec<String>, coverage:Vec<ProblemCoverage>, entries:Vec<ProblemEntry>,
  related:BTreeMap<String,Vec<ProblemLocation>>, limits:ProblemsOptions, compile_count:u32 }`
- `ProblemEntry { id:String, domain:ProblemDomain, severity:Severity, code:String,
  message:String, note:Option<String>, suggestion:Option<String>, primary:ProblemLocation,
  related_count:usize, text_truncated:bool }`
- `ProblemCoverage { domain:ProblemDomain, path:Option<String>, state:ProblemCoverageState,
  reasons:Vec<String> }`。每域有一项 path=null 摘要，注册文档另有相对路径项；content
  另列每份活动源码。state 为 checked / partial / unavailable / not_applicable。
- `ProblemLocation { path:Option<String>, precision:ProblemPrecision, span:Option<Span>,
  byte_range:Option<ProblemRange>, char_range:Option<ProblemRange>, excerpt:Option<String>,
  excerpt_truncated:bool, reason:Option<String> }`
- `ProblemRange { start:usize,end:usize }` 是文档内半开范围；byte_range 为 UTF-8 字节，
  char_range 为 Unicode scalar 索引，不是 UTF-16 或 grapheme。
- `ProblemPrecision` 为 span / document / unavailable。仅有真实语言来源 span 才能是
  span；所有旧 JSON/WS 适配器的 1:1:1 占位必须是 document。document 可打开文件但
  无范围、无伪造行列；不可读/删除/越界为 unavailable。不得解析中文 message 定位。
- `ProblemQuery { severities:Vec<Severity>, domains:Vec<ProblemDomain>, path:Option<String>,
  text:String }`，默认空条件。path 是完整规范相对路径的精确过滤（当前文件使用同字段）；
  text 是对 message/code/note/suggestion/主路径的 Unicode 小写子串查询；各维度 AND，
  同维度 OR，空集不限制。非法绝对/父目录路径拒绝。
- `ProblemCursor { report_version:String, query_key:String, offset:usize }`；query_key
  绑定规范化全部筛选条件和分页类型，related 类型另绑定问题 ID。
- `ProblemPage { report_version:String, content_baseline:String, total:usize, matched:usize,
  entries:Vec<ProblemEntry>, next_cursor:Option<ProblemCursor>, complete:bool, truncated:bool }`
- `ProblemRelatedPage { report_version:String, problem_id:String, total:usize,
  locations:Vec<ProblemLocation>, next_cursor:Option<ProblemCursor>, truncated:bool }`
- `ProblemsError { code:String,message:String }`；code 使用 INVALID_QUERY、STALE_REPORT、
  INVALID_CURSOR、UNKNOWN_PROBLEM、CANCELLED、BUDGET_EXCEEDED。

API 位于 `worldline_core::problems`（类型同时可从 core 根导入）：

```
Project::problems_observation_key(&self) -> Result<String, ProblemsError>
Project::problems_report(&self, options: &ProblemsOptions) -> Result<ProblemsReport, ProblemsError>
Project::problems_report_with_content(&self, content: &CompileResult,
    expected_baseline: &str, options: &ProblemsOptions) -> Result<ProblemsReport, ProblemsError>
Project::problems_report_with_progress(&self, options: &ProblemsOptions,
    progress: &mut dyn FnMut(ProblemDomain) -> bool) -> Result<ProblemsReport, ProblemsError>
ProblemsReport::query(&self, query: &ProblemQuery, cursor: Option<&ProblemCursor>,
    limit: usize) -> Result<ProblemPage, ProblemsError>
ProblemsReport::related_page(&self, problem_id: &str, cursor: Option<&ProblemCursor>,
    limit: usize) -> Result<ProblemRelatedPage, ProblemsError>
Project::problem_location(&self, report: &ProblemsReport, problem_id: &str,
    related_index: Option<usize>) -> Result<ProblemLocation, ProblemsError>
```

普通构建恰好调用一次报告专用buffer-only编译（复用同一Compiler/语言管线，禁止磁盘include fallback，不使用会收养include缓冲的可变compile）；未载入目标以A105报告，必须显式refresh进入活动缓冲后才参与。既有一般编译/check/runtime的磁盘加载合同不变。
with_content 校验 expected_baseline、活动 sources、CompileOptions，以及活动入口存在时实际program.files首项属于该入口后零编译复用；缺入口坏稿不因首项变化而误拒。
map/graph/preset/comment/proposal/template/manuscript 等验证器复用同一 CompileResult；
筛选、选择、分页、位置读取均零编译。`compile_count` 是本次 API 内实际次数（0/1），
测试同时以路径调用结构/计数回归证明，不允许仅写常量掩盖重复 compile。

progress 在编译前及各域前调用，false 返回 CANCELLED，不返回半成功报告。计数/字节
限制产生明确 partial/truncated 报告；非法预算或无法容纳报告元数据返回 BUDGET_EXCEEDED。

## 3. 范围、精度、身份与确定性

coverage.checked 表示该来源在支持的静态范围内完成检查，不表示无错误；无注册为
not_applicable；清单损坏、未知能力、未知 schema、坏 JSON 或编译不完整令受影响语义
依赖为 partial，并列 reason。缺失、删除、坏 UTF-8/不可读为 unavailable。其他来源
仍继续检查。reader 与 localization 摘要始终注明 operation_checks_excluded，此说明
本身不降低所承诺静态范围的 complete。未知/无效 registry 不能把所有域伪装成不适用。
外部冲突标记 source_conflict，complete=false，但不清空保留的本地缓冲或改变只读。

Unicode 范围从同一报告来源快照转换：行列按 1 起 Unicode scalar，CRLF 只作为一条
物理换行；范围不得跨行、拆 UTF-8 字符或超过行尾。0 行/列、溢出、坏 span 返回
unavailable + invalid_span，不夹到首行/文尾。相对路径只能指向工作区内已加载缓冲，
不为生成摘录访问外部文件。source navigation 先校验当前 content_baseline；过期报告
一律 STALE_REPORT，不对新稿沿用旧字节范围。外部刷新后消费者须丢弃旧 report。

排序固定为 Error → Warning → Hint，再规范相对路径（不可用最后）、行、列、code、
domain、message、note、suggestion、related 的完整稳定并列键。仅完整事实相同的条目
去重；WS 从任何适配器传播时都归 workspace。不同路径、span、note/related 或事实不
合并；同 basename 不合并。相关来源维持 producer 次序，不擅自丢弃重复证据。

report_version 是 schema、基线、覆盖、问题与预算的确定性摘要；不是安全签名。
问题 ID 为不透明 `<report_version>:pN`，仅在该报告内有效，不是跨稿件永久身份。先对同长度16个0占位前缀的ID及其余完整事实计算report_version，再把前缀替换为摘要，避免自引用；输出预算按完整定长前缀计。首次related或location请求的ID前缀不是当前报告版本时也返回STALE_REPORT，不能因序号碰撞重定向。畸形或不存在ID返回UNKNOWN_PROBLEM。报告/基线/筛选或分页种类不符，游标
拒绝；游标非权限凭证。相同输入相同结果，分页无漏无重。

编辑器只能把旧项标为“不在当前结果／未重新检查”；本合同不提供跨快照语义身份，
因此不得仅凭旧 ID、数组序号、旧行号、同文案或消失宣称“已解决”。若 UI 独立有可靠
修复确认，则仍须同来源新基线完整重检且无截断；过滤变化不提供此证据。

外部可读性观测：`problems_observation_key` 最多枚举10000个工作区文件，遵守现有
workspace_files 的链接/联接和目录边界拒绝；不读普通文件全文、不编译。摘要绑定工作区
root身份、非受管普通文件的相对路径和本次 readable 状态、已知恢复冲突路径。受管
源码/JSON由 content_baseline 覆盖，不因未保存新文件在磁盘尚不存在而误失效。尚不存在
的全新草稿 root 按空磁盘稳定处理；其他枚举失败返回 OBSERVATION_UNAVAILABLE，报告
保留可检查缓冲并标不完整，不能把失败当空磁盘。WASM仅观察已授权挂载快照，live及
worker均使用 `/world`；原生worker保留原root，不能任意改root后比较。该摘要不承诺
附件内容摘要或实时权限，仅给缓存失效依据。report.source_observation 纳入report_version；
RPC缓存须比较 baseline/options/此key，位置守卫也拒绝不匹配。with_content还比较编译快照中的资产可读性与当前core观测，不一致拒绝STALE_REPORT；报告首尾观测发生变化时标external_observation_changed及不完整，不承诺原子磁盘快照。

## 4. 有界输出与性能验收（实现前冻结）

ProblemsOptions 使用 deny_unknown_fields，默认值/硬上限如下；调用方只能降低：
max_entries=20000、max_related_locations=50000（报告总数）、max_report_bytes=32MiB、
max_text_bytes=16384（每 message/note/suggestion）、max_excerpt_bytes=512（每位置）。
文本裁剪保持字符边界，显式 text_truncated / excerpt_truncated；达到总量预算将
truncated=true、complete=false，并记录 entry_limit / related_limit / report_bytes /
text_limit，受影响 coverage 不得仍称完整。全部报告序列化 JSON 不超过 max_report_bytes。

主列表与 related 分页默认 50，最大 200；limit=0 使用默认，超过 200 拒绝。
单页序列化响应上限 1MiB；容纳不下下项时提前结束并返回 next_cursor，单项也无法
容纳则 BUDGET_EXCEEDED，不能返回貌似全部成功的空页。完整 related 只存报告 related
表，通过 related_page 读取；主列表不重复传输全部相关来源。

固定验收条件：Rust 1.98、同台 Linux cloud CPU、共享依赖 cache、jobs=2、incremental=0，
DEV/TEST_DEBUG=0，默认 dev opt-level=0，不新增构建 profile；记录 CPU/内存、fixture 摘要。
先一次热身，再至少10次独立报告构建；331源码/1920实体/3,425,183原文字节与本轮
注册文档故障稿固定不变。report 每次≤5秒；另以至少5000问题跑10次不同筛选及翻页，
query/page p95≤50ms；报告加UI的增量峰值 RSS≤128MiB（不含构建编译器进程）。GUI
列表只排可见行、详情只排当前有界文本；同机热身后至少60帧，排除首次字体装载的
steady offscreen layout p95≤40ms。超限为验收失败，不倒改阈值。原生实际观测另记；
release、其他平台未实测不得外推，offscreen不等于真实GUI/IME。

## 5. CLI 与 JSON-RPC

能力 `authoring.problems.v1`；旧协议版本和方法不变。`wl problems DIR --json`
构建一次报告并返回默认页；可传 `--query-json`、`--cursor-json`、`--limit`、
`--options-json`；`--related ID` 切换相关位置页且不允许非空 query。
CLI 每次进程重建同输入确定性报告；游标基线变化必须拒绝。JSON 输出
`{ok:true,report:{schema_version,report_version,content_baseline,language_version,
content_has_errors,read_only,complete,truncated,reasons,coverage,limits,compile_count,source_observation},page}`，
不返回完整 entries/related 巨量数组；问题存在仍 ok:true（读取成功）。退出码：无错误且
完整为0；报告含任何 error 或不完整为1；协议/用法/打开/预算/游标失败为2。

RPC `project.problems` params：path 或 project_id 二选一；可选 query、cursor、limit、
options、related_id。结果同 CLI；project_id 沿用会话 refresh/conflicts，并缓存本次报告，
相同基线与 options 下后续筛选/分页复用、零编译。含 `external_observation_changed`
的报告不得缓存复用，下一次请求须重新构建；其他 partial 报告仍按既有缓存键复用。
显式 `refresh:true` 可重建报告；无
会话的 path 请求每次重新观测并构建。每次构建最多一次编译。方法严格拒绝未知字段、
重复 key、非法类型和不兼容 related/query 组合。形状错误 -32602，重复 key -32700；
业务失败 `{ok:false,error:{code,message}}`，与“存在工程错误”区分。WASM 复用 core DTO
及同一方法的纯协议路径，不从 UI 建立另一套解析器。
