//! # Writer Core - 核心业务逻辑层
//!
//! 本 crate 是整个应用的**唯一事实来源**（Single Source of Truth）。
//! 所有文件 I/O、项目管理、同步、格式化、设置规则都在此实现。
//!
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::field_reassign_with_default,
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        clippy::excessive_nesting,
        clippy::too_many_arguments,
        clippy::type_complexity,
        deprecated
    )
)]
//! ## 架构约束
//!
//! - **严格排除 UI 逻辑**：不允许出现动画、窗口管理、输入法、平台特定代码。
//! - **客户端只做展示**：平台客户端通过 `writer_uniffi` 或 `writer_platform_api` 调用 Core。
//! - **正文永远是纯文本**：所有文件读写都是纯文本，不使用 HTML。
//! - **平台能力注入**：目录、设备 ID、密钥存储、网络状态由 `writer_platform_api` 契约注入，Core 不自行猜测平台。
//!
//! ## 模块分层
//!
//! | 模块 | 职责 | 边界 |
//! |------|------|------|
//! | `facade` | Core 内部统一入口，聚合所有子模块 | 不直接作为平台稳定边界 |
//! | `api` | 跨平台稳定 API 层，提供 DTO / Error / Service | `writer_uniffi` 和平台适配层的底座 |
//! | `recent_edits` | 最近编辑记录（首页"继续写作"入口） | 不处理项目内容 |
//! | `project` | 作品 CRUD、统计、排序、删除 | 删除走 `delete_guard` |
//! | `volume` | 卷 CRUD、排序、删除 | 删除走 `delete_guard` |
//! | `chapter` | 章节 CRUD、内容读写、验证保存 | 正文永远是纯文本 |
//! | `settings` | 本地设置 & 可同步设置 | 分为 LocalSettings / SyncableSettings |
//! | `sync` | 同步配置、密钥、状态、诊断、实际同步（合并了 sync） | 唯一同步模块 |

//! | `starmap` | 星图（元数据、图、布局）- 正式图谱路线 | 唯一推荐图谱接口 |
//! | `writing_stats` | 写作统计（事件记录、聚合、查询） | 按设备/项目/章节统计 |
//! | `error` | 统一错误类型 | 所有模块共享 |
//! | `storage` | 原子替换文件写入（写临时文件 + fsync 临时文件 + rename） | 防止半写入；耐久性受文件系统语义影响 |
//! | `delete_guard` | 删除安全守卫（验证 ID、防止误删数据根） | 所有删除操作必须经过此模块 |
//!
//! ## 调用链路示例
//!
//! ```text
//! 客户端 → facade::WriterCore::create_chapter()
//!         → chapter::create_chapter()
//!         → storage::atomic_write_string()
//! ```

pub mod action_registry;
pub mod api;
pub(crate) mod delete_guard;
// Always export these for UniFFI
pub mod ai_service;
pub use crate::ai_service::{AiAction, AiActionResponse, AiActionType};
pub mod app_config;
pub mod settings_registry;

pub mod chapter;
pub mod editor;
pub mod error;
pub mod history;
pub mod platform_interaction;

#[cfg(feature = "harmony-ffi")]
pub mod ffi;
pub mod presentation;
pub mod project;
pub mod recent_edits;
pub mod search;
pub mod settings;
pub mod starmap;
pub mod storage;
pub mod sync;
pub mod trash;
pub mod volume;
pub mod writing_stats;

pub use api::*;
pub use error::{Error, Result};

pub mod facade;
#[cfg(test)]
mod tests;

uniffi::include_scaffolding!("api");
pub mod app_service;
pub use app_service::WriterAppService;
