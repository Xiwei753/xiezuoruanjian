//! Issue #826: 正文「吞字吐字」的单一遮罩前沿。
//!
//! 正文永远立即变成最新真实内容。本模块**只**负责一件事：控制「本轮改动的这段
//! 文字现在露出多少」。
//!
//! - 吐字（Insert）：最新 canonical 正文只画一份；本轮新增范围里落在前沿**之后**的
//!   部分被遮罩裁掉，前沿沿 changed range 的**视觉路径**逐步打开到最新目标。
//! - 吞字（Delete）：canonical 正文立即没有这些字；额外画一份本轮删除开始前的旧正文
//!   overlay，前沿沿旧正文的视觉路径逐步把它收掉。
//! - 替换（Replace）：旧 overlay 收掉 + 新字 mask 打开，共用**同一个时间 progress**。
//!
//! 连续同方向输入/删除**只更新同一个前沿对象**：先采样当前前沿，把采样出的「已走过
//! 距离」继承下来，再重建最新路径。不复制上一笔动画，不携带历史 Reveal/Conceal，不排队。
//!
//! Issue #826 评论 7 阻塞 2：前沿**不是**屏幕上斜着飞的一个二维矩形。
//! 「单一前沿」= 单一时间 progress + 单一 active state + 一条按视觉顺序排好的
//! 文本路径。自动换行时路径自然是「第一行剩余部分走完 → 下一行从左侧开始 → 再向右走」，
//! 不会斜穿屏幕导致新字突然整块出现。
//!
//! 本模块不拥有光标动画，也不处理 IME preedit。

use std::time::Instant;

use writer_core::editor::OffsetMap;

use crate::sujian_editor_item::layout_snapshot::{
    EditorLayoutSnapshot, LineSnapshotId, PreparedLineSnapshot, SourceRect,
};

/// Issue #826: 本轮正文改动的前沿种类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EditFrontierKind {
    /// 只新增。
    Insert,
    /// 只删除。
    Delete,
    /// 同时有删除和插入。
    Replace,
}

impl EditFrontierKind {
    /// 是否需要在 canonical 正文之外额外画旧正文 overlay。
    pub(crate) fn needs_old_overlay(self) -> bool {
        matches!(self, EditFrontierKind::Delete | EditFrontierKind::Replace)
    }

    /// 是否需要遮罩 canonical 正文里的新字。
    pub(crate) fn needs_new_mask(self) -> bool {
        matches!(self, EditFrontierKind::Insert | EditFrontierKind::Replace)
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            EditFrontierKind::Insert => "Insert",
            EditFrontierKind::Delete => "Delete",
            EditFrontierKind::Replace => "Replace",
        }
    }

    /// 连续编辑能否并入当前前沿。
    ///
    /// 同一种类才能并入：吐字不能并入吞字前沿，反之亦然。
    pub(crate) fn can_extend(self, other: EditFrontierKind) -> bool {
        self == other
    }
}

/// Issue #826 评论 7：一段视觉路径上的一个视觉行片段。
///
/// 一个 segment 是「某个视觉行上，changed range 覆盖的那段横向区间」。
/// 路径按视觉顺序把若干 segment 串起来，前沿沿路径推进而不是在屏幕上斜飞。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct FrontierSegment {
    /// 这一段属于哪条视觉行（用 `line.id` 精确匹配，不用 y 近似）。
    pub line_id: LineSnapshotId,
    /// 该视觉行的文档坐标上边界。
    pub y: f64,
    /// 该视觉行的高度。
    pub h: f64,
    /// changed range 在这一行上的最左文档坐标。
    pub x_start: f64,
    /// changed range 在这一行上的最右文档坐标。
    pub x_end: f64,
    /// 这一段的横向长度。
    pub visual_length: f64,
}

/// Issue #826 评论 7：一条按视觉顺序排好的 changed range 视觉路径。
///
/// `total_length` 是所有 segment 长度之和；前沿的「已走过距离」就在这条路径上度量。
/// 跨行、跨自动换行、IME 一次提交跨行都自然由多段路径表达。
#[derive(Clone, Debug, Default)]
pub(crate) struct FrontierPath {
    pub segments: Vec<FrontierSegment>,
    pub total_length: f64,
}

