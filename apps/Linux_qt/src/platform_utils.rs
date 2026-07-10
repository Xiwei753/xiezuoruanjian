// =============================================================================
// platform_utils.rs — 平台特定工具函数
// =============================================================================
//
// 引用了什么：
// - std::process::Command：调用系统命令
//
// 干什么的：
// - 提供 Linux 桌面系统级工具函数
// - open_directory：用系统文件管理器打开目录
//
// 被什么引用：
// - workspace_backend.rs：打开工作区目录
// - diagnostics.rs：打开日志目录
// =============================================================================

/// 用系统文件管理器打开目录
/// Linux: xdg-open / nautilus / dolphin fallback
///
/// 安全措施（CWE-73/CWE-78）：
/// - 路径非空检查
/// - canonicalize 消除符号链接和 `../` 路径遍历
/// - is_dir() 确保目标是目录而非文件
/// - 使用 .arg() 传参避免 shell 注入
pub fn open_directory(path: &str) -> Result<(), String> {
    if path.is_empty() {
        return Err("路径为空".to_string());
    }

    let dir = std::path::Path::new(path);
    let canonical = dir.canonicalize().map_err(|e| {
        format!("路径无法解析（可能不存在或含非法遍历）：{}", e)
    })?;

    if !canonical.is_dir() {
        return Err(format!("路径不是有效目录：{}", path));
    }

    let safe_path = canonical.to_string_lossy();

    #[cfg(target_os = "linux")]
    {
        for cmd in &["xdg-open", "nautilus", "dolphin"] {
            if std::process::Command::new(cmd)
                .arg(&*safe_path)
                .spawn()
                .is_ok()
            {
                return Ok(());
            }
        }
        return Err("无法打开文件管理器：未找到 xdg-open/nautilus/dolphin".to_string());
    }

    #[allow(unreachable_code)]
    Err("不支持的 Linux 桌面环境".to_string())
}
