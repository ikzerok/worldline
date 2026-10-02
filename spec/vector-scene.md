# 原生矢量场景与安全 SVG（工具 0.15）

本文是 scene 模型、编辑事务与 SVG 交换的共同契约；实现由 `worldline_core::vector_scene` 提供。作者 UI、读者发布、CLI/RPC 只调用同一 core，不另建 SVG 解析器或可编辑几何真源。本文不改变世界运行、故事存档、DSL 默认 1.9 或最高支持 1.13。

## 1. 格式、身份与单一真源

地图大版本仍为 `schema_version:1`。新增可选 `scene`，存在时同地图的 `required_features` 必须包含 `presentation.vector_scene.v1`。该能力从首个公开版本起包含本文的根 viewport 矩形裁剪；未知 scene schema 或必需能力只读保留原始字节，不能丢字段后回写。没有 scene 的旧地图打开不自动升级；升级是 `EnableScene` 的明确事务。

`MapDocument.scene: Option<MapScene>` 与旧 `placements` 可共存，但每个图形只能有一个持久可编辑表示。节点和旧 placement 在**同地图共同 ID 域**中唯一，不允许相同 ID 同时出现在两处。临时采样、SVG 渲染和选择框是派生投影，不回写代替曲线。原 SVG 可作为不执行的来源信息保存，不能被 UI 直接执行或作为另一真源。

现有 layers、raster_layers、measurement、点/文字/折线/多边形 placements 均保留。跨层按 `MapDocument.layer_order`；每层先按既有稳定顺序绘制 legacy placements，再按 `scene.root_order[layer_id]` 绘制 scene roots。不能用 BTreeMap 字典序替代图层顺序。

## 2. 冻结 DTO

Rust 字段类型以 `core/src/vector_scene.rs` 与 `vector_scene/contract.rs` 为机器单源。JSON enum 使用 `kind` 标签和 snake_case variant，矩阵是六元素数组。

| 类型 | 字段与含义 |
|---|---|
| MapScene | schema_version、view_box `[x,y,width,height]`、preserve_aspect_ratio、root_order、nodes、extra |
| SceneNode | id、name、layer_id、parent_id、geometry、transform、style、visible、locked、clip_rect、target_ref、annotation、role、label_override、navigation、scope_refs、extra |
| SceneNavigation | map_id 与未知可选 extra；可用性从当前注册地图查询派生 |
| Affine | `[a,b,c,d,e,f]`，SVG 列向量约定；`then(rhs)` 是 `self × rhs` |
| TextRun | text、可选绝对 x/y、相对 dx/dy、style、extra |
| SceneStyle | 可选 fill/stroke/stroke_width/opacity/fill_opacity/stroke_opacity/fill_rule/line_cap/line_join/miter_limit/font_size/font_family/font_weight/font_style/text_anchor 与 extra |

geometry 变体为 Group{children}、Point{position}、Polyline{points}、Polygon{points}、Rect{x,y,width,height,rx,ry}、Ellipse{cx,cy,rx,ry}、Path{segments}、Text{x,y,runs}。路径段为 Move{to}、Line{to}、Cubic{control1,control2,to}、Quadratic{control,to}、Arc{rx,ry,rotation,large_arc,sweep,to}、Close。圆与线分别归一为 ellipse 与 polyline，贝塞尔和圆弧仍保存为曲线。

group 的 children 顺序与各节点 parent_id 必须一致；每节点恰好出现一次，无循环、孤儿、重复顺序或跨层 group。ID 与 nodes 的 key 相等。对象引用保留完整 TargetRef，不以显示名替代；annotation、role、label_override、navigation、scope_refs 不因导入/迁移失落。label_override 为空时由调用方依据当前目录投影对象显示名，不能把该派生标签永久复制成正文。

未知可选字段原位保全；extra 不能覆盖已知字段。path segment/geometry 内尚不理解的可选字段通过原 JSON 定位保留。样式继承只合并已知可继承字段，未知 extra 不向所有后代复制。opacity 是节点合成属性，不按普通继承样式累计；group 透明度必须按组离屏合成。

## 3. 坐标、viewport 与最小矩形裁剪

旧 placements 继续使用 normalized 坐标；scene 使用 view_box 中的逻辑坐标，经 root viewBox 矩阵映到 canvas，再由编辑器个人镜头映到屏幕。不得混淆两种持久坐标。Affine 支持 translate、非等比/负 scale、rotate、skewX/Y、matrix 及任意合法组合。viewBox 支持非零 min，preserveAspectRatio 支持 none、九个对齐点、meet/slice。

