//! Issue #826 评论 24/25：不可拆 shaping cluster 的当前态视觉交接。
//!
//! ## 底层规则
//!
//! **Core 的 `inserted_ranges` / `deleted_ranges` / `OffsetMap` 可以按字符切；
//! Qt 的视觉 owner 绝不能切开一个 shaping cluster。**
//!
//! `engine.rs` 里 cluster 边界来自 `QGlyphRun::stringIndexes()`，一个 cluster 可能
//! 覆盖多个字符（fi 连字、e + 组合音标、emoji ZWJ）。Core 只知道逻辑字符身份，
//! 不知道这些字符被 shaping 合成了哪一块视觉资源。
//!
//! 之前 `layout_snapshot::clusters_in_byte_range()` 只是 overlap 查询，
//! `EditFrontier` 拿到结果后却当成「这块 cluster 就属于这个逻辑 range」，于是：
//!
//! ```text
//! 第一笔 af   f 的 cluster = 1..2
//! 第二笔 afi  最新 shaping 把 fi 合成一块 cluster = 1..3
//!             Core 逻辑上：旧 f 仍映射 1..2，新 i 是 2..3
//! ```
//!
//! - 评论 23 的 `mapped_previous` 层用 range `1..2` 建 path，`overlap` 把整块
//!   `fi` 拉进来 —— 「只含旧 owner」的 path 视觉上已经含了新 `i`，身份判据再次失效；
//! - `find_cluster_geometry(target, 1..2)` 同样返回整块 `fi`，于是
//!   `carried.range = 1..2` 却拿整块 `fi` 的纹理，`subtract_ranges` 又把
//!   `2..3`  留给 scalar Reveal —— **同一块视觉 cluster 被两个 owner 同时控制**。
//!
//! ## 本层的职责
//!
//! 从「逻辑 changed range + 前后两份 layout」派生「视觉 affected cluster」层：
//!
//! - 改动**完整覆盖**的 cluster：照旧交给 `EditFrontier` 的 Reveal / Conceal
//!   （绝大多数中文单字、独立 glyph 都属于这一类，观感完全不变）；
//! - 不可拆的 cluster（逻辑改动只覆盖它一部分、边界变了、或 `shaping_identity`
//!   变了）：整块退出 EditFrontier 与 Reflow，作为
//!   [`ShapingTransitionGroup`] 进入本层。
//!
//! ## 评论 25 补上的两条硬约束
//!
//! ### 1. 必须从当前屏幕接手，不能从 canonical 重建
//!
//! `af` → `afi` 时，上一帧屏幕上的 `f` 只露了 8.75px。若本层直接拿
//! `base_snapshot` 里的**完整** `f` 当 old 侧起步，第二笔同一帧就会
//! 「8.75px 突然变成 10px，再开始 f → fi 淡变」。
//!
//! 所以 coordinator 在任何状态变更前先采一份 [`CurrentVisualCluster`]（这一帧
//! 屏幕上真实存在的视觉原子：Reveal scalar 的已露宽度、carry 的可见前缀、
//! Reflow 的当前位置、Conceal 仍可见的旧 glyph、本层自己正在淡入淡出的两侧），
//! `build_or_retarget()` 的第一帧必须从这份事实起步。
//!
//! ### 2. 必须能跨下一笔编辑 retarget
//!
//! 生命周期与 `ReflowState` 完全对齐：
//!
//! - 下一笔不碰它：映射到最新 revision 后继续；
//! - 下一笔影响它：先 sample 当前帧，再 retarget 到最新 shaping；
//! - 只有 `is_finished()` 或显式 `finish_edit_frontier_to_canonical()` 才消失。
//!
//! 这里**不排历史队列、不 per-key 累积、没有第二个动画时钟** —— 只保留
//! 「当前这一组 source atoms + 最新 target atoms」。
//!
//! 架构归属：`EditFrontier` 管 changed logical fact + mask timing；`Reflow` 管
//! same-shaping unchanged move；本层只管当前不可拆 cluster 的 old/new 视觉交接。
//! 三者都只保存「当前屏幕事实」。
//!
//! ## 评论 26 补上的三条不变量
//!
//! ### 1. new 侧的终点必须是它自己的 canonical rect
//!
//! 旧实现 old / new 两侧共用一个 `toward`：old 侧朝新区域走（合理，它最后
//! opacity = 0），new 侧也被塞了 old 区域包围盒。于是 new 侧「越淡越不透明，
//! 越往 old 位置跑」，`is_finished()` 清层后静态 canonical 再跳回 `atom.rect`
//! —— 动画末尾肉眼可见地再跳一次。现在两侧的终点分开定义。
//!
//! ### 2. `CurrentVisualCluster.source_rect` 永远表示「这一帧实际可见的精确 slice」
//!
//! 吐字侧是整块 + visible_clip、吞字侧是已裁 slice、本层交接是 `sample()` 裁过的
//! 片段 —— 三种语义混在一起，接手方一旦再乘一次 `visible / rect.w` 就是二次裁剪
//! （10px cluster 剩 2.5px 时取原图 6.25% 的 UV 拉伸到 2.5px）。现在 producer
//! 一律交出 exact slice，`sample()` 两侧都**不再裁 source**。
//!
//! ### 3. group 是原子粒度，不是整组粒度
//!
//! 1:N 的 group 里第二笔只改其中一个 child 时，旧实现 `any absorbed => skip
//! whole group`，被接管的 child 之外，兄弟 new atom 与仍在淡出的 old atom 一起
//! 凭空消失。现在按 atom 拆成「本笔接管的」与「继续淡的」两份。
//!
//! ## 评论 27 补上的两条不变量
//!
//! ### 1. handoff 必须认视觉身份，不能只认逻辑身份
//!
//! 1:N 的 `old O 0..4 -> new A 0..1 + B 1..3` 进行中，old O 的
//! `primary_handoff_key()` 就是第一个 survivor `A_range`，new A 的
//! `logical_range` 也是 `A_range` —— 同一帧里两条 current_visual 的
//! **逻辑身份完全相同，视觉资源却完全不同**（两张行纹理、两种 revision、
//! 两个 opacity、两份 source slice、两个位置）。
//!
//! 下一笔改 A 时 old 侧若只按 `logical_range` 匹配，而 `current_visuals` 又是
//! old atoms 在前，就会从「更早的 O」接手，而不是从屏幕上那份「正在淡入的 A」
//! 接手。所以 [`CurrentVisualCluster`] / [`VisualClusterAtom`] 都多带一份
//! [`CurrentVisualCluster::visual_cluster_range`]，与 `snapshot_id` 合成视觉身份。
//!
//! 逻辑身份回答「是不是同一段字」，视觉身份回答「这一笔的 base old cluster
//! 该接哪一份当前像素」，两层不能合成一个 `(start, end)`。
//!
//! ### 2. 区间查询必须是 O(log N + k)
//!
//! [`ClusterIndex::nodes_overlapping_interval`] 旧实现每次都从 `sorted` 的开头
//! 重扫到上界 —— N 个 old 对 N 个 new 时总量 `1+2+…+N = O(N²)`，等于把
//! 「按正文 byte 数量扫」换成了「按 snapshot cluster 数平方扫」。
//! 现在 `sorted` 同时存 `byte_start` / `byte_end`（cluster 互不重叠，两者都升序），
//! 两端各二分一次，只遍历真正相交的那批。
//!
//! ## 评论 28 补上的两条不变量
//!
//! ### 1. 消费 `CurrentVisualCluster` 时**只认视觉身份**
//!
//! 评论 27 还留着「视觉身份失败就退回逻辑身份」的兜底
//! （`take_visual_handoff(...).or_else(|| take_handoff_for_keys / take_exact_handoff)`）。
//! 既然同一 logical range 同帧可以挂两份完全不同的资源，逻辑 range 就不再有
//! 资格决定「拿哪份像素」。现在逻辑身份只用于 OffsetMap 身份映射、component
//! 连通关系与 `handoff_keys`；真正消费像素时只有 [`take_visual_handoff`] 一个
//! 入口，`take_handoff_for_keys` / `take_exact_handoff` / `containment_overlap`
//! 已整体删除。抓不到就是「这一帧没有那份像素」：
//! - 新 component old 侧 → base canonical；
//! - previous old/new atom → 就此结束，不抢别人的；
//! - new side → 自己的新 canonical resource、opacity 0。
//!
//! ### 2. 一份视觉像素一帧只能有一个 owner
//!
//! [`build_group`] 曾经在收下 component 自己的 old node 之后，再扫一遍
//! `current_visuals`，把 `logical_range` 与 `claimed` containment 重叠的**全部**
//! leftover 都塞进 `old_atoms`。1:N 场景里历史 old O 的 `logical_range` 正好是
//! `A_range`，于是第二笔只改 A 时，新 component 把 old O 也吸进来；而后面
//! [`split_previous_group_by_absorbed`] 又要求 old O 留在 previous rest group
//! 继续和 B 一起淡 —— 同一份像素同帧属于两个 group，render 会画两遍。
//!
//! 正确 ownership：新 group `old = [当前 A] / new = [C]`，previous rest
//! `old = [历史 O] / new = [未修改的 B]`，old O 只出现一次。
//! 历史 old visual 是 previous shaping transition 的视觉债，不属于这一笔
//! `base_snapshot` 的 A cluster，必须完全交给 [`split_previous_group_by_absorbed`] /
//! [`retain_visible_old_atoms`] 决定继续淡出还是结束。
//!
//! ## 评论 29 补上的不变量
//!
//! ### atom 是渲染资源，claims 是 revision ownership，两者不能互推
//!
//! [`VisualClusterAtom`] 同时带着 `cluster`（它真正的 source snapshot 上的完整
//! cluster range）、`snapshot_id`、`visual_cluster_range`、`source_rect`，这些回答
//! 「这份像素从哪里来」。第一笔时 `cluster` 与当前 `base_snapshot` 恰好同坐标，
//! 于是 [`ShapingTransitionState::owned_old_clusters`] 顺手从渲染 atoms 收集
//! —— 连续 retarget 之后就分叉了。
//!
//! 三笔反例：`O 0..4 -> A 0..1 + B 1..3`，第二笔只改 A 得到 `C 0..2 + B 2..4`，
//! 历史 old O 仍在 previous rest group 里淡。第三笔做一次与 shaping 无关的普通
//! Insert 时，当前 `base_snapshot` 归 shaping 的应该是 `0..2 / 2..4`；而从
//! `old_atoms` 反推会得到 `0..1`（更早 old A）与 `0..4`（更早 old O）——两笔
//! 以前的 source 坐标。
//!
//! stale `0..4` 会顺着两条链污染运行时：`begin_or_extend_reflow()` 把它并进
//! `excluded_old`，当前 base 里同坐标的新字被禁止 Reflow，直接瞬移；同一份
//! `shaping_old_owned` 还要传给 `extend_delete` / `begin_delete` / `extend_replace`
//! / `begin_replace`，普通删除落进 stale range 就被误判成「已归 ShapingTransition」，
//! 该吞的字不进 Conceal。
//!
//! 所以 state 显式存两组**当前 revision** 的 owner ranges：
//! [`ShapingTransitionState::old_owned_in_base`] 与
//! [`ShapingTransitionState::new_owned_in_target`]，在 [`build_or_retarget`] 里
//! 从正确坐标来源填：本笔新 component 的 claims 取自两个 `ClusterIndex`；
//! previous rest group 的 base claim 取自 **kept 的 previous new atom**
//! （`previous.target == request.base`），new claim 取自 retarget 之后的
//! new atom。历史 old atom 只是视觉淡出资源，**不产生任何 current-base claim**。
//!
//! ## 评论 30 补上的不变量
//!
//! ### 「pure delete 交给 EditFrontier」只对 canonical owner 成立
//!
//! [`build_or_retarget`] 原来对 `old_nodes.nonempty && new_nodes.empty` 的
//! component 无条件 `continue`，理由是「整块消失，EditFrontier 自己就够」。
//! 对 canonical 字成立；但当这块 old cluster 上一帧正由本层的 **new side** 画时
//! 不成立。`f -> fi` 跑到 40ms 时屏幕是「历史 old `f` 淡到 0.42 + current new
//! `fi` 淡到 0.58」，此时整块删掉 `fi`：
//!
//! - 本笔没有新 Shaping component，pure delete 被跳过；
//! - previous 的 `fi` 被 `retarget_new_atoms()` 判为整块已删，不再进新 groups；
//! - 历史 old `f` 仍在 rest group 里；
//! - 于是 `old_owned_in_base = [1..3]` 却没有任何 new atom 在画它，
//!   Conceal 又因这份 claim 把 `1..3` 排除、canonical target 里也没有它。
//!
//! **`fi` 一帧消失** —— 直接违反 #826「owner 换手的第一帧必须等于上一帧真实屏幕」。
//!
//! 也不能简单不 claim：那会让 Conceal 从 `base_snapshot` 的**完整不透明** `fi`
//! 起步，屏幕从 0.58 跳到 1.0 再开始吞，同样是跳。
//!
//! 所以判据落在「old cluster 的视觉键同时满足『出现在
//! `previous.groups[*].new_atoms` 的视觉键集合里』与『`current_visuals` 里有
//! exact visual』」—— 这两条回答的都是「上一帧这块字归谁画」，不引入任何
//! origin 枚举。
//!
//! ## 评论 31：pure delete 的 owner 下一站是 Conceal，不是 shaping 淡出
//!
//! 评论 30 让本层自己从当前帧把被删的 `fi` 淡掉，虽然补上了「一帧消失」，
//! 却让吞字吐字长出**两条并行时间轴**：遮罩前沿按方向 clip 一条，shaping 的
//! `old -> opacity 0` 又一条。同一块字同时被两条曲线控制，方向、进度、收尾
//! 都对不齐，而 #826 要求的是**单一**前沿。
//!
//! 所以本层的职责收窄为**只交棒、不自留**：
//!
//! - [`absorb_previous_new_side`] 按上面那套判据把被删的 old cluster 记进
//!   `absorbed`，让 previous rest 不再把对应的 previous new atom 当 kept child
//!   —— 否则那份 visual 会既归 rest 又归 Conceal；
//! - 本层**不 claim、不建 group、不消费 current visual**。`old_owned_in_base`
//!   里不再有被删的 `fi`，Conceal 因此必须亲自画它；
//! - coordinator 在调 [`build_or_retarget`] 之前用**同一套判据**采
//!   `conceal_handoffs`（上一帧真实的 snapshot / source / rect / opacity），
//!   frontier 拿它从真实起点开始按方向缩 keep rect。`opacity` 是起点常量，
//!   时间只由遮罩前沿的 clip 表达。
//!
//! 历史 old `f` residual 与其它没被碰到的 previous new child 仍留在 previous
//! rest 组里继续淡 —— 它们没被删除，下一帧照样由本层交棒。

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use writer_core::editor::{OffsetMap, OffsetMapEntry};

