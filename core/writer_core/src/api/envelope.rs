use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::api::error::WriterError;

/// Standard cross-platform capability response envelope.
///
/// Platform adapters must branch on `success` and `errorCode`; `rawError` is
/// only for diagnostics and must not drive UI/business behavior.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ResultEnvelope<T>
where
    T: Serialize,
{
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    /// i18n message key，供 UI 层做本地化映射。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_key: Option<String>,
    /// i18n 模板参数，配合 message_key 做插值。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_args: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_error: Option<String>,
    pub warnings: Vec<String>,
    pub changed_paths: Vec<String>,
    pub changed_entities: Vec<ChangedEntityDto>,
}

/// 变更实体 DTO — 标识一次 API 调用导致的实体变更。
/// `entity_type` 为 API 契约字符串（如 "ChapterSaved"、"ProjectCreated"），
/// 平台端据此刷新对应 UI 区域。`entity_id` 为可选的具体实体标识。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChangedEntityDto {
    pub entity_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entity_id: Option<String>,
}

impl<T> ResultEnvelope<T>
where
    T: Serialize,
{
    /// 构造成功信封，无变更路径和实体。
    pub fn success(data: T) -> Self {
        Self {
            success: true,
            data: Some(data),
            error_code: None,
            message_key: None,
            message_args: None,
            raw_error: None,
            warnings: Vec::new(),
            changed_paths: Vec::new(),
            changed_entities: Vec::new(),
        }
    }

    /// 构造成功信封，附带变更路径和实体列表（供平台端增量刷新）。
    pub fn success_with_changes(
        data: T,
        changed_paths: Vec<String>,
        changed_entities: Vec<ChangedEntityDto>,
    ) -> Self {
        Self {
            changed_paths,
            changed_entities,
            ..Self::success(data)
        }
    }

    /// 构造错误信封——从 WriterError 提取 code、message_key、params 和 raw_error。
    pub fn error(error: WriterError) -> Self {
        Self {
            success: false,
            data: None,
            error_code: Some(error.code().to_string()),
            message_key: Some(error.message_key().to_string()),
            message_args: Some(error.params()),
            raw_error: Some(error.to_string()),
            warnings: Vec::new(),
            changed_paths: Vec::new(),
            changed_entities: Vec::new(),
        }
    }

    /// 从 ApiResult 构造信封——Ok 映射为 success，Err 映射为 error。
    pub fn from_api_result(result: Result<T, WriterError>) -> Self {
        match result {
            Ok(data) => Self::success(data),
            Err(error) => Self::error(error),
        }
    }

    /// 将类型化信封转为 `serde_json::Value` 信封（用于 FFI 边界统一序列化）。
    pub fn into_value_envelope(self) -> ResultEnvelope<serde_json::Value> {
        ResultEnvelope {
            success: self.success,
            data: self.data.and_then(|d| serde_json::to_value(d).ok()),
            error_code: self.error_code,
            message_key: self.message_key,
            message_args: self.message_args,
            raw_error: self.raw_error,
            warnings: self.warnings,
            changed_paths: self.changed_paths,
            changed_entities: self.changed_entities,
        }
    }

    /// 序列化为 JSON 字符串。序列化失败时返回标准错误信封（success=false, errorCode=JSON_ERROR）。
    pub fn to_json_string(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|err| {
            serde_json::json!({
                "success": false,
                "errorCode": "JSON_ERROR",
                "messageKey": "error.json",

                "rawError": err.to_string(),
                "warnings": [],
                "changedPaths": [],
                "changedEntities": []
            })
            .to_string()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn success_creates_envelope_with_correct_defaults() {
        let envelope = ResultEnvelope::success("test_data".to_string());

        assert!(envelope.success);
        assert_eq!(envelope.data, Some("test_data".to_string()));
        assert_eq!(envelope.error_code, None);
        assert_eq!(envelope.message_key, None);
        assert_eq!(envelope.message_args, None);
        assert_eq!(envelope.raw_error, None);
        assert!(envelope.warnings.is_empty());
        assert!(envelope.changed_paths.is_empty());
        assert!(envelope.changed_entities.is_empty());
    }

    #[test]
    fn success_envelope_uses_standard_field_names() {
        let json = ResultEnvelope::success_with_changes(
            true,
            vec!["settings.json".to_string()],
            vec![ChangedEntityDto {
                entity_type: "SettingsSaved".to_string(),
                entity_id: None,
            }],
        )
        .to_json_string();

        assert!(json.contains("\"success\":true"));
        assert!(json.contains("\"data\":true"));
        assert!(json.contains("\"changedPaths\":[\"settings.json\"]"));
        assert!(json.contains("\"changedEntities\""));
        assert!(!json.contains("errorCode"));
        assert!(!json.contains("rawError"));
    }

    #[test]
    fn error_envelope_separates_code_message_and_raw_error() {
        let json = ResultEnvelope::<()>::error(WriterError::ProjectNotFound).to_json_string();

        assert!(json.contains("\"success\":false"));
        assert!(json.contains("\"errorCode\":\"PROJECT_NOT_FOUND\""));
        assert!(json.contains("\"messageKey\":\"error.project_not_found\""));
        assert!(json.contains("\"rawError\":"));
        assert!(json.contains("\"warnings\":[]"));
    }

    #[test]
    fn from_api_result_maps_ok_to_success() {
        let result: Result<String, WriterError> = Ok("test_data".to_string());
        let envelope = ResultEnvelope::from_api_result(result);

        assert!(envelope.success);
        assert_eq!(envelope.data, Some("test_data".to_string()));
        assert_eq!(envelope.error_code, None);
        assert_eq!(envelope.message_key, None);
        assert_eq!(envelope.message_args, None);
        assert_eq!(envelope.raw_error, None);
    }

    #[test]
    fn from_api_result_maps_err_to_error() {
        let error = WriterError::ProjectNotFound;
        let expected_error_code = error.code().to_string();
        let expected_raw_error = error.to_string();

        let result: Result<String, WriterError> = Err(error);
        let envelope = ResultEnvelope::from_api_result(result);

        assert!(!envelope.success);
        assert_eq!(envelope.data, None);
        assert_eq!(envelope.error_code, Some(expected_error_code));
        assert_eq!(
            envelope.message_key,
            Some("error.project_not_found".to_string())
        );
        assert!(envelope.message_args.unwrap().is_empty());
        assert_eq!(envelope.raw_error, Some(expected_raw_error));
    }

    #[test]
    fn error_envelope_extracts_fields_correctly() {
        let error = WriterError::ProjectNotFound;
        let envelope = ResultEnvelope::<()>::error(error);

        assert!(!envelope.success);
        assert_eq!(envelope.error_code.as_deref(), Some("PROJECT_NOT_FOUND"));
        assert_eq!(
            envelope.message_key.as_deref(),
            Some("error.project_not_found")
        );
        assert!(envelope.message_args.as_ref().unwrap().is_empty());
        assert_eq!(envelope.raw_error.as_deref(), Some("Project not found"));
        assert!(envelope.warnings.is_empty());
        assert!(envelope.changed_paths.is_empty());
        assert!(envelope.changed_entities.is_empty());
    }

    #[test]
    fn error_envelope_with_params() {
        let error = WriterError::EmptyOverwriteBlocked {
            chapter_id: "ch1".into(),
            old_len: 100,
            new_len: 0,
            reason: "empty content".into(),
        };
        let envelope = ResultEnvelope::<()>::error(error);

        assert_eq!(
            envelope.message_key.as_deref(),
            Some("error.empty_overwrite_blocked")
        );
        let args = envelope.message_args.unwrap();
        assert_eq!(args.get("chapter_id").unwrap(), "ch1");
        assert_eq!(args.get("old_len").unwrap(), "100");
        assert_eq!(args.get("new_len").unwrap(), "0");
        assert_eq!(args.get("reason").unwrap(), "empty content");
    }

    #[test]
    fn error_envelope_serializes_message_key() {
        let error = WriterError::SyncConflict("path conflict".into());
        let json = ResultEnvelope::<()>::error(error).to_json_string();

        assert!(json.contains("\"messageKey\":\"error.sync_conflict\""));
        assert!(json.contains("\"messageArgs\""));
        assert!(json.contains("\"detail\""));
    }
}