`view_box_transform(view_box:[f64;4],width:f64,height:f64,aspect:&str)->Result<Affine,SceneError>` 是唯一 viewport 公式。`import_view_transform(source_scene:&MapScene,width:f64,height:f64,target_scene:&MapScene)->Result<Affine,SceneError>` 同时供导入计划和 UI 精确预览使用，不允许 UI 复制一套 fit 公式。

`SceneNode.clip_rect: Option<[f64;4]>` 仅表示导入根 viewport 的矩形裁剪，值为该导入根组局部坐标的 `[x,y,width,height]`，宽高正数。该组以 `extra.svg_root=true` 标识；标识不构成信任，仍验证全部结构、几何和预算。它可以被用户整体移动/缩放或置于其它组内，但不能把它当作任意图形裁剪机制。多个独立 SVG 导入可以各有自己的 viewport 根组。

导入时以源 viewport 矩形经源 viewBox 逆变换计算局部 clip_rect，连同曲线和 group 持久保留；在目标画布 fit 后依然裁剪，尤其 `slice` 不能露出原 viewport 外的内容。仅乘变换却丢失 crop 是错误。

`ScenePrimitive.clips: Vec<SceneClip{rect,transform}>` 提供命中/选择所需的所有祖先矩形及累计 affine；矩形可能在世界坐标旋转。UI 通过逆矩阵测试点，不以扩大后的轴对齐框代替裁剪。projection 的 paths 已是场景坐标；text_origin/runs 是局部坐标，transform 是累计矩阵。实际绘制通过同一安全 SVG serializer，临时采样不得替换合成语义。

## 4. 支持的 SVG profile

接受单根 svg、嵌套 g、rect、ellipse、circle、line、polyline、polygon、path、text/basic tspan。路径支持 M/L/H/V/C/S/Q/T/A/Z 的绝对、相对、重复参数、多子路径与 Close；S/T 展开成明确控制点，C/Q/A 不离散化存储。fill-rule 支持 nonzero/evenodd。text/tspan 的标量 x/y/dx/dy、纯文本和受控样式保持可编辑，不承诺文字路径、任意 CSS 排版或外部字体。

样式只接受受控纯色 fill/stroke、描边参数、opacity、基础字体和文本 anchor。颜色允许 CSS named colors、#RGB/#RGBA/#RRGGBB/#RRGGBBAA 和逗号形式 rgb()/rgba()；不接受 url paint、变量、任意表达式或 CSS selector。inline style 逐属性白名单解析，不能交给浏览器解释用户 CSS。XML predefined/numeric entities 仅作安全文本字符解码。

整批拒绝脚本、事件属性、DTD/entity declaration、外部资源/URL/href、foreignObject、style 元素、filter、mask、动画、未知元素/属性和 nested svg 子 viewport。nested svg 不能扁平化成 g 后假装成功。所有拒绝带原因、节点/属性及可用的 XML 行列，保留原输入，零项目变更。不宣称支持所有 SVG。

### 4.1 自身生成的矩形裁剪交换

唯一允许的 clip 扩展是 serializer 生成的本地根 viewport 矩形，形式为根 svg 下 defs 中的 `clipPath`，`clipPathUnits="userSpaceOnUse"`，内部恰好一个没有 transform/style/children 的 rect；clipPath ID 使用 `wl-viewport-` 加十进制编号。对应 g 具有 `data-worldline-viewport="1"` 和精确 `clip-path="url(#wl-viewport-N)"`。

parser 核验定义、局部引用、唯一性、位置、矩形字段和全部预算，再还原为 typed clip_rect。标记不是来源认证；任何输入只有满足完整白名单结构才可接受。禁止引用其它 ID、网络 URL、任意 path/复合形状、嵌套 clip、objectBoundingBox、额外属性、悬空或重复定义。导出多层片段使用不冲突的安全生成 ID，不使用作者原节点 ID。

安全 SVG 再导入不应不断添加无意义根组。只有无有效样式/变换/裁剪的 synthetic 根容器可透明消除；不能丢失真正的 group 或 viewport。

### 4.2 旧 Rust API 兼容

`svg_import::preview/apply` 保留原 legacy 采样 placements 契约，不自动启用 scene、不静默替换旧持久结构。此接口不承诺新版可编辑曲线保真，新的作者入口须使用 `preview_scene` 与 SceneBatch。旧接口仅优化为一次候选构造/校验/提交，功能范围不被删除。未来改变旧接口需要独立版本契约，本轮不以 deprecated 警告破坏现有消费者。

## 5. 原子 batch

`SceneBatch{map_id,expected_revision:Revision,expected_documents:BTreeMap<PathBuf,String>,operations:Vec<SceneOp>}` 使用既有展示修订与原始文档 hash。操作字段精确见 contract.rs：EnableScene、Insert、Update、Delete、Reorder、Group、Ungroup、Duplicate、ImportSvg、ImportScene、MoveToLayer、MigratePlacements。

