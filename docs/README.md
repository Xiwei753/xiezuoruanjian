# 项目文档

Status: active
Last verified: 2026-06-23
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

## 归档文档 (Archived Documents)

以下文档已归档至 `archive/` 目录，内容已合并入 `TECHNICAL_ROUTE.md` 或不再作为主要入口：

| 文档名称 | 归档原因 |
|:---|:---|
| [archive/CROSS_PLATFORM_CAPABILITY_CONTRACT.md](archive/CROSS_PLATFORM_CAPABILITY_CONTRACT.md) | Core-first 契约已内化为全局技术路线 |
| [archive/API_CONTRACTS.md](archive/API_CONTRACTS.md) | 接口契约已内化为全局技术路线 |
| [archive/PRODUCT_DESIGN.md](archive/PRODUCT_DESIGN.md) | 产品设计参考，非日常开发入口 |
| [archive/desktop_ime_notes.md](archive/desktop_ime_notes.md) | 输入法排错笔记，仅排查时查阅 |
| [archive/screen_policy_acceptance.md](archive/screen_policy_acceptance.md) | 三端布局验收表，仅验收时查阅 |

## 使用说明

- 修改任何底层业务逻辑，必须优先在 Rust Core 中设计并通过 FFI 暴露。
- 修改自研编辑器逻辑前，必须查阅 `editor_engine_route.md` 与 `TECHNICAL_ROUTE.md`。
- `workspace_format.md` 描述的文件与目录结构为最高优先级规范，绝对不能为了前端 UI 展现进行妥协修改。
