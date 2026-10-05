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
//! 「单一前沿」= 单一时间 progress + 单一 active state + 一组按视觉顺序排好的
//! 文本路径。自动换行时路径自然是「第一行剩余部分走完 → 下一行从左侧开始 → 再向右走」，
//! 不会斜穿屏幕导致新字突然整块出现。
//!
//! Issue #826 评论 8 阻塞 3：Core 一次编辑可能给出**多条** `display_patches`
//! （Undo/Redo 的 history batch、replace-all、apply 原子 batch、IME commit…）。
//! 所以 changed range 是一组而不是单个：`old_ranges` / `new_ranges`，
//! 每条互不相邻的 patch 各有自己的视觉路径，但**共享同一个 progress**——
//! 一笔 batch 的所有 patch 同时被接管，不排历史队列，也绝不把不相邻的两段
//! union 成一个大 range（那会把中间的正常正文也当成改动过的字）。
//!
//! Issue #826 评论 8 阻塞 1：吞字路径必须带**方向**。Backspace 时 old range 向左扩，
//! Delete 键时向右扩；路径的视觉顺序必须跟着走，否则连续跨自动换行删除时，
//! 已吞掉的那一行会随着 old range 扩展而被重绑到别的字符上。
//!
//! 本模块不拥有光标动画，也不处理 IME preedit。

use std::time::Instant;

use super::coordinator::EditFrontierRequest;
use super::reflow_motion::ReflowCurrentGeometry;

use writer_core::editor::OffsetMap;

use crate::sujian_editor_item::layout_snapshot::{
    EditorLayoutSnapshot, LineSnapshotId, SourceRect,
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

/// Issue #826 评论 8 阻塞 1：吞字路径的行进方向。
///
/// 方向不需要猜键盘事件：连续删除时 old range 是**向左**扩（Backspace）
/// 还是**向右**扩（Delete 键），可以直接从 old/new head 与 deleted range 的
/// 相对位置推出来。方向决定了视觉路径的行进顺序。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ConcealDirection {
    /// Delete 键：old range 向右扩。路径按视觉正序，行内 x_start → x_end。
    Forward,
    /// Backspace：old range 向左扩。路径按视觉逆序，行内 x_end → x_start。
    Backward,
}

impl ConcealDirection {
    pub(crate) fn label(self) -> &'static str {
        match self {
            ConcealDirection::Forward => "Forward",
            ConcealDirection::Backward => "Backward",
        }
    }
}

/// Issue #826 评论 7/8：一段视觉路径上的一个视觉行片段。
///
/// 一个 segment 是「某个视觉行上，changed range 覆盖的那段横向区间」。
/// 路径按行进方向把若干 segment 串起来，前沿沿路径推进而不是在屏幕上斜飞。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct FrontierSegment {
    /// 这一段属于哪条视觉行（用 `line.id` 精确匹配，不用 y 近似）。
    pub line_id: LineSnapshotId,
    /// 该视觉行的文档坐标上边界。
    pub y: f64,
    /// 该视觉行的高度。
    pub h: f64,
    /// 行内区间的**左**文档坐标。
    pub x_left: f64,
    /// 行内区间的**右**文档坐标。
    pub x_right: f64,
    /// 前沿进入这一段时的 x（方向起点）。
    pub x_from: f64,
    /// 前沿走完这一段时的 x（方向终点）。
    pub x_to: f64,
    /// 这一段的横向长度。
    pub visual_length: f64,
}

impl FrontierSegment {
    /// 前沿走过 `distance` 之后落在哪个 x。
    fn boundary_after(&self, distance: f64) -> f64 {
        let take = distance.clamp(0.0, self.visual_length);
        self.x_from + (self.x_to - self.x_from) * (take / self.visual_length.max(f64::MIN_POSITIVE))
    }
}

/// Issue #826 评论 7/8：一条按视觉顺序排好的 changed range 视觉路径。
///
/// `total_length` 是所有 segment 长度之和；前沿的「已走过距离」就在这条路径上度量。
#[derive(Clone, Debug, Default)]
pub(crate) struct FrontierPath {
    pub segments: Vec<FrontierSegment>,
    pub total_length: f64,
    /// 行进方向。吐字恒为 `Forward`；吞字由 [`ConcealDirection`] 决定。
    pub direction: PathDirection,
}

/// Issue #826 评论 8 阻塞 1：视觉路径的行进方向。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum PathDirection {
    /// 视觉正序（行 1 → 行 2 → …），行内 x_start → x_end。吐字与 Delete 键吞字用。
    #[default]
    Forward,
    /// 视觉逆序（行 N → … → 行 1），行内 x_end → x_start。Backspace 吞字用。
    Backward,
}

impl From<ConcealDirection> for PathDirection {
    fn from(direction: ConcealDirection) -> Self {
        match direction {
            ConcealDirection::Forward => PathDirection::Forward,
            ConcealDirection::Backward => PathDirection::Backward,
        }
    }
}

impl FrontierPath {
    /// Issue #826 评论 13：从「上一帧 Reflow 的采样几何」构建吞字路径。
    ///
    /// 这些 glyph 这一帧已经在半路（屏幕位置 55），不该先瞬移回 canonical
    /// 位置（60）再开始吞。路径直接从采样几何构建，第一帧边界就落在 55。
    ///
    /// 按 `dest_rect.y` 分行（同一视觉行聚成一段），段内取 min/max x；
    /// `Backward` 时按视觉逆序。行分组用 `y` 的精确相等而不是 EPS 容差 ——
    /// 采样几何本来就来自同一份排版，同一行的 `y` 必然相同。
    pub(crate) fn from_glyph_geometry(
        glyphs: &[ConcealGlyphGeometry],
        direction: PathDirection,
    ) -> Self {
        let mut order: Vec<f64> = Vec::new();
        let mut grouped: Vec<(f64, f64, f64, f64)> = Vec::new();
        for glyph in glyphs {
            let dest = glyph.dest_rect.clone();
            match order.iter().position(|&y| y == dest.y) {
                Some(index) => {
                    let row = grouped[index];
                    grouped[index] = (
                        row.0,
                        row.1.max(dest.y + dest.h),
                        row.2.min(dest.x),
                        row.3.max(dest.x + dest.w),
                    );
                }
                None => {
                    order.push(dest.y);
                    grouped.push((dest.y, dest.y + dest.h, dest.x, dest.x + dest.w));
                }
            }
        }
        let mut segments: Vec<FrontierSegment> = grouped
            .into_iter()
            .filter(|&(_, _, x_left, x_right)| x_right > x_left)
            .map(|(y, bottom, x_left, x_right)| {
                let (x_from, x_to) = match direction {
                    PathDirection::Forward => (x_left, x_right),
                    PathDirection::Backward => (x_right, x_left),
                };
                FrontierSegment {
                    line_id: LineSnapshotId::new(0, 0, 0),
                    y,
                    h: bottom - y,
                    x_left,
                    x_right,
                    x_from,
                    x_to,
                    visual_length: (x_right - x_left).max(0.0),
                }
            })
            .collect();
        // glyph 收集顺序 = 编辑事件顺序，不是视觉顺序。必须先按 y 升序排好，
        // 再按方向决定正序 / 逆序，否则 Backspace 的第一段会是「最早收集的那行」
        // 而不是「视觉上最后一行」。
        segments.sort_by(|a, b| a.y.partial_cmp(&b.y).unwrap_or(std::cmp::Ordering::Equal));
        if direction == PathDirection::Backward {
            segments.reverse();
        }
        let total_length = segments.iter().map(|s| s.visual_length).sum();
        Self {
            segments,
            total_length,
            direction,
        }
    }