use crate::sujian_editor_item::animation::edit_frontier::{ease_out_cubic, ConcealSourceLine};
use crate::sujian_editor_item::layout_snapshot::{
    EditorLayoutSnapshot, LineClusterSnapshot, LineSnapshotId, PreparedLineSnapshot, SourceRect,
};

const EPS: f64 = 1e-6;

/// Issue #826 评论 25：**这一帧**屏幕上真实存在的一个视觉原子。
///
/// 它不是历史动画单元 —— 没有 `started_at`、没有 remaining duration、没有
/// historical stage、没有第二个动画对象。它就是「owner 换手时，旧 owner 交出去
/// 的那一帧像素事实」，与 `ReflowCurrentGeometry` 同一性质。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CurrentVisualCluster {
    /// 字符身份，坐标在**当前 target 坐标系**里。
    ///
    /// 下一笔编辑的 `base_snapshot` 就是这份 target，所以这个 range 可以直接
    /// 拿去和下一笔派生出来的 atom 对齐。
    ///
    /// **它不能单独当 handoff 身份用** —— 见 [`CurrentVisualCluster::visual_cluster_range`]。
    pub logical_range: (usize, usize),
    /// 视觉身份：这些像素取自哪张行纹理、那张纹理里的哪一块 cluster。
    ///
    /// Issue #826 评论 27 阻塞 1：`logical_range` 只回答「是不是同一段字」，
    /// 回答不了「这一帧的像素是哪一份资源画的」。
    ///
    /// 1:N 的 `old O 0..4 -> new A 0..1 + B 1..3` 进行中，old O 的
    /// `handoff_keys` 是 `[A_range, B_range]`，`visual_from()` 取 primary 就是
    /// `A_range` —— 于是同一帧同时存在两条 `logical_range = A_range`：
    ///
    /// - 历史 old O 的像素（资源是 `first_old` 那张行纹理的 `0..4`）；
    /// - 当前 new A 的像素（资源是 `first_new` 那张行纹理的 `0..1`）。
    ///
    /// 下一笔改 A 时，`build_group()` 的 old 侧必须先按
    /// `(base_snapshot 的 line.id, base cluster)` 精确匹配到**后者**；历史
    /// old-side visual 只能作为 fallback。两层身份合成一个 `(start,end)`
    /// 就会抓错纹理 / 错 opacity / 错 source slice / 错位置。
    pub visual_cluster_range: (usize, usize),
    /// 贴图来源行纹理 —— 与 [`visual_cluster_range`] 一起构成视觉身份。
    pub snapshot_id: LineSnapshotId,
    /// 那张行纹理里的源矩形 —— **这一帧实际可见的精确 slice**。
    ///
    /// Issue #826 评论 26 阻塞 2：语义必须唯一。接手方（[`VisualClusterAtom`]）
    /// 拿到它就直接用，绝不能再按 `visible / rect.w` 裁一次，否则 10px 的 cluster
    /// 只剩 2.5px 时会取到原图 6.25% 的 UV 再拉伸。
    /// 用 [`visible_source_slice`] 从整块 source + 整字宽 + 已露宽度算出来。
    pub source_rect: SourceRect,
    /// 这一帧它在屏幕上的矩形（文档坐标）。
    pub dest_rect: SourceRect,
    /// 这一帧的不透明度。
    pub opacity: f64,
    /// 这一帧真正可见的宽度（`0..= dest_rect.w`）。
    ///
    /// 吐字只露了 8.75px 就必须是 8.75 —— 新 owner 第一帧若按整字宽起步，
    /// 屏幕会「先补满再淡变」。
    pub visible_clip: f64,
}

/// 从整块 cluster 的 source 与「这一帧可见宽度」算出精确 slice。
///
/// Issue #826 评论 26 阻塞 2：[`CurrentVisualCluster::source_rect`] 的唯一算法。
///
/// `full_dest_width` 是这块 cluster **整字**的文档宽度 —— carry 正在补间时也
/// 必须用 canonical 的 `to_rect.w`，不能用当前帧那个还在变的矩形宽，否则 UV
/// 比率会随 progress 漂移。`visible` 是这一帧已经露出的宽度。
pub(crate) fn visible_source_slice(
    full: &SourceRect,
    full_dest_width: f64,
    visible: f64,
) -> SourceRect {
    let ratio = (visible / full_dest_width.max(EPS)).clamp(0.0, 1.0);
    SourceRect {
        x: full.x,
        y: full.y,
        w: full.w * ratio,
        h: full.h,
    }
}

