// =============================================================================
// diagnostics.rs — Desktop 诊断日志管理
// =============================================================================
//
// 引用了什么：
// - std::fs / std::path：文件系统操作
// - std::io：I/O 读写
// - chrono：时间戳格式化
// - serde_json：JSON 序列化
//
// 干什么的：
// - 管理 Desktop 端诊断日志的写入、轮转、清空、导出
// - 收集设备信息（OS、架构、Qt 版本等）
// - 导出诊断包（zip 格式，包含日志、设备信息、设置快照）
// - 脱敏规则：与 Android DiagnosticsLogger.REDACT_RULES 对齐
//
// 被什么引用：
// - 被 settings_backend.rs 调用，实现 QML 可调用的诊断操作方法
// =============================================================================

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

/// 日志文件最大大小（1 MiB），超过则轮转
const MAX_FILE_SIZE: u64 = 1024 * 1024;
/// 最多保留的日志文件数
const MAX_LOG_FILES: usize = 5;
/// 日志文件前缀
const LOG_PREFIX: &str = "sujian-current";

/// 全局日志目录路径，在 main() 最早期通过 init_global_log_dir() 或 ensure_early_log_dir() 设置
static GLOBAL_LOG_DIR: OnceLock<PathBuf> = OnceLock::new();

/// 全局 diagnostics_enabled 开关状态，由 SettingsBackend 在设置变更时同步更新
/// 默认 true（alpha 阶段），稳定版应改为 false
/// - ERROR/WARN/CRASH 永远写入，不受此开关控制
/// - INFO/DEBUG/TRACE 同时受 diagnostics_enabled 和 diagnostics_verbose 控制
static DIAGNOSTICS_ENABLED: AtomicBool = AtomicBool::new(true);

/// 全局 verbose 开关状态，由 SettingsBackend 在设置变更时同步更新
/// 默认 true（alpha 阶段），稳定版应改为 false
static VERBOSE_ENABLED: AtomicBool = AtomicBool::new(true);

/// 初始化全局日志目录（使用 workspace 路径）
///
/// 在 workspace 打开时调用，设置日志目录为平台标准路径
pub fn init_global_log_dir(workspace_path: &Path) {
    let log_dir = get_log_dir(workspace_path);
    if let Err(e) = ensure_log_dir(&log_dir) {
        eprintln!("[Diagnostics] init_global_log_dir 创建目录失败: {}", e);
    }
    let _ = GLOBAL_LOG_DIR.set(log_dir);
}

/// 确保在没有 workspace 的情况下也能获取日志目录
///
/// 在 main() 最最开始调用，使用平台默认路径
/// Windows: %APPDATA%/sujian/logs/
/// Linux: ~/.local/share/sujian/logs/
/// macOS: ~/Library/Application Support/sujian/logs/
/// Fallback: /tmp/sujian/logs/
pub fn ensure_early_log_dir() {
    let log_dir = get_early_log_dir();
    if let Err(e) = ensure_log_dir(&log_dir) {
        eprintln!("[Diagnostics] ensure_early_log_dir 创建目录失败: {}", e);
    }
    let _ = GLOBAL_LOG_DIR.set(log_dir);
}

/// 获取早期日志目录（不依赖 workspace）
fn get_early_log_dir() -> PathBuf {
    if cfg!(target_os = "linux") {
        if let Ok(xdg_data) = std::env::var("XDG_DATA_HOME") {
            return PathBuf::from(xdg_data).join("sujian/logs");
        }
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join(".local/share/sujian/logs");
        }
    } else if cfg!(target_os = "windows") {
        if let Ok(appdata) = std::env::var("APPDATA") {
            return PathBuf::from(appdata).join("sujian/logs");
        }
    } else if cfg!(target_os = "macos") {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join("Library/Application Support/sujian/logs");
        }
    }
    // Fallback: /tmp/sujian/logs
    PathBuf::from("/tmp/sujian/logs")
}

/// 获取当前全局日志目录
///
/// 优先返回 GLOBAL_LOG_DIR（已初始化），否则返回早期日志目录
pub fn get_global_log_dir() -> PathBuf {
    GLOBAL_LOG_DIR
        .get()
        .cloned()
        .unwrap_or_else(get_early_log_dir)
}

/// 写入结构化日志到文件
///
/// 格式：[YYYY-MM-DD HH:MM:SS.mmm] [LEVEL] [module::event] message
/// 自动脱敏，自动获取全局日志目录
/// - ERROR/WARN/CRASH 级别永远写入（不受任何开关控制）
/// - INFO/DEBUG/TRACE 级别同时受 diagnostics_enabled 和 diagnostics_verbose 控制
pub fn log_to_file(level: &str, module: &str, event: &str, message: &str) {
    let level_upper = level.to_uppercase();
    let should_write = match level_upper.as_str() {
        "ERROR" | "WARN" | "CRASH" => true,
        "INFO" | "DEBUG" | "TRACE" => is_diagnostics_enabled() && is_verbose_enabled(),
        _ => true, // 未知级别默认写入
    };

    if !should_write {
        return;
    }

    let log_dir = get_global_log_dir();
    let _ = ensure_log_dir(&log_dir);
    let _ = rotate_if_needed(&log_dir);

    let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
    let formatted = format!(
        "[{}] [{}] [{}::{}] {}",
        timestamp, level_upper, module, event, message
    );
    let redacted = redact(&formatted);

    let current_file = log_dir.join(format!("{}.log", LOG_PREFIX));
    if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(&current_file) {
        let _ = writeln!(file, "{}", redacted);
    }
}