```rust
preview_batch(&Project, Revision, SceneBatch) -> Result<ScenePlan, SceneError>
preview_batch_with_control(&Project, Revision, SceneBatch, &SceneLimits,
    &mut dyn FnMut(SceneProgress) -> bool) -> Result<ScenePlan, SceneError>
apply_batch(&mut Project, &mut Revision, &ScenePlan) -> Result<CommandResult, SceneError>
apply_batch_with_control(&mut Project, &mut Revision, &ScenePlan,
    &mut dyn FnMut(SceneProgress) -> bool) -> Result<CommandResult, SceneError>
```

expected_documents 的键按 Project 登记文档使用的路径身份规范化后比对，包括本机路径分隔符、当前目录片段与 Windows 设备前缀等价形式；不以原始 PathBuf 拼写判断是否缺少基线。多个键规范化为同一文档时返回 `SCENE_CONFLICT`，不能选择其中一个 hash；所有基线仍逐一验证，缺失、错误 hash、未注册或越界目标不能因规范化获准。原请求保留，摘要继续绑定原请求。

preview 只读当前缓冲；一次 parse/plan/serialize，构造私有完整候选。apply 重新校验 revision、内容基线、文档 hash、磁盘保存基线及只读状态后一次提交内存，一个 undo record、一次展示修订；不逐图元改写整图。取消、失败、陈旧或磁盘冲突均零修改，不自动保存。

ScenePlan 无 Deserialize，私有 path/before/after/baseline/normalized 字段序列化跳过。公开摘要为 map_id、affected_nodes、affected_refs、diagnostics、expected_revision、document_hash、after_hash、operation_count；可信内存 accessor 为 `document_path()->&Path`、`document_before()->&[u8]`、`document_after()->&[u8]`、`normalized_batch()->&SceneBatch`。

ImportSvg 在 normalized_batch 中替换为已校验 typed ImportScene，不再次 XML 解析。worker 只回传 normalized batch 与原 snapshot/baseline；UI 在当前 Project 重新 core preview/apply 得到本地可信计划，禁止接受 worker/user 任意 after 字节。

`SceneProgress{stage,completed,total}` 回调返回 false 表示取消。limits 调用方只能收紧，不允许扩大硬预算。长任务在后台执行并持续报告阶段；UI 不得在完成之后才显示“处理中”。

纯几何编辑使用 `Project::compile_current()`，不要求故事可运行或全部 `.wl` 编译成功。仅新建/改变的 target_ref 必须解析有效；既有未解析引用保留诊断，不阻止无关几何编辑。

## 6. 编辑、锁与迁移

Group 支持同父同层的非连续选择：保持所选项相对次序，整体置于原最高选中位置。若因此改变与未选项的叠放，计划发出 `SCENE_GROUP_REORDER` warning，列出全部受影响 ID，UI 必须先展示预览再确认提交；不能以连续选择限制替代分组。跨父/跨层选择明确提示先调整结构。

Update 不得隐式改变 parent_id/layer_id；同层顺序用 Reorder，跨层移动用 MoveToLayer。锁包括节点、祖先和层；仅解自身锁也不能绕过上级锁。MoveToLayer 保持 world affine、有效样式和原顺序；不得穿越无法等价保留的 ancestor opacity/viewport 合成边界，需移动完整边界组并明确诊断。Ungroup 若会丢失非单位 group opacity 或 viewport crop，拒绝而非改变画面。

旧 placement 显式 MigratePlacements：保持 ID、typed metadata、styles、extensions/未知可选字段，normalized 坐标按 canvas 变逻辑坐标，移除旧 placement 后加入 scene，不能 copy 两份。批注 MapPlacement 锚点、reader 授权、对象引用和 refactor 定点身份不变。

为保留 legacy→scene paint order，每层只接受整层或原 legacy 顺序的连续后缀；转换节点按原顺序 prepend 到旧 scene roots。任意中段 subset 返回 `SCENE_MIGRATION_ORDER`，提示预览本层全部旧标记；不能扩大用户选择。旧 text 内置字体/颜色与 style 冲突、或旧无效样式迁移后将获得不同语义，返回 `SCENE_MIGRATION_STYLE` 并列出损失，不能偷偷舍弃。

新建地点并绑定已有 scene 节点由 core 组合事务提供：`SceneEntityRequest{expected_baseline,expected_revision,expected_documents,map_id,node_id,path,draft:EntityDraft}`；`preview_entity_binding(&Project,Revision,request)` 生成私有 candidate，`apply_entity_binding(&mut Project,&mut Revision,&plan)` 一次提交源码和链接。两者共享基线、活动源码验证与 Project 快照撤销，不在 UI 拼凑两个可部分成功的写入，不自动升级语言。

