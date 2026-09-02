//! # 同步服务模块 (Sync Service)
//!
//! 本模块是写作软件的核心同步服务实现，负责处理作品目录（同步根）与远程仓库之间的数据同步。
//!
//! ## 主要功能
//!
//! - **同步后端**: 当前仅暴露 Git 和 GitHub API；其它后端不在运行期配置枚举中
//! - **Git 操作封装**: 基于 git2 库实现完整的 Git 操作，包括 clone、pull、push、commit 等
//! - **同步配置管理**: 管理远程 URL、认证信息、代理设置、分支等配置
//! - **冲突检测与解决**: 提供文件冲突检测、设置冲突语义合并等功能
//! - **同步诊断**: 提供网络探测、认证检查、仓库状态检查等诊断功能
//! - **安全处理**: URL 凭证脱敏、敏感信息保护
//!
//! ## 依赖关系
//!
//! - `git2`: Git 操作核心库
//! - `serde` / `serde_json`: 序列化/反序列化
//! - `base64`: URL 编码解码
//!
//! ## 使用场景
//!
//! - 作品数据备份与恢复
//! - 多设备间数据同步
//! - 团队协作时的数据共享
//! - 版本控制与历史追踪

#![allow(clippy::module_inception)]

pub mod config_store;
pub mod conflict;
/// #644 评论 5473789298 第3节：纯内容分类/三方比较（始终可用，不依赖 feature gate）。
pub mod content_class;
pub mod diagnostics;
pub mod full_sync;
pub mod full_sync_state;
pub(crate) mod full_sync_utils;
pub(crate) mod commit_helpers;
pub mod git;
#[cfg(feature = "github-api")]
pub mod lww;
pub mod provider;
pub mod scanner;
pub mod service;
pub mod staging;
pub mod tests;
pub mod types;
pub mod url;
pub mod utils;

pub use provider::*;
pub use service::*;
pub use types::*;
pub use url::*;

#[cfg(test)]
mod api_tests;
