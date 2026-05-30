//! # 同步服务模块 (Sync Service)
//!
//! 本模块是写作软件的核心同步服务实现，负责处理工作区与远程仓库之间的数据同步。
//!
//! ## 主要功能
//!
//! - **多种同步后端支持**: 支持 Git、GitHub API、WebDAV、S3、本地文件夹等多种同步方式
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
//! - 工作区数据备份与恢复
//! - 多设备间数据同步
//! - 团队协作时的数据共享
//! - 版本控制与历史追踪

use base64::Engine;
use serde::{Deserialize, Serialize};
use std::path::Path;


pub mod types;
pub mod url;
pub mod config_store;
pub mod diagnostics;
pub mod git_backend;
pub mod github_backend;
pub mod backends;
pub mod conflict;
pub mod service;
pub mod utils;
pub mod tests;

pub use types::*;
pub use url::*;
pub use config_store::*;
pub use diagnostics::*;
pub use git_backend::*;
pub use github_backend::*;
pub use backends::*;
pub use conflict::*;
pub use service::*;
pub use utils::*;