/// Issue #826 评论 25：一个不可拆视觉 cluster 的一侧原子。
///
/// 1:1 的一对 `old_cluster` / `new_cluster` 表达不了真实 shaping：
/// 一块 old `0..4` 删掉中间一位后可能变成两块 new（`0..1` + `1..3`），
/// 反向 N:1 也一样。所以本层用「一组 old 原子 ↔ 一组 new 原子」表达
/// **当前这一笔不可拆区域**，组内共用同一个 progress。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct VisualClusterAtom {
    /// 这个 cluster 的完整边界（它所属 snapshot 的坐标系）。
    pub cluster: (usize, usize),
    /// 交给**下一笔**做 handoff 对齐用的字符身份（坐标在当前 state 的 target 系）。
    ///
    /// Issue #826 评论 26 阻塞 3：1:N 的 old atom 可能有多段 survivor ——
    /// 旧 cluster `0..4` 删掉中间 `1..2` 后剩 `0..1` 与 `2..4` 两段，它们可能
    /// 分别 shaping 成两块 new cluster。只存第一段时，下一笔改后面那一段会让
    /// 这份 old atom 在 `current_visuals` 里找不到归属而凭空消失。
    pub handoff_keys: Vec<(usize, usize)>,
    /// 视觉身份：`snapshot_id` 那张行纹理里的哪一块 cluster 真正画出了这个原子。
    ///
    /// Issue #826 评论 27 阻塞 1：老侧接过 handoff 时，像素来自更早的
    /// revision，`cluster`（base 坐标系）却已经不是那份纹理里的 range 了。
    /// 两者必须分开存，[`visual_from`] 才能把「哪张纹理 + 哪块」原样交出去。
    pub visual_cluster_range: (usize, usize),
    /// 贴图来源行纹理。
    pub snapshot_id: LineSnapshotId,
    /// 那张行纹理里的源矩形（覆盖整个 cluster，绝不按 byte 比例裁）。
    pub source_rect: SourceRect,
    /// 目标矩形（文档坐标）。
    pub rect: SourceRect,
    /// 当前帧起点 —— 有 handoff 就是上一帧的真实矩形，没有就是 canonical。
    pub start_rect: SourceRect,
    /// 当前帧不透明度。old 侧从它淡出到 0，new 侧从它淡入到 1。
    pub start_opacity: f64,
    /// 当前帧可见宽度。old 侧保持不变（本来就在淡出），new 侧增长到 `rect.w`。
    pub start_visible_width: f64,
}

impl VisualClusterAtom {
    /// 这个原子是否拥有某段 target 坐标身份。
    pub(crate) fn claims(&self, range: (usize, usize)) -> bool {
        self.handoff_keys.iter().any(|&key| key == range)
    }

    /// 首选 handoff 身份（拿不到多段时用它）。
    pub(crate) fn primary_handoff_key(&self) -> (usize, usize) {
        self.handoff_keys.first().copied().unwrap_or(self.cluster)
    }
}

/// Issue #826 评论 25：一次不可拆 shaping 区域的当前态 old/new 视觉集合。
///
/// 共用一个 progress：`old_atoms` 整组淡出、`new_atoms` 整组淡入，
/// 每个原子保留自己的 snapshot / source / dest rect。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ShapingTransitionGroup {
    pub old_atoms: Vec<VisualClusterAtom>,
    pub new_atoms: Vec<VisualClusterAtom>,
}

/// 一个原子在某一帧、某一侧的画面。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ShapingTransitionSide {
    pub snapshot_id: LineSnapshotId,
    pub source_rect: SourceRect,
    pub rect: SourceRect,
    /// old 侧 `-> 0`，new 侧 `-> 1`。
    pub opacity: f64,
}

/// 一个 group 在某一帧的画面。某一侧所有原子都归零时不再画这一侧。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ShapingTransitionFrame {
    pub old: Vec<ShapingTransitionSide>,
    pub new: Vec<ShapingTransitionSide>,
}

/// Issue #826 评论 24/25：不可拆 cluster 的当前态交接层。
///
/// 与 `ReflowState` 同构：只有当前这一组 cluster、共用一个 `progress`，
/// 没有历史队列。
#[derive(Clone, Debug, Default)]
pub(crate) struct ShapingTransitionState {
    pub groups: Vec<ShapingTransitionGroup>,
    pub started_at: Option<Instant>,
    pub duration_ms: u64,
    /// 旧侧 cluster 真正引用的行图（画旧 cluster 必须有这张图）。
    pub old_sources: Vec<ConcealSourceLine>,
    /// Issue #826 评论 29：这一笔 **`request.base_snapshot` 坐标系**里归
    /// shaping 占用的 old cluster。
    ///
    /// 它和 `groups[*].old_atoms[*].cluster` **不是一回事**，两层不能混：
    ///
    /// - atom 的 `snapshot_id` / `visual_cluster_range` / `source_rect` 回答
    ///   「这份像素从哪里来」——历史 old atom 的 `cluster` 因此可能停在两笔以前
    ///   的 source 坐标（例如整块 `0..4`）；
    /// - 这里的 claim 回答「当前 revision 里哪些 old cluster 归 shaping」——
    ///   必须全部落在本笔 `base_snapshot` 上。
    ///
    /// 用前者反推后者会把陈旧坐标喂给 Reflow exclusion 和普通 Conceal
    /// exclusion：stale `0..4` 会连带把当前 base 里同坐标的新字一起排掉。
    pub old_owned_in_base: Vec<(usize, usize)>,
    /// Issue #826 评论 29：这一笔 **`request.target_snapshot` 坐标系**里归
    /// shaping 占用的 new cluster。
    pub new_owned_in_target: Vec<(usize, usize)>,
    /// 本 state 的 target 坐标系对应的正文纯文本。
    ///
    /// `previous.target_text() == request.base_text` 表示下一笔仍在同一条编辑
    /// revision 链上，这时才可以 retarget 而不是从 canonical 重建。
    pub target_text: String,
}

