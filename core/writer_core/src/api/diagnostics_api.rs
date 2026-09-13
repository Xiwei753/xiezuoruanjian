//! 统一诊断 API — UniFFI 暴露的统一诊断接口实现。
//!
//! 这些函数在 `writer_core` namespace 中通过 UDL 暴露给平台绑定层，
//! 内部调用 `writer_diagnostics`。平台端不再自己定义一套日志后端。
//!
//! UDL 中已声明这些函数签名，此处只提供实现，不加 `#[::uniffi::export]` 宏
//! （UDL scaffolding 已生成绑定，加宏会重复定义符号）。

use std::collections::BTreeMap;
use std::path::Path;

use writer_diagnostics::{
    DiagnosticEvent, DiagnosticLevel, DiagnosticOrigin, PlatformAttachment,
};

use crate::api::error::WriterError;
use crate::api::types::{DiagnosticFieldDto, DiagnosticOriginDto};

/// 记录结构化诊断事件。
///
/// `origin` 区分用户操作 / 系统回调 / 应用自身。`event` 是稳定的事件名，
/// `target` 是模块名，`message` 是可选的可读消息，`fields` 是动态字段。
/// 所有字符串值会被 `writer_diagnostics::redact` 脱敏。
pub fn record_diagnostic_event(
    origin: DiagnosticOriginDto,
    event: String,
    target: String,
    message: Option<String>,
    fields: Vec<DiagnosticFieldDto>,
) -> std::result::Result<(), WriterError> {
    let origin: DiagnosticOrigin = origin.into();
    let mut field_map = BTreeMap::new();
    for f in fields {
        field_map.insert(f.key, serde_json::Value::String(f.value));
    }
    let diag_event = DiagnosticEvent {
        timestamp_ms: chrono::Utc::now().timestamp_millis(),
        sequence: 0,
        session_id: String::new(),
        level: DiagnosticLevel::Info,
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
pub fn set_diagnostics_config(enabled: bool, verbose: bool) -> std::result::Result<(), WriterError> {
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

/// 导出诊断包到 `output_dir`，返回生成的 zip 文件路径。
///
/// 平台特有附件由平台采集器在 Kotlin/Qt 侧收集后通过文件路径传入，
/// 本函数只负责复制日志、生成 manifest、打 zip。
pub fn export_diagnostics(output_dir: String) -> std::result::Result<String, WriterError> {
    let output_path = Path::new(&output_dir);
    // 平台附件由平台端自行收集并放入 output_dir，本函数不接收附件。
    let attachments: Vec<PlatformAttachment> = Vec::new();
    let zip_path = writer_diagnostics::export(output_path, "unknown", "unknown", &attachments)
        .map_err(WriterError::Io)?;
    Ok(zip_path.to_string_lossy().to_string())
}
