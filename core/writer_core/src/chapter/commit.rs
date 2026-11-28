//! 章节正文的单一版本化提交协议。
//!
//! 所有正文写入（编辑器保存、清空、同步合并、恢复、MCP）都必须经过本模块：
//! 先在同一把 [`crate::storage::WorkspaceWriteLock`] 下核对章节版本，再把
//! 决议写入 durable journal，最后发布正文/元数据/附加文件投影。journal 是
//! 本地提交权威；`chapter.md`、`chapter.meta.json` 与附加文件只是可幂等重放的
//! 投影。崩溃后恢复只按 journal 的 before/after 摘要裁决，绝不把旧 staging
//! 无条件 rename 到新正文上。
//!
//! ## 版本
//!
//! 每个章节有稳定的 `object_generation`（创建时分配）、单调递增的 `revision`
//! 与正文 SHA-256。`revision` 只在正文真实变化时推进；空正文是有效内容，
//! 章节不存在是 tombstone。编辑器传入的 base version 与当前版本不一致时，
//! 保留 base/local/incoming 三方材料并返回结构化冲突，不允许按当前时间
//! 把过期正文变成新版本。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{Chapter, ChapterSaveReceipt};
use crate::error::{Error, Result};

/// 章节 journal 所在目录（相对 project root）。
const JOURNAL_DIR: &str = "app-meta/chapter-commits";
/// 存在未完成 journal 的快速探针；读取路径见到它才做恢复扫描。
const PENDING_MARKER: &str = "pending.marker";
/// 冲突材料目录。
const CONFLICT_DIR: &str = "conflicts";
/// 每章未完成正文决议的定位目录，不是 journal 事务目录。
const CHAPTERS_DIR: &str = "chapters";
/// 正文投影文件名。
const BODY_FILE: &str = "chapter.md";
/// 元数据投影文件名。
const META_FILE: &str = "chapter.meta.json";

/// 章节身份。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChapterKey {
    pub project_id: String,
    pub volume_id: String,
    pub chapter_id: String,
}

impl ChapterKey {
    pub fn new(
        project_id: impl Into<String>,
        volume_id: impl Into<String>,
        chapter_id: impl Into<String>,
    ) -> Self {
        Self {
            project_id: project_id.into(),
            volume_id: volume_id.into(),
            chapter_id: chapter_id.into(),
        }
    }

    fn chapter_dir(&self, project_root: &Path) -> PathBuf {
        project_root
            .join("volumes")
            .join(&self.volume_id)
            .join("chapters")
            .join(&self.chapter_id)
    }

    /// 正文相对 project root 的稳定路径。
    pub fn body_relative_path(&self) -> String {
        journal::chapter_relative_path(self)
    }

    fn body_path(&self, project_root: &Path) -> PathBuf {
        self.chapter_dir(project_root).join(BODY_FILE)
    }

    fn meta_path(&self, project_root: &Path) -> PathBuf {
        self.chapter_dir(project_root).join(META_FILE)
    }
}

/// 一个章节正文的持久化版本。
///
/// `object_generation` 在章节创建时分配，删除后重建会得到新的 generation；
/// `revision` 是跨进程、跨设备的持久化修订号；`content_sha256` 是正文
/// （不是展示文本）的 SHA-256。编辑器内的临时 revision 不写进这里。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChapterVersion {
    #[serde(default)]
    pub object_generation: String,
    #[serde(default)]
    pub revision: u64,
    #[serde(default)]
    pub content_sha256: String,
}

/// 与正文同一 durable journal 的证明事件投影。
///
/// 事件由编辑热路径产生，但只有正文提交决议 durable 后才允许写入统计/证明；
/// 崩溃后由投影 worker 从 journal 幂等重放。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChapterProofProjection {
    pub event: crate::writing_stats::WritingInputEvent,
    pub baseline_text: Option<String>,
}

impl ChapterProofProjection {
    pub fn event_id(&self) -> &str {
        &self.event.event_id
    }
}

/// 同步正文变化事实。与正文/meta 同一 journal durable，投影时再由
/// WriterCore 按当前证明链头生成正式 `WritingInputEvent`；这样同步提交
/// 不需要在正文落盘后另写一条独立证明。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChapterSyncProofFact {
    pub event_id: String,
    pub key: ChapterKey,
    pub before_text: Option<String>,
    pub after_text: Option<String>,
    pub source_device_id: String,
    pub occurred_at_ms: i64,
}

