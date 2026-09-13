//! 诊断接口 DTO — 与 UDL 中定义的 `DiagnosticOriginDto` / `DiagnosticFieldDto` /
//! `DiagnosticsInitDto` / `DiagnosticAttachmentDto` / `DiagnosticLevelDto` 对应。
//!
//! `DiagnosticOriginDto` / `DiagnosticLevelDto` 由 Rust/UniFFI 定义并生成 Kotlin 绑定，
//! 不再在 Kotlin 自己定义一份 enum（Issue #670 评论 5651060802 / 5651816143）。

/// 事件来源分类。与 `writer_diagnostics::DiagnosticOrigin` 一一对应。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
pub enum DiagnosticOriginDto {
    User,
    System,
    #[default]
    App,
}

impl From<DiagnosticOriginDto> for writer_diagnostics::DiagnosticOrigin {
    fn from(dto: DiagnosticOriginDto) -> Self {
        match dto {
            DiagnosticOriginDto::User => Self::User,
            DiagnosticOriginDto::System => Self::System,
            DiagnosticOriginDto::App => Self::App,
        }
    }
}

/// 诊断事件动态字段 — 透传给 `writer_diagnostics::DiagnosticEvent.fields`。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DiagnosticFieldDto {
    pub key: String,
    pub value: String,
}

/// 诊断初始化 DTO — Issue #670 评论 5651816143 修改 1。
///
/// 进程级基础设施初始化参数。`session_id` 由 Rust 内部用 `uuid::Uuid::new_v4()`
/// 生成，不暴露给平台端。`enabled` / `verbose` 默认 true，后续由
/// `set_diagnostics_config` 调整。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsInitDto {
    pub log_dir: String,
    pub platform: String,
    pub device_id: String,
    pub app_version: String,
    pub build_key: String,
    pub locale: String,
    pub timezone: String,
}

/// 诊断附件 DTO — Issue #670 评论 5651816143 修改 3。
///
/// 平台采集器交进来的附件（logcat、processExits、threadDump 等）。
/// `relative_path` 是相对输出目录的路径（如 `"logcat.txt"`），`content` 是附件内容字节。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticAttachmentDto {
    pub relative_path: String,
    pub content: Vec<u8>,
}

impl From<DiagnosticAttachmentDto> for writer_diagnostics::PlatformAttachment {
    fn from(dto: DiagnosticAttachmentDto) -> Self {
        Self {
            relative_path: dto.relative_path,
            content: dto.content,
        }
    }
}

/// 诊断日志级别 DTO — Issue #670 评论 5651816143 修改 5。
///
/// 与 `writer_diagnostics::DiagnosticLevel` 一一对应。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
pub enum DiagnosticLevelDto {
    Error,
    Warn,
    #[default]
    Info,
    Debug,
    Trace,
}

impl From<DiagnosticLevelDto> for writer_diagnostics::DiagnosticLevel {
    fn from(dto: DiagnosticLevelDto) -> Self {
        match dto {
            DiagnosticLevelDto::Error => Self::Error,
            DiagnosticLevelDto::Warn => Self::Warn,
            DiagnosticLevelDto::Info => Self::Info,
            DiagnosticLevelDto::Debug => Self::Debug,
            DiagnosticLevelDto::Trace => Self::Trace,
        }
    }
}
