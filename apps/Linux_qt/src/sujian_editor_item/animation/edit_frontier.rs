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
    /// 吞字 track 集合（评论 9 阻塞 3）。每个 track 自己拥有 range / path / travelled，
    /// 动画状态按**编辑身份**保存，不靠平行数组下标对齐。
    pub(crate) conceal_tracks: Vec<ConcealTrack>,
    /// 吐字 track 集合（评论 9 阻塞 3）。语义同 ConcealTrack。
    pub(crate) reveal_tracks: Vec<RevealTrack>,
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

/// 本帧前沿走过的距离 = 继承的距离 + 本帧 ease 推进的剩余距离。
fn travelled(inherited: f64, total: f64, eased: f64) -> f64 {
    let inherited = inherited.clamp(0.0, total);
    inherited + (total - inherited) * eased
}

/// Issue #826 评论 9 阻塞 3：吐字 track。
///
/// `range` / `path` / `travelled` **由同一个 track 自己拥有**，不再拆成三个平行
/// `Vec` 靠下标对齐。评论里的反例说明为什么必须这样做：已有两段
/// `[10,12]` / `[100,102]` 且都走完，又来一笔插入在正文最前面，新 range `[0,2]`
/// 归一化排序后变成 `[0,2]` / `[12,14]` / `[102,104]` —— 按下标继承会让新 patch
/// 凭空拿到旧 patch 的进度、而已走完的最后一段反而回到 0。
#[derive(Clone, Debug)]
pub(crate) struct RevealTrack {
    /// 本 track 拥有的新文字范围（最新 target 坐标系）。
    pub(crate) range: (usize, usize),
    /// 本 track 的视觉路径（在最新 target snapshot 上重建）。
    pub(crate) path: FrontierPath,
    /// 已经走过的距离，跟随本 track 自身，不经过任何排序下标。
    pub(crate) travelled: f64,
}

/// Issue #826 评论 9 阻塞 3：吞字 track。语义同 `RevealTrack`。
#[derive(Clone, Debug)]
pub(crate) struct ConcealTrack {
    /// 本 track 拥有的旧文字范围（burst base 坐标系）。
    pub(crate) range: (usize, usize),
    /// 本 track 的视觉路径。
    ///
    /// 普通 Delete 从 burst base snapshot 的 canonical 几何构建；
    /// Reflow -> Delete 交接时从「上一帧 Reflow 的采样几何」构建
    /// （见 `build_conceal_tracks` 的 `current` 参数）。
    pub(crate) path: FrontierPath,
    /// 已经吞掉的距离，跟随本 track 自身。
    pub(crate) travelled: f64,
    /// Issue #826 评论 13：本 track 这一轮自己的旧字显示几何。
    ///
    /// `source_rect` 取自 burst base 的旧行纹理（贴图来源不变），
    /// `dest_rect` 是**这一帧它实际该画在哪**。
    ///
    /// 没有这一份的话，overlay 绘制会退回
    /// `line.source_rect_to_document_rect(&source)`，而 clip 前沿是按 sampled
    /// 几何算的 —— 会出现「前沿从 55 算、glyph 仍画在 60」的错位。
    /// ConcealTrack 必须真正拥有自己这一轮的 overlay dest geometry。
    pub(crate) glyphs: Vec<ConcealGlyphGeometry>,
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
    /// 供 coordinator 把本次 deleted ranges 映回 burst base 坐标。
    pub(crate) fn old_ranges(&self) -> Vec<(usize, usize)> {
        self.conceal_tracks
            .iter()
            .map(|track| track.range)
            .collect()
    }

