// Issue #815 评论 6042062633 修改 8: 输入路径"编辑发生了但没有动画"的正式跳过事件。
use super::edit_motion::{CompositionSession, CursorRect};
use super::edit_snapshot::EditorSnapshot;
use super::layout_revision::LayoutRevision;
use super::layout_snapshot::{EditorLayoutSnapshot, PreparedLineSnapshot};
use super::text_motion::TextMotion;
use super::text_utils::{clamp_to_char_boundary, normalize_plain_text};
use super::texture_cache::{LineSnapshotId, TextureCache};
use super::PreeditAttribute;
use crate::editor::layout;
use crate::platform::linux_qt::LinuxQtClipboardFocusAdapter;
use qmetaobject::QObject;
use writer_core::editor::{
    DisplayPatch, EditorCommand, EditorEditOutcome, EditorEditResult, EditorKernel, EditorRevision,
    EditorTransactionCause, Utf8ByteOffset, Utf8ByteRange,
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
    pub was_composing: bool,
    pub preedit_byte_start: usize,
    pub preedit_byte_end: usize,
    pub saved_virtual_text: String,
    pub session_replace_start: usize,
    pub session_replace_end: usize,
    pub candidate_byte_start: usize,
    pub candidate_byte_end: usize,
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
    /// 视觉画布——统一接收 Core 编辑结果，产出整体绘制计划（Issue #901）。
    /// Issue #904：本帧唯一需要动画的局部文字片段（吐字/吞字/位移）。
    ///
    /// 取代 #901 的整章 `visual_canvas`。只有一个 owner，只有一个进度；
    /// 没有 FrameClock/RevisionChain/Ownership/PresentedScene 第二套状态机。
    text_motion: Option<TextMotion>,
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
    /// 布局修订——宽度/字号/字体/行距等变化时递增
    layout_revision: LayoutRevision,
    /// Issue #658 评论 5622829886 问题 1: record_visual_transaction 全篇排版 new text
    /// 后把 new prepared layout 存在此处，由 record_transaction 取出交给
    /// EditorLayout::promote_prepared_layout 提升为 current，
    /// 后续 EditorLayout::snapshot 不再重新排版同一 new text。
    pending_promoted_layout: Option<crate::editor::layout::PromotedLayout>,
}

/// Issue #819 评论 5967250411 问题 3：Linux 平台内部的编辑结果分类。
///
/// `apply_kernel_outcome` 把 Core `EditorEditOutcome` 拍扁成 `Option<EditorEditResult>`，
/// 在 `none_on_noop=false` 时对 Applied/NoChange/StaleRevision/InvalidOffset/InvalidRange
/// 都返回 `Some(result)`，调用方无法区分"真的 Applied"和"NoChange/StaleRevision"。
/// `edit_flow.rs` 用 `is_some()` 判 Applied 会把 NoChange/StaleRevision 误判成 Applied。
///
/// 本枚举保留 Core 的完整分类：只有 `Applied` / `AppliedWithAdjustedSelection` 才算
/// 真正应用了编辑，`NotApplied` 带上具体的 kind 供 edit_flow 区分跳过原因。
#[derive(Clone, Debug)]
pub(crate) enum PipelineEditOutcome {
    /// Core 真正应用了编辑（Applied / AppliedWithAdjustedSelection）。
    Applied(EditorEditResult),
    /// Core 没有应用编辑（NoChange / StaleRevision / InvalidOffset / InvalidRange）。
    NotApplied {
        kind: PipelineEditOutcomeKind,
        result: EditorEditResult,
    },
}

/// `PipelineEditOutcome::NotApplied` 的具体原因。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PipelineEditOutcomeKind {
    NoChange,
    StaleRevision,
    InvalidOffset,
    InvalidRange,
}

/// Issue #819 评论 5968240881 问题 2：`prepare_edit_motion` 的明确结果。
///
/// 每个跳过点原地返回自己的 `EditVisualSkipReason`，调用方只透传、不重新猜测原因。
pub(crate) enum VisualPrepareOutcome {
    /// Issue #853: 由单一 VisualEditState 创建了正文视觉过渡。
    ///
    /// 不再携带事务 key——新模型没有 prepared transaction 队列。
    Created,
    /// Core edit 已应用但视觉动画未启动，附带具体跳过原因。
    Skipped(super::edit_flow::EditVisualSkipReason),
    /// 动画未请求（三个开关全关），Core edit 已应用但无视觉动画。
    AnimationDisabled,
}

