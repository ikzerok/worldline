# 检查当前工程问题（0.19）

`wl problems` 和 `project.problems` 使用同一个 core 只读报告。它汇总活动源码、工作区清单以及已注册的地图、网络视图、预设、批注、提案、模板、保存查询、书稿、读者发布配置、本地化文档的静态问题。0.19 为主位置和相关位置补充可信来源角色与有界原文上下文，报告 `schema_version` 仍为 1。

报告只检查 Project 已应用的缓冲，不保存、迁移或自动修复作品。编辑器中尚未应用的表单、书稿或输入法草稿不会由独立 CLI 读取；CLI 从磁盘打开自己的工程，RPC 的 `project_id` 使用自己的会话。

## CLI：检查、筛选与续页

```sh
wl problems my-world --json
wl problems my-world --query-json '{"severities":["error"],"domains":["content"]}' --limit 50 --json
wl problems my-world --query-json '{"path":"people/bell.wl"}' --json
wl problems my-world --options-json '{"max_excerpt_bytes":128}' --json
```

从响应的 `page.next_cursor` 原样取出续页游标，下一次调用保持相同筛选并传 `--cursor-json`。从 `page.entries[].id` 复制问题 ID，使用 `--related ID --json` 查看其相关来源；相关来源有自己的 `next_cursor`。问题 ID 是绑定报告的不透明字符串，不要按序号自行拼接。作品或检查观测已改变时，旧 ID／游标会失效，需先重新读取报告。

`--options-json` 可降低报告条目、相关来源、正文、摘录与总字节预算，不能放宽 core 硬上限。主列表与相关来源每页默认 50，最大 200。达到预算时必须查看 `complete`、`truncated` 和 `reasons`，不要把截断后的空筛选理解为整个作品没有问题。

读取报告成功时 JSON 的 `ok` 为 true，即使报告中存在 error。CLI 退出码为：完整且无 error 为 0；有 error 或报告不完整为 1；参数、IO、预算或游标失败为 2。自动化应同时检查读取是否成功、退出码及报告覆盖状态。

## 先区分位置精度与来源角色

`precision` 回答“当前可以定位到哪里”，仍只有三种值：

- `span`：当前原稿中由诊断生产者证明、经 core 验证的范围；不是只因行列在界内就可信
- `document`：只说明哪份文档有问题；例如 JSON 验证器只有文档级位置时，不把占位坐标当成精确字符
- `unavailable`：没有可用位置，或当前来源未激活、不可读、已失效、范围无效；保留原因，不跳到别的同名文件

新增的 `context.role` 回答“来源范围表达什么”：

- `target`：具体目标，例如变量名、跃迁目标或正文链接
- `expression`：完整表达式或真实子表达式，包括纯 literal 运算
- `statement`：整条语句或真实所属子句
- `declaration`：完整声明头
- `document`：只有文档身份，没有精确命中
- `unavailable`：没有可证明的来源

角色与精度不能互相替代。一个真实 expression 在文件不可读时仍保留角色，但位置精度是 unavailable。主位置和每个 related 各有自己的证据，不能把主位置的角色或范围复制给 related。

