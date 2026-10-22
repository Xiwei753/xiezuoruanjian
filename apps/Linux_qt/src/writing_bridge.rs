// =============================================================================
// writing_bridge.rs — 写作与编辑器数据桥接层
// =============================================================================
//
// 引用了什么：
// - writer_core::api::error::WriterError：核心统一业务错误。
// - writer_core::api::types::ChapterSaveReceiptDto：章节保存结果回执 DTO。
// - writer_core::api::WriterCoreApi：核心库主业务 API。
// - writer_core::api::EditorChangeStatsInputDto：编辑事务统计入参 DTO。
// - writer_core::editor::EditorTransactionCause：编辑事务原因枚举。
//
// 干什么的：
// - 负责编辑器界面底层与核心写作 API 的桥接。
// - 提供打开章节、缓存并返回 LinuxChapterOpenData 的接口。
// - 封装章节内容安全保存语义（支持allow_empty_overwrite校验）、清空正文内容（clear_chapter_content）的核心实现。
// - 将编辑事务的 cause 和 inserted/deleted 字数透传给 Core 统计入口（record_editor_change_stats）。
//
// 被什么引用：
// - 被 apps/Linux_qt/src/backend/editor_backend.rs 引用，作为主写作编辑器的后端状态控制器与统计源。
// =============================================================================

//! # 写作桥接函数（Linux_qt UI 层 - Backend Adapter）
//!
//! 将 WriterCoreApi 的写作 API 包装为兼容 DTO，供 AppBackend 转为 QML 对象。

use serde::Serialize;
use writer_core::api::error::WriterError;
use writer_core::api::WriterCoreApi;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinuxChapterOpenData {
    pub content: String,
    pub title: String,
    pub project_id: String,
    pub volume_id: String,
    pub chapter_id: String,
    pub meta: writer_core::api::types::ChapterMetaDto,
}

pub fn open_chapter(
    api: &WriterCoreApi,
    project_id: &str,
    volume_id: &str,
    chapter_id: &str,
) -> Result<LinuxChapterOpenData, writer_core::api::error::WriterError> {
    let chapters = api.list_chapters(project_id, volume_id)?;
    let chapter_meta = chapters.into_iter().find(|ch| ch.id == chapter_id);

    if let Some(meta) = chapter_meta {
        let content = api.open_chapter(project_id, volume_id, chapter_id)?;
        Ok(LinuxChapterOpenData {
            content: content.content,
            title: meta.title.clone(),
            project_id: project_id.to_string(),
            volume_id: volume_id.to_string(),
            chapter_id: chapter_id.to_string(),
            meta,
        })
    } else {
        Err(writer_core::api::error::WriterError::ChapterNotFound)
    }
}

/// 按编辑事务上报写作统计。
///
/// 平台端只透传编辑事实（cause + inserted/deleted + 稳定身份），不自己拼
/// source、device_id、session_id。`cause → EventSource` 映射和设备身份管理由
/// Core 内部完成。`platform` 固定为 `"linux"`。
///
/// Issue #935 复核：`event_id` / `occurred_at_ms` 由调用方首次入队时固定，
/// 写盘失败重发必须原样带回，Core 才能按 `event_id` 判重而不是把重发当新事件。
pub fn record_editor_change_stats(
    api: &WriterCoreApi,
    project_id: &str,
    volume_id: &str,
    chapter_id: &str,
    cause: writer_core::editor::EditorTransactionCause,
    inserted_chars: u32,
    deleted_chars: u32,
    event_id: &str,
    occurred_at_ms: i64,
) -> Result<(), WriterError> {
    let input = writer_core::api::EditorChangeStatsInputDto {
        platform: "linux".to_string(),
        project_id: project_id.to_string(),
        volume_id: volume_id.to_string(),
        chapter_id: chapter_id.to_string(),
        cause: cause.into(),
        inserted_chars,
        deleted_chars,
        // Issue #935 复核：身份与发生时间由入队时固定，重发原样带回。
        event_id: event_id.to_string(),
        occurred_at_ms,
    };
    api.record_editor_change_stats(input)
        .map_err(WriterError::from)
}

/// Issue #981：上报一段平台端已用单调时钟测量完成的前台写作时长。
///
/// 平台只提供会话、时间范围和章节事实；设备身份、平台归类和本地日期由 Core
/// 补齐并按 `event_id` 幂等落盘。
pub fn record_writing_activity_segment(
    api: &WriterCoreApi,
    input: writer_core::api::WritingActivitySegmentInputDto,
) -> Result<(), WriterError> {
    api.record_writing_activity_segment(input)
        .map_err(WriterError::from)
}