impl ShapingTransitionState {
    /// Issue #826 评论 25：从当前屏幕事实出发，建立或 retarget 这一笔的交接层。
    ///
    /// `previous` 非 `None` 表示上一份交接层与本笔在同一条 revision 链上
    /// （coordinator 用 `target_text()` 判定）。此时做两件事：
    ///
    /// 1. 本笔**新产生**的不可拆 component 从 `current_visuals` 起步；
    /// 2. 上一份里**没被接管**的 group 继续映射到最新 target 后接着淡。
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn build_or_retarget(
        previous: Option<&ShapingTransitionState>,
        current_visuals: &[CurrentVisualCluster],
        old_snapshot: &EditorLayoutSnapshot,
        new_snapshot: &EditorLayoutSnapshot,
        deleted_ranges: &[(usize, usize)],
        inserted_ranges: &[(usize, usize)],
        old_to_new: &OffsetMap,
        now: Instant,
        duration_ms: u64,
        target_text: String,
    ) -> Self {
        let old_side = ClusterIndex::build(old_snapshot);
        let new_side = ClusterIndex::build(new_snapshot);
        let (components, identity_pairs) = collect_components(&old_side, &new_side, old_to_new);

        let mut consumed: HashSet<usize> = HashSet::new();
        let mut groups: Vec<ShapingTransitionGroup> = Vec::new();
        // 本笔接管掉的旧坐标 cluster —— 上一份的 new 侧若落进这里就被吸收了。
        let mut absorbed: Vec<(usize, usize)> = Vec::new();
        // Issue #826 评论 29：**当前 revision** 的 ownership，与渲染 atoms 分开存。
        // 全部在构建这一刻从正确坐标来源填好，之后绝不再从 `groups` 反推。
        let mut old_owned_in_base: Vec<(usize, usize)> = Vec::new();
        let mut new_owned_in_target: Vec<(usize, usize)> = Vec::new();

        for component in &components {
            if component.old_nodes.is_empty() {
                // 纯插：整块出现，交给 EditFrontier 的 Reveal。
                continue;
            }
            if component.new_nodes.is_empty() {
                // ── Issue #826 评论 31 阻塞 ──────────────────────────────
                //
                // 评论 30 先确认了：如果这块 old cluster 上一帧还由
                // ShapingTransition 的 new side 在画，普通 Delete/Conceal 就接不住
                // —— 新 groups 里没有它，`old_owned_in_base` 却 claim 它，
                // Conceal 因此排除，canonical target 也没有它，一帧消失。
                //
                // 评论 31 推翻了「让 shaping 自己把它淡掉」的收尾：吞字吐字
                // 的唯一交棒对象是**单一遮罩前沿**，shaping 不该再养一条并行的
                // 时间轴。本层因此只做一件事 —— 把这些 old cluster 记进
                // `absorbed`，让 previous rest 不再把对应的 previous new atom
                // 当 kept child（否则那份 visual 会既归 rest 又归 Conceal）。
                // 真正的 owner 交接由 coordinator 的 `conceal_handoffs` 完成：
                // 它按**完全相同**的判据采下一份「上一帧真实」的
                // snapshot/source/rect/opacity，交给 Delete/Replace frontier。
                //
                // 不 claim、不建 group、不消费 current visual。
                for range in absorb_previous_new_side(&component.old_nodes, &old_side, previous) {
                    if !absorbed.contains(&range) {
                        absorbed.push(range);
                    }
                }
                continue;
            }
            if !component.needs_transition(
                &old_side,
                &new_side,
                &identity_pairs,
                deleted_ranges,
                inserted_ranges,
                old_to_new,
            ) {
                continue;
            }
            let group = build_group(
                component,
                &old_side,
                &new_side,
                old_to_new,
                deleted_ranges,
                current_visuals,
                &mut consumed,
            );
            let Some(group) = group else { continue };
            // 本笔新 component 的 claims 直接取自两个 `ClusterIndex`：
            // old 侧天然属于 `request.base_snapshot`，new 侧天然属于
            // `request.target_snapshot` —— 这是唯一不需要换算的坐标来源。
            for node in &component.old_nodes {
                let range = old_side.cluster_range(*node);
                if !absorbed.contains(&range) {
                    absorbed.push(range);
                }
                if !old_owned_in_base.contains(&range) {
                    old_owned_in_base.push(range);
                }
            }
            for node in &component.new_nodes {
                let range = new_side.cluster_range(*node);
                if !new_owned_in_target.contains(&range) {
                    new_owned_in_target.push(range);
                }
            }
            groups.push(group);
        }

        // 上一份里没被本笔接管的 group：映射到最新 target 后继续淡。
        //
        // Issue #826 评论 26 阻塞 3：不能 `any absorbed => skip whole group`。
        // 1:N 的 group 里第二笔只改其中一个 child 时，整组被跳过 —— 那个 child
        // 被新 group 接住，但没被碰的兄弟 atom 与仍在淡出的 old atom 一起消失。
        if let Some(previous) = previous {
            for group in &previous.groups {
                let Some((rest, old_claims_in_base)) = split_previous_group_by_absorbed(
                    group,
                    &absorbed,
                    new_snapshot,
                    old_to_new,
                    current_visuals,
                    &mut consumed,
                ) else {
                    continue;
                };
                // Issue #826 评论 29：previous rest group 的 base 侧 claim 来自
                // **previous 的 new atom**（`previous.target == request.base`），
                // 不是它的 old atom。后者是历史淡出资源，坐标停在更早 revision。
                for range in old_claims_in_base {
                    if !old_owned_in_base.contains(&range) {
                        old_owned_in_base.push(range);
                    }
                }
                for atom in &rest.new_atoms {
                    if !new_owned_in_target.contains(&atom.cluster) {
                        new_owned_in_target.push(atom.cluster);
                    }
                }
                groups.push(rest);
            }
        }

        let mut old_sources: Vec<ConcealSourceLine> = Vec::new();
        for group in &groups {
            for atom in &group.old_atoms {
                if old_sources
                    .iter()
                    .any(|source| source.snapshot_id == atom.snapshot_id)
                {
                    continue;
                }
                if let Some(line) = old_snapshot
                    .line_snapshots
                    .iter()
                    .find(|line| line.id == atom.snapshot_id)
                    .or_else(|| {
                        new_snapshot
                            .line_snapshots
                            .iter()
                            .find(|line| line.id == atom.snapshot_id)
                    })
                {
                    old_sources.push(ConcealSourceLine {
                        snapshot_id: line.id,
                        image: line.image.clone(),
                    });
                }
            }
        }
        old_sources.sort_by_key(|source| {
            (
                source.snapshot_id.layout_revision,
                source.snapshot_id.visual_line_ordinal,
            )
        });

        Self {
            groups,
            started_at: Some(now),
            duration_ms: duration_ms.max(1),
            old_sources,
            old_owned_in_base,
            new_owned_in_target,
            target_text,
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }

    /// Issue #826 评论 34：把整条 shaping 时间轴整体平移 `delta`。
    ///
    /// 滚动 pause / resume 用：resume 时把暂停期间的墙钟时长补回 `started_at`，
    /// 恢复后仍从 pause 那一刻的 opacity / 位置继续，不会按墙钟淡到终态。
    /// `started_at` 为 None（没有活动 shaping）时什么都不做。
    pub(crate) fn shift_started_at(&mut self, delta: std::time::Duration) {
        if let Some(started_at) = self.started_at.as_mut() {
            *started_at += delta;
        }
    }

    pub(crate) fn is_finished(&self, now: Instant) -> bool {
        let Some(started_at) = self.started_at else {
            return true;
        };
        if self.groups.is_empty() {
            return true;
        }
        let elapsed_ms = now.saturating_duration_since(started_at).as_millis() as f64;
        self.duration_ms == 0 || elapsed_ms >= self.duration_ms as f64
    }

    /// Issue #826 评论 25：交给下一笔做 handoff 的当前帧视觉事实。
    ///
    /// range 用各原子在**本 state 的 target 坐标系**里的 `handoff_key`，
    /// 下一笔的 `base_snapshot` 正好就是这份 target。
    pub(crate) fn current_visuals(&self, now: Instant) -> Vec<CurrentVisualCluster> {
        let mut out = Vec::new();
        for (frame, group) in self.sample(now).into_iter().zip(self.groups.iter()) {
            // `sample` 的两侧与各自的 atom 列表逐项对齐，位置一一对应。
            for (atom, side) in group.old_atoms.iter().zip(frame.old.iter()) {
                if side.opacity > EPS && side.rect.w > EPS {
                    out.push(visual_from(atom, side));
                }
            }
            for (atom, side) in group.new_atoms.iter().zip(frame.new.iter()) {
                if side.opacity > EPS && side.rect.w > EPS {
                    out.push(visual_from(atom, side));
                }
            }
        }
        out
    }

    /// 按当前 progress 采样本帧画面：old 侧淡出、new 侧淡入，位置一起补间。
    pub(crate) fn sample(&self, now: Instant) -> Vec<ShapingTransitionFrame> {
        let Some(started_at) = self.started_at else {
            return Vec::new();
        };
        let elapsed_ms = now.saturating_duration_since(started_at).as_millis() as f64;
        let progress = if self.duration_ms == 0 {
            1.0
        } else {
            (elapsed_ms / self.duration_ms as f64).clamp(0.0, 1.0)
        };
        let t = ease_out_cubic(progress);
        self.groups
            .iter()
            .map(|group| {
                // 1:1 时这就是 old_rect / new_rect；1:N、N:1 时是整组包围盒，
                // 视觉上就是「一块 cluster 淡出成另一组 cluster」。
                //
                // Issue #826 评论 26 阻塞 1：**只有 old 侧**朝对侧走。new 侧的
                // 终点必须是它自己的 `atom.rect`，否则它越淡越不透明却越往
                // old 位置跑，`is_finished()` 清层后 canonical 再跳回去。
                let new_region = union_rect(group.new_atoms.iter().map(|atom| &atom.rect));
                let old = sample_old_side(&group.old_atoms, t, new_region.as_ref());
                let new = sample_new_side(&group.new_atoms, t);
                ShapingTransitionFrame { old, new }
            })
            .collect()
    }

    /// 本层真正引用到的行纹理 id（old 侧 + new 侧）。
    pub(crate) fn active_snapshot_ids(&self) -> Vec<LineSnapshotId> {
        let mut ids: Vec<LineSnapshotId> = Vec::new();
        for group in &self.groups {
            for atom in group.old_atoms.iter().chain(group.new_atoms.iter()) {
                if !ids.contains(&atom.snapshot_id) {
                    ids.push(atom.snapshot_id);
                }
            }
        }
        ids
    }

    /// 旧侧 cluster 的行图资源（新侧用最新 target 的行图，由 canonical 栅格化）。
    pub(crate) fn old_source_lines(&self) -> Vec<ConcealSourceLine> {
        self.old_sources.clone()
    }

    /// 新侧 cluster 在 canonical 静态层要挖掉的目标矩形。
    ///
    /// 动画层正在画「正在淡入的那一份」，静态层如果同时画最终位置就是重影。
    pub(crate) fn target_clip_rects(&self) -> Vec<(SourceRect, LineSnapshotId)> {
        self.groups
            .iter()
            .flat_map(|group| {
                group
                    .new_atoms
                    .iter()
                    .map(|atom| (atom.rect.clone(), atom.snapshot_id))
            })
            .collect()
    }

    /// 被本层整块占用的、**属于本次 `base_snapshot` 坐标系**的 old cluster。
    ///
    /// `EditFrontier` 的吞字侧必须把它们排除：普通 Conceal 绝不能声称
    /// `owner = 2..3` 却拿整块 old `fi` 的 glyph 来吞。
    ///
    /// Issue #826 评论 29：**不再从渲染 atoms 反推**。历史 old atom 的
    /// `cluster` 停在它自己那份 source snapshot 上，连续 retarget 之后与当前
    /// base 已经不是同一套坐标 —— 直接返回会让 stale `0..4` 把当前 base 里
    /// 同坐标的新字一起排掉。这里只回填好的 `old_owned_in_base`。
    pub(crate) fn owned_old_clusters(&self) -> Vec<(usize, usize)> {
        self.old_owned_in_base.clone()
    }

    /// 被本层整块占用的、**属于本次 `target_snapshot` 坐标系**的 new cluster。
    ///
    /// `EditFrontier` 的吐字侧（scalar region、carry、settled）必须把它们排除：
    /// 否则同一块视觉 cluster 会同时被 carry 与 scalar Reveal 控制。
    ///
    /// Issue #826 评论 29：同 [`owned_old_clusters`]，只回 `new_owned_in_target`，
    /// 不遍历渲染 atoms 猜坐标。
    pub(crate) fn owned_new_clusters(&self) -> Vec<(usize, usize)> {
        self.new_owned_in_target.clone()
    }

    pub(crate) fn target_text(&self) -> &str {
        &self.target_text
    }

    /// 测试用：当前被本层整块占用的 group 形状（old 原子数 / new 原子数）。
    #[cfg(test)]
    pub(crate) fn owned_clusters_for_test(&self) -> Vec<((usize, usize), (usize, usize))> {
        self.groups
            .iter()
            .map(|group| {
                (
                    group
                        .old_atoms
                        .first()
                        .map(|atom| atom.cluster)
                        .unwrap_or((0, 0)),
                    group
                        .new_atoms
                        .first()
                        .map(|atom| atom.cluster)
                        .unwrap_or((0, 0)),
                )
            })
            .collect()
    }
}

/// 采样 old 侧：从这一帧的真实位置与真实可见宽度出发，朝 `exit_target`
/// 退出，不透明度 `-> 0`。
///
/// `exit_target` 是 1:1 时的新那一块、1:N / N:1 时是整组包围盒。朝对侧走是
/// 有意的：它最后 opacity = 0，终点在哪都看不见。
///
/// **宽度与源矩形都不参与补间**：
///
/// - 宽度保持 `start_visible_width` —— 它本来就在淡出，不需要再长；
/// - `atom.source_rect` 已经是这一帧实际可见的 exact slice（见
///   [`CurrentVisualCluster::source_rect`]），再乘 `visible / rect.w`
///   就是二次裁剪。
fn sample_old_side(
    atoms: &[VisualClusterAtom],
    t: f64,
    exit_target: Option<&SourceRect>,
) -> Vec<ShapingTransitionSide> {
    atoms
        .iter()
        .map(|atom| {
            let to_rect = exit_target.cloned().unwrap_or_else(|| atom.rect.clone());
            let visible = atom.start_visible_width.min(atom.rect.w).max(0.0);
            ShapingTransitionSide {
                snapshot_id: atom.snapshot_id,
                source_rect: atom.source_rect.clone(),
                rect: SourceRect {
                    x: lerp(atom.start_rect.x, to_rect.x, t),
                    y: lerp(atom.start_rect.y, to_rect.y, t),
                    w: visible,
                    h: lerp(atom.start_rect.h, to_rect.h, t),
                },
                opacity: (atom.start_opacity * (1.0 - t)).clamp(0.0, 1.0),
            }
        })
        .collect()
}

/// 采样 new 侧：从这一帧的真实位置出发，朝**自己的 canonical rect**
/// （`atom.rect`）淡入，不透明度 `-> 1`。
///
/// Issue #826 评论 26 阻塞 1：终点必须是 `atom.rect`。旧实现 old / new
/// 两侧共用同一个 `toward`，new 侧被塞的是 old 区域包围盒，于是 old
/// cluster 在 x=10、new cluster 在 x=40 时，new 侧会在 160ms 里一路朝
/// x=10 跑，最后一帧几乎全不透明地待在 old 位置；下一帧
/// `is_finished()` 清层，静态 canonical 立刻跳回 x=40。肉眼就是动画
/// 末尾再跳一次。
///
/// **宽度与源矩形同样不补间**：new 侧是一套**完全不同的字形资源**，
/// 保持自己整字宽 + 完整 source，按比例裁会出现半个连字。
fn sample_new_side(atoms: &[VisualClusterAtom], t: f64) -> Vec<ShapingTransitionSide> {
    atoms
        .iter()
        .map(|atom| ShapingTransitionSide {
            snapshot_id: atom.snapshot_id,
            source_rect: atom.source_rect.clone(),
            rect: SourceRect {
                x: lerp(atom.start_rect.x, atom.rect.x, t),
                y: lerp(atom.start_rect.y, atom.rect.y, t),
                w: atom.rect.w,
                h: lerp(atom.start_rect.h, atom.rect.h, t),
            },
            opacity: (atom.start_opacity + (1.0 - atom.start_opacity) * t).clamp(0.0, 1.0),
        })
        .collect()
}

fn lerp(from: f64, to: f64, t: f64) -> f64 {
    from + (to - from) * t
}

fn visual_from(atom: &VisualClusterAtom, side: &ShapingTransitionSide) -> CurrentVisualCluster {
    CurrentVisualCluster {
        logical_range: atom.primary_handoff_key(),
        // 评论 27 阻塞 1：视觉身份必须跟着**像素实际用的那张纹理**走，
        // 不能拿 `cluster`（base 坐标系的字符串范围）冒充。
        visual_cluster_range: atom.visual_cluster_range,
        snapshot_id: side.snapshot_id,
        source_rect: side.source_rect.clone(),
        dest_rect: side.rect.clone(),
        opacity: side.opacity,
        visible_clip: side.rect.w,
    }
}

/// Issue #826 评论 31：pure delete 的 old cluster 此刻正由上一份 ShapingTransition
/// 的 **new side** 持有时，本层要为这笔删除**让位**，而不是接管。
///
/// 判据只有一条，完全是视觉身份，不引入任何 origin 枚举：本笔 old cluster 的
/// 视觉键 `(base_line.id, base_cluster_range)` 必须出现在
/// `previous.groups[*].new_atoms` 的视觉键集合里 —— 这才说明它是上一份 shaping
/// 的 current new side，而不是某个 canonical 字或历史残影。
///
/// 返回这些 range，调用方只把它们推进 `absorbed`：previous rest 因此不再把
/// 对应的 previous new atom 当 kept child，那份 visual 就干净地交给 Conceal。
///
/// 本层**不 claim、不建 group、不消费 current visual**。吞字吐字的下一站只有
/// 单一遮罩前沿，shaping 不再留一条并行的时间轴（评论 30 曾让它自己淡出，
/// 评论 31 推翻）。coordinator 用同一套判据另采 `conceal_handoffs`。
fn absorb_previous_new_side(
    old_nodes: &[usize],
    old_side: &ClusterIndex<'_>,
    previous: Option<&ShapingTransitionState>,
) -> Vec<(usize, usize)> {
    let Some(previous) = previous else {
        return Vec::new();
    };
    let mut absorbed: Vec<(usize, usize)> = Vec::new();
    for node in old_nodes {
        let (line, cluster) = old_side.cluster(*node);
        let cluster_range = (cluster.byte_start, cluster.byte_end);
        let key = (line.id, cluster_range);
        let held_by_previous_new_side = previous.groups.iter().any(|group| {
            group
                .new_atoms
                .iter()
                .any(|atom| atom.snapshot_id == key.0 && atom.visual_cluster_range == key.1)
        });
        if !held_by_previous_new_side {
            // 不是上一份 shaping 的 current new side：没有需要让位的 previous
            // new child，这块字本就归普通 Delete / Conceal。
            continue;
        }
        if !absorbed.contains(&cluster_range) {
            absorbed.push(cluster_range);
        }
    }
    absorbed
}

/// 为一个新 component 造一个 group。
fn build_group(
    component: &Component,
    old_side: &ClusterIndex<'_>,
    new_side: &ClusterIndex<'_>,
    old_to_new: &OffsetMap,
    deleted_ranges: &[(usize, usize)],
    current_visuals: &[CurrentVisualCluster],
    consumed: &mut HashSet<usize>,
) -> Option<ShapingTransitionGroup> {
    // 先确认 new 侧真有 cluster，再开始消费 handoff —— 消费不可逆。
    if component.new_nodes.is_empty() {
        return None;
    }

    // ── 1. old 侧先接手：屏幕上这一帧的像素属于它 ────────────────────
    //
    // Issue #826 评论 28 阻塞：**只认视觉身份**。
    //
    // `request.base_snapshot` 这一块 old cluster 当前真正对应的视觉原子，
    // 键是 `(base_line.id, base_cluster_range)`。抓不到就没有 handoff ——
    // 直接用 base canonical。绝不能因为某个**历史** visual 的 `logical_range`
    // 与它 overlap 就把那份像素顺手吸进来：同一个 logical range 同帧可以同时
    // 挂着历史 old-side 与当前 new-side 两份完全不同的资源（评论 27 已证明）。
    // 历史 old visual 归 `split_previous_group_by_absorbed()` 维护，不属于
    // 这一笔的 component。
    let mut old_atoms = Vec::new();
    for node in &component.old_nodes {
        let (line, cluster) = old_side.cluster(*node);
        let rect = line.source_rect_to_document_rect(&cluster.source_rect);
        let cluster_range = (cluster.byte_start, cluster.byte_end);
        let handoff = take_visual_handoff(current_visuals, consumed, (line.id, cluster_range));
        // 这一块 old cluster 在最新正文里还剩下哪几段（target 坐标系）——
        // 下一笔要靠它对齐，因为那时 base 坐标系已经变成这份 target。
        //
        // Issue #826 评论 26 阻塞 3：**全部** survivor 都要存，不能 `.next()`
        // 只取第一段。`0..4` 删掉 `1..2` 后剩 `0..1` 与 `2..4`，下一笔只改
        // 后一段时，old atom 必须还能在 current_visuals 里找到自己。
        let keys = handoff_keys_in_target(cluster_range, deleted_ranges, old_to_new);
        old_atoms.push(atom_from_cluster(
            cluster_range,
            keys,
            line,
            cluster,
            rect,
            handoff,
            false,
        ));
    }

    // Issue #826 评论 28 阻塞：这里**曾经**有一个「扫掉所有 leftover、按
    // logical containment 全塞进 old_atoms」的循环，它会把历史 old O 也吸进
    // `A -> C` 这个新 component —— 而后面 `split_previous_group_by_absorbed()`
    // 又要求 old O 继续留在 previous rest group 里淡。同一份像素同帧属于两个
    // group，直接违反「一份视觉像素一帧只能有一个 owner」，render 还会画两遍。
    //
    // 现在历史 old visual 完全交给 `split_previous_group_by_absorbed()` /
    // `retain_visible_old_atoms()` 决定继续淡出还是结束。`f -> fi -> fij` 因此
    // 是：新 group `fi -> fij`，previous rest 里旧 `f` 自己继续淡 —— 仍然都是
    // 同一时钟下的当前态 groups，不是历史队列。
    if old_atoms.is_empty() {
        return None;
    }

    // ── 2. new 侧最后接手，且**只认视觉身份** ────────────────────────
    //
    // Issue #826 评论 28：new 侧默认从自己的新 canonical resource、opacity 0
    // 起步淡入；只有 `(target_line.id, target_cluster_range)` 在屏幕上真实
    // 存在时才从当前像素接手。抓不到就退回 canonical —— 绝不按 logical range
    // 去抢 old 侧或历史像素。
    let mut new_atoms = Vec::new();
    for node in &component.new_nodes {
        let (line, cluster) = new_side.cluster(*node);
        let rect = line.source_rect_to_document_rect(&cluster.source_rect);
        let cluster_range = (cluster.byte_start, cluster.byte_end);
        // 评论 28：new 侧同样**只认视觉身份**。抓不到就是默认路径 ——
        // 从自己的新 canonical resource、opacity 0 起步淡入，绝不按 logical
        // range 去抢别人的像素。
        let handoff = take_visual_handoff(current_visuals, consumed, (line.id, cluster_range));
        new_atoms.push(atom_from_cluster(
            cluster_range,
            vec![cluster_range],
            line,
            cluster,
            rect,
            handoff,
            true,
        ));
    }
    Some(ShapingTransitionGroup {
        old_atoms,
        new_atoms,
    })
}

/// 一块 old cluster 在 target 坐标系里还剩哪几段身份。
///
/// 先按 `deleted_ranges` 求 untouched 片段，再逐段映射到 target；一段都映不
/// 出去时退回 `cluster_range` 本身，保证 handoff 至少有一个可匹配的键。
fn handoff_keys_in_target(
    cluster_range: (usize, usize),
    deleted_ranges: &[(usize, usize)],
    old_to_new: &OffsetMap,
) -> Vec<(usize, usize)> {
    let keys: Vec<(usize, usize)> = untouched_parts(cluster_range, deleted_ranges)
        .into_iter()
        .filter_map(|(start, end)| old_to_new.map_old_range_to_new(start, end))
        .collect();
    if keys.is_empty() {
        vec![cluster_range]
    } else {
        keys
    }
}

/// 把上一份 group 按「本笔接管了哪些 child」拆开。
///
/// Issue #826 评论 26 阻塞 3：不能 `any absorbed => skip whole group`。
/// 1:N 的 group 里第二笔只改其中一个 child 时，整组被跳过 —— 那个 child 被新
/// group 接住，但没被碰的兄弟 new atom 与仍在淡出的 old atom 一起凭空消失。
///
/// 拆法：新 group 只消费被接管的 child 对应的视觉事实；剩下的 new atom 连同
/// 全部仍可见的 old atom 继续挂在 state 里淡。
///
/// 返回 `(拆出来的 group, base 坐标系的 old claims)`。claims 取自 **kept 的
/// previous new atom 的 `cluster`**：`previous.target == request.base`，所以
/// 它们正好就是这一笔 `base_snapshot` 里的 ownership。历史 old atom 不产生
/// claim（Issue #826 评论 29）。
fn split_previous_group_by_absorbed(
    group: &ShapingTransitionGroup,
    absorbed: &[(usize, usize)],
    new_snapshot: &EditorLayoutSnapshot,
    prev_target_to_new: &OffsetMap,
    current_visuals: &[CurrentVisualCluster],
    consumed: &mut HashSet<usize>,
) -> Option<(ShapingTransitionGroup, Vec<(usize, usize)>)> {
    let kept_new: Vec<&VisualClusterAtom> = group
        .new_atoms
        .iter()
        .filter(|atom| {
            !absorbed
                .iter()
                .any(|range| ranges_overlap(atom.cluster, *range))
        })
        .collect();
    // Issue #826 评论 29：必须在 retarget 之前留下 base 系 claims。retarget 之后
    // 这些 atom 的 `cluster` 已经换到**最新 target** 坐标，再拿来当 base claim
    // 就又是一次坐标系混用。
    let old_claims_in_base: Vec<(usize, usize)> =
        kept_new.iter().map(|atom| atom.cluster).collect();
    // 顺序必须与 `current_visuals()` 的产出顺序一致：它先列 old 原子再列 new
    // 原子。1:N 拆分时 old 与它的第一个 survivor、以及首块 new atom 可能共用
    // 同一个字节身份（old `0..4` 的 survivor `0..1` 与 new `0..1`），先处理
    // old 才能让双方各自认领**自己**那份像素，反过来会互相换手、opacity 跳变。
    let old_atoms = retain_visible_old_atoms(group, current_visuals, consumed);
    // `retarget_new_atoms` 只碰 kept 的那些 new atom，被本笔接管的 child 对应的
    // current_visuals 因此原样留给新 group。
    let new_atoms = retarget_new_atoms(
        &kept_new,
        new_snapshot,
        prev_target_to_new,
        current_visuals,
        consumed,
    )
    .unwrap_or_default();
    if new_atoms.is_empty() && old_atoms.is_empty() {
        return None;
    }
    Some((
        ShapingTransitionGroup {
            old_atoms,
            new_atoms,
        },
        old_claims_in_base,
    ))
}

/// 把一份 previous group 里**没被本笔接管**的 new atom 重新映到最新
/// target，继续淡出。
fn retarget_new_atoms(
    kept_new: &[&VisualClusterAtom],
    new_snapshot: &EditorLayoutSnapshot,
    prev_target_to_new: &OffsetMap,
    current_visuals: &[CurrentVisualCluster],
    consumed: &mut HashSet<usize>,
) -> Option<Vec<VisualClusterAtom>> {
    let mut new_atoms = Vec::new();
    for atom in kept_new {
        let Some((start, end)) =
            prev_target_to_new.map_old_range_to_new(atom.cluster.0, atom.cluster.1)
        else {
            // 这段字在新正文里已经没有连续的对应（被吸收进别的 cluster），
            // 本笔若有新 component 会接手；这里直接丢掉这一侧。
            continue;
        };
        let Some((line, cluster)) = find_exact_cluster(new_snapshot, start, end) else {
            continue;
        };
        let rect = line.source_rect_to_document_rect(&cluster.source_rect);
        let cluster_range = (cluster.byte_start, cluster.byte_end);
        let handoff = take_visual_handoff(
            current_visuals,
            consumed,
            (atom.snapshot_id, atom.visual_cluster_range),
        );
        new_atoms.push(atom_from_cluster(
            cluster_range,
            vec![cluster_range],
            line,
            cluster,
            rect,
            handoff,
            true,
        ));
    }
    if new_atoms.is_empty() {
        None
    } else {
        Some(new_atoms)
    }
}

/// 保留上一份里仍可见的 old atom（各自独立匹配 handoff）。
///
/// 旧侧只能保留**这一帧还看得见**的那些原子。`current_visuals` 是上一份交接层
/// 在 `now` 时刻的真实采样，并过滤掉了不透明度 / 可见宽度归零的原子。所以
/// 「没有 handoff」只有一个含义：它已经淡出，屏幕上不再有它的像素。此时若退回
/// `atom.start_opacity`（new group 恒为 1.0），`started_at` 重置会让它闪回
/// 全不透明。
fn retain_visible_old_atoms(
    group: &ShapingTransitionGroup,
    current_visuals: &[CurrentVisualCluster],
    consumed: &mut HashSet<usize>,
) -> Vec<VisualClusterAtom> {
    let mut old_atoms = Vec::new();
    for atom in &group.old_atoms {
        // 评论 26 阻塞 3：按**全部** `handoff_keys` 匹配。1:N 的 old atom
        // 可能有多段 survivor，只取第一段时，下一笔只改后面那一段会让这份
        // old atom 在 `current_visuals` 里找不到归属而消失。
        let Some(handoff) = take_visual_handoff(
            current_visuals,
            consumed,
            (atom.snapshot_id, atom.visual_cluster_range),
        ) else {
            // 评论 28：这一帧已经没有那份像素 —— 不许凭 logical range 去抢
            // 别人的。旧侧就此结束，而不是换一张别人的纹理继续淡。
            continue;
        };
        if handoff.opacity <= EPS {
            continue;
        }
        // handoff 可能来自上一份交接层的 old 侧 —— 那一块字的贴图在更早的
        // 行纹理里，必须继续用它，否则旧侧会突然换一张图。
        old_atoms.push(VisualClusterAtom {
            cluster: atom.cluster,
            handoff_keys: atom.handoff_keys.clone(),
            snapshot_id: handoff.snapshot_id,
            // 评论 27 阻塞 1：retarget 后这一层继续用**接手来的那份**视觉
            // 身份，否则下下笔会拿 base 系的 `cluster` 去当视觉 key。
            visual_cluster_range: handoff.visual_cluster_range,
            source_rect: handoff.source_rect,
            rect: atom.rect.clone(),
            start_rect: handoff.dest_rect,
            start_opacity: handoff.opacity,
            start_visible_width: handoff.visible_clip,
        });
    }
    old_atoms
}

#[allow(clippy::too_many_arguments)]
fn atom_from_cluster(
    cluster: (usize, usize),
    handoff_keys: Vec<(usize, usize)>,
    line: &PreparedLineSnapshot,
    cluster_snapshot: &LineClusterSnapshot,
    rect: SourceRect,
    handoff: Option<CurrentVisualCluster>,
    is_new_side: bool,
) -> VisualClusterAtom {
    let (start_rect, start_opacity, start_visible, snapshot_id, source_rect, visual_cluster_range) =
        match &handoff {
            // old 侧：handoff 就是这一帧实际可见的 exact slice（见
            // [`CurrentVisualCluster::source_rect`]），直接沿用，**不再裁一次**。
            // handoff 可能来自更早的 revision，那时这张行图才是唯一来源。
            //
            // new 侧：handoff 的 `source_rect` 是上一份字形的裁剪片段，拿来画
            // 新 cluster 就是半个连字 —— 必须用它自己的完整新资源。
            //
            // 评论 26 阻塞 2：这里不再有 `visible / rect.w` 的第二次裁剪。
            //
            // 评论 27 阻塞 1：视觉身份跟着**真正用来贴图的那张纹理**走。old 侧
            // 接过 handoff 后像素来自更早的 revision，`cluster` 已经不是那份纹理
            // 里的 range；两侧只有没接手 handoff 时才用自己的 `cluster`。
            Some(h) if !is_new_side => (
                h.dest_rect.clone(),
                h.opacity,
                h.visible_clip,
                h.snapshot_id,
                h.source_rect.clone(),
                h.visual_cluster_range,
            ),
            Some(h) => (
                h.dest_rect.clone(),
                h.opacity,
                h.visible_clip,
                line.id,
                cluster_snapshot.source_rect.clone(),
                cluster,
            ),
            None => (
                rect.clone(),
                if is_new_side { 0.0 } else { 1.0 },
                rect.w,
                line.id,
                cluster_snapshot.source_rect.clone(),
                cluster,
            ),
        };
    VisualClusterAtom {
        cluster,
        handoff_keys,
        visual_cluster_range,
        snapshot_id,
        source_rect,
        rect,
        start_rect,
        start_opacity,
        start_visible_width: start_visible,
    }
}

/// 只按**视觉身份**取一条 handoff：`(snapshot_id, visual_cluster_range)`。
///
/// Issue #826 评论 27 阻塞 1：`logical_range` 只回答「是不是同一段字」，
/// 回答不了「这一帧的像素是哪份资源画的」。1:N 的 `old O -> A + B` 进行中，
/// 历史 old O 与当前 new A 的 `logical_range` 可以**完全相同**（都是
/// `A_range`），而它们的纹理 / revision / opacity / source slice 全都不同。
///
/// 所以**消费 `CurrentVisualCluster` 时只认视觉身份**。
///
/// Issue #826 评论 28 进一步收紧：逻辑 range 仍然用于 OffsetMap 身份映射、
/// component 连通关系、`handoff_keys`，但**不再有资格决定「拿哪份像素」**。
/// 评论 27 已经证明同一个 logical range 同帧可以挂 old/new 两份完全不同的
/// 资源，那就不该再保留「视觉身份失败就退回逻辑身份」的歧义入口。
///
/// 抓不到时的语义是唯一的：这一帧没有那份像素。
/// - 新 component old 侧 → base canonical；
/// - previous old/new atom → 就此结束，不抢别人的；
/// - new side → 自己的新 canonical resource、opacity 0。
fn take_visual_handoff(
    current_visuals: &[CurrentVisualCluster],
    consumed: &mut HashSet<usize>,
    key: (LineSnapshotId, (usize, usize)),
) -> Option<CurrentVisualCluster> {
    let index = current_visuals
        .iter()
        .enumerate()
        .position(|(index, visual)| {
            !consumed.contains(&index)
                && visual.snapshot_id == key.0
                && visual.visual_cluster_range == key.1
        })?;
    consumed.insert(index);
    Some(current_visuals[index].clone())
}

fn ranges_overlap(a: (usize, usize), b: (usize, usize)) -> bool {
    a.0 < b.1 && b.0 < a.1
}

// ── cluster 连通分量 ────────────────────────────────────────────────────────

/// 一个 snapshot 里所有 cluster 的扁平索引，带按 byte 二分查找。
struct ClusterIndex<'a> {
    entries: Vec<Entry<'a>>,
    /// `(byte_start, byte_end, node)` 排序后的查找表。
    ///
    /// Issue #826 评论 27 阻塞 2：**两端都要存**，而且 `byte_end` 这一维
    /// 同样是有序的 —— cluster 互不重叠、byte offset 在整个 document 上全局
    /// 连续，所以 `byte_start` 升序必然蕴含 `byte_end` 升序。
    /// 这样区间查询能在 `[..first]` 这个前缀上**再二分一次**下界，
    /// 否则每个 old cluster 都要从 new 的开头重新扫一遍，N 个 cluster 就是
    /// `1+2+…+N = O(N²)`，只是把「按正文 byte 数量线性扫」换成了
    /// 「按 snapshot cluster 数平方扫」。
    sorted: Vec<(usize, usize, usize)>,
}

