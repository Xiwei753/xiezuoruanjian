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
// - 封装 target 级 progress 回调通道（SyncTargetProgressOutcome），让后台同步线程
//   每跑完一个 target 就能把冲突状态回主线程，不必等最终 FullSyncResult（Issue #762）。
// - 负责错误消息脱敏处理（mask_sync_error），剥离 Token 等隐私信息，严守数据防泄露红线。
// - 将底层网络或 Git 抛出的原始错误分类映射为 UI 状态码（sync_error_category），供 StatusPill 等组件渲染。
//
// 被什么引用：
// - 被 apps/Linux_qt/src/backend/sync_backend.rs 引用，用于启动异步同步线程并处理其回调结果。
// - 被 apps/Linux_qt/src/backend/workspace_backend.rs 引用，协助 GitHub 初始化克隆工作区。
// =============================================================================

use writer_core::api::types::SyncDiagnosticsResultDto;
use writer_core::sync::full_sync::{SyncProgressCallback, SyncTargetProgress};
use writer_core::sync::{SyncConfig, SyncSecrets};

/// Issue #762 评论 5826175490 第 5 点：单个 target 完成后的进度载荷（平台侧 DTO）。
///
/// 后台同步线程每跑完一个 target 就把 `project_id/status/conflict_count`
/// 通过 [`make_target_progress_callback`] 投递回主线程。主线程据此刷新全局冲突状态，
/// 不必等最终 `FullSyncResult` 落地——"同步中"和"等待用户解决冲突"可以同时成立。
///
/// `project_id` 为 `None` 表示 App target（非作品）。
/// `status` 是线格式状态码（`"success"` / `"partial_conflict"` / `"error"` 等）。
pub struct SyncTargetProgressOutcome {
    pub project_id: Option<String>,
    pub target_kind: String,
    pub status: String,
    pub conflict_count: u32,
}

impl From<SyncTargetProgress> for SyncTargetProgressOutcome {
    fn from(p: SyncTargetProgress) -> Self {
        Self {
            project_id: p.project_id,
            target_kind: p.target_kind,
            status: p.status,
            conflict_count: p.conflict_count,
        }
    }
}

/// 构造 Core → 平台主线程的 target progress 回调通道。
///
/// Core 的 [`SyncProgressCallback`] 是 `Arc<dyn Fn(SyncTargetProgress) + Send + Sync>`，
/// 可 move 进后台同步线程。`dispatch` 由调用方提供"回到主线程做什么"——
/// 桌面端用 `qmetaobject::queued_callback` 包一个 `QPointer<SyncBackend>`；
/// 本函数只负责 Core 载荷 → [`SyncTargetProgressOutcome`] 的转换，
/// 不引入 Qt 类型，便于单测。
pub fn make_target_progress_callback<F>(dispatch: F) -> SyncProgressCallback
where
    F: Fn(SyncTargetProgressOutcome) + Send + Sync + 'static,
{
    std::sync::Arc::new(move |progress: SyncTargetProgress| dispatch(progress.into()))
}

/// 同步任务结果封装。
pub struct SyncTaskOutcome {
    pub operation_id: String,
    pub sync_status: String,
    pub action_result: String,
    /// Issue #729：启动此同步线程时捕获的 workspace generation。
    /// 回调进入 `handle_sync_outcome` 时与 `AppBackend.current_workspace_generation` 比对，
    /// 不匹配说明工作区已切换，结果必须丢弃，避免旧同步污染新工作区状态。
    pub workspace_generation: u64,
    /// Issue #729 评论 5763441474：启动此同步线程时捕获的 data_root。
    /// 回调进入 `handle_sync_outcome` 时与 `AppBackend.current_data_root` 比对，
    /// 不匹配说明工作区已切换，结果必须丢弃。
    /// 与 `operation_id` + `workspace_generation` 三者同时匹配才接受结果。
    pub data_root: String,
}

/// 对错误消息进行脱敏处理（移除 Token、密钥等敏感信息）。
pub fn mask_sync_error(msg: &str) -> String {
    writer_core::sync::redact_secrets_from_message(msg, None, None)
}

/// 将 core 返回的强类型错误分类映射为 UI 状态码。
///
/// 先尝试 `legacy_category_compat` 处理旧 GitHub/Git 特定 code，
/// 再回退到 `from_code` 处理 provider-neutral code。
pub fn sync_error_category_from_code(category: Option<&str>, fallback_msg: &str) -> String {
    let code = category.unwrap_or("");
    let cat = writer_core::sync::legacy_category_compat(code)
        .unwrap_or_else(|| writer_core::sync::SyncErrorCategory::from_code(code, fallback_msg));
    cat.to_ui_status().to_string()
}

