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
    /// Issue #826 评论 20：**部分露出**的那一段字，当前由这一小段 overlay
    /// 从「上一帧屏幕位置」补间到最新 canonical 位置。
    ///
    /// 只有当前视觉事实不再能用 scalar distance 表达时（几何或视觉顺序变了）
    /// 才非空；普通尾部 append 走绝对距离快路径，这里始终为空。
    pub(crate) reveal_carried: Vec<RevealCarriedPrefix>,
    /// Issue #826 评论 20：**已经完整露出、且已经不需要动画**的 cluster。
    ///
    /// 前沿不再遮罩它们（canonical 自己画），也不再把它们算进
    /// `pending_reveal_ranges` —— 位置真的变了就交给 Reflow 从旧位置补过去。
    pub(crate) reveal_settled: Vec<(usize, usize)>,
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

    /// Issue #826 评论 17：按时间 progress 推进后的**单一**前沿位置。
    ///
    /// `travelled` 是「连续编辑继承下来的已走过距离」，本函数按本帧的
    /// `ease_out_cubic(progress)` 推进成当前帧的位置。整层只有这一个时钟。
    pub(crate) fn advanced(&self, progress: f64) -> f64 {
        let total = self.total_length();
        let inherited = self.travelled.clamp(0.0, total);
        inherited + (total - inherited) * ease_out_cubic(progress)
    }

    /// 按视觉顺序遍历整条前沿路径的 segment（region 顺序 + region 内段序）。
    fn segments_in_order(&self) -> impl Iterator<Item = &FrontierSegment> {
        self.regions
            .iter()
            .flat_map(|region| region.path.segments.iter())
    }

    /// 前沿走到 `distance` 时边界落在哪个视觉位置：`(x, y, h)`。
    fn point_at(&self, distance: f64) -> Option<(f64, f64, f64)> {
        let mut rest = distance;
        for segment in self.segments_in_order() {
            if rest <= segment.visual_length + 1e-9 {
                return Some((segment.boundary_after(rest), segment.y, segment.h));
            }
            rest -= segment.visual_length;
        }
        None
    }

    /// Issue #826 评论 20：前 `distance` 像素是不是**同一批字、同一块屏幕位置**。
    ///
    /// 这是「绝对距离能不能直接继承」的唯一判据。评论 18 把「按百分比 rescale」
    /// 改成「保留绝对距离」只对 `旧 path = A，新 path = A + B 且 A 完全没变` 成立；
    /// 只要新 path 的前半段换了字、换了行或换了顺序，同一个 `8.75` 就不再代表
    /// 「上一帧真正已经露出的那一块字」。
    ///
    /// 判据是「边界在这几个距离上落在同一处」：采样点取**两条**路径在前
    /// `distance` 内的全部 segment 边界加上 `distance` 本身，然后逐点比
    /// `(x, y, h)`。
    ///
    /// 不能逐 segment 比 `x_from` / `x_to` —— `FrontierPath::build` 是**每个视觉行
    /// 一段**，所以普通尾部 append 会把同一行从 `x 0..10` 变成 `x 0..20`，
    /// `x_to` 不同但前半段几何其实完全没变。
    ///
    /// 只比几何，**不比 `line_id`** —— 同一视觉行在不同 snapshot 里的
    /// `LineSnapshotId` 本来就不同。
    pub(crate) fn shares_geometry_prefix(&self, other: &Self, distance: f64) -> bool {
        if distance <= 1e-9 {
            // 前沿还在起点：继承 0 与从 0 重起等价，走快路径即可。
            return true;
        }
        let mut samples: Vec<f64> = vec![distance];
        for layer in [self, other] {
            let mut acc = 0.0;
            for segment in layer.segments_in_order() {
                acc += segment.visual_length;
                if acc >= distance - 1e-9 {
                    break;
                }
                if acc > 1e-9 {
                    samples.push(acc);
                }
            }
        }
        samples.sort_by(|a, b| a.total_cmp(b));
        samples.dedup_by(|a, b| (*a - *b).abs() <= 1e-9);
        samples
            .into_iter()
            .all(|at| match (self.point_at(at), other.point_at(at)) {
                // 任意一条 path 走不到这个距离 -> 新 path 比旧 path 还短，
                // 继承距离没有意义。
                (Some((ax, ay, ah)), Some((bx, by, bh))) => {
                    (ax - bx).abs() <= 1e-9 && (ay - by).abs() <= 1e-9 && (ah - bh).abs() <= 1e-9
                }
                _ => false,
            })
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
    /// 本 glyph 在**当前** snapshot 里的 byte 范围（取 cluster、算 dest 用）。
    pub range: (usize, usize),
    /// 本 glyph 对应的 **burst base identity**（给 region ownership 用）。
    ///
    /// Issue #826 评论 19：`range` 用当前坐标，而 region 必须按编辑身份
    /// （base 坐标）归属 —— 否则一个已经完全没有 glyph 的历史 region 只能拿
    /// base byte range 回去 `FrontierPath::build(current_snapshot, ...)`，
    /// 而那里已经是删完之后的新正文，同一 byte 坐标现在是**别的活着的字符**，
    /// 会造出一段没有 glyph 可画、却照样吃单前沿 distance 的**幽灵路径**。
    ///
    /// fresh glyph 在 collect 时已经有 `(current_range, base_range)` 配对，
    /// 这一层对应关系不能丢。
    pub base_range: (usize, usize),
    /// 贴图来源行纹理。
    pub snapshot_id: LineSnapshotId,
    /// 从上面那张行纹理取这块的源矩形。
    pub source_rect: SourceRect,
    /// 这一轮真正画在屏幕上的目标矩形（文档坐标）。
    pub dest_rect: SourceRect,
}