struct Entry<'a> {
    line: &'a PreparedLineSnapshot,
    cluster: &'a LineClusterSnapshot,
}

impl<'a> ClusterIndex<'a> {
    fn build(snapshot: &'a EditorLayoutSnapshot) -> Self {
        let mut entries = Vec::new();
        for line in &snapshot.line_snapshots {
            for cluster in &line.clusters {
                entries.push(Entry { line, cluster });
            }
        }
        // 两端一起存：`byte_end` 升序是 cluster 互不重叠的必然结果，查询时
        // 要在 `[..first]` 里再二分下界（见 `nodes_overlapping_interval`）。
        let mut sorted: Vec<(usize, usize, usize)> = entries
            .iter()
            .enumerate()
            .map(|(index, entry)| (entry.cluster.byte_start, entry.cluster.byte_end, index))
            .collect();
        sorted.sort_unstable();
        Self { entries, sorted }
    }

    fn cluster(&self, node: usize) -> (&'a PreparedLineSnapshot, &'a LineClusterSnapshot) {
        let entry = &self.entries[node];
        (entry.line, entry.cluster)
    }

    fn cluster_range(&self, node: usize) -> (usize, usize) {
        let entry = &self.entries[node];
        (entry.cluster.byte_start, entry.cluster.byte_end)
    }

