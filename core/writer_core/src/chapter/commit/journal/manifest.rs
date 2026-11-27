//! durable commit decision 的磁盘结构（旧 journal 兼容字段见 serde default）。

use super::*;

/// journal 中的一个附加文件投影。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AdditionalFilePlan {
    pub(crate) target_relative: String,
    pub(crate) staging_filename: String,
    /// 提交决议时的目标文件摘要；`None` 表示目标原本不存在。
    #[serde(default)]
    pub(crate) before_sha256: Option<String>,
    pub(crate) sha256: String,
}

/// 与提交决议同一 durable 事实的同步投影材料（Issue #1065 复审 #1）。
///
/// 对象路径、op、前后字节摘要、业务时间与事务 ID 在 Decided 之前就写进
/// manifest；Applied 之后只按它幂等投影，崩溃恢复也从同一决议重放，不再
/// 依赖提交后才落盘的第二份 outbox 文件。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ChapterSyncMutationPlan {
    /// 项目根相对的同步对象路径。
    pub(crate) path: String,
    /// `upsert` 或 `delete`。
    pub(crate) op: String,
    /// upsert 为发布后目标字节的 md5；delete 为空。
    #[serde(default)]
    pub(crate) content_hash: String,
    /// 目标对象的业务版本（meta 的 revision/updatedAt）；delete 为空。
    #[serde(default)]
    pub(crate) object_revision: Option<String>,
    pub(crate) updated_at_ms: i64,
    #[serde(default)]
    pub(crate) deleted_at_ms: Option<i64>,
    #[serde(default)]
    pub(crate) device_id: String,
}

/// durable commit decision。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ChapterCommitManifest {
    pub(crate) schema_version: u32,
    pub(crate) transaction_id: String,
    /// project root 的绝对路径；journal 本身在 project root 内，恢复时以它
    /// 为准校验，防止 journal 被移动到另一个项目后误写。
    pub(crate) project_root: String,
    pub(crate) key: ChapterKey,
    pub(crate) mutation: ChapterMutationKind,
    pub(crate) source: ChapterCommitSource,
    pub(crate) created_at_ms: i64,
    pub(crate) occurred_at_ms: i64,
    /// 项目内单调提交序号，用于证明投影按因果顺序重放。
    #[serde(default)]
    pub(crate) commit_sequence: u64,
    pub(crate) event_ids: Vec<String>,
    #[serde(default)]
    pub(crate) proof_events: Vec<super::ChapterProofProjection>,
    #[serde(default)]
    pub(crate) sync_proof_facts: Vec<super::ChapterSyncProofFact>,
    pub(crate) expected: Option<ChapterVersion>,
    pub(crate) before: ChapterVersion,
    pub(crate) after: ChapterVersion,
    pub(crate) body_changed: bool,
    pub(crate) content_len: usize,
    pub(crate) content_md5: String,
    pub(crate) word_count: u32,
    pub(crate) updated_at: String,
    pub(crate) additional_files: Vec<AdditionalFilePlan>,
    pub(crate) phase: ChapterCommitPhase,
    pub(crate) pending_projections: Vec<String>,
    /// Issue #1062：由平台草稿负责确认的原始证明事件 ID。与
    /// pending_projections 同一 durable 决议保存；平台草稿完成精确 ack 后
    /// 才清除，重启时平台据此重放草稿 ack（Core 不重复写统计）。
    #[serde(default)]
    pub(crate) draft_ack_projections: Vec<String>,
    /// Create 提交的初始 meta；发布时正文/meta 都不存在才允许写入。
    #[serde(default)]
    pub(crate) created_meta: Option<Chapter>,
    /// 与正文同一决议的标题/顺序；None 表示不修改。
    #[serde(default)]
    pub(crate) meta_title: Option<String>,
    #[serde(default)]
    pub(crate) meta_order: Option<i32>,
    /// 恢复时判断是否为创建提交；旧 journal 无此字段默认 false。
    #[serde(default)]
    pub(crate) created: bool,
    /// 删除提交：after 是 tombstone 版本，live 章节目录应移动到 trash。
    #[serde(default)]
    pub(crate) deleted: bool,
    /// 删除提交的真实 app data root（用于计算相对 trash 路径）。
    #[serde(default)]
    pub(crate) app_data_root: Option<String>,
    /// 删除提交预先确定的 trash 绝对路径；恢复时按它幂等补全移动。
    #[serde(default)]
    pub(crate) trash_path: Option<String>,
    /// 恢复时发现当前正文既不是 before 也不是 after，记录原因。
    #[serde(default)]
    pub(crate) superseded_reason: Option<String>,
    /// 本次决议要投影到同步账本的对象事实；远程同步提交为空。
    #[serde(default)]
    pub(crate) sync_mutations: Vec<ChapterSyncMutationPlan>,
    /// 同步投影是否已经 durable 落进账本；false 时 journal 不能进 Finished。
    #[serde(default)]
    pub(crate) sync_projection_done: bool,
}

/// 一次待恢复 journal 的可读结果。
#[derive(Debug, Clone)]
pub struct ChapterCommitRecovery {
    pub transaction_id: String,
    pub key: ChapterKey,
    pub after: ChapterVersion,
    pub pending_projections: Vec<String>,
    /// 仍需平台草稿确认的原始证明事件 ID（与 pending_projections 分开）。
    pub draft_ack_projections: Vec<String>,
    pub superseded: bool,
    pub conflict_dir: Option<String>,
}
