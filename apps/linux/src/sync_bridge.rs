//! # 同步桥接（Linux UI 层 - Backend Adapter）
//!
//! 同步相关的辅助函数：错误掩码、错误分类、诊断状态判定。
//!
//! ## 架构定位
//!
//! ```text
//! QML SyncPage → sync_bridge::mask_sync_error() / sync_error_category()
//!   → WriterCore / sync_service
//! ```
//!
//! ## 职责边界
//!
//! - **做**：错误消息脱敏（移除 Token）、错误分类（网络/认证/冲突等）、诊断状态判定
//! - **不做**：实际同步操作（由 WriterCore::perform_sync 负责）
//! - **不做**：同步配置管理（由 WriterCore::load_sync_config 负责）

use writer_core::sync_service::{SyncConfig, SyncSecrets, SyncDiagnosticsResult};

/// 同步任务结果封装。
pub struct SyncTaskOutcome {
    pub sync_status: String,
    pub action_result: String,
}

/// 对错误消息进行脱敏处理（移除 Token、密钥等敏感信息）。
pub fn mask_sync_error(msg: &str) -> String {
    writer_core::sync_service::redact_secrets_from_message(msg, None, None)
}

/// 根据错误消息内容分类错误类型（用于 UI 展示不同的错误提示）。
pub fn sync_error_category(msg: &str) -> String {
    let lower = msg.to_lowercase();
    if lower.contains("token") && (lower.contains("missing") || lower.contains("empty") || lower.contains("not provided")) {
        return "configured_untested".to_string();
    }
    if lower.contains("repository not found") || (lower.contains("not found") && lower.contains("repo")) || lower.contains("404") ||
       lower.contains("permission denied") || lower.contains("403") {
        return "auth_failed".to_string();
    }
    if lower.contains("ref not found") || lower.contains("couldn't find remote ref") ||
       lower.contains("remote branch not found") ||
       (lower.contains("branch") && lower.contains("not found")) {
        return "branch_missing".to_string();
    }
    if lower.contains("non-fast-forward") || lower.contains("non fast forward") || lower.contains("nonfastforward") ||
       (lower.contains("fetch first") && lower.contains("push")) {
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
    if lower.contains("authentication") || lower.contains("auth failed") || lower.contains("401") ||
       lower.contains("credentials") || lower.contains("could not authenticate") ||
       lower.contains("bad credentials") {
        return "auth_failed".to_string();
    }
    if lower.contains("resolve") || lower.contains("timeout") || lower.contains("connection refused") ||
       lower.contains("dns") || lower.contains("network") || lower.contains("proxy") ||
       lower.contains("eof") || lower.contains("tls") || lower.contains("ssl") ||
       lower.contains("certificate") || lower.contains("unreachable") ||
       lower.contains("connection reset") || lower.contains("no route to host") {
        return "network_failed".to_string();
    }
    "error".to_string()
}

pub fn determine_diagnostics_status(result: &SyncDiagnosticsResult) -> &'static str {
    if !result.success {
        match result.error_category.as_str() {
            "token_missing" => "configured_untested",
            "empty_url" => "not_configured",
            cat if cat.contains("auth") || cat == "token_missing" => "configured_untested",
            cat if cat.contains("network") || cat.contains("proxy") || cat.contains("connect") => "network_failed",
            "repo_not_found_or_no_permission" => "auth_failed",
            _ => "error",
        }
    } else {
        "configured_untested"
    }
}

pub fn format_diagnostics_message(result: &SyncDiagnosticsResult) -> String {
    let mut msg = format!("诊断结果: {}", if result.success { "成功" } else { "失败" });

    msg.push_str(&format!("\n后端类型: {}", result.backend_type));
    msg.push_str(&format!("\n应用内代理: {}", result.app_proxy_status));

    if !result.remote_url_sanitized.is_empty() {
        msg.push_str(&format!("\nRemote URL: {}", result.remote_url_sanitized));
    }
    msg.push_str(&format!("\nTransport: {}", result.transport));
    msg.push_str(&format!("\n代理配置: {}", result.app_proxy_status));

    msg.push_str(&format!("\n网络连接: {}", if result.network_ok { "正常" } else { "异常" }));
    msg.push_str(&format!("\n身份认证: {}", if result.auth_ok { "正常" } else { "异常" }));
    msg.push_str(&format!("\n仓库访问: {}", if result.repo_ok { "正常" } else { "异常" }));
    msg.push_str(&format!("\n分支存在: {}", if result.branch_ok { "正常" } else { "异常" }));

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

pub fn save_sync_configs(path: &str, config: &SyncConfig, secrets: &SyncSecrets) -> Result<(), String> {
    let core = writer_core::facade::WriterCore::new(path);
    core.save_sync_config(config).map_err(|e| format!("保存同步配置失败: {}", e))?;
    core.save_sync_secrets(secrets).map_err(|e| format!("保存同步凭证失败: {}", e))?;
    Ok(())
}
