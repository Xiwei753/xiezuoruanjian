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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_error: Option<String>,
    pub warnings: Vec<String>,
    pub changed_paths: Vec<String>,
    pub changed_entities: Vec<ChangedEntityDto>,
}

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
    pub fn success(data: T) -> Self {
        Self {
            success: true,
            data: Some(data),
            error_code: None,
            user_message: None,
            raw_error: None,
            warnings: Vec::new(),
            changed_paths: Vec::new(),
            changed_entities: Vec::new(),
        }
    }

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

    pub fn error(error: WriterError) -> Self {
        Self {
            success: false,
            data: None,
            error_code: Some(error.code().to_string()),
            user_message: Some(error.user_message().to_string()),
            raw_error: Some(error.to_string()),
            warnings: Vec::new(),
            changed_paths: Vec::new(),
            changed_entities: Vec::new(),
        }
    }

    pub fn from_api_result(result: Result<T, WriterError>) -> Self {
        match result {
            Ok(data) => Self::success(data),
            Err(error) => Self::error(error),
        }
    }

    pub fn to_json_string(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|err| {
            serde_json::json!({
                "success": false,
                "errorCode": "JSON_ERROR",
                "userMessage": "结果序列化失败",
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
        assert!(json.contains("\"userMessage\":"));
        assert!(json.contains("\"rawError\":"));
        assert!(json.contains("\"warnings\":[]"));
    }
}
