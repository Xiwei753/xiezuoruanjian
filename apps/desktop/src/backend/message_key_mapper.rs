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
        "error.io" | "error.json" | "error.invalid_workspace" | "error.project_not_found"
        | "error.volume_not_found" | "error.chapter_not_found" | "error.empty_overwrite_blocked"
        | "error.not_implemented" | "error.refuse_delete_workspace_root"
        | "error.invalid_delete_target" | "error.sync_conflict" | "error.sync_failed"
        | "error.other" | "error.core_error" | "error.clipboard_unavailable"
        | "error.json_parse" | "error.empty_title" => message_key,
        _ => "error.other",
    }
}