/// 检查 diagnostics_enabled 是否开启
///
/// 读取全局 AtomicBool 状态，由 SettingsBackend 在设置变更时通过 set_diagnostics_enabled() 同步更新
fn is_diagnostics_enabled() -> bool {
    DIAGNOSTICS_ENABLED.load(Ordering::Relaxed)
}

/// 设置 diagnostics_enabled 开关状态
///
/// 由 SettingsBackend 在读取/更新设置时调用，同步全局 AtomicBool 状态
pub fn set_diagnostics_enabled(enabled: bool) {
    DIAGNOSTICS_ENABLED.store(enabled, Ordering::Relaxed);
}

/// 检查 diagnostics_verbose 是否开启
///
/// 读取全局 AtomicBool 状态，由 SettingsBackend 在设置变更时通过 set_verbose_enabled() 同步更新
fn is_verbose_enabled() -> bool {
    VERBOSE_ENABLED.load(Ordering::Relaxed)
}

/// 设置 diagnostics_verbose 开关状态
///
/// 由 SettingsBackend 在读取/更新设置时调用，同步全局 AtomicBool 状态
pub fn set_verbose_enabled(enabled: bool) {
    VERBOSE_ENABLED.store(enabled, Ordering::Relaxed);
}

/// 安装 panic hook，将 panic 信息写入日志文件
pub fn install_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let panic_msg = format!("PANIC: {}", info);
        // 直接写入文件（不用 log_to_file 避免依赖 is_verbose_enabled）
        let log_dir = get_global_log_dir();
        let _ = ensure_log_dir(&log_dir);
        let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
        let formatted = format!("[{}] [ERROR] [panic::hook] {}", timestamp, panic_msg);
        let redacted_msg = redact(&formatted);
        let current_file = log_dir.join(format!("{}.log", LOG_PREFIX));
        if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(&current_file) {
            let _ = writeln!(file, "{}", redacted_msg);
        }
        // 仍然调用默认 hook（打印到 stderr）
        default_hook(info);
    }));
}

/// 脱敏正则规则列表
/// 与 Android DiagnosticsLogger.REDACT_RULES 对齐
fn redact(message: &str) -> String {
    let mut result = message.to_string();

    // SSH private key blocks
    let re_ssh_key = regex::Regex::new(
        r"(?i)ssh_private_key\s*[:=]\s*[\s\S]*?-----END[^\n]*PRIVATE KEY-----",
    )
    .unwrap_or_else(|_| regex::Regex::new(r"$^").unwrap());
    result = re_ssh_key
        .replace_all(&result, "ssh_private_key=[REDACTED]")
        .to_string();

    // PEM private key blocks
    let re_pem = regex::Regex::new(
        r"-----BEGIN[^\n]*PRIVATE KEY-----[\s\S]*?-----END[^\n]*PRIVATE KEY-----",
    )
    .unwrap_or_else(|_| regex::Regex::new(r"$^").unwrap());
    result = re_pem
        .replace_all(&result, "[REDACTED_PEM]")
        .to_string();

    // Bearer token in header-style (Authorization: Bearer xxx)
    let re_bearer_header = regex::Regex::new(r"(?i)\b(authorization)\s*[:=]\s*Bearer\s+\S+")
        .unwrap_or_else(|_| regex::Regex::new(r"$^").unwrap());
    result = re_bearer_header
        .replace_all(&result, "Authorization: Bearer [REDACTED]")
        .to_string();

    // Sensitive key=value pairs (token, password, secret, etc.)
    let re_sensitive_kv = regex::Regex::new(
        r#"(?i)\b(token|access_token|refresh_token|authorization|password|passwd|secret|private_key)\s*[:=]\s*(?:"[^"]*"|\S+)"#,
    )
    .unwrap_or_else(|_| regex::Regex::new(r"$^").unwrap());
    result = re_sensitive_kv
        .replace_all(&result, "$1=[REDACTED]")
        .to_string();

    // Content/body/chapter key=value pairs (user content)
    let re_content_kv = regex::Regex::new(
        r#"(?i)\b(content|text|body|chapter|chapter_content|chapterContent)\s*[:=]\s*(?:"[^"]*"|[^,}\]\n]+)"#,
    )
    .unwrap_or_else(|_| regex::Regex::new(r"$^").unwrap());
    result = re_content_kv
        .replace_all(&result, "$1=[REDACTED]")
        .to_string();

    // Bearer tokens standalone
    let re_bearer_standalone =
        regex::Regex::new(r"(?i)Bearer\s+[A-Za-z0-9\-._~+/]+=*").unwrap_or_else(|_| regex::Regex::new(r"$^").unwrap());
    result = re_bearer_standalone
        .replace_all(&result, "Bearer [REDACTED]")
        .to_string();

    // GitHub PAT patterns
    let re_ghp = regex::Regex::new(r"ghp_[A-Za-z0-9]{36}").unwrap_or_else(|_| regex::Regex::new(r"$^").unwrap());
    result = re_ghp.replace_all(&result, "[REDACTED]").to_string();

    let re_gho = regex::Regex::new(r"gho_[A-Za-z0-9]{36}").unwrap_or_else(|_| regex::Regex::new(r"$^").unwrap());
    result = re_gho.replace_all(&result, "[REDACTED]").to_string();

    let re_github_pat =
        regex::Regex::new(r"github_pat_[A-Za-z0-9_]{82}").unwrap_or_else(|_| regex::Regex::new(r"$^").unwrap());
    result = re_github_pat.replace_all(&result, "[REDACTED]").to_string();

    result
}

