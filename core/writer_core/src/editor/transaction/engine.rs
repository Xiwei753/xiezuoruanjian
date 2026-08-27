use super::composition::OffsetMap;
use super::rebase::{
    RebaseContinuation, RebaseFrameSnapshot, RebaseReason, RebaseSliceMapping, TransactionRebase,
};
use super::types::{
    AnimationMode, EditorAnimationKind, EditorChange, EditorSelection, EditorTransaction,
    EditorTransactionCause,
};
#[cfg(test)]
use super::visual::EditorAnimationEvent;
use super::visual::{
    AnimatedSliceRole, ClusterRect, ClusterRun, EditorVisualTransaction, HiddenVisualRange,
    UnifiedTransactionKind, VisualClassKind, VisualCoordinateMode,
};
use crate::editor::strong_types::{Utf8ByteOffset, Utf8ByteRange};

/// 编辑引擎 — 创建 EditorTransaction 和 EditorVisualTransaction 的工厂。
///
/// 维护动画 ID 计数器和动画参数（max_animated_chars、animation_duration_ms）。
/// 无状态：不持有正文、选区或 undo/redo 栈，每次调用传入当前文本。
///
/// 与 `EditorKernel` 的关系：
/// - `EditorEngine` 是无状态工厂，负责计算动画参数和生成视觉事务
/// - `EditorKernel` 是有状态编辑器，持有正文、选区、undo/redo 栈和 composition session
/// - 两者共享 `max_animated_chars` 和 `animation_duration_ms` 参数，
///   但 `EditorEngine` 不参与编辑操作的状态管理
#[derive(Debug, Clone)]
pub struct EditorEngine {
    /// 下一个动画 ID（单调递增）
    next_animation_id: u64,
    /// 单次动画最大 glyph 数，超过时降级为 RunAnimation 或 SnapshotAnimation
    max_animated_chars: usize,
    /// 动画时长（毫秒），默认 160
    animation_duration_ms: u64,
}

impl Default for EditorEngine {
    fn default() -> Self {
        Self {
            next_animation_id: 1,
            max_animated_chars: 8,
            animation_duration_ms: 160,
        }
    }
}

impl EditorEngine {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_animation_limits(max_animated_chars: usize, animation_duration_ms: u64) -> Self {
        Self {
            next_animation_id: 1,
            max_animated_chars,
            animation_duration_ms,
        }
    }

    pub fn set_animation_duration_ms(&mut self, duration_ms: u64) {
        self.animation_duration_ms = duration_ms;
    }

    pub fn create_transaction(
        &self,
        old_text: impl Into<String>,
        new_text: impl Into<String>,
        old_selection: EditorSelection,
        new_selection: EditorSelection,
        cause: EditorTransactionCause,
    ) -> EditorTransaction {
        let old_text = old_text.into();
        let new_text = new_text.into();
        let changes = diff_plain_text(&old_text, &new_text);
        let should_animate = should_animate_changes(&changes, cause, self.max_animated_chars);

        EditorTransaction {
            old_text,
            new_text,
            changes,
            old_selection,
            new_selection,
            cause,
            should_animate,
        }
    }

