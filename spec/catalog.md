# 标签与创作素材

**版本：**1.9。此目录是作者的世界资料索引,独立于播放流程。

## 1. 标签定义与标记

```wl
tag coordinate as "坐标"
  description "世界中的已定义位置"
tag harbor_place as "雾港码头"
  property x = 120
  property y = 36

mark tag harbor_place with coordinate
mark event arrival with harbor_place
mark event farewell with harbor_place
mark character lin with harbor_place
```

tag 的语义类似指向对象的引用集合:存储稳定对象 ID,不复制内容,可通过其他 tag 间接引用。
`tag <ID> [as "名称"]` 是全局唯一声明,可带 description 与字面量 property。
`mark <类型> <对象ID或带引号路径> with <标签ID列表>` 是顶层标记,可放在任何被 include 的文件。
类型包括 anchor、state、event、scene、character、world、storyline、period、variable、tag、asset、file。
同一对象可被多个标签引用,标签也可以被标签引用。重复标记合并为集合。
文件对象的路径相对于标记所在文件,必须指向本工程已引用的源码。

正文与属性属于所在完整对象,不创建独立可寻址片段。正文已有 `文字 #标签` 继续保留输出元数据语义,
目录将其归到所在完整事件或场景,保留正文行作为引用位置。旧正文标签允许不声明;目录显示为隐式标签。
对结构对象使用 mark 则必须引用已声明标签。
普通文本中的引用、条件和人物属性不能由标签自动改变。

索引区分直接命中和递归命中。查询 coordinate 的直接标签对象数量等于已标记的位置数量;
递归查询继续沿命中的标签查找关联事件、人物、素材等完整对象。访问集合去重,即使标签互相标记也会终止。
递归查询不向对象自动写入祖先标签;删除某个分类关系只改变索引路径。
所有查询保留来源文件和行号,可定位标记或原始声明。

## 2. 工作区图片与音频

```wl
asset harbor_art image "assets/harbor.png" as "码头概念图"
asset lin_voice audio "assets/lin.wav" as "林舟声音参考"
attach event arrival with harbor_art
attach character lin with lin_voice
mark asset harbor_art with harbor_place
```

`asset <ID> <image|audio|file> "路径" [as "名称"]` 声明全局唯一素材。file 可引用其他格式的设定资料。
`attach <对象类型> <对象ID或路径> with <素材ID列表>` 将素材关联到任意上述对象。
路径相对于 asset 声明文件解析，必须位于工作区内，禁止绝对路径与远程网址。外部素材先复制进工作区再关联。点击打开时交给系统关联应用；浏览器下载已导入的原始素材。

打开工程允许缺失素材继续编辑，core 产生 A215；越界引用产生 A109。导出要求声明的素材存在且可读取，完整复制工作区全部文件（含未引用素材），保留名称与目录，源码采用当前缓冲。另存遵守目录边界，允许未完成源码。不会自动覆盖 README 或生成 spec 目录。详见 [workspace.md](workspace.md)。

## 3. 校验与兼容

- 标签/素材重复 ID: A104;属性重复: A212。
- 标记/附件引用未知对象、标签或素材: A214,错误阻止完整导出。
- 不存在、不可读取或类型与扩展名不符的素材: A215 提醒,完整导出会拒绝。
- tag/asset/mark/attach/anchor_def/anchor_link 不直接改变运行时、存档指纹、事件先后关系。
- 结构检查覆盖 ID、引用、时间约束及属性定义的一致性;不会自动判定任意自然语言设定是否互相矛盾。

CLI 的 `catalog` 与 agent 的 `analyze.catalog` 提供对象、标签、素材、标记与附件及源位置。
`catalog --tag ID --recursive --json` 返回去重后的对象索引,`--kind tag` 可统计分类下的位置等标签对象。

状态作为完整对象，可被标签标记、关联文件，状态的内容标签与分类标记相互独立。状态引用的标签必须显式声明；详见 [states.md](states.md)。

## 4. 独立叙事锚点

```wl
anchor_def turning_point as "读信后的决定"
  description "林舟开始把港口的命运看作自己的责任。"
anchor_link turning_point character lin
anchor_link turning_point event arrival
anchor_link turning_point state mood
mark anchor turning_point with harbor_place
attach anchor turning_point with harbor_art
```

