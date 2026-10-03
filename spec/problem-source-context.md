# 问题来源上下文（工具 0.19，schema 1 加法）

本合同补充 [problems.md](problems.md) 与 [diagnostic-sources.md](diagnostic-sources.md)。它不增加语言语法、作品
required_features、Story save 能力或新的 ProblemPrecision 值，也不改变旧 Diagnostic
JSON 的八个字段、诊断事实集合与运行语义。

## 1. 来源角色与投影

Diagnostic 内部来源元数据使用 `DiagnosticSourceRole`：target / expression / statement /
declaration / document / unavailable。前四项分别表示具体目标、完整表达式、语句上下文、
声明上下文；它们可以投影为 span，但只在原稿范围已被 producer 证明且转换合法时。
角色不能由 message、错误码猜词、搜索同名对象或 UI 补缩进生成。

`Diagnostic::source_role() -> Option<DiagnosticSourceRole>` 与
`related_source_role(index) -> Option<DiagnosticSourceRole>` 提供 primary/related 证据；
related 元数据和既有 related 数组同序，缺项为 None。None 按 document 投影，不能仅因
所属域为 content 就承认占位 span。明确 unavailable 不提供范围。角色为 target 等而
文件不可读或 span 非法时保留角色，ProblemPrecision 为 unavailable，并注明 reason。
内部字段不进入 Diagnostic 的手写 JSON 序列化。

## 2. DTO 与坐标

`ProblemLocation` 保留全部旧字段，新增：

```
context: Option<ProblemSourceContext> // serde default；None 序列化时省略
```

新实现产生的位置均含 context，包括 document、unavailable、0 字节摘录。类型为：

```
ProblemSourceContext {
  version: u32, // 当前为 1
  role: ProblemSourceRole,
  text: Option<String>,
  slice_byte_range: Option<ProblemRange>,
  slice_char_range: Option<ProblemRange>,
  hit_byte_range: Option<ProblemRange>,
  hit_char_range: Option<ProblemRange>,
  visibility: ProblemContextVisibility,
  prefix_clipped: bool,
  suffix_clipped: bool,
}
ProblemSourceRole = DiagnosticSourceRole
ProblemContextVisibility = full | partial | no_text
```

`ProblemSourceRole` 从 problems 模块公开重导出。`ProblemPrecision` 仍且仅为 span /
document / unavailable；角色描述范围含义，precision 描述当前范围是否可导航。

- location.byte_range / char_range 是原稿内权威完整命中，摘录预算永不缩短它们
- slice_byte_range / slice_char_range 是 text 在同一原稿中的半开范围，分别按 UTF-8 字节
  与 Unicode scalar 计数；不是 UTF-16，也不是 grapheme 索引
- hit_byte_range / hit_char_range 是完整命中与 slice 的可见交集，坐标从 text 的 0 开始
- text 必须严格等于原稿 byte slice；局部字节与 scalar 切片必须得到同一原文
- span 为同一物理行内范围；上下文只取该物理行（不带 LF/CRLF），document 摘录取文档
- full 表示完整命中都在 text 中；partial 表示只显示命中的非空交集；no_text 表示 text
  及四个 slice/hit 范围均为空。零宽合法命中可以有 full 与零宽局部范围
- document 不捏造 hit 范围；有 text 时，full/partial 表示整个文档是否展示完整
- unavailable 为 no_text。不能把无 text 当成来源角色不存在
- prefix_clipped / suffix_clipped 表示 eligible 原文在摘录之前/之后被省略，span 的
  eligible 范围为物理行，document 为全文。no_text 时按命中起点将省略部分分为前后
- 省略号、强调、role 标签只属于展示，不能写入 text 或用其长度转换定位

旧 excerpt 与 context.text 相同，excerpt_truncated 为前后裁切或 partial；兼容期间
重复文本的所有 JSON 字节必须计入报告/分页/传输预算。旧消费者可以继续只读显示。

## 3. 有界窗口和 Unicode

默认/硬上限 max_excerpt_bytes=512 不变，调用方可以降低到 0。窗口围绕命中；完整
命中可放入时须完整包含，优先均衡前后上下文，再利用剩余预算。命中超预算时从命中
开始的可显示部分投影，必须 partial，不能谎称完整。单个显示单元容不下时 no_text。

边界选择使用已有 unicode-segmentation 的扩展 grapheme 边界，避免割裂组合字、
emoji modifier、ZWJ 家庭/职业序列。原始坐标仍严格按 scalar/字节，不升级为 grapheme。
即使命中端点在 grapheme 内，也只扩展显示窗口、不修改权威命中。预算不足时不得为
容纳字符超支；0/1/2/3/4/511/512 字节均保留 role、precision 与权威位置。

