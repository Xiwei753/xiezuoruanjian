// =============================================================================
// system_utils.rs — Desktop 平台系统工具
// =============================================================================
//
// 本文件只负责 Desktop 平台的系统级工具函数，例如：
//   - 系统主题检测（dark/light，XDG/KDE/GTK/Qt portal）
//   - Portal / KDE 颜色方案读取
//   - 剪贴板操作
//
// 禁止事项：
//   - 不允许放 workspace / project / chapter / sync / editor 业务逻辑
//   - 不允许绕过 writer_core 写业务数据
//   - 不允许引入新的平台检测方式而不在此文件说明
// =============================================================================

use std::io::Write;

fn command_stdout(cmd: &str, args: &[&str]) -> Option<String> {
    let output = std::process::Command::new(cmd).args(args).output().ok()?;
    if output.status.success() {
        return Some(String::from_utf8_lossy(&output.stdout).trim().to_string());
    }
    None
}

fn read_kde_config_value(cmd: &str, group: &str, key: &str) -> Option<String> {
    command_stdout(
        cmd,
        &["--file", "kdeglobals", "--group", group, "--key", key],
    )
}

fn scheme_from_theme_name(value: &str) -> Option<&'static str> {
    let lower = value.trim().to_lowercase();
    if lower.contains("dark") || lower.contains("-dark") || lower.contains("_dark") {
        Some("dark")
    } else if lower.contains("light") || lower.contains("-light") || lower.contains("_light") {
        Some("light")
    } else {
        None
    }
}

fn parse_portal_color_scheme(value: &str) -> Option<&'static str> {
    let lower = value.to_lowercase();
    // org.freedesktop.appearance color-scheme: 0=no preference, 1=prefer dark, 2=prefer light.
    if lower.contains("uint32 1") || lower.contains("<1>") || lower.trim() == "1" {
        Some("dark")
    } else if lower.contains("uint32 2") || lower.contains("<2>") || lower.trim() == "2" {
        Some("light")
    } else if lower.contains("uint32 0") || lower.contains("<0>") || lower.trim() == "0" {
        Some("light")
    } else {
        None
    }
}

fn detect_portal_color_scheme() -> Option<String> {
    let value = command_stdout(
        "gdbus",
        &[
            "call",
            "--session",
            "--dest",
            "org.freedesktop.portal.Desktop",
            "--object-path",
            "/org/freedesktop/portal/desktop",
            "--method",
            "org.freedesktop.portal.Settings.Read",
            "org.freedesktop.appearance",
            "color-scheme",
        ],
    )?;
    parse_portal_color_scheme(&value).map(str::to_string)
}

fn parse_color_component(value: &str) -> Option<f64> {
    let parsed = value.trim().parse::<f64>().ok()?;
    if parsed > 1.0 {
        Some((parsed / 255.0).clamp(0.0, 1.0))
    } else {
        Some(parsed.clamp(0.0, 1.0))
    }
}

fn parse_color_value(value: &str) -> Option<(f64, f64, f64)> {
    let trimmed = value.trim().trim_matches('"').trim_matches('\'');
    if let Some(hex) = trimmed.strip_prefix('#') {
        if hex.len() >= 6 {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()? as f64 / 255.0;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()? as f64 / 255.0;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()? as f64 / 255.0;
            return Some((r, g, b));
        }
    }

    let parts: Vec<_> = trimmed.split(',').collect();
    if parts.len() >= 3 {
        return Some((
            parse_color_component(parts[0])?,
            parse_color_component(parts[1])?,
            parse_color_component(parts[2])?,
        ));
    }

    None
}

fn relative_luminance((r, g, b): (f64, f64, f64)) -> f64 {
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

fn scheme_from_background_foreground(background: &str, foreground: &str) -> Option<&'static str> {
    let bg = relative_luminance(parse_color_value(background)?);
    let fg = relative_luminance(parse_color_value(foreground)?);
    if bg < fg {
        Some("dark")
    } else {
        Some("light")
    }
}

fn detect_kde_color_scheme_from_brightness(cmd: &str) -> Option<String> {
    let background = read_kde_config_value(cmd, "Colors:Window", "BackgroundNormal")?;
    let foreground = read_kde_config_value(cmd, "Colors:Window", "ForegroundNormal")?;
    scheme_from_background_foreground(&background, &foreground).map(str::to_string)
}

pub(crate) fn try_kreadconfig(cmd: &str) -> Option<String> {
    let value = read_kde_config_value(cmd, "General", "ColorScheme")?;
    scheme_from_theme_name(&value).map(str::to_string)
}

fn detect_gsettings_color_scheme() -> Option<String> {
    let value = command_stdout(
        "gsettings",
        &["get", "org.gnome.desktop.interface", "color-scheme"],
    )?
    .to_lowercase();
    if value.contains("prefer-dark") || value.contains("dark") {
        Some("dark".to_string())
    } else if value.contains("prefer-light") || value.contains("light") || value.contains("default")
    {
        Some("light".to_string())
    } else {
        None
    }
}

fn detect_gtk_theme_name() -> Option<String> {
    if let Ok(theme) = std::env::var("GTK_THEME") {
        if let Some(scheme) = scheme_from_theme_name(&theme) {
            return Some(scheme.to_string());
        }
    }

    let value = command_stdout(
        "gsettings",
        &["get", "org.gnome.desktop.interface", "gtk-theme"],
    )?;
    scheme_from_theme_name(&value).map(str::to_string)
}

pub(crate) fn detect_system_theme_from_platform() -> String {
    // QML SystemPalette is still the primary live source when themeMode=system.
    // This Rust value is a backend fallback: portal > KDE color brightness >
    // GNOME/GTK explicit color-scheme > theme-name fallback > light.
    detect_portal_color_scheme()
        .or_else(|| detect_kde_color_scheme_from_brightness("kreadconfig6"))
        .or_else(|| detect_kde_color_scheme_from_brightness("kreadconfig5"))
        .or_else(detect_gsettings_color_scheme)
        .or_else(detect_gtk_theme_name)
        .or_else(|| try_kreadconfig("kreadconfig6"))
        .or_else(|| try_kreadconfig("kreadconfig5"))
        .unwrap_or_else(|| "light".to_string())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_portal_color_scheme() {
        assert_eq!(parse_portal_color_scheme("(<uint32 1>,)"), Some("dark"));
        assert_eq!(parse_portal_color_scheme("(<uint32 2>,)"), Some("light"));
        assert_eq!(parse_portal_color_scheme("(<uint32 0>,)"), Some("light"));
        assert_eq!(parse_portal_color_scheme("(<uint32 99>,)"), None);
    }

    #[test]
    fn parses_kde_rgb_brightness() {
        assert_eq!(
            scheme_from_background_foreground("35,38,39", "239,240,241"),
            Some("dark")
        );
        assert_eq!(
            scheme_from_background_foreground("239,240,241", "35,38,39"),
            Some("light")
        );
    }

    #[test]
    fn parses_hex_brightness() {
        assert_eq!(
            scheme_from_background_foreground("#202124", "#e8eaed"),
            Some("dark")
        );
        assert_eq!(
            scheme_from_background_foreground("#ffffff", "#1a1c1e"),
            Some("light")
        );
    }
}