    /// **DEPRECATED**: 已被 `visual_transaction()` 替代。
    /// 保留仅为现有测试覆盖；生产代码不得调用此方法。
    /// 当前主链是 `visual_transaction()`，见该方法文档。
    #[cfg(test)]
    #[deprecated(
        since = "0.12.0",
        note = "Use visual_transaction() instead. This will be removed in a future version."
    )]
    #[allow(deprecated)]
    pub(crate) fn animation_events(
        &mut self,
        transaction: &EditorTransaction,
    ) -> Vec<EditorAnimationEvent> {
        let mut events = Vec::new();
        if transaction.should_animate {
            for change in &transaction.changes {
                let kind = match change {
                    EditorChange::Insert { .. } => EditorAnimationKind::Insert,
                    EditorChange::Delete { .. } => EditorAnimationKind::Delete,
                };
                events.push(EditorAnimationEvent {
                    id: self.take_animation_id(),
                    kind,
                    range_start: change.index(),
                    range_len: Utf8ByteOffset::unchecked(change.text().len()),
                    text: change.text().to_string(),
                    old_cursor: transaction.old_selection.head,
                    new_cursor: transaction.new_selection.head,
                    duration_ms: self.animation_duration_ms,
                    glyph_rects: Vec::new(),
                    old_cursor_rect: None,
                    new_cursor_rect: None,
                });
            }
        }

        if transaction.cause != EditorTransactionCause::Load
            && transaction.old_selection.head != transaction.new_selection.head
        {
            events.push(EditorAnimationEvent {
                id: self.take_animation_id(),
                kind: EditorAnimationKind::Cursor,
                range_start: transaction.new_selection.head.index,
                range_len: Utf8ByteOffset::unchecked(0),
                text: String::new(),
                old_cursor: transaction.old_selection.head,
                new_cursor: transaction.new_selection.head,
                duration_ms: self.animation_duration_ms,
                glyph_rects: Vec::new(),
                old_cursor_rect: None,
                new_cursor_rect: None,
            });
        }

        events
    }

    fn take_animation_id(&mut self) -> u64 {
        let id = self.next_animation_id;
        self.next_animation_id = self.next_animation_id.saturating_add(1);
        id
    }

    /// 从 transaction 生成 EditorVisualTransaction。
    ///
    /// 职责划分：
    /// - Core 填充语义字段（id, kind, cause, old/new text, selection, inserted_range,
    ///   deleted_range, duration, coordinate_mode, animation_mode, cluster_rects, cluster_runs,
    ///   hidden_visual_ranges）。所有 byte range 均为 UTF-8 byte offset（半开区间）。
    /// - 平台层负责填充坐标字段（glyph_rects, cursor_rect, reflow_glyph_rects），
    ///   因为坐标需要平台布局引擎计算。
    ///
    /// 前置条件：transaction.should_animate == true 且 changes.len() == 1，
    /// 否则返回 None（多变更事务和不需要动画的事务不生成视觉事务）。
    pub fn visual_transaction(
        &mut self,
        transaction: &EditorTransaction,
    ) -> Option<EditorVisualTransaction> {
        if !transaction.should_animate {
            return None;
        }
        if transaction.changes.len() != 1 {
            return None;
        }
        let change = &transaction.changes[0];
        let kind = match change {
            EditorChange::Insert { .. } => EditorAnimationKind::Insert,
            EditorChange::Delete { .. } => EditorAnimationKind::Delete,
        };
        let inserted_range = match change {
            EditorChange::Insert { index, text } => {
                Utf8ByteRange::from_values(index.value(), index.value() + text.len())
            }
            EditorChange::Delete { .. } => None,
        };
        let deleted_range = match change {
            EditorChange::Insert { .. } => None,
            EditorChange::Delete { index, text } => {
                Utf8ByteRange::from_values(index.value(), index.value() + text.len())
            }
        };

        let text = change.text();
        let cluster_count = count_grapheme_clusters(text);
        let contains_newline = text.contains('\n');
        let contains_complex_grapheme = text_contains_complex_grapheme(text);

        // choose_animation_mode — 根据 cause 传入系统状态
        let is_loading = transaction.cause == EditorTransactionCause::Load;
        let is_applying_format = transaction.cause == EditorTransactionCause::Format;
        let animation_mode = choose_animation_mode(
            cluster_count,
            contains_newline,
            contains_complex_grapheme,
            false, // is_scrolling
            is_loading,
            is_applying_format,
            false, // is_applying_settings
            true,  // animation_enabled
        );

        // 如果是 Insert，计算 cluster_rects 和 cluster_runs
        let (cluster_rects, cluster_runs) = match change {
            EditorChange::Insert { index, text: _ } => {
                let rects = split_text_into_clusters(text, index.value());
                let runs = split_text_into_runs(text, index.value());
                (Some(rects), Some(runs))
            }
            EditorChange::Delete { .. } => (None, None),
        };

        // 构建 hidden_visual_ranges
        let hidden_visual_ranges = match inserted_range {
            Some(range) => vec![HiddenVisualRange {
                id: self.take_animation_id(),
                kind: animation_mode,
                range,
                old_rect: None,
                new_rect: None,
                line_index: 0,
                payload_ref: None,
            }],
            None => Vec::new(),
        };

        Some(EditorVisualTransaction {
            id: self.take_animation_id(),
            kind,
            cause: transaction.cause,
            old_text: transaction.old_text.clone(),
            new_text: transaction.new_text.clone(),
            old_selection: transaction.old_selection,
            new_selection: transaction.new_selection,
            inserted_range,
            deleted_range,
            deleted_glyph_rects: None,
            insert_glyph_rects: None,
            reflow_glyph_rects: None,
            animation_mode,
            cluster_rects,
            cluster_runs,
            hidden_visual_ranges,
            old_cursor_rect: None,
            new_cursor_rect: None,
            duration_ms: self.animation_duration_ms,
            coordinate_mode: VisualCoordinateMode::Baseline,
        })
    }

    /// #516: 创建 CursorOnly 事务 — 仅光标移动，无正文变更。
    ///
    /// 普通光标移动也必须创建 CursorOnly 事务并由 Renderer 队列驱动，
    /// 不允许光标拥有独立位移动画时间源。
    pub fn cursor_only_transaction(
        &mut self,
        text: &str,
        old_cursor_index: usize,
        new_cursor_index: usize,
    ) -> Option<EditorVisualTransaction> {
        if old_cursor_index == new_cursor_index {
            return None;
        }
        let old_sel = EditorSelection::collapsed(text, old_cursor_index);
        let new_sel = EditorSelection::collapsed(text, new_cursor_index);
        Some(EditorVisualTransaction {
            id: self.take_animation_id(),
            kind: EditorAnimationKind::Cursor,
            cause: EditorTransactionCause::Programmatic,
            old_text: text.to_string(),
            new_text: text.to_string(),
            old_selection: old_sel,
            new_selection: new_sel,
            inserted_range: None,
            deleted_range: None,
            deleted_glyph_rects: None,
            insert_glyph_rects: None,
            reflow_glyph_rects: None,
            animation_mode: AnimationMode::GlyphAnimation,
            cluster_rects: None,
            cluster_runs: None,
            hidden_visual_ranges: Vec::new(),
            old_cursor_rect: None,
            new_cursor_rect: None,
            duration_ms: self.animation_duration_ms,
            coordinate_mode: VisualCoordinateMode::Baseline,
        })
    }
}

/// #606: Composition 操作类型 — 三种 composition 操作共用同一视觉分类入口。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompositionOperationKind {
    /// setComposingText 触发的预输入更新
    Update,
    /// commitText 触发的预输入提交
    Commit,
    /// 取消预输入
    Cancel,
}

/// #606: Composition 视觉分类结果 — 平台无关的视觉事务语义。
///
/// 由 `classify_composition_visual` 计算，三种 composition 操作（update/commit/cancel）
/// 共用此结果，不再各自手写另一套视觉分类。
#[derive(Debug, Clone, PartialEq)]
pub struct CompositionVisualClassification {
    pub old_affected_byte_ranges: Vec<Utf8ByteRange>,
    pub new_affected_byte_ranges: Vec<Utf8ByteRange>,
    pub animation_mode: AnimationMode,
    pub is_visual_same: bool,
    pub visual_class_kinds: Vec<VisualClassKind>,
}

