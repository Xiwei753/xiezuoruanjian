use super::types::{
    AnimationMode, EditorAnimationKind, EditorChange, EditorSelection,
    EditorTransaction, EditorTransactionCause,
};
use super::visual::{
    ClusterRect, ClusterRun, EditorVisualTransaction,
    HiddenVisualRange, UnifiedTransactionKind, VisualClassKind, VisualCoordinateMode,
};
#[cfg(test)]
use super::visual::EditorAnimationEvent;
use super::composition::{
    CompositionCommitOrCancelTransaction, CompositionUpdateTransaction, CompositionVisualRevision,
};
use super::rebase::{RebaseFrameSnapshot, TransactionRebase};
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
            EditorChange::Insert { index, text } => Utf8ByteRange::from_values(index.value(), index.value() + text.len()),
            EditorChange::Delete { .. } => None,
        };
        let deleted_range = match change {
            EditorChange::Insert { .. } => None,
            EditorChange::Delete { index, text } => Utf8ByteRange::from_values(index.value(), index.value() + text.len()),
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

    /// #516/#517: 创建 CompositionUpdate 事务 — 预输入更新。
    ///
    /// 每次 setComposingText 触发。预输入文字必须真实推动后续正文、
    /// 触发换行和 reflow，不能在原正文上盖一段文字。
    ///
    /// composing 更新不会修改 committed text、Undo、保存和同步状态。
    ///
    /// #517: 支持从 previous visual revision 接续。
    /// 如果提供了 previous_revision，新 revision 从 previous 接续，
    /// 自动计算 OffsetMap，后续正文 cluster 保持身份并生成 Move。
    /// 如果没有提供，则从 committed 状态开始（首次预输入）。
    pub fn composition_update_transaction(
        &mut self,
        committed_text: &str,
        composition_replace_range: Option<(usize, usize)>,
        old_preedit_text: &str,
        new_preedit_text: &str,
    ) -> CompositionUpdateTransaction {
        let range = composition_replace_range.and_then(|(s, e)| Utf8ByteRange::from_values(s, e));
        let old_revision = CompositionVisualRevision::new(
            committed_text.to_string(),
            range,
            old_preedit_text.to_string(),
            Utf8ByteRange::from_values(0, committed_text.len()).unwrap_or_else(|| Utf8ByteRange::from_values(0, 0).unwrap()),
        );
        let new_revision = CompositionVisualRevision::new(
            committed_text.to_string(),
            range,
            new_preedit_text.to_string(),
            Utf8ByteRange::from_values(0, committed_text.len()).unwrap_or_else(|| Utf8ByteRange::from_values(0, 0).unwrap()),
        );
        let visual_class_kinds = classify_visual_diff(
            &old_revision.virtual_text,
            &new_revision.virtual_text,
        );
        CompositionUpdateTransaction {
            id: self.take_animation_id(),
            old_revision,
            new_revision,
            visual_class_kinds,
            duration_ms: self.animation_duration_ms,
        }
    }

    /// #517: 从 previous visual revision 创建 CompositionUpdate 事务。
    ///
    /// 更新链必须是：previous visual revision -> new visual revision，
    /// 而不是：committed revision -> 每一次新的 preedit。
    ///
    /// 此方法使用 CompositionVisualRevision::from_previous 自动计算 OffsetMap，
    /// 后续正文 cluster 通过 OffsetMap 保持身份并生成 Move。
    pub fn composition_update_from_previous(
        &mut self,
        previous_revision: &CompositionVisualRevision,
        new_preedit_text: &str,
        new_preedit_cursor_offset: usize,
    ) -> CompositionUpdateTransaction {
        let new_revision = CompositionVisualRevision::from_previous(
            previous_revision,
            new_preedit_text.to_string(),
            new_preedit_cursor_offset,
            previous_revision.affected_paragraph_range,
        );
        let visual_class_kinds = classify_visual_diff(
            &previous_revision.virtual_text,
            &new_revision.virtual_text,
        );
        CompositionUpdateTransaction {
            id: self.take_animation_id(),
            old_revision: previous_revision.clone(),
            new_revision,
            visual_class_kinds,
            duration_ms: self.animation_duration_ms,
        }
    }

    /// #516: 创建 CompositionCommitOrCancel 事务 — 预输入提交或取消。
    ///
    /// commitText: current CompositionVisualRevision → new committed VisualRevision
    /// cancel: current CompositionVisualRevision → original committed VisualRevision
    ///
    /// 视觉文字完全相同时，不重复播放吐字，只移除 underline、segment style
    /// 和 composing cursor，并完成 revision 所有权转移。
    /// 候选转换导致文字变化时，旧 preedit 执行 Delete/Crossfade，
    /// 新 committed 文字执行 Insert/Crossfade，后续正文执行 Move/Crossfade。
    pub fn composition_commit_or_cancel_transaction(
        &mut self,
        committed_text_before: &str,
        committed_text_after: &str,
        composition_revision: CompositionVisualRevision,
        is_commit: bool,
    ) -> CompositionCommitOrCancelTransaction {
        let visual_class_kinds = if is_commit {
            classify_visual_diff(
                &composition_revision.virtual_text,
                committed_text_after,
            )
        } else {
            classify_visual_diff(
                &composition_revision.virtual_text,
                committed_text_before,
            )
        };
        let is_visual_same = composition_revision.virtual_text == committed_text_after;
        CompositionCommitOrCancelTransaction {
            id: self.take_animation_id(),
            is_commit,
            is_visual_same,
            composition_revision,
            committed_text_after: committed_text_after.to_string(),
            visual_class_kinds,
            duration_ms: self.animation_duration_ms,
        }
    }
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
pub fn compute_rebase(
    cancelled_transaction_id: u64,
    old_progress: f64,
    old_frame_snapshot: Option<RebaseFrameSnapshot>,
) -> TransactionRebase {
    TransactionRebase {
        cancelled_transaction_id,
        old_progress,
        old_frame_snapshot,
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
    if !animation_enabled || is_scrolling || is_loading || is_applying_format || is_applying_settings {
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
    text.graphemes(true).any(|g| g.chars().any(|ch| is_complex_grapheme_code_point(ch as u32)))
}

/// 检测单个 code point 是否属于复杂 grapheme
pub fn is_complex_grapheme_code_point(cp: u32) -> bool {
    // Surrogate pairs: code point > 0xFFFF (non-BMP, e.g. emoji)
    if cp > 0xFFFF { return true; }
    // Zero Width Joiner
    if cp == 0x200D { return true; }
    // Variation selectors (FE00-FE0F, E0100-E01EF)
    if (0xFE00..=0xFE0F).contains(&cp) || (0xE0100..=0xE01EF).contains(&cp) { return true; }
    // Combining Diacritical Marks (0300-036F)
    if (0x0300..=0x036F).contains(&cp) { return true; }
    // Combining Diacritical Marks Extended (1AB0-1AFF)
    if (0x1AB0..=0x1AFF).contains(&cp) { return true; }
    // Combining Diacritical Marks Supplement (1DC0-1DFF)
    if (0x1DC0..=0x1DFF).contains(&cp) { return true; }
    // Combining Diacritical Marks for Symbols (20D0-20FF)
    if (0x20D0..=0x20FF).contains(&cp) { return true; }
    // Combining Half Marks (FE20-FE2F)
    if (0xFE20..=0xFE2F).contains(&cp) { return true; }
    // Emoji code points (common ranges)
    if (0x1F600..=0x1F64F).contains(&cp) { return true; }
    if (0x1F300..=0x1F5FF).contains(&cp) { return true; }
    if (0x1F680..=0x1F6FF).contains(&cp) { return true; }
    if (0x1F900..=0x1F9FF).contains(&cp) { return true; }
    // Regional Indicator (U+1F1E6-U+1F1FF)
    if (0x1F1E6..=0x1F1FF).contains(&cp) { return true; }
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
        let is_complex = grapheme.chars().any(|ch| is_complex_grapheme_code_point(ch as u32));
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