core 从正式词法、解析和分析生产入口记录原稿来源，不按错误码、中文 message、同名文字搜索或 UI 缩进猜位置。不同生产入口即使用同一 code，也可能指向不同角色。语句/声明上下文不是“精确到错误单词”的承诺；缺证据时明确降级。具体生产入口范围见 [诊断来源合同](../spec/diagnostic-sources.md#3-生产者覆盖矩阵)，该矩阵也不表示所有恢复分支均已独立实测。

## 怎样读取原文摘录

0.19 产生的每个位置都有 `context`，其 `version` 为 1。摘录为空、位置仅文档级或不可用时仍保留角色及状态。

- `location.byte_range` / `char_range` 是原稿内完整权威命中；摘录预算不会把它缩短
- `context.text` 是原稿的真实切片，不含人为加上的省略号或标记
- `slice_byte_range` / `slice_char_range` 指示摘录在原稿中的位置
- `hit_byte_range` / `hit_char_range` 是完整命中与摘录的可见交集，从摘录开头的 0 起算
- `visibility=full` 表示完整命中可见，`partial` 表示只显示命中的非空一部分，`no_text` 表示没有文本及 slice/hit 范围；合法零宽命中可以是 full
- `prefix_clipped` / `suffix_clipped` 表示前后原文被省略。document 没有伪造的 hit；其 full/partial 描述全文是否显示完整

行列从 1 起，按 Unicode scalar 计数；字节范围是原始 UTF-8 半开范围。它们不是 UTF-16、屏幕列数或 grapheme 索引。CRLF 是一条物理换行，span 摘录只取所属物理行，不包含换行符。

摘录默认最多 512 字节，可降低到 0。窗口围绕命中：能放下时显示完整命中；表达式超过预算时显示可见交集并标 partial。显示窗口沿扩展 grapheme 边界裁切，避免拆开组合字、肤色修饰或 ZWJ emoji；权威字节/scalar 坐标不随之改写。连一个显示单元也放不下时为 no_text，不超支；0 字节预算也不删除角色或权威位置。

兼容字段 `excerpt` 与 `context.text` 相同，`excerpt_truncated` 继续表示裁切或 partial。旧客户端可以继续阅读 excerpt，但不能靠它重新搜索或重建来源位置。

## 当前稿、覆盖状态与刷新

- `coverage` 逐域／逐来源说明 checked、partial、unavailable 或 not_applicable。checked 表示在承诺的静态范围内检查过，不表示没有 error
- `content_baseline` 绑定已应用缓冲；`source_observation` 记录有界路径／可读性观测。两者及 `report_version` 均不是安全签名，也不保证以后磁盘不变
- 导航依次核对路径、基线、观测、版本、报告摘要与完整位置证据，再从已加载缓冲重投影；不重新编译，不为摘录增加磁盘全文读取
- 旧位置、角色、摘录或局部范围被修改，即使另一范围仍合法，也不能用来跳到别处；失败返回 `STALE_REPORT`，应重新检查
- 已知 `source_conflict` 或 `external_observation_changed` 的报告仍可只读查看，但拒绝位置导航。其他 partial/truncated 不因此一律禁用有效来源

报告编译严格消费当前活动缓冲。仅存在于磁盘但尚未载入 Project 的 include 目标不会被偷偷读入诊断；先明确刷新会话，再检查新的已应用来源。旧 `wl check` 的编译范围和行为不因此改变。

报告没有跨版本的“问题已解决”身份承诺。旧问题不再显示可能是修正，也可能是筛选、截断、来源未检查或范围变更，必须查看新报告状态。

## RPC、旧消费者与传输预算

`initialize` 的 `authoring.problems.v1` 表示支持 `project.problems`，0.19 新增工具能力 `authoring.problem_source_context.v1`。提供 path 或 project_id 之一；query、cursor、limit、options、related_id 的含义与 CLI 对齐。会话在缓冲、来源观测、选项与冲突状态未变时复用报告，筛选和分页不编译；`refresh:true` 可显式重建。每次报告构建至多编译一次。

新能力不增加请求协商字段，不改变严格未知字段/重复 key 拒绝、游标或退出码，也不进入语言能力、作品 `required_features` 或 Story save。旧 Diagnostic JSON 的八字段形状不变；来源修正可以改变旧 Span 的值。明确无可信来源的编译诊断使用 line=0 的无效哨兵，消费者不能把它夹到第一行。

新实现能读取无 context 的旧 schema 1 报告，旧 excerpt 可只读显示、查询和相关来源分页，首次导航必须刷新重建。未知 schema/context.version 同样不能作为当前可导航报告。经项目验证的旧 DTO 可忽略新增响应字段；不承诺任意第三方 strict JSON 实现兼容。接入端应区分“能读取旧报告”和“能在当前稿导航”，不能用默认字段绕过证据验证。

所有预算按实际序列化字节计算，包括 context 元数据与兼容 excerpt 的重复文本：

- 完整 core report 硬上限 32 MiB，可由请求降低
- 主列表/related 页各不超过 1 MiB；`wl problems` 和成功解析并识别为 `project.problems` 的 RPC 方法响应，完整 JSON 加行尾一个 LF 也各不超过 1 MiB
- RPC 计入实际 jsonrpc、原始 id 的 JSON 编码及 result/error 外壳。仅 `project.problems` 遇到连最小错误都无法回显的巨大 id 时，在零方法执行下返回 `-32600`、`id:null` 与 `request_id_exceeds_response_budget`；其他可回显 id 原样保留
- CLI 超长错误详情保留 error.code 和退出码 2，只缩短 message；RPC 业务详情过长使用紧凑 `BUDGET_EXCEEDED`，协议详情过长保留原协议 code。通知仍无响应
- 编辑器原生/Web 使用 16 MiB report 请求预算，并检查完整 worker `WorkOutput` JSON 不超过 32 MiB；不能以裸 report 合格代替外壳检查

超过预算按既有语义明确截断或返回 `BUDGET_EXCEEDED`，不会静默丢弃角色，也不会把超限空页说成全部成功。

此整行上限是上述问题方法的边界，不是全局 JSON-RPC 解析器保证。巨大重复 key 等在方法识别前发生的通用 JSON 解析失败仍走原协议错误路径，不属于本版方法响应预算承诺。

## 与其他动作的关系

`wl check` 保持既有活动内容＋清单检查；它的成功不代表全部展示文档、特定公开选择或本地化交换包都已验证。

统一问题报告不统一阻断策略。地图或书稿 error 不会因出现在列表就自动禁止无关故事运行；发布、导出、保存和只读保护仍使用原本各自的校验。读者配置的静态合法不代表实际发布计划已批准；本地化文档的静态合法不代表选定字符串没有过期、缺失或保护 token 问题。

0.19 不新增 DSL；默认语言 1.9、最高既有 1.13，表达式语义、指纹、choice signature、once 身份与运行版本守卫保持原合同。普通 Story save、trace 与 checkpoint 的兼容不能互相推断；升级实证和验证边界见配对编辑器的 [0.19 发布说明](../../worldedit/docs/releases/v0.19.0.md)。

完整 DTO、预算、错误及身份合同见 [工程问题规范](../spec/problems.md)、[来源上下文规范](../spec/problem-source-context.md) 与 [问题 Schema](../spec/schemas/problems.schema.json)。
