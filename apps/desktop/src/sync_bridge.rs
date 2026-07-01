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
// - 封装多线程异步同步/诊断任务结果传输结构体（SyncTaskOutcome），提供 operation_id 和 operation_kind。
// - 负责错误消息脱敏处理（mask_sync_error），剥离 Token 等隐私信息，严守数据防泄露红线。
// - 将底层网络或 Git 抛出的原始错误分类映射为 UI 状态码（sync_error_category），供 StatusPill 等组件渲染。
//
// 被什么引用：
// - 被 apps/desktop/src/backend/sync_backend.rs 引用，用于启动异步同步线程并处理其回调结果。
// - 被 apps/desktop/src/backend/workspace_backend.rs 引用，协助 GitHub 初始化克隆工作区。
// =============================================================================

use writer_core::api::types::SyncDiagnosticsResultDto;
use writer_core::api::WriterCoreApi;
use writer_core::sync::{SyncConfig, SyncSecrets};

/// 同步任务结果封装。
pub struct SyncTaskOutcome {
    pub operation_id: String,
    pub operation_kind: String,
    pub sync_status: String,
    pub action_result: String,
}

/// 对错误消息进行脱敏处理（移除 Token、密钥等敏感信息）。
pub fn mask_sync_error(msg: &str) -> String {
    writer_core::sync::redact_secrets_from_message(msg, None, None)
}

/// 将 core 返回的强类型错误分类映射为 UI 状态码。
pub fn sync_error_category_from_code(category: Option<&str>, fallback_msg: &str) -> String {
    match category.unwrap_or("") {
        "none" | "" => sync_error_category(fallback_msg),
        "token_missing" => "token_missing".to_string(),
        "token_invalid" => "token_invalid".to_string(),
        "token_permission_denied" => "token_permission_denied".to_string(),
        "repo_not_found_or_no_permission" => "repo_not_found_or_no_permission".to_string(),
        "github_unauthorized" | "github_forbidden" => "auth_failed".to_string(),
        "empty_url" => "not_configured".to_string(),
        "missing_permission" => "permission_missing".to_string(),
        "network_probe_failed" | "github_network_failed" | "dns_failed" | "tls_failed" => {
            "network_failed".to_string()
        }
        "branch_missing" | "remote_branch_missing" => "branch_missing".to_string(),
        "not_found" | "file_not_found" => {
            // 404 含义需结合 fallback 消息判断：可能是仓库/权限问题，也可能是分支不存在
            let lower = fallback_msg.to_lowercase();
            if lower.contains("branch") || lower.contains("ref") {
                "branch_missing".to_string()
            } else {
                "auth_failed".to_string()
            }
        }
        "non_fast_forward" => "non_fast_forward".to_string(),
        "conflict" | "checkout_conflict" | "local_blocking_file" => "conflict".to_string(),
        "unrelated_histories" => "unrelated_histories".to_string(),
        _ => "error".to_string(),
    }
}

/// 根据遗留错误消息内容分类错误类型。仅作为 core 未提供 error_category 时的 fallback。
pub fn sync_error_category(msg: &str) -> String {
    let lower = msg.to_lowercase();
    if lower.contains("token")
        && (lower.contains("missing") || lower.contains("empty") || lower.contains("not provided"))
    {
        return "token_missing".to_string();
    }
    if lower.contains("repository not found")
        || (lower.contains("not found") && lower.contains("repo"))
        || lower.contains("404")
        || lower.contains("permission denied")
        || lower.contains("403")
    {
        return "auth_failed".to_string();
    }
    if lower.contains("ref not found")
        || lower.contains("couldn't find remote ref")
        || lower.contains("remote branch not found")
        || (lower.contains("branch") && lower.contains("not found"))
    {
        return "branch_missing".to_string();
    }
    if lower.contains("non-fast-forward")
        || lower.contains("non fast forward")
        || lower.contains("nonfastforward")
        || (lower.contains("fetch first") && lower.contains("push"))
    {
        return "non_fast_forward".to_string();
    }
    if lower.contains("checkout_conflict") || lower.contains("local_blocking_file") {
        return "conflict".to_string();
    }
    if lower.contains("conflict") || lower.contains("merge conflict") {
        return "conflict".to_string();
    }
    if lower.contains("unrelated") {
        return "unrelated_histories".to_string();
    }
    if lower.contains("authentication")
        || lower.contains("auth failed")
        || lower.contains("401")
        || lower.contains("credentials")
        || lower.contains("could not authenticate")
        || lower.contains("bad credentials")
    {
        return "auth_failed".to_string();
    }
    if lower.contains("resolve")
        || lower.contains("timeout")
        || lower.contains("connection refused")
        || lower.contains("dns")
        || lower.contains("network")
        || lower.contains("proxy")
        || lower.contains("eof")
        || lower.contains("tls")
        || lower.contains("ssl")
        || lower.contains("certificate")
        || lower.contains("unreachable")
        || lower.contains("connection reset")
        || lower.contains("no route to host")
    {
        return "network_failed".to_string();
    }
    "error".to_string()
}

