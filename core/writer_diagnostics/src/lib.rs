//! # writer_diagnostics — 统一诊断后端
//!
//! 一套 Rust 日志后端，多端只采集事件。本 crate 接管现有 Rust `log::*`，
//! 通过 UniFFI 暴露统一诊断接口供 Android / Linux / Harmony 调用。
//!
//! ## 架构
//!
//! ```text
//! 平台端 → writer_diagnostics::init(PlatformInit) → log::set_logger(SharedLogger)
//!        → writer 线程独占文件 I/O（JSONL 持久化）
//!        → export_diagnostics() 生成 zip 包
//! ```
//!
//! ## 模块
//!
//! - [`event`]：统一定义事件协议（`DiagnosticEvent` / `DiagnosticOrigin` / `DiagnosticLevel`）
//! - [`redact`]：合并 Android/Linux 现有脱敏规则
//! - [`writer`]：后台写入队列、顺序、轮转、flush/clear barrier
//! - [`logger`]：`log::Log` 实现，把 `log::*` 接入统一后端
//! - [`export`]：导出日志清单、manifest、zip 包
//!
//! ## 不依赖平台 crate
//!
//! 只依赖 `log` / `serde` / `serde_json` / `regex` / `chrono` / `uuid` / `std`，
//! 不依赖 Android / Qt / Harmony 任何平台类型。

// 测试代码允许 unwrap/expect：与 writer_core 做法一致，只对 test cfg 生效，
// 不在生产路径掩盖 warning。
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::too_many_lines,
        clippy::cognitive_complexity
    )
)]

pub mod event;
pub mod export;
pub mod logger;
pub mod redact;
pub mod writer;

/// 测试共享锁 — 所有触碰全局 writer 单例（`writer::init`/`enqueue`/`flush`/…）的
/// 测试都必须串行，否则并发改写全局 config 会让事件写到错误目录，进而污染断言。
/// 统一用这一把锁，并在上锁后容忍已被其他测试 panic 毒化的锁。
#[cfg(test)]
pub(crate) static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub use event::{DiagnosticEvent, DiagnosticLevel, DiagnosticOrigin};
pub use export::{export_diagnostics, PlatformAttachment};

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

/// panic hook 是否已安装 — 保证只在首次 init 时安装一次（Issue #670 评论 5651816143 修改 6）。
static PANIC_HOOK_INSTALLED: AtomicBool = AtomicBool::new(false);

/// 诊断配置 — 由平台 init 传入，初始化日志后端。
#[derive(Debug, Clone)]
pub struct DiagnosticsConfig {
    /// 日志目录（来自 `PlatformInit.log_dir`）。
    pub log_dir: PathBuf,
    /// 平台名（`android` / `desktop` / `harmony` 等）。
    pub platform_name: String,
    /// 设备 ID。
    pub device_id: String,
    /// 应用版本。
    pub app_version: String,
    /// 构建 key（用于日志文件名 `sujian-current-{build_key}.log`）。
    pub build_key: String,
    /// locale（如 `zh_CN`）。
    pub locale: String,
    /// timezone（如 `Asia/Shanghai`）。
    pub timezone: String,
    /// 会话 ID（每次启动唯一）。
    pub session_id: String,
    /// 是否启用日志。
    pub enabled: bool,
    /// 是否输出 debug/trace 级别。
    pub verbose: bool,
}

/// 全局导出元数据 — init 时保存，export 时读取，避免调用方传 "unknown"。
struct ExportMetadata {
    platform_name: String,
    build_key: String,
}

static EXPORT_META: OnceLock<ExportMetadata> = OnceLock::new();

/// 初始化日志后端 — 安装 `log::Log`，启动 writer 线程。幂等。
///
/// 平台初始化完成后把完整 `PlatformInit` 转成 [`DiagnosticsConfig`] 交给本函数；
/// 日志目录、平台名、设备 ID、应用版本、locale/timezone 都从这一个入口进来。
pub fn init(config: DiagnosticsConfig) {
    let _ = EXPORT_META.set(ExportMetadata {
        platform_name: config.platform_name.clone(),
        build_key: config.build_key.clone(),
    });
    logger::install(
        config.log_dir,
        config.build_key,
        config.session_id,
        config.enabled,
        config.verbose,
    );
    // 安装 panic hook — Issue #670 评论 5651816143 修改 6。
    // 把 Rust panic 转成结构化诊断事件（origin=app, event=app.panic, level=error），
    // 带 thread/location/脱敏后的 panic message，然后 flush 落盘，再链回原 hook。
    // 幂等：只在首次安装时设置，避免多次 init 重复链式安装。
    install_panic_hook();
    // 记录启动事件。
    let mut fields = std::collections::BTreeMap::new();
    fields.insert(
        "platform".to_string(),
        serde_json::Value::String(config.platform_name),
    );
    fields.insert(
        "device_id".to_string(),
        serde_json::Value::String(config.device_id),
    );
    fields.insert(
        "app_version".to_string(),
        serde_json::Value::String(config.app_version),
    );
    fields.insert(
        "locale".to_string(),
        serde_json::Value::String(config.locale),
    );
    fields.insert(
        "timezone".to_string(),
        serde_json::Value::String(config.timezone),
    );
    let event = DiagnosticEvent {
        timestamp_ms: chrono::Utc::now().timestamp_millis(),
        // sequence / session_id 由 record_event 统一补全（Issue #670 评论 5651816143 修改 2）。
        sequence: 0,
        session_id: String::new(),
        level: DiagnosticLevel::Info,
        origin: DiagnosticOrigin::App,
        event: "diagnostics.init".to_string(),
        target: "writer_diagnostics".to_string(),
        message: None,
        fields,
    };
    record_event(event);
}

