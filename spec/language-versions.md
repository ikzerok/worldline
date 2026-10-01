# 语言版本与机器 Schema 一致性

默认源码编译和无 language_version 的旧工程仍使用 1.9。产品 0.10 不自动迁移作品；产品版本与 DSL 版本独立。显式支持的语言集合由 core `LanguageVersion::SUPPORTED` 表示，目前为 1.9、1.10、1.11、1.12、1.13；manifest、CLI、RPC 共用其解析，不放宽未知版本。机器 `project.schema.json` 的枚举必须与集合逐项一致，产品回归同时检查序列化/反序列化与真实清单读取，防止再次漏列已支持版本。

1.13 静态时间扩展见 [language-1.13.md](language-1.13.md)。人物强引用见 [character-refs.md](character-refs.md)：除显式 1.13 外，要求既有 `content.object_refs.v1` 与新增 `content.character_refs.v1`；独立源码编译默认关闭两个引用能力，只有调用方明确开启时才接受。未知版本或必需能力的工作区按原始字节只读保留，不能删掉能力字段后继续编辑。

不含新静态功能的旧工程不增加运行指纹盐；静态 Ref 属性、schema 与时间元数据沿用既有 fingerprint 排除规则。人物运行身份的稳定 ID 改名仍遵守既有存档不兼容规则。