/// 同步正文提交的 durable 回执。调用方必须用它驱动证明投影/同步结果，
/// 不能再用 committed_paths 的枚举序号反推章节事实。
#[derive(Debug, Clone)]
pub struct ChapterSyncCommitReceipt {
    pub transaction_id: String,
    pub key: ChapterKey,
    pub event_id: Option<String>,
    pub source_device_id: String,
    pub occurred_at_ms: i64,
    pub before_text: Option<String>,
    pub after_text: Option<String>,
    pub deleted: bool,
}

/// 提交来源。只用于 journal/诊断，不改变版本语义。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChapterCommitSource {
    Editor,
    Api,
    Mcp,
    Sync,
    Restore,
    Import,
}

/// 一次提交的语义。journal 记录它，恢复时据此判断投影范围。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChapterMutationKind {
    EditBatch,
    Replace,
    Clear,
    SyncMerge,
    Restore,
    Create,
    Delete,
}

/// 章节提交请求。
#[derive(Debug, Clone)]
pub struct ChapterCommitRequest {
    pub key: ChapterKey,
    /// 提交后的正文。空字符串是有效正文（清空章节正文，不是删除章节）。
    pub content: String,
    /// 调用方读取/上次保存时拿到的版本。旧客户端传 `None`，仍会做
    /// read-modify-write，但不会把过期正文无条件盖到新版本上。
    pub expected: Option<ChapterVersion>,
    /// 调用方的 base 正文，仅用于冲突材料；正文比对永远以磁盘当前内容为准。
    pub base_content: Option<String>,
    /// Create 提交的初始元数据模板（标题/顺序来自远端或导入来源）。
    /// 其他 mutation 忽略。
    pub meta_template: Option<Chapter>,
    /// 与正文同一决议提交的标题/顺序（同步远端章对象变更）。
    pub meta_title: Option<String>,
    pub meta_order: Option<i32>,
    pub mutation: ChapterMutationKind,
    pub source: ChapterCommitSource,
    /// 同一次编辑批次固定的事件 ID。journal durable 后由调用方补投影。
    pub event_ids: Vec<String>,
    /// 原始证明事件；与正文、meta 同一决议 durable，提交后由 worker 写入。
    pub proof_events: Vec<ChapterProofProjection>,
    /// 平台冻结的编辑器快照 revision；用于精确截取证明批次，避免按摘要猜范围。
    pub proof_snapshot_revision: Option<u64>,
    /// 同步正文变化事实；投影时才构造正式事件，避免正文后独立写证明。
    pub sync_proof_facts: Vec<ChapterSyncProofFact>,
    /// 与正文同一提交的附加文件（annotation 等），project-relative 路径。
    pub additional_files: Vec<(String, String)>,
    pub allow_empty_overwrite: bool,
    pub transaction_id: Option<String>,
    /// 删除提交的真实 app data root 与固定 trash 路径；由上层传入，避免
    /// journal 用 `ancestors().nth(2)` 猜错独立的 projects_root。
    pub app_data_root: Option<std::path::PathBuf>,
    pub trash_path: Option<std::path::PathBuf>,
    /// 需要调用方在正文/元数据发布后补写的投影 ID；为空表示无滞后投影。
    pub projection_ids: Vec<String>,
}

impl ChapterCommitRequest {
    pub fn new(key: ChapterKey, content: impl Into<String>) -> Self {
        Self {
            key,
            content: content.into(),
            expected: None,
            base_content: None,
            meta_template: None,
            meta_title: None,
            meta_order: None,
            mutation: ChapterMutationKind::Replace,
            source: ChapterCommitSource::Api,
            event_ids: Vec::new(),
            proof_events: Vec::new(),
            proof_snapshot_revision: None,
            sync_proof_facts: Vec::new(),
            additional_files: Vec::new(),
            allow_empty_overwrite: false,
            transaction_id: None,
            app_data_root: None,
            trash_path: None,
            projection_ids: Vec::new(),
        }
    }
}

/// 提交成功回执。`before`/`after` 是持久化版本，不是编辑器内部 revision。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChapterCommitReceipt {
    pub transaction_id: String,
    pub key: ChapterKey,
    pub before: ChapterVersion,
    pub after: ChapterVersion,
    pub content_changed: bool,
    pub content_len: usize,
    /// 兼容既有客户端的正文 MD5。
    pub content_hash: String,
    pub meta_hash: String,
    pub updated_at: String,
    pub word_count: u32,
    /// 本次真实写入的 project-relative 路径；no-op 时为空。
    pub written_files: Vec<String>,
    /// 尚未完成的投影 ID；空表示 journal 已可以清理。
    pub pending_projections: Vec<String>,
    /// 本次 durable 提交的项目级序号；平台 ack 重放顺序的唯一依据。
    #[serde(default)]
    pub commit_sequence: u64,
}