    /// 从 snapshot 里 `range` 覆盖的 cluster 按行进方向构建路径。
    ///
    /// 每个视觉行产出一个 segment；该行没有 cluster（如换行符）时**不**产出
    /// segment——换行没有可见 glyph，就不该让前沿为它花掉行程。
    pub(crate) fn build(
        snapshot: &EditorLayoutSnapshot,
        range: (usize, usize),
        direction: PathDirection,
    ) -> Self {
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
                x_left: left,
                x_right: right,
                x_from: match direction {
                    PathDirection::Forward => left,
                    PathDirection::Backward => right,
                },
                x_to: match direction {
                    PathDirection::Forward => right,
                    PathDirection::Backward => left,
                },
                visual_length: right - left,
            });
        }
        // glyph 收集顺序 = 编辑事件顺序，不是视觉顺序。必须先按 y 升序排好，
        // 再按方向决定正序 / 逆序，否则 Backspace 的第一段会是「最早收集的那行」
        // 而不是「视觉上最后一行」。
        segments.sort_by(|a, b| a.y.partial_cmp(&b.y).unwrap_or(std::cmp::Ordering::Equal));
        if direction == PathDirection::Backward {
            segments.reverse();
        }
        let total_length = segments.iter().map(|s| s.visual_length).sum();
        Self {
            segments,
            total_length,
            direction,
        }
    }

    /// 某条视觉行在路径里的段序号。
    pub(crate) fn segment_index_for_line(&self, line_id: LineSnapshotId) -> Option<usize> {
        self.segments.iter().position(|s| s.line_id == line_id)
    }

    /// 吐字：本帧每一段「已打开到哪个 x」。
    ///
    /// 返回的 `x` 之前是新字已露出的部分，之后要被遮罩裁掉。
    pub(crate) fn reveal_bounds(&self, distance: f64) -> Vec<(f64, f64)> {
        let mut remaining = distance;
        self.segments
            .iter()
            .map(|seg| {
                let boundary = seg.boundary_after(remaining);
                remaining = (remaining - seg.visual_length).max(0.0);
                (boundary, seg.x_right.max(seg.x_left))
            })
            .collect()
    }

    /// 吞字：本帧每一段「还保留哪一个 x 区间」。
    ///
    /// 前沿沿路径推进，推进过的部分已经被吞掉，剩下的是旧正文 overlay 仍要画的区间。
    pub(crate) fn conceal_bounds(&self, distance: f64) -> Vec<(f64, f64)> {
        let mut remaining = distance;
        self.segments
            .iter()
            .map(|seg| {
                let boundary = seg.boundary_after(remaining);
                remaining = (remaining - seg.visual_length).max(0.0);
                match self.direction {
                    PathDirection::Forward => (boundary, seg.x_right),
                    PathDirection::Backward => (seg.x_left, boundary),
                }
            })
            .collect()
    }
}

/// Issue #826: 唯一的正文改动前沿状态。
///
/// - `base_snapshot`：连续 burst 开始前的旧正文。只有 Delete / Replace 的
///   overlay 需要它（overlay 必须画「本轮连续删除开始前真正需要显示的旧文字」）。
/// - `target_snapshot`：当前最新正文。吐字的新字、吞字后回流的位置都取自它。
/// - `old_ranges`：旧正文坐标系里本轮被删掉的范围（burst base 坐标系）。
/// - `new_ranges`：最新正文坐标系里本轮新增的范围（最新 target 坐标系）。
///   两组都按 overlap / adjacent 归一化，**不会跨 gap 合并**。
/// - `reveal_tracks` / `conceal_tracks`：每个 disjoint patch 一条 track，
///   track 自己拥有 range / path / travelled（评论 9 阻塞 3）。
///   连续编辑时按 track 身份继承上一帧的已走过距离，所以「已经吐出来的字」
///   不会因为下一笔而回退，也不会把进度串到别的 patch 上。
#[derive(Clone, Debug)]
pub(crate) struct EditFrontierState {
    pub kind: EditFrontierKind,
    pub base_snapshot: EditorLayoutSnapshot,
    pub target_snapshot: EditorLayoutSnapshot,
    /// Issue #826 评论 17：吞字侧的当前前沿（**一个时钟** + 当前 region 集合）。
    ///
    /// 连续删除会把相邻的 deleted range 合并进同一个 region，region 数量
    /// ∝ 不相交 patch 数，不随按键次数增长。
    pub(crate) conceal: FrontierLayer,
    /// Issue #826 评论 17：吐字侧的当前前沿（语义同 `conceal`）。
    pub(crate) reveal: FrontierLayer,
    /// Issue #826 评论 15/17：当前**仍可见**的旧字 glyph。
    ///
    /// 已完全吞掉的 glyph 立刻从这里移除（连同它的 `conceal_sources` 行图），
    /// 状态大小 ∝ 当前屏幕还没吞完的内容，而不是 ∝ 这轮一共删过多少次。
    pub(crate) conceal_glyphs: Vec<ConcealGlyphGeometry>,
    /// `conceal_glyphs` 真正还引用的行图。
    pub(crate) conceal_sources: Vec<ConcealSourceLine>,
    /// 吞字路径的行进方向（评论 8 阻塞 1）。
    pub(crate) conceal_direction: ConcealDirection,
    pub started_at: Instant,
    pub duration_ms: u64,
    /// 这一轮 burst **开始前**的正文纯文本。
    ///
    /// `old_ranges` 一直用这个坐标系，所以连续吞字时必须靠它把每次编辑的
    /// old range 映射回同一个基准（`extend_delete` / `extend_replace`）。
    pub base_text: String,
    /// 上一次 `target_snapshot` 对应的正文纯文本。
    ///
    /// `new_ranges` 用这个坐标系，所以连续吐字时必须靠它把已累计的 new ranges
    /// 映射到最新 target 坐标（`extend_insert` / `extend_replace`）。
    pub target_text: String,
    /// Issue #826 评论 10 阻塞 1：burst 最初 base 正文 → **当前 target 正文** 的
    /// 精确累计映射。
    ///
    /// 连续 Delete / Replace 必须靠它把本次 deleted ranges 映回 burst base 坐标。
    /// 之前每笔都 `OffsetMap::build(base_text, current_base_text)` 重新全文 diff，
    /// 但 `build` 只是最长公共前缀+后缀，多 patch 中间的 unchanged island 会丢：
    /// 例如 `aXbXc -> abc` 再删 `b`，第二次的 `map_new_range_to_old(1,2)` 返回
    /// `None`，`b` 明明被 Core 真正删除、Reflow 也把它当 changed 排除，却没有
    /// ConcealTrack，视觉上直接从 canonical 消失。
    ///
    /// 现在 begin 时存 `request.offset_map`，每次 extend 后
    /// `base_to_target_map.compose(&request.offset_map)` 继续累计，
    /// 整个 burst 始终沿 Core 的精确字符身份走。
    pub(crate) base_to_target_map: OffsetMap,
}