/// Issue #904 评论 4：局部动画源纹理的准备结果。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LocalTextTexturePrep {
    /// 本次准备好（cache 中已存在或刚刚生成/插入）的源行快照 id。
    pub prepared: Vec<LineSnapshotId>,
    /// 本计划真正需要、但拿不到 image 的源行快照 id（显式失败，不静默降级）。
    pub missing: Vec<LineSnapshotId>,
}

/// Issue #904 评论 5：产出某个 `EditorLayoutSnapshot` 的 Qt 布局事实源。
///
/// 只由同一次排版在快照建立处产出，与该版快照一一对应。`lines` 是该 generation
/// 下已物化为文档绝对坐标的视觉行，可把 `LineSnapshotId`（段落 logical slot +
/// 段内视觉行号）定位到 QTextLine 并取出真实行图。
///
/// 不变量：与 `lines` 配套的 generation 一旦被下一代布局**原地重排覆盖**就不再
/// 能复现旧字形；那种快照不能携带本句柄，只能用快照里已冻结的 image。
pub(crate) struct LocalLayoutSource {
    /// 产出这些行图的 Qt 布局 generation。
    pub generation: u64,
    /// 与该 generation 同源的物化视觉行。
    pub lines: std::sync::Arc<
        crate::editor::layout::chunked_vec::PersistentChunkedVec<crate::editor::layout::VisualLine>,
    >,
}

/// Issue #904 评论 5：一侧局部动画的视觉事实源。
///
/// old / new 两侧各自传入自己的快照。行图只能取自 `snapshot` 里在产出该版布局时
/// 冻结的真实 image；`layout` 只有在与 `snapshot` 是**同一版** Qt 布局时才存在。
/// 旧版快照恒以 `layout: None` 传入：缺图显式记入 `missing`，绝不用当前 live
/// `EditorLayout` cache（已是另一版正文）冒充。
pub(crate) struct LocalTextSource<'a> {
    pub snapshot: &'a EditorLayoutSnapshot,
    pub layout: Option<&'a LocalLayoutSource>,
}

impl LocalTextTexturePrep {
    pub fn ready(&self) -> bool {
        self.missing.is_empty()
    }

    /// Issue #904 评论 4：合并 old/new 两侧的准备结果。
    ///
    /// 任一侧 `prepared` 即算准备好；`required_ids` 中两侧都没能准备好的算 missing
    /// （显式失败，不静默降级）。`required_ids` 决定顺序，保证结果稳定可读。
    pub fn merge_sides(required_ids: &[LineSnapshotId], sides: &[&Self]) -> Self {
        let mut merged = Self::default();
        for &id in required_ids {
            if merged.prepared.contains(&id) || merged.missing.contains(&id) {
                continue;
            }
            if sides.iter().any(|side| side.prepared.contains(&id)) {
                merged.prepared.push(id);
            } else {
                merged.missing.push(id);
            }
        }
        merged
    }
}

