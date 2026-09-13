//! 统一诊断事件协议。
//!
//! 所有平台（Android / Linux / Harmony）只采集 [`DiagnosticEvent`]，
//! 不再各自定义一套日志后端。事件持久化为 JSONL（一行一个事件），
//! 动态值统一进 `fields`，事件名（`event`）保持稳定。
//!
//! 序列化到 JSONL 时字段名按 Issue #670 评论 5651060802 约定缩短：
//! - `timestamp_ms` → `ts`
//! - `sequence` → `seq`
//! - `session_id` → `session`
//!
//! 其余字段名与 Rust 字段一致（`level` / `origin` / `event` / `target` /
//! `message` / `fields`），便于接收端直接 `jq` 解析。

use std::collections::BTreeMap;

use serde::Serialize;

/// 事件来源分类。`User` 表示用户主动操作，`System` 表示系统回调/环境变化，
/// `App` 表示应用自身生命周期/内部状态（普通 `log::*` 默认归为此类）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticOrigin {
    User,
    System,
    App,
}

impl DiagnosticOrigin {
    /// 从字符串解析来源，未知值回退到 `App`。
    pub fn parse(s: &str) -> Self {
        match s {
            "user" => Self::User,
            "system" => Self::System,
            _ => Self::App,
        }
    }
}

/// 日志级别，与 `log::Level` 一一对应，但独立枚举以便序列化稳定。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum DiagnosticLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl From<log::Level> for DiagnosticLevel {
    fn from(level: log::Level) -> Self {
        match level {
            log::Level::Error => Self::Error,
            log::Level::Warn => Self::Warn,
            log::Level::Info => Self::Info,
            log::Level::Debug => Self::Debug,
            log::Level::Trace => Self::Trace,
        }
    }
}

/// 统一诊断事件。
///
/// 动态值全部进 `fields`，`event` 保持稳定的事件名（如 `theme.appearance_select`）。
/// 序列化时通过 [`DiagnosticEventJson`] 把字段名缩短为 JSONL 约定。
#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticEvent {
    pub timestamp_ms: i64,
    pub sequence: u64,
    pub session_id: String,
    pub level: DiagnosticLevel,
    pub origin: DiagnosticOrigin,
    pub event: String,
    pub target: String,
    pub message: Option<String>,
    pub fields: BTreeMap<String, serde_json::Value>,
}

impl DiagnosticEvent {
    /// 从 `log::Record` 构造事件，来源默认为 `origin`（普通 `log::*` 调用点
    /// 传 `App`）。`sequence` / `session_id` / `timestamp_ms` 由调用方
    /// （`logger::record_event`）填充，本方法只填 0/空串/当前时间。
    pub fn from_log_record(record: &log::Record<'_>, origin: DiagnosticOrigin) -> Self {
        let timestamp_ms = chrono::Utc::now().timestamp_millis();
        let target = record.target().to_string();
        let event = record.target().to_string();
        let message = Some(record.args().to_string());
        let mut fields = BTreeMap::new();
        if let Some(module) = record.module_path() {
            fields.insert(
                "module".to_string(),
                serde_json::Value::String(module.to_string()),
            );
        }
        if let Some(file) = record.file() {
            fields.insert(
                "file".to_string(),
                serde_json::Value::String(file.to_string()),
            );
        }
        if let Some(line) = record.line() {
            fields.insert(
                "line".to_string(),
                serde_json::Value::Number(serde_json::Number::from(line)),
            );
        }
        Self {
            timestamp_ms,
            sequence: 0,
            session_id: String::new(),
            level: record.level().into(),
            origin,
            event,
            target,
            message,
            fields,
        }
    }