/// 获取 Desktop 端日志目录路径
///
/// - Linux: ~/.local/share/sujian/logs/
/// - Windows: %APPDATA%/sujian/logs/
/// - macOS: ~/Library/Application Support/sujian/logs/
/// - Fallback: <workspace>/app-meta/logs/
pub fn get_log_dir(workspace_path: &Path) -> PathBuf {
    // 优先使用平台标准目录
    if cfg!(target_os = "linux") {
        if let Ok(xdg_data) = std::env::var("XDG_DATA_HOME") {
            let dir = PathBuf::from(xdg_data).join("sujian/logs");
            if dir.parent().map_or(false, |p| p.exists()) || dir.exists() {
                return dir;
            }
        }
        if let Ok(home) = std::env::var("HOME") {
            let dir = PathBuf::from(home).join(".local/share/sujian/logs");
            if dir.parent().map_or(false, |p| p.exists()) || dir.exists() {
                return dir;
            }
        }
    } else if cfg!(target_os = "windows") {
        if let Ok(appdata) = std::env::var("APPDATA") {
            let dir = PathBuf::from(appdata).join("sujian/logs");
            return dir;
        }
    } else if cfg!(target_os = "macos") {
        if let Ok(home) = std::env::var("HOME") {
            let dir = PathBuf::from(home).join("Library/Application Support/sujian/logs");
            return dir;
        }
    }

    // Fallback: workspace 内
    workspace_path.join("app-meta/logs")
}

/// 确保日志目录存在
fn ensure_log_dir(log_dir: &Path) -> Result<(), String> {
    if !log_dir.exists() {
        fs::create_dir_all(log_dir).map_err(|e| format!("创建日志目录失败: {}", e))?;
    }
    Ok(())
}

/// 日志轮转：如果当前日志文件超过 MAX_FILE_SIZE，重命名为带时间戳的备份
fn rotate_if_needed(log_dir: &Path) -> Result<(), String> {
    let current_file = log_dir.join(format!("{}.log", LOG_PREFIX));
    if !current_file.exists() {
        return Ok(());
    }
    let metadata = fs::metadata(&current_file).map_err(|e| format!("读取日志文件元数据失败: {}", e))?;
    if metadata.len() < MAX_FILE_SIZE {
        return Ok(());
    }

    let timestamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let rotated = log_dir.join(format!("{}-{}.log", LOG_PREFIX, timestamp));
    fs::rename(&current_file, &rotated).map_err(|e| format!("轮转日志文件失败: {}", e))?;

    prune_old_logs(log_dir);
    Ok(())
}

/// 清理超出 MAX_LOG_FILES 的旧日志文件
fn prune_old_logs(log_dir: &Path) {
    let mut log_files: Vec<_> = match fs::read_dir(log_dir) {
        Ok(entries) => entries
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with(LOG_PREFIX)
                    && e.file_name().to_string_lossy().ends_with(".log")
            })
            .collect(),
        Err(_) => return,
    };

    log_files.sort_by_key(|e| {
        e.metadata()
            .and_then(|m| m.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
    });
    log_files.reverse(); // newest first

    // Keep MAX_LOG_FILES files, delete the rest
    for entry in log_files.iter().skip(MAX_LOG_FILES) {
        let _ = fs::remove_file(entry.path());
    }
}

/// 追加一行日志到当前日志文件
///
/// crash/error/warn 级别永远写入，info/debug/trace 级别同时受 diagnostics_enabled 和 diagnostics_verbose 控制
/// 已废弃 enabled 参数，改用 log_to_file() 进行结构化日志写入
pub fn append_log_line(log_dir: &Path, line: &str, verbose: bool) {
    let upper = line.to_uppercase();
    let is_error_or_warn = upper.contains("ERROR") || upper.contains("CRASH") || upper.contains("WARN");

    if !is_error_or_warn {
        // INFO/DEBUG/TRACE 需要同时满足 diagnostics_enabled 和 verbose
        if !is_diagnostics_enabled() || (!verbose && !is_verbose_enabled()) {
            return;
        }
    }
    let _ = ensure_log_dir(log_dir);
    let _ = rotate_if_needed(log_dir);

    let current_file = log_dir.join(format!("{}.log", LOG_PREFIX));
    if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(&current_file) {
        let redacted = redact(line);
        let _ = writeln!(file, "{}", redacted);
    }
}