impl LinuxEditorPipeline {
    pub fn new() -> Self {
        Self {
            kernel: EditorKernel::new(),
            mirror: CommittedTextMirror::new(),
            composition: CompositionState::new(),
            text_motion: None,
            texture_cache: TextureCache::new(),
            clipboard_adapter: LinuxQtClipboardFocusAdapter::new(),
            text_revision: 0,
            visual_revision: 0,
            typing_animation_duration_ms: 160,
            cursor_animation_duration_ms: 120,
            current_layout_snapshot: None,
            layout_revision: LayoutRevision::initial(),
            pending_promoted_layout: None,
        }
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

    pub fn text_motion(&self) -> Option<&TextMotion> {
        self.text_motion.as_ref()
    }

    pub fn text_motion_mut(&mut self) -> &mut Option<TextMotion> {
        &mut self.text_motion
    }

    /// 取出本帧动画，若已完成或不存在则返回 `None`。
    pub fn take_finished_text_motion(&mut self, now_ms: f64) -> Option<TextMotion> {
        if let Some(motion) = self.text_motion.as_ref() {
            if motion.sample(now_ms).finished {
                return self.text_motion.take();
            }
        }
        None
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

    /// 视觉帧序号（诊断/订阅用）；正文动画由唯一的 `text_motion` 计划管理。
    pub fn visual_revision(&self) -> u64 {
        self.visual_revision
    }

    pub fn bump_visual_revision(&mut self) {
        self.visual_revision = self.visual_revision.wrapping_add(1);
    }

    pub fn bump_text_revision(&mut self) {
        self.text_revision = self.text_revision.wrapping_add(1);
    }

    /// Issue #901 第五轮阻塞4：用 Core `EditorEditResult.new_revision` 直接设置
    /// text_revision，使排版版本在 Core 编辑完成时立即一致。
    ///
    /// 普通编辑流程是先 Core edit → `record_transaction`（调 `set_text_revision`）
    /// → 后续 `emit_content_changed` 只做 UI 通知。如果 `record_transaction` 内部
    /// 用尚未更新的 `text_revision()` 调 `EditorLayout::snapshot`，等字节长度替换
    ///（text_ptr + text_len 不变）会命中 cache 直接返回旧布局，导致中段同宽替换/
    /// IME 替换的光标、字形和正文版本脱节。
    ///
    /// 此方法在 `record_transaction` 建立增量目标**之前**调用，把 text_revision
    /// 提交到 Core 的 new_revision，使 `snapshot` 收到的 revision 一定和 cache
    /// 中的不同（cache 中是旧 revision），强制走重排路径。
    ///
    /// Issue #901 第六轮阻塞1: `emit_content_changed` 不再 `bump_text_revision`，
    /// `set_text_revision` 是 text_revision 唯一的推进入口。所有编辑路径
    ///（record_transaction / IME commit / Load）都通过它绑定 Core Applied 的
    /// new_revision，确保 EditorLayout cache、    /// current_layout_snapshot、TargetLayoutDescriptor 同一次编辑全部绑定同一
    /// Core revision。
    pub fn set_text_revision(&mut self, revision: u64) {
        self.text_revision = revision;
    }

    /// 保留最后成功显示帧和当前视觉过渡引用的行纹理。
    pub fn retain_active_snapshot_ids(&mut self, active_ids: &[LineSnapshotId]) {
        self.texture_cache.retain_active_snapshot_ids(active_ids);
    }

    /// Issue #904：typing 动画时长是正文局部吞吐动画的唯一响应时长。
    /// 直接写进本地唯一 `TextMotion` 的时长，不再有第二套硬编码速度。
    pub fn set_typing_animation_duration_ms(&mut self, ms: u32) {
        self.typing_animation_duration_ms = ms;
    }

    /// Issue #904：cursor 动画时长控制光标 Tween 的响应速度。
    /// 光标动画由 `CursorController` 独立推进；这里只保存用户设置，
    /// 不再错误地用 `ms > 0` 覆写正文光标开关。
    pub fn set_cursor_animation_duration_ms(&mut self, ms: u32) {
        self.cursor_animation_duration_ms = ms;
    }

    pub fn typing_animation_duration_ms(&self) -> u32 {
        self.typing_animation_duration_ms
    }

    pub fn cursor_animation_duration_ms(&self) -> u32 {
        self.cursor_animation_duration_ms
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

    /// Issue #819 评论 5967250411 问题 3：保留 Core `EditorEditOutcome` 完整分类的
    /// `apply_kernel_outcome` 变体。返回 `PipelineEditOutcome`，让 edit_flow 能区分
    /// Applied 和 NoChange/StaleRevision。
    ///
    /// mirror 更新逻辑与 `apply_kernel_outcome` 完全一致，只是返回值不拍扁。
    fn apply_kernel_outcome_typed(&mut self, outcome: EditorEditOutcome) -> PipelineEditOutcome {
        match outcome {
            EditorEditOutcome::Applied(result)
            | EditorEditOutcome::AppliedWithAdjustedSelection(result) => {
                if self.mirror.apply_edit_result(&result).is_err() {
                    self.reload_mirror_from_kernel();
                }
                PipelineEditOutcome::Applied(result)
            }
            EditorEditOutcome::NoChange(result) => {
                if self.mirror.apply_edit_result(&result).is_err() {
                    self.reload_mirror_from_kernel();
                }
                PipelineEditOutcome::NotApplied {
                    kind: PipelineEditOutcomeKind::NoChange,
                    result,
                }
            }
            EditorEditOutcome::StaleRevision(result) => {
                self.reload_mirror_from_kernel();
                PipelineEditOutcome::NotApplied {
                    kind: PipelineEditOutcomeKind::StaleRevision,
                    result,
                }
            }
            EditorEditOutcome::InvalidOffset(result) => PipelineEditOutcome::NotApplied {
                kind: PipelineEditOutcomeKind::InvalidOffset,
                result,
            },
            EditorEditOutcome::InvalidRange(result) => PipelineEditOutcome::NotApplied {
                kind: PipelineEditOutcomeKind::InvalidRange,
                result,
            },
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
                // Issue #904：正文动画由唯一的 `text_motion` 计划管理，无第二套状态机。
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
    ) -> PipelineEditOutcome {
        let command = EditorCommand::Insert {
            byte_offset: Utf8ByteOffset::clamp_rope(self.kernel.rope(), byte_offset),
            text: text.to_string(),
            cause,
            expected_revision: EditorRevision::new(self.mirror.revision()),
        };
        let outcome = self.kernel.apply(command);
        self.apply_kernel_outcome_typed(outcome)
    }

    pub fn delete_range(
        &mut self,
        byte_start: usize,
        byte_end_exclusive: usize,
        cause: EditorTransactionCause,
    ) -> PipelineEditOutcome {
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
        self.apply_kernel_outcome_typed(outcome)
    }

    pub fn replace_range(
        &mut self,
        byte_start: usize,
        byte_end_exclusive: usize,
        replacement: &str,
        cause: EditorTransactionCause,
    ) -> PipelineEditOutcome {
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
        self.apply_kernel_outcome_typed(outcome)
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
    ) -> PipelineEditOutcome {
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
        self.apply_kernel_outcome_typed(outcome)
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
        let was_composing = self.composition.is_composing();
        let saved_virtual_text = self.composition.virtual_text();
        let (session_replace_start, session_replace_end) =
            self.composition.session_replace_range(fallback_cursor);
        let candidate_byte_start = session_replace_start;
        let candidate_byte_end = session_replace_start + inserted_text.len();

        self.composition.clear_preedit_fields();

        CompositionCommitResult {
            was_composing,
            preedit_byte_start,
            preedit_byte_end,
            saved_virtual_text,
            session_replace_start,
            session_replace_end,
            candidate_byte_start,
            candidate_byte_end,
        }
    }

    pub fn finish_composition_commit(&mut self) {
        self.composition.finish_session();
    }

    pub fn current_layout_snapshot(&self) -> &Option<EditorLayoutSnapshot> {
        &self.current_layout_snapshot
    }

    /// Issue #904：从当前布局快照的 line_snapshots 提取所有非 none 的
    /// LineSnapshotId，作为纹理生命周期 retain 的 active ids。
    ///
    /// 局部动画的旧行纹理不再走这里，而是由 `TextMotion::source_snapshot_ids()`
    /// 显式持有；这里只保留当前 canonical 布局的行。
    pub fn collect_active_snapshot_ids_from_layout(&self) -> Vec<LineSnapshotId> {
        let mut ids: Vec<LineSnapshotId> = Vec::new();
        if let Some(snap) = self.current_layout_snapshot.as_ref() {
            for line in &snap.line_snapshots {
                if !line.id.is_none() && !ids.contains(&line.id) {
                    ids.push(line.id);
                }
            }
        }
        ids
    }

    pub fn set_current_layout_snapshot(&mut self, snapshot: Option<EditorLayoutSnapshot>) {
        self.current_layout_snapshot = snapshot;
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
}

impl super::SujianEditorItem {
    /// Issue #904 评论 5：按“本次局部动画真正引用的源 LineSnapshotId”准备纹理。
    ///
    /// 不再用编辑字节范围猜几行：调用方先把 TextMotion 的 pieces 建完，再把它
    /// 真正引用的源快照 id 交进来，并显式给出这一侧的视觉事实源 `source`
    ///（old / new 各自传入）。
    ///
    /// 准备顺序（每一侧都独立成立）：
    /// 1. 快照里在产出该版布局时冻结的 `image` —— 这是唯一能证明“就是那一版字形”
    ///    的材料，old 侧靠它拿到真正的删除前字形；
    /// 2. 快照缺图时，只有 `LocalTextSource::layout` 与快照同版才允许用它补图；
    ///    old 侧恒为 `None`，缺图显式记入 `missing`，绝不用另一版正文冒充。
    ///
    /// 注：补图需要 Qt 布局句柄，故实现挂在 `SujianEditorItem` 上（`pipeline.rs` 内），
    /// 而不是只持有纹理缓存的 `LinuxEditorPipeline`。
    pub(crate) fn prepare_local_text_textures(
        &mut self,
        source: &LocalTextSource<'_>,
        required_ids: &[LineSnapshotId],
    ) -> LocalTextTexturePrep {
        let mut prep = LocalTextTexturePrep::default();
        let mut seen: Vec<LineSnapshotId> = Vec::new();
        for &id in required_ids {
            if id.is_none() || seen.contains(&id) {
                continue;
            }
            seen.push(id);

            if self.pipeline.texture_cache.contains_line(&id) {
                prep.prepared.push(id);
                continue;
            }

            // 本源快照里没有这个 id：本侧忽略（另一侧可能持有该源行），不伪造、不报错。
            let Some(line) = source.snapshot.line_snapshots.iter().find(|l| l.id == id) else {
                continue;
            };

            if let Some(image) = line.image.as_ref() {
                self.pipeline.texture_cache.insert_line(id, image.clone());
                prep.prepared.push(id);
                continue;
            }

            // 快照缺图：只允许用**与快照同一版**的 Qt 布局补图。
            match self.generate_local_line_image(source, line) {
                Some(image) => {
                    self.pipeline.texture_cache.insert_line(id, image);
                    prep.prepared.push(id);
                }
                None => prep.missing.push(id),
            }
        }
        prep
    }

    /// Issue #904 评论 5：用**与快照同一版**的 Qt 布局给缺图快照行补行图。
    ///
    /// 只用 `source.layout`（产出该快照那一版的 generation + 物化视觉行）：
    /// 按 `LineSnapshotId`（段落 logical slot + 段内视觉行号）在源视觉行里定位，
    /// 再用 byte range 二次确认是同一行。同一版布局由 `LocalLayoutSource` 唯一标识，
    /// 因此取出的字形必定属于该快照版本。
    ///
    /// `layout: None`（旧版布局已被下一代原地重排覆盖）、定位不到或 byte range
    /// 不一致时返回 `None`，由调用方记入 missing。**绝不**回退到当前 live
    /// `editor_layout.cache()`：那时缓存已是另一版正文，用 byte range 相等会把新字
    /// 冒充成旧字。
    fn generate_local_line_image(
        &self,
        source: &LocalTextSource<'_>,
        snapshot_line: &PreparedLineSnapshot,
    ) -> Option<qmetaobject::QImage> {
        let layout = source.layout?;
        let id = snapshot_line.id;

        let line_index = source_line_index_in_layout(
            &layout.lines,
            id,
            snapshot_line.byte_start,
            snapshot_line.byte_end,
        )?;

        let item_ptr = self.get_cpp_object();
        let dpr = if item_ptr.is_null() {
            1.0
        } else {
            crate::editor::renderer::sujian_item_dpr(item_ptr)
        };
        let text_color = self.current_text_color.to_string();

        // 同帧临时 handle：generation 是产出该快照的布局代，布局在本帧存活，
        // 不需要持有 lease（与 layout_ops.rs 的动画提取一致）。
        let handle = crate::editor::layout::PreparedLayoutHandle {
            generation: layout.generation,
            lines: std::sync::Arc::clone(&layout.lines),
            leases: Vec::new(),
        };
        crate::editor::layout::prepare_animation_visuals_from_layout(
            &handle,
            &[line_index],
            dpr,
            &text_color,
        )
        .into_iter()
        .find_map(|visual| visual.image)
    }
}

/// Issue #904 评论 5：按 `LineSnapshotId`（段落 logical slot + 段内视觉行号）在
/// 一版物化视觉行里定位对应的行索引。
///
/// 视觉行按文档次序排列，同段落的行连续。用 `logical_slot` 匹配段落、按出现次序
/// 数到 `visual_line_ordinal`，再用非空正文 + byte range 确认确实是该快照行。
/// 定位不到或 byte range 不一致时返回 `None`（调用方显式记 missing）。
fn source_line_index_in_layout(
    lines: &crate::editor::layout::chunked_vec::PersistentChunkedVec<
        crate::editor::layout::VisualLine,
    >,
    id: LineSnapshotId,
    byte_start: usize,
    byte_end: usize,
) -> Option<usize> {
    let mut seen_ordinal: u32 = 0;
    for index in 0..lines.len() {
        let Some(line) = lines.get(index) else {
            continue;
        };
        if line.logical_slot as u64 != id.paragraph_id {
            continue;
        }
        if seen_ordinal == id.visual_line_ordinal {
            if !line.para_text.is_empty()
                && line.byte_start == byte_start
                && line.byte_end == byte_end
            {
                return Some(index);
            }
            return None;
        }
        seen_ordinal += 1;
    }
    None
}

// Issue #808 评论 5918236360: 内嵌测试模块超过生产文件结构上限，测试拆到
// `pipeline/tests.rs`；本模块仍在 `pipeline` 内部，可直接访问私有条目。
#[cfg(test)]
mod tests;
