use super::animation_coordinator::LinuxEditorAnimationCoordinator;
use super::buffer::{clamp_to_char_boundary, normalize_plain_text, EditorSnapshot};
use super::layout_revision::LayoutRevision;
use super::layout_snapshot::EditorLayoutSnapshot;
use super::line_snapshot_builder::LineSnapshotBuilder;
use super::texture_cache::TextureCache;
use super::transaction_key::VisualTransactionKey;
use super::PreeditAttribute;
use crate::editor::layout;
use crate::platform::linux_qt::LinuxQtClipboardFocusAdapter;
use writer_core::editor::CompositionSession;
use writer_core::editor::{
    CursorRect, EditorAnimationKind, EditorCommand, EditorCursor, EditorEditOutcome,
    EditorEditResult, EditorKernel, EditorRevision, EditorSelection, EditorTransactionCause,
    EditorVisualTransaction, PreeditVisualTransaction, Utf8ByteOffset, Utf8ByteRange,
};

/// Qt 侧已确认正文镜像 — 持有与 Rust EditorKernel revision 对应的纯文本快照。
///
/// 不变量：镜像的 revision 必须与 kernel 的 base_revision 匹配，
/// 否则 patch 被拒绝。所有修改必须通过 EditorKernel.apply() →
/// EditorEditResult → apply_edit_result() 增量同步。
///
/// 平台端不得维护第二份可独立编辑的正文真相（见 AGENTS.md）。
/// 不得先本地改 Buffer 再通知 Core——必须先调 Core，再按返回结果更新镜像。
///
/// cursor 和 selection_anchor 均为 UTF-8 byte offset（半开区间语义）。
/// selection_anchor 是选区锚点（非移动端），cursor 是光标（移动端/插入点）。
/// 当 anchor == cursor 时为折叠光标（无选中）。
pub(crate) struct CommittedTextMirror {
    text: String,
    revision: u64,
    cursor: usize,
    selection_anchor: usize,
}

