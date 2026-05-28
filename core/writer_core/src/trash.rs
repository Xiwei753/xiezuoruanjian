//! # 回收站管理（Core 层）
//!
//! 本模块负责章节的回收站功能，当前为桩实现（未完成）。
//!
//! ## 功能说明
//!
//! - `move_chapter_to_trash`: 将章节移动到回收站（当前返回 NotImplemented 错误）
//!
//! ## 设计说明
//!
//! 回收站功能尚未实现，所有操作都会返回 `Error::NotImplemented` 错误。
//! 未来实现时，应该：
//! 1. 将章节文件移动到工作区的 `trash/` 目录
//! 2. 保留原始文件的元数据（项目ID、卷ID、章节ID）
//! 3. 提供恢复功能
//!
//! ## 依赖关系
//!
//! - 依赖 `crate::error` 模块提供统一错误类型
//! - 依赖 `std::path::Path` 进行路径操作

use crate::error::{Error, Result};
use std::path::Path;

/// 将章节移动到回收站。
///
/// # 参数
///
/// - `_workspace_path`: 工作区根目录路径
/// - `_chapter_id`: 要移动到回收站的章节ID
///
/// # 返回值
///
/// 返回 `Result<()>`，当前总是返回 `Error::NotImplemented` 错误。
///
/// # 示例
///
/// ```rust
/// use std::path::Path;
/// use writer_core::trash::move_chapter_to_trash;
///
/// let workspace_path = Path::new("/path/to/workspace");
/// let result = move_chapter_to_trash(workspace_path, "chapter_123");
/// assert!(result.is_err()); // 当前未实现，返回错误
/// ```
pub fn move_chapter_to_trash(_workspace_path: &Path, _chapter_id: &str) -> Result<()> {
    Err(Error::NotImplemented)
}
