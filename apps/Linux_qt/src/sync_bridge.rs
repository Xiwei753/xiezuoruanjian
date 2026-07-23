// =============================================================================
// sync_bridge.rs — 网络同步与诊断任务桥接层
// =============================================================================
//
// 引用了什么：
// - writer_core::api::types::SyncDiagnosticsResultDto：核心库提供的强类型同步诊断 DTO。
// - writer_core::api::WriterCoreApi：核心库对外的统一 API 入口。
// - writer_core::sync：核心底层的 Git 与 RESTful 同步控制服务（唯一正式同步模块）。
//
// 干什么的：
// - 封装多线程异步同步/诊断任务结果传输结构体（SyncTaskOutcome），提供 operation_id。
// - 负责错误消息脱敏处理（mask_sync_error），剥离 Token 等隐私信息，严守数据防泄露红线。
// - 将底层网络或 Git 抛出的原始错误分类映射为 UI 状态码（sync_error_category），供 StatusPill 等组件渲染。
//
// 被什么引用：
// - 被 apps/Linux_qt/src/backend/sync_backend.rs 引用，用于启动异步同步线程并处理其回调结果。
// - 被 apps/Linux_qt/src/backend/workspace_backend.rs 引用，协助 GitHub 初始化克隆工作区。
// =============================================================================

use writer_core::api::types::SyncDiagnosticsResultDto;
use writer_core::api::WriterCoreApi;
use writer_core::sync::{SyncConfig, SyncSecrets};

/// 同步任务结果封装。
pub struct SyncTaskOutcome {
    pub operation_id: String,
    pub sync_status: String,
    pub action_result: String,
}

/// 对错误消息进行脱敏处理（移除 Token、密钥等敏感信息）。
pub fn mask_sync_error(msg: &str) -> String {
    writer_core::sync::redact_secrets_from_message(msg, None, None)
}

/// 将 core 返回的强类型错误分类映射为 UI 状态码。
pub fn sync_error_category_from_code(category: Option<&str>, fallback_msg: &str) -> String {
    let cat = writer_core::sync::SyncErrorCategory::from_code(
        category.unwrap_or(""),
        fallback_msg,
    );
    cat.to_ui_status().to_string()
}

#[cfg(test)]
mod tests {
    use super::{determine_diagnostics_status, sync_error_category_from_code};
    use writer_core::api::types::SyncDiagnosticsResultDto;

    #[test]
    fn typed_sync_error_category_takes_precedence() {
        assert_eq!(
            sync_error_category_from_code(Some("repo_not_found_or_no_permission"), "unhelpful"),
            "repo_not_found_or_no_permission"
        );
        assert_eq!(
            sync_error_category_from_code(Some("dns_failed"), "unhelpful"),
            "network_failed"
        );
        assert_eq!(
            sync_error_category_from_code(Some("local_blocking_file"), "unhelpful"),
            "conflict"
        );
    }

    #[test]
    fn typed_sync_error_category_returns_other_when_missing() {
        assert_eq!(
            sync_error_category_from_code(None, "repository not found"),
            "error"
        );
        assert_eq!(
            sync_error_category_from_code(Some(""), "timeout while connecting"),
            "error"
        );
    }

    #[test]
    fn test_not_found_maps_to_not_found_category() {
        assert_eq!(
            sync_error_category_from_code(Some("not_found"), "some error occurred"),
            "auth_failed"
        );
    }

    #[test]
    fn test_file_not_found_maps_to_auth_failed() {
        assert_eq!(
            sync_error_category_from_code(Some("file_not_found"), "some error"),
            "auth_failed"
        );
    }

    #[test]
    fn test_repo_not_found_mapped_to_own_category() {
        assert_eq!(
            sync_error_category_from_code(Some("repo_not_found_or_no_permission"), "any message"),
            "repo_not_found_or_no_permission"
        );
    }

    #[test]
    fn test_remote_branch_missing_mapped_to_branch_missing() {
        assert_eq!(
            sync_error_category_from_code(Some("remote_branch_missing"), "any message"),
            "branch_missing"
        );
    }

    #[test]
    fn test_token_invalid_and_permission_denied_mapped_to_own_categories() {
        assert_eq!(
            sync_error_category_from_code(Some("token_invalid"), "any message"),
            "token_invalid"
        );
        assert_eq!(
            sync_error_category_from_code(Some("token_permission_denied"), "any message"),
            "token_permission_denied"
        );
    }

    #[test]
    fn test_auth_error_maps_to_auth_failed() {
        assert_eq!(
            sync_error_category_from_code(Some("auth_error"), "any message"),
            "auth_failed"
        );
    }

    #[test]
    fn test_network_error_maps_to_network_failed() {
        assert_eq!(
            sync_error_category_from_code(Some("network_error"), "any message"),
            "network_failed"
        );
    }