/// Issue #826: 前沿的一帧采样结果。
#[derive(Clone, Copy, Debug)]
pub(crate) struct EditFrontierSample {
    pub kind: EditFrontierKind,
    /// 0.0 = 改动一点都没露出；1.0 = 改动全部露出。
    pub progress: f64,
}

impl EditFrontierSample {
    /// 采样结果里前沿之后（还没露出）的部分是否需要遮罩 canonical 新字。
    pub(crate) fn masks_new_text(self) -> bool {
        self.progress < 1.0 && self.kind.needs_new_mask()
    }

    /// 采样结果里是否还需要画旧正文 overlay。
    ///
    /// Issue #826 评论 3 问题 4：吞字第一帧（progress=0）旧字必须**完整可见**，
    /// 随后才被前沿逐步收掉；progress=1 才完全消失。
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

// ConcealTrack / ConcealSourceLine 含 QImage，没有 Debug；这里手写一份只暴露
// 身份与几何的 Debug，避免为了 derive Debug 把纹理字段也塞进去。
impl std::fmt::Debug for ConcealSourceLine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConcealSourceLine")
            .field("snapshot_id", &self.snapshot_id)
            .field("has_image", &self.image.is_some())
            .finish()
    }
}

/// Issue #826 评论 17：当前**这一份**前沿几何区域。
///
/// 只有当前几何，没有自己的 `started_at` / `travelled` / 历史来源事件 ——
/// 整个 state 只有一个前沿时钟（[`FrontierLayer::travelled`]）。
/// 一笔 Core 事务的多条不相邻 patch 可以有多个 region，它们共享同一时钟。
#[derive(Clone, Debug)]
pub(crate) struct FrontierRegion {
    /// 本 region 的 byte 范围（reveal 用最新 target 坐标，conceal 用 burst base 坐标）。
    pub(crate) range: (usize, usize),
    /// 本 region 的视觉路径。
    pub(crate) path: FrontierPath,
    /// 本 region 起点在**整条前沿路径**上的距离偏移。
    ///
    /// 单一时钟跨 region 连续推进：region 的实际距离 =
    /// `(layer.travelled - distance_start).clamp(0, path.total_length)`。
    pub(crate) distance_start: f64,
}

/// Issue #826 评论 17：一侧（reveal / conceal）的当前前沿状态。
///
/// 关键性质（评论 9 的 `Vec<RevealTrack>` / 评论 8-16 的 `Vec<ConcealTrack>`
/// 都违反这条）：
/// - **只有一个** `travelled`（一个前沿时钟），不是每条历史单元一个；
/// - `regions` 只描述**当前仍由前沿负责**的几何，连续按键通过合并相邻
///   range 落进同一个 region，所以 region 数量 ∝ 不相交 patch 数，
///   **不随按键次数线性增长**。
#[derive(Clone, Debug, Default)]
pub(crate) struct FrontierLayer {
    pub(crate) regions: Vec<FrontierRegion>,
    /// 单一前沿进度：沿所有 region 按顺序拼接后的已走过距离。
    pub(crate) travelled: f64,
}

impl FrontierLayer {
    /// 整条前沿路径的总长度。
    fn total_length(&self) -> f64 {
        self.regions
            .iter()
            .map(|region| region.path.total_length)
            .sum()
    }

    /// Issue #826 评论 17：本帧已推进到的位置（连续编辑时继承它当新起点）。
    pub(crate) fn inherited(&self, progress: f64) -> f64 {
        self.advanced(progress)
    }

    /// 本 region 这一帧走过的距离（`self` 不需要可变）。
    fn distance_for(&self, region: &FrontierRegion) -> f64 {
        (self.travelled - region.distance_start).clamp(0.0, region.path.total_length)
    }

    /// Issue #826 评论 17：按时间 progress 推进后的**单一**前沿位置。
    ///
    /// `travelled` 是「连续编辑继承下来的已走过距离」，本函数按本帧的
    /// `ease_out_cubic(progress)` 推进成当前帧的位置。整层只有这一个时钟。
    pub(crate) fn advanced(&self, progress: f64) -> f64 {
        let total = self.total_length();
        let inherited = self.travelled.clamp(0.0, total);
        inherited + (total - inherited) * ease_out_cubic(progress)
    }

    /// 本 region 在本帧的位置。
    fn distance_at(&self, region: &FrontierRegion, progress: f64) -> f64 {
        (self.advanced(progress) - region.distance_start).clamp(0.0, region.path.total_length)
    }

    /// 整层是否已经走完（按 progress 判定）。
    fn is_advanced_done(&self, progress: f64) -> bool {
        self.advanced(progress) >= self.total_length() - 1e-9
    }

    /// 按视觉顺序重新计算每个 region 的 `distance_start`。
    fn reseat(&mut self) {
        let mut offset = 0.0;
        for region in &mut self.regions {
            region.distance_start = offset;
            offset += region.path.total_length;
        }
    }

    /// 由 (range, path) 列表构建并重新排好座位。
    fn from_parts(parts: Vec<((usize, usize), FrontierPath)>) -> Self {
        let regions: Vec<FrontierRegion> = parts
            .into_iter()
            .map(|(range, path)| FrontierRegion {
                range,
                path,
                distance_start: 0.0,
            })
            .collect();
        let mut layer = Self {
            regions,
            travelled: 0.0,
        };
        layer.reseat();
        layer
    }
}

