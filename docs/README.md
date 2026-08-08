# 项目文档

本目录只保存长期有效的架构约束和数据格式。具体实现步骤、历史迁移、阶段性审计和能力快照统一放在 GitHub Issue，不进入长期文档。

## 活动文档

- [TECHNICAL_ROUTE.md](TECHNICAL_ROUTE.md)：全局技术路线与平台边界。
- [data_directory_format.md](data_directory_format.md)：数据目录格式（各平台数据根目录与作品仓库布局）。
- [settings_schema.md](settings_schema.md)：设置项及同步属性。
- [sync_rules.md](sync_rules.md)：同步与冲突规则。
- [starmap_semantics.md](starmap_semantics.md)：星图对象、引用和语义约束。

## 维护规则

- 文档描述稳定的“谁负责什么”和“数据长什么样”，不记录某个类或函数如何实现。
- 被代码替代的方案直接删除，不保留历史说明。
- 已淘汰的平台、旧目录、旧编辑器和兼容路线不得继续出现在活动文档中。
- 接口或磁盘格式变化时更新对应契约；普通重构不修改文档。