impl FrontierPath {
    /// 从 snapshot 里 `range` 覆盖的 cluster 按视觉顺序构建路径。
    ///
    /// 每个视觉行产出一个 segment；该行没有 cluster（如换行符）时**不**产出
    /// segment——换行没有可见 glyph，就不该让前沿为它花掉行程。
    pub(crate) fn build(snapshot: &EditorLayoutSnapshot, range: (usize, usize)) -> Self {
        let mut segments: Vec<FrontierSegment> = Vec::new();
        for line in snapshot.lines_in_byte_range(range.0, range.1) {
            let mut left = f64::MAX;
            let mut right = f64::MIN;
            for cluster in line.clusters_in_byte_range(range.0, range.1) {
                let rect = line.source_rect_to_document_rect(&cluster.source_rect);
                left = left.min(rect.x);
                right = right.max(rect.x + rect.w);
            }
            if left >= right {
                // 这一行没有可见 glyph（换行符、空段落），不产出 segment。
                continue;
            }
            segments.push(FrontierSegment {
                line_id: line.id,
                y: line.visual_line_top,
                h: line.visual_line_bottom - line.visual_line_top,
                x_start: left,
                x_end: right,
                visual_length: right - left,
            });
        }
        let total_length = segments.iter().map(|s| s.visual_length).sum();
        Self {
            segments,
            total_length,
        }
    }

    /// 某条视觉行在路径里的段序号。
    pub(crate) fn segment_index_for_line(&self, line_id: LineSnapshotId) -> Option<usize> {
        self.segments.iter().position(|s| s.line_id == line_id)
    }

    /// 吐字：本帧每一段的「已打开到哪个 x」。
    ///
    /// 语义：从 `x_start` 向 `x_end` 打开。返回的 `x_start` 表示完全没打开，
    /// `x_end` 表示完全打开。
    pub(crate) fn reveal_boundaries(&self, distance: f64) -> Vec<f64> {
        let mut remaining = distance;
        self.segments
            .iter()
            .map(|seg| {
                let take = remaining.clamp(0.0, seg.visual_length);
                remaining = (remaining - seg.visual_length).max(0.0);
                seg.x_start + take
            })
            .collect()
    }

    /// 吞字：本帧每一段的「还保留到哪个 x」。
    ///
    /// 语义：从 `x_end` 向 `x_start` 收（向 caret 方向坍缩）。返回 `x_end` 表示
    /// 整段还完整可见（progress=0），返回 `x_start` 表示整段已被吞掉。
    pub(crate) fn conceal_boundaries(&self, distance: f64) -> Vec<f64> {
        let mut remaining = distance;
        self.segments
            .iter()
            .map(|seg| {
                let take = remaining.clamp(0.0, seg.visual_length);
                remaining = (remaining - seg.visual_length).max(0.0);
                seg.x_end - take
            })
            .collect()
    }
}

/// Issue #826: 唯一的正文改动前沿状态。
///
/// - `base_snapshot`：连续 burst 开始前的旧正文。只有 Delete / Replace 的
///   overlay 需要它（overlay 必须画「本轮连续删除开始前真正需要显示的旧文字」）。
/// - `target_snapshot`：当前最新正文。吐字的新字、吞字后回流的位置都取自它。
/// - `old_range`：旧正文坐标系里本轮被删掉的范围（base 坐标系）。
/// - `new_range`：最新正文坐标系里本轮新增的范围（最新 target 坐标系）。
/// - `reveal_path` / `conceal_path`：分别是 new_range 在 target 排版上、
///   old_range 在 base 排版上的视觉路径。
/// - `reveal_travelled` / `conceal_travelled`：本轮 burst 已经走过的距离。
///   连续编辑时继承上一帧的采样值，所以「已经吐出来的字」不会因为下一笔而回退。
#[derive(Clone, Debug)]
pub(crate) struct EditFrontierState {
    pub kind: EditFrontierKind,
    pub base_snapshot: EditorLayoutSnapshot,
    pub target_snapshot: EditorLayoutSnapshot,
    pub old_range: Option<(usize, usize)>,
    pub new_range: Option<(usize, usize)>,
    pub(crate) reveal_path: FrontierPath,
    pub(crate) conceal_path: FrontierPath,
    pub(crate) reveal_travelled: f64,
    pub(crate) conceal_travelled: f64,
    pub started_at: Instant,
    pub duration_ms: u64,
    /// Issue #826 评论 3 问题 1/2：这一轮 burst **开始前**的正文纯文本。
    ///
    /// `old_range` 一直用这个坐标系，所以连续吞字时必须靠它把每次编辑的
    /// old range 映射回同一个基准（`extend_delete`）。
    pub base_text: String,
    /// 上一次 `target_snapshot` 对应的正文纯文本。
    ///
    /// `new_range` 用这个坐标系，所以连续吐字时必须靠它把已累计的 new range
    /// 映射到最新 target 坐标（`extend_insert`）。
    pub target_text: String,
}