    /// 与 `[lo, hi)` 相交的 cluster 节点（按字节序）。
    ///
    /// Issue #826 评论 26 阻塞 4：替代逐 byte 的 `node_at_byte`。
    ///
    /// Issue #826 评论 27 阻塞 2：**必须是 O(log N + k)**。
    /// 上一版写成 `self.sorted[..first].filter(...)`，每个查询都从 `sorted`
    /// 的开头重扫到 `first` —— N 个 old cluster 对 N 个 new cluster 时总扫描量
    /// `1+2+…+N = O(N²)`。现在两端各二分一次：
    ///
    /// - `to` = 第一个 `start >= hi` 的位置（上界，O(log N)）；
    /// - `from` = 第一个 `end > lo` 的位置（下界，同样 O(log N)，靠 `byte_end`
    ///   升序这个不变量）；
    /// - 真正遍历的只有 `sorted[from..to]`，即**实际相交的那 k 个**。
    ///
    /// `lo < hi` 时恒有 `from <= to`：否则存在 `i < from` 且 `i >= to`，
    /// 即 `end_i <= lo` 且 `start_i >= hi`，而 `start_i < end_i <= lo < hi`
    /// 与 `start_i >= hi` 矛盾。
    ///
    /// 被实际检查的 cluster 数记进测试计数器，用来断言这里是线性而非平方。
    fn nodes_overlapping_interval(&self, lo: usize, hi: usize) -> Vec<usize> {
        if lo >= hi {
            return Vec::new();
        }
        let to = self.sorted.partition_point(|&(start, _, _)| start < hi);
        let from = self
            .sorted
            .partition_point(|&(_, end, _)| end <= lo)
            // 排序不变量只在「cluster 互不重叠」时成立；防御性夹一下，
            // 绝不能在输入热路径上因为 `[from..to]` 反序而 panic。
            .min(to);
        #[cfg(test)]
        test_helpers::note_component_cluster_visits(to - from);
        self.sorted[from..to]
            .iter()
            .map(|&(_, _, node)| node)
            .collect()
    }
}

