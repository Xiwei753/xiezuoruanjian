//! # 原子文件写入（Core 层基础设施）
//!
//! 所有文件写入必须通过此模块，确保：
//! 1. 写入临时文件（带随机后缀，防止文件名冲突）
//! 2. `fsync` 确保数据落盘
//! 3. `rename` 原子替换目标文件
//!
//! 这样即使写入过程中断电，也不会损坏目标文件。

use crate::error::Result;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::Path;
use uuid::Uuid;

/// 原子写入字符串到文件。
///
/// 流程：创建临时文件 → 写入 → fsync → rename 替换目标文件。
/// 这是 Core 层所有文件写入的唯一入口。
pub fn atomic_write_string(path: &Path, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let tmp_path = path.with_extension(format!("tmp.{}", Uuid::new_v4()));
    {
        let file = File::create(&tmp_path)?;
        let mut writer = BufWriter::new(file);
        writer.write_all(content.as_bytes())?;
        writer.flush()?;
        let file = writer.into_inner().map_err(|e| e.into_error())?;
        file.sync_all()?;
    }

    fs::rename(&tmp_path, path)?;

    Ok(())
}