/// Issue #826: 前沿的一帧采样结果。
#[derive(Clone, Copy, Debug)]
pub(crate) struct EditFrontierSample {
    pub kind: EditFrontierKind,
    /// 0.0 = 改动一点都没露出；1.0 = 改动全部露出。
    pub progress: f64,
    /// 吐字路径上已走过的距离。
    pub(crate) reveal_distance: f64,
    /// 吞字路径上已走过的距离。
    pub(crate) conceal_distance: f64,
    pub old_range: Option<(usize, usize)>,
    pub new_range: Option<(usize, usize)>,
}

impl EditFrontierSample {
    /// 采样结果里前沿之后（还没露出）的部分是否需要遮罩 canonical 新字。
    pub(crate) fn masks_new_text(self) -> bool {
        self.progress < 1.0 && self.kind.needs_new_mask()
    }

    /// 采样结果里是否还需要画旧正文 overlay。
    ///
    /// Issue #826 评论 3 问题 4：吞字第一帧（progress=0）旧字必须**完整可见**，
    /// 随后才被前沿逐步收掉；progress=1 才完全消失。用 `progress > 0.0` 会造成
    /// 「第一帧不画旧字、下一帧旧字又出现」的首帧闪烁窗口。
    pub(crate) fn needs_old_overlay(self) -> bool {
        self.progress < 1.0 && self.kind.needs_old_overlay()
    }
}

/// 一条前沿驱动的裁剪 / overlay 矩形（文档坐标）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct FrontierRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl FrontierRect {
    fn is_degenerate(self) -> bool {
        !(self.w > 0.0 && self.h > 0.0)
    }
}

