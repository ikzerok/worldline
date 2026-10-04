# 资料表改稿：港口与人物

先复制整个示例目录，再在 Worldedit 打开副本。工程已显式启用语言1.13和实体/人物引用能力；导入本身不会升级作品。

## 在工作台试一次

1. 从工程菜单选择“世界资料导入”（或命令面板“资料 · 世界资料导入”），选 `资料修订.csv`，新增对象的目标选 `world.wl`
2. 每列明确映射：
   - kind → 身份类型；id → 稳定ID；显示名 → 显示名
   - 实体分类 → entity_type，描述 → description；这两列空单元格都选“保留”，人物不支持它们
   - 评分 → property `score`，类型number；启用 → property `enabled`，类型bool
   - 地点 → property `place`，类型ref，目标kind为entity；空单元格选“保留”
   - 其余列的空值策略保持“错误”，没有忽略列
3. 预览应显示2条更新、2条新建。lin仍改people.wl，old_harbor仍改places.wl；mei和new_harbor追加world.wl。人物指向同批最后一行才新建的港口，也必须通过最终候选校验
4. 检查0和false没有被当成缺值、双引号与多行描述正确保留。lin的note、旧注释和港口别名不变
5. 本批改变人物显示名/评分并新增人物，真实运行指纹会改变。审阅工作台的旧Story存档/检查点兼容提示后，再确认并应用。一次撤销会恢复整个批次；应用尚未保存
6. 保存全部，关闭后重新打开，再选同一CSV和映射：应4条无变化，文件字节不重排，也不新增历史

CSV每行的kind+id是唯一身份。改显示名不会生成新ID；把同一身份复制成第二行会阻断整批。无需按显示名合并，也不要把本表当持续同步来源。

## CLI / RPC 核验

CLI加载完整工作区快照最多4096个普通作者文件、合计64 MiB（含附件），另加CSV自身2 MiB上限；它不会为预览自动恢复待处理的保存事务。

`mapping.json` 是上面八列的同一映射，列号从0起。`wl workspace check <副本目录> --json` 的 `workspace_revision` 可填入请求的 `expected_baseline`。

请求形状：`{"schema_version":1,"expected_baseline":"当前workspace_revision","csv":"","destination":"world.wl","columns":[mapping.json中的八项]}`。

- `wl catalog-import preview <副本目录> --request-json '<请求>' --csv '<副本目录>/资料修订.csv' --json`
- 审阅后用相同请求、CSV和返回的plan_digest：`wl catalog-import apply <副本目录> --request-json '<请求>' --csv '<副本目录>/资料修订.csv' --plan-digest '<摘要>' --save --json`
- 不加--save只修改短命CLI内存，输出saved:false，退出即丢弃；只有显式--save才落盘。再次导入前重新取得当前workspace_revision

RPC使用相同DTO，先project.open，再catalog.import.preview/apply，最后project.save。完整边界见[资料表规范](../../spec/catalog-import.md)。不要通过导入流程迁移旧存档或修改已有指纹守卫。
