//! 最早期启动诊断记录器。
//!
//! 只依赖 `std`，不依赖 Qt、QML 或 WriterCore 初始化成功。用于在进程最早期
//! 记录启动阶段、环境信息，并安装 panic hook 把崩溃信息写入诊断目录。
//!
//! ## 目录布局
//!
//! ```text
//! ~/.sujianxiezuo/
//! └── diagnostics/
//!     ├── startup/
//!     │   ├── latest.log
//!     │   └── history/
//!     │       └── startup-YYYYMMDD-HHMMSS-PID.log
//!     ├── runtime/
//!     ├── crash/
//!     │   └── crash-YYYYMMDD-HHMMSS-PID.log
//!     └── exports/
//! ```
//!
//! ## 安全约束
//!
//! - 不写 token、正文、仓库地址等敏感信息。
//! - panic hook 里不能再 panic：所有 IO 错误都静默忽略。
//! - `mark` 每次以 append 模式重新打开文件写入，即使 panic 也不会丢数据。

use std::path::PathBuf;
use std::sync::OnceLock;

use super::dirs::{crash_diagnostics_dir, startup_diagnostics_dir};

/// 当前启动 session 的日志路径，供 panic hook 访问。
static CURRENT_SESSION_LOG: OnceLock<PathBuf> = OnceLock::new();

/// 启动诊断句柄。持有本次 session 的日志文件路径。
///
/// 创建由 [`begin_startup_diagnostics`] 完成，之后可通过 [`mark`] / [`mark_ready`] /
/// [`mark_exit`] 追加阶段记录。句柄本身不持有文件句柄，每次写入都重新以 append
/// 模式打开文件，保证 panic 安全。
///
/// [`mark`]: StartupDiagnostics::mark
/// [`mark_ready`]: StartupDiagnostics::mark_ready
/// [`mark_exit`]: StartupDiagnostics::mark_exit
pub struct StartupDiagnostics {
    session_log: PathBuf,
}

/// 启动最早期诊断记录，返回本次 session 的句柄。
///
/// 立即 `create_dir_all` 建立启动历史目录和崩溃目录，写入本次 session 文件
/// （`history/startup-YYYYMMDD-HHMMSS-PID.log`），并把 `latest.log` 更新为本次
/// 启动日志（原子写：先写临时文件再 rename 覆盖）。同时安装 panic hook。
///
/// 该函数应在 `main` 最早期调用，早于任何 Qt/QML/Core 初始化。
pub fn begin_startup_diagnostics() -> StartupDiagnostics {
    let startup_dir = startup_diagnostics_dir();
    let history_dir = startup_dir.join("history");
    let _ = std::fs::create_dir_all(&history_dir);
    let _ = std::fs::create_dir_all(crash_diagnostics_dir());

    let (ts_file, ts_human) = current_timestamp();
    let pid = std::process::id();
    let session_log = history_dir.join(format!("startup-{}-{}.log", ts_file, pid));

    // 写启动头信息到 session 文件。
    let header = format_startup_header(&ts_human, pid);
    let _ = std::fs::write(&session_log, &header);

    // 原子更新 latest.log：写临时文件再 rename 覆盖。
    let latest = startup_dir.join("latest.log");
    atomic_write(&latest, &header);

    // 记划之后 mark 也同步追加到 latest.log，所以这里 latest.log 已包含 header。

    // 注册当前 session 路径供 panic hook 使用。若已被设置（理论上不会），忽略新值。
    let _ = CURRENT_SESSION_LOG.set(session_log.clone());

    // 安装 panic hook（只安装一次，重复调用 begin 不会重复 set_hook）。
    install_panic_hook();

    StartupDiagnostics { session_log }
}

impl StartupDiagnostics {
    /// 记录一个启动阶段。立即 append 写入并 flush，不等异步 logger。
    ///
    /// 格式：`[YYYY-MM-DD HH:MM:SS UTC] stage=<stage> message=<message>\n`。
    /// 同时写入 session history 文件和 `latest.log`。
    pub fn mark(&self, stage: &str, message: &str) {
        let (ts_file, ts_human) = current_timestamp();
        let _ = ts_file; // mark 行只用人类可读时间
        let line = format!("[{} UTC] stage={} message={}\n", ts_human, stage, message);
        append_and_flush(&self.session_log, &line);

        let latest = startup_diagnostics_dir().join("latest.log");
        append_and_flush(&latest, &line);
    }

