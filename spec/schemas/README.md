# worldline 展示契约 Schema

本目录是 worldline 共同展示与文件契约的唯一机器格式真源。Schema 描述设计目标，不代表当前 Rust 客户端已经实现全部能力；能力协商和跨文件引用约束见 [共同契约](../presentation.md)。

JSON Schema 采用 2020-12 语法。已实现的清单、地图、独立关系、模板、共享网络视图与展示预设需同时满足本目录形状和核心语义验证；Schema 本身仍不能替代产品行为验收。地图已支持点、折线及简单多边形，遇未知必需能力只读。

relation.schema.json 对应语言 1.10 的 Catalog.relations 单项 SemanticRelationInfo：来源说明可为 null，properties 为非执行字面量；版本由查询外层携带，仍接受早期样例的逐项 schema_version。关系真源是 .wl 声明，不另存一份关系 JSON；地图/图布局 JSON 才是持久展示配置。

Schema检查形状、基础类型与数值范围；跨文件引用、图层顺序集合、多边形自交、未知必需能力和图片安全预算必须由核心语义验证补充。设计验证器只演示其中的格式/引用/坐标/依赖检查，不实现Rust解析器或图片安全沙箱。

未知可选字段允许保存，生产读取器需要原文/flatten或等价机制保留，不能只依赖Schema允许就声称serde一定保留。

格式Schema覆盖设计目标，不表示M1客户端具备全部能力。清单required_features还须校验客户端能力：点标记为`presentation.maps.v1`；线面额外要求`presentation.geometry.line_area.v1`，不支持者只读。语义实体/关系须明确启用语言1.10及对应内容能力；本包关系DTO用于查询输出的契约验证。
