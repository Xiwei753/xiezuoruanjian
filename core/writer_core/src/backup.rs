//! # 备份管理（Core 层）
//!
//! 本模块负责项目的备份功能，当前为桩实现（未完成）。
//!
//! ## 功能说明
//!
//! - `backup_project`: 备份指定项目（当前返回 NotImplemented 错误）
//!
//! ## 设计说明
//!
//! 备份功能尚未实现，所有操作都会返回 `Error::NotImplemented` 错误。
//! 未来实现时，应该：
//! 1. 创建项目的完整备份（包括所有卷和章节）
//! 2. 备份文件存储在工作区的 `backups/` 目录
//! 3. 支持增量备份和全量备份
//! 4. 提供备份恢复功能
//!
//! ## 依赖关系
//!
//! - 依赖 `crate::error` 模块提供统一错误类型
//! - 依赖 `std::path::Path` 进行路径操作

use crate::error::{Error, Result};
use std::path::Path;

/// 备份指定项目。
///
/// # 参数
///
/// - `_workspace_path`: 工作区根目录路径
/// - `_project_id`: 要备份的项目ID
///
/// # 返回值
///
/// 返回 `Result<()>`，当前总是返回 `Error::NotImplemented` 错误。
///
/// # 示例
///
/// ```rust
/// use std::path::Path;
/// use writer_core::backup::backup_project;
///
/// let workspace_path = Path::new("/path/to/workspace");
/// let result = backup_project(workspace_path, "project_123");
/// assert!(result.is_err()); // 当前未实现，返回错误
/// ```
pub fn backup_project(_workspace_path: &Path, _project_id: &str) -> Result<()> {
    Err(Error::NotImplemented)
}