    /// 标记 GUI 事件循环已就绪。等价于 `mark("gui_ready", "event loop ready")`。
    pub fn mark_ready(&self) {
        self.mark("gui_ready", "event loop ready");
    }

    /// 标记进程退出。写入 `stage=process_exit message=exit_code=<code>` 并 flush。
    pub fn mark_exit(&self, code: i32) {
        self.mark("process_exit", &format!("exit_code={}", code));
    }
}

/// 以 append 模式打开文件写入一行并 flush。任何 IO 错误都静默忽略。
fn append_and_flush(path: &PathBuf, line: &str) {
    use std::io::Write;
    if let Ok(mut file) = std::fs::OpenOptions::new().append(true).create(true).open(path) {
        let _ = file.write_all(line.as_bytes());
        let _ = file.flush();
    }
}

/// 原子写：先写临时文件再 rename 覆盖目标。失败时回退到直接写。
fn atomic_write(target: &PathBuf, content: &str) {
    let tmp = target.with_extension("log.tmp");
    if std::fs::write(&tmp, content).is_ok() {
        if std::fs::rename(&tmp, target).is_ok() {
            return;
        }
        // rename 失败（例如跨设备），清理临时文件后回退。
        let _ = std::fs::remove_file(&tmp);
    }
    let _ = std::fs::write(target, content);
}

/// 安装 panic hook。只安装一次（用 `OnceLock` 守卫）。hook 内不能再 panic。
fn install_panic_hook() {
    static INSTALLED: OnceLock<()> = OnceLock::new();
    if INSTALLED.set(()).is_err() {
        return; // 已安装
    }

    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // 先把崩溃信息落盘，再调用原 hook（可能打印到 stderr）。
        write_crash_report(info);

        prev(info);
    }));
}

/// 把 panic 信息写入 crash 文件，并把摘要追加到当前 session log。
///
/// 所有 IO 错误都静默忽略，确保 hook 内不会再 panic。
fn write_crash_report(info: &std::panic::PanicHookInfo<'_>) {
    let (ts_file, ts_human) = current_timestamp();
    let pid = std::process::id();

    let location = info
        .location()
        .map(|loc| format!("{}:{}:{}", loc.file(), loc.line(), loc.column()))
        .unwrap_or_else(|| "<unknown>".to_string());
    let payload = info.payload().downcast_ref::<&str>().map(|s| s.to_string())
        .or_else(|| info.payload().downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "<non-string panic payload>".to_string());

    // backtrace：force_capture 是 std 稳定 API（1.65+），不依赖 RUST_BACKTRACE 环境变量。
    let backtrace = std::backtrace::Backtrace::force_capture();
    let backtrace_str = format!("{}", backtrace);

    let crash_dir = crash_diagnostics_dir();
    let _ = std::fs::create_dir_all(&crash_dir);
    let crash_path = crash_dir.join(format!("crash-{}-{}.log", ts_file, pid));

    let report = format!(
        "crash report\n\
         timestamp: {} UTC\n\
         pid: {}\n\
         panic location: {}\n\
         panic payload: {}\n\
         rust_backtrace env: {}\n\
         backtrace:\n{}\n",
        ts_human,
        pid,
        location,
        payload,
        std::env::var("RUST_BACKTRACE").unwrap_or_else(|_| "<unset>".to_string()),
        backtrace_str,
    );
    let _ = std::fs::write(&crash_path, &report);

    // 把 crash 摘要追加到当前 session log（如果存在）。
    if let Some(session_log) = CURRENT_SESSION_LOG.get() {
        let summary = format!(
            "[{} UTC] stage=panic message=location={} payload={} crash_file={}\n",
            ts_human, location, payload, crash_path.display(),
        );
        append_and_flush(session_log, &summary);
    }
}

