// =============================================================================
// message_key_mapper.rs — messageKey 验证与规范化
// =============================================================================
//
// 将 Core 返回的 messageKey 验证并规范化。
// 返回有效的 messageKey 字符串，供 QML 侧 MessageKeyHelper 做最终 qsTr 翻译。
// 如果 messageKey 不在已知列表中，返回 "error.other"。
//
// 被什么引用：
// - project_operations.rs (core_envelope_to_result)
// - settings_backend.rs (save_local_settings)
// - sync_backend.rs (save_sync_config)
// =============================================================================

/// 将 Core 返回的 messageKey 验证并规范化。
///
/// 返回有效的 messageKey 字符串，供 QML 侧 MessageKeyHelper 做最终 qsTr 翻译。
/// 如果 messageKey 不在已知列表中，返回 "error.other"。
pub fn resolve_message_key(message_key: &str) -> &'static str {
    match message_key {
        "error.io" => "error.io",
        "error.json" => "error.json",
        "error.invalid_workspace" => "error.invalid_workspace",
        "error.project_not_found" => "error.project_not_found",
        "error.volume_not_found" => "error.volume_not_found",
        "error.chapter_not_found" => "error.chapter_not_found",
        "error.empty_overwrite_blocked" => "error.empty_overwrite_blocked",
        "error.not_implemented" => "error.not_implemented",
        "error.refuse_delete_workspace_root" => "error.refuse_delete_workspace_root",
        "error.invalid_delete_target" => "error.invalid_delete_target",
        "error.sync_conflict" => "error.sync_conflict",
        "error.sync_failed" => "error.sync_failed",
        "error.other" => "error.other",
        "error.core_error" => "error.core_error",
        "error.clipboard_unavailable" => "error.clipboard_unavailable",
        "error.json_parse" => "error.json_parse",
        "error.empty_title" => "error.empty_title",
        "sync.block.no_workspace" => "sync.block.no_workspace",
        "sync.block.disabled" => "sync.block.disabled",
        "sync.block.remote_url_missing" => "sync.block.remote_url_missing",
        "sync.block.token_missing" => "sync.block.token_missing",
        "sync.phase.diagnose" => "sync.phase.diagnose",
        "sync.phase.dry_run" => "sync.phase.dry_run",
        "sync.phase.syncing" => "sync.phase.syncing",
        "sync.phase.background_syncing" => "sync.phase.background_syncing",
        "sync.result.diagnose_success" => "sync.result.diagnose_success",
        "sync.result.diagnose_failed" => "sync.result.diagnose_failed",
        "sync.result.dry_run_summary" => "sync.result.dry_run_summary",
        "sync.result.dry_run_failed" => "sync.result.dry_run_failed",
        "sync.result.success_summary" => "sync.result.success_summary",
        "sync.result.latest_wins_summary" => "sync.result.latest_wins_summary",
        "sync.result.no_changes_summary" => "sync.result.no_changes_summary",
        "sync.result.conflict_summary" => "sync.result.conflict_summary",
        "sync.result.partial_conflict_summary" => "sync.result.partial_conflict_summary",
        "sync.result.dirty_repo_blocked" => "sync.result.dirty_repo_blocked",
        "sync.result.branch_recovered_summary" => "sync.result.branch_recovered_summary",
        "sync.result.generic_error" => "sync.result.generic_error",
        "sync.result.save_config_success" => "sync.result.save_config_success",
        "sync.status.already_running" => "sync.status.already_running",
        "chapter.deleted_remotely_refreshed" => "chapter.deleted_remotely_refreshed",
        "error.sync_diagnose_panic" => "error.sync_diagnose_panic",
        "error.sync_dry_run_panic" => "error.sync_dry_run_panic",
        "error.sync_panic" => "error.sync_panic",
        "error.load_sync_config_failed" => "error.load_sync_config_failed",
        "error.core_not_initialized" => "error.core_not_initialized",
        "error.parse_json_failed" => "error.parse_json_failed",
        "error.save_sync_config_failed" => "error.save_sync_config_failed",
        "error.save_sync_secrets_failed" => "error.save_sync_secrets_failed",
        "sync.block.invalid_directory" => "sync.block.invalid_directory",
        "sync.phase.github_init" => "sync.phase.github_init",
        "sync.result.clone_success_init_failed" => "sync.result.clone_success_init_failed",
        "sync.result.push_failed_save_config_failed" => {
            "sync.result.push_failed_save_config_failed"
        }
        "sync.result.push_failed" => "sync.result.push_failed",
        "sync.result.clone_init_success" => "sync.result.clone_init_success",
        "sync.result.save_config_failed" => "sync.result.save_config_failed",
        "sync.result.clone_failed" => "sync.result.clone_failed",
        "sync.result.remote_configured_sync_success" => {
            "sync.result.remote_configured_sync_success"
        }
        "sync.result.no_conflict_files" => "sync.result.no_conflict_files",
        "sync.result.more_files_count" => "sync.result.more_files_count",
        "sync.result.git_repo_not_workspace" => "sync.result.git_repo_not_workspace",
        "sync.result.directory_not_empty_not_workspace" => {
            "sync.result.directory_not_empty_not_workspace"
        }
        "sync.result.configured_not_tested" => "sync.result.configured_not_tested",
        _ => "error.other",
    }
}
