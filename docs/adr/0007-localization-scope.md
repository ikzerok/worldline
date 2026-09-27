# ADR-0007：本地化字符串身份与翻译往返范围

- 状态：已决定（用户已批准选项 A）
- 关联：ikzerok/worldline#26（CAP-09B）、ikzerok/worldline#24（已关闭，不提供游戏引擎适配器）

## 背景

CAP-09B 起初缺少获批的字符串身份和翻译保存生命周期。当前 `TextStmt` / `ChoiceStmt` 没有持久字符串 ID；源码位置、事件/场景名、选择顺序都会因移动或重排改变，不能单独充当稳定 ID。`Project::content_baseline()` 绑定工作区当前内容；运行 fingerprint 则绑定可见原文及插值表达式，另有不同用途。两者不能被假定为同一个翻译修订号。

正文和选择中的显式链接具有目标 `TargetRef` 与可译显示文字；插值由 AST 表达式表示。当前语言只有一份作者原文，没有 locale 选择、翻译覆盖层或运行时语言回退契约。`Project` 有基线保护及可恢复保存事务，但跨文件文件系统原子性不作保证。

依赖 #24 已决定不选引擎、不制作引擎产物或适配器。这不决定翻译数据是否参与运行时输出，也不批准任何自动 ID 派生或自动选择内容策略。

## 决定

用户批准选项 A：采用翻译交换与工程内 locale sidecar，不改写 `.wl` 原文，不增加运行时 locale、回退或引擎适配器。

- `.wl` 文本行和选择标签可在行尾声明作者维护的 `#wl-localization:<id>`。ID 项目内唯一、随片段移动，不由内容、路径或位置生成；不自动补建。
- 导出必须携带显式 ID 白名单。仅导出白名单中的源文、相对来源位置和必要的受保护 token；不沿 include/链接扩展，不默认导出说话者、相邻文案或链接目标资料。
- 交换文件是版本化 UTF-8 JSON。插值与显式链接用不可编辑 token 表示；链接显示文字可译，链接目标不进入交换文件。译文作为 `.world/project.json` 注册的独立 locale sidecar 保存。
- preview 报告缺译、重复/未知 ID、token/版本错误及源文过期；过期标记为 `needs_review`。任一错误、过期项或 Project 基线不匹配均使整批 apply 失败。apply 重算预览与基线后，在同一 Project 候选/可恢复保存事务中写入清单及 sidecar；不宣称跨文件系统原子性。
- 每个字符串的 source revision 与 Project `content_baseline` 分离。源文或插值/链接结构变化标记过期；移动/改名本身不重建 ID。身份注记、locale sidecar 不改变运行 fingerprint，原文仍按既有规则参与 fingerprint。

## 后果

作者须显式维护 ID；缺少 ID 的字符串不能加入导出，复制片段时重复 ID 会使涉及该 ID 的往返失败。译文能安全往返并保留多 locale，但在后续获批 runtime 能力前不会被 `worldline-runtime` 消费。源文替换、自动 ID 生成、默认公开选择、引用闭包、机器翻译与所有引擎集成都不在本决定内。实现细节和 CLI/RPC 形状以 `spec/localization.md` 与 `spec/agent-protocol.md` 为准。
