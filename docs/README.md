# 项目文档

Status: active
Last verified: 2026-06-27
Truth source: product decision / code
Supersedes: docs/README.md (previous version)

本目录包含项目的各类技术文档和设计规范。

## 活动文档 (Active Documents)

| 文档名称 | 用途说明 |
|:---|:---|
| [TECHNICAL_ROUTE.md](TECHNICAL_ROUTE.md) | 全局技术路线与架构约束（唯一事实来源） |
| [editor_engine_route.md](editor_engine_route.md) | 自研编辑器渲染引擎路线（SujianEditorItem 唯一主路径） |
| [workspace_format.md](workspace_format.md) | 工作区物理磁盘布局规范（权威定义，绝对禁止修改） |
| [settings_schema.md](settings_schema.md) | 应用设置与偏好 JSON Schema 规范 |
| [sync_rules.md](sync_rules.md) | 数据同步与多端冲突合并策略 |
| [starmap_semantics.md](starmap_semantics.md) | 星图语义模型与独立引用安全机制 |
| [starmap_canvas_model.md](starmap_canvas_model.md) | 星图画布模型契约（独立画布、节点、边、超链接定义） |
| [starmap_implementation_route.md](starmap_implementation_route.md) | 星图实现路线（存储、渲染、交互、迁移） |

## 使用说明

- 修改任何底层业务逻辑，必须优先在 Rust Core 中设计并通过 FFI 暴露。
- 修改自研编辑器逻辑前，必须查阅 `editor_engine_route.md` 与 `TECHNICAL_ROUTE.md`。
- `workspace_format.md` 描述的文件与目录结构为最高优先级规范，绝对不能为了前端 UI 展现进行妥协修改。
