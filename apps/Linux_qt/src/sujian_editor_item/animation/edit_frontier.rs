//! Issue #826: 正文「吞字吐字」的单一遮罩前沿。
//!
//! 正文永远立即变成最新真实内容。本模块**只**负责一件事：控制「本轮改动的这段
//! 文字现在露出多少」。
//!
//! - 吐字（Insert）：最新 canonical 正文只画一份；本轮新增范围里落在前沿**之后**的
//!   部分被遮罩裁掉，前沿从本轮改动的起点逐步打开到最新目标。
//! - 吞字（Delete）：canonical 正文立即没有这些字；额外画一份本轮删除开始前的旧正文
//!   overlay，遮罩从删除区间尾部向前沿逐步收掉 overlay。
//! - 替换（Replace）：旧 overlay 收掉 + 新字 mask 打开，共用同一条前沿。
//!
//! 连续同方向输入/删除**只更新同一个前沿对象**：先采样当前前沿，把采样结果当作新的
//! 起点，再更新最新 target。不复制上一笔动画，不携带历史 Reveal/Conceal，不排队。
//!
//! 本模块不拥有光标动画，也不处理 IME preedit。

use std::time::Instant;

use writer_core::editor::OffsetMap;

use crate::sujian_editor_item::edit_motion::CursorRect;
use crate::sujian_editor_item::layout_snapshot::{
    EditorLayoutSnapshot, PreparedLineSnapshot, SourceRect,
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

/// Issue #826: 唯一的正文改动前沿状态。
///
/// - `base_snapshot`：连续 burst 开始前的旧正文。只有 Delete / Replace 的
///   overlay 需要它（overlay 必须画「本轮连续删除开始前真正需要显示的旧文字」）。
/// - `target_snapshot`：当前最新正文。吐字的新字、吞字后回流的位置都取自它。
/// - `old_range`：旧正文坐标系里本轮被删掉的范围。
/// - `new_range`：最新正文坐标系里本轮新增的范围。
/// - `start_frontier` / `target_frontier`：前沿的起点和最新目标。progress 0 时前沿在
///   起点（本轮改动一点都没露出），progress 1 时前沿到目标（改动全部露出）。
#[derive(Clone, Debug)]
pub(crate) struct EditFrontierState {
    pub kind: EditFrontierKind,
    pub base_snapshot: EditorLayoutSnapshot,
    pub target_snapshot: EditorLayoutSnapshot,
    pub old_range: Option<(usize, usize)>,
    pub new_range: Option<(usize, usize)>,
    pub start_frontier: CursorRect,
    pub target_frontier: CursorRect,
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
    /// 当前帧前沿位置（文档坐标）。
    pub frontier: CursorRect,
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
///
/// `hidden` 为真表示「把这一块从 canonical 静态层裁掉」；为假表示「旧正文 overlay
/// 只保留这一块」。
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

/// 前沿进度 → 当前前沿矩形。起点与目标之间线性插值后套 ease。
fn interpolate_frontier(from: &CursorRect, to: &CursorRect, progress: f64) -> CursorRect {
    let t = ease_out_cubic(progress);
    CursorRect {
        x: from.x + (to.x - from.x) * t,
        top: from.top + (to.top - from.top) * t,
        bottom: from.bottom + (to.bottom - from.bottom) * t,
        baseline_y: from.baseline_y + (to.baseline_y - from.baseline_y) * t,
    }
}

impl EditFrontierState {
    /// 开始一轮吐字。
    pub(crate) fn begin_insert(
        target_snapshot: EditorLayoutSnapshot,
        target_text: String,
        new_range: (usize, usize),
        start_frontier: CursorRect,
        target_frontier: CursorRect,
        started_at: Instant,
        duration_ms: u64,
    ) -> Self {
        Self {
            kind: EditFrontierKind::Insert,
            // 纯吐字不需要旧正文 overlay，base_snapshot 与 target 相同。
            base_snapshot: target_snapshot.clone(),
            target_snapshot,
            old_range: None,
            new_range: Some(new_range),
            start_frontier,
            target_frontier,
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
        start_frontier: CursorRect,
        target_frontier: CursorRect,
        started_at: Instant,
        duration_ms: u64,
    ) -> Self {
        Self {
            kind: EditFrontierKind::Delete,
            // 前向删除（Delete 键）时被删文字在旧 caret **右侧**，而遮罩从
            // 右往左收，前沿必须先站到被删文字的右边缘，否则第一帧就已经
            // 收完、overlay 全空。后向删除（Backspace）时旧 caret 本身就是
            // 被删文字的右边缘，`max` 不会改动它。
            start_frontier: Self::delete_start_frontier(&base_snapshot, old_range, start_frontier),
            base_snapshot,
            target_snapshot,
            old_range: Some(old_range),
            new_range: None,
            target_frontier,
            started_at,
            duration_ms: duration_ms.max(1),
            base_text,
            target_text,
        }
    }

    /// 吞字前沿的起点：至少要盖住本轮被删文字的右边缘。
    fn delete_start_frontier(
        base_snapshot: &EditorLayoutSnapshot,
        old_range: (usize, usize),
        start_frontier: CursorRect,
    ) -> CursorRect {
        let mut frontier = start_frontier;
        for line in base_snapshot.lines_in_byte_range(old_range.0, old_range.1) {
            for cluster in line.clusters_in_byte_range(old_range.0, old_range.1) {
                let rect = line.source_rect_to_document_rect(&cluster.source_rect);
                if rect.x + rect.w > frontier.x {
                    frontier.x = rect.x + rect.w;
                    frontier.top = rect.y;
                    frontier.bottom = rect.y + rect.h;
                }
            }
        }
        frontier
    }

    /// 开始一轮替换。旧 overlay 收掉 + 新字 mask 打开共用一条前沿。
    pub(crate) fn begin_replace(
        base_snapshot: EditorLayoutSnapshot,
        base_text: String,
        target_snapshot: EditorLayoutSnapshot,
        target_text: String,
        old_range: (usize, usize),
        new_range: (usize, usize),
        start_frontier: CursorRect,
        target_frontier: CursorRect,
        started_at: Instant,
        duration_ms: u64,
    ) -> Self {
        Self {
            kind: EditFrontierKind::Replace,
            start_frontier: Self::delete_start_frontier(&base_snapshot, old_range, start_frontier),
            base_snapshot,
            target_snapshot,
            old_range: Some(old_range),
            new_range: Some(new_range),
            target_frontier,
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
    pub(crate) fn extend_insert(
        &mut self,
        target_snapshot: EditorLayoutSnapshot,
        target_text: String,
        new_range: (usize, usize),
        target_frontier: CursorRect,
        prev_target_to_new: &OffsetMap,
        now: Instant,
    ) {
        let sampled = self.sample(now);
        self.start_frontier = sampled.frontier;
        self.target_snapshot = target_snapshot;
        self.new_range = Some(accumulate_new_range(
            self.new_range,
            new_range,
            prev_target_to_new,
        ));
        self.target_text = target_text;
        self.target_frontier = target_frontier;
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
        target_frontier: CursorRect,
        base_to_current: &OffsetMap,
        now: Instant,
    ) {
        let sampled = self.sample(now);
        self.start_frontier = sampled.frontier;
        self.target_snapshot = target_snapshot;
        let mapped = base_to_current
            .map_new_range_to_old(old_range.0, old_range.1)
            .unwrap_or(old_range);
        self.old_range = Some(union_range(self.old_range, mapped));
        self.target_text = target_text;
        self.target_frontier = target_frontier;
        self.started_at = now;
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
        EditFrontierSample {
            kind: self.kind,
            progress,
            frontier: interpolate_frontier(&self.start_frontier, &self.target_frontier, progress),
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
    /// 前沿之前的行完整显示；前沿所在行按 x 裁；还没到的行整段裁掉。
    pub(crate) fn hidden_new_text_rects(&self, sample: &EditFrontierSample) -> Vec<FrontierRect> {
        let new_range = match sample.new_range {
            Some(range) => range,
            None => return Vec::new(),
        };
        if !sample.masks_new_text() {
            return Vec::new();
        }
        let frontier = &sample.frontier;
        let mut rects = Vec::new();
        for line in self
            .target_snapshot
            .lines_in_byte_range(new_range.0, new_range.1)
        {
            let line_top = line.visual_line_top;
            let line_bottom = line.visual_line_bottom;
            // 完全在前沿之前的行：已经完整露出，不裁。
            if line_bottom <= frontier.top {
                continue;
            }
            let (left, right) = line_content_x_extent(line);
            let rect = if line_top >= frontier.bottom {
                // 还没到的行：整段裁掉。
                FrontierRect {
                    x: left,
                    y: line_top,
                    w: right - left,
                    h: line_bottom - line_top,
                }
            } else {
                // 前沿所在行：从前沿 x 裁到行尾。
                let cut = frontier.x.max(left).min(right);
                FrontierRect {
                    x: cut,
                    y: line_top,
                    w: right - cut,
                    h: line_bottom - line_top,
                }
            };
            if !rect.is_degenerate() {
                rects.push(rect);
            }
        }
        rects
    }

    /// 本帧旧正文 overlay 要保留的矩形（Delete / Replace 用）。
    ///
    /// 前沿之后的行已经吞完，不画；前沿所在行只画到前沿 x；之前的行完整画。
    pub(crate) fn old_overlay_rects(&self, sample: &EditFrontierSample) -> Vec<FrontierRect> {
        let old_range = match sample.old_range {
            Some(range) => range,
            None => return Vec::new(),
        };
        if !sample.needs_old_overlay() {
            return Vec::new();
        }
        let frontier = &sample.frontier;
        let mut rects = Vec::new();
        for line in self
            .base_snapshot
            .lines_in_byte_range(old_range.0, old_range.1)
        {
            let line_top = line.visual_line_top;
            let line_bottom = line.visual_line_bottom;
            if line_top >= frontier.bottom {
                // 前沿之后的行：已经吞完。
                continue;
            }
            let (left, right) = line_content_x_extent(line);
            let rect = if line_bottom <= frontier.top {
                FrontierRect {
                    x: left,
                    y: line_top,
                    w: right - left,
                    h: line_bottom - line_top,
                }
            } else {
                // 前沿所在行：只保留前沿之前的部分。
                let cut = frontier.x.max(left).min(right);
                FrontierRect {
                    x: left,
                    y: line_top,
                    w: cut - left,
                    h: line_bottom - line_top,
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

/// Issue #826: 前沿驱动的一枚 overlay glyph。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FrontierGlyph {
    /// 取哪张行纹理。
    pub snapshot_id: crate::sujian_editor_item::layout_snapshot::LineSnapshotId,
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