/// 清空日志目录中的所有日志文件
pub fn clear_logs(log_dir: &Path) -> Result<(), String> {
    if !log_dir.exists() {
        return Ok(());
    }
    let entries = fs::read_dir(log_dir).map_err(|e| format!("读取日志目录失败: {}", e))?;
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.extension().map_or(false, |ext| ext == "log") {
            let _ = fs::remove_file(&path);
        }
    }
    Ok(())
}

/// 全局 Qt 版本缓存，由 main.rs 通过 set_qt_version() 注入
static QT_VERSION: OnceLock<String> = OnceLock::new();

/// 设置 Qt 版本（由 main.rs 在启动时调用，使用 cpp! 宏获取运行时版本）
pub fn set_qt_version(version: &str) {
    let _ = QT_VERSION.set(version.to_string());
}

/// 收集设备信息并返回 JSON 字符串
///
/// 不依赖运行时 qmake/rustc 命令（Windows 打包后这些命令不存在）。
/// Qt 版本通过 set_qt_version() 由 main.rs 注入。
/// rustc 版本使用编译期 env! 宏。
pub fn device_info_json() -> String {
    let os_type = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    let os_name = if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "unknown"
    };

    let qt_version = QT_VERSION.get().cloned().unwrap_or_else(|| "unknown".to_string());
    let app_version = env!("CARGO_PKG_VERSION");

    let info = serde_json::json!({
        "platform": os_name,
        "osType": os_type,
        "arch": arch,
        "appVersion": app_version,
        "qtVersion": qt_version,
        "rustcVersion": "unknown",
    });

    redact(&serde_json::to_string_pretty(&info).unwrap_or_else(|_| "{}".to_string()))
}

/// 导出诊断包
///
/// 收集日志文件、设备信息、设置快照，打包为 zip，返回 zip 文件路径
///
/// - 有 workspace 且路径可写：导出到 workspace/app-meta/diagnostics
/// - 没 workspace 或路径不可写：导出到平台标准 AppData 目录
///   - Windows: %APPDATA%/sujian/diagnostics
///   - Linux: ~/.local/share/sujian/diagnostics
///   - macOS: ~/Library/Application Support/sujian/diagnostics
///   - Fallback: /tmp/sujian/diagnostics
pub fn export_diagnostics_pack(
    workspace_path: &Path,
    log_dir: &Path,
) -> Result<PathBuf, String> {
    let timestamp = chrono::Local::now().format("%Y%m%d-%H%M%S");

    // 确定导出目录：优先 workspace，不可写则用平台标准目录
    let export_dir = determine_export_dir(workspace_path);
    ensure_log_dir(&export_dir)?;

    // Clean previous exports
    if let Ok(entries) = fs::read_dir(&export_dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let _ = fs::remove_file(entry.path());
        }
    }

    let zip_path = export_dir.join(format!("sujian-diagnostics-{}.zip", timestamp));
    let temp_dir = export_dir.join(format!("temp_{}", timestamp));
    fs::create_dir_all(&temp_dir).map_err(|e| format!("创建临时目录失败: {}", e))?;

    // Write logs
    write_logs_to_dir(log_dir, &temp_dir);

    // Write device info
    write_device_info(&temp_dir);

    // Write settings snapshot (sanitized) — only if workspace exists
    if !workspace_path.as_os_str().is_empty() && workspace_path.exists() {
        write_settings_snapshot(workspace_path, &temp_dir);
    }

    // Zip the temp dir
    zip_directory(&temp_dir, &zip_path)?;

    // Clean temp dir
    let _ = fs::remove_dir_all(&temp_dir);

    Ok(zip_path)
}

/// 确定诊断包导出目录
///
/// 优先使用 workspace/app-meta/diagnostics，不可写则回退到平台标准目录
fn determine_export_dir(workspace_path: &Path) -> PathBuf {
    // 1. 尝试 workspace 路径
    if !workspace_path.as_os_str().is_empty() {
        let ws_export = workspace_path.join("app-meta/diagnostics");
        // 尝试创建目录，成功则可用
        if fs::create_dir_all(&ws_export).is_ok() {
            // 验证可写：尝试创建并删除一个临时文件
            let test_file = ws_export.join(".write_test");
            if let Ok(mut f) = fs::File::create(&test_file) {
                let _ = write!(f, "test");
                drop(f);
                let _ = fs::remove_file(&test_file);
                return ws_export;
            }
        }
    }

    // 2. 回退到平台标准目录
    if cfg!(target_os = "windows") {
        if let Ok(appdata) = std::env::var("APPDATA") {
            return PathBuf::from(appdata).join("sujian/diagnostics");
        }
    } else if cfg!(target_os = "linux") {
        if let Ok(xdg_data) = std::env::var("XDG_DATA_HOME") {
            return PathBuf::from(xdg_data).join("sujian/diagnostics");
        }
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join(".local/share/sujian/diagnostics");
        }
    } else if cfg!(target_os = "macos") {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join("Library/Application Support/sujian/diagnostics");
        }
    }

    // 3. 最终回退
    PathBuf::from("/tmp/sujian/diagnostics")
}

