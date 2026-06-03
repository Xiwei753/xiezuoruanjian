// =============================================================================
// sync_bridge.rs — 网络同步与诊断任务桥接层
// =============================================================================
//
// 引用了什么：
// - writer_core::api::types::SyncDiagnosticsResultDto：核心库提供的强类型同步诊断 DTO。
// - writer_core::api::WriterCoreApi：核心库对外的统一 API 入口。
// - writer_core::sync_service：核心底层的 Git 与 RESTful 同步控制服务。
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
use writer_core::sync_service::{SyncConfig, SyncSecrets};

/// 同步任务结果封装。
pub struct SyncTaskOutcome {
    pub operation_id: String,
    pub operation_kind: String,
    pub sync_status: String,
    pub action_result: String,
}

/// 对错误消息进行脱敏处理（移除 Token、密钥等敏感信息）。
pub fn mask_sync_error(msg: &str) -> String {
    writer_core::sync_service::redact_secrets_from_message(msg, None, None)
}

/// 将 core 返回的强类型错误分类映射为 UI 状态码。
pub fn sync_error_category_from_code(category: Option<&str>, fallback_msg: &str) -> String {
    match category.unwrap_or("") {
        "none" | "" => sync_error_category(fallback_msg),
        "token_missing" => "configured_untested".to_string(),
        "empty_url" => "not_configured".to_string(),
        "missing_permission" => "permission_missing".to_string(),
        "repo_not_found_or_no_permission" | "github_unauthorized" | "github_forbidden" => {
            "auth_failed".to_string()
        }
        "network_probe_failed" | "github_network_failed" | "dns_failed" | "tls_failed" => {
            "network_failed".to_string()
        }
        "branch_missing" | "remote_branch_missing" => "branch_missing".to_string(),
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
        return "configured_untested".to_string();
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
    use super::sync_error_category_from_code;

    #[test]
    fn typed_sync_error_category_takes_precedence() {
        assert_eq!(
            sync_error_category_from_code(Some("repo_not_found_or_no_permission"), "unhelpful"),
            "auth_failed"
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
}

pub fn determine_diagnostics_status(result: &SyncDiagnosticsResultDto) -> &'static str {
    if !result.success {
        match result.error_category.as_str() {
            "token_missing" => "configured_untested",
            "empty_url" => "not_configured",
            cat if cat.contains("auth") || cat == "token_missing" => "configured_untested",
            cat if cat.contains("network") || cat.contains("proxy") || cat.contains("connect") => {
                "network_failed"
            }
            "repo_not_found_or_no_permission" => "auth_failed",
            _ => "error",
        }
    } else {
        "configured_untested"
    }
}

pub fn format_diagnostics_message(result: &SyncDiagnosticsResultDto) -> String {
    let mut msg = format!("诊断结果: {}", if result.success { "成功" } else { "失败" });

    msg.push_str(&format!("\n后端类型: {}", result.backend_type));
    msg.push_str(&format!("\nTCP 探测: {}", result.tcp_probe_status));

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

    if !result.user_message.is_empty() {
        msg.push_str(&format!("\n\n说明:\n{}", result.user_message));
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
    let config_json = api.save_sync_config_envelope_json(config.clone().into());
    let config_envelope: serde_json::Value =
        serde_json::from_str(&config_json).map_err(|e| format!("解析配置保存结果失败: {}", e))?;
    if config_envelope["success"] != true {
        let error_code = config_envelope["errorCode"]
            .as_str()
            .unwrap_or("UNKNOWN");
        let user_message = config_envelope["userMessage"]
            .as_str()
            .unwrap_or("保存同步配置失败");
        return Err(format!("{} ({})", user_message, error_code));
    }

    let secrets_json = api.save_sync_secrets_envelope_json(secrets.clone().into());
    let secrets_envelope: serde_json::Value =
        serde_json::from_str(&secrets_json).map_err(|e| format!("解析凭证保存结果失败: {}", e))?;
    if secrets_envelope["success"] != true {
        let error_code = secrets_envelope["errorCode"]
            .as_str()
            .unwrap_or("UNKNOWN");
        let user_message = secrets_envelope["userMessage"]
            .as_str()
            .unwrap_or("保存同步凭证失败");
        return Err(format!("{} ({})", user_message, error_code));
    }

    Ok(())
}
