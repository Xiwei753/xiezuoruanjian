//! `log::Log` 实现 — 把 Rust `log::*` 接入统一诊断后端。
//!
//! 普通 `log::debug!/info!/warn!` 默认 `origin=app`；需要还原用户操作/系统回调
//! 的事件走显式结构化 [`record_event`]，由调用点传 `User/System/App`。
//!
//! [`init`] 安装 logger 到 `log::set_logger`，幂等。

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::OnceLock;

use super::event::{DiagnosticEvent, DiagnosticOrigin};
use super::writer;

/// 全局 session_id，由 init 设置。
static SESSION_ID: OnceLock<String> = OnceLock::new();

/// 全局事件序号计数器。
static SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// logger 是否已安装。
static LOGGER_INSTALLED: AtomicBool = AtomicBool::new(false);

/// `log::Log` 实现 — 把 `log::Record` 转成 `DiagnosticEvent` 并入队。
struct SharedLogger;

impl log::Log for SharedLogger {
    fn log(&self, record: &log::Record<'_>) {
        if !LOGGER_INSTALLED.load(Ordering::SeqCst) {
            return;
        }
        let mut event = DiagnosticEvent::from_log_record(record, DiagnosticOrigin::App);
        event.sequence = SEQUENCE.fetch_add(1, Ordering::SeqCst) + 1;
        if let Some(sid) = SESSION_ID.get() {
            event.session_id = sid.clone();
        }
        record_event(event);
    }

    fn enabled(&self, _metadata: &log::Metadata<'_>) -> bool {
        true
    }

    fn flush(&self) {
        writer::flush();
    }
}

static SHARED_LOGGER: SharedLogger = SharedLogger;

/// 记录结构化事件 — 脱敏后序列化为 JSONL 并入队 writer。
///
/// 调用点负责传 `origin`：用户操作 → `User`，系统回调 → `System`，应用自身 → `App`。
pub fn record_event(event: DiagnosticEvent) {
    // 脱敏 message 和 fields 中的字符串值。
    let event = redact_event(event);
    let json = event.to_jsonl();
    writer::enqueue(json);
}

/// 对事件的 message 和 fields 字符串值做脱敏。
fn redact_event(mut event: DiagnosticEvent) -> DiagnosticEvent {
    if let Some(msg) = event.message.take() {
        event.message = Some(super::redact::redact(&msg));
    }
    let mut redacted_fields = std::collections::BTreeMap::new();
    for (k, v) in event.fields.iter() {
        let v = match v {
            serde_json::Value::String(s) => {
                serde_json::Value::String(super::redact::redact(s))
            }
            other => other.clone(),
        };
        redacted_fields.insert(k.clone(), v);
    }
    event.fields = redacted_fields;
    event
}

/// 安装 logger 到 `log::set_logger`，启动 writer 线程。幂等。
///
/// `log_dir` 是日志目录，`build_key` 用于日志文件名，`session_id` 是会话 ID，
/// `enabled` 是否启用，`verbose` 控制是否输出 debug/trace 级别。
pub(crate) fn install(
    log_dir: std::path::PathBuf,
    build_key: String,
    session_id: String,
    enabled: bool,
    verbose: bool,
) {
    let _ = SESSION_ID.set(session_id);
    writer::init(log_dir, build_key, enabled);
    if LOGGER_INSTALLED.swap(true, Ordering::SeqCst) {
        // 已安装：只更新 enabled。
        writer::set_enabled(enabled);
        return;
    }
    let _ = log::set_logger(&SHARED_LOGGER);
    let max_level = if verbose {
        log::LevelFilter::Trace
    } else if enabled {
        log::LevelFilter::Info
    } else {
        log::LevelFilter::Off
    };
    log::set_max_level(max_level);
    writer::set_enabled(enabled);
}

/// 更新 enabled / verbose 配置（运行时切换）。
pub(crate) fn set_config(enabled: bool, verbose: bool) {
    writer::set_enabled(enabled);
    let max_level = if verbose {
        log::LevelFilter::Trace
    } else if enabled {
        log::LevelFilter::Info
    } else {
        log::LevelFilter::Off
    };
    log::set_max_level(max_level);
}

/// flush barrier — 阻塞直到前序日志落盘。
pub(crate) fn flush() -> bool {
    writer::flush()
}

/// clear barrier — 清空日志文件。
pub(crate) fn clear() -> bool {
    writer::clear()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU64;
    use std::sync::Mutex;

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    static TEST_SEQ: AtomicU64 = AtomicU64::new(0);

    fn make_test_event(event: &str, origin: DiagnosticOrigin) -> DiagnosticEvent {
        DiagnosticEvent {
            timestamp_ms: 0,
            sequence: TEST_SEQ.fetch_add(1, Ordering::SeqCst) + 1,
            session_id: "test-session".to_string(),
            level: super::super::event::DiagnosticLevel::Info,
            origin,
            event: event.to_string(),
            target: "test".to_string(),
            message: None,
            fields: std::collections::BTreeMap::new(),
        }
    }

    #[test]
    fn record_event_redacts_sensitive_message() {
        let _lock = TEST_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        install(
            tmp.path().to_path_buf(),
            "test-redact".to_string(),
            "sess".to_string(),
            true,
            false,
        );
        writer::set_enabled(true);
        let mut event = make_test_event("test.sensitive", DiagnosticOrigin::App);
        event.message = Some("token=my-secret".to_string());
        record_event(event);
        assert!(flush());
        let files = super::writer::log_files();
        assert!(!files.is_empty());
        let content = std::fs::read_to_string(&files[0]).unwrap();
        assert!(content.contains("[REDACTED]"), "content: {content}");
        assert!(!content.contains("my-secret"));
        super::writer::reset_for_test();
    }
}
