use super::animation::LinuxEditorAnimationCoordinator;
use super::edit_motion::{CompositionSession, CursorRect, EditorAnimationKind, PreparedEditMotion};
use super::edit_snapshot::EditorSnapshot;
use super::layout_revision::LayoutRevision;
use super::layout_snapshot::EditorLayoutSnapshot;
use super::line_snapshot_builder::LineSnapshotBuilder;
use super::text_utils::{clamp_to_char_boundary, normalize_plain_text};
use super::texture_cache::TextureCache;
use super::transaction_key::VisualTransactionKey;
use super::PreeditAttribute;
use crate::editor::layout;
use crate::platform::linux_qt::LinuxQtClipboardFocusAdapter;
use std::time::Instant;
use writer_core::editor::{
    DisplayPatch, EditorChange, EditorCommand, EditorEditOutcome, EditorEditResult, EditorKernel,
    EditorRevision, EditorTransactionCause, Utf8ByteOffset, Utf8ByteRange,
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

    /// 是否有非空选区（cursor != selection_anchor）。
    pub fn has_selection(&self) -> bool {
        self.cursor != self.selection_anchor
    }

    /// 返回选区的半开区间 [start, end)（UTF-8 byte offset）。
    /// start ≤ end，无论光标和锚点的相对位置。
    pub fn selection_range(&self) -> (usize, usize) {
        if self.cursor <= self.selection_anchor {
            (self.cursor, self.selection_anchor)
        } else {
            (self.selection_anchor, self.cursor)
        }
    }

    /// 返回选区文本。无选区时返回空字符串。
    pub fn selected_text(&self) -> String {
        if !self.has_selection() {
            return String::new();
        }
        let (start, end) = self.selection_range();
        self.text[start..end].to_string()
    }

    /// 返回当前 text/cursor/selection_anchor 的不可变快照。
    ///
    /// 供动画/事务记录 old/new 状态使用。正文真相仍在 EditorKernel，
    /// 此快照仅用于动画对比，不维护 undo/redo 栈。
    pub fn snapshot(&self) -> EditorSnapshot {
        EditorSnapshot {
            text: self.text.clone(),
            cursor: self.cursor,
            selection_anchor: self.selection_anchor,
        }
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
        // Issue #701 评论 5704688994 问题 4: 原子 patch batch。
        // display_patches 是"同一 base revision 的原子 batch"：Core ImeCommit /
        // DeleteSurrounding 会为多个 delta 生成多条 DisplayPatch，共享同一个
        // base_revision/new_revision。旧实现循环中每应用一条 patch 就立刻把
        // self.revision 改成 new_revision，第二条 patch 一定报 revision discontinuity。
        //
        // 协议：
        // 1. 先一次检查 result.base_revision 与 mirror.revision 一致；
        // 2. 所有 patch 按原始 mirror 文本坐标校验（应用前校验，失败则 self.text 不变）；
        // 3. 按 replace_byte_range.start 从大到小 stable 排序后应用（前面的编辑不会
        //    移动后面的坐标）；同起点保持原顺序（防御性，Core 应保证不产生同起点 patch）；
        // 4. 全部应用完成后只设置一次 self.revision = new_revision。
        if result.base_revision.value() != self.revision {
            return Err(format!(
                "CommittedTextMirror revision discontinuity: expected {}, got {}. Must reload from kernel snapshot.",
                self.revision, result.base_revision.value()
            ));
        }

        // 校验阶段：所有 patch 按原始 mirror 文本坐标检查越界 / char boundary。
        // 任何一条失败则返回错误，self.text 未被修改，状态一致。
        for patch in &result.display_patches {
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
        }

        // 应用阶段：按 replace_byte_range.start 从大到小 stable 排序后应用。
        // 右侧修改不影响左侧旧坐标。应用阶段不再校验（校验阶段已完成）。
        let mut sorted: Vec<&DisplayPatch> = result.display_patches.iter().collect();
        sorted.sort_by_key(|p| std::cmp::Reverse(p.replace_byte_range.start().value()));
        for patch in sorted {
            let range = patch.replace_byte_range.to_std_range();
            self.text.replace_range(range, &patch.inserted_text);
        }
        // 全部 patch 应用完成后只设置一次 revision。
        self.revision = result.new_revision.value();
        // Issue #683：用 result 的 new anchor/head 更新 mirror，不再从
        // new_selection_byte_range.to_std_range() 反推 anchor=start, head=end
        // （方向会丢失）。display_patches 为空时（如 SetSelection）也要更新
        // mirror cursor/selection_anchor，否则光标锁死。
        let new_anchor = result.new_selection.anchor.index.value();
        let new_head = result.new_selection.head.index.value();
        if new_anchor > self.text.len() || new_head > self.text.len() {
            return Err(format!(
                "CommittedTextMirror selection out of bounds: ({}, {}) vs text len {}. Must reload from kernel snapshot.",
                new_anchor, new_head, self.text.len()
            ));
        }
        if !self.text.is_char_boundary(new_anchor) || !self.text.is_char_boundary(new_head) {
            return Err(format!(
                "CommittedTextMirror selection not on char boundary: ({}, {}). Must reload from kernel snapshot.",
                new_anchor, new_head
            ));
        }
        self.cursor = new_head;
        self.selection_anchor = new_anchor;
        Ok(())
    }
}

