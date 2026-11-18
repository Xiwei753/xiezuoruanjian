//! 桌面平台设备 ID 的原子创建与读取。
//!
//! Linux 与 Windows 共用同一份实现。先尝试读取已存在的 ID；若不存在，
//! 将新 ID 写入带 UUID 后缀的临时文件，完整 `write_all` + `sync_all`
//! 后关闭句柄，再以 `fs::hard_link` 原子发布到目标路径（仅当目标不存在
//! 才成功）。成功时清理临时文件并返回磁盘确认的 ID；目标已存在时清理
//! 临时文件并重读已完成的 ID。不覆盖已有有效 ID，不返回未持久化的随机 ID。
//! 任何错误路径均清理临时文件。

use std::io::{self, Read, Write};
use std::path::Path;

pub fn load_or_create_device_id(state_dir: &Path) -> io::Result<String> {
    let device_id_path = state_dir.join("device_id");

    if let Some(id) = read_existing(&device_id_path) {
        return Ok(id);
    }

    if let Some(parent) = device_id_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let new_id = uuid::Uuid::new_v4().to_string();
    let temp_path = state_dir.join(format!("device_id.{}.tmp", uuid::Uuid::new_v4()));

    let result = try_publish_device_id(&temp_path, &device_id_path, &new_id);

    let _ = fs::remove_file(&temp_path);

    result
}

fn try_publish_device_id(
    temp_path: &Path,
    device_id_path: &Path,
    new_id: &str,
) -> io::Result<String> {
    {
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(temp_path)?;
        file.write_all(new_id.as_bytes())?;
        file.sync_all()?;
    }

    match fs::hard_link(temp_path, device_id_path) {
        Ok(()) => Ok(new_id.to_string()),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
            for _ in 0..5 {
                if let Some(id) = read_existing(device_id_path) {
                    return Ok(id);
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(io::Error::new(
                io::ErrorKind::Other,
                "device_id file exists but cannot be read",
            ))
        }
        Err(e) => Err(e),
    }
}

fn read_existing(path: &Path) -> Option<String> {
    let mut content = String::new();
    let mut file = fs::OpenOptions::new().read(true).open(path).ok()?;
    file.read_to_string(&mut content).ok()?;
    let trimmed = content.trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

use std::fs;