/// ease-out-cubic：前沿起步快、收尾慢。
fn ease_out_cubic(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

impl EditFrontierState {
    /// 开始一轮吐字。
    pub(crate) fn begin_insert(
        target_snapshot: EditorLayoutSnapshot,
        target_text: String,
        new_range: (usize, usize),
        started_at: Instant,
        duration_ms: u64,
    ) -> Self {
        let reveal_path = FrontierPath::build(&target_snapshot, new_range);
        Self {
            kind: EditFrontierKind::Insert,
            // 纯吐字不需要旧正文 overlay，base_snapshot 与 target 相同。
            base_snapshot: target_snapshot.clone(),
            target_snapshot,
            old_range: None,
            new_range: Some(new_range),
            reveal_path,
            conceal_path: FrontierPath::default(),
            reveal_travelled: 0.0,
            conceal_travelled: 0.0,
            started_at,
            duration_ms: duration_ms.max(1),
            // 纯吐字没有「删除开始前」的正文，base_text 与 target_text 同值，
            // 只是为了让两个坐标系字段语义统一、连续吐字时不会误用 base 坐标。
            base_text: target_text.clone(),
            target_text,
        }
    }

    /// 开始一轮吞字。overlay 用删除开始前的旧正文。
    pub(crate) fn begin_delete(
        base_snapshot: EditorLayoutSnapshot,
        base_text: String,
        target_snapshot: EditorLayoutSnapshot,
        target_text: String,
        old_range: (usize, usize),
        started_at: Instant,
        duration_ms: u64,
    ) -> Self {
        let conceal_path = FrontierPath::build(&base_snapshot, old_range);
        Self {
            kind: EditFrontierKind::Delete,
            base_snapshot,
            target_snapshot,
            old_range: Some(old_range),
            new_range: None,
            reveal_path: FrontierPath::default(),
            conceal_path,
            reveal_travelled: 0.0,
            conceal_travelled: 0.0,
            started_at,
            duration_ms: duration_ms.max(1),
            base_text,
            target_text,
        }
    }

    /// 开始一轮替换。旧 overlay 收掉 + 新字 mask 打开共用同一个时间 progress，
    /// 但各自在自己的排版路径上采样——old/new 布局可能完全不同（自动换行、
    /// 跨行 IME 提交），没必要强迫它们共享同一个二维坐标。
    pub(crate) fn begin_replace(
        base_snapshot: EditorLayoutSnapshot,
        base_text: String,
        target_snapshot: EditorLayoutSnapshot,
        target_text: String,
        old_range: (usize, usize),
        new_range: (usize, usize),
        started_at: Instant,
        duration_ms: u64,
    ) -> Self {
        let reveal_path = FrontierPath::build(&target_snapshot, new_range);
        let conceal_path = FrontierPath::build(&base_snapshot, old_range);
        Self {
            kind: EditFrontierKind::Replace,
            base_snapshot,
            target_snapshot,
            old_range: Some(old_range),
            new_range: Some(new_range),
            reveal_path,
            conceal_path,
            reveal_travelled: 0.0,
            conceal_travelled: 0.0,
            started_at,
            duration_ms: duration_ms.max(1),
            base_text,
            target_text,
        }
    }

    /// 连续吐字并入同一个前沿：先采样当前前沿当新起点，再更新最新 target。
    ///
    /// 不生成第二个历史动画对象，也不携带上一笔的 Reveal/Conceal。
    ///
    /// Issue #826 评论 3 问题 1：`new_range` 不能直接被本次的 `new_range` 覆盖。
    /// 已累计的遮罩范围在**上一次 target 坐标系**里，必须先用
    /// `prev_target_to_new`（上一次 target 文本 → 最新 target 文本）映射到最新
    /// 坐标再合并，否则第一个字还没吐完就从遮罩范围里消失、被 canonical 瞬间补全。
    ///
    /// 已吐出的部分由 `reveal_travelled` 继承，路径按合并后的完整范围重建。
    pub(crate) fn extend_insert(
        &mut self,
        target_snapshot: EditorLayoutSnapshot,
        target_text: String,
        new_range: (usize, usize),
        prev_target_to_new: &OffsetMap,
        now: Instant,
    ) {
        let sampled = self.sample(now);
        self.reveal_travelled = sampled.reveal_distance;
        self.conceal_travelled = sampled.conceal_distance;
        self.target_snapshot = target_snapshot;
        self.new_range = Some(accumulate_new_range(
            self.new_range,
            new_range,
            prev_target_to_new,
        ));
        self.target_text = target_text;
        self.reveal_path = FrontierPath::build(&self.target_snapshot, self.new_range.unwrap());
        self.rebase_conceal_path();
        self.started_at = now;
    }

    /// 连续吞字并入同一个前沿。
    ///
    /// `base_snapshot` **保持不变**：overlay 必须画本轮连续删除开始前的旧文字，
    /// 中途换 base 会让已经露出的旧字突然变样。
    ///
    /// Issue #826 评论 3 问题 2：`old_range` 一直用 burst 最初 `base_snapshot` 的
    /// 坐标系，而本次传入的 `old_range` 属于「这一次编辑前」的 snapshot。
    /// 例如 `ABC|DEF` 连续 Delete：第一次删 D 得 [3,4]，第二次删 E 仍是 [3,4]，
    /// 但在 burst 最初的 `ABCDEF` 里应累计成 [3,5]。所以先用
    /// `base_to_current`（burst 最初 base 文本 → 本次编辑前文本）把本次范围
    /// 映射回 base 坐标再 union，不靠 byte 数字碰巧一致。
    pub(crate) fn extend_delete(
        &mut self,
        target_snapshot: EditorLayoutSnapshot,
        target_text: String,
        old_range: (usize, usize),
        base_to_current: &OffsetMap,
        now: Instant,
    ) {
        let sampled = self.sample(now);
        self.reveal_travelled = sampled.reveal_distance;
        self.conceal_travelled = sampled.conceal_distance;
        self.target_snapshot = target_snapshot;
        let mapped = base_to_current
            .map_new_range_to_old(old_range.0, old_range.1)
            .unwrap_or(old_range);
        self.old_range = Some(union_range(self.old_range, mapped));
        self.target_text = target_text;
        self.rebase_conceal_path();
        self.rebase_reveal_path();
        self.started_at = now;
    }

    fn rebase_conceal_path(&mut self) {
        self.conceal_path = match self.old_range {
            Some(range) => FrontierPath::build(&self.base_snapshot, range),
            None => FrontierPath::default(),
        };
    }

    fn rebase_reveal_path(&mut self) {
        self.reveal_path = match self.new_range {
            Some(range) => FrontierPath::build(&self.target_snapshot, range),
            None => FrontierPath::default(),
        };
    }

    /// 采样当前帧前沿。
    pub(crate) fn sample(&self, now: Instant) -> EditFrontierSample {
        let elapsed_ms = now.saturating_duration_since(self.started_at).as_millis() as f64;
        let raw = if self.duration_ms == 0 {
            1.0
        } else {
            elapsed_ms / self.duration_ms as f64
        };
        let progress = raw.clamp(0.0, 1.0);
        let eased = ease_out_cubic(progress);
        EditFrontierSample {
            kind: self.kind,
            progress,
            reveal_distance: travelled(self.reveal_travelled, self.reveal_path.total_length, eased),
            conceal_distance: travelled(
                self.conceal_travelled,
                self.conceal_path.total_length,
                eased,
            ),
            old_range: self.old_range,
            new_range: self.new_range,
        }
    }

    /// 前沿是否已经走完（可以收掉本轮遮罩/overlay）。
    pub(crate) fn is_finished(&self, now: Instant) -> bool {
        self.sample(now).progress >= 1.0
    }

    /// 本帧吐字要裁掉 canonical 新字的矩形。
    ///
    /// Issue #826 评论 6 阻塞 3：**只裁 inserted cluster**，不能用整行内容右边界。
    ///
    /// 例：旧正文 `A|B`，中间插入 X 得 `AX|B`。target 行三个 cluster：
    /// A（unchanged）/ X（inserted）/ B（unchanged + Reflow）。`new_range` 只有 X，
    /// 若裁到整行右边界，B 会被 FrontierMask 一起挖掉——但 B 属于 Reflow 层职责，
    /// FrontierMask 越权会让 Reflow 的动画层和静态层互相抢同一块区域。
    ///
    /// Issue #826 评论 7 阻塞 2：边界来自**视觉路径**而不是一个二维 CursorRect。
    /// 自动换行时路径会先走完第一行的 inserted 部分，再从下一行左侧继续，
    /// 新字永远不会因为前沿「斜穿屏幕到很右的 x」而被判成已经完整露出。
    pub(crate) fn hidden_new_text_rects(&self, sample: &EditFrontierSample) -> Vec<FrontierRect> {
        let new_range = match sample.new_range {
            Some(range) => range,
            None => return Vec::new(),
        };
        if !sample.masks_new_text() {
            return Vec::new();
        }
        let boundaries = self.reveal_path.reveal_boundaries(sample.reveal_distance);
        let mut rects = Vec::new();
        for line in self
            .target_snapshot
            .lines_in_byte_range(new_range.0, new_range.1)
        {
            let Some(seg_index) = self.reveal_path.segment_index_for_line(line.id) else {
                // 这一行没有可见 glyph（换行符），不产生 FrontierMask。
                continue;
            };
            let boundary = boundaries.get(seg_index).copied().unwrap_or(f64::MAX);
            for cluster in line.clusters_in_byte_range(new_range.0, new_range.1) {
                // inserted 自己的文档坐标矩形，不牵连同行的 unchanged cluster。
                let glyph = line.source_rect_to_document_rect(&cluster.source_rect);
                let glyph_right = glyph.x + glyph.w;
                let rect = if glyph.x >= boundary {
                    // 整块 cluster 都还在前沿之后：全藏。
                    FrontierRect {
                        x: glyph.x,
                        y: glyph.y,
                        w: glyph.w,
                        h: glyph.h,
                    }
                } else if glyph_right > boundary {
                    // 前沿切在这个 cluster 中间：只藏前沿右侧那一段。
                    FrontierRect {
                        x: boundary,
                        y: glyph.y,
                        w: glyph_right - boundary,
                        h: glyph.h,
                    }
                } else {
                    // 已在前沿之前，完整露出。
                    continue;
                };
                if !rect.is_degenerate() {
                    rects.push(rect);
                }
            }
        }
        rects
    }

    /// 本帧旧正文 overlay 要保留的矩形（Delete / Replace 用）。
    ///
    /// 边界来自旧正文的视觉路径：已收完的段不画，当前段只保留 `[x_start, boundary]`，
    /// 已走过的段完整保留。
    pub(crate) fn old_overlay_rects(&self, sample: &EditFrontierSample) -> Vec<FrontierRect> {
        let old_range = match sample.old_range {
            Some(range) => range,
            None => return Vec::new(),
        };
        if !sample.needs_old_overlay() {
            return Vec::new();
        }
        let boundaries = self
            .conceal_path
            .conceal_boundaries(sample.conceal_distance);
        let mut rects = Vec::new();
        for line in self
            .base_snapshot
            .lines_in_byte_range(old_range.0, old_range.1)
        {
            let rect = match self.conceal_path.segment_index_for_line(line.id) {
                Some(seg_index) => {
                    let seg = self.conceal_path.segments[seg_index];
                    let boundary = boundaries.get(seg_index).copied().unwrap_or(seg.x_end);
                    FrontierRect {
                        x: seg.x_start,
                        y: line.visual_line_top,
                        w: (boundary - seg.x_start).max(0.0),
                        h: line.visual_line_bottom - line.visual_line_top,
                    }
                }
                // 这一行没有可见 glyph，但仍在删除范围内：整行保留，
                // 让旧正文的行结构不至于突然少一行。
                None => {
                    let (left, right) = line_content_x_extent(line);
                    FrontierRect {
                        x: left,
                        y: line.visual_line_top,
                        w: (right - left).max(0.0),
                        h: line.visual_line_bottom - line.visual_line_top,
                    }
                }
            };
            if !rect.is_degenerate() {
                rects.push(rect);
            }
        }
        rects
    }

    /// 本帧旧正文 overlay 要画的 cluster（含 source / dest 矩形）。
    ///
    /// 只画 `old_range`，且只画落在 [`Self::old_overlay_rects`] 里的部分。
    pub(crate) fn old_overlay_glyphs(&self, sample: &EditFrontierSample) -> Vec<FrontierGlyph> {
        let old_range = match sample.old_range {
            Some(range) => range,
            None => return Vec::new(),
        };
        if !sample.needs_old_overlay() {
            return Vec::new();
        }
        let keep = self.old_overlay_rects(sample);
        let mut glyphs = Vec::new();
        for line in self
            .base_snapshot
            .lines_in_byte_range(old_range.0, old_range.1)
        {
            for cluster in line.clusters_in_byte_range(old_range.0, old_range.1) {
                let source = cluster.source_rect.clone();
                let dest = line.source_rect_to_document_rect(&source);
                let clipped = clip_dest_to_rects(dest.clone(), &keep);
                for (dest_x, dest_w) in clipped {
                    if dest_w <= 0.0 {
                        continue;
                    }
                    // dest 与 source 都是同一 cluster 内的水平子区间，按比例回推
                    // source 的水平偏移与宽度。
                    let ratio = if dest.w > 0.0 {
                        (dest_x - dest.x) / dest.w
                    } else {
                        0.0
                    };
                    let src_x = source.x + source.w * ratio;
                    let src_w = (dest_w / dest.w.max(f64::MIN_POSITIVE)) * source.w;
                    let src_h = source.h;
                    let src_y = source.y;
                    glyphs.push(FrontierGlyph {
                        snapshot_id: line.id,
                        source_rect: SourceRect {
                            x: src_x,
                            y: src_y,
                            w: src_w,
                            h: src_h,
                        },
                        dest_rect: SourceRect {
                            x: dest_x,
                            y: dest.y,
                            w: dest_w,
                            h: dest.h,
                        },
                    });
                }
            }
        }
        glyphs
    }
}

/// 本帧前沿走过的距离 = 继承的距离 + 本帧 ease 推进的剩余距离。
///
/// 连续编辑时 `inherited` > 0：已经吐出来/吞掉的部分不回退，剩余 `total - inherited`
/// 的距离由新时长重新分配。
fn travelled(inherited: f64, total: f64, eased: f64) -> f64 {
    let inherited = inherited.clamp(0.0, total);
    inherited + (total - inherited) * eased
}

/// Issue #826: 前沿驱动的一枚 overlay glyph。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FrontierGlyph {
    /// 取哪张行纹理。
    pub snapshot_id: LineSnapshotId,
    /// 行内局部物理像素坐标。
    pub source_rect: SourceRect,
    /// 文档坐标。
    pub dest_rect: SourceRect,
}

