# 本地化字符串交换

**版本：**1（`content.localization.v1`）  
**状态：**CAP-09B 批准范围 A。此契约交换作者明确选择的台词/选择译文，不提供 runtime locale、自动回退、机器翻译或引擎适配。

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

非空 `localizations` 必须声明 `content.localization.v1`；路径须工作区内、`.json` 扩展名，且不得与清单或其他注册文档共用。首次导入使用 `.world/localization/<target_locale>.json`；已登记 locale 沿用清单原路径。sidecar schema 为 `1`，包含 `required_features`、`source_locale`、`target_locale` 及按 ID 索引的 `entries`；每项持久化 `source_revision` 与 `translation_parts`。它是作者文件，进入 Project content baseline 与工程备份，但不参与运行 fingerprint，也不由 runtime 读取。未知格式/能力遵守 Project 注册文档的只读保护。

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
