# 当前未应用正文的隔离试演

core `Project::compile_draft_rehearsal` 使用真实 `compile_writing_drafts` 编译当前
WritingBuffer覆盖。完整Project内容基线、每文件原文/新文/代次、同文件唯一性和
活动源边界先校验，再形成不可变快照；没有应用、保存或替换作者原稿的副作用。

Rust可用 `DraftRehearsalRequest::from_writing_buffers` 捕获输入，再将快照交给
`worldline_runtime::draft_rehearsal::DraftRehearsal::new(snapshot, seed)`。每次
`continue_bounded` / 明确 `choose_id` 操作同一真实拥有型Story；`inspect_state`
查询它的真实状态，`snapshot().evidence_source` / `declaration_source`解析同一稿的
声明头。回源前必须 `verify_navigation`，来源失效时保留只读证据。

机器一轮试演由 `run_draft_rehearsal` 使用同一实现，参数为
`DraftRehearsalRunRequest { input, seed, budget, choice_ids, inspection }`。所有
choice ID须是这份快照的实际可选身份，由调用者本次明确提供；没有自动旧路线接续。
参数版本/形状/预算错误返回Err；基线、编译或故事失败返回业务`ok:false`。返回含
真实scope、outputs、choices、conditions、state、inspection、预算结局及错误，
`outputs_complete:false` 表示超限/失败导致未返回的输出不完整。

请求JSON最多32MiB、完整响应最多8MiB，单会话逐语句实际输出另限1MiB/32768项。
在形成完整状态JSON前借用检查真实值；正文初始输入最多256草稿文件/16MiB，完整
活动源最多4096文件/64MiB。JSON seed/generation须在安全整数范围，fingerprint
用十进制字符串。详见 [规范](../spec/draft-rehearsal.md)。

本适配器不提供正式save/checkpoint/trace导出。作者返回原稿、明确应用后，从新的
已应用工程开启普通试玩再记录；既有存档和strict replay语义保持不变。

## CLI / JSON-RPC

CLI 使用已连接的真实作品路径和有界JSON请求，二选一：

- `wl draft-rehearsal PATH --request request.json --json`
- `wl draft-rehearsal PATH --request-json '{"input":…, "seed":1}' --json`

文件方式读取完整JSON请求，最多32MiB；内联方式使用相同DTO和校验。`input` 必须
提供真实当前 `content_baseline` 与逐文件完整原文/新文/代次；可从当前Project和
WritingBuffer生成。`budget`、本次明确 `choice_ids`、`inspection` 可省略并采用默认。
文本与JSON输出均不会应用正文或保存作品。

JSON-RPC方法为 `project.draft_rehearsal`，参数
`{"project_id":"已打开的工程ID","request":{…DraftRehearsalRunRequest…}}`。
结果外壳为 `{"ok":…, "applied":false, "saved":false, "result":{…}}`；
领域失败仍有 `result.ok:false` 与真实错误/已执行信息。协议形状、版本或参数预算
失败使用JSON-RPC `-32602`。RPC请求ID编码最多2048字节。

32MiB是请求JSON预算，8MiB是runtime完整结果预算；CLI/RPC完整输出行另预留4096
字节协议外壳。它们与1MiB/32768项的会话实际输出预算不同，也不等于编辑器Worker
消息预算。接口不把草稿结果冒充正式ReplayTrace。