/// Issue #826 评论 20：一次 retarget 采下的「当前屏幕上已经看见的那一段字」。
///
/// 这是**唯一**带字符身份的 Reveal 视觉事实：`travelled = 8.75` 只说明沿路径走了
/// 8.75px，一旦新 path 的顺序或几何变了，这 8.75px 就不再对应同一批字。
#[derive(Clone, Debug, PartialEq)]
struct RevealVisibleSample {
    /// 字符身份（当时的 target 坐标系）。
    range: (usize, usize),
    /// 这一帧它在屏幕上的矩形（文档坐标）。
    rect: SourceRect,
    /// 这一帧已经露出的宽度（`0..= rect.w`）。
    visible_width: f64,
}

/// Issue #826 评论 20：已可见前缀的一次性补间交接。
///
/// 吐字 retarget 时，「上一帧真正看得见的那几个像素」必须一路拥有到最新
/// canonical 位置：canonical layout 更新（换行、reflow、新的 patch 插到前面）
/// 会把这段字挪走，但**已经看见的像素不能瞬移**。
///
/// 这**不是**历史动画单元：
/// - 不带自己的 `started_at`，用与前沿同一个时钟；
/// - 不排队，每次 retarget 从当前视觉事实整体重建；
/// - 每个条目对应前沿 path 上的**一个边界 cluster**（每条 region 最多一个），
///   所以条目数 ∝ 不相交 patch 数，不随按键次数增长。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RevealCarriedPrefix {
    /// 字符身份（最新 target 坐标系）。
    pub(crate) range: (usize, usize),
    /// 上一帧它在屏幕上的矩形 —— 补间起点。
    pub(crate) from_rect: SourceRect,
    /// 最新 target 里同一段字的新矩形 —— 补间终点。
    pub(crate) to_rect: SourceRect,
    /// 上一帧已经露出的宽度。补间期间按同一进度增长到 `to_rect.w`。
    pub(crate) visible_width: f64,
    /// 贴图来源：最新 target 的行纹理。
    ///
    /// 吐字画的就是 canonical 新字，所以用新行纹理在旧位置补间是正确的
    /// —— `ReflowSpan` 一直是这么做的（`snapshot_id`/`source_rect` 取新行）。
    pub(crate) snapshot_id: LineSnapshotId,
    /// 上面那张行纹理里的源矩形（覆盖整个 cluster）。
    pub(crate) source_rect: SourceRect,
}

/// 一个 carry 条目在某一帧的实际画面。
#[derive(Clone, Debug, PartialEq)]
struct RevealCarriedFrame {
    rect: SourceRect,
    visible_width: f64,
}

impl RevealCarriedPrefix {
    /// 按前沿的同一个 progress 采样：位置与可见宽度一起补间。
    fn sample(&self, progress: f64) -> RevealCarriedFrame {
        let t = ease_out_cubic(progress);
        let lerp = |from: f64, to: f64| from + (to - from) * t;
        let rect = SourceRect {
            x: lerp(self.from_rect.x, self.to_rect.x),
            y: lerp(self.from_rect.y, self.to_rect.y),
            w: lerp(self.from_rect.w, self.to_rect.w),
            h: lerp(self.from_rect.h, self.to_rect.h),
        };
        let visible_width = lerp(self.visible_width, self.to_rect.w).clamp(0.0, rect.w);
        RevealCarriedFrame {
            rect,
            visible_width,
        }
    }
}

impl EditFrontierState {
    /// 本轮吞字的全部旧文字范围（burst base 坐标系）。
    ///
    /// Issue #826 评论 17：本轮仍由前沿负责的**旧文字范围**（burst base 坐标）。
    ///
    /// Issue #826 评论 19：语义收紧成「**当前屏幕上还有 glyph 在被吞**的 owner」。
    /// 已经被完全吞干净的 base range 不再出现 —— 没有 glyph 的 region 只是
    /// 一段吃 distance 却什么都画不出来的幽灵路径。
    ///
    /// 评论 19 之后吞字侧不再需要「历史累计范围」来重建 region（region 只由
    /// 仍有可见 glyph 的 owner 生成），所以这个入口只剩测试在用。
    #[cfg(test)]
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