## 4. 能力、版本与旧消费者

公共常量 `PROBLEM_SOURCE_CONTEXT_CAPABILITY = "authoring.problem_source_context.v1"`。
RPC initialize.capabilities 新增该工具能力；CLI 的加法输出由 context.version=1
及本合同标识，不增加 report 的必需字段，不增加请求协商。该常量与 context DTO 可
由 core/native/WASM 消费者直接使用，不进入 language capabilities、required_features
或 Story save。

schema_version 仍为 1。CLI/RPC 请求字段、严格未知字段/重复 key 拒绝、游标及退出码
不变，不增加必须协商的请求字段。worker 继续接收 schema1，使用同一 core DTO；有
context 走新路径，缺 context 按旧纯文本只读路径。新实现须能读取无 context 的冻结旧 report；已冻结的旧 DTO 须能忽略新增的 context
响应字段读取新 payload。
此兼容承诺限于项目已测试的旧 DTO/消费者，不保证任意第三方 strict JSON 实现。

旧 report 的只读 query/related page 不需要编译；首次导航返回 STALE_REPORT 要求刷新。
未知 schema/context.version 不能被当成当前可导航报告；缺 context 也不得通过兼容
默认值放行。

## 5. 同稿证据、摘要与预算

所有 context 绑定所在 report 的 content_baseline / source_observation。导航保持旧
路径合法性、基线、观测守卫，然后核验 schema/context.version、当前报告摘要及位置证据，最后
从已加载缓冲重投影并严格比较整个 ProblemLocation（包括 context）；任何差异均返回
STALE_REPORT。不能忽略 context 比较以接纳旧报告，不重新编译，不增加磁盘读取面。

report_version 确定性摘要纳入 schema 与所有新 context 字段，仍使用
定长占位 ID 消除自引用；摘要不是安全签名。位置篡改、合法但被替换的 span、角色和
局部范围篡改不得定向到另一处原文。compile_count 不参与报告身份。

max_report_bytes 仍可自定义降低，硬上限 32MiB；整个 report 和旧 excerpt 重复字节、
context 元数据均按实际序列化计算。主/related 页以及 CLI/RPC 完整响应各不超过 1MiB。
编辑器原生/WASM 沿用现有 16MiB report_options，为 32MiB 外壳保留空间；通用 worker
保持请求 options/响应 limits 一致，不悄悄改低预算，并以有界 serializer 检查完整
WorkOutput 不超过 32MiB，不能只保证裸 report 合格。预算耗尽保留既有明确截断或
BUDGET_EXCEEDED 语义，不能静默丢角色字段。

验收包括长中文行尾命中、超预算完整表达式、主/相关来源、文档/不可用/缺字段旧报告、
字节/scalar/grapheme、CRLF、同字重复、上下文篡改、零编译 query/page/location；性能沿
R3 冻结的 129 源码/3072 问题 fixture，热身后十次独立 debug CLI 每次不超过 1.6 秒。

协调补充：未激活 `.wl` 不能投影有效命中范围，保留路径/角色并标 unavailable；已知
source_conflict 或 external_observation_changed 的报告拒绝位置导航，仍可只读列出原因。
不把任意 partial/truncated 一律禁用，不改变其他域的解释或已加载缓冲。这里不新增
磁盘全文读取；编辑器保留既有 verify_source_navigation 与外部冲突检查。

RPC 外壳补充：`project.problems` 的 1MiB 响应预算包含实际 JSON-RPC `jsonrpc`、原始
`id` 的 JSON 编码、result/error 外壳及逐行传输的一个 LF。分页先扣除这部分开销；
最终 dispatch/handle/run 再核验完整响应。不能仅计算 result Value 或裸 page。

仅此方法：执行前按原 id 与规定的紧凑业务错误/协议错误外壳计算能否安全回显。
若原 id 连该最小错误响应都容不下，零方法执行返回 `-32600`、`id:null`、
`data:"request_id_exceeds_response_budget"`，明确请求标识超过响应预算；这是巨大 id
不能被回显的局部例外，不截断或另造 id。能回显的 id 始终原样返回；过长业务详情
变为紧凑 `BUDGET_EXCEEDED`，过长协议错误详情保留原协议 code（如 -32602），使用
固定简短 message 与 null data。通知仍不产生响应。其他 RPC 方法及旧请求字段校验
完全不变，不将同一查询重复执行以重试缩页。

CLI 外壳补充：`wl problems --json` 的成功和失败输出均以完整 UTF-8 JSON 加一个 LF
计入 1MiB。成功结果缩页时预留 LF；错误详情超限保留原 error.code 与退出码 2，
仅把 message 换为“工程问题错误详情超过字节预算”。不改变其他 CLI 或人类文本输出。
