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
    /// 本次 durable 提交在项目 journal 中的序号；no-op/旧记录为 0。
    ///
    /// 平台草稿 ack 必须按它（而不是 generation 字符串或随机 ID）重放，
    /// 跨对象 generation 的提交顺序也由项目级 commit_sequence 决定。
    pub commit_sequence: u64,
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
            commit_sequence: r.commit_sequence,
        }
    }
}

/// 编辑会话冲突材料请求。只写材料，不发布正文、不做空覆盖检查与版本 CAS。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorConflictMaterialRequestDto {
    pub project_id: String,
    pub volume_id: String,
    pub chapter_id: String,
    /// `None` 表示没有共同基线；`Some("")` 是合法空基线，仍写 base.md。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_content: Option<String>,
    /// 冲突时 Core 编辑会话当前正文（local.md）。
    pub core_content: String,
    /// 冲突时原生编辑器正文（incoming.md）。
    pub native_content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub core_version: Option<ChapterVersionDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_version: Option<ChapterVersionDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub core_session_id: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_session_id: Option<u64>,
    /// Core 会话当前 revision；版本对象缺 generation/sha 时仍可追踪。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub core_revision: Option<u64>,
    /// 共同基线 revision。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_revision: Option<u64>,
    /// 平台本地变更序号（同一 revision 的连续冲突内容）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub change_sequence: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl EditorConflictMaterialRequestDto {
    pub fn into_core(self) -> crate::chapter::commit::EditorConflictMaterialRequest {
        crate::chapter::commit::EditorConflictMaterialRequest {
            key: crate::chapter::ChapterKey::new(self.project_id, self.volume_id, self.chapter_id),
            base_content: self.base_content,
            core_content: self.core_content,
            native_content: self.native_content,
            core_version: self.core_version.map(ChapterVersionDto::into_core),
            local_version: self.local_version.map(ChapterVersionDto::into_core),
            core_session_id: self.core_session_id,
            local_session_id: self.local_session_id,
            core_revision: self.core_revision,
            base_revision: self.base_revision,
            change_sequence: self.change_sequence,
            detail: self.detail,
        }
    }
}

/// 编辑会话冲突材料回执：实际目录与双方正文摘要。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorConflictMaterialReceiptDto {
    pub transaction_id: String,
    pub conflict_dir: String,
    pub core_content_sha256: String,
    pub native_content_sha256: String,
}

impl From<crate::chapter::commit::EditorConflictMaterialReceipt>
    for EditorConflictMaterialReceiptDto
{
    fn from(receipt: crate::chapter::commit::EditorConflictMaterialReceipt) -> Self {
        Self {
            transaction_id: receipt.transaction_id,
            conflict_dir: receipt.conflict_dir,
            core_content_sha256: receipt.core_content_sha256,
            native_content_sha256: receipt.native_content_sha256,
        }
    }
}
