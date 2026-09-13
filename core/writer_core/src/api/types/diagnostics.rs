//! 诊断接口 DTO — 与 UDL 中定义的 `DiagnosticOriginDto` / `DiagnosticFieldDto` 对应。
//!
//! `DiagnosticOriginDto` 由 Rust/UniFFI 定义并生成 Kotlin 绑定，
//! 不再在 Kotlin 自己定义一份 enum（Issue #670 评论 5651060802）。

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