/// Issue #826 评论 15：一条吞字 track 真正需要的行图。
///
/// 手写 Debug：`QImage` 没有 Debug，且纹理内容对调试毫无价值。
#[derive(Clone)]
pub(crate) struct ConcealSourceLine {
    pub snapshot_id: LineSnapshotId,
    pub image: Option<qmetaobject::QImage>,
}

/// Issue #826 评论 13：一个旧字 glyph 的贴图来源 + 本轮显示位置。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ConcealGlyphGeometry {
    pub range: (usize, usize),
    /// 贴图来源行纹理。
    pub snapshot_id: LineSnapshotId,
    /// 从上面那张行纹理取这块的源矩形。
    pub source_rect: SourceRect,
    /// 这一轮真正画在屏幕上的目标矩形（文档坐标）。
    pub dest_rect: SourceRect,
}

impl EditFrontierState {
    /// 本轮吞字的全部旧文字范围（burst base 坐标系）。
    ///
    /// Issue #826 评论 17：本轮仍由前沿负责的**旧文字范围**（burst base 坐标）。
    pub(crate) fn old_ranges(&self) -> Vec<(usize, usize)> {
        self.conceal
            .regions
            .iter()
            .map(|region| region.range)
            .collect()
    }

    /// Issue #826 评论 17：本轮仍由前沿负责的**新文字范围**（最新 target 坐标）。
    pub(crate) fn new_ranges(&self) -> Vec<(usize, usize)> {
        self.reveal
            .regions
            .iter()
            .map(|region| region.range)
            .collect()
    }

    /// Issue #826 评论 17：**仍未吐完**的新文字范围。
    ///
    /// Reflow 的 `excluded_new` 必须用这个而不是整个 `new_ranges()`：已经完整
    /// 露出的字对本次编辑已经是 unchanged text，应该让 Reflow 正常接管它从
    /// 旧位置移到新行。之前用「整轮历史插入」会让上一笔已经吐完的字在下一笔
    /// 触发自动换行时既不能 Reveal（path 换到新行）也不能 Reflow，直接瞬移。
    pub(crate) fn pending_reveal_ranges(&self, progress: f64) -> Vec<(usize, usize)> {
        let mut pending: Vec<(usize, usize)> = Vec::new();
        for region in &self.reveal.regions {
            let distance = self.reveal.distance_at(region, progress);
            // 已完全走完的 region 整体释放给 Reflow。
            if distance >= region.path.total_length - 1e-9 {
                continue;
            }
            // 只把**尚未露完的后缀**留在前沿名下：按 reveal 边界切掉已露出的
            // prefix。评论 17 的核心 —— 已经完整露出的字对本次编辑已经是
            // unchanged text，必须让 Reflow 正常接管它从旧位置移到新行；
            // 之前整段 region 都被排除，触发自动换行时既不能 Reveal
            // （path 换到新行）也不能 Reflow，直接瞬移。
            let boundary = region
                .path
                .reveal_bounds(distance)
                .first()
                .map(|(edge, _)| *edge)
                .unwrap_or(f64::NEG_INFINITY);
            let mut cut = region.range.0;
            for line in self
                .target_snapshot
                .lines_in_byte_range(region.range.0, region.range.1)
            {
                for cluster in line.clusters_in_byte_range(region.range.0, region.range.1) {
                    let glyph = line.source_rect_to_document_rect(&cluster.source_rect);
                    let fully_revealed = glyph.x + glyph.w <= boundary + 1e-9;
                    if !fully_revealed {
                        break;
                    }
                    cut = cut.max(cluster.byte_end);
                }
                if cut >= region.range.1 {
                    break;
                }
            }
            if cut < region.range.1 {
                pending.push((cut, region.range.1));
            }
        }
        pending
    }

    /// Issue #826 评论 11 阻塞 2：编辑**身份**是否连续 —— 决定这一笔能不能并入
    /// 当前 burst。
    ///
    /// 稳定反例：快速连续两次 Undo。正文历史 `a -> b -> c`，当前是 `c`。
    /// 第一次 Undo `c -> b`（Replace，burst base = `c`）；动画未结束立刻第二次
    /// Undo `b -> a`，`b` 是第一次 Undo 刚插出来的字，在 burst base `c` 里根本
    /// 不存在，映射必然失败。静默丢掉会让 `b` 闪没 —— 与 #826 最初要解决的
    /// 「快速编辑时历史字突然消失」是同一类问题，只是载体换成了 silent map failure。
    ///
    /// 身份不连续不是错误 fallback，而是**新的 burst 语义边界**。
    pub(crate) fn can_extend_identity(
        &self,
        kind: EditFrontierKind,
        request: &EditFrontierRequest,
    ) -> bool {
        if kind.needs_old_overlay() {
            let all_mapped = request
                .deleted_ranges
                .iter()
                .filter(|&&(start, end)| end > start)
                .all(|&(start, end)| {
                    self.base_to_target_map
                        .map_new_range_to_old(start, end)
                        .is_some()
                });
            if !all_mapped {
                return false;
            }
        }
        if kind.needs_new_mask() {
            self.reveal.regions.iter().all(|region| {
                request
                    .offset_map
                    .map_old_range_to_new(region.range.0, region.range.1)
                    .is_some()
            })
        } else {
            true
        }
    }

    /// Issue #826 评论 15：pipeline 用来**重建**吞字 overlay 纹理的资源集合。
    ///
    /// 按 `snapshot_id` 去重：多个 glyph 可能引用同一行图。
    pub(crate) fn active_conceal_source_lines(&self) -> Vec<ConcealSourceLine> {
        self.conceal_sources.clone()
    }

    /// Issue #826 评论 14/17：当前仍可见的旧字真正引用的行纹理 id。
    ///
    /// 直接从 `conceal_glyphs` 收集 —— 已完全吞掉的 glyph 已经从这里移除，
    /// 它的行图也就不再是 active owner。
    pub(crate) fn active_conceal_snapshot_ids(&self) -> Vec<LineSnapshotId> {
        let mut ids: Vec<LineSnapshotId> = Vec::new();
        for glyph in &self.conceal_glyphs {
            if !ids.contains(&glyph.snapshot_id) {
                ids.push(glyph.snapshot_id);
            }
        }
        ids
    }

