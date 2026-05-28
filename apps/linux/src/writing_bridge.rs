//! # 写作桥接函数（Linux UI 层 - Backend Adapter）
//!
//! 将 WriterCore 的写作 API 包装为兼容 DTO，供 AppBackend 转为 QML 对象。

use writer_core::facade::WriterCore;
use writer_core::error::Error;
use serde_json::Value;

pub fn open_chapter(core: &WriterCore, project_id: &str, volume_id: &str, chapter_id: &str) -> Result<Value, String> {
    let chapter_exists = core
        .list_chapters(project_id, volume_id)
        .map(|chapters| chapters.iter().any(|ch| ch.id == chapter_id))
        .unwrap_or(false);

    if chapter_exists {
        match core.open_chapter(project_id, volume_id, chapter_id) {
            Ok(content) => Ok(serde_json::json!({
                "success": true,
                "content": content.content,
                "title": content.meta.title,
                "projectId": project_id,
                "volumeId": volume_id,
                "chapterId": chapter_id,
                "meta": content.meta,
            })),
            Err(e) => Err(format!("error={}", e)),
        }
    } else {
        Err("chapter_not_exists".to_string())
    }
}

pub fn save_chapter(core: &WriterCore, project_id: &str, volume_id: &str, chapter_id: &str, text_str: &str) -> Result<Value, Error> {
    let chapters = core.list_chapters(project_id, volume_id).unwrap_or_default();
    if chapters.iter().any(|ch| ch.id == chapter_id) {
        let receipt = core.write_chapter_verified(project_id, volume_id, chapter_id, text_str)?;
        Ok(serde_json::json!({
            "success": true,
            "data": receipt
        }))
    } else {
        Err(Error::Io(std::io::Error::new(std::io::ErrorKind::NotFound, "chapter_not_exists")))
    }
}

pub fn clear_chapter_content(core: &WriterCore, project_id: &str, volume_id: &str, chapter_id: &str) -> Result<Value, Error> {
    let receipt = core.clear_chapter_content_verified(project_id, volume_id, chapter_id)?;
    Ok(serde_json::json!({
        "success": true,
        "data": receipt
    }))
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
) {
    let _ = core.record_writing_event(
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
    );
}