impl FrontierGlyph {
    /// overlay 完全被吞掉（宽度为 0）时不需要画。
    pub(crate) fn is_visible(&self) -> bool {
        self.dest_rect.w > 0.0 && self.dest_rect.h > 0.0
    }
}

/// Issue #826 评论 3 问题 1：把上一次 target 坐标系里已累计的遮罩范围映射到最新
/// target 坐标，再与本次新增范围求并集。
///
/// 映射失败（跨 OffsetMap 条目边界）时只保留本次范围：宁可少遮一点，
/// 也不让已吐出一半的字整段凭空闪出来。
fn accumulate_new_range(
    accumulated: Option<(usize, usize)>,
    new_range: (usize, usize),
    prev_target_to_new: &OffsetMap,
) -> (usize, usize) {
    let Some((start, end)) = accumulated else {
        return new_range;
    };
    match prev_target_to_new.map_old_range_to_new(start, end) {
        Some(mapped) => union_range(Some(mapped), new_range),
        None => new_range,
    }
}

/// 合并两个 byte range（半开区间，允许空）。
fn union_range(current: Option<(usize, usize)>, next: (usize, usize)) -> (usize, usize) {
    match current {
        Some((start, end)) => (start.min(next.0), end.max(next.1)),
        None => next,
    }
}

