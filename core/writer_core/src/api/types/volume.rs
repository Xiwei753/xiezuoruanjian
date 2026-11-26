use crate::facade::ChapterOpenResult;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct VolumeDto {
    pub id: String,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
    pub order: i32,
}

impl From<crate::volume::Volume> for VolumeDto {
    fn from(v: crate::volume::Volume) -> Self {
        Self {
            id: v.id,
            title: v.title,
            created_at: v.created_at,
            updated_at: v.updated_at,
            order: v.order,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ChapterMetaDto {
    pub id: String,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
    pub order: i32,
    pub word_count: u32,
    pub hash: String,
    pub note: Option<String>,
}

impl From<crate::chapter::Chapter> for ChapterMetaDto {
    fn from(c: crate::chapter::Chapter) -> Self {
        Self {
            id: c.id,
            title: c.title,
            created_at: c.created_at,
            updated_at: c.updated_at,
            order: c.order,
            word_count: c.word_count,
            hash: c.hash,
            note: c.note,
        }
    }
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ChapterVersionDto {
    pub object_generation: String,
    pub revision: u64,
    pub content_sha256: String,
}

impl From<crate::chapter::ChapterVersion> for ChapterVersionDto {
    fn from(version: crate::chapter::ChapterVersion) -> Self {
        Self {
            object_generation: version.object_generation,
            revision: version.revision,
            content_sha256: version.content_sha256,
        }
    }
}

impl ChapterVersionDto {
    pub fn into_core(self) -> crate::chapter::ChapterVersion {
        crate::chapter::ChapterVersion {
            object_generation: self.object_generation,
            revision: self.revision,
            content_sha256: self.content_sha256,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ChapterContentDto {
    pub meta: ChapterMetaDto,
    pub content: String,
    /// 打开时正文的持久化版本；与 `meta.hash`（可能是旧值）不同，它总是
    /// 按实际读到的正文重算。
    pub version: ChapterVersionDto,
}

impl From<crate::chapter::ChapterContent> for ChapterContentDto {
    fn from(c: crate::chapter::ChapterContent) -> Self {
        Self {
            meta: c.meta.into(),
            content: c.content,
            version: c.version.into(),
        }
    }
}

impl From<ChapterOpenResult> for ChapterContentDto {
    fn from(c: ChapterOpenResult) -> Self {
        Self {
            meta: c.meta.into(),
            content: c.content,
            version: c.version.into(),
        }
    }
}

/// 一笔 Core 已 durable、但平台草稿尚未确认的提交（Issue #1062）。
///
/// 平台进入作品时按 `commit_sequence` 顺序重放：先本地 `mark_committed`，
/// 成功后才调用 `acknowledge_chapter_commit_draft`。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ChapterDraftAckDto {
    pub transaction_id: String,
    pub project_id: String,
    pub volume_id: String,
    pub chapter_id: String,
    pub event_ids: Vec<String>,
    /// 该提交实际发布的正文版本；重放 ack 时用它推进草稿 base。
    pub after: ChapterVersionDto,
    pub commit_sequence: u64,
}

impl From<crate::chapter::commit::PendingChapterDraftAck> for ChapterDraftAckDto {
    fn from(ack: crate::chapter::commit::PendingChapterDraftAck) -> Self {
        Self {
            transaction_id: ack.transaction_id,
            project_id: ack.key.project_id,
            volume_id: ack.key.volume_id,
            chapter_id: ack.key.chapter_id,
            event_ids: ack.event_ids,
            after: ack.after.into(),
            commit_sequence: ack.commit_sequence,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ChapterSaveReceiptDto {
    pub chapter_relative_path: String,
    pub content_len: u32,
    pub content_hash: String,
    pub meta_hash: String,
    pub updated_at: String,
    pub word_count: u32,
    /// 本次提交后的准确持久化版本。
    pub version: ChapterVersionDto,
    /// durable journal ID；no-op 时为空。
    pub transaction_id: String,
    /// 尚未确认的投影 ID（本批证明事件）。
    pub pending_projections: Vec<String>,
}

#[allow(clippy::cast_possible_truncation)]
impl From<crate::chapter::ChapterSaveReceipt> for ChapterSaveReceiptDto {
    fn from(r: crate::chapter::ChapterSaveReceipt) -> Self {
        Self {
            chapter_relative_path: r.chapter_relative_path,
            content_len: r.content_len as u32,
            content_hash: r.content_hash,
            meta_hash: r.meta_hash,
            updated_at: r.updated_at,
            word_count: r.word_count,
            version: r.version.into(),
            transaction_id: r.transaction_id,
            pending_projections: r.pending_projections,
        }
    }
}