## 7. 输出与公共选择

```rust
scene_to_safe_svg(&MapScene, f64, f64) -> Result<String, SceneError>
to_safe_svg(&MapDocument, Option<&BTreeSet<String>>) -> Result<String, SceneError>
map_to_safe_svg(&MapDocument, Option<&BTreeSet<String>>) -> Result<String, SceneError>
to_safe_svg_with_links(&MapDocument, Option<&BTreeSet<String>>,
    &BTreeMap<String, ScenePublicLink>) -> Result<String, SceneError>
to_safe_svg_layers_with_links(&MapDocument, Option<&BTreeSet<String>>,
    &BTreeMap<String, ScenePublicLink>) -> Result<BTreeMap<String, String>, SceneError>
```

`to_safe_svg` 仅 scene；`map_to_safe_svg` 包括旧 geometry 与 scene，提供完整矢量交换。raster 与作者语义元数据仍需原生工程备份。`scene_to_safe_svg` 无 map layer_order，UI 必须按地图顺序逐层投影；reader 的 layers 入口一次校验并返回各层同 viewport SVG，按每层旧图元→scene 组合，不能把所有 scene 统一放到所有旧图元上方。

selected=None 尊重作者显隐；Some 是精确公开白名单，忽略作者临时隐藏状态，不因选中 group 自动公开后代，仅保留显示被选节点必要的 ancestor 几何结构。所有来源文字与属性转义，原节点 ID/name/target/extra 不进入公开 SVG。精确公开选择不输出无关空图层；内部 clip 编号仅在必需祖先集合内分配，未选私有节点不改变公开编号。

`ScenePublicLink{href:Option<String>,anchor:String,label:String}` 由 reader 提供已授权显示文字与包内链接。href 拒绝冒号、反斜杠、控制字符或 `/` 起始；anchor 只含小写 ASCII 字母、数字、连字符且不重复。链接只应用于明确选中的对应节点，不从 target_ref 自动扩发布闭包。

## 8. 验证、诊断和预算

所有 numeric/资源/性能数值只在 [presentation.md §11](presentation.md#11-阈值登记单源) 登记。SceneLimits 默认遵守该表，caller 只可调低；每个 node matrix、累计 world matrix、viewport组合、变换后的控制点、保守 curve/text/stroke bounds 和最终投影点都检查有限性与世界硬预算。不得仅逐字段 finite 后让多层累乘生成 f32 infinity。临时投影超预算明确失败，不降低精度冒充成功。批量输入、复制子树和原JSON未知字段保全必须在分配派生副本前计算有界字节成本，不能只在最终提交时发现元数据放大。

`SceneError{code,message,node_id,line,column,field,operation_index}` 使用英文 code/中文 message。稳定错误域：`SCENE_SCHEMA`、`SCENE_FEATURE`、`SCENE_STRUCTURE`、`SCENE_GEOMETRY`、`SCENE_STYLE`、`SCENE_NUMERIC`、`SCENE_LIMIT`、`SCENE_SVG_PROFILE`、`SCENE_REFERENCE`、`SCENE_LOCKED`、`SCENE_COMPOSITING_BOUNDARY`、`SCENE_MIGRATION_ORDER`、`SCENE_MIGRATION_STYLE`、`SCENE_STALE`、`SCENE_CONFLICT`、`SCENE_CANCELLED`、`SCENE_STORAGE`。Rust的line/column/operation_index使用Option<u32>，覆盖输入与操作硬预算且避免错误值不必要膨胀；JSON仍为整数或null。XML可定位时行列从1开始，operation_index从0开始；没有位置时用null，不伪造位置。

资源预算既约束解析也约束渲染；最多实际活动任务数与显式 RGBA 工作缓冲/scene纹理合计在阈值表登记。该合计不是总RSS，resvg内部合成内存另须预检/拒绝，不能隐含降DPI、丢图层或取消后提前释放仍运行线程的许可。

## 9. 必要回归

完整路径命令/多子路径/fill-rule、text/affine/clip、source→scene→SVG→scene 保真；危险/超预算/nested输入定位拒绝零改；一次batch/undo/reopen/worker规范化/stale/cancel/未知feature保全；旧placement迁移ID与comments/refactor连续；whole-layer/后缀顺序与中段拒绝；全图旧新混合导出/公开精确选择；累计32层数值恶意输入、极小viewBox、unknown style放大；release导入性能及前台取消进度。当前候选须通过全量验收。
