//! 桌面平台设备 ID 的原子创建与读取。
//!
//! Linux 与 Windows 共用同一份实现。先尝试读取已存在的 ID；若不存在，
//! 使用 `create_new` 原子创建文件，完整写入并 sync_all；写入失败时
//! 清理未完成文件。已存在时重读磁盘的 ID，不覆盖已有有效 ID。

use std::io::Write;
use std::path::Path;

pub fn load_or_create_device_id(state_dir: &Path) -> String {
    let device_id_path = state_dir.join("device_id");

    if let Ok(existing) = std::fs::read_to_string(&device_id_path) {
        let trimmed = existing.trim().to_string();
        if !trimmed.is_empty() {
            return trimmed;
        }
    }

    if let Some(parent) = device_id_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let new_id = uuid::Uuid::new_v4().to_string();

    match try_create_device_id_file(&device_id_path, &new_id) {
        Ok(()) => return new_id,
        Err(_) => {}
    }

    if let Ok(existing) = std::fs::read_to_string(&device_id_path) {
        let trimmed = existing.trim().to_string();
        if !trimmed.is_empty() {
            return trimmed;
        }
    }

    let _ = writer_core::storage::atomic_write_bytes(&device_id_path, new_id.as_bytes());

    if let Ok(existing) = std::fs::read_to_string(&device_id_path) {
        let trimmed = existing.trim().to_string();
        if !trimmed.is_empty() {
            return trimmed;
        }
    }

    new_id
}

fn try_create_device_id_file(path: &Path, content: &str) -> Result<(), std::io::Error> {
    let mut file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)?;

    if let Err(e) = file.write_all(content.as_bytes()) {
        let _ = std::fs::remove_file(path);
        return Err(e);
    }

    if let Err(e) = file.sync_all() {
        let _ = std::fs::remove_file(path);
        return Err(e);
    }

    Ok(())
}
