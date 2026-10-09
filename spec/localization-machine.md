# 0.33 本地化机器工作流

本契约只提供作者本地工作区和运行会话的接口，不增加公开作品包、自动翻译、网络上传或第三方服务。语言和译文规则由 [localization.md](localization.md) 与 [localization-runtime.md](localization-runtime.md) 定义；CLI/RPC 不重新分析源码或解释 token。

## 1. 新的目录与内存创作入口

保留原有 `wl localization export|import preview|apply` 与 `localization.export.*` / `localization.import.*` 的参数、成功结果和保存语义。特别是旧 import apply 仍要求干净工程，成功后立即持久化，不能将旧命令悄悄改为退出就丢失的草稿。

新增一次性 CLI：

```text
wl localization catalog <目录或入口> --request-json <LocalizationCatalogQuery> [--json]
wl localization ids preview|apply <目录或入口> --request-json <LocalizationIdDraft> [--json]
wl localization edit preview|apply <目录或入口> --request-json <LocalizationEditDraft> [--json]
wl localization import-candidate preview|apply <目录或入口> --request-json <导入草稿> [--json]
```

导入草稿只包含 `selection` 与 `exchange`。所有请求是 core DTO；JSON 对象重复键拒绝。apply 必须额外提供 `--plan-digest`，允许显式 `--save`；preview 不接受这两个参数。重复、未知、空值及缺失参数是用法错误，不允许静默覆盖先前参数。

- catalog 是只读查询，返回 `page`、当前内容基线和工作区诊断。筛选、排序、分页身份、准确总数及预算全由 core 生成；陈旧续页失败后须从第一页重新查询
- catalog 顶层 `read_only` 直接转发 core 页中的只读状态，包含该语言 sidecar 的类型安全限制；其他创作响应的顶层 `read_only` 指工作区层限制，具体候选可应用性仍由 `plan.can_apply` 与诊断决定
- preview 返回 `plan`，不改变工程；`can_apply:false` 是可审阅的业务结果，不是 JSON-RPC 协议错误
- apply 重建 core 计划并核验 digest，成功只修改本进程中的 Project，返回 `applied:true,saved:false` 与明确的“进程退出将丢弃候选”提示
- 只有显式 `--save` 才在成功内存应用后另行调用 Project 保存事务。保存失败返回 `ok:false,applied:true,saved:false,stage:"save"`，不谎称没有发生内存应用，也不宣称文件系统多文件原子性
- authoring 基线、受影响源码/sidecar、只读与内容错误保持 core 诊断；不能在命令层绕开计划重验、自动升语言或补 token

有状态 RPC 新增：

| 方法 | 参数 | 结果 |
|---|---|---|
| `localization.catalog` | `{project_id,request}` | `{ok,page,baseline,workspace_diagnostics,read_only}` |
| `localization.ids.preview` / `.apply` | `{project_id,request,plan_digest?}` | `{ok,operation,plan,changed_files?,new_baseline?,applied,saved:false,baseline,workspace_diagnostics,read_only}` |
| `localization.edit.preview` / `.apply` | 同上 | 同上 |
| `localization.import_candidate.preview` / `.apply` | 同上 | 同上 |

apply 成功的 `changed_files` 与 `new_baseline` 位于响应顶层；preview 不返回这两个字段。

上述方法只针对已打开 Project，读取当前已应用内存稿，不隐式刷新、应用正文 WritingBuffer 或保存。apply 需要 digest，preview 禁止 digest；未知参数拒绝。成功候选仍留在原 `project_id` 下，由既有 `project.save` 显式保存。请求结构错误为 `-32602`；合法 DTO 的旧计划、预算、只读、缺失身份、源文修订或 token 失败为 `ok:false` 的故事层结果。initialize 公开 `authoring.localization_workbench.v1` 能力。

## 2. 真实 locale 播放

```text
wl play <目录或入口> --locale <目标语言> [--locale-fallback source] [既有播放参数]
```

未提供 `--locale` 完全保持原来 source-only 路径。提供 locale 时默认严格策略；只有显式 `--locale-fallback source` 才请求按 runtime 规范使用可辨认的源文回退。fallback 必须与 locale 同用；未知值、重复参数或在非 play 命令使用这些参数是用法错误。准备由当前 Project 生成验证后的只读 presentation 快照，并在初始化和任何执行前验证与编译产物一致。准备失败不开始会话、不推进随机流。

`--save`、`--load` 和路径 trace 沿既有命令边界工作，但 locale 身份按 runtime 契约严格绑定。读取 locale 存档必须显式选择同一 locale/回退请求且当前译文身份匹配；无 locale 的旧调用不能静默载入成源文。源文默认 JSON 输出保持旧形状；locale 输出只增加 runtime 生成的可选本地化元数据，包括同次物化的源文显示、来源及回退原因。

`wl replay` 从明确提交的 trace 中读取 presentation 请求，在当前工程重新准备相同快照，再调用带 presentation 的 runtime 重放入口。locale/digest 不匹配或不可准备时明确失败，不自动降级为源文、不通过展示替换蒙混 observation 比较。

既有 `wl route-compare`、`wl playthrough-report` 与对应 RPC 同样从 trace 选择经过验证的 locale 入口。双路线必须使用相同 presentation 身份；不同语言/策略或源文与译文的路线不能混成同一个展示重放。源译文字对照来自一次运行输出中的同次物化值，无需伪造两条轨迹。报告逐条标明回退状态，不自动增加译者包未包含的作者私密数据。

## 3. RPC 会话与编译快照

`compile` 新增互斥的 `{project_id}` 输入形式，从已打开工程当前应用稿生成 story；原 `{path}` 和 `{source,file_name?,language_version?}` 形式继续有效。`project_id` 不刷新或保存。编译自工作区时 story 同时保留该次 Project 的只读副本用于随后准备 locale，避免在 `session.open` 随意重新读磁盘造成源码与译文时间点不一致。直接 source 编译没有 sidecar 工作区，不能假装具备 locale。

`session.open` 可选 `localization`，其结构为 core `LocalizationPresentationRequest`，并须在 `capabilities` 中显式申请 `runtime.localization.v1`。没有 localization 时不启用 locale；请求 locale 但缺少能力、没有工作区、准备失败或存档身份不匹配，均不创建 session。合法请求的译文或存档失败返回 `ok:false`；参数形状/能力协商错误为 `-32602`。响应返回实际协商能力及绑定的 presentation 身份。

`session.continue` / `session.choose` 序列化同一个 runtime 输出；`session.save` / `.checkpoint` / `.trace` 保留 locale 身份；restart 使用同一冻结 presentation。`trace.replay` 按 trace 请求从 story 的冻结 Project 准备 locale，再验证内容身份。所有源文旧入口、稳定 choice 身份、seed、运行副作用和旧错误分域不变。

## 4. 必测兼容性

- 原 export/import CLI 与 RPC 仍可完成既有持久化流程，未选 ID 和未知作者字段保持
- 新目录/ID/edit/candidate-import preview 和默认 apply 零磁盘写；显式 save 后重开字节正确
- 重复 JSON 键、未知参数、超预算、旧 digest、只读、外改保存冲突与错误阶段清楚
- 同 seed 两个随机插值反序翻译后，source/译文结果、状态、choice IDs、保存恢复及重放一致
- locale 保存/trace 在缺省 source-only 入口拒绝，改变译文后的身份不匹配拒绝，失败无额外执行
- dirty Project 经 RPC compile 使用其应用稿；此前创建的 story 保持旧快照并可清楚重开

本文件描述产品契约，不代表当前候选已运行通过或已经发布。
