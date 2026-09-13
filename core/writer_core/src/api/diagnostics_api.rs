//! 统一诊断 API — UniFFI 暴露的统一诊断接口实现。
//!
//! 这些函数在 `writer_core` namespace 中通过 UDL 暴露给平台绑定层，
//! 内部调用 `writer_diagnostics`。平台端不再自己定义一套日志后端。
//!
//! UDL 中已声明这些函数签名，此处只提供实现，不加 `#[::uniffi::export]` 宏
//! （UDL scaffolding 已生成绑定，加宏会重复定义符号）。

use std::collections::BTreeMap;
use std::path::Path;

use writer_diagnostics::{DiagnosticEvent, DiagnosticLevel, DiagnosticOrigin, PlatformAttachment};

use crate::api::error::WriterError;
use crate::api::types::{
    DiagnosticAttachmentDto, DiagnosticFieldDto, DiagnosticLevelDto, DiagnosticOriginDto,
    DiagnosticsInitDto,
};

/// 初始化诊断后端 — Issue #670 评论 5651816143 修改 1。
///
/// 进程级基础设施，在任何日志产生之前调用。`session_id` 由 Rust 内部用
/// `uuid::Uuid::new_v4()` 生成。`enabled` / `verbose` 默认 true，后续由
/// `set_diagnostics_config` 调整。
///
/// 不要把 diagnostics 生命周期绑在 WriterAppService 上。它应当在任何日志产生之前
/// 初始化。`LocalSettings` 仍是 enabled/verbose 唯一持久事实来源，设置加载后
/// 只调用 `set_diagnostics_config()` 更新运行时状态。
pub fn init_diagnostics(init: DiagnosticsInitDto) -> std::result::Result<(), WriterError> {
    let session_id = uuid::Uuid::new_v4().to_string();
    let config = writer_diagnostics::DiagnosticsConfig {
        log_dir: Path::new(&init.log_dir).to_path_buf(),
        platform_name: init.platform,
        device_id: init.device_id,
        app_version: init.app_version,
        build_key: init.build_key,
        locale: init.locale,
        timezone: init.timezone,
        session_id,
        // enabled / verbose 默认 true；后续由 set_diagnostics_config 调整。
        enabled: true,
        verbose: true,
    };
    writer_diagnostics::init(config);
    Ok(())
}

/// 记录结构化诊断事件 — Issue #670 评论 5651816143 修改 5。
///
/// `level` 区分日志级别（Error/Warn/Info/Debug/Trace），`origin` 区分用户操作 /
/// 系统回调 / 应用自身。`event` 是稳定的事件名，`target` 是模块名，`message` 是
/// 可选的可读消息，`fields` 是动态字段。所有字符串值会被
/// `writer_diagnostics::redact` 脱敏。
pub fn record_diagnostic_event(
    level: DiagnosticLevelDto,
    origin: DiagnosticOriginDto,
    event: String,
    target: String,
    message: Option<String>,
    fields: Vec<DiagnosticFieldDto>,
) -> std::result::Result<(), WriterError> {
    let level: DiagnosticLevel = level.into();
    let origin: DiagnosticOrigin = origin.into();
    let mut field_map = BTreeMap::new();
    for f in fields {
        field_map.insert(f.key, serde_json::Value::String(f.value));
    }
    let diag_event = DiagnosticEvent {
        timestamp_ms: chrono::Utc::now().timestamp_millis(),
        // sequence / session_id 由 writer_diagnostics::record_event 统一补全
        // （Issue #670 评论 5651816143 修改 2）。
        sequence: 0,
        session_id: String::new(),
        level,
        origin,
        event,
        target,
        message,
        fields: field_map,
    };
    writer_diagnostics::record_event(diag_event);
    Ok(())
}

/// 更新诊断配置（运行时切换 enabled / verbose）。
pub fn set_diagnostics_config(
    enabled: bool,
    verbose: bool,
) -> std::result::Result<(), WriterError> {
    writer_diagnostics::set_config(enabled, verbose);
    Ok(())
}

/// flush barrier — 阻塞直到前序日志落盘。
///
/// 返回 `true` 表示落盘成功；writer 死亡超时或写盘失败返回 `false`。
pub fn flush_diagnostics() -> std::result::Result<bool, WriterError> {
    Ok(writer_diagnostics::flush())
}

/// clear barrier — 清空日志文件。
///
/// 返回 `true` 表示删除成功；超时/中断/删除失败返回 `false`。
pub fn clear_diagnostics() -> std::result::Result<bool, WriterError> {
    Ok(writer_diagnostics::clear())
}

/// 导出诊断包到 `output_dir`，返回生成的 zip 文件路径 —
/// Issue #670 评论 5651816143 修改 3。
///
/// 平台特有附件由平台采集器在 Kotlin/Qt 侧收集后通过 `attachments` 传入，
/// 本函数负责复制日志、写附件、生成 manifest、打 zip。附件打包只由 Rust 生成一次，
/// 平台端不再自己 ZipOutputStream 打包。
pub fn export_diagnostics(
    output_dir: String,
    attachments: Vec<DiagnosticAttachmentDto>,
) -> std::result::Result<String, WriterError> {
    let output_path = Path::new(&output_dir);
    let platform_attachments: Vec<PlatformAttachment> =
        attachments.into_iter().map(Into::into).collect();
    let zip_path =
        writer_diagnostics::export(output_path, &platform_attachments).map_err(WriterError::Io)?;
    Ok(zip_path.to_string_lossy().to_string())
}

/// 对文本做脱敏 — Issue #670 评论 5651816143 修改 4。
///
/// 暴露 Rust `writer_diagnostics::redact::redact` 给平台采集阶段先处理文本，
/// 保证脱敏只有一份事实来源（Rust `redact`），不再在 Kotlin 端复制一套规则。
pub fn redact_diagnostic_text(text: String) -> std::result::Result<String, WriterError> {
    Ok(writer_diagnostics::redact::redact(&text))
}