/// #606: 统一的 composition 视觉分类入口 — 三种 composition 操作共用。
///
/// 把 `apply_update_composition`、`apply_finish_composition`、`apply_cancel_composition`
/// 中各自手写的视觉分类逻辑统一收到此函数，通过 `classify_visual_diff` 和
/// `choose_animation_mode` 确定，不再各自手写另一套分类。
#[allow(clippy::too_many_arguments)]
pub fn classify_composition_visual(
    old_visual_text: &str,
    new_visual_text: &str,
    replace_start: usize,
    replace_end_exclusive: usize,
    operation_kind: CompositionOperationKind,
    animation_enabled: bool,
) -> CompositionVisualClassification {
    let visual_class_kinds = classify_visual_diff(old_visual_text, new_visual_text);
    let is_visual_same = visual_class_kinds.is_empty();

    let (old_affected, new_affected) = compute_composition_affected_ranges(
        old_visual_text,
        new_visual_text,
        replace_start,
        replace_end_exclusive,
        operation_kind,
    );

    let animation_mode = compute_composition_animation_mode(
        old_visual_text,
        new_visual_text,
        operation_kind,
        animation_enabled,
        &old_affected,
    );

    CompositionVisualClassification {
        old_affected_byte_ranges: old_affected,
        new_affected_byte_ranges: new_affected,
        animation_mode,
        is_visual_same,
        visual_class_kinds,
    }
}

/// #606: 计算 composition 操作的受影响 byte ranges。
fn compute_composition_affected_ranges(
    old_visual_text: &str,
    new_visual_text: &str,
    replace_start: usize,
    replace_end_exclusive: usize,
    operation_kind: CompositionOperationKind,
) -> (Vec<Utf8ByteRange>, Vec<Utf8ByteRange>) {
    match operation_kind {
        CompositionOperationKind::Update => {
            let old_affected = if old_visual_text.is_empty() {
                Vec::new()
            } else {
                vec![Utf8ByteRange::from_start_len(
                    replace_start,
                    old_visual_text.len(),
                )]
            };
            let new_affected = if new_visual_text.is_empty() {
                Vec::new()
            } else {
                vec![Utf8ByteRange::from_start_len(
                    replace_start,
                    new_visual_text.len(),
                )]
            };
            (old_affected, new_affected)
        }
        CompositionOperationKind::Commit => {
            let range = if new_visual_text.is_empty() {
                Vec::new()
            } else {
                vec![Utf8ByteRange::from_start_len(
                    replace_start,
                    new_visual_text.len(),
                )]
            };
            (range.clone(), range)
        }
        CompositionOperationKind::Cancel => {
            let old_affected = if !old_visual_text.is_empty() {
                vec![Utf8ByteRange::from_start_len(
                    replace_start,
                    old_visual_text.len(),
                )]
            } else if replace_start != replace_end_exclusive {
                vec![Utf8ByteRange::from_ordered(
                    replace_start,
                    replace_end_exclusive,
                )]
            } else {
                Vec::new()
            };
            (old_affected, Vec::new())
        }
    }
}

/// #606: 统一计算 composition 操作的动画模式。
fn compute_composition_animation_mode(
    old_visual_text: &str,
    new_visual_text: &str,
    operation_kind: CompositionOperationKind,
    animation_enabled: bool,
    old_affected: &[Utf8ByteRange],
) -> AnimationMode {
    if !animation_enabled {
        return AnimationMode::SystemSuppressed;
    }

    if operation_kind == CompositionOperationKind::Cancel && old_affected.is_empty() {
        return AnimationMode::SystemSuppressed;
    }

    let changed_text = if new_visual_text.len() >= old_visual_text.len() {
        new_visual_text
    } else {
        old_visual_text
    };

    let cluster_count = count_grapheme_clusters(changed_text);
    let contains_newline = changed_text.contains("\n");
    let contains_complex = text_contains_complex_grapheme(changed_text);

    choose_animation_mode(
        cluster_count,
        contains_newline,
        contains_complex,
        false,
        false,
        false,
        false,
        animation_enabled,
    )
}

/// #516: 视觉对象分类器 — 通过 old/new 文本差异分类。
///
/// 所有 old/new revision 比较都通过此函数分类，不按场景写特例。
/// 中间插入、换行、段落合并、删除回流、预输入更新和候选转换
/// 全部使用同一分类器。
///
/// 分类规则：
/// - 相同位置文本和位置都相同 → Static
/// - 仅 new 存在 → Insert
/// - 仅 old 存在 → Delete
/// - 文本可映射但可能有 shaping 改变 → Crossfade（保守策略）
/// - 文本相同但位置变化 → Move
///
/// 注：精确的 shaping identity 比较需要平台端提供 shaping fingerprint，
/// Core 层在文本内容相同时保守返回 Crossfade。
/// 平台端可利用 shaping_identity 做更精确的分类。
pub fn classify_visual_diff(old_text: &str, new_text: &str) -> Vec<VisualClassKind> {
    if old_text == new_text {
        return Vec::new();
    }
    if old_text.is_empty() && !new_text.is_empty() {
        return vec![VisualClassKind::Insert];
    }
    if !old_text.is_empty() && new_text.is_empty() {
        return vec![VisualClassKind::Delete];
    }

    let prefix = common_prefix_byte_len(old_text, new_text);
    let suffix = common_suffix_byte_len(old_text, new_text, prefix);
    let old_end = old_text.len().saturating_sub(suffix);
    let new_end = new_text.len().saturating_sub(suffix);

    let mut kinds = Vec::new();

    // 前缀相同部分 → Static
    if prefix > 0 {
        kinds.push(VisualClassKind::Static);
    }

    // 中间差异部分
    let removed = &old_text[prefix..old_end];
    let inserted = &new_text[prefix..new_end];

    if !removed.is_empty() && !inserted.is_empty() {
        // 替换：old 文本淡出 + new 文本淡入
        kinds.push(VisualClassKind::Crossfade);
    } else if !removed.is_empty() {
        kinds.push(VisualClassKind::Delete);
    } else if !inserted.is_empty() {
        kinds.push(VisualClassKind::Insert);
    }

    // 后缀相同部分 → Static 或 Move（位置可能变化）
    if suffix > 0 {
        // 如果有插入/删除，后缀文字位置会变化
        if !removed.is_empty() || !inserted.is_empty() {
            kinds.push(VisualClassKind::Move);
        } else {
            kinds.push(VisualClassKind::Static);
        }
    }

    kinds
}

