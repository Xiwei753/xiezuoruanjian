//! # Writer Core - 核心业务逻辑层
//!
//! 本 crate 是整个应用的**唯一事实来源**（Single Source of Truth）。
//! 所有文件 I/O、项目管理、同步、格式化、设置规则都在此实现。
//!
//! ## 架构约束
//!
//! - **严格排除 UI 逻辑**：不允许出现动画、窗口管理、输入法、平台特定代码。
//! - **客户端只做展示**：Android / Linux 客户端通过 Facade（`facade.rs`）调用 Core。
//! - **正文永远是纯文本**：所有文件读写都是纯文本，不使用 HTML。
//!
//! ## 模块分层
//!
//! | 模块 | 职责 | 边界 |
//! |------|------|------|
//! | `facade` | 对外 API 入口，聚合所有子模块 | 客户端唯一调用点 |
//! | `workspace` | 工作区创建、验证、最近编辑 | 不处理项目内容 |
//! | `project` | 作品 CRUD、统计、排序、删除 | 删除走 `delete_guard` |
//! | `volume` | 卷 CRUD、排序、删除 | 删除走 `delete_guard` |
//! | `chapter` | 章节 CRUD、内容读写、备份、验证保存 | 正文永远是纯文本 |
//! | `settings` | 本地设置 & 可同步设置 | 分为 LocalSettings / SyncableSettings |
//! | `sync_service` | 同步配置、密钥、状态、诊断、实际同步 | 支持 Git/GitHub API/WebDAV/S3 |
//! | `mind_map` | 思维导图（图数据、布局、快照） | 独立于星图模块 |
//! | `starmap` | 星图（元数据、图、布局） | 独立于思维导图模块 |
//! | `writing_stats` | 写作统计（事件记录、聚合、查询） | 按设备/项目/章节统计 |
//! | `error` | 统一错误类型 | 所有模块共享 |
//! | `storage` | 原子文件写入（写临时文件 + fsync + rename） | 防止写入中断导致数据损坏 |
//! | `delete_guard` | 删除安全守卫（验证 ID、防止误删工作区根） | 所有删除操作必须经过此模块 |
//!
//! ## 调用链路示例
//!
//! ```text
//! 客户端 → facade::WriterCore::create_chapter()
//!         → chapter::create_chapter()
//!         → storage::atomic_write_string()
//! ```

pub mod action_registry;
pub(crate) mod delete_guard;
// Always export these for UniFFI
pub mod ai_service;
pub use crate::ai_service::{AiAction, AiActionResponse, AiActionType};
pub mod app_config;
pub mod graph_service;
pub mod proofreading_service;
pub mod settings_registry;
pub mod sync_service;

pub mod backup;
pub mod chapter;
pub mod editor;
pub mod error;
pub mod index;
pub mod mind_map;
pub mod project;
pub mod settings;
pub mod starmap;
pub mod storage;
pub mod sync;
pub mod trash;
pub mod volume;
pub mod workspace;
pub mod writing_stats;

pub use error::{Error, Result};

#[cfg(test)]
pub mod backup_tests;
#[cfg(test)]
pub mod chapter_tests;
pub mod facade;
#[cfg(test)]
pub mod fixture_tests;
#[cfg(test)]
pub mod project_tests;
#[cfg(test)]
pub mod settings_tests;
#[cfg(test)]
pub mod trash_tests;
#[cfg(test)]
pub mod volume_tests;
#[cfg(test)]
pub mod workspace_tests;
#[cfg(test)]
pub mod writing_stats_tests;

/// UniFFI 占位函数，用于跨平台绑定测试。
pub fn perform_dummy_action() -> String {
    "hello from uniffi".to_string()
}

uniffi::include_scaffolding!("api");
pub mod app_service;
pub use app_service::*;
