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
    latest_log: PathBuf,
}

/// 启动最早期诊断记录，返回本次 session 的句柄。
///
/// 立即 `create_dir_all` 建立启动历史目录和崩溃目录，写入本次 session 文件
/// （`history/startup-YYYYMMDD-HHMMSS-PID.log`），并把 `latest.log` 更新为本次
/// 启动日志（原子写：先写临时文件再 rename 覆盖）。同时安装 panic hook。
///
/// `app_version` / `build_key` / `package_type` 由调用方（main.rs）传入，确保
/// startup header 反映真实 build identity，而非本 crate 编译期的 option_env! 猜测
/// （本 crate 无 build.rs 注入，option_env! 恒为 None）。见 Issue #803 评论 5905373722。
///
/// 该函数应在 `main` 最早期调用，早于任何 Qt/QML/Core 初始化。
pub fn begin_startup_diagnostics(
    app_version: &str,
    build_key: &str,
    package_type: &str,
) -> StartupDiagnostics {
    // 在创建新 session 之前，检查上一次启动遗留的 pending marker。
    // 如果存在，说明上次启动 panic 后进程未正常退出（既没有 mark_exit(0) 也没有
    // mark_exit(non-zero)），把 pending 指向的 session 提升为 last_failed.log。
    promote_pending_last_failed();

    let startup_dir = startup_diagnostics_dir();
    let history_dir = startup_dir.join("history");
    let _ = std::fs::create_dir_all(&history_dir);
    let _ = std::fs::create_dir_all(crash_diagnostics_dir());

    let latest = startup_dir.join("latest.log");

    // 检测外部是否已建立 session（launcher 或开发脚本已创建 history 和 latest.log）。
    // 以两个路径环境变量同时存在为准，不再依赖 SUJIAN_STARTED_BY_LAUNCHER 名字。
    let external_session_log = std::env::var_os("SUJIAN_STARTUP_SESSION_LOG").map(PathBuf::from);
    let external_latest_log = std::env::var_os("SUJIAN_STARTUP_LATEST_LOG").map(PathBuf::from);
    let has_external_session = external_session_log.is_some() && external_latest_log.is_some();

    let (session_log, latest_log) = if has_external_session {
        // 外部已创建唯一 history 和 latest.log，直接复用，不新建不覆盖。
        let session = external_session_log.unwrap();
        let latest_from_env = external_latest_log.unwrap();
        // 追加 Rust startup header（append，不覆盖外部已写的内容）。
        let (ts_file, ts_human) = current_timestamp();
        let _ = ts_file;
        let pid = std::process::id();
        let header = format_startup_header(&ts_human, pid, app_version, build_key, package_type);
        append_and_flush(&session, &header);
        append_and_flush(&latest_from_env, &header);
        (session, latest_from_env)
    } else {
        // 直接执行真实 ELF（无外部 session），自己建 session。
        let (ts_file, ts_human) = current_timestamp();
        let pid = std::process::id();
        let session_log = history_dir.join(format!("startup-{}-{}.log", ts_file, pid));

        let header = format_startup_header(&ts_human, pid, app_version, build_key, package_type);
        let _ = std::fs::write(&session_log, &header);
        atomic_write(&latest, &header);

        // history 轮转：保留最近 30 份。
        rotate_history(&history_dir, 30);

        (session_log, latest)
    };

    let _ = CURRENT_SESSION_LOG.set(session_log.clone());
    install_panic_hook();

    StartupDiagnostics {
        session_log,
        latest_log,
    }
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

        append_and_flush(&self.latest_log, &line);
    }

    /// 标记 GUI 事件循环已就绪。等价于 `mark("gui_ready", "event loop ready")`。
    pub fn mark_ready(&self) {
        self.mark("gui_ready", "event loop ready");
    }

    /// 标记进程退出。写入 `stage=process_exit message=exit_code=<code>` 并 flush。
    ///
    /// Issue #803 评论 5905373722：把"记录 panic"和"认定本次启动失败"分开。
    /// - 非 0 退出 = 确认失败，提升当前 session 为 last_failed.log，并清掉 pending。
    /// - 正常退出（code == 0）：如果有 pending（之前 panic 被 catch_unwind 恢复），
    ///   清掉 pending，不覆盖 last_failed.log。
    pub fn mark_exit(&self, code: i32) {
        self.mark("process_exit", &format!("exit_code={}", code));
        let pending = startup_diagnostics_dir().join("last_failed.pending");
        if code != 0 {
            // 非 0 退出 = 确认失败，提升当前 session 为 last_failed.log。
            update_last_failed_log(&self.session_log);
            let _ = std::fs::remove_file(&pending);
        } else {
            // 正常退出：如果有 pending（之前 panic 被恢复），清掉，不覆盖 last_failed.log。
            let _ = std::fs::remove_file(&pending);
        }
    }
}

