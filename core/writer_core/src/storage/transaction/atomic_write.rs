use std::fs;
use std::path::Path;
use uuid::Uuid;

// ── 原子写入工具函数（从原 storage.rs 迁移） ──

use std::fs::File;
use std::io::{BufWriter, Write};

/// 原子写入字符串到文件。
///
/// 流程：创建临时文件 → 写入 → fsync 临时文件 → rename 替换目标文件 → fsync 父目录（Unix）。
/// 这是 Core 层所有文件写入的唯一入口。
pub fn atomic_write_string(path: &Path, content: &str) -> crate::Result<()> {
    atomic_write_bytes(path, content.as_bytes())
}

/// 原子写入字节到文件。
///
/// 与 [atomic_write_string] 同一流程，但接收任意字节（full sync staging 提交
/// 二进制文件、非 UTF-8 内容时使用）。`add_file(&str)` 转成 bytes 后委托本函数。
pub fn atomic_write_bytes(path: &Path, content: &[u8]) -> crate::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let tmp_path = path.with_extension(format!("tmp.{}", Uuid::new_v4()));

    let write_and_sync_tmp = || -> crate::Result<()> {
        let file = File::create(&tmp_path)?;
        let mut writer = BufWriter::new(file);
        writer.write_all(content)?;
        writer.flush()?;
        let file = writer.into_inner().map_err(|e| e.into_error())?;
        file.sync_all()?;
        Ok(())
    };

    if let Err(e) = write_and_sync_tmp() {
        if let Err(cleanup_err) = fs::remove_file(&tmp_path) {
            log::warn!(
                "[storage] failed to cleanup tmp file {}: {}",
                tmp_path.display(),
                cleanup_err
            );
        }
        return Err(e);
    }

    if let Err(e) = fs::rename(&tmp_path, path) {
        if let Err(cleanup_err) = fs::remove_file(&tmp_path) {
            log::warn!(
                "[storage] failed to cleanup tmp file {}: {}",
                tmp_path.display(),
                cleanup_err
            );
        }
        return Err(e.into());
    }

    sync_parent(path)?;
    Ok(())
}

/// fsync 目录（持久化目录项）。
///
/// Unix 上对目录 handle 调 `sync_all` 持久化目录项（rename/unlink/create 的结果）。
/// Windows 上 `sync_all` 对目录 handle 是合法但通常为 no-op，保留调用以统一代码路径。
/// 目录不存在视为成功（调用方可能尚未创建子目录）。
pub fn sync_dir(path: &Path) -> crate::Result<()> {
    let path = if path.as_os_str().is_empty() {
        Path::new(".")
    } else {
        path
    };
    let dir = File::open(path)?;
    dir.sync_all()?;
    Ok(())
}

/// fsync `path` 的父目录。
///
/// `path` 无父目录（根路径）时为 no-op。
pub fn sync_parent(path: &Path) -> crate::Result<()> {
    if let Some(parent) = path.parent() {
        sync_dir(parent)?;
    }
    Ok(())
}

/// durable copy — copy 后对目标文件 + 目标父目录 fsync。
///
/// 与 `atomic_write_bytes` 的 fsync 做法对齐：文件内容 `sync_all` + 父目录 `sync_all`
/// 持久化目录项。供 `transaction.rs` 的 backup/rollback 路径使用，替代裸 `fs::copy`。
pub fn durable_copy_file(src: &Path, dst: &Path) -> crate::Result<()> {
    fs::copy(src, dst)?;
    let f = File::open(dst)?;
    f.sync_all()?;
    sync_parent(dst)?;
    Ok(())
}