    #[test]
    fn test_404_never_maps_to_generic_error() {
        let categories_404 = [
            "not_found",
            "file_not_found",
            "repo_not_found_or_no_permission",
        ];
        for cat in &categories_404 {
            let result = sync_error_category_from_code(Some(cat), "generic message");
            assert_ne!(
                result, "error",
                "category '{}' should not map to generic 'error'",
                cat
            );
            assert_ne!(
                result, "api_error",
                "category '{}' should not map to 'api_error'",
                cat
            );
        }
    }

    /// Helper: 构造一个 success=true 的 SyncDiagnosticsResultDto，仅关键字段有值。
    fn make_success_dto() -> SyncDiagnosticsResultDto {
        SyncDiagnosticsResultDto {
            success: true,
            backend_type: "github".to_string(),
            has_network_permission: false,
            has_network_state_permission: false,
            network_state: String::new(),
            network_ok: true,
            auth_ok: true,
            repo_ok: true,
            branch_ok: true,
            network_status: String::new(),
            auth_status: String::new(),
            repo_status: String::new(),
            branch_status: String::new(),
            remote_url_sanitized: String::new(),
            transport: String::new(),
            error_category: String::new(),
            raw_error: None,
        }
    }

    #[test]
    fn test_determine_diagnostics_status_auth_categories() {
        let mut result = make_success_dto();
        result.success = false;

        result.error_category = "token_invalid".to_string();
        assert_eq!(determine_diagnostics_status(&result), "token_invalid");

        result.error_category = "token_permission_denied".to_string();
        assert_eq!(determine_diagnostics_status(&result), "token_permission_denied");

        result.error_category = "repo_not_found_or_no_permission".to_string();
        assert_eq!(determine_diagnostics_status(&result), "repo_not_found_or_no_permission");
    }

    #[test]
    fn test_determine_diagnostics_status_branch_missing() {
        let mut result = make_success_dto();
        result.success = false;
        result.error_category = "remote_branch_missing".to_string();
        assert_eq!(determine_diagnostics_status(&result), "branch_missing");
    }

    #[test]
    fn test_determine_diagnostics_status_network_failed() {
        let mut result = make_success_dto();
        result.success = false;

        result.error_category = "dns_failed".to_string();
        assert_eq!(determine_diagnostics_status(&result), "network_failed");

        result.error_category = "tls_failed".to_string();
        assert_eq!(determine_diagnostics_status(&result), "network_failed");
    }

    #[test]
    fn test_determine_diagnostics_status_success() {
        let result = make_success_dto();
        assert_eq!(determine_diagnostics_status(&result), "diagnostics_success");
    }
}

pub fn determine_diagnostics_status(result: &SyncDiagnosticsResultDto) -> String {
    if result.success {
        "diagnostics_success".to_string()
    } else {
        sync_error_category_from_code(
            Some(result.error_category.as_str()),
            result.raw_error.as_deref().unwrap_or(""),
        )
    }
}

pub fn save_sync_configs(
    path: &str,
    config: &SyncConfig,
    secrets: &SyncSecrets,
) -> Result<(), String> {
    let api = WriterCoreApi::new(path);
    let config_result = api.save_sync_config(config.clone().into());
    let config_envelope = match config_result {
        Ok(data) => writer_core::api::ResultEnvelope::success_with_changes(
            data,
            vec!["sync_config.json".to_string()],
            vec![writer_core::api::ChangedEntityDto {
                entity_type: "SyncConfigSaved".to_string(),
                entity_id: None,
            }],
        ),
        Err(error) => writer_core::api::ResultEnvelope::<bool>::error(error),
    };
    if !config_envelope.success {
        let error_code = config_envelope.error_code.as_deref().unwrap_or("UNKNOWN");
        let raw_error = config_envelope.raw_error.as_deref().unwrap_or("error.save_sync_config_failed");
        return Err(format!("{} ({})", raw_error, error_code));
    }

    let secrets_result = api.save_sync_secrets(secrets.clone().into());
    let secrets_envelope = match secrets_result {
        Ok(data) => writer_core::api::ResultEnvelope::success_with_changes(
            data,
            vec!["sync_secrets.local.json".to_string()],
            vec![writer_core::api::ChangedEntityDto {
                entity_type: "SyncConfigSaved".to_string(),
                entity_id: None,
            }],
        ),
        Err(error) => writer_core::api::ResultEnvelope::<bool>::error(error),
    };
    if !secrets_envelope.success {
        let error_code = secrets_envelope.error_code.as_deref().unwrap_or("UNKNOWN");
        let raw_error = secrets_envelope.raw_error.as_deref().unwrap_or("error.save_sync_secrets_failed");
        return Err(format!("{} ({})", raw_error, error_code));
    }

    Ok(())
}