fn write_logs_to_dir(log_dir: &Path, dest_dir: &Path) {
    let logs_dest = dest_dir.join("logs");
    if let Err(e) = fs::create_dir_all(&logs_dest) {
        eprintln!("[Diagnostics] 创建日志目标目录失败: {}", e);
        return;
    }

    if !log_dir.exists() {
        return;
    }

    if let Ok(entries) = fs::read_dir(log_dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.extension().map_or(false, |ext| ext == "log") {
                if let Ok(content) = fs::read_to_string(&path) {
                    let redacted = redact(&content);
                    let dest = logs_dest.join(path.file_name().unwrap_or_default());
                    let _ = fs::write(&dest, redacted);
                }
            }
        }
    }
}

fn write_device_info(dest_dir: &Path) {
    let json = device_info_json();
    let _ = fs::write(dest_dir.join("current_device.json"), json);
}

fn write_settings_snapshot(workspace_path: &Path, dest_dir: &Path) {
    let settings_path = workspace_path.join("app-meta/settings/settings.local.json");
    if let Ok(content) = fs::read_to_string(&settings_path) {
        let redacted = redact(&content);
        let _ = fs::write(dest_dir.join("app_settings_sanitized.json"), redacted);
    }
}

fn zip_directory(source_dir: &Path, zip_path: &Path) -> Result<(), String> {
    let file = fs::File::create(zip_path).map_err(|e| format!("创建 zip 文件失败: {}", e))?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    zip_dir_recursive(source_dir, source_dir, &mut zip, &options)?;

    zip.finish()
        .map_err(|e| format!("完成 zip 写入失败: {}", e))?;
    Ok(())
}

fn zip_dir_recursive(
    base_dir: &Path,
    current_dir: &Path,
    zip: &mut zip::ZipWriter<fs::File>,
    options: &zip::write::SimpleFileOptions,
) -> Result<(), String> {
    let entries = fs::read_dir(current_dir).map_err(|e| format!("读取目录失败: {}", e))?;
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_dir() {
            zip_dir_recursive(base_dir, &path, zip, options)?;
        } else {
            let relative = path
                .strip_prefix(base_dir)
                .map_err(|e| format!("计算相对路径失败: {}", e))?;
            let entry_name = relative.to_string_lossy().replace('\\', "/");
            zip.start_file(&entry_name, *options)
                .map_err(|e| format!("创建 zip 条目失败: {}", e))?;
            let content =
                fs::read(&path).map_err(|e| format!("读取文件失败: {}: {}", path.display(), e))?;
            zip.write_all(&content)
                .map_err(|e| format!("写入 zip 条目失败: {}", e))?;
        }
    }
    Ok(())
}