/// #516: 统一 rebase — 新事务与旧事务冲突时的处理。
///
/// rebase 必须覆盖四种事务（BodyEdit、CompositionUpdate、
/// CompositionCommitOrCancel、CursorOnly），不只覆盖 Insert。
///
/// 新事务入队前：
/// 1. 根据视觉区域、revision 和 byte/UTF-16 映射查找冲突事务
/// 2. 读取旧事务当前 progress
/// 3. 将当前帧作为新事务 old state
/// 4. 取消旧事务，但不能提前释放已转移资源
/// 5. 启动新事务
///
/// 冲突判断不能只看 AnimatedSlice：CursorOnly、纯 Decoration、
/// 视觉文字相同的 CompositionCommit 也必须能通过 revision/affected range 参与替换。
///
/// #606: rebase slice 角色兼容性 — 平台无关的唯一事实来源，平台端不再自己判断。
///
/// Move/Insert/CrossfadeNew 互相兼容（都是“新出现的文字”动画）；
/// Delete/CrossfadeOld 互相兼容（都是“消失的文字”动画）；
/// 其余组合不兼容（Insert 与 Delete 不能接续，Move 与 CrossfadeOld 不能接续）。
fn compatible_rebase_roles(new_role: AnimatedSliceRole, old_role: AnimatedSliceRole) -> bool {
    use AnimatedSliceRole::*;
    matches!(
        (new_role, old_role),
        (Move, Move)
            | (Move, Insert)
            | (Move, CrossfadeNew)
            | (Insert, Move)
            | (Insert, Insert)
            | (Insert, CrossfadeNew)
            | (CrossfadeNew, CrossfadeNew)
            | (CrossfadeNew, Move)
            | (CrossfadeNew, Insert)
            | (Delete, Delete)
            | (Delete, CrossfadeOld)
            | (CrossfadeOld, CrossfadeOld)
            | (CrossfadeOld, Delete)
    )
}

/// #606: rebase slice 匹配输入 — 旧/新事务 slice 的角色与 UTF-8 byte range。
///
/// 平台无关的唯一事实来源，Android `RebasePlanner` 不再自己匹配，
/// 直接消费 `compute_rebase_slice_mappings` 的结果。
#[derive(Debug, Clone, Copy)]
pub struct SliceMatchInput<'a> {
    /// 旧事务各 slice 的角色
    pub old_slice_roles: &'a [AnimatedSliceRole],
    /// 旧事务各 slice 的 UTF-8 byte range
    pub old_slice_byte_ranges: &'a [(usize, usize)],
    /// 新事务各 slice 的角色
    pub new_slice_roles: &'a [AnimatedSliceRole],
    /// 新事务各 slice 的 UTF-8 byte range
    pub new_slice_byte_ranges: &'a [(usize, usize)],
    /// 旧正文 → 新正文偏移映射（可能为 None）
    pub offset_map: Option<&'a OffsetMap>,
}

/// 尝试为单个旧 slice 找到匹配的新 slice。
///
/// 返回 `(new_slice_index, reason)`；无匹配返回 `None`。
/// 匹配规则（按优先级）：
/// 1. 旧/新 slice 的 byte range 完全相同 + 角色兼容 → `SameByteRange`
/// 2. 旧 slice range 经 `offset_map`（旧正文 → 新正文）映射后与新 slice range
///    相等 + 角色兼容 → `OffsetMapMatched`（旧/新事务正文坐标不同但指向同一
///    逻辑对象，例如前一个事务的 Move slice 在新事务正文中整体移位）
///
/// 匹配依据只使用 byte range/OffsetMap/角色兼容（平台无关），不使用像素坐标。
fn try_match_slice(
    old_role: AnimatedSliceRole,
    (old_start, old_end): (usize, usize),
    new_slice_roles: &[AnimatedSliceRole],
    new_slice_byte_ranges: &[(usize, usize)],
    used_new: &std::collections::HashSet<usize>,
    offset_map: Option<&OffsetMap>,
) -> Option<(usize, RebaseReason)> {
    for (new_idx, (new_role, &(new_start, new_end))) in new_slice_roles
        .iter()
        .zip(new_slice_byte_ranges.iter())
        .enumerate()
    {
        if used_new.contains(&new_idx) || !compatible_rebase_roles(*new_role, old_role) {
            continue;
        }
        if old_start == new_start && old_end == new_end {
            return Some((new_idx, RebaseReason::SameByteRange));
        }
        // #606: 旧正文坐标与新正文坐标不同但 OffsetMap 可映射 → 同一逻辑对象。
        let Some(map) = offset_map else {
            continue;
        };
        if let Some((mapped_start, mapped_end)) = map.map_old_range_to_new(old_start, old_end) {
            if mapped_start == new_start && mapped_end == new_end {
                return Some((new_idx, RebaseReason::OffsetMapMatched));
            }
        }
    }
    None
}

/// #639 评论 5420317382：判断旧 slice 角色是否属于"当前屏幕上已经可见的新出现文字"。
///
/// 这些角色在遇到新 `CrossfadeOld + CrossfadeNew` pair 时，应优先映射到
/// **CrossfadeOld**（从当前位置退场），而非 CrossfadeNew（新位置淡入）。
/// 否则旧 Move 的 `currentAlpha`（通常 == 1）会被填给新 CrossfadeNew 的
/// `startAlpha`，新位置字直接全亮，同时配对 CrossfadeOld 在旧位置继续淡出
/// → 闪一下/跳一下。
fn is_emergence_role(role: AnimatedSliceRole) -> bool {
    matches!(
        role,
        AnimatedSliceRole::Move | AnimatedSliceRole::Insert | AnimatedSliceRole::CrossfadeNew
    )
}