    /// Issue #826：开始一轮吐字。
    pub(crate) fn begin_insert(
        base_text: String,
        target_snapshot: EditorLayoutSnapshot,
        target_text: String,
        new_ranges: Vec<(usize, usize)>,
        base_to_target_map: OffsetMap,
        started_at: Instant,
        duration_ms: u64,
    ) -> Self {
        let reveal = build_reveal_layer(&target_snapshot, &normalize_ranges(new_ranges));
        Self {
            kind: EditFrontierKind::Insert,
            // 纯吐字不需要旧正文 overlay，base_snapshot 与 target 相同。
            base_snapshot: target_snapshot.clone(),
            target_snapshot,
            conceal: FrontierLayer::default(),
            reveal,
            conceal_glyphs: Vec::new(),
            conceal_sources: Vec::new(),
            conceal_direction: ConcealDirection::Forward,
            started_at,
            duration_ms: duration_ms.max(1),
            // Issue #826 评论 11：纯 Insert 不画旧正文 overlay，但 `base_text` /
            // `base_to_target_map` 必须同属 burst 开始前那一份正文。
            base_text,
            target_text,
            base_to_target_map,
        }
    }

    /// Issue #826：开始一轮吞字。overlay 用删除开始前的旧正文。
    pub(crate) fn begin_delete(
        base_snapshot: EditorLayoutSnapshot,
        base_text: String,
        target_snapshot: EditorLayoutSnapshot,
        target_text: String,
        old_ranges: Vec<(usize, usize)>,
        base_to_target_map: OffsetMap,
        // Issue #826 评论 13/17：上一帧还在 Reflow、这一笔变成 changed old text
        // 的 glyph 的屏幕几何。空 slice 就是普通 Delete。
        reflow_current: &[ReflowCurrentGeometry],
        direction: ConcealDirection,
        started_at: Instant,
        duration_ms: u64,
    ) -> Self {
        let current_snapshot = base_snapshot.clone();
        let ranges = normalize_ranges(old_ranges);
        let (conceal_glyphs, conceal_sources) =
            collect_conceal_glyphs(&current_snapshot, &ranges, reflow_current);
        let conceal = build_conceal_layer(&current_snapshot, &ranges, direction, &conceal_glyphs);
        Self {
            kind: EditFrontierKind::Delete,
            base_snapshot,
            target_snapshot,
            conceal,
            reveal: FrontierLayer::default(),
            conceal_glyphs,
            conceal_sources,
            conceal_direction: direction,
            started_at,
            duration_ms: duration_ms.max(1),
            base_text,
            target_text,
            base_to_target_map,
        }
    }

    /// Issue #826：开始一轮替换。旧 overlay 收掉 + 新字 mask 打开共用同一个
    /// 时间 progress，但各自在自己的排版路径上采样。
    pub(crate) fn begin_replace(
        base_snapshot: EditorLayoutSnapshot,
        base_text: String,
        target_snapshot: EditorLayoutSnapshot,
        target_text: String,
        old_ranges: Vec<(usize, usize)>,
        new_ranges: Vec<(usize, usize)>,
        base_to_target_map: OffsetMap,
        reflow_current: &[ReflowCurrentGeometry],
        direction: ConcealDirection,
        started_at: Instant,
        duration_ms: u64,
    ) -> Self {
        let current_snapshot = base_snapshot.clone();
        let ranges = normalize_ranges(old_ranges);
        let (conceal_glyphs, conceal_sources) =
            collect_conceal_glyphs(&current_snapshot, &ranges, reflow_current);
        let conceal = build_conceal_layer(&current_snapshot, &ranges, direction, &conceal_glyphs);
        let reveal = build_reveal_layer(&target_snapshot, &normalize_ranges(new_ranges));
        Self {
            kind: EditFrontierKind::Replace,
            base_snapshot,
            target_snapshot,
            conceal,
            reveal,
            conceal_glyphs,
            conceal_sources,
            conceal_direction: direction,
            started_at,
            duration_ms: duration_ms.max(1),
            base_text,
            target_text,
            base_to_target_map,
        }
    }

    /// Issue #826 评论 17：连续吐字**只更新同一个前沿**。
    ///
    /// 连续按键不再"保留所有旧 track + 新增一条"（那正是议题正文禁止的
    /// 「按了多少次键就积多少个动画单元」）。这里是：
    /// 1. 采样当前单前沿，继承已走过距离；
    /// 2. 把已累计的 reveal range 用 Core 本次 OffsetMap 映到最新 target；
    /// 3. **相邻就合并**（不再坚持 `[1,2]` / `[2,3]` 必须永远分两条），
    ///    所以连打 100 次键盘仍然只有 1 个 region；
    /// 4. 在最新 target 上重建 path。
    pub(crate) fn extend_insert(
        &mut self,
        target_snapshot: EditorLayoutSnapshot,
        target_text: String,
        inserted_ranges: Vec<(usize, usize)>,
        prev_target_to_new: &OffsetMap,
        base_to_current: &OffsetMap,
        now: Instant,
    ) {
        let carried = map_ranges_forward(&self.new_ranges(), prev_target_to_new);
        let merged = normalize_ranges(merge_all(carried, inserted_ranges));
        let reveal = build_reveal_layer(&target_snapshot, &merged);
        // 继承**本帧已经推进到的位置**（而不是上一次 extend 存下的旧值），
        // 再按新路径总长等比缩放，保持连续推进不倒退。
        let inherited = self.reveal.inherited(self.sample(now).progress);
        let travelled = rescale(inherited, self.reveal.total_length(), reveal.total_length());
        self.base_to_target_map = base_to_current.compose(prev_target_to_new);
        self.target_snapshot = target_snapshot;
        self.target_text = target_text;
        self.reveal = reveal;
        self.reveal.travelled = travelled;
        self.started_at = now;
    }

