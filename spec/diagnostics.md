# worldline 诊断契约

**版本:** 1.9
**生产者:** `worldline-core`(唯一分析真相)
**消费者:** `wl` CLI、worldedit、测试

---

## 1. 结构

```rust
Severity ::= Error | Warning | Hint

Diagnostic {
  severity: Severity
  code: String          // 见 §2 编号表
  message: String       // 中文,面向作者
  file: String          // 源文件路径(include 场景下指向真实文件)
  span: Span { line: u32, column: u32, length: u32 }  // 行/列均 1-based,长度按字符数
  note: Option<String>      // 补充说明
  suggestion: Option<String> // 可选修复文本(编辑器可做一键替换)
  related: Vec<(String, Span)> // 关联位置,如重复符号的另一处
}
```

排序规则:severity 降序 → 行号 → 列号。同位置多诊断按 code 升序。

## 2. 编号表

### P0xx — 词法/语法(error)

| code | 场景 |
|---|---|
| P001 | 非法 token / 未知字符 |
| P002 | 缩进非法:Tab 空格混用、子句对齐错误 |
| P003 | 字符串未闭合或非法转义 |
| P004 | 期望 token 缺失(如 `choice` 后缺标签) |
| P005 | 块为空或声明后无缩进体 |
| P006 | 表达式语法错误 |
| P007 | include 路径语法错误 |

### A1xx — 符号与引用

| code | severity | 场景 |
|---|---|---|
| A101 | error | 跃迁目标不存在(unknown symbol) |
| A102 | error | `set`/引用未声明变量 |
| A103 | error | 条件/操作数类型不匹配(含非布尔条件) |
| A104 | error | 重复符号定义(事件/场景/变量) |
| A105 | error | include 环路或文件缺失 |
| A106 | error | 对 const 赋值 |
| A107 | warning | `let` 变量从未使用/读取 |
| A108 | hint | 变量只写不读,或遮蔽同名字段(预留) |
| A109 | error | 工作区引用使用绝对路径或越过目录边界 |

### A2xx — 流与结构

| code | severity | 场景 |
|---|---|---|
| A201 | warning | 非时段事件/场景在执行图中不可达;时段世界记录不要求接入执行流程 |
| A202 | warning | 非时段事件体结束无跃迁(视同 END,但建议显式 `-> END`) |
| A203 | warning | 选择组所有分支带 `if`,存在全部落空即直接穿组的风险 |
| A204 | hint | 事件仅有跃迁无文本("走廊事件") |
| A205 | warning | `once` 选择出现在不可重访的事件中(等价于粘性) |
| A206 | warning | 自跃迁(`-> self`)且无条件保护,疑似死循环 |
| A207 | hint | 重复的选择标签(同组内文案重复) |
| A208 | error | 引用未定义角色(`with` / `meet` / `part`) |
| A209 | warning | 漂流跃迁 `->>` 目标与当前节点在同一故事线(应使用 `->`) |
| A210 | error | 引用未定义故事线(效果动作 `to` 的目标不存在) |
| A211 | error | 合并后的工程包含多个世界观声明 |
| A212 | error | 同一对象内属性名重复,或人物关系目标与标签重复 |
| A213 | error | 时段或前驱事件不存在、跨时段约束、时间先后约束自依赖或有环 |
| A214 | error | 标签标记或附件引用了不存在的对象、标签或素材 |
| A215 | warning | 素材缺失、不可读取或类型与扩展名不符;完整目录导出会拒绝 |
| A216 | error | 状态 ID 重复、目标或标签不存在、状态变更引用无效 |
| A217 | error | 独立锚点重复，或 anchor_link 引用了不存在的锚点／对象 |
| A218 | error | 别名或正文对象链接的目标不存在 |
| A219 | error | 时段的上级不存在、自包含或包含关系有环 |
| A220 | error | 关系类型或关系资料重复、属性重复或声明结构无效 |
| A221 | error | 关系实例引用了未定义的关系类型 |
| A222 | error | 关系端点、scope 或端点约束无效 |
| A223 | error | 关系实例 ID 重复 |