#[cfg(test)]
mod tests {
    use super::{
        determine_diagnostics_status, make_target_progress_callback, sync_error_category_from_code,
        SyncTargetProgressOutcome,
    };
    use writer_core::api::types::SyncDiagnosticsResultDto;
    use writer_core::sync::full_sync::SyncTargetProgress;

    /// Issue #762 评论 5826175490 第 5 点：progress 通道必须把每个 target 的
    /// project_id/status/conflict_count 原样投递给平台回调，平台才能在任何一轮全量同步
    /// 结束前刷新该作品的持久冲突状态。
    #[test]
    fn progress_callback_forwards_target_payload() {
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = seen.clone();
        let callback = make_target_progress_callback(move |outcome: SyncTargetProgressOutcome| {
            if let Ok(mut guard) = sink.lock() {
                guard.push((
                    outcome.project_id,
                    outcome.target_kind,
                    outcome.status,
                    outcome.conflict_count,
                ));
            }
        });

        callback(SyncTargetProgress {
            project_id: Some("p1".to_string()),
            target_kind: "project".to_string(),
            status: "partial_conflict".to_string(),
            conflict_count: 2,
        });
        callback(SyncTargetProgress {
            project_id: None,
            target_kind: "app".to_string(),
            status: "success".to_string(),
            conflict_count: 0,
        });

        let guard = seen.lock().expect("progress sink lock");
        assert_eq!(guard.len(), 2);
        assert_eq!(
            guard[0],
            (
                Some("p1".to_string()),
                "project".to_string(),
                "partial_conflict".to_string(),
                2
            )
        );
        assert_eq!(
            guard[1],
            (None, "app".to_string(), "success".to_string(), 0)
        );
    }

    /// progress 回调必须满足 `Send + Sync`，才能 move 进后台同步线程。
    #[test]
    fn progress_callback_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>(_: &T) {}
        let callback = make_target_progress_callback(|_| {});
        assert_send_sync(&callback);
    }

    #[test]
    fn typed_sync_error_category_takes_precedence() {
        assert_eq!(
            sync_error_category_from_code(Some("repo_not_found_or_no_permission"), "unhelpful"),
            "token_permission_denied"
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
            "not_found"
        );
    }

    #[test]
    fn test_file_not_found_maps_to_not_found() {
        assert_eq!(
            sync_error_category_from_code(Some("file_not_found"), "some error"),
            "not_found"
        );
    }

    #[test]
    fn test_repo_not_found_mapped_to_permission_denied() {
        assert_eq!(
            sync_error_category_from_code(Some("repo_not_found_or_no_permission"), "any message"),
            "token_permission_denied"
        );
    }

    #[test]
    fn test_remote_branch_missing_mapped_to_error() {
        assert_eq!(
            sync_error_category_from_code(Some("remote_branch_missing"), "any message"),
            "error"
        );
    }

    #[test]
    fn test_token_invalid_and_permission_denied_mapped_to_own_categories() {
        assert_eq!(
            sync_error_category_from_code(Some("token_invalid"), "any message"),
            "auth_failed"
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
            provider_type: "github_api".to_string(),
            has_network_permission: false,
            has_network_state_permission: false,
            network_state: String::new(),
            network_ok: true,
            auth_ok: true,
            remote_ok: true,
            network_status: String::new(),
            auth_status: String::new(),
            error_category: String::new(),
            raw_error: None,
            provider_details: None,
        }
    }

    #[test]
    fn test_determine_diagnostics_status_auth_categories() {
        let mut result = make_success_dto();
        result.success = false;

        result.error_category = "token_invalid".to_string();
        assert_eq!(determine_diagnostics_status(&result), "auth_failed");

        result.error_category = "token_permission_denied".to_string();
        assert_eq!(
            determine_diagnostics_status(&result),
            "token_permission_denied"
        );

        result.error_category = "repo_not_found_or_no_permission".to_string();
        assert_eq!(
            determine_diagnostics_status(&result),
            "token_permission_denied"
        );
    }

    #[test]
    fn test_determine_diagnostics_status_branch_missing() {
        let mut result = make_success_dto();
        result.success = false;
        result.error_category = "remote_branch_missing".to_string();
        assert_eq!(determine_diagnostics_status(&result), "error");
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
    // path 是作品目录绝对路径；projects_root 为其父目录。
    // 这样作品目录 = projects_root/path 的 file_name = path。
    let path_obj = std::path::Path::new(path);
    let projects_root = path_obj
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| ".".to_string());
    let api = crate::backend::app_backend::create_core_api(path, &projects_root)
        .map_err(|e| format!("workspace bootstrap 失败: {e}"))?;
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
        let raw_error = config_envelope
            .raw_error
            .as_deref()
            .unwrap_or("error.save_sync_config_failed");
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
        let raw_error = secrets_envelope
            .raw_error
            .as_deref()
            .unwrap_or("error.save_sync_secrets_failed");
        return Err(format!("{} ({})", raw_error, error_code));
    }

    Ok(())
}
