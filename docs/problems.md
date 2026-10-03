# 检查当前工程问题

`wl problems` 和 `project.problems` 使用同一个 core 只读报告。它汇总活动源码、工作区清单以及已注册的地图、网络视图、预设、批注、提案、模板、保存查询、书稿、读者发布配置、本地化文档的静态问题。

报告只检查 Project 已应用的缓冲，不保存、迁移或自动修复作品。编辑器中尚未应用的表单、书稿或输入法草稿不会由独立 CLI 读取；CLI 从磁盘打开自己的工程，RPC 的 project_id 使用自己的会话。

## CLI

```sh
wl problems my-world --json
wl problems my-world --query-json '{"severities":["error"],"domains":["content"]}' --limit 50 --json
wl problems my-world --query-json '{"path":"people/bell.wl"}' --json
```

从响应的 `page.next_cursor` 原样取出续页游标，下一次调用保持相同筛选并传 `--cursor-json`。从 `page.entries[].id` 复制问题 ID，使用 `--related ID --json` 查看其相关来源；相关来源有自己的 `next_cursor`。问题 ID 是绑定报告的 opaque 字符串，不要按序号自行拼接。作品或检查观测已改变时，旧 ID／游标会失效，需先重新读取报告。

`--options-json` 可降低报告条目、相关来源、正文与总字节预算，不能放宽 core 硬上限。主列表与相关来源每页默认 50，最大 200。达到预算时必须查看 `complete`、`truncated` 和 `reasons`，不要把截断后的空筛选理解为整个作品没有问题。

读取报告成功时 JSON 的 `ok` 为 true，即使报告中存在 error。CLI 退出码为：完整且无 error 为 0；有 error 或报告不完整为 1；参数、IO、预算或游标失败为 2。自动化应同时检查读取是否成功、退出码及报告覆盖状态。

## 来源与检查范围

- `span` 表示 core 验证过的来源范围；行列按 1 起 Unicode 字符计算，不是 UTF-8 字节列
- `document` 只说明哪份文档有问题；例如现有 JSON 验证器只有文档级位置时，不把占位的 1:1:1 宣称为精确字符
- `unavailable` 说明当前位置不可读取、已失效或范围无效；不能跳到别的同名文件作为替代
- `coverage` 逐域／逐来源说明 checked、partial、unavailable 或 not_applicable。checked 表示在承诺的静态范围内检查过，不表示没有 error
- `content_baseline` 绑定已应用缓冲；`source_observation` 记录有界路径／可读性观测；两者都不是安全签名，也不保证以后磁盘不变

报告编译严格消费当前活动缓冲。仅存在于磁盘但尚未载入 Project 的 include 目标不会被偷偷读入诊断；先明确刷新会话，再检查新的已应用来源。旧 `wl check` 的编译范围和行为不因此改变。

## 与其他动作的关系

`wl check` 保持既有活动内容＋清单检查；它的成功不代表全部展示文档、特定公开选择或本地化交换包都已验证。

统一问题报告不统一阻断策略。地图或书稿 error 不会因出现在这个列表就自动禁止无关故事运行；发布、导出、保存和只读保护仍使用原本各自的校验。读者配置的静态合法不代表实际发布计划已批准；本地化文档的静态合法不代表选定字符串没有过期、缺失或保护 token 问题。

新报告没有跨版本的“问题已解决”身份承诺。旧问题不再显示可能是修正，也可能是筛选、截断、来源未检查或范围变更，必须查看新报告状态。

## RPC 与规范

`initialize` 的能力 `authoring.problems.v1` 表示支持 `project.problems`。提供 path 或 project_id 之一；query、cursor、limit、options、related_id 的含义与 CLI 对齐。会话在缓冲、来源观测、选项与冲突状态未变时复用报告，筛选和分页不编译；`refresh:true` 可显式重建。

完整 DTO、预算、错误及身份合同见 [工程问题规范](../spec/problems.md)；机器 JSON 形状见 [问题 Schema](../spec/schemas/problems.schema.json)。