    /// Issue #826 评论 17：连续吞字**只更新同一个前沿**。
    ///
    /// 同 extend_insert：相邻的 deleted range 合并进同一 region，已完全吞掉的
    /// glyph 当场从 `conceal_glyphs` / `conceal_sources` 移除。
    pub(crate) fn extend_delete(
        &mut self,
        target_snapshot: EditorLayoutSnapshot,
        target_text: String,
        deleted_ranges: Vec<(usize, usize)>,
        // Issue #826 评论 14：新进入删除的 glyph 的显示几何与贴图来源取本笔
        // 删除前用户正在看的 current old layout。
        current_snapshot: &EditorLayoutSnapshot,
        base_to_current: &OffsetMap,
        prev_target_to_new: &OffsetMap,
        reflow_current: &[ReflowCurrentGeometry],
        direction: ConcealDirection,
        now: Instant,
    ) {
        let mapped = map_ranges_backward(&deleted_ranges, base_to_current)
            .unwrap_or_else(|| identity_breakdown("extend_delete", &deleted_ranges));
        // 已累计的 old_ranges 本来就在 burst base 坐标；本次传入的 deleted_ranges
        // 属于「这一次编辑前」的文本，先用 base_to_current 映回 base。
        let mut ranges: Vec<(usize, usize)> = self.old_ranges();
        let incoming: Vec<(usize, usize)> = mapped.iter().map(|(_, base)| *base).collect();
        // glyph 几何与 Reflow handoff 都在**当前**坐标系里判定。
        let incoming_current: Vec<(usize, usize)> =
            mapped.iter().map(|(current, _)| *current).collect();
        for base_range in &incoming {
            ranges.push(*base_range);
        }
        // 相邻合并 —— 连续删除因此不会按按键次数累积 region。
        let merged = normalize_ranges(merge_all(ranges, Vec::new()));
        // Issue #826 评论 17：只把**本次新删**的 glyph 从 current snapshot 收进来，
        // 已经收集到的旧 glyph 必须保留 —— 它们来自更早的 snapshot（那一行的字
        // 早就从正文里消失了），但仍然在被吞、仍然要画、仍然要占着纹理。
        let (fresh_glyphs, fresh_sources) =
            collect_conceal_glyphs(current_snapshot, &incoming_current, reflow_current);
        let glyphs = merge_conceal_glyphs(self.conceal_glyphs.clone(), fresh_glyphs);
        let sources = merge_conceal_sources(self.conceal_sources.clone(), fresh_sources);
        let conceal = build_conceal_layer(current_snapshot, &merged, direction, &glyphs);
        let inherited = self.conceal.inherited(self.sample(now).progress);
        let travelled = rescale(
            inherited,
            self.conceal.total_length(),
            conceal.total_length(),
        );
        self.base_to_target_map = base_to_current.compose(prev_target_to_new);
        self.target_snapshot = target_snapshot;
        self.target_text = target_text;
        self.conceal_direction = direction;
        self.conceal = conceal;
        self.conceal.travelled = travelled;
        self.conceal_glyphs = glyphs;
        self.conceal_sources = sources;
        self.started_at = now;
    }