/// 一行在文档坐标里的内容左右边界。
///
/// 用 cluster 的实际覆盖范围，没有 cluster 时退回 visual_x。
fn line_content_x_extent(line: &PreparedLineSnapshot) -> (f64, f64) {
    let mut left = f64::MAX;
    let mut right = f64::MIN;
    for cluster in &line.clusters {
        let rect = line.source_rect_to_document_rect(&cluster.source_rect);
        left = left.min(rect.x);
        right = right.max(rect.x + rect.w);
    }
    if left < f64::MAX && right > f64::MIN {
        (left, right)
    } else {
        (line.visual_x, line.visual_x)
    }
}

/// 把一个 dest 矩形按 keep 矩形列表水平裁剪，返回裁完后的 `(x, w)` 列表。
///
/// 前沿只沿 x 方向切开同一行的文字，所以只需要水平裁剪。
fn clip_dest_to_rects(dest: SourceRect, keep: &[FrontierRect]) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    for rect in keep {
        // 垂直方向必须相交，否则不是同一行上的裁剪。
        if dest.y + dest.h <= rect.y || dest.y >= rect.y + rect.h {
            continue;
        }
        let left = dest.x.max(rect.x);
        let right = (dest.x + dest.w).min(rect.x + rect.w);
        if right > left {
            out.push((left, right - left));
        }
    }
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    out
}

#[cfg(test)]
mod tests;