/// #639 评论 5421085782：对新事务中的 `CrossfadeOld + CrossfadeNew` pair 建索引。
///
/// 返回 `old_side_byte_range → (crossfade_old_index, crossfade_new_index)` 的映射，
/// 用于优先把旧 Move/Insert/CrossfadeNew 映射到 CrossfadeOld。**索引键始终是
/// CrossfadeOld 的 old-side byte range**（CrossfadeOld.range 本身就是 old 文档坐标），
/// 这样 `compute_rebase_slice_mappings` 的旧活动 slice（也在 old 文档坐标）一次
/// 查询即可命中，不需要再做二次 OffsetMap 映射。
///
/// 配对规则：对每个 CrossfadeOld，先用 `offset_map.map_old_range_to_new(old_range)`
/// 把 old-side range 映射到 new-side range，再按这个 new-side range 在
/// `new_by_range` 里找 CrossfadeNew。若 `offset_map` 为 `None`、映射返回 `None`
/// 或映射后范围与 CrossfadeOld.range 相同（映射不变），则 fallback 按同 range
/// 在 `new_by_range` 里找。
///
/// 这修复了 #639 评论 5421085782 问题1：插字/回车时保留字符的 old range
/// （例如 [30,33)）被 OffsetMap 平移到 new range（例如 [33,36)），CrossfadeOld.range
/// （old 坐标）与 CrossfadeNew.range（new 坐标）不同，旧实现要求两者相同才能配对，
/// 导致 pair 建不出来，旧 Move 掉回 `try_match_slice` 接到 CrossfadeNew，新位置
/// 突然全亮。
///
/// 若同一 byte range 出现多个 CrossfadeOld 或 CrossfadeNew，取第一个（与
/// `try_match_slice` 的"第一个兼容"语义一致）。
fn build_crossfade_pair_index(
    new_slice_roles: &[AnimatedSliceRole],
    new_slice_byte_ranges: &[(usize, usize)],
    offset_map: Option<&OffsetMap>,
) -> std::collections::HashMap<(usize, usize), (usize, usize)> {
    use AnimatedSliceRole::*;
    let mut old_by_range: std::collections::HashMap<(usize, usize), usize> =
        std::collections::HashMap::new();
    let mut new_by_range: std::collections::HashMap<(usize, usize), usize> =
        std::collections::HashMap::new();
    for (idx, (role, &range)) in new_slice_roles
        .iter()
        .zip(new_slice_byte_ranges.iter())
        .enumerate()
    {
        match *role {
            CrossfadeOld => {
                old_by_range.entry(range).or_insert(idx);
            }
            CrossfadeNew => {
                new_by_range.entry(range).or_insert(idx);
            }
            _ => {}
        }
    }
    let mut pairs = std::collections::HashMap::new();
    for (&old_range, &old_idx) in old_by_range.iter() {
        // #639 评论 5421085782：CrossfadeOld.range 是 old 文档坐标，
        // CrossfadeNew.range 是 new 文档坐标。用 OffsetMap 把 old-side range
        // 映射到 new-side range 后再找 CrossfadeNew。映射不变/无 OffsetMap
        // 时 fallback 按同 range（保留旧实现语义作为子集）。
        let new_lookup_range = match offset_map {
            Some(map) => map
                .map_old_range_to_new(old_range.0, old_range.1)
                .unwrap_or(old_range),
            None => old_range,
        };
        if let Some(&new_idx) = new_by_range.get(&new_lookup_range) {
            pairs.insert(old_range, (old_idx, new_idx));
        }
    }
    pairs
}

/// #606: 计算旧事务逻辑 slice → 新事务逻辑 slice 的对应关系。
///
/// 平台无关的唯一事实来源 — Android `RebasePlanner` 不再自己匹配，
/// 直接消费此结果。
///
/// 匹配规则（按优先级）：
/// 1. #639 评论 5421085782：旧 Move/Insert/CrossfadeNew 是"当前屏幕上已经可见
///    的新出现文字"。若新事务存在 `CrossfadeOld + CrossfadeNew` pair，且 pair
///    索引键（CrossfadeOld 的 old-side range）与旧 slice 的 byte range 相同，
///    优先映射到 **CrossfadeOld**（从当前位置退场）；配对 CrossfadeNew 不接旧
///    状态，保持新规划的 `startAlpha = 0` 在新 Layout 自己淡入。pair 索引键
///    已是 old-side 坐标，旧 slice 也在 old 坐标，一次查询即可命中，不需要
///    二次 OffsetMap 映射。
/// 2. 没有 Crossfade pair 时，走 [try_match_slice] 的现有 Move→Move、
///    Insert→Insert/CrossfadeNew 等普通 continuation 规则。
///
/// 每个新 slice 至多被一个旧 slice 匹配（`used_new` 去重），
/// 避免多旧 slice 接续同一新 slice 造成 progress 抢占。
/// 匹配依据只使用 byte range/OffsetMap/角色兼容（平台无关），不使用像素坐标。
pub fn compute_rebase_slice_mappings(input: SliceMatchInput) -> Vec<RebaseSliceMapping> {
    let mut mappings = Vec::new();
    let mut used_new = std::collections::HashSet::new();
    let crossfade_pairs = build_crossfade_pair_index(
        input.new_slice_roles,
        input.new_slice_byte_ranges,
        input.offset_map,
    );
    for (old_idx, (old_role, &(old_start, old_end))) in input
        .old_slice_roles
        .iter()
        .zip(input.old_slice_byte_ranges.iter())
        .enumerate()
    {
        // #639 评论 5421085782：优先把旧 emergence role 映射到 Crossfade pair 的
        // CrossfadeOld，避免旧 Move 的 currentAlpha 被填给新 CrossfadeNew。
        // pair 索引键已是 CrossfadeOld 的 old-side range，旧 slice 也在 old 坐标，
        // 一次查询即可命中。
        let mut matched: Option<(usize, RebaseReason)> = None;
        if is_emergence_role(*old_role) {
            // #639 评论 5421085782：pair 索引键已是 CrossfadeOld 的 old-side range，
            // 旧 slice 也在 old 坐标，一次查询即可命中。filter 跳过已被其他旧 slice
            // 占用的 pair（used_new 去重），避免多旧 slice 接续同一新 slice。
            matched = crossfade_pairs
                .get(&(old_start, old_end))
                .filter(|&&(idx, _)| !used_new.contains(&idx))
                .map(|&(idx, _)| (idx, RebaseReason::SameByteRange));
        }
        // 没有 Crossfade pair 时，走现有普通 continuation 规则
        if matched.is_none() {
            matched = try_match_slice(
                *old_role,
                (old_start, old_end),
                input.new_slice_roles,
                input.new_slice_byte_ranges,
                &used_new,
                input.offset_map,
            );
        }
        if let Some((new_idx, reason)) = matched {
            mappings.push(RebaseSliceMapping {
                old_slice_index: old_idx,
                new_slice_index: new_idx,
                continuation: RebaseContinuation::Continue,
                reason,
            });
            used_new.insert(new_idx);
        }
    }
    mappings
}

