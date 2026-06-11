//! # 原子文件写入（Core 层基础设施）
//!
//! 所有文件写入必须通过此模块，提供目标文件替换的原子性：
//! 1. 写入临时文件（带随机后缀，防止文件名冲突）
//! 2. `fsync` 临时文件内容
//! 3. `rename` 原子替换目标文件
//! 4. 对父目录进行 `fsync`（仅 Unix 平台，持久化目录项以确保 rename 结果持久可见）
//!
//! 这样可避免写入过程中断导致目标文件半写入。不同文件系统 and 挂载参数下，
//! 目录项持久化仍取决于平台语义，本模块不宣称跨设备断电的绝对耐久性。

pub mod transaction;

use crate::error::Result;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::Path;
use uuid::Uuid;

/// 原子写入字符串到文件。
///
/// 流程：创建临时文件 → 写入 → fsync 临时文件 → rename 替换目标文件 → fsync 父目录（Unix）。
/// 这是 Core 层所有文件写入的唯一入口。
pub fn atomic_write_string(path: &Path, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let tmp_path = path.with_extension(format!("tmp.{}", Uuid::new_v4()));

    let write_and_sync_tmp = || -> Result<()> {
        let file = File::create(&tmp_path)?;
        let mut writer = BufWriter::new(file);
        writer.write_all(content.as_bytes())?;
        writer.flush()?;
        let file = writer.into_inner().map_err(|e| e.into_error())?;
        file.sync_all()?;
        Ok(())
    };

    if let Err(e) = write_and_sync_tmp() {
        let _ = fs::remove_file(&tmp_path);
        return Err(e);
    }

    if let Err(e) = fs::rename(&tmp_path, path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(e.into());
    }

    if let Some(parent) = path.parent() {
        let parent_path = if parent.as_os_str().is_empty() {
            Path::new(".")
        } else {
            parent
        };
        #[cfg(unix)]
        {
            let dir = File::open(parent_path)?;
            dir.sync_all()?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_atomic_write_success() {
        let temp_dir = tempfile::tempdir().unwrap();
        let file_path = temp_dir.path().join("test.txt");

        atomic_write_string(&file_path, "hello world").unwrap();
        let read_content = std::fs::read_to_string(&file_path).unwrap();
        assert_eq!(read_content, "hello world");
    }

    #[test]
    fn test_atomic_write_continuous_overwrite() {
        let temp_dir = tempfile::tempdir().unwrap();
        let file_path = temp_dir.path().join("test.txt");

        for i in 0..10 {
            let content = format!("content {}", i);
            atomic_write_string(&file_path, &content).unwrap();
            let read_content = std::fs::read_to_string(&file_path).unwrap();
            assert_eq!(read_content, content);
        }
    }

    #[test]
    #[cfg(unix)]
    fn test_atomic_write_failure_keeps_old_file() {
        use std::fs::Permissions;
        use std::os::unix::fs::PermissionsExt;

        let temp_dir = tempfile::tempdir().unwrap();
        let file_path = temp_dir.path().join("test.json");

        // 1. First write: success
        let original_content = r#"{"status": "ok", "version": 1}"#;
        atomic_write_string(&file_path, original_content).unwrap();

        // Check it exists and is correct
        let read_content = std::fs::read_to_string(&file_path).unwrap();
        assert_eq!(read_content, original_content);

        // 2. Make the directory read-only (0o500)
        let dir_permissions = Permissions::from_mode(0o500);
        std::fs::set_permissions(temp_dir.path(), dir_permissions).unwrap();

        // 3. Attempt to overwrite: should fail because we can't create the tmp file in the read-only dir
        let new_content = r#"{"status": "updated", "version": 2}"#;
        let result = atomic_write_string(&file_path, new_content);
        assert!(result.is_err());

        // Restore directory permissions so we can clean up and read
        let restore_permissions = Permissions::from_mode(0o700);
        std::fs::set_permissions(temp_dir.path(), restore_permissions).unwrap();

        // 4. Verify the old file is still intact and readable (no half-written JSON)
        let read_content_after = std::fs::read_to_string(&file_path).unwrap();
        assert_eq!(read_content_after, original_content);
        
        // 5. Verify no tmp files are left in the directory
        let mut entries = std::fs::read_dir(temp_dir.path()).unwrap();
        let mut file_names: Vec<String> = Vec::new();
        while let Some(Ok(entry)) = entries.next() {
            file_names.push(entry.file_name().to_string_lossy().into_owned());
        }
        // Should only contain "test.json", no tmp files
        assert_eq!(file_names, vec!["test.json".to_string()]);
    }
}

