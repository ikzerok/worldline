# 有界试玩路径交换（工具 0.29）

runtime 是 ReplayTrace 交换的唯一生产者。`decode_replay_trace(bytes)` 接受外部 UTF-8 JSON；
`encode_replay_trace(trace)` 返回可重新导入的紧凑 JSON。两者不执行故事、不访问工程、
不修复观察或迁移 runtime 版本。原 ReplayTrace schema 和字段形状不变。

## 限额与兼容

- 单份输入原始字节和最终输出 UTF-8 字节各最多 4 MiB；最多 20,000 个选择步骤。
  计数包含 JSON 转义、标点和字段名。编码流式写入有界容器，不能先生成巨大字符串再检查。
- 输入在解析前检查实际 bytes；复用 core `parse_unique_json`，拒绝顶层和任意嵌套对象的
  重复键、非法 UTF-8、非有限数值和尾部内容，保留 serde_json 的默认递归深度保护。
  总字节限额约束单个字符串与容器；不把字符串按字符数误计成 bytes。
- 交换上限沿用既有会话记录 4 MiB / 20,000 步能力；不再由 UI 使用另一个 1 MiB 上限。
  既有不超过 1 MiB 的合法输入仍可读。旧 UI 的 256 KiB 单字符串约束不是 trace 语义；
  新交换统一由 4 MiB 总输入/输出约束，不能关闭总体资源保护。
- 当前支持 schema 1。未知可选字段沿原 serde DTO 兼容策略读取，编码输出当前 DTO 已知
  字段；不承诺保留未知字段的原字节、空白或键顺序。成功编码保证解码后 DTO 相等，
  再编码得到相同的紧凑字节；不会把 pretty 膨胀绕过限额。
- `runtime_version` 不匹配仍允许只读导入/查看；这不代表可重放。执行仍使用已有
  schema/runtime/fingerprint/checkpoint 校验，入口与检查点的兼容规则不变。
- route comparison、playthrough report 的独立业务步数/字节/输出预算保持不变。
  一条能交换的路径不一定能生成报告，界面须明确区分交换失败和业务限额。

## 错误与消费者

错误 `ReplayExchangeError { code, message }` 的 code 为英文、message 中文：
`input_limit`、`output_limit`、`step_limit`、`invalid_json`、`invalid_trace`、`unsupported_schema`。
错误不作为 JSON 产物返回。解码失败不产生部分路径，编码失败不篡改输入 trace。

编辑器记录、导入和导出使用同一个交换契约；每会话最多64条记录。只有成功后才替换
导出正文或加入路径；失败保留原记录、选择、源码、live Story、RNG与保存基线。
导出失败清除可交付JSON区，切换所选路径也清除旧产物，避免把上次成功内容冒充当前选择。
界面明确 JSON 是含完整运行状态的作者数据，不等同于默认省略私密字段的 Markdown 报告。
不增加路径数据库、工程文件、自动保存或发布权限。

## CLI 显式交换副本

`wl play <入口> --trace-output <目标.json> --trace-exchange` 显式采用本合同的紧凑
4MiB/20,000步交换副本；`--trace-exchange`只接受一次且必须同时指定`--trace-output`。
只用于play，不改变普通`replay --trace-json`及其运行预算。路径内容不增加新wrapper。

没有`--trace-exchange`时，原trace-output的pretty输出、可输出超过交换限额的路径、
覆盖行为及退出码保持不变。显式交换先完整编码，失败不创建/截断目标JSON，剧情stdout
仍按原play语义输出。显式交换要求尚不存在的新目标；同目录新临时文件写入/同步/关闭
成功后以hard_link原子no-replace发布，再清理暂存名。已有目标（包括竞态创建）或不支持
hardlink的文件系统安全拒绝，不删除/覆盖旧目标。发布失败清理暂存文件；若发布成功后
仅暂存名清理失败，成功产物保留并给出清理警告，不把已成功发布冒称零输出失败。
不承诺掉电持久性。业务失败沿现有CLI调用错误通道返回，不改普通play。

最低回归：真实1–4MiB trace往返；4MiB边界与20,000步边界；中文/引号/换行/转义；
任意层重复键、极深JSON、大量短元素、无效UTF-8、非法schema、旧runtime可读但重放拒绝；
旧合法DTO、entry/checkpoint、输出失败保留原trace；业务预算不扩大，UI失败零修改。