/// 以 append 模式打开文件写入一行并 flush。任何 IO 错误都静默忽略。
fn append_and_flush(path: &PathBuf, line: &str) {
    use std::io::Write;
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(path)
    {
        let _ = file.write_all(line.as_bytes());
        let _ = file.flush();
    }
}

/// 把当前 session 日志复制到 `last_failed.log`，记录最近一次失败启动。
/// IO 错误静默忽略。
fn update_last_failed_log(session_log: &PathBuf) {
    if let Ok(content) = std::fs::read(session_log) {
        let last_failed = startup_diagnostics_dir().join("last_failed.log");
        let _ = std::fs::write(&last_failed, &content);
    }
}

/// 写 pending marker：记录"上一次启动发生了 panic，尚未确认最终退出码"。
/// 内容为当前 session log 路径。IO 错误静默忽略。
///
/// Issue #803 评论 5905373722：panic hook 不再立刻覆盖 last_failed.log，
/// 改由 `mark_exit` 根据真实退出码决定是否提升。
fn write_pending_last_failed(session_log: &PathBuf) {
    let pending = startup_diagnostics_dir().join("last_failed.pending");
    let _ = std::fs::write(&pending, session_log.to_string_lossy().as_bytes());
}

/// 检查上一次启动遗留的 pending marker。如果存在，把指向的 session 提升为
/// `last_failed.log`。IO 错误静默忽略。
///
/// 在 `begin_startup_diagnostics` 最早期调用：如果上次启动 panic 后进程未正常退出
/// （既没有 `mark_exit(0)` 也没有 `mark_exit(non-zero)`），pending 仍残留，
/// 说明那次启动确实失败了，提升为 last_failed.log。
fn promote_pending_last_failed() {
    let pending = startup_diagnostics_dir().join("last_failed.pending");
    if let Ok(session_path_bytes) = std::fs::read(&pending) {
        let session_path = PathBuf::from(String::from_utf8_lossy(&session_path_bytes).to_string());
        if session_path.exists() {
            if let Ok(content) = std::fs::read(&session_path) {
                let last_failed = startup_diagnostics_dir().join("last_failed.log");
                let _ = std::fs::write(&last_failed, &content);
            }
        }
    }
    let _ = std::fs::remove_file(&pending);
}

/// history 轮转：保留最近 `keep` 份 startup-*.log，删除更旧的。
/// IO 错误静默忽略。
fn rotate_history(history_dir: &PathBuf, keep: usize) {
    use std::time::SystemTime;
    let entries = match std::fs::read_dir(history_dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    let mut files: Vec<(SystemTime, PathBuf)> = Vec::new();
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "log") {
            if let Ok(meta) = std::fs::metadata(&path) {
                let mtime = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
                files.push((mtime, path));
            }
        }
    }
    // 按 mtime 降序排列，保留最新的 `keep` 份，删除其余。
    files.sort_by(|a, b| b.0.cmp(&a.0));
    for (_, path) in files.iter().skip(keep) {
        let _ = std::fs::remove_file(path);
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
    let payload = info
        .payload()
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
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
            ts_human,
            location,
            payload,
            crash_path.display(),
        );
        append_and_flush(session_log, &summary);
        // Issue #803 评论 5905373722：panic 不立刻认定本次启动失败。
        // 写 pending marker，由后续 mark_exit 根据真实退出码决定是否提升为 last_failed.log。
        write_pending_last_failed(session_log);
    }
}

/// 格式化启动头信息。包含时间、PID、版本/build key/package type、环境检测。
///
/// `app_version` / `build_key` / `package_type` 由调用方传入，确保反映真实
/// build identity（见 Issue #803 评论 5905373722），而非本 crate 编译期的
/// option_env! 猜测（本 crate 无 build.rs 注入，option_env! 恒为 None）。
///
/// 严格不写 token、正文、仓库地址。
fn format_startup_header(
    ts_human: &str,
    pid: u32,
    app_version: &str,
    build_key: &str,
    package_type: &str,
) -> String {
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
