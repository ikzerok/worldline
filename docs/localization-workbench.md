# 译文制作与真实体验接入

0.33 将已有本地化交换接入目录巡检、直接译文草稿和真实运行。所有源码提取、状态、token 校验、计划和运行展示由 core/runtime 提供；调用方只选择明确范围、序列化和呈现结果。

## 制作译文

1. 对当前 Project 调用 `query_localization_catalog`，发现正文、say 和 choice 单元；未启用能力时可以查看缺 ID，不会自动启用或分配身份
2. 通过已有能力计划显式启用 `content.localization.v1`。缺 ID 的条目用 `LocalizationIdDraft` 绑定实际来源、修订和预期旧 ID，先预览再应用
3. 使用 `LocalizationEditDraft` 提交明确的源/目标语言、来源基线和 typed 译文。文字及链接标签可以改，占位和链接 token 必须完整；不把表达式或链接目标交给译者重写
4. `preview_localization_edit` 或 `preview_localization_import_candidate` 返回可审阅计划。apply 重建并核对摘要，只替换内存 Project；保存另行调用 `Project::save`
5. 源文修订后，目录显示过期状态。作者重审并按当前修订重新提交，不能用“已完成”标签抹掉真实过期

新 CLI 使用 `wl localization catalog` 及 `ids|edit|import-candidate preview|apply`，请求通过 `--request-json` 提供。apply 需要 `--plan-digest`，没有 `--save` 就只在本进程内应用，退出后丢弃；RPC 新候选操作留在 `project_id` 下，由 `project.save` 独立保存。旧 `localization import apply` 保持原来立即持久化的契约。

## 真实体验

`wl play <工程> --locale <目标语言>` 请求严格译文。需要明确允许缺 ID、缺译、过期等条目显示源文时，加 `--locale-fallback source`；每条回退都有真实状态。没有 locale 的旧命令保持源文。

runtime 先沿源 AST 顺序把每个表达式求值一次，再按译文 token 次序摆放实际值。对照的源文和译文来自同次求值，不再次运行；选择 ID、条件、随机流、变量、状态、访问和出口不被翻译改写。

locale 存档、检查点和 trace 带独立 presentation 身份。恢复必须提供相同语言、回退策略和译文内容快照；译文已变时应重新录制，不能将旧记录冒充新展示。回放、路线比较和试玩报告使用同一 runtime；双路线要求一致的 presentation 身份。

## 保护与边界

- 正文、say 文字、choice 标签纳入本轮；人物显示名、direction、禁用理由与状态日志仍为源文
- 作者工作台可看到上下文，不代表交换包获得额外公开范围；包只含明确选择的稳定 ID
- 默认语言仍为 1.9，最高既有版本 1.13；不新增 DSL，不自动升级，也不增加独立可玩发布包
- 浏览器使用导入快照和相同内存候选规则，下载/浏览器保存与原生磁盘事务分别看待
- 超预算直接失败，不提供看似完整的截断计数。精确上限、状态优先级和错误代码见规范
- include 展开的正文、say、choice 使用 parser 记录的真实物理来源，相对文件链接也从该文件解析。旧包若曾误记录 event/fragment 声明文件，导入会明确拒绝不匹配来源并提示“重新导出交换包”，不修改原工程或自动重定位。请保留旧包，重新导出后按稳定 ID 核对迁移译文，再预览导入；未受此缺陷影响的旧包沿用原接口和立即保存方式

完整契约：[本地化](../spec/localization.md)、[运行展示](../spec/localization-runtime.md)、[机器接口](../spec/localization-machine.md)。本页不是测试完成或发行状态声明。