/// #516/#606: 统一 rebase — 新事务与旧事务冲突时的处理。
///
/// rebase 必须覆盖四种事务（BodyEdit、CompositionUpdate、
/// CompositionCommitOrCancel、CursorOnly），不只覆盖 Insert。
///
/// 新事务入队前：
/// 1. 根据视觉区域、revision 和 byte/UTF-16 映射查找冲突事务
/// 2. 读取旧事务当前 progress
/// 3. 将当前帧作为新事务 old state
/// 4. 取消旧事务，但不能提前释放已转移资源
/// 5. 启动新事务
///
/// #606: 同时计算旧→新逻辑 slice 对应关系（`slice_mappings`），
/// 平台端不再自己匹配。
pub fn compute_rebase(
    cancelled_transaction_id: u64,
    old_progress: f64,
    old_frame_snapshot: Option<RebaseFrameSnapshot>,
    input: SliceMatchInput,
) -> TransactionRebase {
    let slice_mappings = compute_rebase_slice_mappings(input);
    TransactionRebase {
        cancelled_transaction_id,
        old_progress,
        old_frame_snapshot,
        slice_mappings,
    }
}

/// #516: 检查两个事务是否在视觉区域上冲突。
///
/// 用于决定是否需要 rebase。冲突条件：
/// - 同一 unified_kind 的连续事务
/// - 视觉区域有重叠
/// - CursorOnly 与任何影响光标位置的事务冲突
pub fn transactions_overlap(
    old_kind: UnifiedTransactionKind,
    old_affected_range: (usize, usize),
    new_kind: UnifiedTransactionKind,
    new_affected_range: (usize, usize),
) -> bool {
    let (old_start, old_end) = old_affected_range;
    let (new_start, new_end) = new_affected_range;

    // CursorOnly 与任何影响光标的事务冲突
    if matches!(old_kind, UnifiedTransactionKind::CursorOnly)
        || matches!(new_kind, UnifiedTransactionKind::CursorOnly)
    {
        return true;
    }

    // 视觉区域重叠
    old_start < new_end && new_start < old_end
}

/// 基于最长公共前缀/后缀的纯文本差异算法。
///
/// 返回的 `EditorChange::Insert/Delete` 的 `index` 均为 UTF-8 byte offset，
/// 基于 `old_text` 的坐标系。替换操作分解为 Delete + Insert（同一 index）。
/// 空输入或完全相同时返回空 Vec。
pub fn diff_plain_text(old_text: &str, new_text: &str) -> Vec<EditorChange> {
    if old_text == new_text {
        return Vec::new();
    }

    let prefix = common_prefix_byte_len(old_text, new_text);
    let suffix = common_suffix_byte_len(old_text, new_text, prefix);
    let old_end = old_text.len() - suffix;
    let new_end = new_text.len() - suffix;
    let removed = &old_text[prefix..old_end];
    let inserted = &new_text[prefix..new_end];

    let mut changes = Vec::new();
    if !removed.is_empty() {
        changes.push(EditorChange::Delete {
            index: Utf8ByteOffset::unchecked(prefix),
            text: removed.to_string(),
        });
    }
    if !inserted.is_empty() {
        changes.push(EditorChange::Insert {
            index: Utf8ByteOffset::unchecked(prefix),
            text: inserted.to_string(),
        });
    }
    changes
}

/// 判断编辑变更是否应产生动画。
///
/// 系统状态（Load/Format/Programmatic）和 IME 预输入阶段（ImeComposition）
/// 不进吞吐动画——预输入有自己的 CompositionUpdate 视觉层。
/// IME commit 走 TypingCommit cause，已允许动画。
/// 多变更（changes.len() != 1）和空文本变更不进动画。
/// 具体动画模式由 `choose_animation_mode` 决定。
fn should_animate_changes(
    changes: &[EditorChange],
    cause: EditorTransactionCause,
    _max_animated_chars: usize,
) -> bool {
    // 系统状态和 preedit 不进动画
    // ImeComposition 是 preedit 阶段，有自己的视觉层，不需要吞吐动画
    // IME commit 走 TypingCommit cause，已经允许动画
    if matches!(
        cause,
        EditorTransactionCause::Load
            | EditorTransactionCause::Format
            | EditorTransactionCause::Programmatic
            | EditorTransactionCause::ImeComposition
    ) {
        return false;
    }
    if changes.len() != 1 {
        return false;
    }
    let text = changes[0].text();
    // 不再限制换行和字符数量 — 由 choose_animation_mode 决定具体动画模式
    !text.is_empty()
}

