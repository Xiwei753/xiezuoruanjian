# Rust 核心库源代码

本目录是写作软件的核心业务逻辑层，所有文件操作、项目管理、同步、格式化和设置规则都在此实现。

## 主要文件

| 文件 | 用途 |
|------|------|
| `lib.rs` | 库入口，定义模块结构 |
| `facade.rs` | 对外 API 入口，客户端通过此文件调用核心功能 |
| `workspace.rs` | 工作区管理，处理项目根目录和配置 |
| `project.rs` | 项目管理，处理作品的创建、删除、重命名 |
| `volume.rs` | 卷管理，处理作品中的卷结构 |
| `chapter.rs` | 章节管理，处理章节的创建、编辑、删除 |
| `storage.rs` | 存储层，处理文件读写操作 |
| `sync_service.rs` | 同步服务，协调同步逻辑 |
| `ai_service.rs` | AI 服务，处理 AI 相关功能 |
| `proofreading_service.rs` | 校对服务，处理文本校对 |
| `graph_service.rs` | 图服务，处理关系图谱 |
| `backup.rs` | 备份功能 |
| `trash.rs` | 回收站功能 |
| `delete_guard.rs` | 删除保护，防止误删 |
| `error.rs` | 错误类型定义 |
| `index.rs` | 索引管理 |
| `app_config.rs` | 应用配置 |
| `settings_registry.rs` | 设置注册表 |
| `action_registry.rs` | 操作注册表 |
| `api.udl` | UniFFI 接口定义文件 |

## 子模块

| 目录 | 用途 |
|------|------|
| `editor/` | 编辑器模块，处理文本编辑和自动校正 |
| `settings/` | 设置模块，管理用户偏好设置 |
| `sync/` | 同步模块，处理多设备同步 |
| `writing_stats/` | 写作统计模块，统计字数、时长等 |
| `starmap/` | 星图模块，可视化作品结构 |
| `mind_map/` | 思维导图模块，管理大纲和笔记 |

## 依赖关系

- 本模块是整个项目的核心，所有客户端（Android、Linux）都依赖此库
- 通过 `facade.rs` 暴露统一 API，客户端不应直接调用内部模块

## 使用说明

- 修改核心逻辑后必须运行 `cargo test` 验证
- 修改后需更新 `docs/core_api.md` 文档
- 严格禁止在此模块中引入 UI 相关代码