`anchor_def ID as "名称"` 是顶层声明；可选缩进块只允许一个 `description "叙事意义"`，不接受 property。锚点 ID 在全工程的锚点命名空间唯一，与名称分离。`anchor_link ID KIND TARGET` 可放在任意引用文件，关联完整 character/event/state/anchor 对象；两端均须存在，重复锚点与未知关联对象报 A217。标记/附件本身的未知引用继续使用 A214。

`catalog.anchors: BTreeMap<String, AnchorInfo>` 保存 `id/display/description/file/line/links`；每条 `AnchorLink` 含 `anchor/target/file/line`。通用 objects、references、标签查询与附件查询均包括锚点。`Catalog::anchors_for(&TargetRef)` 反查直接关联某角色、事件、状态或锚点的独立锚点；不隐式沿角色的全部事件扩散关联。

`Catalog::anchor_changes(id)` 返回 `Vec<(&StateInfo, &StateChangeSite)>`，只取关联状态中属于关联事件的动作，包括嵌套条件、场景和效果；源码文件与行号用于定位，持久身份仍是对象 ID。无状态或无事件关联时返回空。不会因链接到另一个锚点而自动复制其变化。

结构编辑使用 `AnchorDraft { id, display, description, targets: Vec<TargetRef> }`，前三项为 String。`Project::write_anchor(original, &draft)` 创建或编辑资料及关联，`Project::set_anchor_links(id, &targets)` 替换全工程直接关联；调用方用 `Project::edit` 校验并提交交易。已有 ID 不可由表单改名，注释保留，引用失败整批回滚；显示名称可以修改。

独立锚点描述作者定义的意义，不执行动作，不制造时间顺序，不改变存档指纹。正文旧 `anchor "名称"` 继续生成手动演练记录；`analysis.anchors` 与运行时 `state.anchors` 不自动转为独立对象。

## 5. 别名与正文对象链接

顶层 `alias character lin as "阿舟"` 为完整对象增加查找名称。目标类型与 mark 相同，文件目标使用带引号的工程源码路径，相对于声明文件解析。可跨文件声明；相同对象的重复别名合并显示，不同对象允许同名，搜索返回全部候选。别名不能为空白或包含换行，不产生新对象或运行状态；目标不存在报 A218，格式错误报 P004。

正文及选择文案支持 `[[character:lin|阿舟]]`，也可链接 tag、event 等完整对象。类型与目标 ID 用冒号分隔，竖线后必须明确写出显示文字。scene 使用限定 ID，file 使用相对于所在源文件的已引用源码路径。显示文字不包含方括号、竖线、花括号、反斜杠、双引号、#、~、换行及注释分隔符；格式错误报 P004，未知目标报 A218。写 `\[\[` 可输出普通双左方括号；选择引号内按字符串规则写双反斜杠。属性和说明中的相同文字不自动建立正文链接。

播放只输出显示文字；链接不执行目标、不改变时间、准入、人物在场或状态。修改目标显示名、别名或链接目标身份不改变既有文字；新增／删除链接但保留相同显示文字也不改变存档指纹。显示文字本身变化仍改变指纹。人物 ID 的结构改名同步别名目标和正文／选择显式链接，保留显示文字、普通同名提及与注释。

`catalog.aliases` 保存 target/name/file/line；`catalog.text_links` 保存 source/target/label/file/line/column，source 是所属完整事件或场景。正文链接同时进入 references（kind 为“正文链接”）；多个位置可分别定位，搜索对象按类型与 ID 去重。核心提供按名称、ID、别名筛选的对象查询；同名候选不自动合并。

编辑器提供人物常用属性栏目（外貌、经历、性格、动机、底线、口吻及例句）和字符串多行编辑，均为可选静态资料；既有字段不覆盖、不强制填写、不自动变为状态或事件。长文本沿用字符串的换行转义，不另存一份人物卡。

对象资料阅读页从当前工程快照汇总说明、属性、别名、关系、事件、状态初始定义及源码变化、锚点、引用与素材路径。源事件的变化分开列出条件与出处，不计算唯一当前事实；文件继续交系统应用打开。正文中的对象链接可点击阅读目标，资料页不是第二份可编辑真源。