/// 统一动画模式选择函数 — 替代旧的 should_create_text_animation。
///
/// 输入：文本特征 + 系统状态
/// 输出：AnimationMode — 平台层据此决定如何渲染动画
///
/// 规则（按优先级）：
/// 1. 系统抑制条件（动画关闭/滚动/加载/格式化/设置变化）→ SystemSuppressed
/// 2. glyph 为空 → SystemSuppressed（无内容可动画）
/// 3. 包含换行 → LineReflowAnimation（换行必须做行级 reflow，不许只动光标）
/// 4. 包含复杂 grapheme → ClusterAnimation（整组动画，不跳过）
/// 5. cluster 数量 1–8 → GlyphAnimation（逐 cluster 动画）
/// 6. cluster 数量 9–40 → RunAnimation（按 word/run/chunk 分组动画）
/// 7. cluster 数量 > 40 → RunAnimation（SnapshotAnimation unavailable，无 snapshot renderer）
#[allow(clippy::too_many_arguments)]
pub fn choose_animation_mode(
    cluster_count: usize,
    contains_newline: bool,
    contains_complex_grapheme: bool,
    is_scrolling: bool,
    is_loading: bool,
    is_applying_format: bool,
    is_applying_settings: bool,
    animation_enabled: bool,
) -> AnimationMode {
    // 1. 系统抑制条件
    if !animation_enabled
        || is_scrolling
        || is_loading
        || is_applying_format
        || is_applying_settings
    {
        return AnimationMode::SystemSuppressed;
    }
    // 2. 无内容可动画
    if cluster_count == 0 {
        return AnimationMode::SystemSuppressed;
    }
    // 3. 包含换行 → 行级 reflow
    if contains_newline {
        return AnimationMode::LineReflowAnimation;
    }
    // 4. 包含复杂 grapheme → 整组动画
    if contains_complex_grapheme {
        return AnimationMode::ClusterAnimation;
    }
    // 5–7. 按 cluster 数量分级
    // NOTE: SnapshotAnimation is unavailable (no snapshot renderer exists on any platform).
    // >40 cluster edits use RunAnimation instead. SnapshotAnimation enum variant is retained
    // for forward compatibility but must never be returned by this function.
    if cluster_count <= 8 {
        AnimationMode::GlyphAnimation
    } else {
        AnimationMode::RunAnimation
    }
}

/// 计算文本的 grapheme cluster 数量
pub fn count_grapheme_clusters(text: &str) -> usize {
    use unicode_segmentation::UnicodeSegmentation;
    text.graphemes(true).count()
}

/// 检测文本是否包含复杂 grapheme（emoji/ZWJ/组合字符等）
pub fn text_contains_complex_grapheme(text: &str) -> bool {
    use unicode_segmentation::UnicodeSegmentation;
    text.graphemes(true).any(|g| {
        g.chars()
            .any(|ch| is_complex_grapheme_code_point(ch as u32))
    })
}

/// 检测单个 code point 是否属于复杂 grapheme
pub fn is_complex_grapheme_code_point(cp: u32) -> bool {
    // Surrogate pairs: code point > 0xFFFF (non-BMP, e.g. emoji)
    if cp > 0xFFFF {
        return true;
    }
    // Zero Width Joiner
    if cp == 0x200D {
        return true;
    }
    // Variation selectors (FE00-FE0F, E0100-E01EF)
    if (0xFE00..=0xFE0F).contains(&cp) || (0xE0100..=0xE01EF).contains(&cp) {
        return true;
    }
    // Combining Diacritical Marks (0300-036F)
    if (0x0300..=0x036F).contains(&cp) {
        return true;
    }
    // Combining Diacritical Marks Extended (1AB0-1AFF)
    if (0x1AB0..=0x1AFF).contains(&cp) {
        return true;
    }
    // Combining Diacritical Marks Supplement (1DC0-1DFF)
    if (0x1DC0..=0x1DFF).contains(&cp) {
        return true;
    }
    // Combining Diacritical Marks for Symbols (20D0-20FF)
    if (0x20D0..=0x20FF).contains(&cp) {
        return true;
    }
    // Combining Half Marks (FE20-FE2F)
    if (0xFE20..=0xFE2F).contains(&cp) {
        return true;
    }
    // Emoji code points (common ranges)
    if (0x1F600..=0x1F64F).contains(&cp) {
        return true;
    }
    if (0x1F300..=0x1F5FF).contains(&cp) {
        return true;
    }
    if (0x1F680..=0x1F6FF).contains(&cp) {
        return true;
    }
    if (0x1F900..=0x1F9FF).contains(&cp) {
        return true;
    }
    // Regional Indicator (U+1F1E6-U+1F1FF)
    if (0x1F1E6..=0x1F1FF).contains(&cp) {
        return true;
    }
    false
}

/// 检测单个 code point 是否为组合字符（附加到前一个 base character）
pub fn is_combining_code_point(cp: u32) -> bool {
    // Combining Diacritical Marks (0300-036F)
    (0x0300..=0x036F).contains(&cp)
    // Combining Diacritical Marks Extended (1AB0-1AFF)
    || (0x1AB0..=0x1AFF).contains(&cp)
    // Combining Diacritical Marks Supplement (1DC0-1DFF)
    || (0x1DC0..=0x1DFF).contains(&cp)
    // Combining Diacritical Marks for Symbols (20D0-20FF)
    || (0x20D0..=0x20FF).contains(&cp)
    // Combining Half Marks (FE20-FE2F)
    || (0xFE20..=0xFE2F).contains(&cp)
    // Variation selectors
    || (0xFE00..=0xFE0F).contains(&cp)
    || (0xE0100..=0xE01EF).contains(&cp)
    // Zero Width Joiner
    || cp == 0x200D
}

/// 检测单个 code point 是否属于 CJK 字符
pub fn is_cjk_code_point(cp: u32) -> bool {
    (0x4E00..=0x9FFF).contains(&cp)   // CJK Unified Ideographs
    || (0x3400..=0x4DBF).contains(&cp) // CJK Unified Ideographs Extension A
    || (0x20000..=0x2A6DF).contains(&cp) // CJK Unified Ideographs Extension B
    || (0x2A700..=0x2B73F).contains(&cp) // CJK Unified Ideographs Extension C
    || (0x2B740..=0x2B81F).contains(&cp) // CJK Unified Ideographs Extension D
    || (0xF900..=0xFAFF).contains(&cp) // CJK Compatibility Ideographs
    || (0x2F800..=0x2FA1F).contains(&cp) // CJK Compatibility Ideographs Supplement
    || (0x3000..=0x303F).contains(&cp) // CJK Symbols and Punctuation
    || (0x3040..=0x309F).contains(&cp) // Hiragana
    || (0x30A0..=0x30FF).contains(&cp) // Katakana
    || (0xAC00..=0xD7AF).contains(&cp) // Hangul Syllables
}

