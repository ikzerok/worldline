# 语言版本与机器 Schema 一致性

默认源码编译和无 language_version 的旧工程仍使用 1.9。产品 0.10 不自动迁移作品；产品版本与 DSL 版本独立。显式支持的语言集合由 core `LanguageVersion::SUPPORTED` 表示，目前为 1.9、1.10、1.11、1.12、1.13；manifest、CLI、RPC 共用其解析，不放宽未知版本。机器 `project.schema.json` 的枚举必须与集合逐项一致，产品回归同时检查序列化/反序列化与真实清单读取，防止再次漏列已支持版本。

1.13 静态时间扩展见 [language-1.13.md](language-1.13.md)。人物强引用见 [character-refs.md](character-refs.md)：除显式 1.13 外，要求既有 `content.object_refs.v1` 与新增 `content.character_refs.v1`；独立源码编译默认关闭两个引用能力，只有调用方明确开启时才接受。未知版本或必需能力的工作区按原始字节只读保留，不能删掉能力字段后继续编辑。

不含新静态功能的旧工程不增加运行指纹盐；静态 Ref 属性、schema 与时间元数据沿用既有 fingerprint 排除规则。人物运行身份的稳定 ID 改名仍遵守既有存档不兼容规则。

## 显式语言与资料能力启用事务（产品 0.14）

本入口只启用已经支持的 1.9–1.13 语法与资料能力，不增加 DSL，也不改变新作品、独立源码或旧工程的默认 1.9。打开、预览和取消均不自动升级。保持当前版本且未增加能力是无变化预览，不创建清单、不允许提交。语言版本与产品版本继续独立。

core `capabilities::language_capabilities()` 提供版本、中文名称、既有语义说明和该选项追加的准确 `required_features`；`feature_capabilities()` 提供可显式追加的资料能力、最低语言版本和依赖。1.10/1.11 选项声明 `content.entities.v1`、`content.relations.v1`；1.12/1.13 还声明 `content.choice_presentation.v1`。这些声明保护旧客户端，不意味着启用后自动创建对象、关系或选择。对象强引用需 `content.object_refs.v1`；人物强引用另外需 1.13 和 `content.character_refs.v1`；本地化身份使用既有 `content.localization.v1`。规则、片段、集合、台词、schema 和同根跨时段偏序仍由对应显式语言版本控制，不把运行存档能力加入工程清单。

`Project::plan_capability_enable(CapabilityEnableRequest)` 接收目标版本、只增不减的 `enable_features` 和 `expected_baseline`。低于当前语言版本、新增未知/非此入口能力、低于能力最低版本、缺失依赖的组合均拒绝；不静默移除既有能力。已知的其他工程展示能力可随请求原样传回并保留，不能借此入口新增声明或创建其必需的注册文档。未知语言、schema 或 required feature 的工程只读拒绝；整个清单原始字节保持不变，不删除未知能力后重试。

计划以全部已应用活动源码（含已应用但未保存修改）分别按当前与候选选项完整编译，提供前后全部 diagnostics、新增 diagnostics、词法分类改变的真实文件/行/原文、候选清单字节和实际前后运行 fingerprint。归档源码遵守原 source_config 边界，不谎称为运行输入。词法变化列表来自正式 lexer，是辅助定位，不是全文语义等价证明；编译 diagnostics 与兼容说明始终同时提供。新版本可能把旧正文中的关键字解释成语法；源码不自动转义、改写或修复。候选有任何编译错误仍可查看计划，`can_apply=false`，原错误稿保留；编译失败时 fingerprint 比较不作兼容证据。编译成功且指纹相同也只描述这一次候选，不承诺所有升级或后续改稿都不改变指纹。指纹变化明确意味着旧 Story 存档和运行检查点不能直接载入，入口 trace 只能按既有严格契约重新验证，不保证沿用。

计划绑定内容基线、工作区身份和完整预览内容。预览及提交均只读检查全部受控文件保存基线与磁盘，拒绝外部改写/删除、新增未载入源码或清单、未解决保存事务。已有内存脏稿并不自动保存，也不被磁盘覆盖；UI 必须先保护尚未进入 Project 的编辑草稿，不能悄悄批量应用表单。`apply_capability_enable` 重新检查基线、磁盘并重建整份计划；篡改诊断、能力、兼容投影或候选字节均拒绝，零部分提交。

成功只修改 Project 内存中的清单，以一次 Project 快照接入既有撤销/重做，不保存、不迁移存档、不运行故事、不消耗随机数。无清单时显式创建最小清单；已有清单只替换 language_version 的值或追加所需字段/能力，其他字段及其缩进、转义和原始字节不重编码。取消或失败保持文件字节、语言版本、内容基线和 fingerprint；保存仍由用户显式执行并遵守既有保存冲突保护。