    /// Issue #826 评论 17：连续替换双侧都只更新同一个前沿。
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn extend_replace(
        &mut self,
        target_snapshot: EditorLayoutSnapshot,
        target_text: String,
        deleted_ranges: Vec<(usize, usize)>,
        inserted_ranges: Vec<(usize, usize)>,
        current_snapshot: &EditorLayoutSnapshot,
        base_to_current: &OffsetMap,
        prev_target_to_new: &OffsetMap,
        reflow_current: &[ReflowCurrentGeometry],
        direction: ConcealDirection,
        now: Instant,
    ) {
        let mapped = map_ranges_backward(&deleted_ranges, base_to_current)
            .unwrap_or_else(|| identity_breakdown("extend_replace", &deleted_ranges));
        let mut old: Vec<(usize, usize)> = self.old_ranges();
        let incoming: Vec<(usize, usize)> = mapped.iter().map(|(_, base)| *base).collect();
        let incoming_current: Vec<(usize, usize)> =
            mapped.iter().map(|(current, _)| *current).collect();
        for base_range in &incoming {
            old.push(*base_range);
        }
        let merged_old = normalize_ranges(merge_all(old, Vec::new()));
        let (fresh_glyphs, fresh_sources) =
            collect_conceal_glyphs(current_snapshot, &incoming_current, reflow_current);
        let glyphs = merge_conceal_glyphs(self.conceal_glyphs.clone(), fresh_glyphs);
        let sources = merge_conceal_sources(self.conceal_sources.clone(), fresh_sources);
        let conceal = build_conceal_layer(current_snapshot, &merged_old, direction, &glyphs);
        let conceal_travelled = self
            .conceal
            .inherited(self.sample(now).progress)
            .min(conceal.total_length());

        let carried_new = map_ranges_forward(&self.new_ranges(), prev_target_to_new);
        let merged_new = normalize_ranges(merge_all(carried_new, inserted_ranges));
        let reveal = build_reveal_layer(&target_snapshot, &merged_new);
        let reveal_travelled = self
            .reveal
            .inherited(self.sample(now).progress)
            .min(reveal.total_length());

        self.base_to_target_map = base_to_current.compose(prev_target_to_new);
        self.target_snapshot = target_snapshot;
        self.target_text = target_text;
        self.conceal_direction = direction;
        self.conceal = conceal;
        self.conceal.travelled = conceal_travelled;
        self.reveal = reveal;
        self.reveal.travelled = reveal_travelled;
        self.conceal_glyphs = glyphs;
        self.conceal_sources = sources;
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
        }
    }

    /// 前沿是否已经走完（可以收掉本轮遮罩/overlay）。
    pub(crate) fn is_finished(&self, now: Instant) -> bool {
        let progress = self.sample(now).progress;
        if self.kind.needs_new_mask() && !self.reveal.is_advanced_done(progress) {
            return false;
        }
        if self.kind.needs_old_overlay() && !self.conceal.is_advanced_done(progress) {
            return false;
        }
        true
    }

    /// 本帧吐字遮罩的裁剪矩形（只覆盖 inserted cluster）。
    ///
    /// Issue #826 评论 6 阻塞 3：**只裁 inserted cluster**，不能用整行内容右边界。
    /// `A|B` 中间插入 X 得 `AX|B`：A / X / B 三个 cluster 里只有 X 是 inserted，
    /// 若裁到整行右边界，B 会被 FrontierMask 一起挖掉——但 B 属于 Reflow 层职责。
    ///
    /// Issue #826 评论 7/8：边界来自**视觉路径**而不是一个二维 CursorRect。
    pub(crate) fn hidden_new_text_rects(&self, sample: &EditFrontierSample) -> Vec<FrontierRect> {
        if self.reveal.regions.is_empty() || !sample.masks_new_text() {
            return Vec::new();
        }
        let mut rects = Vec::new();
        for region in &self.reveal.regions {
            let distance = self.reveal.distance_at(region, sample.progress);
            let bounds = region.path.reveal_bounds(distance);
            for line in self
                .target_snapshot
                .lines_in_byte_range(region.range.0, region.range.1)
            {
                let Some(seg_index) = region.path.segment_index_for_line(line.id) else {
                    // 这一行没有可见 glyph（换行符），不产生 FrontierMask。
                    continue;
                };
                let Some(&(boundary, _right)) = bounds.get(seg_index) else {
                    continue;
                };
                for cluster in line.clusters_in_byte_range(region.range.0, region.range.1) {
                    let glyph = line.source_rect_to_document_rect(&cluster.source_rect);
                    let glyph_right = glyph.x + glyph.w;
                    let rect = if glyph.x >= boundary {
                        FrontierRect {
                            x: glyph.x,
                            y: glyph.y,
                            w: glyph.w,
                            h: glyph.h,
                        }
                    } else if glyph_right > boundary {
                        FrontierRect {
                            x: boundary,
                            y: glyph.y,
                            w: glyph_right - boundary,
                            h: glyph.h,
                        }
                    } else {
                        continue;
                    };
                    if !rect.is_degenerate() {
                        rects.push(rect);
                    }
                }
            }
        }
        rects
    }

    /// Issue #826 评论 17：本帧旧正文 overlay 要保留的矩形（全部 region 的合集）。
    ///
    /// 只用于汇总 / 展示 / 测试；真正裁 glyph 时必须用 region-local 的 keep
    /// （否则一条 region 的字会被别的 region 的 keep「救活」—— 评论 16）。
    pub(crate) fn old_overlay_rects(&self, sample: &EditFrontierSample) -> Vec<FrontierRect> {
        if self.conceal.regions.is_empty() || !sample.needs_old_overlay() {
            return Vec::new();
        }
        self.conceal
            .regions
            .iter()
            .flat_map(|region| self.region_conceal_rects(region, sample.progress))
            .collect()
    }

    /// 一条 region 本帧的 keep rect（region-local，评论 16）。
    fn region_conceal_rects(&self, region: &FrontierRegion, progress: f64) -> Vec<FrontierRect> {
        let distance = self.conceal.distance_at(region, progress);
        let bounds = region.path.conceal_bounds(distance);
        region
            .path
            .segments
            .iter()
            .zip(bounds.iter())
            .filter_map(|(segment, &(left, right))| {
                let rect = FrontierRect {
                    x: left.min(right),
                    y: segment.y,
                    w: (right - left).abs(),
                    h: segment.h,
                };
                if rect.is_degenerate() {
                    None
                } else {
                    Some(rect)
                }
            })
            .collect()
    }

    /// Issue #826 评论 17：本帧要画的旧正文 overlay glyph。
    ///
    /// keep 必须 **region-local**：一条 region 的字只能被自己的前沿裁剪，
    /// 不能被别的 region 的 keep 救回来（否则上一笔已吞掉的字会视觉复活）。
    pub(crate) fn old_overlay_glyphs(&self, sample: &EditFrontierSample) -> Vec<FrontierGlyph> {
        if self.conceal.regions.is_empty() || !sample.needs_old_overlay() {
            return Vec::new();
        }
        let mut glyphs = Vec::new();
        for region in &self.conceal.regions {
            let keep = self.region_conceal_rects(region, sample.progress);
            for geometry in &self.conceal_glyphs {
                if !overlaps(geometry.range, region.range) {
                    continue;
                }
                let source = geometry.source_rect.clone();
                let dest = geometry.dest_rect.clone();
                for (dest_x, dest_w) in clip_dest_to_rects(dest.clone(), &keep) {
                    if dest_w <= 0.0 {
                        continue;
                    }
                    let ratio = if dest.w > 0.0 {
                        (dest_x - dest.x) / dest.w
                    } else {
                        0.0
                    };
                    let src_x = source.x + source.w * ratio;
                    let src_w = (dest_w / dest.w.max(f64::MIN_POSITIVE)) * source.w;
                    glyphs.push(FrontierGlyph {
                        snapshot_id: geometry.snapshot_id,
                        source_rect: SourceRect {
                            x: src_x,
                            y: source.y,
                            w: src_w,
                            h: source.h,
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

/// Issue #826: 前沿驱动的单个旧正文 glyph。
/// Issue #826: 前沿驱动的单个旧正文 glyph。
#[derive(Clone, Debug)]
pub(crate) struct FrontierGlyph {
    pub snapshot_id: LineSnapshotId,
    /// 从旧行纹理里取这块的源矩形。
    pub source_rect: SourceRect,
    /// 画在屏幕上的目标矩形（文档坐标）。
    pub dest_rect: SourceRect,
}

impl FrontierGlyph {
    /// 本帧这块是否还要画。
    pub(crate) fn is_visible(&self) -> bool {
        self.dest_rect.w > 0.0 && self.dest_rect.h > 0.0
    }
}

/// Issue #826 评论 17：收集一条吞字 range 在 current snapshot 里的**全部可见 glyph**。
///
/// Reflow handoff 只覆盖其中部分 glyph 的 `dest_rect`（换成上一帧的屏幕位置），
/// 不再产生"只有 handed_off glyph、没有其他字"的那种特殊单元。
fn collect_conceal_glyphs(
    current_snapshot: &EditorLayoutSnapshot,
    ranges: &[(usize, usize)],
    reflow_current: &[ReflowCurrentGeometry],
) -> (Vec<ConcealGlyphGeometry>, Vec<ConcealSourceLine>) {
    let mut glyphs: Vec<ConcealGlyphGeometry> = Vec::new();
    let mut sources: Vec<ConcealSourceLine> = Vec::new();
    for &range in ranges {
        for line in current_snapshot.lines_in_byte_range(range.0, range.1) {
            if !sources.iter().any(|source| source.snapshot_id == line.id) {
                sources.push(ConcealSourceLine {
                    snapshot_id: line.id,
                    image: line.image.clone(),
                });
            }
            for cluster in line.clusters_in_byte_range(range.0, range.1) {
                let canonical = line.source_rect_to_document_rect(&cluster.source_rect);
                let glyph_range = (cluster.byte_start, cluster.byte_end);
                let sampled = reflow_current
                    .iter()
                    .find(|item| overlaps(item.current_range, glyph_range))
                    .map(|item| item.dest_rect.clone());
                glyphs.push(ConcealGlyphGeometry {
                    range: glyph_range,
                    snapshot_id: line.id,
                    source_rect: cluster.source_rect.clone(),
                    dest_rect: sampled.unwrap_or(canonical),
                });
            }
        }
    }
    (glyphs, sources)
}

/// Issue #826 评论 17：由当前 range + 当前 glyph 几何构建吞字层。
fn build_conceal_layer(
    current_snapshot: &EditorLayoutSnapshot,
    ranges: &[(usize, usize)],
    direction: ConcealDirection,
    glyphs: &[ConcealGlyphGeometry],
) -> FrontierLayer {
    let path_direction = PathDirection::from(direction);
    let parts: Vec<((usize, usize), FrontierPath)> = ranges
        .iter()
        .map(|&range| {
            let owned: Vec<ConcealGlyphGeometry> = glyphs
                .iter()
                .filter(|glyph| overlaps(glyph.range, range))
                .cloned()
                .collect();
            let path = if owned.is_empty() {
                FrontierPath::build(current_snapshot, range, path_direction)
            } else {
                FrontierPath::from_glyph_geometry(&owned, path_direction)
            };
            (range, path)
        })
        .collect();
    FrontierLayer::from_parts(parts)
}

/// 把旧路径上的已走过距离按新路径总长等比缩放（保留相对进度，不倒退）。
fn rescale(inherited: f64, old_total: f64, new_total: f64) -> f64 {
    if old_total <= 1e-9 {
        return 0.0;
    }
    let clamped = inherited.clamp(0.0, old_total);
    clamped * (new_total / old_total)
}

/// Issue #826 评论 17：合并两批吞字 glyph（按 range 去重）。
fn merge_conceal_glyphs(
    mut existing: Vec<ConcealGlyphGeometry>,
    fresh: Vec<ConcealGlyphGeometry>,
) -> Vec<ConcealGlyphGeometry> {
    // 身份是 `(snapshot_id, range)` 而不是单独的 range：连续删除跨两次编辑时，
    // 新 snapshot 里的 (0,3) 与旧 snapshot 里的 (0,3) 是**完全不同的文字**
    // （第一次删掉的那一行早就从正文消失了），byte range 相同但不是同一块 glyph。
    for glyph in fresh {
        if !existing
            .iter()
            .any(|item| item.range == glyph.range && item.snapshot_id == glyph.snapshot_id)
        {
            existing.push(glyph);
        }
    }
    existing
}

/// Issue #826 评论 17：合并两批行图来源（按 `snapshot_id` 去重）。
fn merge_conceal_sources(
    mut existing: Vec<ConcealSourceLine>,
    fresh: Vec<ConcealSourceLine>,
) -> Vec<ConcealSourceLine> {
    for source in fresh {
        if !existing
            .iter()
            .any(|item| item.snapshot_id == source.snapshot_id)
        {
            existing.push(source);
        }
    }
    existing
}

/// Issue #826 评论 17：构建吐字层（恒为正向视觉顺序）。
fn build_reveal_layer(
    target_snapshot: &EditorLayoutSnapshot,
    ranges: &[(usize, usize)],
) -> FrontierLayer {
    let parts: Vec<((usize, usize), FrontierPath)> = ranges
        .iter()
        .map(|&range| {
            (
                range,
                FrontierPath::build(target_snapshot, range, PathDirection::Forward),
            )
        })
        .collect();
    FrontierLayer::from_parts(parts)
}

/// Issue #826 评论 8 阻塞 3：changed range 集合的归一化。
///
/// 合并 overlap **和 adjacency**（`start <= last.1`）—— 评论 17 起连续按键的
/// 语义就是「相邻就合并成同一 region」，所以连打 100 次键盘仍然只有 1 个
/// region，状态大小不随按键次数增长。
fn normalize_ranges(ranges: Vec<(usize, usize)>) -> Vec<(usize, usize)> {
    let mut kept: Vec<(usize, usize)> = ranges
        .into_iter()
        .filter(|&(start, end)| end > start)
        .collect();
    kept.sort_unstable();
    let mut merged: Vec<(usize, usize)> = Vec::with_capacity(kept.len());
    for (start, end) in kept {
        match merged.last_mut() {
            Some(last) if start <= last.1 => last.1 = last.1.max(end),
            _ => merged.push((start, end)),
        }
    }
    merged
}

/// Issue #826 评论 17：两个范围是否**真正重叠**（半开区间）。
fn overlaps(a: (usize, usize), b: (usize, usize)) -> bool {
    a.0 < b.1 && b.0 < a.1
}

/// 把一批 range 映射到最新 target 坐标系（`old_to_new` 的方向）。
fn map_ranges_forward(ranges: &[(usize, usize)], old_to_new: &OffsetMap) -> Vec<(usize, usize)> {
    ranges
        .iter()
        .filter_map(|&(start, end)| old_to_new.map_old_range_to_new(start, end))
        .collect()
}

/// 把一批 range 映射回 burst base 坐标系，返回 `((current, base))` 配对。
///
/// `ConcealRegion.range` 用 base 坐标，但 glyph 的 `current_range` 用「上一轮
/// target == 本次 request.base」坐标 —— 两者不能直接求 overlap，所以必须保留配对。
fn map_ranges_backward(
    ranges: &[(usize, usize)],
    new_to_old: &OffsetMap,
) -> Option<Vec<((usize, usize), (usize, usize))>> {
    ranges
        .iter()
        .map(|&(start, end)| {
            new_to_old
                .map_new_range_to_old(start, end)
                .map(|base| ((start, end), base))
        })
        .collect()
}

/// 合并两组 range（调用方再做 `normalize_ranges` 归一化）。
fn merge_all(carried: Vec<(usize, usize)>, incoming: Vec<(usize, usize)>) -> Vec<(usize, usize)> {
    let mut all = carried;
    all.extend(incoming);
    all
}

/// Issue #826 评论 11 阻塞 2：动画身份断裂的**显式兜底**。
///
/// 走到这里说明 coordinator 绕过了 `can_extend_identity` 的 preflight。
/// 按 issue 的规则，正确行为是「当前 burst 到此结束、另开一轮」，而不是把映射
/// 失败的字静默丢掉。这里返回该 range 原样、让调用方继续，并写正式诊断事件。
fn identity_breakdown(
    site: &str,
    ranges: &[(usize, usize)],
) -> Vec<((usize, usize), (usize, usize))> {
    if ranges.is_empty() {
        return Vec::new();
    }
    let mut fields: std::collections::BTreeMap<String, serde_json::Value> =
        std::collections::BTreeMap::new();
    fields.insert("site".to_string(), serde_json::json!(site));
    fields.insert("ranges".to_string(), serde_json::json!(ranges));
    writer_diagnostics::record_event(writer_diagnostics::DiagnosticEvent {
        timestamp_ms: chrono::Utc::now().timestamp_millis(),
        sequence: 0,
        session_id: String::new(),
        level: writer_diagnostics::DiagnosticLevel::Warn,
        origin: writer_diagnostics::DiagnosticOrigin::App,
        event: "editor.anim.frontier.identity_breakdown".to_string(),
        target: "editor.anim".to_string(),
        message: Some(format!(
            "Issue #826 评论 11: {site} 出现动画身份断裂（映射失败），本次改动未被完整继承到当前 burst"
        )),
        fields,
    });
    ranges.iter().map(|&range| (range, range)).collect()
}

/// 把一个 glyph 的目标矩形按 keep 矩形裁成若干段 x 区间（返回 `(left, width)`）。
fn clip_dest_to_rects(dest: SourceRect, keep: &[FrontierRect]) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    for rect in keep {
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