/// 安装共享 panic hook — 把 Rust panic 转成结构化诊断事件并落盘。
///
/// Issue #670 评论 5651816143 修改 6：`diagnostics.rs` 注释说 "panic hook 由
/// writer_diagnostics logger 接管"，但本 crate 之前没有 `std::panic::set_hook()`。
/// 此处补上：记录 `origin=app, event=app.panic, level=error` 事件，带 thread/location/
/// 脱敏后的 panic message，然后 flush 落盘，再链回原 hook。
///
/// 幂等：用 `PANIC_HOOK_INSTALLED` AtomicBool 保护，只在首次调用时安装。
/// 多次 init 不会重复链式安装，避免 hook 链无限增长。
fn install_panic_hook() {
    if PANIC_HOOK_INSTALLED.swap(true, Ordering::SeqCst) {
        // 已安装：不重复安装，避免 hook 链无限增长。
        return;
    }
    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // 把 panic 信息转成结构化诊断事件。
        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_default();
        let thread = std::thread::current()
            .name()
            .unwrap_or("<unnamed>")
            .to_string();
        let msg = format!("{}", info);
        let mut fields = std::collections::BTreeMap::new();
        fields.insert("thread".to_string(), serde_json::Value::String(thread));
        fields.insert("location".to_string(), serde_json::Value::String(location));
        let event = DiagnosticEvent {
            timestamp_ms: chrono::Utc::now().timestamp_millis(),
            // sequence / session_id 由 record_event 统一补全。
            sequence: 0,
            session_id: String::new(),
            level: DiagnosticLevel::Error,
            origin: DiagnosticOrigin::App,
            event: "app.panic".to_string(),
            target: "writer_diagnostics".to_string(),
            message: Some(msg),
            fields,
        };
        // 记录事件并立即 flush 落盘，确保 panic 日志不丢。
        record_event(event);
        flush();
        // 链回原 hook（默认打印到 stderr），保持原有行为。
        prev_hook(info);
    }));
}

/// 记录结构化事件 — 脱敏后序列化为 JSONL 并入队 writer。
///
/// 普通 Rust `log::*` 默认 `origin=app`；需要还原用户操作/系统回调的事件，
/// 走本函数，由调用点传 `User/System/App`。
pub fn record_event(event: DiagnosticEvent) {
    logger::record_event(event);
}

/// 更新配置（运行时切换 enabled / verbose）。
pub fn set_config(enabled: bool, verbose: bool) {
    logger::set_config(enabled, verbose);
}

/// flush barrier — 阻塞直到前序日志落盘。
///
/// 返回 `true` 表示落盘成功；writer 死亡超时或写盘失败返回 `false`。
pub fn flush() -> bool {
    logger::flush()
}

/// clear barrier — 清空日志文件。
///
/// 返回 `true` 表示删除成功；超时/中断/删除失败返回 `false`。
pub fn clear() -> bool {
    logger::clear()
}

/// 导出诊断包到 `output_dir`，返回生成的 zip 文件路径。
///
/// `platform_name` 和 `build_key` 从 init 时保存的配置中读取；
/// 平台特有附件由平台采集器交进来（`attachments`），本函数不自己再
/// 定义一套诊断包格式。
pub fn export(
    output_dir: &std::path::Path,
    attachments: &[PlatformAttachment],
) -> Result<PathBuf, String> {
    let (platform_name, build_key) = EXPORT_META
        .get()
        .map(|m| (m.platform_name.as_str(), m.build_key.as_str()))
        .unwrap_or(("unknown", "unknown"));
    export_diagnostics(output_dir, platform_name, build_key, attachments)
}