### WS00x — 工作区注册

工作区清单诊断保留在工作区域，不混入 `.wl` 编译诊断：

| code | severity | 场景 |
|---|---|---|
| WS001 | error | 清单 JSON 无法解析或顶层结构错误 |
| WS002 | error | 清单 schema 版本不受支持 |
| WS003 | error | 清单必需能力未知或格式错误 |
| WS004 | error | 清单注册项路径、类型或边界无效 |

### MAP00x — 地图展示文档

地图解析由 `worldline-core` 产生，文件原始字节仍由 `Project` 保存。地图诊断
不改变 `.wl` 内容诊断，也不把展示文档加入 `Program` 或运行指纹：

| code | severity | 场景 |
|---|---|---|
| MAP001 | error | 地图 JSON、顶层结构或字段类型错误 |
| MAP002 | error | 地图 schema 版本或必需能力不受支持，只读查看 |
| MAP003 | error | 地图 ID 无效、与注册 ID 不一致或重复 |
| MAP004 | error | 画布尺寸、单位或范围无效 |
| MAP005 | error | 图层、图层顺序或图层引用无效 |
| MAP006 | error | 标记几何、坐标、范围或多边形拓扑无效 |
| MAP007 | error | TargetRef 结构或 kind 无效；未解析对象另作 warning |
| MAP008 | error | 栅格图层结构、矩形或 asset 引用格式无效 |
| MAP009 | error | 导航结构或地图 ID 格式无效 |
| MAP010 | warning | asset 未声明、不是 image、缺失或不可用；地图仍可读 |
| MAP011 | warning | TargetRef 对象尚未在内容目录中解析；地图仍可读 |
| MAP012 | warning | 导航目标尚未注册；地图仍可读 |

### GRAPH00x — 共享网络视图

网络布局由 `worldline-core::graph_views` 读取；以下诊断不加入故事 Program 或运行指纹。
原始字节始终由 Project 保留，未知格式或必需能力不允许覆盖或删除。

| code | severity | 场景 |
|---|---|---|
| GRAPH001 | error | JSON、重复键、结构、坐标、身份或路径复用错误 |
| GRAPH002 | error | 内容目录完整时，视图引用的对象或关系类型确认不存在 |
| GRAPH003 | error | 视图 schema 版本或必需能力不受支持，只读保留 |
| GRAPH004 | warning | 内容本身有错误，视图引用暂时无法解析，不能视作已被删除 |

删除影响检查合并内容、地图和网络诊断；内容有错误时，即使 GRAPH004 为 warning，
整体引用检查仍不完整。关系类型的筛选引用也阻止类型删除，不以当前是否有实例为依据。

## 3. 机器接口

`wl check --json` 输出:

```json
{ "ok": false,
  "stats": { "events": 12, "scenes": 5, "choices": 20, "words": 1834 },
  "diagnostics": [ { "severity": "error", "code": "A101", "message": "...",
      "file": "story.wl", "span": { "line": 14, "column": 8, "length": 7 },
      "note": null, "suggestion": null, "related": [] } ] }
```

severity 序列化为小写字符串。编辑器据此渲染面板并跳转。

其余子命令(`graph`/`timeline`/`play`)的 JSON 模式与有状态机器协议
(`wl-agent`)见 [agent-protocol.md](agent-protocol.md)。

## 4. 原则

- 诊断只产自 `worldline-core`;CLI 与编辑器只做格式化、过滤、跳转。
- 同一事实只报一次:重复符号在两处都报,但互为 `related`。
- 语言编译的 error 必须阻止运行；展示文档的 MAP/WS 错误单独报告，不阻止无关故事运行；warning/hint 不阻止。
- 消息一律中文,含符号名与行号定位描述;修复建议给到可直接替换的文本。

状态操作与旧权限归一规则见 [states.md](states.md)；独立锚点见 [catalog.md](catalog.md) §4。标记/附件的未知引用使用 A214；执行图回环不使用时间环错误 A213。