/// old / new cluster 之间的一个连通分量。
struct Component {
    old_nodes: Vec<usize>,
    new_nodes: Vec<usize>,
}

impl Component {
    /// 这个分量是否必须走 shaping 交接层。
    fn needs_transition(
        &self,
        old_side: &ClusterIndex<'_>,
        new_side: &ClusterIndex<'_>,
        identity_pairs: &[(usize, usize)],
        deleted_ranges: &[(usize, usize)],
        inserted_ranges: &[(usize, usize)],
        old_to_new: &OffsetMap,
    ) -> bool {
        // 1. 逻辑改动只覆盖 cluster 的一部分 —— 评论 24 的原始判据。
        let mixed = self.old_nodes.iter().any(|node| {
            let range = old_side.cluster_range(*node);
            deleted_ranges
                .iter()
                .any(|changed| is_mixed(range, *changed))
        }) || self.new_nodes.iter().any(|node| {
            let range = new_side.cluster_range(*node);
            inserted_ranges
                .iter()
                .any(|changed| is_mixed(range, *changed))
        });
        if mixed {
            return true;
        }
        // 2. 边界变了（1:N / N:1 / N:M）。
        //
        // 注意不能只比数量：`af -> afi -> afij` 里 old `fi 1..3` 与 new `fij 1..4`
        // 数量相同，但 cluster 边界确实变了，必须交接。判据是「把 old 侧 cluster
        // 映到新坐标之后，与 new 侧 cluster 集合是否逐个相等」。
        let mut mapped: Vec<(usize, usize)> = Vec::new();
        for node in &self.old_nodes {
            let (start, end) = old_side.cluster_range(*node);
            match old_to_new.map_old_range_to_new(start, end) {
                Some(range) if !mapped.contains(&range) => mapped.push(range),
                None => return true,
                _ => {}
            }
        }
        let new_ranges: Vec<(usize, usize)> = self
            .new_nodes
            .iter()
            .map(|node| new_side.cluster_range(*node))
            .collect();
        if mapped.len() != new_ranges.len()
            || new_ranges.iter().any(|range| !mapped.contains(range))
        {
            return true;
        }
        // 3. `shaping_identity` 变了。
        //
        // 复杂脚本里在别处插一个字符会改变**未被改动范围 overlap** 的字符形态：
        // 逻辑 range 完整映射、cluster 边界也没变，但 glyph shape 变了。
        // `Reflow` 看到 `is_same_shaping` 为假就 `continue`，本层若也不接，
        // 这块字会直接跳成最新 canonical。
        identity_pairs.iter().any(|(old_node, new_node)| {
            if !self.old_nodes.contains(old_node) || !self.new_nodes.contains(new_node) {
                return false;
            }
            let (_, old_cluster) = old_side.cluster(*old_node);
            let (_, new_cluster) = new_side.cluster(*new_node);
            !old_cluster
                .shaping_identity
                .is_same_shaping(&new_cluster.shaping_identity)
        })
    }
}