impl ChapterCommitReceipt {
    /// 转成既有保存回执，并携带准确的新版本。
    pub fn into_save_receipt(self, chapter_relative_path: String) -> ChapterSaveReceipt {
        ChapterSaveReceipt {
            chapter_relative_path,
            content_len: self.content_len,
            content_hash: self.content_hash,
            meta_hash: self.meta_hash,
            updated_at: self.updated_at,
            word_count: self.word_count,
            version: self.after,
            transaction_id: self.transaction_id,
            pending_projections: self.pending_projections,
            commit_sequence: self.commit_sequence,
        }
    }
}

/// 版本冲突。三方内容已写入 `conflict_dir`，调用方可展示或做三方合并。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChapterConflict {
    pub key: ChapterKey,
    pub expected: Option<ChapterVersion>,
    pub actual: ChapterVersion,
    /// 本次冲突材料所属的章节提交事务 ID（与 `conflict_dir` 目录名一致）。
    pub transaction_id: String,
    pub conflict_dir: String,
}

/// 编辑会话冲突材料输入。
///
/// 与 [`ChapterCommitRequest`] 不同，这里只保存“按会话身份冻结的双方正文与共同基线”，
/// 不发布正文、不检查空覆盖、不参与版本 CAS。`local.md` 是 Core 编辑会话当前正文，
/// `incoming.md` 是原生编辑器正文，`base.md` 是共同基线（`Some("")` 表示合法空基线）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorConflictMaterialRequest {
    pub key: ChapterKey,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_content: Option<String>,
    pub core_content: String,
    pub native_content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub core_version: Option<ChapterVersion>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_version: Option<ChapterVersion>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub core_session_id: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_session_id: Option<u64>,
    /// Core 会话当前 revision（版本对象缺 generation/sha 时仍保留可追踪版本）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub core_revision: Option<u64>,
    /// 共同基线 revision。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_revision: Option<u64>,
    /// 平台本地变更序号，用于区分同一 revision 的连续冲突内容。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub change_sequence: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// 编辑会话冲突材料回执：材料目录与双方正文摘要，供平台按会话+内容版本跟踪。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorConflictMaterialReceipt {
    pub transaction_id: String,
    pub conflict_dir: String,
    pub core_content_sha256: String,
    pub native_content_sha256: String,
}

/// 一个仍有未确认证明投影的 journal，用于按章节/提交序号串行结算。
#[derive(Debug, Clone)]
pub struct PendingChapterProofTransaction {
    pub transaction_id: String,
    pub key: ChapterKey,
    pub commit_sequence: u64,
}

/// 一笔 Core 已 durable、但平台草稿尚未确认的提交。
#[derive(Debug, Clone)]
pub struct PendingChapterDraftAck {
    pub transaction_id: String,
    pub key: ChapterKey,
    pub event_ids: Vec<String>,
    pub after: ChapterVersion,
    pub commit_sequence: u64,
}

/// 一次同步实际应用到某章节的结果。平台端按它决定是否需要重载正文，
/// 不再依赖全局 `local_content_changed` bool。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChapterChangeReceipt {
    pub key: ChapterKey,
    /// 同步前正文版本；文件原本不存在时为空。
    pub before: Option<ChapterVersion>,
    /// 同步后正文版本；删除时为空。
    pub after: Option<ChapterVersion>,
    /// 正文内容是否真的变化（删除也算变化）。
    pub content_changed: bool,
    pub deleted: bool,
    pub operation_id: String,
}

/// 提交结果。冲突不是“写坏了再回滚”，而是根本没发布投影。
#[derive(Debug, Clone)]
pub enum ChapterCommitOutcome {
    Committed(ChapterCommitReceipt),
    Conflict(ChapterConflict),
}

mod journal;
mod publish;
pub(crate) use journal::commit_chapter_with_guard;
pub(crate) use journal::read_chapter_state;
pub(crate) use journal::write_editor_conflict_material;
pub(crate) use journal::{
    pending_chapter_proof_transactions_for_key,
    pending_chapter_proof_transactions_for_transaction,
};
pub use journal::{
    mark_chapter_commit_draft_ack, mark_chapter_commit_projection, mark_chapter_commit_projections,
    pending_chapter_draft_acks,
    pending_chapter_proof_events, pending_chapter_proof_transactions,
    pending_chapter_sync_proof_fact_ids, pending_chapter_sync_proof_facts,
    read_chapter_state_recovering, read_chapter_version, read_chapter_version_recovering,
    recover_pending_chapter_commits, replay_pending_sync_mutation, replay_pending_sync_mutations,
};
#[cfg(test)]
mod tests;