/// 将文本按 run/word/chunk 分组，用于 RunAnimation。
/// 中文每 4–6 字一组，英文按 word 一组。
pub fn split_text_into_runs(text: &str, base_offset: usize) -> Vec<ClusterRun> {
    let mut runs = Vec::new();
    let mut current_text = String::new();
    let mut current_cluster_count = 0usize;
    let mut current_byte_start = base_offset;

    let chinese_chunk_size = 5; // 中文每 5 字一组

    for (byte_offset, ch) in text.char_indices() {
        let absolute_byte = base_offset + byte_offset;

        if ch.is_whitespace() {
            // 空格结束当前 run
            if !current_text.is_empty() {
                runs.push(ClusterRun {
                    byte_start: Utf8ByteOffset::unchecked(current_byte_start),
                    byte_end: Utf8ByteOffset::unchecked(absolute_byte),
                    text: current_text.clone(),
                    cluster_count: current_cluster_count,
                });
                current_text.clear();
                current_cluster_count = 0;
            }
            // 空格本身作为独立 run
            runs.push(ClusterRun {
                byte_start: Utf8ByteOffset::unchecked(absolute_byte),
                byte_end: Utf8ByteOffset::unchecked(absolute_byte + ch.len_utf8()),
                text: ch.to_string(),
                cluster_count: 1,
            });
            current_byte_start = absolute_byte + ch.len_utf8();
            continue;
        }

        let is_cjk = is_cjk_code_point(ch as u32);

        if current_text.is_empty() {
            current_byte_start = absolute_byte;
        }

        current_text.push(ch);
        current_cluster_count += 1;

        // CJK 字符达到 chunk 大小时结束 run
        if is_cjk && current_cluster_count >= chinese_chunk_size {
            runs.push(ClusterRun {
                byte_start: Utf8ByteOffset::unchecked(current_byte_start),
                byte_end: Utf8ByteOffset::unchecked(absolute_byte + ch.len_utf8()),
                text: current_text.clone(),
                cluster_count: current_cluster_count,
            });
            current_text.clear();
            current_cluster_count = 0;
            current_byte_start = absolute_byte + ch.len_utf8();
        }

        // 非 CJK 连续字符达到一定长度也结束 run
        if !is_cjk && current_cluster_count >= 8 {
            runs.push(ClusterRun {
                byte_start: Utf8ByteOffset::unchecked(current_byte_start),
                byte_end: Utf8ByteOffset::unchecked(absolute_byte + ch.len_utf8()),
                text: current_text.clone(),
                cluster_count: current_cluster_count,
            });
            current_text.clear();
            current_cluster_count = 0;
            current_byte_start = absolute_byte + ch.len_utf8();
        }
    }

    // 处理剩余
    if !current_text.is_empty() {
        runs.push(ClusterRun {
            byte_start: Utf8ByteOffset::unchecked(current_byte_start),
            byte_end: Utf8ByteOffset::unchecked(base_offset + text.len()),
            text: current_text,
            cluster_count: current_cluster_count,
        });
    }

    runs
}

/// 将文本按 grapheme cluster 分割，用于 ClusterAnimation。
/// 每个 cluster 记录 byte range 和是否复杂。
///
/// `base_offset` 为该文本在完整正文中的 UTF-8 byte 起始位置，
/// 所有 ClusterRect 的 byte_start/byte_end 均为绝对坐标（base_offset + 片内偏移）。
/// 指针算术 `grapheme.as_ptr() - text.as_ptr()` 仅在同一次 String 分配内有效，
/// 不适用于子串切片后的跨分配场景。
pub fn split_text_into_clusters(text: &str, base_offset: usize) -> Vec<ClusterRect> {
    use unicode_segmentation::UnicodeSegmentation;
    let mut clusters = Vec::new();
    for grapheme in text.graphemes(true) {
        let byte_start = base_offset + (grapheme.as_ptr() as usize - text.as_ptr() as usize);
        let byte_end = byte_start + grapheme.len();
        let is_complex = grapheme
            .chars()
            .any(|ch| is_complex_grapheme_code_point(ch as u32));
        clusters.push(ClusterRect {
            byte_start: Utf8ByteOffset::unchecked(byte_start),
            byte_end: Utf8ByteOffset::unchecked(byte_end),
            text: grapheme.to_string(),
            is_complex,
        });
    }
    clusters
}

/// 计算两个文本的公共前缀长度（UTF-8 byte 单位）。
///
/// 逐字符比较，返回第一个不同字符之前的 byte 长度。
/// 结果保证是合法 UTF-8 char boundary。
pub(crate) fn common_prefix_byte_len(old_text: &str, new_text: &str) -> usize {
    let mut prefix = 0;
    for ((old_index, old_char), (_, new_char)) in
        old_text.char_indices().zip(new_text.char_indices())
    {
        if old_char != new_char {
            break;
        }
        prefix = old_index + old_char.len_utf8();
    }
    prefix
}

/// 计算两个文本的公共后缀长度（UTF-8 byte 单位），排除前缀部分。
///
/// `prefix` 参数为 `common_prefix_byte_len` 的结果，避免前缀/后缀重叠。
/// 从尾部逐字符反向比较，返回公共后缀的 byte 长度。
/// 结果保证是合法 UTF-8 char boundary。
pub(crate) fn common_suffix_byte_len(old_text: &str, new_text: &str, prefix: usize) -> usize {
    let old_tail = &old_text[prefix..];
    let new_tail = &new_text[prefix..];
    let mut suffix = 0;
    for ((_, old_char), (_, new_char)) in old_tail
        .char_indices()
        .rev()
        .zip(new_tail.char_indices().rev())
    {
        if old_char != new_char {
            break;
        }
        suffix += old_char.len_utf8();
    }
    suffix
}
