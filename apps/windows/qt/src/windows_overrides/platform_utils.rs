//! Windows 桌面平台工具。
//!
//! 只提供共享后端需要的 `open_directory`，使用 `explorer.exe` 打开目录。
//! 参数通过 `Command::arg` 传入，不经过 shell，避免命令注入。

pub fn open_directory(path: &str) -> Result<(), String> {
    if path.is_empty() {
        return Err("路径为空".to_string());
    }

    let canonical = std::path::Path::new(path)
        .canonicalize()
        .map_err(|e| format!("路径无法解析（可能不存在或含非法遍历）：{e}"))?;
    if !canonical.is_dir() {
        return Err(format!("路径不是有效目录：{path}"));
    }

    std::process::Command::new("explorer.exe")
        .arg(&canonical)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("无法打开资源管理器：{e}"))
}
