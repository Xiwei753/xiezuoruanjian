use serde::{Deserialize, Serialize};

use super::visual::{
    AnimatedSliceRole, CursorPath, DecorationSlice, StaticLinePatch, Timeline,
    UnifiedTransactionKind, VisualClassKind, VisualLayoutRevision,
    PlatformVisualTransactionState,
};
use super::composition::CompositionVisualRevision;
use super::rebase::{TransactionCancelReason, TransactionRebase};


/// 跨平台视觉事务语义边界。
///
/// Core 输出 EditorVisualTransaction；平台端收到后，根据平台布局
/// 生成 PlatformVisualTransaction。两端共享此结构和状态机概念，
/// 不共享平台渲染结构。
///
/// `visualResource` 字段由平台各自实现，不进入此结构。
///
/// #516: 四种事务（BodyEdit、CompositionUpdate、CompositionCommitOrCancel、CursorOnly）
/// 全部进入同一队列和 Timeline。不再存在独立预输入覆盖主路径、
/// 独立光标位移动画时间源。
///
/// `slice_document_byte_ranges` 与 `slice_roles` / `visual_class_kinds` 一一对应，
/// 每个元素为半开区间 `[start, end)`（UTF-8 byte offset）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformVisualTransaction {
    /// 事务唯一 ID
    pub transaction_id: u64,
    /// 事务 generation，用于过期检测
    pub generation: u64,
    /// 事务当前状态
    pub state: PlatformVisualTransactionState,
    /// 旧视觉布局修订
    pub old_revision: VisualLayoutRevision,
    /// 新视觉布局修订
    pub new_revision: VisualLayoutRevision,
    /// 切片角色列表（与 slice_document_byte_ranges / visual_class_kinds 对应）
    pub slice_roles: Vec<AnimatedSliceRole>,
    /// 切片文档字节范围列表（半开区间 `[start, end)`，UTF-8 byte offset）
    pub slice_document_byte_ranges: Vec<(usize, usize)>,
    /// 静态行补丁列表（不需要动画的行）
    pub static_line_patches: Vec<StaticLinePatch>,
    /// 光标过渡范围起始（UTF-8 byte offset）
    pub cursor_transition_byte_start: usize,
    /// 光标过渡范围结束（UTF-8 byte offset）
    pub cursor_transition_byte_end: usize,
    pub duration_ms: u64,
    pub rendering_started_at_ms: Option<u64>,
    pub accumulated_paused_duration_ms: u64,
    /// #516: 统一时钟 — 文字切片、光标、预输入装饰全部消费同一个 progress
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeline: Option<Timeline>,
    /// #516: 统一事务类型（必填，不再允许 None）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unified_kind: Option<UnifiedTransactionKind>,
    /// #516: 视觉对象分类列表（与 slice_roles/slice_document_byte_ranges 对应）
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub visual_class_kinds: Vec<VisualClassKind>,
    /// #516: 装饰切片（预输入下划线、分段颜色、IME cursor）
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub decoration_slices: Vec<DecorationSlice>,
    /// #516: 光标路径（使用同一 Timeline）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor_path: Option<CursorPath>,
    /// #516: 预输入视觉修订（仅 CompositionUpdate/CompositionCommitOrCancel 事务）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub composition_revision: Option<CompositionVisualRevision>,
    /// #516: 连续事务 rebase 信息
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rebase: Option<TransactionRebase>,
    /// #516: 取消原因（仅 Cancelled 状态有值）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancel_reason: Option<TransactionCancelReason>,
}