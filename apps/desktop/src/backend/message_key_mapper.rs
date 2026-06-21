// =============================================================================
// message_key_mapper.rs — messageKey → qsTr 映射器
// =============================================================================
//
// 将 Core 返回的 messageKey 映射为 Qt 翻译 key（qsTr 用），供 QML 侧做本地化。
//
// 在 Qt i18n 体系里，qsTr 的 source string 就是中文（zh_CN 是 source language），
// 翻译文件 .ts 里的 <source> 就是中文。其他语言翻译时从 source 翻译出去。
// 所以 mapper 返回中文是正确的。
//
// 被什么引用：
// - project_operations.rs (core_envelope_to_result)
// - settings_backend.rs (save_local_settings)
// - sync_backend.rs (save_sync_config)
// =============================================================================

/// 将 Core 返回的 messageKey 映射为 Qt 翻译 key。
///
/// QML 侧通过 qsTr() 加载 .ts 文件中的翻译。
/// 这个 mapper 返回的 key 必须与 zh_CN.ts 中的 <source> 一致。
pub fn message_key_to_qstr_key(message_key: &str) -> &'static str {
    match message_key {
        "error.io" => "文件读写失败，请检查工作区权限和磁盘状态",
        "error.json" => "数据文件格式异常，请检查工作区文件是否损坏",
        "error.invalid_workspace" => "不是有效的工作区",
        "error.project_not_found" => "作品不存在或已被删除",
        "error.volume_not_found" => "卷不存在或已被删除",
        "error.chapter_not_found" => "章节不存在或已被删除",
        "error.empty_overwrite_blocked" => "已阻止空内容覆盖现有章节",
        "error.not_implemented" => "该功能尚未实现",
        "error.refuse_delete_workspace_root" => "拒绝删除工作区根目录",
        "error.invalid_delete_target" => "删除目标无效",
        "error.sync_conflict" => "同步冲突，请手动处理冲突文件后重试",
        "error.sync_failed" => "同步失败，请检查网络和配置",
        "error.other" => "操作失败",
        "error.core_error" => "核心模块错误",
        "error.clipboard_unavailable" => "复制失败：未找到可用的剪贴板后端",
        "error.json_parse" => "数据解析失败",
        _ => "操作失败",
    }
}