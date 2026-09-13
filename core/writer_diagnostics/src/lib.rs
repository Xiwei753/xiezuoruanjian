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

pub use event::{DiagnosticEvent, DiagnosticLevel, DiagnosticOrigin};
pub use export::{export_diagnostics, PlatformAttachment};

use std::path::PathBuf;

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

/// 初始化日志后端 — 安装 `log::Log`，启动 writer 线程。幂等。
///
/// 平台初始化完成后把完整 `PlatformInit` 转成 [`DiagnosticsConfig`] 交给本函数；
/// 日志目录、平台名、设备 ID、应用版本、locale/timezone 都从这一个入口进来。
pub fn init(config: DiagnosticsConfig) {
    logger::install(
        config.log_dir,
        config.build_key,
        config.session_id,
        config.enabled,
        config.verbose,
    );
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
/// 平台特有附件由平台采集器交进来（`attachments`），本函数不自己再
/// 定义一套诊断包格式。
pub fn export(
    output_dir: &std::path::Path,
    platform_name: &str,
    build_key: &str,
    attachments: &[PlatformAttachment],
) -> Result<PathBuf, String> {
    export_diagnostics(output_dir, platform_name, build_key, attachments)
}