#[cfg(test)]
mod tests {
    use super::{determine_diagnostics_status, sync_error_category, sync_error_category_from_code};
    use writer_core::api::types::SyncDiagnosticsResultDto;

    #[test]
    fn test_sync_error_category_fallback_parsing() {
        // token_missing
        assert_eq!(
            sync_error_category("token is missing"),
            "token_missing"
        );
        assert_eq!(sync_error_category("TOKEN is EMPTY"), "token_missing");
        assert_eq!(
            sync_error_category("token not provided here"),
            "token_missing"
        );

        // auth_failed (first block)
        assert_eq!(
            sync_error_category("repository not found on github"),
            "auth_failed"
        );
        assert_eq!(sync_error_category("error: not found repo"), "auth_failed");
        assert_eq!(sync_error_category("status 404"), "auth_failed");
        assert_eq!(
            sync_error_category("Permission denied (publickey)"),
            "auth_failed"
        );
        assert_eq!(sync_error_category("HTTP 403 Forbidden"), "auth_failed");

        // branch_missing
        assert_eq!(
            sync_error_category("fatal: ref not found"),
            "branch_missing"
        );
        assert_eq!(
            sync_error_category("couldn't find remote ref main"),
            "branch_missing"
        );
        assert_eq!(
            sync_error_category("remote branch not found"),
            "branch_missing"
        );
        assert_eq!(
            sync_error_category("branch main not found"),
            "branch_missing"
        );

        // non_fast_forward
        assert_eq!(sync_error_category("hint: Updates were rejected because the tip of your current branch is behind (non-fast-forward)"), "non_fast_forward");
        assert_eq!(
            sync_error_category("non fast forward error"),
            "non_fast_forward"
        );
        assert_eq!(sync_error_category("nonfastforward"), "non_fast_forward");
        assert_eq!(
            sync_error_category("fetch first before push"),
            "non_fast_forward"
        );

        // conflict
        assert_eq!(sync_error_category("checkout_conflict"), "conflict");
        assert_eq!(sync_error_category("local_blocking_file"), "conflict");
        assert_eq!(sync_error_category("merge conflict"), "conflict");
        assert_eq!(sync_error_category("we have a conflict here"), "conflict");

        // unrelated_histories
        assert_eq!(
            sync_error_category("fatal: refusing to merge unrelated histories"),
            "unrelated_histories"
        );

        // auth_failed (second block)
        assert_eq!(sync_error_category("authentication failed"), "auth_failed");
        assert_eq!(sync_error_category("auth failed"), "auth_failed");
        assert_eq!(sync_error_category("status 401"), "auth_failed");
        assert_eq!(sync_error_category("invalid credentials"), "auth_failed");
        assert_eq!(sync_error_category("could not authenticate"), "auth_failed");
        assert_eq!(sync_error_category("bad credentials"), "auth_failed");

        // network_failed
        assert_eq!(
            sync_error_category("could not resolve host"),
            "network_failed"
        );
        assert_eq!(sync_error_category("connection timeout"), "network_failed");
        assert_eq!(sync_error_category("Connection refused"), "network_failed");
        assert_eq!(sync_error_category("dns error"), "network_failed");
        assert_eq!(
            sync_error_category("network is unreachable"),
            "network_failed"
        );
        assert_eq!(sync_error_category("proxy error"), "network_failed");
        assert_eq!(sync_error_category("unexpected eof"), "network_failed");
        assert_eq!(
            sync_error_category("tls handshake failed"),
            "network_failed"
        );
        assert_eq!(
            sync_error_category("ssl certificate problem"),
            "network_failed"
        );
        assert_eq!(sync_error_category("invalid certificate"), "network_failed");
        assert_eq!(sync_error_category("host is unreachable"), "network_failed");
        assert_eq!(
            sync_error_category("connection reset by peer"),
            "network_failed"
        );
        assert_eq!(sync_error_category("no route to host"), "network_failed");

        // error (default)
        assert_eq!(sync_error_category("some unknown strange issue"), "error");
    }

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
    fn typed_sync_error_category_falls_back_when_missing() {
        assert_eq!(
            sync_error_category_from_code(None, "repository not found"),
            "auth_failed"
        );
        assert_eq!(
            sync_error_category_from_code(Some(""), "timeout while connecting"),
            "network_failed"
        );
    }