/// 用系统文件管理器打开日志目录
pub fn open_log_directory(log_dir: &Path) -> Result<(), String> {
    ensure_log_dir(log_dir)?;
    crate::platform_utils::open_directory(&log_dir.to_string_lossy())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use tempfile::tempdir;

    /// 全局状态测试的互斥锁，确保修改 DIAGNOSTICS_ENABLED / VERBOSE_ENABLED 的测试串行执行，
    /// 避免并行测试竞争导致 flaky failure。
    static GLOBAL_STATE_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn test_redact_bearer_token() {
        let input = r#"Authorization: Bearer ghp_1234567890abcdefghijklmnopqrstuvwx"#;
        let result = redact(input);
        assert!(!result.contains("ghp_1234567890"));
        assert!(result.contains("[REDACTED]"));
    }

    #[test]
    fn test_redact_github_pat() {
        let input = "token: ghp_1234567890abcdefghijklmnopqrstuvwx";
        let result = redact(input);
        assert!(!result.contains("ghp_1234567890"));
    }

    #[test]
    fn test_redact_password() {
        let input = r#"password: "mysecret123""#;
        let result = redact(input);
        assert!(!result.contains("mysecret123"));
    }

    #[test]
    fn test_redact_preserves_safe_content() {
        let input = "themeMode: dark autoSaveEnabled: true";
        let result = redact(input);
        assert!(result.contains("themeMode: dark"));
        assert!(result.contains("autoSaveEnabled: true"));
    }

    #[test]
    fn test_log_rotation() {
        let dir = tempdir().unwrap();
        let log_dir = dir.path();

        // Create a large log file
        let current_file = log_dir.join(format!("{}.log", LOG_PREFIX));
        let large_content = "x".repeat((MAX_FILE_SIZE + 1) as usize);
        fs::write(&current_file, &large_content).unwrap();

        rotate_if_needed(log_dir).unwrap();

        // Current file should no longer exist (renamed)
        assert!(!current_file.exists());

        // A rotated file should exist
        let rotated: Vec<_> = fs::read_dir(log_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with(LOG_PREFIX)
                    && e.file_name().to_string_lossy() != format!("{}.log", LOG_PREFIX)
            })
            .collect();
        assert_eq!(rotated.len(), 1);
    }

    #[test]
    fn test_clear_logs() {
        let dir = tempdir().unwrap();
        let log_dir = dir.path();

        // Create some log files
        fs::write(log_dir.join("sujian-current.log"), "test").unwrap();
        fs::write(log_dir.join("sujian-current-20260101.log"), "old").unwrap();
        fs::write(log_dir.join("other.txt"), "keep").unwrap();

        clear_logs(log_dir).unwrap();

        assert!(!log_dir.join("sujian-current.log").exists());
        assert!(!log_dir.join("sujian-current-20260101.log").exists());
        assert!(log_dir.join("other.txt").exists());
    }

    #[test]
    fn test_device_info_json_is_valid() {
        let json = device_info_json();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed["platform"].is_string());
        assert!(parsed["arch"].is_string());
    }

    #[test]
    fn test_export_diagnostics_pack_returns_zip_path_and_export_dir() {
        let workspace = tempdir().unwrap();
        let log_dir = tempdir().unwrap();
        fs::write(log_dir.path().join("sujian-current.log"), "safe log line\n").unwrap();

        let zip_path = export_diagnostics_pack(workspace.path(), log_dir.path()).unwrap();
        assert!(zip_path.exists(), "zip should exist: {}", zip_path.display());
        assert_eq!(zip_path.extension().and_then(|e| e.to_str()), Some("zip"));
        let export_dir = zip_path.parent().expect("zip has parent export dir");
        assert!(export_dir.ends_with("app-meta/diagnostics"), "unexpected export dir: {}", export_dir.display());
    }

    #[test]
    fn test_append_log_line() {
        let prev_enabled = DIAGNOSTICS_ENABLED.load(Ordering::Relaxed);
        let prev_verbose = VERBOSE_ENABLED.load(Ordering::Relaxed);
        DIAGNOSTICS_ENABLED.store(true, Ordering::Relaxed);
        VERBOSE_ENABLED.store(true, Ordering::Relaxed);

        let dir = tempdir().unwrap();
        let log_dir = dir.path();

        append_log_line(log_dir, "Test log message", true);

        let current_file = log_dir.join(format!("{}.log", LOG_PREFIX));
        assert!(current_file.exists());
        let content = fs::read_to_string(&current_file).unwrap();
        assert!(content.contains("Test log message"));

        DIAGNOSTICS_ENABLED.store(prev_enabled, Ordering::Relaxed);
        VERBOSE_ENABLED.store(prev_verbose, Ordering::Relaxed);
    }

    #[test]
    fn test_append_log_line_error_always_writes() {
        let dir = tempdir().unwrap();
        let log_dir = dir.path();

        // ERROR level should always write even with verbose=false
        append_log_line(log_dir, "ERROR something went wrong", false);

        let current_file = log_dir.join(format!("{}.log", LOG_PREFIX));
        assert!(current_file.exists());
        let content = fs::read_to_string(&current_file).unwrap();
        assert!(content.contains("ERROR something went wrong"));
    }

    #[test]
    fn test_early_log_dir_creates_directory() {
        let dir = tempdir().unwrap();
        let test_log_dir = dir.path().join("sujian/logs_early_test");

        // Simulate ensure_early_log_dir by manually creating the directory
        fs::create_dir_all(&test_log_dir).unwrap();
        assert!(test_log_dir.exists());

        // Clean up
        let _ = fs::remove_dir_all(dir.path());
    }

    #[test]
    fn test_log_to_file_writes_content() {
        let dir = tempdir().unwrap();
        let test_log_dir = dir.path().to_path_buf();

        // Set the global log dir for this test.
        // OnceLock can only be set once; if another test already set it, skip this test
        // rather than failing due to writing to the wrong directory.
        if GLOBAL_LOG_DIR.set(test_log_dir.clone()).is_err() {
            eprintln!("test_log_to_file_writes_content: skipped (GLOBAL_LOG_DIR already set by another test)");
            return;
        }

        log_to_file("ERROR", "test_module", "test_event", "test error message");

        let current_file = test_log_dir.join(format!("{}.log", LOG_PREFIX));
        assert!(current_file.exists());
        let content = fs::read_to_string(&current_file).unwrap();
        assert!(content.contains("[ERROR]"));
        assert!(content.contains("[test_module::test_event]"));
        assert!(content.contains("test error message"));
    }

    #[test]
    fn test_panic_hook_writes_to_file() {
        let dir = tempdir().unwrap();
        let test_log_dir = dir.path().to_path_buf();

        // Set the global log dir for this test (best-effort; OnceLock may already be set)
        let _ = GLOBAL_LOG_DIR.set(test_log_dir.clone());

        // Simulate what the panic hook does
        let panic_msg = "PANIC: test panic message";
        let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
        let formatted = format!("[{}] [ERROR] [panic::hook] {}", timestamp, panic_msg);
        let redacted_msg = redact(&formatted);
        let current_file = test_log_dir.join(format!("{}.log", LOG_PREFIX));
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&current_file)
            .unwrap();
        writeln!(file, "{}", redacted_msg).unwrap();

        let content = fs::read_to_string(&current_file).unwrap();
        assert!(content.contains("PANIC: test panic message"));
        assert!(content.contains("[ERROR]"));
        assert!(content.contains("[panic::hook]"));
    }

    #[test]
    fn test_verbose_disabled_filters_info() {
        let _lock = GLOBAL_STATE_LOCK.lock().unwrap();
        // 保存原始状态
        let prev_enabled = DIAGNOSTICS_ENABLED.load(Ordering::Relaxed);
        let prev_verbose = VERBOSE_ENABLED.load(Ordering::Relaxed);
        DIAGNOSTICS_ENABLED.store(true, Ordering::Relaxed);
        VERBOSE_ENABLED.store(false, Ordering::Relaxed);

        let dir = tempdir().unwrap();
        let log_dir = dir.path();

        // verbose=false 时，INFO 级别不应写入
        append_log_line(log_dir, "[INFO] some info message", false);

        let current_file = log_dir.join(format!("{}.log", LOG_PREFIX));
        if current_file.exists() {
            let content = fs::read_to_string(&current_file).unwrap();
            assert!(!content.contains("some info message"), "INFO should be filtered when verbose is disabled");
        }

        // verbose=false 时，WARN/ERROR 仍应写入
        append_log_line(log_dir, "[WARN] some warning", false);
        append_log_line(log_dir, "[ERROR] some error", false);

        let current_file = log_dir.join(format!("{}.log", LOG_PREFIX));
        let content = fs::read_to_string(&current_file).unwrap();
        assert!(content.contains("some warning"), "WARN should always be written");
        assert!(content.contains("some error"), "ERROR should always be written");

        // 恢复原始状态
        DIAGNOSTICS_ENABLED.store(prev_enabled, Ordering::Relaxed);
        VERBOSE_ENABLED.store(prev_verbose, Ordering::Relaxed);
    }

    #[test]
    fn test_verbose_enabled_writes_all() {
        let _lock = GLOBAL_STATE_LOCK.lock().unwrap();
        let prev_enabled = DIAGNOSTICS_ENABLED.load(Ordering::Relaxed);
        let prev_verbose = VERBOSE_ENABLED.load(Ordering::Relaxed);
        DIAGNOSTICS_ENABLED.store(true, Ordering::Relaxed);
        VERBOSE_ENABLED.store(true, Ordering::Relaxed);

        let dir = tempdir().unwrap();
        let log_dir = dir.path();

        append_log_line(log_dir, "[INFO] some info message", true);

        let current_file = log_dir.join(format!("{}.log", LOG_PREFIX));
        assert!(current_file.exists());
        let content = fs::read_to_string(&current_file).unwrap();
        assert!(content.contains("some info message"), "INFO should be written when verbose is enabled");

        DIAGNOSTICS_ENABLED.store(prev_enabled, Ordering::Relaxed);
        VERBOSE_ENABLED.store(prev_verbose, Ordering::Relaxed);
    }

    #[test]
    fn test_set_verbose_enabled_updates_global() {
        let _lock = GLOBAL_STATE_LOCK.lock().unwrap();
        let prev = VERBOSE_ENABLED.load(Ordering::Relaxed);

        set_verbose_enabled(false);
        assert!(!is_verbose_enabled());

        set_verbose_enabled(true);
        assert!(is_verbose_enabled());

        VERBOSE_ENABLED.store(prev, Ordering::Relaxed);
    }

    #[test]
    fn test_error_always_written_even_when_disabled() {
        let _lock = GLOBAL_STATE_LOCK.lock().unwrap();
        let prev_enabled = DIAGNOSTICS_ENABLED.load(Ordering::Relaxed);
        let prev_verbose = VERBOSE_ENABLED.load(Ordering::Relaxed);
        DIAGNOSTICS_ENABLED.store(false, Ordering::Relaxed);
        VERBOSE_ENABLED.store(false, Ordering::Relaxed);

        let dir = tempdir().unwrap();
        let log_dir = dir.path();

        // 使用 append_log_line（接受 log_dir 参数）而非 log_to_file（使用全局目录）
        append_log_line(log_dir, "[ERROR] error message when disabled", false);

        let current_file = log_dir.join(format!("{}.log", LOG_PREFIX));
        let content = fs::read_to_string(&current_file).unwrap();
        assert!(content.contains("error message when disabled"), "ERROR should always be written even when diagnostics disabled");

        DIAGNOSTICS_ENABLED.store(prev_enabled, Ordering::Relaxed);
        VERBOSE_ENABLED.store(prev_verbose, Ordering::Relaxed);
    }

    #[test]
    fn test_warn_always_written_even_when_disabled() {
        let _lock = GLOBAL_STATE_LOCK.lock().unwrap();
        let prev_enabled = DIAGNOSTICS_ENABLED.load(Ordering::Relaxed);
        let prev_verbose = VERBOSE_ENABLED.load(Ordering::Relaxed);
        DIAGNOSTICS_ENABLED.store(false, Ordering::Relaxed);
        VERBOSE_ENABLED.store(false, Ordering::Relaxed);

        let dir = tempdir().unwrap();
        let log_dir = dir.path();

        append_log_line(log_dir, "[WARN] warn message when disabled", false);

        let current_file = log_dir.join(format!("{}.log", LOG_PREFIX));
        let content = fs::read_to_string(&current_file).unwrap();
        assert!(content.contains("warn message when disabled"), "WARN should always be written even when diagnostics disabled");

        DIAGNOSTICS_ENABLED.store(prev_enabled, Ordering::Relaxed);
        VERBOSE_ENABLED.store(prev_verbose, Ordering::Relaxed);
    }

    #[test]
    fn test_info_requires_both_enabled() {
        let _lock = GLOBAL_STATE_LOCK.lock().unwrap();
        let prev_enabled = DIAGNOSTICS_ENABLED.load(Ordering::Relaxed);
        let prev_verbose = VERBOSE_ENABLED.load(Ordering::Relaxed);
        DIAGNOSTICS_ENABLED.store(true, Ordering::Relaxed);
        VERBOSE_ENABLED.store(true, Ordering::Relaxed);

        let dir = tempdir().unwrap();
        let log_dir = dir.path();

        append_log_line(log_dir, "[INFO] info message with both enabled", true);

        let current_file = log_dir.join(format!("{}.log", LOG_PREFIX));
        let content = fs::read_to_string(&current_file).unwrap();
        assert!(content.contains("info message with both enabled"), "INFO should be written when both enabled and verbose");

        DIAGNOSTICS_ENABLED.store(prev_enabled, Ordering::Relaxed);
        VERBOSE_ENABLED.store(prev_verbose, Ordering::Relaxed);
    }

    #[test]
    fn test_info_blocked_when_disabled() {
        let _lock = GLOBAL_STATE_LOCK.lock().unwrap();
        let prev_enabled = DIAGNOSTICS_ENABLED.load(Ordering::Relaxed);
        let prev_verbose = VERBOSE_ENABLED.load(Ordering::Relaxed);
        DIAGNOSTICS_ENABLED.store(false, Ordering::Relaxed);
        VERBOSE_ENABLED.store(true, Ordering::Relaxed);

        let dir = tempdir().unwrap();
        let log_dir = dir.path();

        append_log_line(log_dir, "[INFO] info message when disabled", true);

        let current_file = log_dir.join(format!("{}.log", LOG_PREFIX));
        if current_file.exists() {
            let content = fs::read_to_string(&current_file).unwrap();
            assert!(!content.contains("info message when disabled"), "INFO should NOT be written when diagnostics_enabled=false");
        }

        DIAGNOSTICS_ENABLED.store(prev_enabled, Ordering::Relaxed);
        VERBOSE_ENABLED.store(prev_verbose, Ordering::Relaxed);
    }

    #[test]
    fn test_info_blocked_when_not_verbose() {
        let _lock = GLOBAL_STATE_LOCK.lock().unwrap();
        let prev_enabled = DIAGNOSTICS_ENABLED.load(Ordering::Relaxed);
        let prev_verbose = VERBOSE_ENABLED.load(Ordering::Relaxed);
        DIAGNOSTICS_ENABLED.store(true, Ordering::Relaxed);
        VERBOSE_ENABLED.store(false, Ordering::Relaxed);

        let dir = tempdir().unwrap();
        let log_dir = dir.path();

        append_log_line(log_dir, "[INFO] info message when not verbose", false);

        let current_file = log_dir.join(format!("{}.log", LOG_PREFIX));
        if current_file.exists() {
            let content = fs::read_to_string(&current_file).unwrap();
            assert!(!content.contains("info message when not verbose"), "INFO should NOT be written when diagnostics_verbose=false");
        }

        DIAGNOSTICS_ENABLED.store(prev_enabled, Ordering::Relaxed);
        VERBOSE_ENABLED.store(prev_verbose, Ordering::Relaxed);
    }

    #[test]
    fn test_set_diagnostics_enabled_updates_global() {
        let _lock = GLOBAL_STATE_LOCK.lock().unwrap();
        let prev = DIAGNOSTICS_ENABLED.load(Ordering::Relaxed);

        set_diagnostics_enabled(false);
        assert!(!is_diagnostics_enabled());

        set_diagnostics_enabled(true);
        assert!(is_diagnostics_enabled());

        DIAGNOSTICS_ENABLED.store(prev, Ordering::Relaxed);
    }
}
