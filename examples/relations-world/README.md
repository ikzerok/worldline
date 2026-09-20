# 独立关系示例

这是显式启用语言 1.10 的最小完整工程。两条关系使用相同端点，但保留各自 ID、说明和属性；反向读取名称不新增关系。正文链接可以打开关系资料，关系声明本身不执行剧情。

在 worldline 仓库根目录运行（先构建 wl，或将 wl 换成对应可执行文件路径）：

```powershell
wl workspace check examples/relations-world --json
wl relations examples/relations-world --target entity:keepers --json
wl relations examples/relations-world --target entity:lighthouse --direction incoming --json
wl catalog examples/relations-world --kind relation --json
```

查询应返回 daily_care 与 annual_repair 两条独立边，未截断。沿灯塔入边读取时显示“由其维护”，原始起点、终点与关系 ID 保持不变。要练习编辑，请先将整个目录复制到源码仓库外作为自己的作品。