/// 格式化启动头信息。包含时间、PID、版本/build key/package type、环境检测。
///
/// 严格不写 token、正文、仓库地址。
fn format_startup_header(ts_human: &str, pid: u32) -> String {
    let app_version = env!("CARGO_PKG_VERSION");
    let build_key = option_env!("SUJIAN_BUILD_KEY").unwrap_or("unknown");
    let package_type = option_env!("PACKAGE_TYPE").unwrap_or("debug");
    let is_appimage = std::env::var_os("APPIMAGE").is_some();
    let xdg_session_type = env_or_unset("XDG_SESSION_TYPE");
    let wayland_display = if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        "present"
    } else {
        "absent"
    };
    let qt_qpa_platform = env_or_unset("QT_QPA_PLATFORM");
    let qt_im_module = env_or_unset("QT_IM_MODULE");
    let qt_im_modules = env_or_unset("QT_IM_MODULES");

    format!(
        "startup log\n\
         timestamp: {} UTC\n\
         pid: {}\n\
         app_version: {}\n\
         build_key: {}\n\
         package_type: {}\n\
         appimage: {}\n\
         xdg_session_type: {}\n\
         wayland_display: {}\n\
         qt_qpa_platform: {}\n\
         qt_im_module: {}\n\
         qt_im_modules: {}\n\
         ---\n",
        ts_human,
        pid,
        app_version,
        build_key,
        package_type,
        is_appimage,
        xdg_session_type,
        wayland_display,
        qt_qpa_platform,
        qt_im_module,
        qt_im_modules,
    )
}

/// 读取环境变量，缺失时返回 `<unset>`。
fn env_or_unset(key: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| "<unset>".to_string())
}

/// 返回当前 UTC 时间戳的两种格式。
///
/// 返回 `(file_ts, human_ts)`：
/// - `file_ts`: `YYYYMMDD-HHMMSS`，用于文件名。
/// - `human_ts`: `YYYY-MM-DD HH:MM:SS`，用于日志内容。
fn current_timestamp() -> (String, String) {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let (year, month, day, hour, minute, second) = epoch_to_civil(secs);
    let file_ts = format!(
        "{:04}{:02}{:02}-{:02}{:02}{:02}",
        year, month, day, hour, minute, second,
    );
    let human_ts = format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        year, month, day, hour, minute, second,
    );
    (file_ts, human_ts)
}

/// Unix epoch 秒数转 UTC 公历时间。
///
/// 使用 Howard Hinnant 的 days-from-civil 逆算法，正确处理闰年，不处理闰秒。
/// 返回 `(year, month, day, hour, minute, second)`，均为公历 UTC。
fn epoch_to_civil(secs: u64) -> (u64, u64, u64, u64, u64, u64) {
    let days = (secs / 86_400) as i64;
    let time = secs % 86_400;
    let hour = time / 3600;
    let minute = (time % 3600) / 60;
    let second = time % 60;

    // days_from_civil 的逆运算：把自 1970-01-01 起的天数转为 (year, month, day)。
    // 参考：http://howardhinnant.github.io/date_algorithms.html#civil_from_days
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    let year = if m <= 2 { y + 1 } else { y };

    (year as u64, m, d, hour, minute, second)
}

#[cfg(test)]
mod tests {
    use super::epoch_to_civil;

    #[test]
    fn epoch_zero_is_1970_01_01() {
        let (y, m, d, h, mi, s) = epoch_to_civil(0);
        assert_eq!((y, m, d, h, mi, s), (1970, 1, 1, 0, 0, 0));
    }

    #[test]
    fn known_timestamp_2024_01_01() {
        // 2024-01-01 00:00:00 UTC = 1704067200
        let (y, m, d, h, mi, s) = epoch_to_civil(1_704_067_200);
        assert_eq!((y, m, d, h, mi, s), (2024, 1, 1, 0, 0, 0));
    }

    #[test]
    fn handles_leap_year_2024_02_29() {
        // 2024-02-29 00:00:00 UTC = 1709164800（2024 是闰年，2 月有 29 天）。
        let (y, m, d, h, mi, s) = epoch_to_civil(1_709_164_800);
        assert_eq!((y, m, d, h, mi, s), (2024, 2, 29, 0, 0, 0));
    }

    #[test]
    fn handles_non_leap_year_2023_03_01() {
        // 2023-03-01 00:00:00 UTC = 1677628800（2023 非闰年，2 月只有 28 天）。
        let (y, m, d, h, mi, s) = epoch_to_civil(1_677_628_800);
        assert_eq!((y, m, d, h, mi, s), (2023, 3, 1, 0, 0, 0));
    }

    #[test]
    fn time_components_correct() {
        // 2024-01-01 13:45:30 UTC
        let secs = 1_704_067_200 + 13 * 3600 + 45 * 60 + 30;
        let (y, m, d, h, mi, s) = epoch_to_civil(secs);
        assert_eq!((y, m, d, h, mi, s), (2024, 1, 1, 13, 45, 30));
    }
}