    /// Issue #826 评论 21 阻塞 3：吐字侧仍归 Reveal 拥有的全部 range。
    ///
    /// **不能只读 `new_ranges()`**：评论 21 之后 `reveal.regions` 只包含
    /// 「还需要 FrontierMask 从 0 打开」的 range，而 `reveal_carried` 已经退出
    /// scalar path。下一笔 retarget 要靠这些 range 的身份把 carry 里的字映到
    /// 最新 target，所以必须显式合并，不能靠 regions 顺带带上。
    pub(crate) fn active_reveal_owned_ranges(&self) -> Vec<(usize, usize)> {
        normalize_ranges(
            self.new_ranges()
                .into_iter()
                .chain(self.reveal_carried.iter().map(|carried| carried.range))
                .collect(),
        )
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
            // Issue #826 评论 18 阻塞 3：**逐视觉行**用自己那条 segment 的 boundary。
            //
            // `reveal_bounds` 是每个 segment 各有一个 boundary，不能只取第一条。
            // 反例：
            // ```text
            // 第 1 行 changed segment: x = 80..100，长度 20
            // 第 2 行 changed segment: x = 0..100，长度 100
            // distance = 30
            // ```
            // 实际是「第 1 行 20px 全露完、第 2 行只露了 10px」。只取第一条
            // boundary(=100) 去判断第 2 行，会把第 2 行大量甚至全部 glyph 误判成
            // 已完全露出、从 `excluded_new` 提前移除 —— 屏幕上还没吐出来的字可能被
            // Reflow 直接画出来，穿过 Reveal mask 的 ownership。
            let bounds = region.path.reveal_bounds(distance);
            for line in self
                .target_snapshot
                .lines_in_byte_range(region.range.0, region.range.1)
            {
                let Some(seg_index) = region.path.segment_index_for_line(line.id) else {
                    // 这一行没有可见 glyph（换行符），不参与判定。
                    continue;
                };
                let Some(&(boundary, _right)) = bounds.get(seg_index) else {
                    continue;
                };
                for cluster in line.clusters_in_byte_range(region.range.0, region.range.1) {
                    let range = (
                        cluster.byte_start.max(region.range.0),
                        cluster.byte_end.min(region.range.1),
                    );
                    // Issue #826 评论 21：`settled` 已完全退出 `reveal.regions`
                    // （`subtract_ranges`），所以这里遍历到的都是还需要遮罩的字。
                    // `reveal_settled` 仍保留只是作为「已释放给 canonical / Reflow」
                    // 的显式记录，判定保持幂等。
                    if self
                        .reveal_settled
                        .iter()
                        .any(|&settled| overlaps(range, settled))
                    {
                        continue;
                    }
                    let glyph = line.source_rect_to_document_rect(&cluster.source_rect);
                    // 只有整个 glyph 完全越过**它自己那一行**的 boundary 才算露出。
                    if glyph.x + glyph.w > boundary + 1e-9 {
                        pending.push(range);
                    }
                }
            }
        }
        // Issue #826 评论 20：被 carry 接管的那一段字仍归 Reveal —— Reflow 绝不能
        // 同时搬它，否则 carry overlay 与 Reflow 会各画一份。
        for carried in &self.reveal_carried {
            pending.push(carried.range);
        }
        normalize_ranges(pending)
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
            reveal_carried: Vec::new(),
            reveal_settled: Vec::new(),
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
        // begin 时 burst base 就是 current old layout，owner 与 current 同坐标。
        let pairs: Vec<((usize, usize), (usize, usize))> =
            ranges.iter().map(|&range| (range, range)).collect();
        let (conceal_glyphs, conceal_sources) =
            collect_conceal_glyphs(&current_snapshot, &pairs, reflow_current);
        let conceal = build_conceal_layer(direction, &conceal_glyphs);
        Self {
            kind: EditFrontierKind::Delete,
            base_snapshot,
            target_snapshot,
            conceal,
            reveal: FrontierLayer::default(),
            reveal_carried: Vec::new(),
            reveal_settled: Vec::new(),
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
        // begin 时 burst base 就是 current old layout，owner 与 current 同坐标。
        let pairs: Vec<((usize, usize), (usize, usize))> =
            ranges.iter().map(|&range| (range, range)).collect();
        let (conceal_glyphs, conceal_sources) =
            collect_conceal_glyphs(&current_snapshot, &pairs, reflow_current);
        let conceal = build_conceal_layer(direction, &conceal_glyphs);
        let reveal = build_reveal_layer(&target_snapshot, &normalize_ranges(new_ranges));
        Self {
            kind: EditFrontierKind::Replace,
            base_snapshot,
            target_snapshot,
            conceal,
            reveal,
            reveal_carried: Vec::new(),
            reveal_settled: Vec::new(),
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
    /// Issue #826 评论 18 阻塞 2：把旧 overlay 收成**这一帧真正还看得见**的几何。
    ///
    /// 用当前 region-local keep 裁每个旧 glyph：
    /// - 已完全吞掉的（keep 里没有它）-> 直接丢弃；
    /// - 部分吞掉的 -> 裁成当前真正剩下的 source_rect / dest_rect；
    /// - 仍完整可见的 -> 原样保留。
    ///
    /// 这样每次 extend 之后 state 只保存「这一帧屏幕上还存在的旧 overlay」，
    /// 而不是「这轮历史上一共删过的全部内容」。
    fn sample_visible_conceal_geometry(&self, progress: f64) -> Vec<ConcealGlyphGeometry> {
        if self.conceal.regions.is_empty() {
            return Vec::new();
        }
        let mut out: Vec<ConcealGlyphGeometry> = Vec::new();
        for region in &self.conceal.regions {
            let keep = self.region_conceal_rects(region, progress);
            for geometry in &self.conceal_glyphs {
                // Issue #826 评论 19：归属必须按 **base identity** 判，不能拿
                // glyph 的当前坐标 range 去和 region 的 base range 求交 ——
                // 跨 revision 之后两者坐标系已经不同，同一个字节偏移在两边指的根本
                // 不是同一块字。
                if !overlaps(geometry.base_range, region.range) {
                    continue;
                }
                let dest = geometry.dest_rect.clone();
                let clipped = clip_dest_to_rects(dest.clone(), &keep);
                if clipped.is_empty() {
                    // 已经被完全吞掉 —— 丢弃，连同它的纹理 owner。
                    continue;
                }
                let source = geometry.source_rect.clone();
                let width_ratio = if dest.w > 0.0 { source.w / dest.w } else { 0.0 };
                for (dest_x, dest_w) in clipped {
                    out.push(ConcealGlyphGeometry {
                        range: geometry.range,
                        base_range: geometry.base_range,
                        snapshot_id: geometry.snapshot_id,
                        source_rect: SourceRect {
                            x: source.x + (dest_x - dest.x) * width_ratio,
                            y: source.y,
                            w: dest_w * width_ratio,
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
        out
    }

    /// Issue #826 评论 18 阻塞 2：从**下一帧仍在的 glyph** 重新收集行图来源。
    ///
    /// 不是「旧 sources + fresh sources」—— 已经被完全吞掉的 glyph 对应的
    /// QImage owner 必须当场释放，不等整轮 burst 最终停键。
    fn sources_for_glyphs(
        &self,
        glyphs: &[ConcealGlyphGeometry],
        fresh: &[ConcealSourceLine],
    ) -> Vec<ConcealSourceLine> {
        let mut out: Vec<ConcealSourceLine> = Vec::new();
        for glyph in glyphs {
            if out
                .iter()
                .any(|source| source.snapshot_id == glyph.snapshot_id)
            {
                continue;
            }
            // 先找本笔新收进来的，再回退到上一轮仍在的。
            if let Some(existing) = fresh
                .iter()
                .find(|source| source.snapshot_id == glyph.snapshot_id)
            {
                out.push(existing.clone());
            } else if let Some(existing) = self
                .conceal_sources
                .iter()
                .find(|source| source.snapshot_id == glyph.snapshot_id)
            {
                out.push(existing.clone());
            }
        }
        out
    }

    /// Issue #826 评论 21 结构偏差修复：**完整露出**的 cluster 按「是否完整露出」判定，
    /// 不再看位置有没有变。
    ///
    /// 之前写成 `same_rect && 完整露出 -> settled，否则 carry`，于是「完整露出 +
    /// 位置变了」也长出一个 `RevealCarriedPrefix`。一次 rewrap 把 50 个已完整露出的
    /// 旧插入字挪位置就会长出 50 个 carry，状态量又跟历史已露字符数一起膨胀 ——
    /// 直接违反「每条 region 最多一个边界 cluster carry」。
    ///
    /// 现在按可见宽度分：完整露出的退出前沿（位置没变由 canonical 画，位置变了由
    /// Reflow 用 `request.base_snapshot` 的旧矩形从旧位置补过去），只有真正**部分**
    /// 露出的边界 cluster 才 carry。
    fn reveal_fully_revealed(visible_width: f64, rect: &SourceRect) -> bool {
        visible_width >= rect.w - 1e-9
    }

    /// Issue #826 评论 21 阻塞 2/3：这段字已经不由前沿的遮罩负责了。
    ///
    /// `settled`（已完整露出，位置变了交给 Reflow）与 `carried`（正在用 overlay
    /// 从旧位置补间）都已从 `reveal.regions` 里挖掉，canonical 目标位置必须让位：
    /// - `settled`：canonical 自己画（若位置变了由 Reflow 搬），**不能挖**；
    /// - `carried`：overlay 在画，canonical 挖掉。
    fn reveal_mask_exempt(&self, range: (usize, usize)) -> bool {
        self.reveal_settled
            .iter()
            .chain(self.reveal_carried.iter().map(|carried| &carried.range))
            .any(|&exempt| overlaps(range, exempt))
    }

    /// Issue #826 评论 20：采「这一帧屏幕上已经看得见的吐字内容」。
    ///
    /// 这是 retarget 时唯一带字符身份的事实。`travelled = 8.75` 只说明沿路径走了
    /// 8.75px；新 path 一旦换行或换顺序，这 8.75px 对应的是别的字。
    fn sample_visible_reveal(&self, progress: f64) -> Vec<RevealVisibleSample> {
        let mut out: Vec<RevealVisibleSample> = Vec::new();
        for region in &self.reveal.regions {
            let distance = self.reveal.distance_at(region, progress);
            if distance >= region.path.total_length - 1e-9 {
                // 这一条 region 已经全部露完，屏幕上没有它的像素了。
                continue;
            }
            let bounds = region.path.reveal_bounds(distance);
            for line in self
                .target_snapshot
                .lines_in_byte_range(region.range.0, region.range.1)
            {
                let Some(seg_index) = region.path.segment_index_for_line(line.id) else {
                    continue;
                };
                let Some(&(boundary, _right)) = bounds.get(seg_index) else {
                    continue;
                };
                for cluster in line.clusters_in_byte_range(region.range.0, region.range.1) {
                    let rect = line.source_rect_to_document_rect(&cluster.source_rect);
                    let visible = (boundary - rect.x).clamp(0.0, rect.w);
                    if visible <= 1e-9 {
                        continue;
                    }
                    out.push(RevealVisibleSample {
                        range: (
                            cluster.byte_start.max(region.range.0),
                            cluster.byte_end.min(region.range.1),
                        ),
                        rect,
                        visible_width: visible,
                    });
                }
            }
        }
        // 上一笔留下的 carry 也在屏上，而且它是像素真相：同一段字如果也落在
        // path 采样结果里（例如它还在某个 region 的 range 内），必须让 carry
        // 覆盖掉 —— path 边界算出来的可见宽度对被 carry 接管的字没有意义。
        for carried in &self.reveal_carried {
            let sample = carried.sample(progress);
            out.retain(|item| item.range != carried.range);
            out.push(RevealVisibleSample {
                range: carried.range,
                rect: sample.rect,
                visible_width: sample.visible_width,
            });
        }
        out
    }

    /// Issue #826 评论 20：连续吐字 retarget 的唯一入口。
    ///
    /// 分两类：
    /// - **A 几何没变、仍是 path 前缀**（普通尾部 append）—— 继续用绝对距离
    ///   快路径，实现与观感都不变；
    /// - **B 几何 / 视觉顺序变了** —— scalar distance 不再能表达「谁已经露出」，
    ///   改为从 `sample_visible_reveal` 采到的当前视觉事实重建：已完整露出的退出
    ///   前沿（交给 canonical / Reflow），已部分露出的那一小段用 overlay 从上一帧
    ///   屏幕位置补间到最新 canonical 位置。
    ///
    /// 两种情况都保持「单前沿 + 一个时钟」：carry 条目数 ∝ 不相交 patch 数
    /// （每条 region 最多一个边界 cluster），不随按键次数增长。
    fn retarget_reveal(
        &mut self,
        target_snapshot: &EditorLayoutSnapshot,
        merged: Vec<(usize, usize)>,
        prev_target_to_new: &OffsetMap,
        now: Instant,
    ) {
        let progress = self.sample(now).progress;
        let inherited = self.reveal.inherited(progress);
        let probe = build_reveal_layer(target_snapshot, &merged);
        let geometry_stable = self.reveal_carried.is_empty()
            && self.reveal_settled.is_empty()
            && self.reveal.shares_geometry_prefix(&probe, inherited);
        if geometry_stable {
            // A：path 只是被延长，前半段的字和位置都没变 —— 直接继承绝对距离。
            // 前沿仍在 `inherited` 处：上一笔还差的那点没露完，新输入的字仍然全藏。
            let total = probe.total_length();
            self.reveal = probe;
            self.reveal.travelled = inherit_distance(inherited, total);
            self.reveal_carried.clear();
            self.reveal_settled.clear();
            return;
        }

        // B：先把身份映到最新 target，再逐段决定「谁退出前沿 / 谁继续补间」。
        let visible = self.sample_visible_reveal(progress);
        let mut carried: Vec<RevealCarriedPrefix> = Vec::new();
        let mut settled: Vec<(usize, usize)> = Vec::new();
        for item in visible {
            let item_rect = item.rect.clone();
            let Some(range) = prev_target_to_new.map_old_range_to_new(item.range.0, item.range.1)
            else {
                // 这段字在本次事务里已经被删掉 —— 屏幕上不该再留着它的像素。
                continue;
            };
            let Some((snapshot_id, dest_rect, source_rect)) =
                find_cluster_geometry(target_snapshot, range)
            else {
                continue;
            };
            if Self::reveal_fully_revealed(item.visible_width, &item_rect) {
                // 完整露出：位置没变由 canonical 画，位置变了由 Reflow 用
                // `request.base_snapshot` 的旧矩形从旧位置补过去
                //（评论 20 要求的「已完整露出的 cluster 释放给 Reflow」）。
                settled.push(range);
                continue;
            }
            // 只有真正**部分露出的边界 cluster** 才 carry：「已经看见的像素必须
            // 有从旧位置到新位置的所有权」。
            // 注意部分露出但几何没动（评论 20 反例 2）也要走 carry：慢路径下
            // `travelled` 从 0 起，若不接管，前沿会把这段字整块吞掉。
            carried.push(RevealCarriedPrefix {
                range,
                from_rect: item_rect,
                to_rect: dest_rect,
                visible_width: item.visible_width,
                snapshot_id,
                source_rect,
            });
        }
        settled.sort_unstable();

        // Issue #826 评论 21 阻塞 3：settled 与 carried 都必须**退出 scalar
        // path**，不只是被 mask 豁免。
        //
        // 豁免只挡住了 `hidden_new_text_rects`，`advanced()` / `distance_at()`
        // 仍然把它们的 path 长度算进去，于是它们在 scalar frontier 上白占一段
        // 行程：
        // ```text
        // slow retarget 后 X 已 settled(10px)、Y 是刚输入真正要吐的(10px)
        // probe total = 20px，travelled 从 0 起
        // 前 10px 全是 settled X -> Y 在前沿走过 10px 之前一直 100% hidden
        // ease-out-cubic 要 advanced/total > 0.5 才开始碰到 Y
        // => 160ms 动画开头三十多毫秒「已输入 Y 但吐字完全没开始」
        // ```
        // 与评论 19 删掉 Conceal 幽灵 path 是同一类问题，只是这次在 Reveal。
        //
        // 所以 scalar path 只保留「还需要 FrontierMask 从 0 打开」的 range；
        // 身份保留在 `reveal_carried` 里（`active_reveal_owned_ranges` 显式合并）。
        let owned: Vec<(usize, usize)> = carried.iter().map(|item| item.range).collect();
        let mask_ranges = subtract_ranges(merged, &settled, &owned);

        let reveal = build_reveal_layer(target_snapshot, &mask_ranges);
        // 慢路径下前沿的职责只剩「把还没露出的新字从头打开」，时钟从 0 起。
        self.reveal = reveal;
        self.reveal.travelled = 0.0;
        self.reveal_carried = carried;
        self.reveal_settled = settled;
    }

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
        // Issue #826 评论 21：carry 已退出 scalar path（见
        // `active_reveal_owned_ranges`），身份必须显式带上，否则下一笔会丢掉它。
        let carried = map_ranges_forward(&self.active_reveal_owned_ranges(), prev_target_to_new);
        let merged = normalize_ranges(merge_all(carried, inserted_ranges));
        self.retarget_reveal(&target_snapshot, merged, prev_target_to_new, now);
        self.base_to_target_map = base_to_current.compose(prev_target_to_new);
        self.target_snapshot = target_snapshot;
        self.target_text = target_text;
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
        // 已累计的 old owner 本来就在 burst base 坐标；本次传入的 deleted_ranges
        // 属于「这一次编辑前」的文本，`map_ranges_backward` 把它映回 base，
        // 于是每个 fresh glyph 都带得到自己的 base owner。
        let mapped = map_ranges_backward(&deleted_ranges, base_to_current)
            .unwrap_or_else(|| identity_breakdown("extend_delete", &deleted_ranges));

        // Issue #826 评论 19：先把旧 overlay **收成这一帧真正还看得见的**，
        // 再并入本笔新删的。只增不减等于把历史视觉债从「N 个 ConcealTrack」换成
        // 「1 个 region + N 批历史 glyph/QImage」，并没有真正清掉。
        let visible = self.sample_visible_conceal_geometry(self.sample(now).progress);
        let (fresh_glyphs, fresh_sources) =
            collect_conceal_glyphs(current_snapshot, &mapped, reflow_current);
        let glyphs = merge_conceal_glyphs(visible, fresh_glyphs);
        let sources = self.sources_for_glyphs(&glyphs, &fresh_sources);
        // Issue #826 评论 19 阻塞 3：region 只从**仍有可见 glyph 的 base owner**
        // 生成（并按相邻合并），已经没有 glyph 的历史 owner 直接消失 ——
        // 绝不拿 base byte range 回 current snapshot 猜几何造幽灵路径。
        let conceal = build_conceal_layer(direction, &glyphs);

        self.base_to_target_map = base_to_current.compose(prev_target_to_new);
        self.target_snapshot = target_snapshot;
        self.target_text = target_text;
        self.conceal_direction = direction;
        self.conceal = conceal;
        // Issue #826 评论 19 阻塞 1：prune 之后新几何的原点已经重新定义成
        // 「distance 0 == 当前这一帧的屏幕状态」，所以**必须从 0 起**。
        //
        // 再把旧 path 上已经走过的绝对距离塞进来就是重复消费：
        // ```text
        // AB| 连续 Backspace，等宽 10px，80ms/160ms -> 旧 distance 8.75
        // B 屏幕真正还剩 1.25px
        // 第二笔删 A：prune 后可见几何 = B 的 1.25 + fresh A 的 10 = 11.25px
        // 若再 inherited 8.75 -> 第二笔第一帧就被预吞 8.75/11.25 ≈ 75%
        // ```
        // 这与 Reveal 不同：Reveal 没有裁掉旧几何，延长 path 可以继承绝对距离；
        // Conceal 已经把已吞像素彻底裁掉，消费过的距离不在新 path 里了。
        self.conceal.travelled = 0.0;
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
        // region 只从仍有可见 glyph 的 base owner 生成，历史里已经没有 glyph 的
        // owner 自然消失（不再拿 base byte range 回去猜 current snapshot 的几何）。
        // Issue #826 评论 18 阻塞 2：同 extend_delete，先 prune 再并入本笔新删的。
        let visible = self.sample_visible_conceal_geometry(self.sample(now).progress);
        let (fresh_glyphs, fresh_sources) =
            collect_conceal_glyphs(current_snapshot, &mapped, reflow_current);
        let glyphs = merge_conceal_glyphs(visible, fresh_glyphs);
        let sources = self.sources_for_glyphs(&glyphs, &fresh_sources);
        let conceal = build_conceal_layer(direction, &glyphs);

        // Issue #826 评论 21：同 `extend_insert`，carry 的身份要显式带上。
        let carried_new =
            map_ranges_forward(&self.active_reveal_owned_ranges(), prev_target_to_new);
        let merged_new = normalize_ranges(merge_all(carried_new, inserted_ranges));
        // Issue #826 评论 20：吐字侧同样必须分「几何没变」与「几何/顺序变了」，
        // 不能一律继承绝对距离。
        self.retarget_reveal(&target_snapshot, merged_new, prev_target_to_new, now);

        self.base_to_target_map = base_to_current.compose(prev_target_to_new);
        self.target_snapshot = target_snapshot;
        self.target_text = target_text;
        self.conceal_direction = direction;
        self.conceal = conceal;
        // Issue #826 评论 19 阻塞 1：吞字侧和 `extend_delete` 同理，prune 之后
        // distance 原点已经重新定义成「当前这一帧的屏幕状态」，必须从 0 起。
        self.conceal.travelled = 0.0;
        self.conceal_glyphs = glyphs;
        self.conceal_sources = sources;
        self.started_at = now;
    }

    /// Issue #826 评论 20：本帧要额外画的「已可见前缀」glyph。
    ///
    /// 与吞字的 `old_overlay_glyphs` 同一类东西，但只包含**吐字**那一侧
    /// 「已经看见的那几个像素」，且不携带历史 —— 每次 retarget 整体重建。
    pub(crate) fn reveal_carried_glyphs(&self, sample: &EditFrontierSample) -> Vec<FrontierGlyph> {
        if self.reveal_carried.is_empty() || !sample.masks_new_text() {
            return Vec::new();
        }
        let mut glyphs = Vec::new();
        for carried in &self.reveal_carried {
            let frame = carried.sample(sample.progress);
            if frame.visible_width <= 1e-9 || frame.rect.h <= 0.0 {
                continue;
            }
            // 源矩形按「可见宽度占整字宽的比例」裁右端，dest 则裁在当前屏幕上。
            let scale = if frame.rect.w > 0.0 {
                carried.source_rect.w / frame.rect.w
            } else {
                0.0
            };
            glyphs.push(FrontierGlyph {
                snapshot_id: carried.snapshot_id,
                source_rect: SourceRect {
                    x: carried.source_rect.x,
                    y: carried.source_rect.y,
                    w: frame.visible_width * scale,
                    h: carried.source_rect.h,
                },
                dest_rect: SourceRect {
                    x: frame.rect.x,
                    y: frame.rect.y,
                    w: frame.visible_width,
                    h: frame.rect.h,
                },
            });
        }
        glyphs
    }

    /// Issue #826 评论 20：carry 段落在 canonical 正文里要挖掉的目标矩形。
    ///
    /// carry 用的是**最新 target 的行纹理**，而 canonical 正文正在用同一张纹理
    /// 画它，所以静态层必须在这块让位，否则同一段字画两遍。
    pub(crate) fn reveal_carried_target_rects(
        &self,
        sample: &EditFrontierSample,
    ) -> Vec<(SourceRect, LineSnapshotId)> {
        if self.reveal_carried.is_empty() || !sample.masks_new_text() {
            return Vec::new();
        }
        self.reveal_carried
            .iter()
            .map(|carried| (carried.to_rect.clone(), carried.snapshot_id))
            .collect()
    }

    /// Issue #826 评论 21：carry 真正引用的行纹理 id（供 TextureCache 生命周期）。
    ///
    /// carry 用最新 target 的行纹理在**旧屏幕位置**画已可见前缀。纯 Insert 场景下
    /// `Conceal ids = []`、Reflow 也没有这个字的 span（X 未吐完 → 仍被 pending
    /// Reveal 排除出 Reflow），如果不把它登记成 active，retain 之后纹理可能已被
    /// 回收、prepare 也不会插回去 —— renderer 里 `get_line` 返回 None 直接跳过
    /// carry glyph，而它的 `ReflowTarget` clip 又因纹理 miss 被过滤，
    /// 结果就是「旧行半个 X」直接变成「新行完整 X」，评论 20 要修的瞬移复活。
    pub(crate) fn active_reveal_carried_snapshot_ids(&self) -> Vec<LineSnapshotId> {
        let mut ids: Vec<LineSnapshotId> = Vec::new();
        for carried in &self.reveal_carried {
            if !ids.contains(&carried.snapshot_id) {
                ids.push(carried.snapshot_id);
            }
        }
        ids
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
        if self.kind.needs_new_mask() {
            if !self.reveal.is_advanced_done(progress) {
                return false;
            }
            // Issue #826 评论 21：`settled` / `carried` 退出 scalar path 之后，
            // `reveal.regions` 可能**整条为空**而 carry 还在补间。此时
            // `reveal.is_advanced_done()` 恒为 true，绝不能据此立刻结束整轮 ——
            // 否则 carry 的 overlay 会中途消失、X 从半吐跳成完整（瞬移复活）。
            if !self.reveal_carried.is_empty() && progress < 1.0 {
                return false;
            }
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
                    let range = (
                        cluster.byte_start.max(region.range.0),
                        cluster.byte_end.min(region.range.1),
                    );
                    // Issue #826 评论 20：这段字已经不由前沿遮罩负责了 ——
                    // `settled` 由 canonical 自己画，`carried` 由 overlay 从旧位置
                    // 补间过来。继续挖遮罩会让它凭空消失。
                    // Issue #826 评论 21：`settled` 与 `carried` 都已从
                    // `reveal.regions` 里挖掉（`subtract_ranges`），这里能遍历到的
                    // 一定还需要 FrontierMask。判定保留为幂等的第二道防线。
                    if self.reveal_mask_exempt(range) {
                        continue;
                    }
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
    #[cfg(test)]
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
                // Issue #826 评论 19：region 是按 glyph 的 **base identity** 建的，
                // 所以归属判定也必须用 `base_range`；用当前坐标的 `range` 去和
                // region 的 base range 求交会在跨 revision 的连续编辑里漏掉 glyph。
                if !overlaps(geometry.base_range, region.range) {
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
    // `(current_range, base_range)` 配对：current 用于取 cluster / 算 dest，
    // base 是 region ownership。
    ranges: &[((usize, usize), (usize, usize))],
    reflow_current: &[ReflowCurrentGeometry],
) -> (Vec<ConcealGlyphGeometry>, Vec<ConcealSourceLine>) {
    let mut glyphs: Vec<ConcealGlyphGeometry> = Vec::new();
    let mut sources: Vec<ConcealSourceLine> = Vec::new();
    for &(range, base_range) in ranges {
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
                    base_range,
                    snapshot_id: line.id,
                    source_rect: cluster.source_rect.clone(),
                    dest_rect: sampled.unwrap_or(canonical),
                });
            }
        }
    }
    (glyphs, sources)
}

/// Issue #826 评论 19：region **只从仍有可见 glyph 的 owner 生成**。
///
/// 之前是「按历史 range 列表逐个建 region，某个 range 没有 owned glyph 就回
/// `FrontierPath::build(current_snapshot, range)`」—— 那段 fallback 会用
/// **burst base 的 byte range** 去已经删完的正文里找几何，同一坐标现在是别的
/// 活着的字符，于是造出一段没有 glyph 可画、却照样吃单前沿 distance 的幽灵路径。
/// 肉眼表现是「前沿在一个没有旧字 overlay 的位置空跑，删除中间顿一下」。
///
/// 单前沿状态定义是「只保留当前屏幕还存在的视觉状态」，所以没有任何剩余 glyph
/// 的 owner 直接不生成 region。
///
/// owner 仍然按 overlap / adjacency 归一化（[`normalize_ranges`]）—— 相邻的
/// deleted range 必须合并成同一个 region，否则连打 N 次键盘就会攒出 N 条 region，
/// 议题正文明确禁止那种「按按键次数累积动画单元」。
fn build_conceal_layer(
    direction: ConcealDirection,
    glyphs: &[ConcealGlyphGeometry],
) -> FrontierLayer {
    let path_direction = PathDirection::from(direction);
    let owners = normalize_ranges(glyphs.iter().map(|glyph| glyph.base_range).collect());
    let parts: Vec<((usize, usize), FrontierPath)> = owners
        .into_iter()
        .filter_map(|owner| {
            let owned: Vec<ConcealGlyphGeometry> = glyphs
                .iter()
                .filter(|glyph| overlaps(glyph.base_range, owner))
                .cloned()
                .collect();
            let path = FrontierPath::from_glyph_geometry(&owned, path_direction);
            // 归一化后的一组 owner 里如果一个 glyph 都不剩（本轮之前就已经被
            // 吞干净），就不要为它留一条空 region。
            if path.total_length <= 0.0 {
                return None;
            }
            Some((owner, path))
        })
        .collect();
    FrontierLayer::from_parts(parts)
}

/// Issue #826 评论 21 阻塞 3：从 merged range 里**挖掉**不再由 scalar frontier
/// 负责的段。
///
/// `reveal.regions` 只能包含「还需要 `FrontierMask` 从 0 打开」的 range：
/// - `settled` 已完整露出，位置没变由 canonical 画、位置变了由 Reflow 补；
/// - `carried` 由 overlay 用自己的 progress 补间。
///
/// 两者若仍留在 `regions` 里，就会白占 scalar frontier 的行程
/// （见 `retarget_reveal` 里那段 20px / 10px 的例子）。
///
/// `probe` 参数不需要 —— 只做减法，结果仍交给 `build_reveal_layer` 在最新
/// target 上按真实几何建 path。
fn subtract_ranges(
    merged: Vec<(usize, usize)>,
    settled: &[(usize, usize)],
    carried: &[(usize, usize)],
) -> Vec<(usize, usize)> {
    // 用 byte 粒度切：range 可能覆盖多行多 cluster，只按两端切会丢掉中间还需要
    // 的部分。这里逐字节判定「这段还要不要由 scalar frontier 打开」，再把连续
    // 的「还要」片段合并回去。
    let mut out: Vec<(usize, usize)> = Vec::new();
    for range in normalize_ranges(merged) {
        let mut start: Option<usize> = None;
        for byte in range.0..range.1 {
            let owned = settled
                .iter()
                .chain(carried.iter())
                .any(|exempt| overlaps((byte, byte + 1), *exempt));
            match (owned, start) {
                // 只有在真的走过至少一个「还要 mask」的字节之后才开段，
                // 否则会产出 (begin, begin) 这种空 range，被 `normalize_ranges`
                // 丢掉 —— 那样紧邻的下一笔新字就会整段失去遮罩。
                (false, None) => start = Some(byte),
                (false, Some(begin)) => {
                    if begin < byte {
                        out.push((begin, byte));
                    }
                }
                (true, Some(begin)) => {
                    out.push((begin, byte));
                    start = None;
                }
                (true, None) => {}
            }
        }
        if let Some(begin) = start {
            if begin < range.1 {
                out.push((begin, range.1));
            }
        }
    }
    normalize_ranges(out)
}

/// 在 snapshot 里按 byte 范围找到那一个 cluster 的
/// `(line id, 文档坐标矩形, 贴图源矩形)`。
///
/// 找不到返回 `None`：调用方据此判定「这段字在新正文里已经不存在」。
fn find_cluster_geometry(
    snapshot: &EditorLayoutSnapshot,
    range: (usize, usize),
) -> Option<(LineSnapshotId, SourceRect, SourceRect)> {
    for line in snapshot.lines_in_byte_range(range.0, range.1) {
        for cluster in line.clusters_in_byte_range(range.0, range.1) {
            return Some((
                line.id,
                line.source_rect_to_document_rect(&cluster.source_rect),
                cluster.source_rect.clone(),
            ));
        }
    }
    None
}

/// Issue #826 评论 18 阻塞 1：把「上一帧前沿的绝对距离」继承到新路径上。
///
/// **绝不能按新旧总长比例缩放。** 连续 append 时 path 只是**延长**：
/// ```text
/// 第一笔 1 个 10px 字，80ms/160ms -> 前沿在 8.75px
/// 第二笔再 append 1 个 10px 字 -> new_total = 20
/// ```
/// 单前沿的正确语义是「前沿仍在 8.75px」：第一个字还差 1.25px 吐完，
/// 第二个刚输入的字仍然完全藏住。按比例缩放会得到 8.75 * 20/10 = 17.5 ——
/// 第二个字第一帧就已经露了 75%，「快速输入没有动画」被重新做出来。
///
/// 连续 Delete 同理：刚删的字不能一进来就被预吞 75%。
///
/// 只有路径**几何真的重排**（自动换行）时绝对距离才不再等价，那时要按
/// 「已露出的字符身份」重建，而不是猜一个百分比。
fn inherit_distance(inherited: f64, new_total: f64) -> f64 {
    inherited.clamp(0.0, new_total)
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