    /// Issue #826 评论 11 阻塞 2：编辑**身份**是否连续 —— 决定这一笔能不能并入
    /// 当前 burst。
    ///
    /// 「同 kind + 同方向 + 未结束」还不够：当前这套「所有 ConcealTrack 都引用
    /// burst base snapshot」的模型，无法表示「这一笔要删的字是本 burst 中途才
    /// 产生的，它根本不在 burst base 里」。这时映射必然失败。
    ///
    /// 稳定反例：快速连续两次 Undo。正文历史 `a -> b -> c`，当前是 `c`。
    /// - 第一次 Undo `c -> b`：Replace，burst base = `c`。
    /// - 动画未结束立刻第二次 Undo `b -> a`：kind 相同、cursor 没动、
    ///   conceal direction 也相同，`can_extend` 本来是 true。
    ///   但第二笔要删的 `b` 是第一次 Undo **刚插出来的字**，它在 burst base `c`
    ///   里不存在 → `base_to_target_map.map_new_range_to_old(b_range)` 返回
    ///   `None`。旧代码 `filter_map` 静默丢掉 → `b` 没有 ConcealTrack；
    ///   同时它还在 Reveal 的 track 映射也失败 → `continue` 静默丢掉 →
    ///   **`b` 直接闪没**。这与 #826 最初要解决的「快速编辑时历史字突然消失」
    ///   是同一类问题，只是载体从旧 transaction queue 换成了 silent map failure。
    ///
    /// 规则：
    /// - **old 侧**（Delete / Replace）：本次每条非零 `deleted_ranges` 都必须能
    ///   通过 `self.base_to_target_map.map_new_range_to_old(..)` 完整映回 burst base。
    /// - **new 侧**（Insert / Replace）：每条仍需继承的旧 RevealTrack 都必须能
    ///   通过 `request.offset_map.map_old_range_to_new(..)` 映到 latest target。
    ///   当前编辑若正好把这条 reveal text 改掉，映射失败就不能静默丢 track。
    ///
    /// 身份不连续时这不是错误 fallback，而是**新的 burst 语义边界**：
    /// 当前 burst 到此结束，用 `request.base_snapshot` / `request.target_snapshot`
    /// 开一个新 burst。
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
            self.reveal_tracks.iter().all(|track| {
                request
                    .offset_map
                    .map_old_range_to_new(track.range.0, track.range.1)
                    .is_some()
            })
        } else {
            true
        }
    }

    /// Issue #826 评论 14 阻塞 4：本轮活跃吞字 overlay **真正要画**的行纹理 id。
    ///
    /// 直接从 `conceal_tracks[].glyphs[].snapshot_id` 收集 —— ConcealTrack 已经
    /// 明确知道自己画什么，不再从 `old_ranges + base_snapshot` 推测。
    ///
    /// 这个区别在**同 burst handoff** 时是致命的：`base_snapshot` 是 burst 第一笔
    /// 之前的快照，而 handoff glyph 的贴图来自「上一轮 Reflow target」快照，两边
    /// line id 不同。之前 active ids 只含 burst base 的 id，于是
    /// `texture_cache.retain_active_snapshot_ids()`（实现就是 `line_store.retain`）
    /// 会先把 handoff 那张图删掉；新 Reflow 因该 glyph 已 changed 不再声明它，
    /// old overlay 纹理准备也只看 burst base 补不回来 —— renderer 找不到
    /// `ConcealGlyphGeometry.snapshot_id`，这个 glyph 直接 skip，真机画不出来。
    /// coordinator 单测能看到 overlay geometry，不代表 Scene Graph 一定画得出来。
    pub(crate) fn active_conceal_snapshot_ids(&self) -> Vec<LineSnapshotId> {
        let mut ids: Vec<LineSnapshotId> = Vec::new();
        for track in &self.conceal_tracks {
            for glyph in &track.glyphs {
                if !ids.contains(&glyph.snapshot_id) {
                    ids.push(glyph.snapshot_id);
                }
            }
        }
        ids
    }

    /// 本轮吐字的全部新文字范围（最新 target 坐标系）。
    pub(crate) fn new_ranges(&self) -> Vec<(usize, usize)> {
        self.reveal_tracks.iter().map(|track| track.range).collect()
    }

    /// 开始一轮吐字。
    pub(crate) fn begin_insert(
        base_text: String,
        target_snapshot: EditorLayoutSnapshot,
        target_text: String,
        new_ranges: Vec<(usize, usize)>,
        base_to_target_map: OffsetMap,
        started_at: Instant,
        duration_ms: u64,
    ) -> Self {
        let reveal_tracks =
            build_reveal_tracks(&target_snapshot, &normalize_track_ranges(new_ranges));
        Self {
            kind: EditFrontierKind::Insert,
            // 纯吐字不需要旧正文 overlay，base_snapshot 与 target 相同。
            base_snapshot: target_snapshot.clone(),
            target_snapshot,
            conceal_tracks: Vec::new(),
            reveal_tracks,
            conceal_direction: ConcealDirection::Forward,
            started_at,
            duration_ms: duration_ms.max(1),
            // Issue #826 评论 11：纯 Insert 不画旧正文 overlay，但 `base_text` /
            // `base_to_target_map` 必须同属 burst 开始前那一份正文。
            // 之前这里写的是 `target_text.clone()`，而 `base_to_target_map`
            // 是 old -> target，两者指向不同 revision，state invariant 是假的。
            base_text,
            target_text,
            base_to_target_map,
        }
    }

    /// 开始一轮吞字。overlay 用删除开始前的旧正文。
    pub(crate) fn begin_delete(
        base_snapshot: EditorLayoutSnapshot,
        base_text: String,
        target_snapshot: EditorLayoutSnapshot,
        target_text: String,
        old_ranges: Vec<(usize, usize)>,
        base_to_target_map: OffsetMap,
        // Issue #826 评论 13：上一帧还在 Reflow、这一笔变成 changed old text 的
        // glyph 的屏幕几何。空 slice 就是普通 Delete（走 canonical 几何）。
        reflow_current: &[ReflowCurrentGeometry],
        direction: ConcealDirection,
        started_at: Instant,
        duration_ms: u64,
    ) -> Self {
        // begin 场景：burst base 就是 request.base，current_range == base_range。
        let conceal_tracks = build_conceal_tracks_for_ranges(
            &base_snapshot,
            &normalize_track_ranges(old_ranges),
            direction,
            reflow_current,
        );
        Self {
            kind: EditFrontierKind::Delete,
            base_snapshot,
            target_snapshot,
            conceal_tracks,
            reveal_tracks: Vec::new(),
            conceal_direction: direction,
            started_at,
            duration_ms: duration_ms.max(1),
            base_text,
            target_text,
            base_to_target_map,
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
        old_ranges: Vec<(usize, usize)>,
        new_ranges: Vec<(usize, usize)>,
        base_to_target_map: OffsetMap,
        // Issue #826 评论 13：上一帧还在 Reflow、这一笔变成 changed old text 的
        // glyph 的屏幕几何。空 slice 就是普通 Delete（走 canonical 几何）。
        reflow_current: &[ReflowCurrentGeometry],
        direction: ConcealDirection,
        started_at: Instant,
        duration_ms: u64,
    ) -> Self {
        // begin 场景：burst base 就是 request.base，current_range == base_range。
        let conceal_tracks = build_conceal_tracks_for_ranges(
            &base_snapshot,
            &normalize_track_ranges(old_ranges),
            direction,
            reflow_current,
        );
        let reveal_tracks =
            build_reveal_tracks(&target_snapshot, &normalize_track_ranges(new_ranges));
        Self {
            kind: EditFrontierKind::Replace,
            base_snapshot,
            target_snapshot,
            conceal_tracks,
            reveal_tracks,
            conceal_direction: direction,
            started_at,
            duration_ms: duration_ms.max(1),
            base_text,
            target_text,
            base_to_target_map,
        }
    }

    /// 连续吐字并入同一个前沿（评论 9 阻塞 3：按 track 自己的身份继承）。
    ///
    /// 每条旧 track：
    /// 1. 用 `prev_target_to_new` 映射它自己的 range；
    /// 2. 在**最新 target snapshot** 上重建它自己的 path；
    /// 3. `travelled` 跟着这条 track 本身走，不经过排序下标。
    ///
    /// 本次新增的 patch 开一条 `travelled = 0` 的新 track。相邻但来源不同的 track
    /// 不合并 —— 静态 clip 层渲染时本来就会合并相邻矩形，没必要为了减少 state
    /// 数量把动画 owner 也合掉。**动画状态按编辑身份保存；渲染阶段再合并几何。**
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn extend_insert(
        &mut self,
        target_snapshot: EditorLayoutSnapshot,
        target_text: String,
        inserted_ranges: Vec<(usize, usize)>,
        prev_target_to_new: &OffsetMap,
        base_to_current: &OffsetMap,
        now: Instant,
    ) {
        let eased = ease_out_cubic(self.sample(now).progress);
        let mut next: Vec<RevealTrack> = Vec::with_capacity(self.reveal_tracks.len() + 1);
        for track in &self.reveal_tracks {
            // Issue #826 评论 11 阻塞 2：映射失败不再静默 `continue`。
            // 静默丢 track 会让那些字「既没有新的 Reveal、又被 canonical 立刻画出」，
            // 直接闪没。`can_extend_identity` 已在进 extend 前做完同样的 preflight，
            // 所以这里失败是 invariant violation。
            let range = prev_target_to_new
                .map_old_range_to_new(track.range.0, track.range.1)
                .unwrap_or_else(|| {
                    identity_breakdown_single("extend_insert/extend_replace", track.range)
                });
            let path = FrontierPath::build(&target_snapshot, range, PathDirection::Forward);
            let next_travelled = travelled(track.travelled, path.total_length, eased);
            next.push(RevealTrack {
                range,
                path,
                travelled: next_travelled,
            });
        }
        for range in normalize_track_ranges(inserted_ranges) {
            if next.iter().any(|track| overlaps(range, track.range)) {
                continue;
            }
            next.push(RevealTrack {
                range,
                path: FrontierPath::build(&target_snapshot, range, PathDirection::Forward),
                travelled: 0.0,
            });
        }
        self.base_to_target_map = base_to_current.compose(prev_target_to_new);
        self.target_snapshot = target_snapshot;
        self.target_text = target_text;
        self.reveal_tracks = next;
        self.started_at = now;
    }

    /// 连续吞字并入同一个前沿（评论 9 阻塞 3）。
    ///
    /// 旧 track 本来就在 burst base 坐标系里，**range 与 path 都不需要重建**，
    /// 只需要让 `travelled` 推进一帧。本次传入的 `deleted_ranges` 属于
    /// 「这一次编辑前」的文本，先用 `base_to_current` 映回 base 坐标再开新 track。
    pub(crate) fn extend_delete(
        &mut self,
        target_snapshot: EditorLayoutSnapshot,
        target_text: String,
        deleted_ranges: Vec<(usize, usize)>,
        // Issue #826 comment 14: 本笔删除前用户正在看的 current old layout。
        // 视觉事实与贴图来源都取它，identity 才映回 burst base。
        current_snapshot: &EditorLayoutSnapshot,
        base_to_current: &OffsetMap,
        prev_target_to_new: &OffsetMap,
        // Issue #826 comment 13: Reflow -> Conceal current-frame geometry.
        // Empty slice means a plain Delete (canonical geometry).
        reflow_current: &[ReflowCurrentGeometry],
        direction: ConcealDirection,
        now: Instant,
    ) {
        let eased = ease_out_cubic(self.sample(now).progress);
        let mut next: Vec<ConcealTrack> = Vec::with_capacity(self.conceal_tracks.len() + 1);
        for track in &self.conceal_tracks {
            next.push(ConcealTrack {
                range: track.range,
                path: track.path.clone(),
                travelled: travelled(track.travelled, track.path.total_length, eased),
                glyphs: track.glyphs.clone(),
            });
        }
        // `can_extend_identity` 已经做完同样的 preflight；这里失败说明
        // coordinator 绕过了 preflight 直接 extend，是 invariant violation。
        // 保留 (current, base) 配对：几何要在当前坐标里配，range 要落在 base 坐标。
        let incoming: Vec<((usize, usize), (usize, usize))> =
            map_ranges_backward(&deleted_ranges, base_to_current).unwrap_or_else(|| {
                identity_breakdown("extend_delete", &deleted_ranges)
                    .into_iter()
                    .map(|range| (range, range))
                    .collect()
            });
        for (current_range, base_range) in incoming {
            if next.iter().any(|track| overlaps(base_range, track.range)) {
                continue;
            }
            // 评论 14：视觉事实取**本笔删除前用户正在看的 current old layout**
            // （request.base_snapshot），identity 才映回 burst base。
            next.push(build_conceal_track(
                current_snapshot,
                current_range,
                base_range,
                reflow_current,
                direction,
            ));
        }
        self.base_to_target_map = base_to_current.compose(prev_target_to_new);
        self.target_snapshot = target_snapshot;
        self.target_text = target_text;
        self.conceal_direction = direction;
        self.conceal_tracks = next;
        self.started_at = now;
    }

    /// 连续替换并入同一个前沿：old 侧与 new 侧**都**按 track 身份累计
    /// （评论 8 阻塞 2 + 评论 9 阻塞 3）。
    ///
    /// 同一帧只 sample 一次，两侧共用同一份 eased 进度，然后只重置一次 `started_at`。
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn extend_replace(
        &mut self,
        target_snapshot: EditorLayoutSnapshot,
        target_text: String,
        deleted_ranges: Vec<(usize, usize)>,
        inserted_ranges: Vec<(usize, usize)>,
        // Issue #826 comment 14: 本笔删除前用户正在看的 current old layout。
        // 视觉事实与贴图来源都取它，identity 才映回 burst base。
        current_snapshot: &EditorLayoutSnapshot,
        base_to_current: &OffsetMap,
        prev_target_to_new: &OffsetMap,
        // Issue #826 comment 13: Reflow -> Conceal current-frame geometry.
        // Empty slice means a plain Delete (canonical geometry).
        reflow_current: &[ReflowCurrentGeometry],
        direction: ConcealDirection,
        now: Instant,
    ) {
        let eased = ease_out_cubic(self.sample(now).progress);

        // old 侧：identity 与 path 不变（burst base 坐标），只推进 travelled。
        let mut conceal: Vec<ConcealTrack> = self
            .conceal_tracks
            .iter()
            .map(|track| ConcealTrack {
                range: track.range,
                path: track.path.clone(),
                travelled: travelled(track.travelled, track.path.total_length, eased),
                glyphs: track.glyphs.clone(),
            })
            .collect();
        // 同 extend_delete：`can_extend_identity` 已 preflight，这里失败是 invariant violation。
        let incoming_old: Vec<((usize, usize), (usize, usize))> =
            map_ranges_backward(&deleted_ranges, base_to_current).unwrap_or_else(|| {
                identity_breakdown("extend_replace", &deleted_ranges)
                    .into_iter()
                    .map(|range| (range, range))
                    .collect()
            });
        for (current_range, base_range) in incoming_old {
            if conceal
                .iter()
                .any(|track| overlaps(base_range, track.range))
            {
                continue;
            }
            conceal.push(build_conceal_track(
                current_snapshot,
                current_range,
                base_range,
                reflow_current,
                direction,
            ));
        }

        // new 侧：每条旧 track 映射自己的 range 到最新 target，再重建自己的 path。
        let mut reveal: Vec<RevealTrack> = Vec::with_capacity(self.reveal_tracks.len() + 1);
        for track in &self.reveal_tracks {
            // Issue #826 评论 11 阻塞 2：映射失败不再静默 `continue`。
            // 静默丢 track 会让那些字「既没有新的 Reveal、又被 canonical 立刻画出」，
            // 直接闪没。`can_extend_identity` 已在进 extend 前做完同样的 preflight，
            // 所以这里失败是 invariant violation。
            let range = prev_target_to_new
                .map_old_range_to_new(track.range.0, track.range.1)
                .unwrap_or_else(|| {
                    identity_breakdown_single("extend_insert/extend_replace", track.range)
                });
            let path = FrontierPath::build(&target_snapshot, range, PathDirection::Forward);
            let next_travelled = travelled(track.travelled, path.total_length, eased);
            reveal.push(RevealTrack {
                range,
                path,
                travelled: next_travelled,
            });
        }
        for range in normalize_track_ranges(inserted_ranges) {
            if reveal.iter().any(|track| overlaps(range, track.range)) {
                continue;
            }
            reveal.push(RevealTrack {
                range,
                path: FrontierPath::build(&target_snapshot, range, PathDirection::Forward),
                travelled: 0.0,
            });
        }

        self.base_to_target_map = base_to_current.compose(prev_target_to_new);
        self.target_snapshot = target_snapshot;
        self.target_text = target_text;
        self.conceal_direction = direction;
        self.conceal_tracks = conceal;
        self.reveal_tracks = reveal;
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
        self.sample(now).progress >= 1.0
    }

    /// 本帧吐字遮罩的裁剪矩形（只覆盖 inserted cluster）。
    ///
    /// Issue #826 评论 6 阻塞 3：**只裁 inserted cluster**，不能用整行内容右边界。
    /// `A|B` 中间插入 X 得 `AX|B`：A / X / B 三个 cluster 里只有 X 是 inserted，
    /// 若裁到整行右边界，B 会被 FrontierMask 一起挖掉——但 B 属于 Reflow 层职责。
    ///
    /// Issue #826 评论 7/8：边界来自**视觉路径**而不是一个二维 CursorRect。
    pub(crate) fn hidden_new_text_rects(&self, sample: &EditFrontierSample) -> Vec<FrontierRect> {
        if self.reveal_tracks.is_empty() || !sample.masks_new_text() {
            return Vec::new();
        }
        let eased = ease_out_cubic(sample.progress);
        let mut rects = Vec::new();
        for track in &self.reveal_tracks {
            let (range, path) = (track.range, &track.path);
            let distance = travelled(track.travelled, path.total_length, eased);
            let bounds = path.reveal_bounds(distance);
            for line in self.target_snapshot.lines_in_byte_range(range.0, range.1) {
                let Some(seg_index) = path.segment_index_for_line(line.id) else {
                    // 这一行没有可见 glyph（换行符），不产生 FrontierMask。
                    continue;
                };
                let Some(&(boundary, _right)) = bounds.get(seg_index) else {
                    continue;
                };
                for cluster in line.clusters_in_byte_range(range.0, range.1) {
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

    /// 本帧旧正文 overlay 要保留的矩形（Delete / Replace 用）。
    ///
    /// Issue #826 评论 14：直接按 `track.path` 自己算。
    ///
    /// 之前这里拿 `base_snapshot` 的 line id 去 `path.segment_index_for_line`
    /// 反查，但 Reflow handoff 建的 path 的 `line_id` 是占位值，真实 line id
    /// 几乎不可能匹配上 —— 于是每次都落进「整行保留」分支：progress 0~0.9 一直
    /// 完整显示，progress=1 突然整字消失，根本没有逐步吞。
    ///
    /// 现在所有吞字 track 都完整拥有自己的 glyph 几何（见 `build_conceal_track`），
    /// 路径就是从这些 glyph 生成的，直接按 segment 算边界即可，不再分
    /// 「canonical track / handed-off track」两套坐标系。
    pub(crate) fn old_overlay_rects(&self, sample: &EditFrontierSample) -> Vec<FrontierRect> {
        if self.conceal_tracks.is_empty() || !sample.needs_old_overlay() {
            return Vec::new();
        }
        let eased = ease_out_cubic(sample.progress);
        let mut rects = Vec::new();
        for track in &self.conceal_tracks {
            let path = &track.path;
            let distance = travelled(track.travelled, path.total_length, eased);
            let bounds = path.conceal_bounds(distance);
            for (segment, &(left, right)) in path.segments.iter().zip(bounds.iter()) {
                let rect = FrontierRect {
                    x: left.min(right),
                    y: segment.y,
                    w: (right - left).abs(),
                    h: segment.h,
                };
                if !rect.is_degenerate() {
                    rects.push(rect);
                }
            }
        }
        rects
    }

    /// 本帧旧正文 overlay 要画的 cluster（含 source / dest 矩形）。
    ///
    /// Issue #826 评论 14：全部来自 `track.glyphs` —— 每条吞字 track 都完整拥有
    /// 自己创建瞬间的旧 glyph 几何（Reflow 中的部分 dest 是采样屏幕位置）。
    /// 不再回 `base_snapshot` 反查：那样 handoff 的 dest 会被 canonical 覆盖，
    /// 出现「clip 前沿从 55 算、字画在 60」的错位。
    pub(crate) fn old_overlay_glyphs(&self, sample: &EditFrontierSample) -> Vec<FrontierGlyph> {
        if self.conceal_tracks.is_empty() || !sample.needs_old_overlay() {
            return Vec::new();
        }
        let keep = self.old_overlay_rects(sample);
        let mut glyphs = Vec::new();
        for track in &self.conceal_tracks {
            for geometry in &track.glyphs {
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

/// Issue #826 评论 8 阻塞 3：changed range 集合的归一化。
///
/// **只合并 overlap / adjacent，绝不跨 gap。**
/// `A=[10,12]` 与 `B=[100,102]` 必须保持两条——粗暴 union 成 `[10,102]`
/// 会把中间 88 bytes 的正常正文也当成改动过的字，吐字遮罩会把它们一起裁掉。
fn normalize_ranges(ranges: Vec<(usize, usize)>) -> Vec<(usize, usize)> {
    let mut kept: Vec<(usize, usize)> = ranges
        .into_iter()
        .filter(|(start, end)| end > start)
        .collect();
    if kept.len() <= 1 {
        return kept;
    }
    kept.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    let mut merged: Vec<(usize, usize)> = Vec::with_capacity(kept.len());
    for (start, end) in kept {
        match merged.last_mut() {
            // 相邻（end == start）或重叠才合并。
            Some(last) if start <= last.1 => {
                last.1 = last.1.max(end);
            }
            _ => merged.push((start, end)),
        }
    }
    merged
}

/// Issue #826 评论 10 阻塞 3：track 层专用的 range 归一化。
///
/// 与 `normalize_ranges` 的区别：**只合并真正 overlap，绝不合并 adjacency**
/// （判据是 `start < last.1` 而不是 `start <= last.1`）。
///
/// 评论 9 已定规则「相邻但来源不同的 track 不合并；动画状态按编辑身份保存，
/// 渲染阶段再合并几何」。`normalize_ranges` 仍会在创建 track 之前把
/// `[0,1]` + `[1,2]` 合成 `[0,2]`，owner 在 track 诞生前就丢了。
///
/// 具体危害：Undo 一个 delete-surrounding 时会一次恢复光标两侧的相邻文字，
/// 两条 final-new patch `[0,1]` / `[1,2]` 本该是两条 RevealTrack；合成成
/// `[0,2]` 后，动画未结束立刻在 byte 1 继续输入时，
/// `prev_target_to_new.map_old_range_to_new(0, 2)` 跨过本次插入点返回 `None`，
/// 整条旧 track 被丢弃，上一轮还没吐完的恢复文字瞬间回 canonical。
///
/// 静态裁剪层已经会做几何 interval merge，动画 state 不需要再合一次。
fn normalize_track_ranges(ranges: Vec<(usize, usize)>) -> Vec<(usize, usize)> {
    let mut kept: Vec<(usize, usize)> = ranges
        .into_iter()
        .filter(|&(start, end)| end > start)
        .collect();
    kept.sort_unstable();
    let mut merged: Vec<(usize, usize)> = Vec::with_capacity(kept.len());
    for (start, end) in kept {
        match merged.last_mut() {
            // 只在真正 overlap 时合并；相邻（start == last.1）保持两个 owner。
            Some(last) if start < last.1 => last.1 = last.1.max(end),
            _ => merged.push((start, end)),
        }
    }
    merged
}

/// Issue #826 评论 9 阻塞 3：两个范围是否**真正重叠**（半开区间）。
///
/// 只用这个判据吸收新 patch —— 相邻但不重叠的 track 不合并。静态 clip 层
/// 渲染时本来就会合并相邻矩形，没必要为了减少 state 数量把动画 owner 也合掉。
fn overlaps(a: (usize, usize), b: (usize, usize)) -> bool {
    a.0 < b.1 && b.0 < a.1
}

/// 把一批 range 映射回 burst base 坐标系（`new_to_old` 的方向）。
///
/// Issue #826 评论 11 阻塞 2：**不再用 `filter_map` 静默吞掉映射失败的
/// changed range**。映射失败意味着「本次要删的字不是 burst base 里的同一逻辑
/// 文字」，把它丢掉会让那些字既没有 ConcealTrack、又被 Reflow 当 changed 排除，
/// 视觉上直接从 canonical 消失。
///
/// 这里返回 `None`，调用方**必须**把它当作「不能 extend 当前 burst」处理
/// （`can_extend_identity` 已经在进 extend 之前做完同样的 preflight，
/// 所以走到这里失败属于 invariant violation）。
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

/// Issue #826 评论 11 阻塞 2：动画身份断裂的**显式兜底**。
///
/// 走到这里说明 coordinator 绕过了 `can_extend_identity` 的 preflight，
/// 直接调用了 extend。按 issue 的规则，正确行为是「当前 burst 到此结束、
/// 另开一轮」，而不是把映射失败的字静默丢掉。这里返回该 range 原样、
/// 让调用方继续开 track，同时打一条正式诊断事件便于定位 —— 丢 track 和
/// 保留 track 但不精确相比，前者会让字直接闪没。
fn identity_breakdown_single(site: &str, range: (usize, usize)) -> (usize, usize) {
    identity_breakdown(site, std::slice::from_ref(&range))
        .first()
        .copied()
        .unwrap_or(range)
}

/// 记录身份断裂并原样返回 range 列表（见 `identity_breakdown_single` 的说明）。
fn identity_breakdown(site: &str, ranges: &[(usize, usize)]) -> Vec<(usize, usize)> {
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
    ranges.to_vec()
}

/// 为每个新文字范围建一条吐字 track（吐字恒为正向视觉顺序）。
fn build_reveal_tracks(
    snapshot: &EditorLayoutSnapshot,
    ranges: &[(usize, usize)],
) -> Vec<RevealTrack> {
    ranges
        .iter()
        .map(|&range| RevealTrack {
            range,
            path: FrontierPath::build(snapshot, range, PathDirection::Forward),
            travelled: 0.0,
        })
        .collect()
}

/// Issue #826 评论 14：新建一条吞字 track —— **这条 track 完整拥有「这笔被删文字
/// 在 track 创建瞬间的全部可见 glyph」**。
///
/// 职责划分（评论 14 的核心结论）：
/// ```text
/// ConcealTrack
///   = burst-base 坐标的 identity range   （只负责编辑身份 / 连续编辑累计）
///   + 创建这一刻 current old snapshot 的完整 glyph 几何
///   + 由这些 glyph 几何生成的一条路径
///   + travelled
/// ```
/// Reflow handoff 只负责**覆盖其中部分 glyph 的 dest_rect**（换成上一帧的屏幕
/// 位置），而不是创造一种「特殊的 handed-off ConcealTrack」。
///
/// 步骤：
/// 1. 从 `current_snapshot` 的 `current_range` 枚举**全部可见 cluster**；
/// 2. 默认 `snapshot_id` / `source_rect` / `dest_rect` 都取 current snapshot；
/// 3. 若该 cluster 在 `reflow_current` 里有同一 range，**只覆盖 dest_rect**
///    为采样到的屏幕位置（贴图来源不变，仍是 current snapshot 那张行图）；
/// 4. `FrontierPath::from_glyph_geometry(&glyphs, direction)`；
/// 5. `track.range = base_range`。
///
/// 这样四种情况统一：
/// - 全部 Reflow   -> 全部从当前屏幕位置开始吞；
/// - 部分 Reflow   -> moving 的用 sampled dest，static 的用 current canonical；
/// - 完全没 Reflow -> 全部 current canonical；
/// - 不存在「有一个 handed_off 就把同 range 里没在 Reflow 的字丢掉」。
///
/// `current_range` 与 `base_range` 分属两个坐标系（当前 vs burst base），
/// 必须在调用方配好再传进来 —— 见 `map_ranges_backward` 返回的配对。
fn build_conceal_track(
    current_snapshot: &EditorLayoutSnapshot,
    current_range: (usize, usize),
    base_range: (usize, usize),
    reflow_current: &[ReflowCurrentGeometry],
    direction: ConcealDirection,
) -> ConcealTrack {
    let mut glyphs: Vec<ConcealGlyphGeometry> = Vec::new();
    for line in current_snapshot.lines_in_byte_range(current_range.0, current_range.1) {
        for cluster in line.clusters_in_byte_range(current_range.0, current_range.1) {
            let canonical = line.source_rect_to_document_rect(&cluster.source_rect);
            let glyph_range = (cluster.byte_start, cluster.byte_end);
            // 若这一段上一帧正在 Reflow，dest 用采样到的屏幕位置；
            // 贴图来源不变（仍是 current snapshot 的行纹理）。
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
    let path = FrontierPath::from_glyph_geometry(&glyphs, PathDirection::from(direction));
    ConcealTrack {
        range: base_range,
        path,
        travelled: 0.0,
        glyphs,
    }
}

/// Issue #826 评论 14：批量建 track。
///
/// begin 场景下 current_range == base_range（burst base 就是 request.base）；
/// 逐条 track 自己按来源 range 配对 Reflow 几何 —— 不能在 coordinator 先
/// filter 一次，否则 `deleted_ranges = [A], [B]` 时两条 track 会各自拿到 A+B。
fn build_conceal_tracks_for_ranges(
    current_snapshot: &EditorLayoutSnapshot,
    ranges: &[(usize, usize)],
    direction: ConcealDirection,
    reflow_current: &[ReflowCurrentGeometry],
) -> Vec<ConcealTrack> {
    ranges
        .iter()
        .map(|&range| {
            let current_for_track: Vec<ReflowCurrentGeometry> = reflow_current
                .iter()
                .filter(|item| overlaps(item.current_range, range))
                .cloned()
                .collect();
            build_conceal_track(
                current_snapshot,
                range,
                range,
                &current_for_track,
                direction,
            )
        })
        .collect()
}

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

/// 一行内容在文档坐标里的左右边界（物理像素转文档坐标）。
fn line_content_x_extent(line: &PreparedLineSnapshot) -> (f64, f64) {
    let mut left = f64::INFINITY;
    let mut right = f64::NEG_INFINITY;
    for cluster in &line.clusters {
        let rect = line.source_rect_to_document_rect(&cluster.source_rect);
        left = left.min(rect.x);
        right = right.max(rect.x + rect.w);
    }
    if left > right {
        (line.visual_x, line.visual_x)
    } else {
        (left, right)
    }
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