    #[test]
    fn test_not_found_404_mapped_to_auth_failed_or_branch_missing() {
        // not_found without branch/ref context → auth_failed
        assert_eq!(
            sync_error_category_from_code(Some("not_found"), "some error occurred"),
            "auth_failed"
        );
        // not_found with branch context → branch_missing
        assert_eq!(
            sync_error_category_from_code(Some("not_found"), "branch main not found"),
            "branch_missing"
        );
        // not_found with ref context → branch_missing
        assert_eq!(
            sync_error_category_from_code(Some("not_found"), "ref not found on remote"),
            "branch_missing"
        );
    }

    #[test]
    fn test_file_not_found_mapped_to_auth_failed_or_branch_missing() {
        // file_not_found without branch/ref context → auth_failed
        assert_eq!(
            sync_error_category_from_code(Some("file_not_found"), "some error"),
            "auth_failed"
        );
        // file_not_found with branch context → branch_missing
        assert_eq!(
            sync_error_category_from_code(Some("file_not_found"), "branch does not exist"),
            "branch_missing"
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
    fn test_404_never_maps_to_generic_error() {
        // Ensure 404-related categories never fall through to "error"
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
            android_has_internet_permission: false,
            android_has_access_network_state_permission: false,
            android_network_state: String::new(),
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
            chosen_network_mode: None,
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

pub fn format_diagnostics_message(result: &SyncDiagnosticsResultDto) -> String {
    let mut msg = format!("诊断结果: {}", if result.success { "成功" } else { "失败" });

    msg.push_str(&format!("\n后端类型: {}", result.backend_type));

    if !result.remote_url_sanitized.is_empty() {
        msg.push_str(&format!("\nRemote URL: {}", result.remote_url_sanitized));
    }
    msg.push_str(&format!("\nTransport: {}", result.transport));
    if let Some(mode) = result.chosen_network_mode.as_ref() {
        msg.push_str(&format!("\n网络模式: {}", mode));
    }

    msg.push_str(&format!(
        "\n网络连接: {}",
        if result.network_ok {
            "正常"
        } else {
            "异常"
        }
    ));
    msg.push_str(&format!(
        "\n身份认证: {}",
        if result.auth_ok { "正常" } else { "异常" }
    ));
    msg.push_str(&format!(
        "\n仓库访问: {}",
        if result.repo_ok { "正常" } else { "异常" }
    ));
    msg.push_str(&format!(
        "\n分支存在: {}",
        if result.branch_ok { "正常" } else { "异常" }
    ));

    if !result.error_category.is_empty() && result.error_category != "none" {
        msg.push_str(&format!("\n错误分类: {}", result.error_category));
    }

    if let Some(err) = &result.raw_error {
        msg.push_str(&format!("\n\n错误详情:\n{}", mask_sync_error(err)));
    }

    msg
}

pub fn save_sync_configs(
    path: &str,
    config: &SyncConfig,
    secrets: &SyncSecrets,
) -> Result<(), String> {
    let api = WriterCoreApi::new(path);
    let config_result = api.save_sync_config(config.clone().into());
    let config_json = match config_result {
        Ok(data) => writer_core::api::ResultEnvelope::success_with_changes(
            data,
            vec!["sync_config.json".to_string()],
            vec![writer_core::api::ChangedEntityDto {
                entity_type: "SyncConfigSaved".to_string(),
                entity_id: None,
            }],
        ),
        Err(error) => writer_core::api::ResultEnvelope::<bool>::error(error),
    }
    .to_json_string();
    let config_envelope: serde_json::Value =
        serde_json::from_str(&config_json).map_err(|e| format!("解析配置保存结果失败: {}", e))?;
    if config_envelope["success"] != true {
        let error_code = config_envelope["errorCode"].as_str().unwrap_or("UNKNOWN");
        let raw_error = config_envelope["rawError"]
            .as_str()
            .unwrap_or("保存同步配置失败");
        return Err(format!("{} ({})", raw_error, error_code));
    }

    let secrets_result = api.save_sync_secrets(secrets.clone().into());
    let secrets_json = match secrets_result {
        Ok(data) => writer_core::api::ResultEnvelope::success_with_changes(
            data,
            vec!["sync_secrets.local.json".to_string()],
            vec![writer_core::api::ChangedEntityDto {
                entity_type: "SyncConfigSaved".to_string(),
                entity_id: None,
            }],
        ),
        Err(error) => writer_core::api::ResultEnvelope::<bool>::error(error),
    }
    .to_json_string();
    let secrets_envelope: serde_json::Value =
        serde_json::from_str(&secrets_json).map_err(|e| format!("解析凭证保存结果失败: {}", e))?;
    if secrets_envelope["success"] != true {
        let error_code = secrets_envelope["errorCode"].as_str().unwrap_or("UNKNOWN");
        let raw_error = secrets_envelope["rawError"]
            .as_str()
            .unwrap_or("保存同步凭证失败");
        return Err(format!("{} ({})", raw_error, error_code));
    }

    Ok(())
}