    /// 序列化为单行 JSONL（不含末尾换行）。
    ///
    /// 字段名按 Issue #670 评论 5651060802 约定缩短（ts/seq/session）。
    pub fn to_jsonl(&self) -> String {
        let json = DiagnosticEventJson {
            ts: self.timestamp_ms,
            seq: self.sequence,
            level: self.level,
            origin: self.origin,
            event: &self.event,
            target: &self.target,
            session: &self.session_id,
            message: self.message.as_deref(),
            fields: &self.fields,
        };
        serde_json::to_string(&json).unwrap_or_else(|_| "{}".to_string())
    }
}

/// JSONL 持久格式 — 字段名缩短以匹配 Issue 约定。
///
/// 仅用于序列化，不暴露给上层；上层始终使用 [`DiagnosticEvent`]。
#[derive(Serialize)]
struct DiagnosticEventJson<'a> {
    ts: i64,
    seq: u64,
    level: DiagnosticLevel,
    origin: DiagnosticOrigin,
    event: &'a str,
    target: &'a str,
    session: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<&'a str>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    fields: &'a BTreeMap<String, serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origin_parse_roundtrip() {
        assert_eq!(DiagnosticOrigin::parse("user"), DiagnosticOrigin::User);
        assert_eq!(DiagnosticOrigin::parse("system"), DiagnosticOrigin::System);
        assert_eq!(DiagnosticOrigin::parse("app"), DiagnosticOrigin::App);
        assert_eq!(DiagnosticOrigin::parse("unknown"), DiagnosticOrigin::App);
    }

    #[test]
    fn level_from_log_level() {
        assert_eq!(
            DiagnosticLevel::from(log::Level::Error),
            DiagnosticLevel::Error
        );
        assert_eq!(
            DiagnosticLevel::from(log::Level::Info),
            DiagnosticLevel::Info
        );
    }

    #[test]
    fn jsonl_field_names_shortened() {
        let mut fields = BTreeMap::new();
        fields.insert(
            "requested".to_string(),
            serde_json::Value::String("dark".to_string()),
        );
        let event = DiagnosticEvent {
            timestamp_ms: 1_700_000_000_000,
            sequence: 42,
            session_id: "sess-1".to_string(),
            level: DiagnosticLevel::Info,
            origin: DiagnosticOrigin::User,
            event: "theme.appearance_select".to_string(),
            target: "theme".to_string(),
            message: None,
            fields,
        };
        let jsonl = event.to_jsonl();
        assert!(jsonl.contains("\"ts\":1700000000000"), "ts field: {jsonl}");
        assert!(jsonl.contains("\"seq\":42"), "seq field: {jsonl}");
        assert!(
            jsonl.contains("\"session\":\"sess-1\""),
            "session field: {jsonl}"
        );
        assert!(jsonl.contains("\"level\":\"INFO\""), "level field: {jsonl}");
        assert!(
            jsonl.contains("\"origin\":\"user\""),
            "origin field: {jsonl}"
        );
        assert!(
            jsonl.contains("\"event\":\"theme.appearance_select\""),
            "event field: {jsonl}"
        );
        assert!(
            jsonl.contains("\"fields\":{\"requested\":\"dark\"}"),
            "fields: {jsonl}"
        );
        // message 为 None 时应被 skip
        assert!(
            !jsonl.contains("\"message\""),
            "message should be skipped: {jsonl}"
        );
    }

    #[test]
    fn jsonl_includes_message_when_present() {
        let event = DiagnosticEvent {
            timestamp_ms: 0,
            sequence: 0,
            session_id: String::new(),
            level: DiagnosticLevel::Warn,
            origin: DiagnosticOrigin::App,
            event: "test.event".to_string(),
            target: "test".to_string(),
            message: Some("hello".to_string()),
            fields: BTreeMap::new(),
        };
        let jsonl = event.to_jsonl();
        assert!(jsonl.contains("\"message\":\"hello\""), "message: {jsonl}");
        // fields 为空时应被 skip
        assert!(
            !jsonl.contains("\"fields\""),
            "fields should be skipped: {jsonl}"
        );
    }
}
