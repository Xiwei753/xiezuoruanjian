// =============================================================================
// SyncMessageKeys.qml — 同步消息翻译锚点
// =============================================================================
//
// 此文件唯一目的是让 lupdate 提取同步相关的动态 qsTr key。
// 运行时不会实例化此组件。所有 key 必须以静态 qsTr("...") 形式出现，
// 否则 lupdate 无法识别，运行时翻译表为空会直接显示原始 key。
//
// 维护规则：
// - 新增 sync.*/error.* key 时，必须在此文件同步添加对应 qsTr 行。
// - 不要删除已有行，除非对应 key 已从代码中移除。
// =============================================================================

import QtQuick

QtObject {
    id: _syncMessageKeys

    // sync.block.*
    property string _block_no_workspace: qsTr("sync.block.no_workspace")
    property string _block_disabled: qsTr("sync.block.disabled")
    property string _block_remote_url_missing: qsTr("sync.block.remote_url_missing")
    property string _block_token_missing: qsTr("sync.block.token_missing")
    property string _block_invalid_directory: qsTr("sync.block.invalid_directory")

    // sync.phase.*
    property string _phase_diagnose: qsTr("sync.phase.diagnose")
    property string _phase_dry_run: qsTr("sync.phase.dry_run")
    property string _phase_syncing: qsTr("sync.phase.syncing")
    property string _phase_background_syncing: qsTr("sync.phase.background_syncing")
    property string _phase_github_init: qsTr("sync.phase.github_init")

    // sync.result.*
    property string _result_diagnose_success: qsTr("sync.result.diagnose_success")
    property string _result_diagnose_failed: qsTr("sync.result.diagnose_failed")
    property string _result_dry_run_summary: qsTr("sync.result.dry_run_summary")
    property string _result_dry_run_failed: qsTr("sync.result.dry_run_failed")
    property string _result_success_summary: qsTr("sync.result.success_summary")
    property string _result_latest_wins_summary: qsTr("sync.result.latest_wins_summary")
    property string _result_no_changes_summary: qsTr("sync.result.no_changes_summary")
    property string _result_conflict_summary: qsTr("sync.result.conflict_summary")
    property string _result_partial_conflict_summary: qsTr("sync.result.partial_conflict_summary")
    property string _result_dirty_repo_blocked: qsTr("sync.result.dirty_repo_blocked")
    property string _result_branch_recovered_summary: qsTr("sync.result.branch_recovered_summary")
    property string _result_generic_error: qsTr("sync.result.generic_error")
    property string _result_save_config_success: qsTr("sync.result.save_config_success")
    property string _result_save_config_failed: qsTr("sync.result.save_config_failed")
    property string _result_clone_success_init_failed: qsTr("sync.result.clone_success_init_failed")
    property string _result_push_failed_save_config_failed: qsTr("sync.result.push_failed_save_config_failed")
    property string _result_push_failed: qsTr("sync.result.push_failed")
    property string _result_clone_init_success: qsTr("sync.result.clone_init_success")
    property string _result_clone_failed: qsTr("sync.result.clone_failed")
    property string _result_remote_configured_sync_success: qsTr("sync.result.remote_configured_sync_success")
    property string _result_no_conflict_files: qsTr("sync.result.no_conflict_files")
    property string _result_more_files_count: qsTr("sync.result.more_files_count")
    property string _result_git_repo_not_workspace: qsTr("sync.result.git_repo_not_workspace")
    property string _result_directory_not_empty_not_workspace: qsTr("sync.result.directory_not_empty_not_workspace")
    property string _result_configured_not_tested: qsTr("sync.result.configured_not_tested")

    // sync.status.*
    property string _status_already_running: qsTr("sync.status.already_running")

    // error.*
    property string _error_io: qsTr("error.io")
    property string _error_json: qsTr("error.json")
    property string _error_project_not_found: qsTr("error.project_not_found")
    property string _error_volume_not_found: qsTr("error.volume_not_found")
    property string _error_chapter_not_found: qsTr("error.chapter_not_found")
    property string _error_empty_overwrite_blocked: qsTr("error.empty_overwrite_blocked")
    property string _error_not_implemented: qsTr("error.not_implemented")
    property string _error_refuse_delete_root: qsTr("error.refuse_delete_root")
    property string _error_invalid_delete_target: qsTr("error.invalid_delete_target")
    property string _error_sync_conflict: qsTr("error.sync_conflict")
    property string _error_sync_failed: qsTr("error.sync_failed")
    property string _error_other: qsTr("error.other")
    property string _error_core_error: qsTr("error.core_error")
    property string _error_clipboard_unavailable: qsTr("error.clipboard_unavailable")
    property string _error_json_parse: qsTr("error.json_parse")
    property string _error_empty_title: qsTr("error.empty_title")
    property string _error_sync_diagnose_panic: qsTr("error.sync_diagnose_panic")
    property string _error_sync_dry_run_panic: qsTr("error.sync_dry_run_panic")
    property string _error_sync_panic: qsTr("error.sync_panic")
    property string _error_load_sync_config_failed: qsTr("error.load_sync_config_failed")
    property string _error_core_not_initialized: qsTr("error.core_not_initialized")
    property string _error_parse_json_failed: qsTr("error.parse_json_failed")
    property string _error_save_sync_config_failed: qsTr("error.save_sync_config_failed")
    property string _error_save_sync_secrets_failed: qsTr("error.save_sync_secrets_failed")

    // chapter.*
    property string _chapter_deleted_remotely_refreshed: qsTr("chapter.deleted_remotely_refreshed")
}