/// IME 组合输入状态 — 跟踪一次 composition 从 preedit 到 commit/cancel 的完整生命周期。
///
/// 生命周期：preedit 开始 → (多次 updatePreedit) → commit 或 cancel。
/// composition_session 由 EditorKernel 在 beginComposition 时创建，包含 replace range
/// 和 virtual text。commit 后 session 被清除，preedit 字段归零。
///
/// Issue #704: `suppress_next_ime_commit` 的语义收窄为"刚刚取消过一个真实
/// composition，允许忽略它可能迟到的一次 commit"。它不再表示"最近按过 ESC"。
/// 只有 `input_cancel_preedit_for_escape` 在确认当前确实存在活跃
/// composition/preedit 并真的执行了取消后，才会武装一次该标记。普通 ESC（无
/// composition）不武装，避免吞掉下一次直接 IME commit。新非空 preedit 也会
/// 清除该标记（用户重新开始输入）。
pub(crate) struct CompositionState {
    pub preedit_text: String,
    pub preedit_cursor: usize,
    pub preedit_attributes: Vec<PreeditAttribute>,
    pub preedit_old_text: String,
    pub composition_session: Option<CompositionSession>,
    pub preedit_cursor_rect: Option<CursorRect>,
    pub pending_preedit_cursor_rect: Option<CursorRect>,
    /// Issue #704: "刚刚取消过一个真实 composition，允许忽略它可能迟到的一次
    /// commit"。仅由 `input_cancel_preedit_for_escape` 在确认存在活跃
    /// composition 后武装一次；不再表示"最近按过 ESC"。被一次 commit 消费后
    /// 清除，新非空 preedit 也会清除。
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
        self.preedit_cursor_rect = None;
    }

    pub fn session_replace_range(&self, fallback_cursor: usize) -> (usize, usize) {
        self.composition_session
            .as_ref()
            .map(|s| (s.replace_start, s.replace_end_exclusive))
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
/// 每次布局重算时由平台端填充当前值。所有尺寸均为逻辑像素（device-independent，
/// Qt 逻辑坐标），不是物理像素。`scroll_y` 为文档坐标系中的滚动偏移，不含 viewport
/// 顶部 padding。
///
/// 坐标空间约定（Issue #692 评论 5：写作区字号语义在不同 DPI 下不漂）：
/// - 所有 x/y 坐标为文档逻辑坐标系（不含滚动偏移），布局引擎在渲染时减去 scroll_y
/// - bounding_width / font_pixel_size / padding / text_indent / line_spacing /
///   scroll_y / viewport_height 均为逻辑像素
/// - font_pixel_size 是用户设置字号（来自 settingsBackend.setting_font_size），
///   以逻辑像素语义解释：渲染时通过 QFont.setPixelSize(fs) 配合
///   img.setDevicePixelRatio(dpr) 或 painter.scale(dpr, dpr) 转为物理像素。
///   命名沿用 Qt QFont.setPixelSize 习惯，但语义是逻辑像素，不在此处乘 dpr。
/// - dpr 只用于底层渲染（QImage 纹理尺寸、img.setDevicePixelRatio、sourceRect
///   物理坐标、光标像素对齐），不用于上层 UI 尺寸整体缩放。
/// - 布局几何（line.x/y/height、naturalTextWidth、content_height）保持逻辑坐标；
///   仅纹理 sourceRect 使用物理像素（srcX * dpr），由 source_rect_to_document_rect
///   除以 dpr 转回逻辑坐标。
pub(crate) struct VisualTransactionContext {
    pub typing_animation_enabled: bool,
    pub smooth_cursor_enabled: bool,
    /// Issue #756: 协同动画显式模式开关。
    /// true 时文字与光标绑死，要求有效 caret motion 否则文字动画也不启动；
    /// false 时 typing/smooth 两个独立开关各自决定文字/光标动画。
    pub coordinated_animation_enabled: bool,
    pub is_scrolling: bool,
    pub is_loading: bool,
    pub is_applying_format: bool,
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
    /// Issue #738 评论 5789470425 问题1: 当前 canonical document visual snapshot。
    /// 语义明确为"当前 canonical"——纯布局变化和正文编辑都通过
    /// `reconcile_active_transactions_with_new_canonical` 入口更新此字段：
    /// 先用新 canonical reconcile 旧活动事务，再把它保存为当前 canonical。
    /// 不再用名字和语义都混乱的 `previous_canonical_snapshot` 充当布局变化后的
    /// current canonical。
    current_canonical_snapshot: Option<crate::editor::layout::CanonicalDocumentVisualSnapshot>,
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
            animation_coordinator: LinuxEditorAnimationCoordinator::new(),
            texture_cache: TextureCache::new(),
            clipboard_adapter: LinuxQtClipboardFocusAdapter::new(),
            text_revision: 0,
            visual_revision: 0,
            typing_animation_duration_ms: 160,
            cursor_animation_duration_ms: 120,
            current_layout_snapshot: None,
            previous_layout_snapshot: None,
            current_canonical_snapshot: None,
            layout_revision: LayoutRevision::initial(),
            pending_promoted_layout: None,
        }
    }

    /// Issue #738 评论 5787277777: 获取 Pipeline 当前的 layout revision，
    /// 供 FrameContext 和 reconcile 入口作为 canonical basis revision 使用。
    pub fn layout_revision(&self) -> LayoutRevision {
        self.layout_revision
    }

    /// Issue #738 评论 5797637204: 无条件提交新 layout revision 的 setter。
    /// 供 `editing.rs::record_composition_commit_transaction` 在 pipeline 外部
    /// 把 composition commit 产生的新 canonical basis revision 提交到 Pipeline，
    /// 与普通正文路径 `prepare_edit_motion`（pipeline.rs:1452 `self.layout_revision = new_revision;`）
    /// 保持同一语义：新 canonical 一旦确定，layout_revision 必须无条件一起提交，
    /// 否则 basis 守卫（==/!=）会把"事务 revision 比 Pipeline 当前 revision 更新"
    /// 误当合法事务继续画。不用 `bump_layout_revision`（它会再调一次 next() 生成
    /// 另一个 revision，与已采的 new_revision 不一致）。
    pub fn set_layout_revision(&mut self, rev: LayoutRevision) {
        self.layout_revision = rev;
    }

    /// Issue #738 评论 5787277777: 推进 layout revision，使旧活动事务的 basis revision
    /// 过期。geometry_changed（宽度变化）和 layout_property_changed（字号/字体/行距/缩进/
    /// padding 变化）后调此方法，让 build_render_plan_full 的 basis revision 守卫跳过
    /// 仍绑定旧 canonical 几何的 unit，canonical 正文立即接管。旧事务最终因 is_expired
    /// 超时或下一次 record_visual_transaction 的 reconcile 被移除。
    pub fn bump_layout_revision(&mut self) -> LayoutRevision {
        self.layout_revision = LayoutRevision::next();
        self.layout_revision
    }

    /// Issue #738 评论 5789470425 问题1: 纯布局变化（resize/字号/字体/行距）的 canonical
    /// snapshot 构造/保存入口收口到 Pipeline。此入口接收**已经完成的新
    /// `CanonicalDocumentVisualSnapshot`**（按新 width/font/line_spacing/padding 算完），
    /// 先 bump_layout_revision 得到 new revision，再用这份 snapshot 调
    /// `reconcile_active_transactions_with_canonical` 把旧活动事务从旧 canonical 几何
    /// 重绑到这份新 canonical，最后把它保存为当前 canonical（`current_canonical_snapshot`）。
    ///
    /// 不再用名字和语义都混乱的 `previous_canonical_snapshot` 充当布局变化后的 current
    /// canonical。reconcile 发生在新 canonical 已构造完成之后，而非"先 bump 再拿旧
    /// canonical reconcile"。
    ///
    /// reconcile 删除 unit / 完成事务后同步按剩余 active snapshot ids 收一次 texture cache，
    /// 不让已经失去 owner 的纹理一直挂到后续别的完成路径才释放。
    pub fn reconcile_active_transactions_with_new_canonical(
        &mut self,
        new_snapshot: crate::editor::layout::CanonicalDocumentVisualSnapshot,
    ) -> LayoutRevision {
        let new_revision = self.bump_layout_revision();
        let current_text = self.mirror.text().to_string();
        self.animation_coordinator
            .reconcile_active_transactions_with_canonical(
                &current_text,
                &new_snapshot,
                new_revision,
                std::time::Instant::now(),
            );
        // 把新 canonical 保存为当前 canonical。
        self.current_canonical_snapshot = Some(new_snapshot);
        // reconcile 删除 unit / 完成事务后同步收 texture cache。
        let active_ids = self.animation_coordinator.collect_active_snapshot_ids();
        self.texture_cache.retain_active_snapshot_ids(&active_ids);
        new_revision
    }

    /// Issue #738 评论 5789470425 问题1 / 评论 5792244119 问题 1: 用当前排版参数
    /// 构造一份新的 `CanonicalDocumentVisualSnapshot`，**并提取动画视觉资源
    ///（QImage/glyphRuns/clusters）注入**，使 rebind 路径 `find_clusters_in_canonical`
    /// 能找到 cluster，布局变化后 Timed Reflow 能继续播放而非全部 Snap 回 canonical。
    ///
    /// 供 `geometry_changed` / `layout_property_changed` 在新排版完成后调
    /// `reconcile_active_transactions_with_new_canonical` 使用。
    ///
    /// 实现要点（评论 5792244119 问题 1 修复）：
    /// - **复用** `editor_layout.current_prepared_layout()` 的当前 generation 做基础
    ///   snapshot，不再 `begin_layout_generation()` 分配临时 generation。调用方
    ///   `reconcile_after_layout_change` 之前已 `ensure_layout_cached` 完成新排版，
    ///   该 generation 由 `EditorLayout` 自身生命周期管理，本函数不持有也不释放，
    ///   不会泄漏。
    /// - 用 `prepare_animation_visuals_from_layout` 从该 generation 提取**所有行**
    ///   的 QImage/clusters（布局变化后任意行位置都可能改变，不能只提取受影响行），
    ///   再 `inject_animation_visuals_into_snapshot` 注入到同一份新 canonical。
    /// - 若 `editor_layout` 无当前 prepared layout（首帧/invalidate 后尚未排版），
    ///   fallback 到 `begin_layout_generation` 临时 generation 并在提取完成后
    ///   `clear_layout_generation` 释放，保持原语义不泄漏。
    pub fn build_canonical_snapshot_for_current_layout(
        &self,
        ctx: &VisualTransactionContext,
        editor_layout: &crate::editor::layout::EditorLayout,
    ) -> crate::editor::layout::CanonicalDocumentVisualSnapshot {
        // Issue #738 评论 5792244119 问题 1: 优先复用 EditorLayout 当前 prepared layout
        // 的 generation，不再分配临时 generation（避免泄漏）。
        let prepared_handle = editor_layout.current_prepared_layout();
        let (snapshot, fallback_gen): (
            crate::editor::layout::CanonicalDocumentVisualSnapshot,
            Option<u64>,
        ) = match prepared_handle.as_ref() {
            Some(handle) => {
                // 复用当前 generation 做基础 snapshot（不持有 generation，不释放）。
                let snap = layout::prepare_document_visual_snapshot_scoped(
                    self.mirror.text(),
                    self.text_revision,
                    ctx.font_pixel_size,
                    &ctx.font_family,
                    ctx.line_spacing,
                    ctx.padding,
                    ctx.text_indent,
                    ctx.bounding_width,
                    ctx.dpr,
                    Some(&ctx.text_color),
                    handle.generation,
                    0,
                    0,
                );
                (snap, None)
            }
            None => {
                // fallback: EditorLayout 无当前 prepared layout（首帧/invalidate 后尚未排版）。
                // 分配临时 generation，提取完成后释放，不泄漏。
                let gen = layout::begin_layout_generation();
                let snap = layout::prepare_document_visual_snapshot_scoped(
                    self.mirror.text(),
                    self.text_revision,
                    ctx.font_pixel_size,
                    &ctx.font_family,
                    ctx.line_spacing,
                    ctx.padding,
                    ctx.text_indent,
                    ctx.bounding_width,
                    ctx.dpr,
                    Some(&ctx.text_color),
                    gen,
                    0,
                    0,
                );
                (snap, Some(gen))
            }
        };

        // Issue #738 评论 5792244119 问题 1: 从已有 layout 提取所有行的动画视觉
        //（QImage/glyphRuns/clusters）注入到新 canonical，使 rebind 路径
        // find_clusters_in_canonical 能找到 cluster。
        let mut doc_snap = snapshot;
        let visuals_gen = prepared_handle
            .as_ref()
            .map(|h| h.generation)
            .unwrap_or_else(|| fallback_gen.unwrap_or(0));
        let visuals_lines: &[crate::editor::layout::VisualLine] =
            prepared_handle.as_ref().map(|h| h.lines).unwrap_or(&[]);
        if !visuals_lines.is_empty() {
            let visuals_handle = layout::PreparedLayoutHandle {
                generation: visuals_gen,
                lines: visuals_lines,
            };
            // 布局变化后任意行位置都可能改变，提取所有行，不漏 anchor 覆盖的行。
            let all_line_ids: Vec<usize> = (0..visuals_lines.len()).collect();
            let animation_visuals = layout::prepare_animation_visuals_from_layout(
                &visuals_handle,
                &all_line_ids,
                ctx.dpr,
                &ctx.text_color,
            );
            layout::inject_animation_visuals_into_snapshot(&mut doc_snap, animation_visuals);
        }

        // fallback 路径释放临时 generation，不泄漏。
        if let Some(gen) = fallback_gen {
            layout::clear_layout_generation(gen);
        }
        doc_snap
    }

    pub fn mirror(&self) -> &CommittedTextMirror {
        &self.mirror
    }

    /// Issue #745: 只读投影 API — 把正文状态收口为 Core EditorKernel（业务真相）
    /// 以及 CommittedTextMirror（Qt 只读/增量平台投影）。平台端不再持有第三份正文镜像。
    /// 所有读取都委托到 mirror 或 kernel，保证单一真相来源。
    pub fn committed_text(&self) -> &str {
        self.mirror.text()
    }

    pub fn cursor(&self) -> usize {
        self.mirror.cursor()
    }

    pub fn selection_anchor(&self) -> usize {
        self.mirror.selection_anchor()
    }

    pub fn has_selection(&self) -> bool {
        self.mirror.has_selection()
    }

    pub fn selection_range(&self) -> (usize, usize) {
        self.mirror.selection_range()
    }

    pub fn selected_text(&self) -> String {
        self.mirror.selected_text()
    }

    pub fn snapshot(&self) -> EditorSnapshot {
        self.mirror.snapshot()
    }

    /// 返回严格在 `byte_offset` 之前的最近 grapheme cluster 边界（UTF-8 byte offset）。
    ///
    /// 委托到 Core EditorKernel，保证 grapheme 边界由 Core 唯一决定。
    /// `byte_offset == 0` 时返回 0（无法后退）。
    pub fn previous_grapheme_boundary(&self, byte_offset: usize) -> usize {
        self.kernel.previous_grapheme_boundary(byte_offset as u32) as usize
    }

    /// 返回严格在 `byte_offset` 之后的最近 grapheme cluster 边界（UTF-8 byte offset）。
    ///
    /// 委托到 Core EditorKernel，保证 grapheme 边界由 Core 唯一决定。
    /// `byte_offset >= len` 时返回 len（无法前进）。
    pub fn next_grapheme_boundary(&self, byte_offset: usize) -> usize {
        self.kernel.next_grapheme_boundary(byte_offset as u32) as usize
    }

    pub fn composition(&self) -> &CompositionState {
        &self.composition
    }

    pub fn composition_mut(&mut self) -> &mut CompositionState {
        &mut self.composition
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
        self.animation_coordinator
            .set_typing_animation_duration_ms(ms);
    }

    pub fn set_cursor_animation_duration_ms(&mut self, ms: u32) {
        self.cursor_animation_duration_ms = ms;
        self.animation_coordinator
            .set_cursor_animation_duration_ms(ms);
    }

    /// 从 kernel snapshot 完整重建 mirror。
    ///
    /// 当 `apply_edit_result` 失败（revision 不连续、range 越界等）或
    /// `StaleRevision` 时调用，保证 mirror 与 kernel 状态一致。
    fn reload_mirror_from_kernel(&mut self) {
        self.mirror.load_from_snapshot(
            self.kernel.snapshot_text(),
            self.kernel.cursor(),
            self.kernel.revision(),
            self.kernel.selection_anchor(),
        );
    }

    /// 统一处理 `EditorEditOutcome`，把 Applied / AppliedWithAdjustedSelection /
    /// NoChange 三种分支收成同一条 mirror 更新路径。
    ///
    /// Issue #683：`NoChange` 不再跳过 mirror 更新。真正的 `NoChange` 只代表
    /// "命令没改变编辑器状态"，此时 `new_selection == old_selection`，调用
    /// `apply_edit_result` 是幂等的——但关键修复是：`SetSelection` 在 anchor/head
    /// 真变化时现在返回 `Applied`，mirror cursor 会跟着更新，不再锁死。
    ///
    /// `none_on_noop`：`true` 时 `NoChange`/`InvalidOffset`/`InvalidRange` 返回 `None`
    /// （用于 undo/redo——无操作可撤销时返回 None）；`false` 时返回 `Some(result)`
    /// （用于 insert/delete/replace/set_selection——调用方仍可拿到 result）。
    fn apply_kernel_outcome(
        &mut self,
        outcome: EditorEditOutcome,
        none_on_noop: bool,
    ) -> Option<EditorEditResult> {
        match outcome {
            EditorEditOutcome::Applied(result)
            | EditorEditOutcome::AppliedWithAdjustedSelection(result) => {
                if self.mirror.apply_edit_result(&result).is_err() {
                    self.reload_mirror_from_kernel();
                }
                Some(result)
            }
            EditorEditOutcome::NoChange(result) => {
                // Issue #683：NoChange 也走 mirror 更新（此时 new==old，幂等）。
                // 不再出现 NoChange 直接返回 Some 而不更新 mirror 的路径。
                if self.mirror.apply_edit_result(&result).is_err() {
                    self.reload_mirror_from_kernel();
                }
                if none_on_noop {
                    None
                } else {
                    Some(result)
                }
            }
            EditorEditOutcome::StaleRevision(result) => {
                self.reload_mirror_from_kernel();
                Some(result)
            }
            EditorEditOutcome::InvalidOffset(result) | EditorEditOutcome::InvalidRange(result) => {
                if none_on_noop {
                    None
                } else {
                    Some(result)
                }
            }
        }
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
        self.apply_kernel_outcome(outcome, false)
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
        self.apply_kernel_outcome(outcome, false)
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
        self.apply_kernel_outcome(outcome, false)
    }

    /// 原子 IME commit — Qt `QInputMethodEvent` 两步语义的原子执行：
    /// 1. 先删除 selection `[selection_byte_start, selection_byte_end)`；
    /// 2. 再在删完 selection 后的文本（base_text）上删除
    ///    `[replacement_byte_start, replacement_byte_end)` 并在
    ///    `replacement_byte_start` 插入 `inserted_text`。
    ///
    /// 整个操作只产生一个 revision 推进和一个 UndoEntry。
    /// 不要把 selection 删除和 replacement 拆成两次 pipeline command。
    ///
    /// `replacement_byte_start`/`replacement_byte_end` 是 base_text 坐标
    /// （删完 selection 后的文本），不是原始 committed text 坐标。
    pub fn ime_commit(
        &mut self,
        selection_byte_start: usize,
        selection_byte_end: usize,
        replacement_byte_start: usize,
        replacement_byte_end: usize,
        inserted_text: &str,
        cause: EditorTransactionCause,
    ) -> Option<EditorEditResult> {
        let command = EditorCommand::ImeCommit {
            selection_byte_range: Utf8ByteRange::clamp_rope(
                self.kernel.rope(),
                selection_byte_start,
                selection_byte_end,
            ),
            // Issue #701 评论 5704688994 问题 1: replacement_byte_range_after_selection
            // 是"删完 selection 后的 base_text 坐标"，不是原始 committed text 坐标。
            // 用原始 rope clamp 会把落在 selection UTF-8 continuation byte 里的
            // base 偏移压回 0（例：原文 `你abc`，删 `你` 后 base=`abc`，base range
            // (1,2) 合法，但原始 rope clamp 1/2 落在 `你` 的 continuation byte 里
            // 被压回 0）。这里只做 start<=end 结构归一化，真正的 char boundary /
            // 长度校验由 Core apply_ime_commit 按 base_text 坐标完成。
            // selection_byte_range 仍是原始文本坐标，clamp_rope 正确。
            replacement_byte_range_after_selection: Utf8ByteRange::from_ordered(
                replacement_byte_start,
                replacement_byte_end,
            ),
            inserted_text: inserted_text.to_string(),
            cause,
            expected_revision: EditorRevision::new(self.mirror.revision()),
        };
        let outcome = self.kernel.apply(command);
        self.apply_kernel_outcome(outcome, false)
    }

    pub fn set_selection(&mut self, anchor: usize, head: usize) -> Option<EditorEditResult> {
        let command = EditorCommand::SetSelection {
            anchor: Utf8ByteOffset::clamp_rope(self.kernel.rope(), anchor),
            head: Utf8ByteOffset::clamp_rope(self.kernel.rope(), head),
            expected_revision: EditorRevision::new(self.mirror.revision()),
        };
        let outcome = self.kernel.apply(command);
        // Issue #683：set_selection 不再出现 NoChange 直接返回 Some
        // 然后什么都不更新的路径。apply_kernel_outcome 对 NoChange 也走 mirror 更新。
        self.apply_kernel_outcome(outcome, false)
    }

    pub fn perform_undo(&mut self) -> Option<EditorEditResult> {
        let command = EditorCommand::Undo {
            expected_revision: EditorRevision::new(self.mirror.revision()),
        };
        let outcome = self.kernel.apply(command);
        self.apply_kernel_outcome(outcome, true)
    }

    pub fn perform_redo(&mut self) -> Option<EditorEditResult> {
        let command = EditorCommand::Redo {
            expected_revision: EditorRevision::new(self.mirror.revision()),
        };
        let outcome = self.kernel.apply(command);
        self.apply_kernel_outcome(outcome, true)
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

    pub fn set_current_canonical_snapshot(
        &mut self,
        snapshot: Option<crate::editor::layout::CanonicalDocumentVisualSnapshot>,
    ) {
        self.current_canonical_snapshot = snapshot;
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
                self.animation_coordinator.prepared_queue.mark_prepared(key);
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
                self.animation_coordinator.prepared_queue.mark_prepared(key);
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
                self.animation_coordinator.prepared_queue.mark_prepared(key);
            }
        }
    }

    pub fn prepare_edit_motion(
        &mut self,
        ctx: &VisualTransactionContext,
        result: &EditorEditResult,
        old: &EditorSnapshot,
        new: &EditorSnapshot,
        editor_layout: &crate::editor::layout::EditorLayout,
        cursor_owner_epoch: u64,
    ) -> Option<PreparedEditMotion> {
        // Issue #756: 文字动画与光标动画互相独立，不再把"两个独立开关同时开启"当协同：
        // - coordinated=true：文字与光标绑死，要求有效 caret motion（在
        //   transaction_builder 内部检查），caret motion 建不起来时文字动画也不启动。
        // - coordinated=false：typing_animation_enabled 只决定文字动画，
        //   smooth_cursor_enabled 只决定光标动画；任一为 true 都要构造 motion
        //  （文字动画需要排版 old/new，光标动画需要 motion 的 caret track）。
        let text_animation_enabled =
            ctx.coordinated_animation_enabled || ctx.typing_animation_enabled;
        let caret_animation_enabled =
            ctx.coordinated_animation_enabled || ctx.smooth_cursor_enabled;
        if (!text_animation_enabled && !caret_animation_enabled) || ctx.is_scrolling {
            return None;
        }
        // Issue #756: timeline 时长按本笔真正要播的内容选：只有光标动画时用平滑光标时长。
        let animation_duration_ms = if text_animation_enabled {
            self.typing_animation_duration_ms
        } else {
            self.cursor_animation_duration_ms
        };
        let mut motion = PreparedEditMotion::from_edit_result(
            result,
            &old.text,
            &new.text,
            u64::from(animation_duration_ms),
        );
        {
            let (raw_byte_start, raw_byte_end) = motion
                .inserted_range
                .or(motion.deleted_range)
                .map(|r| (r.start().value(), r.end().value()))
                .unwrap_or_else(|| {
                    let changes =
                        super::edit_motion::diff_plain_text(&motion.old_text, &motion.new_text);
                    let mut min_b = usize::MAX;
                    let mut max_b = 0usize;
                    for change in &changes {
                        match change {
                            EditorChange::Insert { index, text } => {
                                min_b = min_b.min(index.value());
                                max_b = (index.value() + text.len()).max(max_b);
                            }
                            EditorChange::Delete { index, text } => {
                                min_b = min_b.min(index.value());
                                max_b = (index.value() + text.len()).max(max_b);
                            }
                            _ => {}
                        }
                    }
                    (min_b.min(max_b), max_b)
                });

            // Issue #710 评论 5731145076 症状四/五: 当事务包含 newline（插入 "\n"
            // 或删除 "\n"）时，affected_byte range 不能只覆盖 "\n" 的 1 byte，
            // 必须扩展到换行后所有受重排影响的段落边界。
            // 之前只取 "\n" 的 1 byte range，导致 prepare_affected_paragraphs_visual_snapshot
            // 只排版 "\n" 所在段落，换行后的行重排依赖 reflow 但 reflow 只处理
            // unchanged material，文字闪烁/光标乱闪。
            // 现在用 compute_affected_paragraph_ranges 按 old/new text 段落边界扩展，
            // 确保拆开/合并段落的两边 visual lines 都进入 diff。
            //
            // Issue #710 评论 5732160521 问题 1: 不能把同一组 byte 坐标同时套给
            // old/new text。inserted_range 是新文本坐标、deleted_range 是旧文本坐标，
            // 不能互换。这里按事务类型分别传 old/new 坐标系：
            // - Insert: old 侧是插入点 (raw_byte_start, raw_byte_start)，
            //   new 侧是 inserted_range (raw_byte_start, raw_byte_end)。
            // - Delete: old 侧是 deleted_range (raw_byte_start, raw_byte_end)，
            //   new 侧是删除后落点 (raw_byte_start, raw_byte_start)。
            // - Replace/Cursor: 保守地两侧都用 (raw_byte_start, raw_byte_end)，
            //   expand_to_paragraph_boundaries 内部会做 char boundary 调整。
            let (old_edit_range, new_edit_range) = match motion.kind {
                EditorAnimationKind::Insert => (
                    (raw_byte_start, raw_byte_start),
                    (raw_byte_start, raw_byte_end),
                ),
                EditorAnimationKind::Delete => (
                    (raw_byte_start, raw_byte_end),
                    (raw_byte_start, raw_byte_start),
                ),
                EditorAnimationKind::Cursor => (
                    (raw_byte_start, raw_byte_end),
                    (raw_byte_start, raw_byte_end),
                ),
            };
            let (old_affected_start, old_affected_end, new_affected_start, new_affected_end) =
                layout::compute_affected_paragraph_ranges(
                    &motion.old_text,
                    &motion.new_text,
                    old_edit_range,
                    new_edit_range,
                );
            // affected_byte_start/end 用于 prepare_document_visual_snapshot_scoped
            // 和 prepare_affected_paragraphs_visual_snapshot，它们排版 new text，
            // 所以用 new 侧的段落边界。old 侧的段落边界由 compare_old_new_visual_lines
            // 和 fallback 路径自行处理（compare_old_new_visual_lines 用 inserted_range/
            // deleted_range 分别在 old/new 坐标系找受影响行）。
            // 但为了确保 old snapshot 也覆盖完整段落，fallback 路径用 old 侧边界。
            // 这里取 new 侧边界作为 affected_byte_start/end（用于 new_doc_snapshot 排版），
            // old 侧边界单独传给 fallback 路径。
            let affected_byte_start = new_affected_start;
            let affected_byte_end = new_affected_end;
            let old_affected_byte_start = old_affected_start;
            let old_affected_byte_end = old_affected_end;

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
            // old_doc_snapshot.cursor_rect_doc()（fallback 的 prepare_affected_paragraphs_visual_snapshot
            // 会真正排版并生成 cursor_x_map）。
            let old_cursor_byte = motion.old_selection.head.index.value();
            let old_caret_from_cache: Option<layout::CaretRect> = if old_prepared_handle.is_some() {
                editor_layout.cache().map(|snap| {
                    editor_layout.caret_rect_doc(
                        snap,
                        old_cursor_byte,
                        layout::CaretAffinity::Downstream,
                    )
                })
            } else {
                None
            };

            // Issue #658 评论 5620035970 问题 2: 不再 clear_paragraph_layout_cache()，
            // 而是分配独立 generation，与静态正文路径互不干扰。
            let new_generation = layout::begin_layout_generation();

            // Issue #658 评论 5624570557 问题 2: 先做 new 基础排版，得到 new_lines 用于比较
            // Issue #688: 动画路径需要 text_color 用于 QImage 绘制
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
                Some(&ctx.text_color),
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
                // Issue #710 评论 5731145076 症状四/五: 传扩展后的段落边界，
                // 确保换行前后的行都被标记为 raster。之前只传原始 byte range，
                // 对于 "\n" 插入/删除，只覆盖 1 byte，换行前后的行可能被漏掉。
                let diff = layout::compare_old_new_visual_lines(
                    handle.lines,
                    &new_doc_snapshot.visual_lines,
                    motion
                        .inserted_range
                        .map(|_| (new_affected_start, new_affected_end)),
                    motion
                        .deleted_range
                        .map(|_| (old_affected_start, old_affected_end)),
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
                layout::inject_animation_visuals_into_snapshot(&mut doc_snap, old_line_snapshots);

                // Issue #658 评论 5624570557 问题 1+2: 从已有 new layout 提取 new 动画视觉。
                // new_doc_snapshot 已完成基础排版（QTextLayout 存入 new_generation），
                // 从已有 QTextLine 只提取受影响行的 QImage/glyph/cluster。
                let new_handle = layout::PreparedLayoutHandle {
                    generation: new_generation,
                    lines: &new_doc_snapshot.visual_lines,
                };
                let mut new_raster_ids = diff.new_raster_line_ids.clone();
                for (rs, re) in self
                    .animation_coordinator
                    .collect_active_rebind_ranges(&motion.new_text)
                {
                    for (i, l) in new_doc_snapshot.visual_lines.iter().enumerate() {
                        if l.byte_start < re && l.byte_end > rs && !new_raster_ids.contains(&i) {
                            new_raster_ids.push(i);
                        }
                    }
                }
                let new_line_snapshots = layout::prepare_animation_visuals_from_layout(
                    &new_handle,
                    &new_raster_ids,
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
                            let byte_delta: isize =
                                new_line.byte_start as isize - snap.document_byte_start as isize;
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
                let prev_new_snapshot = self.current_canonical_snapshot.as_ref();
                layout::prepare_affected_paragraphs_visual_snapshot(
                    &motion.old_text,
                    0,
                    ctx.font_pixel_size,
                    &ctx.font_family,
                    ctx.line_spacing,
                    ctx.padding,
                    ctx.text_indent,
                    ctx.bounding_width,
                    ctx.dpr,
                    &ctx.text_color,
                    // Issue #710 评论 5731145076 症状四/五: fallback 路径排版
                    // old text，用 old 侧段落边界，确保换行前段落完整排版。
                    old_affected_byte_start,
                    old_affected_byte_end,
                    prev_new_snapshot,
                    fallback_old_generation,
                    true,
                )
            };

            let old_caret = old_caret_from_cache.unwrap_or_else(|| {
                old_doc_snapshot.cursor_rect_doc(
                    motion.old_selection.head.index.value(),
                    layout::CaretAffinity::Downstream,
                )
            });
            let new_caret = new_doc_snapshot.cursor_rect_doc(
                motion.new_selection.head.index.value(),
                layout::CaretAffinity::Downstream,
            );

            motion.old_cursor_rect = Some(make_cursor_rect_from_caret_doc(
                &old_caret,
                &old_doc_snapshot,
                &ctx.font_family,
            ));
            motion.new_cursor_rect = Some(make_cursor_rect_from_caret_doc(
                &new_caret,
                &new_doc_snapshot,
                &ctx.font_family,
            ));

            // Issue #722 评论 5749791161: 获取 from/to 端真实视觉行的 top/bottom。
            // 行几何来自 VisualLine.y 和 VisualLine.y + VisualLine.height，
            // 不是 caret 自己的 CursorRect.top/bottom（光标细矩形边界）。
            let (old_line_top, old_line_bottom) = old_doc_snapshot
                .visual_lines
                .iter()
                .find(|l| l.id == old_caret.visual_line_id)
                .map(|l| (l.y, l.y + l.height))
                .unwrap_or((0.0, 0.0));
            let (new_line_top, new_line_bottom) = new_doc_snapshot
                .visual_lines
                .iter()
                .find(|l| l.id == new_caret.visual_line_id)
                .map(|l| (l.y, l.y + l.height))
                .unwrap_or((0.0, 0.0));

            // Issue #658 评论 5626002895 问题 3: fallback old snapshot 的图片/cluster/cursor map
            // 已复制进 Rust snapshot（old_doc_snapshot），fallback_old_generation 的 QTextLayout
            // 不再需要，立即释放避免生命周期泄漏。old_doc_snapshot 后续 build_old_new_from_canonical
            // 和 previous_layout_snapshot 只消费 Rust 数据，不依赖 QTextLayout。
            if let Some(gen) = fallback_old_generation_opt {
                layout::clear_layout_generation(gen);
            }

            // Issue #735: PreparedEditMotion 不携带 insert_glyph_rects /
            // reflog_glyph_rects / deleted_glyph_rects（Core 已删除这些视觉类型）。
            // 动画纹理由 animation_coordinator 从 old/new layout snapshot 直接构建。

            let old_revision = self.layout_revision;
            let new_revision = LayoutRevision::next();

            let (old_snap, new_snap) = LineSnapshotBuilder::build_old_new_from_canonical(
                &old_doc_snapshot,
                &new_doc_snapshot,
                old_revision,
                new_revision,
                ctx.scroll_y,
                ctx.viewport_height,
                // Issue #736 评论 5777408243 问题1: cluster 的 document byte range
                // 和 snapshot 的 virtual_text 必须属于同一 revision，否则
                // animation_coordinator 取 cluster 文本会得到空串，InsertReveal 全被跳过。
                &motion.old_text,
                &motion.new_text,
            );

            // Issue #738 评论 5787277777: 在 new_doc_snapshot 已完成、创建本次新事务之前，
            // 先把所有旧活动事务从"上一份 canonical 几何"重绑到这份新 canonical，
            // 再处理本次新事务自己的 conflict/rebase。Pipeline 的 new_revision 即将成为
            // 新 canonical basis revision，直接传给 coordinator。
            //
            // Issue #738 评论 5796693007 问题1: 正文编辑路径必须先采 rebase frame/handoff
            // 再 retire CaretDriven。prepare_rebase_handoff_for_edit 在旧事务还活着时
            // 采样 rebase frame + caret handoff（采到的是真实当前帧，不是终态），
            // 取消真正被覆盖的冲突事务。reconcile 之后再 create 新事务。
            // 顺序：prepare → reconcile → create。
            // - prepare 采到的是旧事务真实当前帧（CaretDriven 还没被推到终态）。
            // - reconcile retire 旧事务 CaretDriven + rebind Timed Reflow。rebase frame 已采好，
            //   此时 retire 不影响已采的 frame。
            // - create 用保存的 handoff 创建新事务。
            // 用一个统一的 edit_now，保证 prepare 和 reconcile 用同一时刻采样。
            let edit_now = Instant::now();
            let prepared_handoff = self.animation_coordinator.prepare_rebase_handoff_for_edit(
                &motion,
                ctx.typing_animation_enabled,
                ctx.smooth_cursor_enabled,
                ctx.coordinated_animation_enabled,
                ctx.is_scrolling,
                ctx.is_loading,
                ctx.is_applying_format,
                motion.old_cursor_rect.clone(),
                motion.new_cursor_rect.clone(),
                cursor_owner_epoch,
                edit_now,
            );
            self.animation_coordinator
                .reconcile_active_transactions_with_canonical(
                    &motion.new_text,
                    &new_doc_snapshot,
                    new_revision,
                    edit_now,
                );
            // Issue #738 评论 5788513592 额外要求: reconcile 删除 unit / 完成事务后同步按
            // 剩余 active snapshot ids 收一次 texture cache，不让已经失去 owner 的纹理一直
            // 挂到后续别的完成路径才释放。
            let active_ids = self.animation_coordinator.collect_active_snapshot_ids();
            self.texture_cache.retain_active_snapshot_ids(&active_ids);

            let key = self
                .animation_coordinator
                .create_transaction_from_prepared_handoff(
                    prepared_handoff,
                    &motion,
                    // Issue #756: 文字动画 = coordinated || typing，光标动画 = coordinated || smooth。
                    // 协同模式下文字与光标绑死，caret track 必须生成；coordinated=false 时由
                    // smooth 单独决定（这就是"平滑光标"在正文编辑期间的光标动画）。
                    text_animation_enabled,
                    caret_animation_enabled,
                    ctx.coordinated_animation_enabled,
                    motion.old_cursor_rect.clone(),
                    motion.new_cursor_rect.clone(),
                    Some(old_caret.visual_line_id),
                    Some(new_caret.visual_line_id),
                    old_line_top,
                    old_line_bottom,
                    new_line_top,
                    new_line_bottom,
                    &old_snap,
                    &new_snap,
                    cursor_owner_epoch,
                    new_revision,
                );
            // Issue #738 评论 5793319451 问题1: layout_revision 必须随 canonical 推进
            // 无条件一起提交。process_transaction 在 typing animation 关闭/正在滚动/loading/
            // applying format/smooth cursor 不完整/mode 不创建事务等场景会返回 None，
            // 但此时 canonical 已推进到 new_revision、旧事务已 reconcile 到 new_revision。
            // 若 layout_revision 停在旧值，basis 守卫（已改为 ==/!=）会把"事务 revision
            // 比 Pipeline 当前 revision 更新"误当合法事务继续画。new_doc_snapshot 一旦成为
            // 当前 canonical，layout_revision 就必须无条件一起提交。
            self.layout_revision = new_revision;
            if let Some(key) = key {
                self.prepare_transaction_textures(key);
            }

            self.previous_layout_snapshot =
                Some(self.current_layout_snapshot.clone().unwrap_or_else(|| {
                    EditorLayoutSnapshot::new(
                        old_doc_snapshot.to_layout_snapshot(),
                        Vec::new(),
                        None,
                        None,
                        layout::CaretAffinity::Downstream,
                    )
                }));
            self.current_layout_snapshot = Some(new_snap);

            // Issue #658 评论 5624570557 问题 1: old generation 不在此处释放，
            // 而是存入 PromotedLayout.old_generation，在 promote 时随 new generation 一起释放。
            // 这样保证 old 动画纹理在 new prepared layout 成为 current 之前一直有效。

            // 先 clone visual_lines 用于 PromotedLayout，再 move new_doc_snapshot 到 current_canonical_snapshot。
            let promoted_visual_lines = new_doc_snapshot.visual_lines.clone();
            // Issue #738 评论 5789470425 问题1: 新 canonical 保存为 current_canonical_snapshot。
            // prepare_edit_motion 在生成新事务前已用上一份 current_canonical reconcile 旧事务，
            // 再把这份新 canonical 提升为 current。
            self.current_canonical_snapshot = Some(new_doc_snapshot);

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
                    "prepare_edit_motion: processed via canonical document snapshot pipeline, kind={:?}, has_active_insert={}",
                    motion.kind,
                    self.animation_coordinator.has_active_insert()
                ));
        }

        Some(motion)
    }
}

