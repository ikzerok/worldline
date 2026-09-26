# 工程模板 v1

工程模板是作者资料表单的描述，不是实例的语义来源。内置目录仍由
[`examples/templates.catalog.json`](examples/templates.catalog.json) 定义；已有
16 个内置模板 ID（如 `template_place`）保持不变，工程模板使用
`project:<ID>` 命名空间，不能覆盖或改写内置项。工程清单 `.world/project.json`
的 `templates` 对象把稳定模板 ID 映射到工作区内相对 `.json` 路径。非空注册必须
声明 `required_features` 中的 `content.templates.v1`。每个文档只定义一个模板，注册
键必须等于文档 `id`；注册路径不得与清单或其他展示文档重用。

工程模板文档的 `schema_version` 为 `1`，包含 `id`、`title`、`applies_to`、`fields`。
`applies_to.kind` 为 `world`、`character` 或 `entity`；只有 `entity` 可指定
`entity_type`。字段具有稳定 `id`、显示 `label`、`type` 和可选的实例 `key`；除
`group` 外必须提供 `key`。`required` 表示模板表单完整性提示，不会自动补值或改写
现有实例。字段 ID 在整个模板中唯一；实例 key 在模板中唯一，包括不同分组。

字段 `type` 为 `text`、`number`、`boolean`、`enum`、`object_ref` 或 `group`：

- `text`、`number`、`boolean` 分别对应字符串、有限数值和布尔属性。
- `enum` 的 `choices` 必须非空、无重复且不含空白项；实例值仍是字符串。
- `object_ref` 必须指定 `target.kind` 为 `entity` 或 `relation`；可选
  `target.entity_type` 仅用于 `entity`。v1 限制为 core 可完整重命名和保护删除的目标类型。
  实例必须使用下文的显式 `ref("kind", "id")` 属性值；普通字符串即使内容相同
  也不是对象引用。默认值以 `{"kind":"entity","id":"harbor"}` 的 TargetRef
  形状保存，并且目标必须存在且符合 `target` 约束。引用按完整 `TargetRef` 参与验证、
  目录、重命名与删除影响。其他 TargetRef 类型暂不支持模板对象引用，必须由 core
  定位诊断拒绝。
- `group` 仅是界面分组，不对应实例属性；必须有非空 `fields`，不能有 `key`、
  `required`、`choices`、`target` 或 `default`。子字段继续遵循相同规则。

字段可选包含 `default`，其类型必须符合字段类型（`enum` 默认值必须属于
`choices`）。默认值只作为表单提示，不代表实例属性存在；未设置属性、显式
`null`（若其他资料来源保存了它）和模板默认值是不同状态。模板 API 不写入实例，
也不把 `null` 转换成默认值。未知可选字段必须保留；未知 schema 版本或未知
`required_features` 使该文档只读，并保留原始字节。

导入、替换和删除必须先生成只读影响预览。预览说明字段增删、稳定 ID 对应的改名或
类型变化，并列出适用实例及字段值状态；它不修改 Project、源码、别名、属性或保存
基线。应用操作提交预览时的完整 Project 基线；基线过期、外部保存基线变化、注册或
文档只读时失败，不能部分提交。替换保留原文档中未知可选字段；删除只移除模板注册
和模板文档，不改写实例。类型变更、字段改名和缺失值不触发隐式强转或迁移。

语言属性 `object_ref` 是 1.10 的新增显式值，要求 Project 清单声明
`content.object_refs.v1`。在 1.10 工程中显式导入包含该字段的模板时，预览/应用会一并
加入此已知能力；1.9 工程拒绝导入此类型字段，且已注册的该模板只读：

```wl
entity harbor kind place as "雾港"
character navigator as "领航员"
  property home = ref("entity", "harbor")
```

目标 kind 和 ID 是字面字符串，不做表达式求值；v1 的 kind 只允许 `entity` 与 `relation`。
core 只在 `ref(...)` 值上建立强引用；
字符串属性不会被模板定义、字段同名或内容猜测提升成引用。缺失目标产生语言诊断，
并出现在引用影响、重命名和删除保护中。该属性值不进入运行指纹。1.9 不支持此值。
对象引用字段仍只是编辑提示：本票不提供可执行规则、远程资源、自动默认值、自动
迁移或自动填充。

旧 1.9/1.10 工程没有模板注册时保持原行为；模板注册不改变 `language_version`。
内置目录现有字段 `widget`（如 `multiline`）与工程模板的稳定字段 `type` 是不同
契约，不隐式迁移内置模板 ID 或旧模板目录。模板诊断 code 使用 `TPL` 前缀，message
使用中文。
