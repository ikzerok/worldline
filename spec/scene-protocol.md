# 矢量场景机器接口

`authoring.vector_scene.v1` 是 agent initialize 的附加能力。协议总版本仍为1，场景/导入语义以 [vector-scene.md](vector-scene.md) 为唯一真源。本接口不提高语言版本，不接受任意候选文档字节。

## Agent 方法

- `scene.svg.preview`：仅接收字符串 `source`，按安全 profile 返回 typed SVG 预检，不访问工程、不写入
- `scene.preview`：接收且仅接收一个 `path` 或 `project_id`，以及 typed `batch`，返回 `plan`、`baseline`、`revision` 和 `plan_digest`
- `scene.apply`：接收同样的工程身份与 `batch`，另需原预览的 `baseline`、`plan_digest`；重新生成可信core plan，通过后原子应用并复用 Project 保存事务
- `scene.export`：接收工程身份、`map_id`，返回全图 legacy+scene 安全矢量 `svg`；不隐式包含 raster/作者源码，不写任意输出路径

`source` 与工程寻址不可混合；批次方法的 `batch.expected_revision` 必须匹配当前展示修订。每个有状态 ProjectUnit 保存独立 scene revision，成功持久化后才推进；一次性 path 调用以默认修订开始，跨调用仍必须匹配内容基线、文档hash和计划摘要。拒绝依赖进程内修订绕过内容比对。

摘要是以 `worldline-scene-plan-v1` 域分隔、baseline、原typed batch与plan.after_hash的规范JSON字节计算的FNV-1a-64，小写16位十六进制；用于陈旧/错配检测，不是认证或安全签名。预览、CLI、RPC使用同一core helper。即使提供同一摘要，也不能跳过重新预览、磁盘冲突和只读检查。

会话操作先刷新正常外部变化，有未保存冲突则 `ok:false`；apply 在私有candidate上执行与保存，保存成功才替换会话 Project。失败时不替换会话稿件或推进修订。保存故障仍遵守既有可恢复日志语义，不承诺跨文件物理原子性。

参数形状、未知会话、重复/相互冲突的寻址、缺字段、无效DTO属于JSON-RPC -32602。有效请求中的profile拒绝、过期、锁定、只读、冲突、预算、取消和IO错误是 `ok:false`，含稳定英文code和中文message，不当作协议异常。结果中的private after字节不序列化为可写授权。

## CLI

```text
wl scene svg-preview --source SVG文本 [--json]
wl scene preview <工程目录或入口> --request-json SceneBatch [--json]
wl scene apply <工程目录或入口> --request-json SceneBatch --baseline 基线 --plan-digest 摘要 [--json]
wl scene export <工程目录或入口> --map-id ID [--json]
```

参数可用 `--key=value` 或独立值，重复参数与无关参数拒绝。JSON输入使用core无重复键解析；request最多32MiB，SVG原文最多2MiB。`--json` 成功输出完整typed投影；人类模式给简短结果或SVG。退出码0成功，1有效请求的业务拒绝，2用法/DTO/无法打开工程。apply复用core重算、原子内存事务和保存，不能通过JSON直接提交after字节。

## 验收

CLI/RPC的plan与core相同；typed曲线及导出重导入保真；合法选择/多层引用不变；陈旧基线/摘要/修订、未知feature、nested svg、恶意外链、歧义寻址、JSON重复字段、错误DTO、外部冲突和重复apply均拒绝且零额外写入。core、CLI、RPC的完整错误路径和成功保存重开需分别测试。静态世界站v3继续使用现有reader-export/reader.export方法，不增第二套站点生成逻辑。
