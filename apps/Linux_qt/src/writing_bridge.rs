// =============================================================================
// writing_bridge.rs — 写作与编辑器数据桥接层
// =============================================================================
//
// 引用了什么：
// - writer_core::api::error::WriterError：核心统一业务错误。
// - writer_core::api::types::ChapterSaveReceiptDto：章节保存结果回执 DTO。
// - writer_core::api::WriterCoreApi：核心库主业务 API。
//
// 干什么的：
// - 负责编辑器界面底层与核心写作 API 的桥接。
// - 提供打开章节、缓存并返回 LinuxChapterOpenData 的接口。
// - 封装章节内容安全保存语义（支持allow_empty_overwrite校验）、清空正文内容（clear_chapter_content）的核心实现。
// - 负责将高频按键输入或粘贴动作翻译并记录为写作统计事件流（process_writing_event_from_text等）。
// - 维护统计会话 Session 的生命周期及统计专用设备 ID 的自动生成与本地持久化。
//
// 被什么引用：
// - 被 apps/Linux_qt/src/backend/editor_backend.rs 引用，作为主写作编辑器的后端状态控制器与统计源。
// =============================================================================

//! # 写作桥接函数（Linux_qt UI 层 - Backend Adapter）
//!
//! 将 WriterCoreApi 的写作 API 包装为兼容 DTO，供 AppBackend 转为 QML 对象。

use serde::Serialize;
use writer_core::api::error::WriterError;
use writer_core::api::types::ChapterSaveReceiptDto;
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

pub fn save_chapter(
    api: &WriterCoreApi,
    project_id: &str,
    volume_id: &str,
    chapter_id: &str,
    text_str: &str,
    allow_empty_overwrite: bool,
) -> Result<ChapterSaveReceiptDto, writer_core::api::error::WriterError> {
    let chapters = api.list_chapters(project_id, volume_id)?;
    if chapters.iter().any(|ch| ch.id == chapter_id) {
        let receipt = api.save_chapter_content_with_options(
            project_id,
            volume_id,
            chapter_id,
            text_str,
            allow_empty_overwrite,
        )?;
        Ok(receipt)
    } else {
        Err(writer_core::api::error::WriterError::ChapterNotFound)
    }
}

pub fn clear_chapter_content(
    api: &WriterCoreApi,
    project_id: &str,
    volume_id: &str,
    chapter_id: &str,
) -> Result<ChapterSaveReceiptDto, writer_core::api::error::WriterError> {
    let receipt = api.clear_chapter_content(project_id, volume_id, chapter_id)?;
    Ok(receipt)
}

pub fn report_writing_event(
    api: &WriterCoreApi,
    project_id: &str,
    volume_id: &str,
    chapter_id: &str,
    source: &str,
    inserted_chars: u32,
    deleted_chars: u32,
    pasted_chars: u32,
    ai_inserted_chars: u32,
    device_id: &str,
    session_id: &str,
) -> Result<bool, WriterError> {
    let platform = "linux";
    api.record_writing_event_for_platform(
        device_id,
        platform,
        project_id,
        volume_id,
        chapter_id,
        source,
        inserted_chars as i32,
        deleted_chars as i32,
        pasted_chars as i32,
        ai_inserted_chars as i32,
        0, // duration_seconds: not tracked on Linux_qt, default to 0
        session_id,
    )
}

pub fn process_writing_event_from_text(
    api: &WriterCoreApi,
    project_id: &str,
    volume_id: &str,
    chapter_id: &str,
    old_text: &str,
    new_text: &str,
    device_id: &str,
    session_id: &str,
) -> Result<bool, WriterError> {
    let platform = "linux";
    api.process_writing_event(
        device_id, platform, project_id, volume_id, chapter_id, old_text, new_text, 0, session_id,
    )
}

pub fn ensure_stats_session(
    api: &WriterCoreApi,
    device_id: &mut String,
    session_id: &mut String,
    last_event_ms: &mut i64,
) {
    if device_id.is_empty() {
        *device_id = format!("linux-{}", uuid::Uuid::new_v4());
        if let Ok(mut local) = api.load_local_settings() {
            local.stats_device_id = Some(device_id.clone());
            let _ = api.save_local_settings(local);
        }
    }

    let now_ms = chrono::Utc::now().timestamp_millis();
    if *last_event_ms == 0 || (now_ms - *last_event_ms) > 5 * 60 * 1000 {
        *session_id = uuid::Uuid::new_v4().to_string();
    }
    *last_event_ms = now_ms;
}
