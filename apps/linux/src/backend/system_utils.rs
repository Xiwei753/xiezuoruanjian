// =============================================================================
// system_utils.rs — 系统级工具函数（主题检测、剪贴板）
// =============================================================================

use std::io::Write;

pub(crate) fn try_kreadconfig(cmd: &str) -> Option<String> {
    let output = std::process::Command::new(cmd)
        .args(["--file", "kdeglobals", "--group", "General", "--key", "ColorScheme"])
        .output().ok()?;
    if output.status.success() {
        let value = String::from_utf8_lossy(&output.stdout).trim().to_lowercase();
        if value.contains("dark") {
            return Some("dark".to_string());
        }
    }
    None
}

pub(crate) fn detect_system_theme_from_platform() -> String {
    // Priority: KDE6 kreadconfig6 > KDE5 kreadconfig5 > GNOME gsettings > GTK_THEME env >
    //           gsettings gtk-theme > light fallback
    if let Some("dark") = try_kreadconfig("kreadconfig6").as_deref() {
        return "dark".to_string();
    }
    if let Some("dark") = try_kreadconfig("kreadconfig5").as_deref() {
        return "dark".to_string();
    }
    if let Ok(output) = std::process::Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "color-scheme"])
        .output()
    {
        if output.status.success() {
            let value = String::from_utf8_lossy(&output.stdout).trim().to_lowercase();
            if value.contains("dark") {
                return "dark".to_string();
            }
        }
    }
    if let Ok(theme) = std::env::var("GTK_THEME") {
        if theme.to_lowercase().contains("dark") || theme.to_lowercase().contains("-dark") {
            return "dark".to_string();
        }
    }
    if let Ok(output) = std::process::Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "gtk-theme"])
        .output()
    {
        if output.status.success() {
            let value = String::from_utf8_lossy(&output.stdout).trim().to_lowercase();
            if value.contains("dark") || value.contains("-dark") || value.contains("_dark") {
                return "dark".to_string();
            }
        }
    }
    "light".to_string()
}

pub(crate) fn copy_text_to_clipboard_impl(text_str: &str) -> serde_json::Value {
    let mk_success = |backend: &str| -> serde_json::Value {
        serde_json::json!({
            "success": true,
            "data": { "backend": backend },
            "userMessage": format!("已复制 (backend={})", backend),
            "warnings": [],
            "changedPaths": [],
            "changedEntities": [],
        })
    };

    // 1. Try wl-copy (Wayland)
    if let Ok(mut child) = std::process::Command::new("wl-copy")
        .stdin(std::process::Stdio::piped())
        .spawn()
    {
        if let Some(ref mut stdin) = child.stdin {
            let _ = stdin.write_all(text_str.as_bytes());
        }
        match child.wait() {
            Ok(status) if status.success() => return mk_success("wl-copy"),
            _ => {}
        }
    }

    // 2. Try xclip (X11)
    if let Ok(mut child) = std::process::Command::new("xclip")
        .args(["-selection", "clipboard", "-in"])
        .stdin(std::process::Stdio::piped())
        .spawn()
    {
        if let Some(ref mut stdin) = child.stdin {
            let _ = stdin.write_all(text_str.as_bytes());
        }
        match child.wait() {
            Ok(status) if status.success() => return mk_success("xclip"),
            _ => {}
        }
    }

    // 3. Try xsel (X11 fallback)
    if let Ok(mut child) = std::process::Command::new("xsel")
        .args(["--clipboard", "--input"])
        .stdin(std::process::Stdio::piped())
        .spawn()
    {
        if let Some(ref mut stdin) = child.stdin {
            let _ = stdin.write_all(text_str.as_bytes());
        }
        match child.wait() {
            Ok(status) if status.success() => return mk_success("xsel"),
            _ => {}
        }
    }

    // 4. Last resort: arboard (Rust clipboard API)
    if let Ok(mut clip) = arboard::Clipboard::new() {
        if clip.set_text(text_str.to_string()).is_ok() {
            Box::leak(Box::new(clip));
            return mk_success("arboard");
        }
    }

    serde_json::json!({
        "success": false,
        "errorCode": "CLIPBOARD_UNAVAILABLE",
        "userMessage": "复制失败：未找到可用的剪贴板后端。请安装 wl-copy (Wayland)、xclip 或 xsel (X11)。",
        "rawError": "No clipboard backend available",
        "warnings": [],
        "changedPaths": [],
        "changedEntities": [],
    })
}
