use super::layout_snapshot::{LineSnapshotId, SourceRect};
use super::transaction_key::VisualTransactionKey;

/// 静态正文层在动画期间不能继续完整绘制受影响行，否则会与 overlay 双绘。
/// patch 负责从已录制行快照中只绘制未被动画切片接管的部分。
///
/// 坐标空间：
/// - `hidden_source_rects`：行视觉资源局部坐标（已乘 DPR），标识被动画切片接管的区域。
/// - `byte_start`/`byte_end`：UTF-8 文档范围，用于静态层判断哪些行需要裁剪。
/// - `snapshot_id`：对应的行快照 ID，用于查找视觉资源。
#[derive(Clone, Debug)]
pub(crate) struct StaticLinePatch {
    pub key: VisualTransactionKey,
    pub snapshot_id: LineSnapshotId,
    pub hidden_source_rects: Vec<SourceRect>,
    pub byte_start: usize,
    pub byte_end: usize,
    pub is_insert: bool,
}

impl StaticLinePatch {
    pub fn insert_patch(
        key: VisualTransactionKey,
        snapshot_id: LineSnapshotId,
        hidden_source_rects: Vec<SourceRect>,
        byte_start: usize,
        byte_end: usize,
    ) -> Self {
        Self {
            key,
            snapshot_id,
            hidden_source_rects,
            byte_start,
            byte_end,
            is_insert: true,
        }
    }

    pub fn reflow_patch(
        key: VisualTransactionKey,
        snapshot_id: LineSnapshotId,
        hidden_source_rects: Vec<SourceRect>,
        byte_start: usize,
        byte_end: usize,
    ) -> Self {
        Self {
            key,
            snapshot_id,
            hidden_source_rects,
            byte_start,
            byte_end,
            is_insert: false,
        }
    }

    pub fn intersects(&self, byte_start: usize, byte_end: usize) -> bool {
        self.byte_end > byte_start && self.byte_start < byte_end
    }
}
