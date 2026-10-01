# 静态人物强引用（显式语言 1.13）

本契约只新增静态资料值的 `character` 目标，不引入动态说话者、人物运行值或全部 TargetRef 类型。

## 启用与旧客户端保护

工程必须同时显式选择 `language_version: "1.13"`，在清单 `required_features` 声明
`content.object_refs.v1` 与 `content.character_refs.v1`。独立编译必须显式打开对应
CompileOptions；默认关闭。旧版本即使误带能力也拒绝人物属性值与人物 schema ref，
不自动升级。未知能力/语言版本继续按原始字节只读保留，允许救援副本，不允许编辑覆盖。
既有 entity/relation ref 的语言与能力门不变。

## 值、约束与作者界面

```worldline
schema vessel for entity entity_type ship
  field captain_id captain ref character required
character lin as "林舟"
entity boat kind ship as "渡船"
  property captain = ref("character", "lin")
bind entity boat to vessel
```

`PropertyValue::Ref(TargetRef)` 直接保存唯一人物 ID，序列化仍为 `ref("character", "ID")`。
两个参数必须为静态字符串字面量；不存在的目标报 A214，重复人物仍报既有重复身份诊断。
`ref character` 不接受 entity_type；类型不符报 SCH007，字符串值报 SCH005，缺少必填
值报 SCH004。普通字符串（即使恰好等于人物 ID）不会被自动转换，也不参与身份改名。

工程模板 `object_ref` 的 `target.kind` 可为 character；相应清单必须满足上述三门，
模板文档自身须声明 `content.character_refs.v1`，以便独立传递给旧客户端时只读保留。
缺门模板以 TPL005 只读保护。人物默认值必须指向已存在的人物；否则 TPL006。
人物模板目标不接受 entity_type。界面复用 core 模板类型与目录，只提供人物候选，
选择写入真正的 Ref 值；不把已有字符串或错误 kind 偷偷强转。通用属性编辑同样使用
core 允许的 kind 和目录候选。模板与 schema 切换不补值、不删除未知字段。

## 统一生命周期

目录和反链记录来源对象、人物 TargetRef、源码文件与行号。CLI/RPC 的目录、资料值、
查询和影响投影保持相同结构，不维护第二套解析器。删除影响包含人物 property ref；
删除后若仍有引用则编译报错，不显示为有效的 typed slot。

人物稳定 ID 的跨文件 RenamePlan 同时改写声明、property Ref、关系端点、event with、
正文显式人物链接、已支持的静态台词及注册文档结构引用；普通文字、属性字符串、显示名、
链接 label、备注和别名显示文本保持原样。预览不修改工程；取消不写入；应用绑定完整内容
与磁盘基线，过期或冲突整批拒绝。成功后可用 Project 快照撤销；保存/重开保留类型与 ID。
完整工程导出与保存遵循原工作区边界，未知文档仍按字节保留。

静态 Ref 属性和 schema/template 改动不进入运行 fingerprint。人物本身稳定 ID 改名仍
遵循既有 runtime 身份规则，可能使旧存档不兼容；不得声称人物改名可保留旧存档。
不改变 runtime speaker、参数、局部值、状态、选择、访问次数或执行路径。

## 发布与验证

读者导出仍使用显式对象/字段披露白名单。选择一个带人物 Ref 的对象不会自动公开人物、
其传记或其他资料；只有目标也明确公开才显示该引用。完整备份与读者发布的边界不同。

验收涵盖三门组合、旧 entity/relation、missing/duplicate/wrong kind、字符串不转换、
跨文件反链/改名/删除、预览/取消/过期/磁盘冲突/撤销/保存重开、静态指纹不变、人物改名
指纹改变、模板字段/默认值/旧能力只读、UI typed picker、读者披露边界。