/// 用 `OffsetMap` 里那些**没被改动**的字节身份，把 old / new cluster
/// 连成连通分量。
///
/// Issue #826 评论 26 阻塞 4：**不逐 UTF-8 byte 扫**。
///
/// 每个 mapping entry 是一段没被改动的静态文本，`length` 是 UTF-8 bytes；
/// `from_single_edit()` 会生成覆盖整章的 prefix + suffix，逐 byte 扫会把输入
/// 热路径做成 O(正文长度)，而且每个 byte 还做两次二分。
///
/// 现在按 interval sweep：entries 按 `old_byte_offset` 排序，对每个 old cluster
/// 二分找到第一个可能相交的 entry，求 unchanged 交集 `[lo, hi)`，用 entry 的
/// 固定 delta 映成 new `[lo+delta, hi+delta)`，与这个 mapped interval 相交的
/// new cluster 做 union 并记下身份配对。复杂度跟「本次 snapshot 的 cluster 数
/// + map entry 数 + 实际 overlap 数」走。
fn collect_components(
    old_side: &ClusterIndex<'_>,
    new_side: &ClusterIndex<'_>,
    old_to_new: &OffsetMap,
) -> (Vec<Component>, Vec<(usize, usize)>) {
    let old_len = old_side.entries.len();
    let total = old_len + new_side.entries.len();
    let mut parent: Vec<usize> = (0..total).collect();
    let mut identity_pairs: HashSet<(usize, usize)> = HashSet::new();

    let mut entries: Vec<&OffsetMapEntry> = old_to_new.entries.iter().collect();
    entries.sort_by_key(|entry| entry.old_byte_offset.value());

    let mut probes: usize = 0;
    for old_node in 0..old_len {
        let (start, end) = old_side.cluster_range(old_node);
        // entries 按 old_byte_offset 排序；第一个 `old_end > start` 的才可能相交。
        let first =
            entries.partition_point(|entry| entry.old_byte_offset.value() + entry.length <= start);
        for entry in &entries[first..] {
            let entry_start = entry.old_byte_offset.value();
            if entry_start >= end {
                break;
            }
            probes += 1;
            let entry_end = entry_start + entry.length;
            let lo = start.max(entry_start);
            let hi = end.min(entry_end);
            if lo >= hi {
                continue;
            }
            let delta = entry.new_byte_offset.value() as isize - entry_start as isize;
            let new_lo = (lo as isize + delta) as usize;
            let new_hi = (hi as isize + delta) as usize;
            for new_node in new_side.nodes_overlapping_interval(new_lo, new_hi) {
                // union-find 用「old 侧下标 + old 侧长度」作为 new 侧下标；
                // `identity_pairs` 与 `Component` 一律用**各自侧的局部下标**，
                // 否则 `needs_transition` 里的 `contains` 永远不成立。
                union(&mut parent, old_node, new_node + old_len);
                identity_pairs.insert((old_node, new_node));
            }
        }
    }
    #[cfg(test)]
    test_helpers::note_component_probes(probes);

    let mut buckets: HashMap<usize, Component> = HashMap::new();
    for node in 0..total {
        let root = find(&mut parent, node);
        let bucket = buckets.entry(root).or_insert_with(|| Component {
            old_nodes: Vec::new(),
            new_nodes: Vec::new(),
        });
        if node < old_len {
            bucket.old_nodes.push(node);
        } else {
            bucket.new_nodes.push(node - old_len);
        }
    }
    let mut components: Vec<Component> = buckets
        .into_values()
        // Issue #826 评论 30：**纯插** component（`old_nodes` 为空）仍然丢掉，
        // 整块出现归 Reveal。**纯删** component 必须留下 —— 这正是评论 30 的
        // 根因：这里原本连纯删一起滤掉，`build_or_retarget()` 那句
        // 「pure delete EditFrontier 自己就够」的 `continue` 根本轮不到执行，
        // 于是被删的 old cluster 只能落到 `old_owned_in_base` 上却没人画它。
        // 是否需要为它让位由 `absorb_previous_new_side()` 按视觉身份逐个判。
        .filter(|component| !component.old_nodes.is_empty())
        .collect();
    components.sort_by_key(|component| {
        component
            .old_nodes
            .first()
            .map(|node| old_side.cluster_range(*node))
            .unwrap_or((0, 0))
    });
    (components, identity_pairs.into_iter().collect())
}

fn find(parent: &mut [usize], node: usize) -> usize {
    let mut root = node;
    while parent[root] != root {
        root = parent[root];
    }
    let mut current = node;
    while parent[current] != root {
        let next = parent[current];
        parent[current] = root;
        current = next;
    }
    root
}

fn union(parent: &mut [usize], a: usize, b: usize) {
    let ra = find(parent, a);
    let rb = find(parent, b);
    if ra != rb {
        parent[rb] = ra;
    }
}

// ── 自由函数 ────────────────────────────────────────────────────────────────

/// 本笔逻辑改动是否只覆盖了 `cluster` 的一部分。
///
/// 「完整覆盖」或「完全不碰」都不算 mixed —— 前者照旧走 Reveal / Conceal，
/// 后者照旧走 Reflow。只有落在中间这一档才是不可拆的 mixed visual cluster。
fn is_mixed(cluster: (usize, usize), changed: (usize, usize)) -> bool {
    let overlaps = cluster.0 < changed.1 && changed.0 < cluster.1;
    let fully_covered = changed.0 <= cluster.0 && cluster.1 <= changed.1;
    overlaps && !fully_covered
}

/// `cluster` 去掉 `changed` 之后**仍然存在**的那些片段。
///
/// 评论 25 阻塞 4：这里必须返回**全部**片段。旧 cluster `0..4` 删掉中间 `1..2`
/// 之后还剩 `0..1` 与 `2..4` 两段，它们可能分别 shaping 成两块 new cluster；
/// 只取第一段会让后一块被 canonical 直接放出来。
fn untouched_parts(cluster: (usize, usize), changed: &[(usize, usize)]) -> Vec<(usize, usize)> {
    let mut remaining = vec![cluster];
    for &(start, end) in changed {
        let mut next: Vec<(usize, usize)> = Vec::new();
        for (seg_start, seg_end) in remaining {
            if end <= seg_start || seg_end <= start {
                next.push((seg_start, seg_end));
                continue;
            }
            if seg_start < start {
                next.push((seg_start, start.min(seg_end)));
            }
            if end < seg_end {
                next.push((end.max(seg_start), seg_end));
            }
        }
        remaining = next;
    }
    remaining.retain(|(start, end)| end > start);
    remaining
}

fn find_exact_cluster<'a>(
    snapshot: &'a EditorLayoutSnapshot,
    byte_start: usize,
    byte_end: usize,
) -> Option<(&'a PreparedLineSnapshot, &'a LineClusterSnapshot)> {
    for line in &snapshot.line_snapshots {
        if let Some(cluster) = line.cluster_exact_for_range((byte_start, byte_end)) {
            return Some((line, cluster));
        }
    }
    None
}

fn union_rect<'a>(rects: impl Iterator<Item = &'a SourceRect>) -> Option<SourceRect> {
    let mut acc: Option<SourceRect> = None;
    for rect in rects {
        acc = Some(match acc {
            None => rect.clone(),
            Some(current) => {
                let x0 = current.x.min(rect.x);
                let y0 = current.y.min(rect.y);
                let x1 = (current.x + current.w).max(rect.x + rect.w);
                let y1 = (current.y + current.h).max(rect.y + rect.h);
                SourceRect {
                    x: x0,
                    y: y0,
                    w: x1 - x0,
                    h: y1 - y0,
                }
            }
        });
    }
    acc
}

/// Issue #826 评论 26 阻塞 4 的结构回归计数器。
///
/// 只在测试里编译。`collect_components` 每次调用把本线程累计的区间探测次数
/// 累加进来，测试用 [`take_component_probe_count`] 取走并清零 —— 断言它与
/// snapshot 的 cluster 数同量级，而不是跑满 `OffsetMapEntry.length`。
///
/// Issue #826 评论 27 阻塞 2：光数 mapping-entry 探针会**假绿** ——
/// `nodes_overlapping_interval` 每次真正扫了多少 new cluster 才是成本，
/// 那部分由 [`take_component_cluster_visit_count`] 单独计数。
#[cfg(test)]
pub(crate) mod test_helpers {
    use std::cell::Cell;

    thread_local! {
        static COMPONENT_PROBE_COUNT: Cell<usize> = const { Cell::new(0) };
    }

    pub(crate) fn note_component_probes(count: usize) {
        COMPONENT_PROBE_COUNT.with(|cell| cell.set(cell.get() + count));
    }

    /// 取走并清零自上次调用以来的区间探测量。
    pub(crate) fn take_component_probe_count() -> usize {
        COMPONENT_PROBE_COUNT.with(|cell| cell.replace(0))
    }

    thread_local! {
        static COMPONENT_CLUSTER_VISIT_COUNT: Cell<usize> = const { Cell::new(0) };
    }

    /// 记一笔「实际检查了一个 cluster」。
    ///
    /// Issue #826 评论 27 阻塞 2：只数 mapping entry 探针会假绿 ——
    /// `nodes_overlapping_interval` 扫了多少 **new cluster** 才是真正的成本。
    pub(crate) fn note_component_cluster_visits(count: usize) {
        COMPONENT_CLUSTER_VISIT_COUNT.with(|cell| cell.set(cell.get() + count));
    }

    /// 取走并清零自上次调用以来的 cluster 访问次数。
    pub(crate) fn take_component_cluster_visit_count() -> usize {
        COMPONENT_CLUSTER_VISIT_COUNT.with(|cell| cell.replace(0))
    }
}

#[cfg(test)]
mod tests;