impl CommittedTextMirror {
    pub fn new() -> Self {
        Self {
            text: String::new(),
            revision: 0,
            cursor: 0,
            selection_anchor: 0,
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn selection_anchor(&self) -> usize {
        self.selection_anchor
    }

    pub fn load_from_snapshot(
        &mut self,
        text: String,
        cursor: usize,
        revision: u64,
        anchor: usize,
    ) {
        self.text = text;
        self.cursor = clamp_to_char_boundary(&self.text, cursor);
        self.selection_anchor = clamp_to_char_boundary(&self.text, anchor);
        self.revision = revision;
    }

    /// 将 Core 返回的编辑结果增量应用到正文镜像。
    ///
    /// 错误恢复策略：
    /// - revision 不连续（patch.base_revision != mirror.revision）：说明中间有 patch 丢失，
    ///   返回错误，调用方必须从 kernel snapshot 完整重建镜像。
    /// - patch 范围越界或不在 char boundary 上：返回错误，调用方必须重建镜像。
    ///   这类错误理论上不应发生（Core 保证输出合法 range），若出现说明存在 bug。
    /// - 选区越界或不在 char boundary 上：返回错误，调用方必须重建镜像。
    ///
    /// 不变量：成功返回后，mirror 的 revision 与 kernel 的 new_revision 一致，
    /// 正文和选区与 kernel 状态同步。
    pub fn apply_edit_result(&mut self, result: &EditorEditResult) -> Result<(), String> {
        for patch in &result.display_patches {
            if patch.base_revision.value() != self.revision {
                return Err(format!(
                    "CommittedTextMirror revision discontinuity: expected {}, got {}. Must reload from kernel snapshot.",
                    self.revision, patch.base_revision.value()
                ));
            }
            let range = patch.replace_byte_range.to_std_range();
            let start = range.start;
            let end = range.end;
            if start > self.text.len() || end > self.text.len() {
                return Err(format!(
                    "CommittedTextMirror patch range out of bounds: [{}, {}) vs text len {}. Must reload from kernel snapshot.",
                    start, end, self.text.len()
                ));
            }
            if !self.text.is_char_boundary(start) || !self.text.is_char_boundary(end) {
                return Err(format!(
                    "CommittedTextMirror patch range not on char boundary: [{}, {}). Must reload from kernel snapshot.",
                    start, end
                ));
            }
            self.text.replace_range(start..end, &patch.inserted_text);
            self.revision = patch.new_revision.value();
        }
        let sel_range = result.new_selection_byte_range.to_std_range();
        let anchor = sel_range.start;
        let head = sel_range.end;
        if anchor > self.text.len() || head > self.text.len() {
            return Err(format!(
                "CommittedTextMirror selection out of bounds: ({}, {}) vs text len {}. Must reload from kernel snapshot.",
                anchor, head, self.text.len()
            ));
        }
        if !self.text.is_char_boundary(anchor) || !self.text.is_char_boundary(head) {
            return Err(format!(
                "CommittedTextMirror selection not on char boundary: ({}, {}). Must reload from kernel snapshot.",
                anchor, head
            ));
        }
        self.cursor = head;
        self.selection_anchor = anchor;
        Ok(())
    }
}

/// IME 组合输入状态 — 跟踪一次 composition 从 preedit 到 commit/cancel 的完整生命周期。
///
/// 生命周期：preedit 开始 → (多次 updatePreedit) → commit 或 cancel。
/// composition_session 由 EditorKernel 在 beginComposition 时创建，包含 replace range
/// 和 virtual text。commit 后 session 被清除，preedit 字段归零。
/// suppress_next_ime_commit 用于抑制 fcitx5 等输入法在 cancel 后自动发送的冗余 commit。
pub(crate) struct CompositionState {
    pub preedit_text: String,
    pub preedit_cursor: usize,
    pub preedit_attributes: Vec<PreeditAttribute>,
    pub preedit_old_text: String,
    pub composition_session: Option<CompositionSession>,
    pub preedit_visual_transaction: Option<PreeditVisualTransaction>,
    pub preedit_cursor_rect: Option<CursorRect>,
    pub pending_preedit_cursor_rect: Option<CursorRect>,
    pub suppress_next_ime_commit: bool,
}

/// IME commit 参数 — 描述一次 composition 上屏的替换范围和原因。
///
/// `session_replace_start`/`session_replace_end` 为 composition session 记录的
/// IME commit 结果 — 记录一次 composition 上屏前后的 byte range 映射，
/// 供动画协调器构建视觉事务。
///
/// 所有 byte range 均为 UTF-8 byte offset，半开区间 [start, end)。
/// `candidate_*` 指上屏文本在 committed 文本中的位置；
/// `committed_*` 指被替换的原 preedit 占位范围。
pub(crate) struct CompositionCommitResult {
    pub pending_preedit_cursor_rect: Option<CursorRect>,
    pub was_composing: bool,
    pub preedit_byte_start: usize,
    pub preedit_byte_end: usize,
    pub saved_virtual_text: String,
    pub session_replace_start: usize,
    pub session_replace_end: usize,
    pub candidate_byte_start: usize,
    pub candidate_byte_end: usize,
    pub committed_replace_start: usize,
    pub committed_replace_end: usize,
}

impl CompositionState {
    pub fn new() -> Self {
        Self {
            preedit_text: String::new(),
            preedit_cursor: 0,
            preedit_attributes: Vec::new(),
            preedit_old_text: String::new(),
            composition_session: None,
            preedit_visual_transaction: None,
            preedit_cursor_rect: None,
            pending_preedit_cursor_rect: None,
            suppress_next_ime_commit: false,
        }
    }

    pub fn is_composing(&self) -> bool {
        !self.preedit_text.is_empty() || self.composition_session.is_some()
    }

    pub fn clear(&mut self) {
        self.preedit_text.clear();
        self.preedit_cursor = 0;
        self.preedit_attributes.clear();
        self.preedit_old_text.clear();
        self.composition_session = None;
        self.preedit_visual_transaction = None;
        self.preedit_cursor_rect = None;
        self.pending_preedit_cursor_rect = None;
        self.suppress_next_ime_commit = false;
    }

    pub fn save_pending_preedit_cursor_rect(&mut self) {
        if !self.preedit_text.is_empty() && self.pending_preedit_cursor_rect.is_none() {
            self.pending_preedit_cursor_rect = self.preedit_cursor_rect.clone();
        }
    }

    pub fn take_pending_preedit_cursor_rect(&mut self) -> Option<CursorRect> {
        self.pending_preedit_cursor_rect.take()
    }

    pub fn clear_preedit_fields(&mut self) {
        self.preedit_text.clear();
        self.preedit_cursor = 0;
        self.preedit_attributes.clear();
        self.preedit_old_text.clear();
        self.preedit_visual_transaction = None;
        self.preedit_cursor_rect = None;
    }

    pub fn session_replace_range(&self, fallback_cursor: usize) -> (usize, usize) {
        self.composition_session
            .as_ref()
            .map(|s| (s.replace_start.value(), s.replace_end_exclusive.value()))
            .unwrap_or((fallback_cursor, fallback_cursor))
    }

    pub fn virtual_text(&self) -> String {
        self.composition_session
            .as_ref()
            .map(|s| s.virtual_text())
            .unwrap_or_default()
    }

    pub fn finish_session(&mut self) {
        self.composition_session = None;
    }
}

/// 视觉事务上下文 — 传递给布局引擎的渲染参数快照。
///
/// 每次布局重算时由平台端填充当前值。所有尺寸均为物理像素（已乘 dpr）。
/// `scroll_y` 为文档坐标系中的滚动偏移，不含 viewport 顶部 padding。
///
/// 坐标空间约定：
/// - 所有 x/y 坐标为文档坐标系（不含滚动偏移），布局引擎在渲染时减去 scroll_y
/// - bounding_width / font_pixel_size / padding / text_indent / line_spacing 均为物理像素
/// - dpr 用于逻辑像素到物理像素的转换，布局引擎内部统一使用物理像素
pub(crate) struct VisualTransactionContext {
    pub typing_animation_enabled: bool,
    pub is_scrolling: bool,
    pub is_loading: bool,
    pub is_applying_format: bool,
    pub is_applying_settings: bool,
    pub bounding_width: f64,
    pub font_pixel_size: f64,
    pub font_family: String,
    pub scroll_y: f64,
    pub viewport_height: f64,
    pub text_indent: f64,
    pub line_spacing: f64,
    pub padding: f64,
    pub text_color: String,
    pub dpr: f64,
}

/// Linux Qt 编辑器管线 — 连接 Core EditorKernel 与 Qt 渲染层。
///
/// 双修订模型：
/// - `text_revision`：文本内容变更时递增（insert/delete/undo/redo/load），
///   触发布局重算。
/// - `visual_revision`：任何需要重绘的变化时递增（含文本变更、光标移动、选区变化、
///   preedit 更新等），比 text_revision 更频繁。
///
/// 线程安全：此结构体仅在 GUI 线程使用，不得跨线程访问。
/// Qt 对象（QQuickItem/QSGNode）只能在主线程使用，后台线程只能发送强类型命令。
///
/// 所有 byte offset（cursor、selection_anchor、composition range）均为 UTF-8 byte offset。
/// Qt QChar index 和 QInputMethodEvent 的 UTF-16 code unit offset 只允许存在于
/// 平台适配层（platform_ime.rs），传入管线前必须转换为 UTF-8。
pub(crate) struct LinuxEditorPipeline {
    /// Core 编辑器内核——正文和业务唯一真相
    kernel: EditorKernel,
    /// Qt 侧已确认正文镜像——与 kernel revision 对应的纯文本快照
    mirror: CommittedTextMirror,
    /// IME 组合输入状态——跟踪 preedit 到 commit/cancel 的完整生命周期
    composition: CompositionState,
    /// 编辑引擎工厂——创建 EditorTransaction 和 EditorVisualTransaction
    engine: writer_core::editor::EditorEngine,
    /// 动画协调器——管理视觉事务队列和 Timeline
    animation_coordinator: LinuxEditorAnimationCoordinator,
    /// 纹理缓存——行快照到 QSGTexture 的映射
    texture_cache: TextureCache,
    /// 剪贴板适配器——处理复制/剪切/粘贴
    clipboard_adapter: LinuxQtClipboardFocusAdapter,
    /// 文本内容变更时递增——触发布局重算
    text_revision: u64,
    /// 任何需要重绘的变化时递增——比 text_revision 更频繁
    visual_revision: u64,
    typing_animation_duration_ms: u32,
    cursor_animation_duration_ms: u32,
    /// 当前布局快照——包含视觉行信息和 QChar 边界
    current_layout_snapshot: Option<EditorLayoutSnapshot>,
    /// 前一次布局快照——用于动画 old/new 对比
    previous_layout_snapshot: Option<EditorLayoutSnapshot>,
    previous_canonical_snapshot: Option<crate::editor::layout::CanonicalDocumentVisualSnapshot>,
    /// 布局修订——宽度/字号/字体/行距等变化时递增
    layout_revision: LayoutRevision,
    /// Issue #658 评论 5622829886 问题 1: record_visual_transaction 全篇排版 new text
    /// 后把 new prepared layout 存在此处，由 record_transaction 取出交给
    /// EditorLayout::promote_prepared_layout 提升为 current，
    /// 后续 EditorLayout::snapshot 不再重新排版同一 new text。
    pending_promoted_layout: Option<crate::editor::layout::PromotedLayout>,
}

impl LinuxEditorPipeline {
    pub fn new() -> Self {
        Self {
            kernel: EditorKernel::new(),
            mirror: CommittedTextMirror::new(),
            composition: CompositionState::new(),
            engine: writer_core::editor::EditorEngine::new(),
            animation_coordinator: LinuxEditorAnimationCoordinator::new(),
            texture_cache: TextureCache::new(),
            clipboard_adapter: LinuxQtClipboardFocusAdapter::new(),
            text_revision: 0,
            visual_revision: 0,
            typing_animation_duration_ms: 160,
            cursor_animation_duration_ms: 120,
            current_layout_snapshot: None,
            previous_layout_snapshot: None,
            previous_canonical_snapshot: None,
            layout_revision: LayoutRevision::initial(),
            pending_promoted_layout: None,
        }
    }

    pub fn swap_kernel(&mut self, new_kernel: EditorKernel) -> EditorKernel {
        let old = std::mem::replace(&mut self.kernel, new_kernel);
        let text = self.kernel.snapshot_text();
        let cursor = self.kernel.cursor();
        let revision = self.kernel.revision();
        let anchor = self.kernel.selection_anchor();
        self.mirror
            .load_from_snapshot(text, cursor, revision, anchor);
        self.text_revision = self.text_revision.wrapping_add(1);
        self.visual_revision = self.visual_revision.wrapping_add(1);
        old
    }

    pub fn mirror(&self) -> &CommittedTextMirror {
        &self.mirror
    }

    pub fn composition(&self) -> &CompositionState {
        &self.composition
    }

    pub fn composition_mut(&mut self) -> &mut CompositionState {
        &mut self.composition
    }

    pub fn engine(&self) -> &writer_core::editor::EditorEngine {
        &self.engine
    }

    pub fn engine_mut(&mut self) -> &mut writer_core::editor::EditorEngine {
        &mut self.engine
    }

    pub fn animation_coordinator(&self) -> &LinuxEditorAnimationCoordinator {
        &self.animation_coordinator
    }

    pub fn animation_coordinator_mut(&mut self) -> &mut LinuxEditorAnimationCoordinator {
        &mut self.animation_coordinator
    }

    pub fn texture_cache(&self) -> &TextureCache {
        &self.texture_cache
    }

    pub fn texture_cache_mut(&mut self) -> &mut TextureCache {
        &mut self.texture_cache
    }

    pub fn clipboard_adapter_mut(&mut self) -> &mut LinuxQtClipboardFocusAdapter {
        &mut self.clipboard_adapter
    }

    pub fn text_revision(&self) -> u64 {
        self.text_revision
    }

    pub fn visual_revision(&self) -> u64 {
        self.visual_revision
    }

    pub fn bump_visual_revision(&mut self) {
        self.visual_revision = self.visual_revision.wrapping_add(1);
    }

    pub fn bump_text_revision(&mut self) {
        self.text_revision = self.text_revision.wrapping_add(1);
    }

    pub fn set_typing_animation_duration_ms(&mut self, ms: u32) {
        self.typing_animation_duration_ms = ms;
        self.engine.set_animation_duration_ms(u64::from(ms));
        self.kernel.set_animation_duration_ms(u64::from(ms));
        self.animation_coordinator
            .set_typing_animation_duration_ms(ms);
    }

    pub fn set_cursor_animation_duration_ms(&mut self, ms: u32) {
        self.cursor_animation_duration_ms = ms;
        self.animation_coordinator
            .set_cursor_animation_duration_ms(ms);
    }

    pub fn load_text(&mut self, text: String, cursor: usize) -> bool {
        let normalized = normalize_plain_text(&text);
        let clamped_cursor = clamp_to_char_boundary(&normalized, cursor);
        match EditorKernel::with_text(normalized.clone(), clamped_cursor) {
            Ok(kernel) => {
                self.kernel = kernel;
                self.mirror.load_from_snapshot(
                    self.kernel.snapshot_text(),
                    self.kernel.cursor(),
                    self.kernel.revision(),
                    self.kernel.selection_anchor(),
                );
                self.composition.clear();
                self.animation_coordinator
                    .cancel_active_composition("load_text");
                true
            }
            Err(_) => false,
        }
    }

    pub fn insert_text(
        &mut self,
        byte_offset: usize,
        text: &str,
        cause: EditorTransactionCause,
    ) -> Option<EditorEditResult> {
        let command = EditorCommand::Insert {
            byte_offset: Utf8ByteOffset::clamp_rope(self.kernel.rope(), byte_offset),
            text: text.to_string(),
            cause,
            expected_revision: EditorRevision::new(self.mirror.revision()),
        };
        let outcome = self.kernel.apply(command);
        match outcome {
            EditorEditOutcome::Applied(result)
            | EditorEditOutcome::AppliedWithAdjustedSelection(result) => {
                if self.mirror.apply_edit_result(&result).is_err() {
                    self.mirror.load_from_snapshot(
                        self.kernel.snapshot_text(),
                        self.kernel.cursor(),
                        self.kernel.revision(),
                        self.kernel.selection_anchor(),
                    );
                }
                Some(result)
            }
            EditorEditOutcome::NoChange(result) => Some(result),
            EditorEditOutcome::StaleRevision(result) => {
                self.mirror.load_from_snapshot(
                    self.kernel.snapshot_text(),
                    self.kernel.cursor(),
                    self.kernel.revision(),
                    self.kernel.selection_anchor(),
                );
                Some(result)
            }
            EditorEditOutcome::InvalidOffset(result) | EditorEditOutcome::InvalidRange(result) => {
                Some(result)
            }
        }
    }

    pub fn delete_range(
        &mut self,
        byte_start: usize,
        byte_end_exclusive: usize,
        cause: EditorTransactionCause,
    ) -> Option<EditorEditResult> {
        let command = EditorCommand::Delete {
            byte_range: Utf8ByteRange::clamp_rope(
                self.kernel.rope(),
                byte_start,
                byte_end_exclusive,
            ),
            deleted_text: String::new(),
            cause,
            expected_revision: EditorRevision::new(self.mirror.revision()),
        };
        let outcome = self.kernel.apply(command);
        match outcome {
            EditorEditOutcome::Applied(result)
            | EditorEditOutcome::AppliedWithAdjustedSelection(result) => {
                if self.mirror.apply_edit_result(&result).is_err() {
                    self.mirror.load_from_snapshot(
                        self.kernel.snapshot_text(),
                        self.kernel.cursor(),
                        self.kernel.revision(),
                        self.kernel.selection_anchor(),
                    );
                }
                Some(result)
            }
            EditorEditOutcome::NoChange(result) => Some(result),
            EditorEditOutcome::StaleRevision(result) => {
                self.mirror.load_from_snapshot(
                    self.kernel.snapshot_text(),
                    self.kernel.cursor(),
                    self.kernel.revision(),
                    self.kernel.selection_anchor(),
                );
                Some(result)
            }
            EditorEditOutcome::InvalidOffset(result) | EditorEditOutcome::InvalidRange(result) => {
                Some(result)
            }
        }
    }

    pub fn replace_range(
        &mut self,
        byte_start: usize,
        byte_end_exclusive: usize,
        replacement: &str,
        cause: EditorTransactionCause,
    ) -> Option<EditorEditResult> {
        let command = EditorCommand::Replace {
            byte_range: Utf8ByteRange::clamp_rope(
                self.kernel.rope(),
                byte_start,
                byte_end_exclusive,
            ),
            replacement_text: replacement.to_string(),
            original_text: String::new(),
            cause,
            expected_revision: EditorRevision::new(self.mirror.revision()),
        };
        let outcome = self.kernel.apply(command);
        match outcome {
            EditorEditOutcome::Applied(result)
            | EditorEditOutcome::AppliedWithAdjustedSelection(result) => {
                if self.mirror.apply_edit_result(&result).is_err() {
                    self.mirror.load_from_snapshot(
                        self.kernel.snapshot_text(),
                        self.kernel.cursor(),
                        self.kernel.revision(),
                        self.kernel.selection_anchor(),
                    );
                }
                Some(result)
            }
            EditorEditOutcome::NoChange(result) => Some(result),
            EditorEditOutcome::StaleRevision(result) => {
                self.mirror.load_from_snapshot(
                    self.kernel.snapshot_text(),
                    self.kernel.cursor(),
                    self.kernel.revision(),
                    self.kernel.selection_anchor(),
                );
                Some(result)
            }
            EditorEditOutcome::InvalidOffset(result) | EditorEditOutcome::InvalidRange(result) => {
                Some(result)
            }
        }
    }

    pub fn set_selection(&mut self, anchor: usize, head: usize) -> Option<EditorEditResult> {
        let command = EditorCommand::SetSelection {
            anchor: Utf8ByteOffset::clamp_rope(self.kernel.rope(), anchor),
            head: Utf8ByteOffset::clamp_rope(self.kernel.rope(), head),
            expected_revision: EditorRevision::new(self.mirror.revision()),
        };
        let outcome = self.kernel.apply(command);
        match outcome {
            EditorEditOutcome::Applied(result)
            | EditorEditOutcome::AppliedWithAdjustedSelection(result) => {
                if self.mirror.apply_edit_result(&result).is_err() {
                    self.mirror.load_from_snapshot(
                        self.kernel.snapshot_text(),
                        self.kernel.cursor(),
                        self.kernel.revision(),
                        self.kernel.selection_anchor(),
                    );
                }
                Some(result)
            }
            EditorEditOutcome::NoChange(result) => Some(result),
            EditorEditOutcome::StaleRevision(result) => {
                self.mirror.load_from_snapshot(
                    self.kernel.snapshot_text(),
                    self.kernel.cursor(),
                    self.kernel.revision(),
                    self.kernel.selection_anchor(),
                );
                Some(result)
            }
            EditorEditOutcome::InvalidOffset(result) | EditorEditOutcome::InvalidRange(result) => {
                Some(result)
            }
        }
    }

    pub fn perform_undo(&mut self) -> Option<EditorEditResult> {
        let command = EditorCommand::Undo {
            expected_revision: EditorRevision::new(self.mirror.revision()),
        };
        let outcome = self.kernel.apply(command);
        match outcome {
            EditorEditOutcome::Applied(result)
            | EditorEditOutcome::AppliedWithAdjustedSelection(result) => {
                if self.mirror.apply_edit_result(&result).is_err() {
                    self.mirror.load_from_snapshot(
                        self.kernel.snapshot_text(),
                        self.kernel.cursor(),
                        self.kernel.revision(),
                        self.kernel.selection_anchor(),
                    );
                }
                Some(result)
            }
            EditorEditOutcome::NoChange(_) => None,
            EditorEditOutcome::StaleRevision(result) => {
                self.mirror.load_from_snapshot(
                    self.kernel.snapshot_text(),
                    self.kernel.cursor(),
                    self.kernel.revision(),
                    self.kernel.selection_anchor(),
                );
                Some(result)
            }
            EditorEditOutcome::InvalidOffset(_) | EditorEditOutcome::InvalidRange(_) => None,
        }
    }

    pub fn perform_redo(&mut self) -> Option<EditorEditResult> {
        let command = EditorCommand::Redo {
            expected_revision: EditorRevision::new(self.mirror.revision()),
        };
        let outcome = self.kernel.apply(command);
        match outcome {
            EditorEditOutcome::Applied(result)
            | EditorEditOutcome::AppliedWithAdjustedSelection(result) => {
                if self.mirror.apply_edit_result(&result).is_err() {
                    self.mirror.load_from_snapshot(
                        self.kernel.snapshot_text(),
                        self.kernel.cursor(),
                        self.kernel.revision(),
                        self.kernel.selection_anchor(),
                    );
                }
                Some(result)
            }
            EditorEditOutcome::NoChange(_) => None,
            EditorEditOutcome::StaleRevision(result) => {
                self.mirror.load_from_snapshot(
                    self.kernel.snapshot_text(),
                    self.kernel.cursor(),
                    self.kernel.revision(),
                    self.kernel.selection_anchor(),
                );
                Some(result)
            }
            EditorEditOutcome::InvalidOffset(_) | EditorEditOutcome::InvalidRange(_) => None,
        }
    }

    pub fn clear_undo_redo(&mut self) {
        let text = self.kernel.snapshot_text();
        let cursor = self.kernel.cursor();
        let anchor = self.kernel.selection_anchor();
        self.kernel = EditorKernel::with_text(text, cursor).unwrap_or_else(|_| EditorKernel::new());
        if anchor != cursor {
            let _ = self.kernel.apply(EditorCommand::SetSelection {
                anchor: Utf8ByteOffset::clamp_rope(self.kernel.rope(), anchor),
                head: Utf8ByteOffset::clamp_rope(self.kernel.rope(), cursor),
                expected_revision: EditorRevision::new(self.kernel.revision()),
            });
        }
        self.mirror.load_from_snapshot(
            self.kernel.snapshot_text(),
            self.kernel.cursor(),
            self.kernel.revision(),
            self.kernel.selection_anchor(),
        );
    }

    pub fn prepare_composition_commit(
        &mut self,
        inserted_text: &str,
        fallback_cursor: usize,
        preedit_byte_start: usize,
        preedit_byte_end: usize,
    ) -> CompositionCommitResult {
        self.composition.save_pending_preedit_cursor_rect();
        let pending_pcr = self.composition.take_pending_preedit_cursor_rect();
        let was_composing = self.composition.is_composing();
        let saved_virtual_text = self.composition.virtual_text();
        let (session_replace_start, session_replace_end) =
            self.composition.session_replace_range(fallback_cursor);
        let candidate_byte_start = session_replace_start;
        let candidate_byte_end = session_replace_start + inserted_text.len();
        let committed_replace_start = session_replace_start;
        let committed_replace_end = session_replace_end;

        self.composition.clear_preedit_fields();

        CompositionCommitResult {
            pending_preedit_cursor_rect: pending_pcr,
            was_composing,
            preedit_byte_start,
            preedit_byte_end,
            saved_virtual_text,
            session_replace_start,
            session_replace_end,
            candidate_byte_start,
            candidate_byte_end,
            committed_replace_start,
            committed_replace_end,
        }
    }

    pub fn finish_composition_commit(&mut self) {
        self.composition.finish_session();
    }

    pub fn current_layout_snapshot(&self) -> &Option<EditorLayoutSnapshot> {
        &self.current_layout_snapshot
    }

    pub fn set_current_layout_snapshot(&mut self, snapshot: Option<EditorLayoutSnapshot>) {
        self.current_layout_snapshot = snapshot;
    }

    pub fn set_previous_layout_snapshot(&mut self, snapshot: Option<EditorLayoutSnapshot>) {
        self.previous_layout_snapshot = snapshot;
    }

    pub fn set_previous_canonical_snapshot(
        &mut self,
        snapshot: Option<crate::editor::layout::CanonicalDocumentVisualSnapshot>,
    ) {
        self.previous_canonical_snapshot = snapshot;
    }

    /// Issue #658 评论 5622829886 问题 1: 取出 record_visual_transaction 产生的
    /// pending promoted layout，由 record_transaction 交给 EditorLayout::promote_prepared_layout。
    pub fn take_pending_promoted_layout(
        &mut self,
    ) -> Option<crate::editor::layout::PromotedLayout> {
        self.pending_promoted_layout.take()
    }

    /// Issue #658 评论 5623746506 问题 2b: composition commit 分支需要把
    /// build_editor_layout_snapshot 排好的 new prepared layout 存入 pending，
    /// 由 emit_content_changed 提升为 current，避免 emit_content_changed ->
    /// ensure_layout_cached 对同一已提交正文再次排版。
    pub fn set_pending_promoted_layout(
        &mut self,
        layout: Option<crate::editor::layout::PromotedLayout>,
    ) {
        self.pending_promoted_layout = layout;
    }

    pub fn prepare_transaction_textures(&mut self, key: VisualTransactionKey) {
        let tx = self
            .animation_coordinator
            .prepared_queue
            .active_transactions()
            .iter()
            .find(|t| t.key == key)
            .cloned();

        if let Some(t) = tx {
            let snapshot_ids = t.snapshot_ids();
            if snapshot_ids.is_empty() {
                self.animation_coordinator
                    .prepared_queue
                    .mark_texture_prepared(key);
                return;
            }

            let mut all_found = true;
            for id in &snapshot_ids {
                if !self.texture_cache.contains_line(id) {
                    all_found = false;
                    break;
                }
            }

            if all_found {
                self.animation_coordinator
                    .prepared_queue
                    .mark_texture_prepared(key);
                return;
            }

            if let Some(ref old_snap) = t.old_snapshot {
                for line in &old_snap.line_snapshots {
                    if let Some(ref image) = line.image {
                        self.texture_cache.insert_line(line.id, image.clone());
                    }
                }
            }
            if let Some(ref new_snap) = t.new_snapshot {
                for line in &new_snap.line_snapshots {
                    if let Some(ref image) = line.image {
                        self.texture_cache.insert_line(line.id, image.clone());
                    }
                }
            }

            let mut any_missing = false;
            for id in &snapshot_ids {
                if !self.texture_cache.contains_line(id) {
                    any_missing = true;
                    break;
                }
            }

            if any_missing {
                super::editor_animation_debug_log(&format!(
                    "prepare_transaction_textures: some line textures missing for tid={}, cancelling",
                    key.transaction_id
                ));
                self.animation_coordinator
                    .cancel_by_key(key, "texture_failed");
            } else {
                self.animation_coordinator
                    .prepared_queue
                    .mark_texture_prepared(key);
            }
        }
    }

    pub fn record_visual_transaction(
        &mut self,
        ctx: &VisualTransactionContext,
        old: &EditorSnapshot,
        new: &EditorSnapshot,
        cause: EditorTransactionCause,
        editor_layout: &crate::editor::layout::EditorLayout,
    ) -> Option<EditorVisualTransaction> {
        let transaction = self.engine.create_transaction(
            &old.text,
            &new.text,
            EditorSelection {
                anchor: EditorCursor::new(&old.text, old.selection_anchor),
                head: EditorCursor::new(&old.text, old.cursor),
            },
            EditorSelection {
                anchor: EditorCursor::new(&new.text, new.selection_anchor),
                head: EditorCursor::new(&new.text, new.cursor),
            },
            cause,
        );
        let mut vt = self.engine.visual_transaction(&transaction);

        if ctx.typing_animation_enabled && vt.is_some() && !ctx.is_scrolling {
            if let Some(ref mut vt) = vt {
                let (affected_byte_start, affected_byte_end) = vt
                    .inserted_range
                    .or(vt.deleted_range)
                    .map(|r| (r.start().value(), r.end().value()))
                    .unwrap_or_else(|| {
                        let changes =
                            writer_core::editor::diff_plain_text(&vt.old_text, &vt.new_text);
                        let mut min_b = usize::MAX;
                        let mut max_b = 0usize;
                        for change in &changes {
                            match change {
                                writer_core::editor::EditorChange::Insert { index, text } => {
                                    min_b = min_b.min(index.value());
                                    max_b = (index.value() + text.len()).max(max_b);
                                }
                                writer_core::editor::EditorChange::Delete { index, text } => {
                                    min_b = min_b.min(index.value());
                                    max_b = (index.value() + text.len()).max(max_b);
                                }
                                _ => {}
                            }
                        }
                        (min_b.min(max_b), max_b)
                    });

                // Issue #658 评论 5624570557 问题 1: 从 pipeline 获取 old current prepared layout 句柄，
                // 不再重新排版 old text。
                let old_prepared_handle = editor_layout.current_prepared_layout();
                let old_generation = old_prepared_handle
                    .as_ref()
                    .map(|h| h.generation)
                    .unwrap_or(0);

                // Issue #658 评论 5626002895 问题 1: old caret 从当前 cache 的真实 QTextLine 取 x，
                // 不用 assemble_document_visual_snapshot_from_lines 产生的空 cursor_x_map
                // （cursor_x_from_canonical 在 cursor_x_map 为空时退化为 line.x 行首，
                // 导致正文光标在行中间时打一字后协同光标动画起点从行首开始）。
                // 只有有 old_prepared_handle 的路径才从 cache 算；fallback 路径仍用
                // old_doc_snapshot.cursor_rect()（fallback 的 prepare_affected_paragraphs_visual_snapshot
                // 会真正排版并生成 cursor_x_map）。
                let old_cursor_byte = vt.old_selection.head.index.value();
                let old_caret_from_cache: Option<layout::CaretRect> =
                    if old_prepared_handle.is_some() {
                        editor_layout.cache().map(|snap| {
                            editor_layout.caret_rect(
                                snap,
                                old_cursor_byte,
                                layout::CaretAffinity::Downstream,
                                ctx.scroll_y,
                                ctx.viewport_height,
                            )
                        })
                    } else {
                        None
                    };

                // Issue #658 评论 5620035970 问题 2: 不再 clear_paragraph_layout_cache()，
                // 而是分配独立 generation，与静态正文路径互不干扰。
                let new_generation = layout::begin_layout_generation();

                // Issue #658 评论 5624570557 问题 2: 先做 new 基础排版，得到 new_lines 用于比较
                let mut new_doc_snapshot = layout::prepare_document_visual_snapshot_scoped(
                    &new.text,
                    0,
                    ctx.font_pixel_size,
                    &ctx.font_family,
                    ctx.line_spacing,
                    ctx.padding,
                    ctx.text_indent,
                    ctx.bounding_width,
                    ctx.dpr,
                    &ctx.text_color,
                    new_generation,
                    affected_byte_start,
                    affected_byte_end,
                );

                // Issue #658 评论 5624570557 问题 1+2: 比较 old/new VisualLine，计算受影响 line_ids
                // Issue #658 评论 5626002895 问题 3: fallback 路径分配真实 generation 构造 old snapshot，
                // 用完 clear_layout_generation 释放。generation 0 只作为"无 generation"哨兵值，
                // 不能拿去实际存 QTextLayout（promote_prepared_layout 跳过 old_generation==0 不释放）。
                let mut fallback_old_generation_opt: Option<u64> = None;
                let old_doc_snapshot = if let Some(ref handle) = old_prepared_handle {
                    // 比较 old/new lines 获取受影响的 line_ids（old 侧和 new 侧）
                    // Issue #658 评论 5626628570: compare_old_new_visual_lines 返回 VisualLineDiff，
                    // 把行分成需要重新栅格化的 raster 行和可复用纹理的 reusable_move_pairs。
                    let diff = layout::compare_old_new_visual_lines(
                        handle.lines,
                        &new_doc_snapshot.visual_lines,
                        vt.inserted_range
                            .map(|r| (r.start().value(), r.end().value())),
                        vt.deleted_range
                            .map(|r| (r.start().value(), r.end().value())),
                    );

                    // 从已有 old layout 提取 old 动画视觉（只提取需要重新栅格化的行）
                    // Issue #658 评论 5625515748 问题 1: 不再传整篇正文 + 起点 0，
                    // prepare_animation_visuals_from_layout 内部从每行 para_text/para_start 取段落级文本。
                    let old_line_snapshots = layout::prepare_animation_visuals_from_layout(
                        handle,
                        &diff.old_raster_line_ids,
                        ctx.dpr,
                        &ctx.text_color,
                    );

                    // 构建最小化的 old_doc_snapshot，仅用于 cursor_rect 计算
                    // Issue #658 评论 5625515748 问题 2: 不再调 prepare_document_visual_snapshot
                    // 重新排版整篇 old text（false 只跳过 QImage/glyph 生成，不跳过
                    // QTextLayout beginLayout/createLine）。改为从已有 VisualLine 组装
                    // CanonicalDocumentVisualSnapshot（只填 Rust 几何数据，不调 QTextLayout），
                    // 再由 inject_animation_visuals_into_snapshot 注入动画视觉。
                    let mut doc_snap = layout::assemble_document_visual_snapshot_from_lines(
                        handle.lines,
                        0,
                        ctx.font_pixel_size,
                        &ctx.font_family,
                        ctx.line_spacing,
                        ctx.text_indent,
                        ctx.padding,
                        ctx.bounding_width,
                        ctx.dpr,
                    );

                    // Issue #658 评论 5624570557 问题 1: 把从已有 layout 提取的动画视觉
                    // （QImage/clusters）注入到 old_doc_snapshot，使动画纹理可用。
                    layout::inject_animation_visuals_into_snapshot(
                        &mut doc_snap,
                        old_line_snapshots,
                    );

                    // Issue #658 评论 5624570557 问题 1+2: 从已有 new layout 提取 new 动画视觉。
                    // new_doc_snapshot 已完成基础排版（QTextLayout 存入 new_generation），
                    // 从已有 QTextLine 只提取受影响行的 QImage/glyph/cluster。
                    let new_handle = layout::PreparedLayoutHandle {
                        generation: new_generation,
                        lines: &new_doc_snapshot.visual_lines,
                    };
                    let new_line_snapshots = layout::prepare_animation_visuals_from_layout(
                        &new_handle,
                        &diff.new_raster_line_ids,
                        ctx.dpr,
                        &ctx.text_color,
                    );
                    layout::inject_animation_visuals_into_snapshot(
                        &mut new_doc_snapshot,
                        new_line_snapshots,
                    );

                    // Issue #658 评论 5626628570: reusable_move_pairs —— 内容/shaping 完全相同、
                    // 只是 x/y 文档位置变化的行。复用 old 行已有 image/clusters/source rect，
                    // 用 new VisualLine 的 x/y 生成 reflow_move 终点。
                    //
                    // 修复点 1 (Issue #658 评论 5627327573): 之前只把旧纹理改 byte range 后
                    // 注入 new_doc_snapshot，old snapshot 这一侧没有 image/clusters，导致
                    // animation_coordinator 生成 reflow_move 时 old 侧
                    // source_rect_for_byte_range 返回 None，reflow_move 建不出来。
                    //
                    // 改法：对每个 reusable_move_pair(old_idx, new_idx) 只从 old prepared layout
                    // 提取一次视觉资源（prepare_animation_visuals_from_layout），然后分成两份：
                    // - 第一份：保持原始 byte range / old VisualLine 几何（不改 document_byte_start/end、
                    //   不改 cluster byte range），注入 doc_snap（old snapshot）。
                    // - 第二份：复用同一张 QImage 和同一套 cluster source rect，只把 document byte range
                    //   映射到 new（document_byte_start=new_line.byte_start, document_byte_end=new_line.byte_end，
                    //   cluster 按 byte_delta 偏移），注入 new_doc_snapshot。
                    // QImage clone 是浅拷贝（引用计数），不会重画。完成后 old/new 两边都有 source rect。
                    if !diff.reusable_move_pairs.is_empty() {
                        let mut old_move_visuals: Vec<layout::CanonicalLineSnapshot> = Vec::new();
                        let mut move_visuals: Vec<layout::CanonicalLineSnapshot> = Vec::new();
                        for &(old_idx, new_idx) in &diff.reusable_move_pairs {
                            if old_idx >= handle.lines.len()
                                || new_idx >= new_doc_snapshot.visual_lines.len()
                            {
                                continue;
                            }
                            let old_snaps = layout::prepare_animation_visuals_from_layout(
                                handle,
                                std::slice::from_ref(&old_idx),
                                ctx.dpr,
                                &ctx.text_color,
                            );
                            if let Some(snap) = old_snaps.into_iter().next() {
                                // 第一份：保持原始 old byte range，注入 old snapshot (doc_snap)
                                old_move_visuals.push(snap.clone());

                                // 第二份：复用同一张 QImage 和 cluster source rect，
                                // 只把 document byte range 映射到 new 行
                                let new_line = &new_doc_snapshot.visual_lines[new_idx];
                                let byte_delta: isize = new_line.byte_start as isize
                                    - snap.document_byte_start as isize;
                                let mut new_snap = snap.clone();
                                new_snap.document_byte_start = new_line.byte_start;
                                new_snap.document_byte_end = new_line.byte_end;
                                for cluster in &mut new_snap.clusters {
                                    cluster.document_byte_start = cluster
                                        .document_byte_start
                                        .saturating_add_signed(byte_delta);
                                    cluster.document_byte_end =
                                        cluster.document_byte_end.saturating_add_signed(byte_delta);
                                }
                                move_visuals.push(new_snap);
                            }
                        }
                        if !old_move_visuals.is_empty() {
                            layout::inject_animation_visuals_into_snapshot(
                                &mut doc_snap,
                                old_move_visuals,
                            );
                        }
                        if !move_visuals.is_empty() {
                            layout::inject_animation_visuals_into_snapshot(
                                &mut new_doc_snapshot,
                                move_visuals,
                            );
                        }
                    }

                    doc_snap
                } else {
                    // fallback: 没有 prepared layout，用受影响段落排版
                    // Issue #658 评论 5626002895 问题 3: 分配真实 generation 构造 fallback old snapshot，
                    // 不再用 0（generation 0 是哨兵值，promote_prepared_layout 跳过 0 不释放会导致泄漏）。
                    let fallback_old_generation = layout::begin_layout_generation();
                    fallback_old_generation_opt = Some(fallback_old_generation);
                    let prev_new_snapshot = self.previous_canonical_snapshot.as_ref();
                    layout::prepare_affected_paragraphs_visual_snapshot(
                        &vt.old_text,
                        0,
                        ctx.font_pixel_size,
                        &ctx.font_family,
                        ctx.line_spacing,
                        ctx.padding,
                        ctx.text_indent,
                        ctx.bounding_width,
                        ctx.dpr,
                        &ctx.text_color,
                        affected_byte_start,
                        affected_byte_end,
                        prev_new_snapshot,
                        fallback_old_generation,
                        true,
                    )
                };

                let old_caret = old_caret_from_cache.unwrap_or_else(|| {
                    old_doc_snapshot.cursor_rect(
                        vt.old_selection.head.index.value(),
                        layout::CaretAffinity::Downstream,
                        ctx.scroll_y,
                        ctx.viewport_height,
                    )
                });
                let new_caret = new_doc_snapshot.cursor_rect(
                    vt.new_selection.head.index.value(),
                    layout::CaretAffinity::Downstream,
                    ctx.scroll_y,
                    ctx.viewport_height,
                );

                vt.old_cursor_rect = Some(make_cursor_rect_from_caret_doc(
                    &old_caret,
                    &old_doc_snapshot,
                    &ctx.font_family,
                    ctx.scroll_y,
                ));
                vt.new_cursor_rect = Some(make_cursor_rect_from_caret_doc(
                    &new_caret,
                    &new_doc_snapshot,
                    &ctx.font_family,
                    ctx.scroll_y,
                ));

                // Issue #658 评论 5626002895 问题 3: fallback old snapshot 的图片/cluster/cursor map
                // 已复制进 Rust snapshot（old_doc_snapshot），fallback_old_generation 的 QTextLayout
                // 不再需要，立即释放避免生命周期泄漏。old_doc_snapshot 后续 build_old_new_from_canonical
                // 和 previous_layout_snapshot 只消费 Rust 数据，不依赖 QTextLayout。
                if let Some(gen) = fallback_old_generation_opt {
                    layout::clear_layout_generation(gen);
                }

                match vt.kind {
                    EditorAnimationKind::Insert => {
                        vt.insert_glyph_rects = Some(Vec::new());
                        vt.reflow_glyph_rects = None;
                    }
                    EditorAnimationKind::Delete => {
                        vt.deleted_glyph_rects = None;
                    }
                    EditorAnimationKind::Cursor => {}
                }

                let old_revision = self.layout_revision;
                let new_revision = LayoutRevision::next();

                let (old_snap, new_snap) = LineSnapshotBuilder::build_old_new_from_canonical(
                    &old_doc_snapshot,
                    &new_doc_snapshot,
                    old_revision,
                    new_revision,
                    ctx.scroll_y,
                    ctx.viewport_height,
                );

                let key = self.animation_coordinator.process_transaction(
                    vt,
                    ctx.typing_animation_enabled,
                    ctx.is_scrolling,
                    ctx.is_loading,
                    ctx.is_applying_format,
                    ctx.is_applying_settings,
                    vt.old_cursor_rect.clone(),
                    vt.new_cursor_rect.clone(),
                    &old_snap,
                    &new_snap,
                );
                if let Some(key) = key {
                    self.prepare_transaction_textures(key);
                    self.layout_revision = new_revision;
                }

                self.previous_layout_snapshot =
                    Some(self.current_layout_snapshot.clone().unwrap_or_else(|| {
                        EditorLayoutSnapshot::new(
                            old_doc_snapshot.to_layout_snapshot(),
                            Vec::new(),
                            None,
                            layout::CaretAffinity::Downstream,
                        )
                    }));
                self.current_layout_snapshot = Some(new_snap);

                // Issue #658 评论 5624570557 问题 1: old generation 不在此处释放，
                // 而是存入 PromotedLayout.old_generation，在 promote 时随 new generation 一起释放。
                // 这样保证 old 动画纹理在 new prepared layout 成为 current 之前一直有效。

                // 先 clone visual_lines 用于 PromotedLayout，再 move new_doc_snapshot 到 previous_canonical_snapshot。
                let promoted_visual_lines = new_doc_snapshot.visual_lines.clone();
                self.previous_canonical_snapshot = Some(new_doc_snapshot);

                self.pending_promoted_layout = Some(layout::PromotedLayout {
                    generation: new_generation,
                    visual_lines: promoted_visual_lines,
                    width: ctx.bounding_width,
                    font_size: ctx.font_pixel_size as f32,
                    font_family: ctx.font_family.clone(),
                    line_spacing: ctx.line_spacing as f32,
                    text_indent: ctx.text_indent as f32,
                    padding: ctx.padding as f32,
                    old_generation,
                });

                super::editor_animation_debug_log(&format!(
                    "record_visual_transaction: processed via canonical document snapshot pipeline, kind={:?}, has_active_insert={}",
                    vt.kind,
                    self.animation_coordinator.has_active_insert()
                ));
            }
        }

        vt
    }
}

fn make_cursor_rect_from_caret_doc(
    caret: &layout::CursorLayoutRect,
    doc_snapshot: &layout::CanonicalDocumentVisualSnapshot,
    font_family: &str,
    scroll_y: f64,
) -> CursorRect {
    let line = doc_snapshot
        .visual_lines
        .iter()
        .find(|l| l.id == caret.visual_line_id);
    let baseline_y = match line {
        Some(l) => layout::text_baseline_y(l, doc_snapshot.font_size, font_family) - scroll_y,
        None => caret.y + caret.h * 0.8,
    };
    CursorRect {
        x: caret.x,
        top: caret.y,
        bottom: caret.y + caret.h,
        baseline_y,
    }
}
