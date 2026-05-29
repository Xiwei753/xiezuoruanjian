//! # 写作桥接函数（Linux UI 层 - Backend Adapter）
//!
//! 将 WriterCore 的写作 API 包装为兼容 DTO，供 AppBackend 转为 QML 对象。

use writer_core::api::WriterCoreApi;
use writer_core::facade::WriterCore;
use writer_core::error::Error;
use writer_core::chapter::{Chapter, ChapterSaveReceipt};
use writer_core::api::types::ChapterSaveReceiptDto;
use serde::Serialize;

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

pub fn open_chapter(api: &WriterCoreApi, project_id: &str, volume_id: &str, chapter_id: &str) -> Result<LinuxChapterOpenData, writer_core::api::error::WriterError> {
    let chapters = api.list_chapters(project_id, volume_id).unwrap_or_default();
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

pub fn save_chapter(api: &WriterCoreApi, project_id: &str, volume_id: &str, chapter_id: &str, text_str: &str) -> Result<ChapterSaveReceiptDto, writer_core::api::error::WriterError> {
    let chapters = api.list_chapters(project_id, volume_id).unwrap_or_default();
    if chapters.iter().any(|ch| ch.id == chapter_id) {
        let receipt = api.save_chapter_content(project_id, volume_id, chapter_id, text_str)?;
        Ok(receipt)
    } else {
        Err(writer_core::api::error::WriterError::ChapterNotFound)
    }
}

pub fn clear_chapter_content(api: &WriterCoreApi, project_id: &str, volume_id: &str, chapter_id: &str) -> Result<ChapterSaveReceiptDto, writer_core::api::error::WriterError> {
    let receipt = api.clear_chapter_content(project_id, volume_id, chapter_id)?;
    Ok(receipt)
}

pub fn report_writing_event(
    core: &WriterCore, 
    project_id: &str, 
    volume_id: &str, 
    chapter_id: &str, 
    source: &str, 
    inserted_chars: u32, 
    deleted_chars: u32, 
    pasted_chars: u32, 
    time_spent: u32,
    device_id: &str,
    session_id: &str
) -> Result<(), Error> {
    core.record_writing_event(
        device_id,
        "linux",
        project_id,
        volume_id,
        chapter_id,
        source,
        inserted_chars,
        deleted_chars,
        pasted_chars,
        time_spent,
        session_id
    )
}

pub fn process_writing_event_from_text(
    core: &WriterCore,
    project_id: &str,
    volume_id: &str,
    chapter_id: &str,
    old_text: &str,
    new_text: &str,
    device_id: &str,
    session_id: &str
) -> Result<(), Error> {
    core.process_writing_event(
        device_id,
        "linux",
        project_id,
        volume_id,
        chapter_id,
        old_text,
        new_text,
        session_id
    )
}

pub fn ensure_stats_session(
    core: &WriterCore,
    device_id: &mut String,
    session_id: &mut String,
    last_event_ms: &mut i64
) {
    if device_id.is_empty() {
        *device_id = format!("linux-{}", uuid::Uuid::new_v4());
        let mut local = core.load_local_settings().unwrap_or_default();
        local.stats_device_id = Some(device_id.clone());
        let _ = core.save_local_settings(&local);
    }

    let now_ms = chrono::Utc::now().timestamp_millis();
    if *last_event_ms == 0 || (now_ms - *last_event_ms) > 5 * 60 * 1000 {
        *session_id = uuid::Uuid::new_v4().to_string();
    }
    *last_event_ms = now_ms;
}