fn make_cursor_rect_from_caret_doc(
    caret: &layout::CursorLayoutRect,
    doc_snapshot: &layout::CanonicalDocumentVisualSnapshot,
    font_family: &str,
) -> CursorRect {
    let line = doc_snapshot
        .visual_lines
        .iter()
        .find(|l| l.id == caret.visual_line_id);
    // Issue #722 评论 5748596920 问题1: caret track 保存文档坐标（不减 scroll_y），
    // 与 AnimatedSlice/StaticPatch 文档坐标系一致。scene graph 在渲染时按当前
    // scroll_y 做 viewport transform。
    let baseline_y = match line {
        Some(l) => layout::text_baseline_y(l, doc_snapshot.font_size, font_family),
        None => caret.y + caret.h * 0.8,
    };
    CursorRect {
        x: caret.x,
        top: caret.y,
        bottom: caret.y + caret.h,
        baseline_y,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Issue #683 复现 6（行为测试版）：验证 set_selection 后 mirror.cursor 正确更新，
    /// 后续 delete_range 能删除旧正文中的字符，而非因光标锁死删除错误位置。
    ///
    /// 链路：load "abcdef" → set_selection(3,3) → mirror.cursor == 3 →
    /// delete_range(2,3) → "abdef"
    #[test]
    fn set_selection_updates_mirror_cursor_then_delete_removes_correct_char() {
        let mut pipeline = LinuxEditorPipeline::new();
        assert!(pipeline.load_text("abcdef".to_string(), 6));

        // 光标初始在末尾 (6)。
        assert_eq!(pipeline.mirror().cursor(), 6);

        // 把光标移到 3（'c' 后面）。
        pipeline.set_selection(3, 3);

        // mirror.cursor 必须跟着更新到 3，否则光标锁死。
        assert_eq!(
            pipeline.mirror().cursor(),
            3,
            "set_selection(3,3) 后 mirror.cursor 应为 3，实际 {} — 光标锁死",
            pipeline.mirror().cursor()
        );

        // 删除 [2,3) 即 'c'，应得到 "abdef"。
        pipeline.delete_range(2, 3, EditorTransactionCause::Delete);
        assert_eq!(
            pipeline.mirror().text(),
            "abdef",
            "删除 'c' 后应为 'abdef'，实际 {:?}",
            pipeline.mirror().text()
        );
    }
}
