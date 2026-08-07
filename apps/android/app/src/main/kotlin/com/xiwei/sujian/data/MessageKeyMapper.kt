package com.xiwei.sujian.data

import android.content.Context
import com.xiwei.sujian.R

/**
 * 将 Core 返回的 messageKey + messageArgs 映射到 Android string resource。
 *
 * 这是 messageKey 合同在 Android 端的唯一映射点。
 * 所有错误展示都应通过此 mapper 获取用户可见文案。
 */
object MessageKeyMapper {
    fun resolveMessage(
        context: Context,
        messageKey: String?,
        messageArgs: Map<String, String>?,
        errorCode: String?,
    ): String {
        if (messageKey.isNullOrBlank()) {
            return fallbackMessage(context, errorCode)
        }
        return when (messageKey) {
            "error.io" -> context.getString(R.string.error_io)
            "error.json" -> context.getString(R.string.error_json)
            "error.invalid_workspace" -> context.getString(R.string.error_invalid_workspace)
            "error.project_not_found" -> context.getString(R.string.error_project_not_found)
            "error.volume_not_found" -> context.getString(R.string.error_volume_not_found)
            "error.chapter_not_found" -> context.getString(R.string.error_chapter_not_found)
            "error.empty_overwrite_blocked" -> context.getString(R.string.error_empty_overwrite_blocked)
            "error.not_implemented" -> context.getString(R.string.error_not_implemented)
            "error.refuse_delete_workspace_root" -> context.getString(R.string.error_refuse_delete_workspace_root)
            "error.invalid_delete_target" -> context.getString(R.string.error_invalid_delete_target)
            "error.sync_conflict" -> context.getString(R.string.error_sync_conflict)
            "error.sync_failed" -> context.getString(R.string.error_sync_failed)
            "error.sync_auth_failed" -> context.getString(R.string.error_sync_auth_failed)
            "error.sync_network_unavailable" -> context.getString(R.string.error_sync_network_unavailable)
            "error.sync_rate_limited" -> context.getString(R.string.error_sync_rate_limited)
            "error.sync_document_conflict" -> context.getString(R.string.error_sync_document_conflict)
            "error.sync_incomplete_transaction" -> context.getString(R.string.error_sync_incomplete_transaction)
            "error.sync_checkout_conflict" -> context.getString(R.string.error_sync_checkout_conflict)
            "error.sync_settings_conflict" -> context.getString(R.string.error_sync_settings_conflict)
            "error.sync_conflict_detected" -> context.getString(R.string.error_sync_conflict_detected)
            "error.sync_non_fast_forward" -> context.getString(R.string.error_sync_non_fast_forward)
            "error.sync_unrelated_histories" -> context.getString(R.string.error_sync_unrelated_histories)
            "error.sync_remote_branch_not_found" -> context.getString(R.string.error_sync_remote_branch_not_found)
            "error.sync_github_api_error" -> context.getString(R.string.error_sync_github_api_error)
            "error.disk_full" -> context.getString(R.string.error_disk_full)
            "error.storage_transaction_incomplete" -> context.getString(R.string.error_storage_transaction_incomplete)
            "error.save_queue_flush_incomplete" -> context.getString(R.string.error_save_queue_flush_incomplete)
            "error.other" -> context.getString(R.string.error_other)
            "error.native_error" -> context.getString(R.string.error_internal)
            "error.parse_error" -> context.getString(R.string.error_json)
            "error.not_implemented_bridge" -> context.getString(R.string.error_not_implemented)
            "error.flush" -> context.getString(R.string.error_flush)
            "error.close" -> context.getString(R.string.error_close)
            "error.flush_all" -> context.getString(R.string.error_flush_all)
            "error.conversion" -> context.getString(R.string.error_conversion)
            "error.snapshot_cache_not_initialized" -> context.getString(R.string.error_snapshot_cache_not_initialized)
            "error.star_map_cache_missing" -> context.getString(R.string.error_star_map_cache_missing)
            "error.native_not_loaded" -> context.getString(R.string.error_native_not_loaded)
            "error.unknown" -> context.getString(R.string.error_unknown)
            else -> fallbackMessage(context, errorCode)
        }
    }

    private fun fallbackMessage(
        context: Context,
        errorCode: String?,
    ): String {
        return if (!errorCode.isNullOrBlank()) {
            context.getString(R.string.error_internal)
        } else {
            context.getString(R.string.error_other)
        }
    }
}
