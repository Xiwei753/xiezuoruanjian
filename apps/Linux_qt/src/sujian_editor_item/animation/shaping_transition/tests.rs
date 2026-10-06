//! Issue #826 评论 24/25：不可拆 shaping cluster 的当前态视觉交接 —— 回归测试。
//!
//! 所有测试咬的是同一条底层规则：
//!
//! **Core 的 range 可以按字符切；Qt 的视觉 owner 绝不能切开 shaping cluster。**
//!
//! 所有 cluster 都是 synthetic 的（`fi` 直接构造成 byte range `1..3` 的一块
//! cluster），不依赖系统字体是否真的启用 fi ligature —— 否则这台机器上关掉
//! ligature 就会让测试假绿。
//!
//! 评论 25 又补上两条：
//!
//! 1. **从当前屏幕接手**：owner 换手时，新 owner 的第一帧必须等于旧 owner
//!    上一帧真正画出来的像素，不能回 canonical 起步；
//! 2. **能跨下一笔编辑 retarget**：无关的下一笔不能把正在淡入淡出的东西砍掉。

use std::time::{Duration, Instant};

use writer_core::editor::{OffsetMap, OffsetMapEntry, OffsetMapKind, Utf8ByteOffset};

use super::{collect_components, test_helpers, ClusterIndex, CurrentVisualCluster};
use super::{visible_source_slice, ShapingTransitionState};
use crate::editor::layout::{CaretAffinity, LayoutSnapshot};
use crate::sujian_editor_item::animation::coordinator::EditFrontierRequest;
use crate::sujian_editor_item::animation::coordinator::LinuxEditorAnimationCoordinator;
use crate::sujian_editor_item::animation::edit_frontier::ConcealDirection;
use crate::sujian_editor_item::edit_motion::EditorAnimationKind;
use crate::sujian_editor_item::layout_snapshot::{
    EditorLayoutSnapshot, LineClusterSnapshot, LineSnapshotId, PreparedLineSnapshot,
    ShapingIdentity, SourceRect,
};

const DURATION_MS: u64 = 160;
const HALF_MS: u64 = 80;

fn instant_at(base: Instant, ms: u64) -> Instant {
    base + Duration::from_millis(ms)
}

fn shaping() -> ShapingIdentity {
    shaping_with(1)
}

/// 同一段文字、但 `glyph_indexes_hash` 不同 —— 用来造「字形形态变了」。
fn shaping_with(glyph_indexes_hash: u64) -> ShapingIdentity {
    ShapingIdentity {
        text_content_hash: 1,
        raw_font_fingerprint: String::from("test-font"),
        glyph_indexes_hash,
        cluster_glyph_count: 1,
        direction_rtl: false,
        format_fingerprint: 1,
    }
}

/// 一个 cluster，宽度固定 10px（`stub_for_tests` 用最小 x 当 `visual_x`，
/// 所以文档 x 要靠行内放一个 `x = 0` 的 cluster 钉住偏移）。
fn cluster(byte_start: usize, byte_end: usize, x: f64) -> LineClusterSnapshot {
    cluster_sized(byte_start, byte_end, x, 10.0)
}

fn cluster_sized(byte_start: usize, byte_end: usize, x: f64, w: f64) -> LineClusterSnapshot {
    LineClusterSnapshot {
        byte_start,
        byte_end,
        source_rect: SourceRect {
            x,
            y: 0.0,
            w,
            h: 20.0,
        },
        shaping_identity: shaping(),
    }
}

fn snapshot(lines: Vec<PreparedLineSnapshot>) -> EditorLayoutSnapshot {
    EditorLayoutSnapshot::new(
        LayoutSnapshot::empty_for_tests(),
        lines,
        None,
        None,
        CaretAffinity::Downstream,
    )
}

fn line(clusters: Vec<LineClusterSnapshot>) -> PreparedLineSnapshot {
    PreparedLineSnapshot::stub_for_tests(0, 0.0, 0, clusters)
}

/// `a` 单独一行（第一笔的 base 正文）。
fn a_snapshot() -> EditorLayoutSnapshot {
    snapshot(vec![line(vec![cluster(0, 1, 0.0)])])
}

/// `af`：两个单字符 cluster —— `a` 0..1 @x0，`f` 1..2 @x10。
fn af_snapshot() -> EditorLayoutSnapshot {
    snapshot(vec![line(vec![cluster(0, 1, 0.0), cluster(1, 2, 10.0)])])
}

/// `afi`：**合成后**的正文 —— `a` 0..1 @x0，`fi` 1..3 @x10（16px 宽一块）。
///
/// 注意最新正文里 `f` 与 `i` 已经是一块 16px 宽的视觉 cluster，不是两块。
/// 这正是评论 24 的核心事实：Core 只知道「在 `f` 后面插了一个 `i`」，
/// Qt 却已经把三个字形合成了一块资源。
fn afi_ligated_snapshot() -> EditorLayoutSnapshot {
    snapshot(vec![line(vec![
        cluster(0, 1, 0.0),
        cluster_sized(1, 3, 10.0, 16.0),
    ])])
}

/// `afij`：`fij` 又合成一块 1..4。
fn afij_ligated_snapshot() -> EditorLayoutSnapshot {
    snapshot(vec![line(vec![
        cluster(0, 1, 0.0),
        cluster_sized(1, 4, 10.0, 17.0),
    ])])
}

/// `afi `：在行尾多一个空格，`fi` 仍然是一块 1..3。
fn afi_with_trailing_space_snapshot() -> EditorLayoutSnapshot {
    snapshot(vec![line(vec![
        cluster(0, 1, 0.0),
        cluster_sized(1, 3, 10.0, 16.0),
        cluster(3, 4, 26.0),
    ])])
}

fn insert_request(
    base_snapshot: EditorLayoutSnapshot,
    target_snapshot: EditorLayoutSnapshot,
    base_text: &str,
    target_text: &str,
    inserted_ranges: Vec<(usize, usize)>,
    offset_map: OffsetMap,
    now: Instant,
) -> EditFrontierRequest {
    EditFrontierRequest {
        kind: EditorAnimationKind::Insert,
        base_snapshot,
        target_snapshot,
        deleted_ranges: Vec::new(),
        inserted_ranges,
        offset_map,
        base_text: String::from(base_text),
        target_text: String::from(target_text),
        conceal_direction: ConcealDirection::Forward,
        now,
    }
}

// ── 评论 24：不可拆 cluster 的整块归属 ───────────────────────────────────────

/// 反例 A：部分插入不能把一块 cluster 切成两个视觉 owner。
///
/// - 第一笔 `af`，`f` 的 reveal region 已露 8.75px。
/// - 第二笔在 `f` 后面插 `i`；最新 shaping 把 `fi` 合成一块 1..3。
///
/// 旧代码的两个坏结果（任一都会让这块 cluster 同时归两个 owner）：
/// - 评论 23 的 `mapped_previous` 层用 range `1..2` 建 path，
///   `overlap` 把整块 `fi` 拉进来 → 身份判据失效，走上 fast path；
/// - slow path 下 `carried.range = 1..2` 却拿整块 `fi` 的纹理，而 `2..3`
///   仍留在 scalar reveal path 里 → 同一块视觉 cluster 同时归 carry 和 scalar。
#[test]
fn partial_logical_insert_inside_one_shaping_cluster_cannot_split_visual_owner() {
    let now = Instant::now();
    let half = instant_at(now, HALF_MS);
    let mut coord = LinuxEditorAnimationCoordinator::new();
    coord.set_typing_animation_duration_ms(DURATION_MS as u32);

    // 第一笔：插入 `f`。
    coord.begin_or_extend_edit_frontier(insert_request(
        a_snapshot(),
        af_snapshot(),
        "a",
        "af",
        vec![(1, 2)],
        OffsetMap::from_single_edit(1, (1, 1), 1),
        now,
    ));
    // 80ms：scalar reveal 走了 8.75px（ease_out_cubic(0.5) = 0.875）。
    let pending_before = coord.active_reveal_pending_ranges_for_test(half);
    assert_eq!(
        pending_before,
        vec![(1, 2)],
        "第一笔 80ms 时 f 还没吐完，仍归 scalar reveal"
    );

    // 第二笔：在 `f` 后面插 `i`。Core 逻辑上 f 仍映射 1..2、新 i 是 2..3，
    // 但最新正文里只有一块 1..3 的 fi cluster。
    coord.begin_or_extend_edit_frontier(insert_request(
        af_snapshot(),
        afi_ligated_snapshot(),
        "af",
        "afi",
        vec![(2, 3)],
        OffsetMap::from_single_edit(2, (2, 2), 1),
        half,
    ));

    // 整块 fi 归交接层，不归 Reveal 的任何一层。
    let frontier = coord
        .active_edit_frontier
        .as_ref()
        .expect("第二笔之后前沿仍在");
    assert_eq!(
        frontier.shaping_new_owned,
        vec![(1, 3)],
        "整块 fi cluster（1..3）必须归 shaping_transition，不能留在前沿里"
    );
    assert!(
        frontier.reveal.regions.is_empty(),
        "scalar reveal path 必须把 fi 整块挖掉，不能只挖掉 2..3：\
         剩下的 1..2 会被 build_reveal_layer 的 overlap 再次拉回整块 fi"
    );
    assert!(
        frontier.reveal_carried.is_empty(),
        "carry 也不能声称自己拥有 1..2 却画整块 fi —— 那与 scalar reveal \
         是同一块视觉 cluster 的第二个 owner"
    );
    assert!(
        frontier.reveal_settled.is_empty(),
        "已完整露出的部分也不能在 mixed cluster 上被 settle 成 canonical：\
         canonical 画的是合成后的 fi，视觉上仍是跳变"
    );

    // 交接层确实建出来了这一对 old/new cluster。
    let shaping = coord
        .active_shaping_transition
        .as_ref()
        .expect("mixed cluster 必须进入交接层");
    assert_eq!(shaping.owned_clusters_for_test(), vec![((1, 2), (1, 3))]);
}

/// 反例 B：部分删除时，吞字层不能拿着整块 old `fi` 的 glyph 声称 owner 只是 `i`。
///
/// 旧 `fi` cluster = 1..3，只 Backspace 删掉 `i`（2..3），最新正文重新
/// shaping 出单独的 `f`（1..2）。
///
/// 旧代码的 `collect_conceal_glyphs` 走 `overlap`，会把整块 1..3 的 fi
/// glyph 塞进 ConcealGlyphGeometry，而 base owner 只有 2..3 —— 肉眼是
/// 「删一个字符却把整个 fi 吞掉」，而且与新 canonical 的单独 `f` 重影。
#[test]
fn partial_logical_delete_inside_one_shaping_cluster_uses_atomic_cluster_transition() {
    let now = Instant::now();
    let mut coord = LinuxEditorAnimationCoordinator::new();
    coord.set_typing_animation_duration_ms(DURATION_MS as u32);

    coord.begin_or_extend_edit_frontier(EditFrontierRequest {
        kind: EditorAnimationKind::Delete,
        base_snapshot: afi_ligated_snapshot(),
        base_text: String::from("afi"),
        target_snapshot: af_snapshot(),
        target_text: String::from("af"),
        deleted_ranges: vec![(2, 3)],
        inserted_ranges: Vec::new(),
        offset_map: OffsetMap::from_single_edit(3, (2, 3), 0),
        conceal_direction: ConcealDirection::Backward,
        now,
    });

    let frontier = coord.active_edit_frontier.as_ref().expect("删除后前沿仍在");
    assert_eq!(
        frontier.shaping_old_owned,
        vec![(1, 3)],
        "整块 old fi cluster（1..3）必须归 shaping_transition"
    );
    assert!(
        frontier.conceal.regions.is_empty(),
        "吞字 path 必须把 fi 整块挖掉。留着的唯一可能来源就是 \
         collect_conceal_glyphs 的 overlap 拿到了整块 fi 的 glyph，\
         那正是「删 i 却吞掉整个 fi」"
    );
    assert!(
        frontier.conceal_glyphs.is_empty(),
        "旧 fi 的 glyph 不能进普通吞字 overlay"
    );

    let shaping = coord
        .active_shaping_transition
        .as_ref()
        .expect("mixed cluster 必须进入交接层");
    assert_eq!(shaping.owned_clusters_for_test(), vec![((1, 3), (1, 2))]);

    // 旧侧行图要真的带过来（画旧 cluster 必须有那张图）。
    let sources = coord.active_shaping_transition_source_lines();
    assert_eq!(sources.len(), 1, "旧 fi 所在行必须作为过渡层的纹理来源登记");
}

// ── 评论 25 阻塞 1：从当前屏幕接手 ───────────────────────────────────────────

/// 阻塞 1：交接层的第一帧必须接着 Reveal 上一帧真正画出来的像素。
///
/// `af` 的 `f` 在 80ms 时只露了 8.75px。第二笔插 `i` 让 shaping 变成
/// `fi cluster 1..3`，此时 owner 从 Reveal 换到 ShapingTransition。
///
/// 旧实现直接拿 `base_snapshot` 里的**完整** `f` 当 old 侧起步，
/// 于是第二笔同一帧就会「8.75px 突然补满 10px，再开始 f -> fi 淡变」。
#[test]
fn shaping_transition_starts_from_current_reveal_pixels_not_full_base_cluster() {
    let now = Instant::now();
    let half = instant_at(now, HALF_MS);
    let mut coord = LinuxEditorAnimationCoordinator::new();
    coord.set_typing_animation_duration_ms(DURATION_MS as u32);

    coord.begin_or_extend_edit_frontier(insert_request(
        a_snapshot(),
        af_snapshot(),
        "a",
        "af",
        vec![(1, 2)],
        OffsetMap::from_single_edit(1, (1, 1), 1),
        now,
    ));
    // 确认前提：80ms 时 f 只露了 8.75px。
    let sample = coord.sample_edit_frontier(half).expect("第一笔动画仍在");
    let before: Vec<f64> = coord
        .active_edit_frontier
        .as_ref()
        .expect("前沿仍在")
        .current_reveal_visuals(sample.progress)
        .iter()
        .map(|item| item.visible_width)
        .collect();
    assert_eq!(before.len(), 1, "第一帧屏幕上只有 f 这一块");
    assert!(
        (before[0] - 8.75).abs() < 1e-6,
        "前提：80ms 时 f 只露了 8.75px，实际 {}",
        before[0]
    );

    // 第二笔插 `i`：owner 换手。
    coord.begin_or_extend_edit_frontier(insert_request(
        af_snapshot(),
        afi_ligated_snapshot(),
        "af",
        "afi",
        vec![(2, 3)],
        OffsetMap::from_single_edit(2, (2, 2), 1),
        half,
    ));

    let frames = coord.shaping_transition_glyphs(half);
    assert_eq!(frames.len(), 1, "只有一个 group");
    let old = &frames[0].old;
    assert_eq!(old.len(), 1, "old 侧只有一块 cluster");
    assert!(
        (old[0].rect.w - 8.75).abs() < 1e-6,
        "old 侧第一帧的可见宽度必须仍是上一帧的 8.75px，实际 {}：\
         回 canonical 起步会先补满 10px 再淡变",
        old[0].rect.w
    );
    assert!(
        (old[0].rect.x - 10.0).abs() < 1e-6,
        "old 侧第一帧的位置也必须是上一帧的屏幕位置，实际 {}",
        old[0].rect.x
    );
    assert!(
        (old[0].opacity - 1.0).abs() < 1e-6,
        "old 侧第一帧不透明度必须仍是 1.0（屏幕上它就是完整可见的那一块）"
    );
    // 源矩形裁的也是「同一块 cluster 里已露出的那一段」。
    assert!(
        (old[0].source_rect.w - 8.75).abs() < 1e-6,
        "源矩形同样只能露出 8.75/10，实际 {}",
        old[0].source_rect.w
    );

    // new 侧是另一套资源：整字宽、从 0 不透明度淡入，绝不能裁成半个连字。
    let new = &frames[0].new;
    assert_eq!(new.len(), 1);
    assert!(
        (new[0].rect.w - 16.0).abs() < 1e-6,
        "new 侧是整块 fi，必须是完整 16px，实际 {}",
        new[0].rect.w
    );
    assert!(
        (new[0].source_rect.w - 16.0).abs() < 1e-6,
        "new 侧必须用 fi 自己的完整资源，实际 {}",
        new[0].source_rect.w
    );
    assert!(
        new[0].opacity <= 1e-6,
        "new 侧第一帧还没淡入，实际 opacity = {}",
        new[0].opacity
    );

    // 过渡中段：new 侧必须始终是**完整**的 fi 资源。
    //
    // 旧实现把宽度朝对侧矩形补间，于是 new 侧会从 16px 朝 old 侧的 10px 收缩 ——
    // 屏幕上就是「淡入过程中 fi 被横向裁短」，也就是半个连字。视觉 owner 一旦
    // 接手就是整块，宽度不参与补间。
    let mid = coord.shaping_transition_glyphs(instant_at(half, 80));
    let new_mid = &mid[0].new;
    assert_eq!(new_mid.len(), 1);
    assert!(
        (new_mid[0].rect.w - 16.0).abs() < 1e-6,
        "过渡中段 new 侧仍是完整 16px，实际 {}：宽度朝对侧补间会裁出半个连字",
        new_mid[0].rect.w
    );
    assert!(
        (new_mid[0].source_rect.w - 16.0).abs() < 1e-6,
        "过渡中段 new 侧的源矩形也必须是完整 fi，实际 {}",
        new_mid[0].source_rect.w
    );
    // old 侧则保持这一帧真实的可见宽度，不因为 new 侧更宽而横向拉伸。
    let old_mid = &mid[0].old;
    assert!(
        (old_mid[0].rect.w - 8.75).abs() < 1e-6,
        "过渡中段 old 侧可见宽度仍是 8.75px，实际 {}",
        old_mid[0].rect.w
    );
    assert!(old_mid[0].opacity < 1.0, "old 侧确实在淡出");
    assert!(new_mid[0].opacity > 0.0, "new 侧确实在淡入");
}

// ── 评论 25 阻塞 2：跨下一笔编辑 retarget ─────────────────────────────────────

/// 阻塞 2：`f -> fi` 的交接播到一半时，来一笔**与它无关**的普通输入，
/// 交接层不能被清掉。
///
/// 旧实现每一笔新编辑都重新 `build()`，这一笔没有 mixed cluster 就直接
/// `active_shaping_transition = None` —— 肉眼是「淡变播到一半突然跳终态」。
#[test]
fn unrelated_next_edit_does_not_finish_active_shaping_transition() {
    let now = Instant::now();
    let half = instant_at(now, HALF_MS);
    let quarter_after = instant_at(half, 40);
    let mut coord = LinuxEditorAnimationCoordinator::new();
    coord.set_typing_animation_duration_ms(DURATION_MS as u32);

    coord.begin_or_extend_edit_frontier(insert_request(
        a_snapshot(),
        af_snapshot(),
        "a",
        "af",
        vec![(1, 2)],
        OffsetMap::from_single_edit(1, (1, 1), 1),
        now,
    ));
    coord.begin_or_extend_edit_frontier(insert_request(
        af_snapshot(),
        afi_ligated_snapshot(),
        "af",
        "afi",
        vec![(2, 3)],
        OffsetMap::from_single_edit(2, (2, 2), 1),
        half,
    ));
    let before = coord.shaping_transition_glyphs(quarter_after);
    assert_eq!(before.len(), 1, "交接层已经建立");
    let old_before = before[0].old.clone();
    let new_before = before[0].new.clone();
    assert!(
        (old_before[0].opacity - 0.421875).abs() < 1e-6,
        "40ms 时 old 侧 opacity 约 0.4219，实际 {}",
        old_before[0].opacity
    );

    // 第三笔：在行尾插一个与 `fi` 完全无关的空格。这一笔没有任何 mixed cluster。
    coord.begin_or_extend_edit_frontier(insert_request(
        afi_ligated_snapshot(),
        afi_with_trailing_space_snapshot(),
        "afi",
        "afi ",
        vec![(3, 4)],
        OffsetMap::from_single_edit(3, (3, 3), 1),
        quarter_after,
    ));
    assert!(
        coord.active_shaping_transition.is_some(),
        "无关的下一笔不能把正在淡入淡出的交接层清掉 —— 那会让它播到一半跳终态"
    );

    // 同一瞬间的画面必须与插字前逐项连续。
    let after = coord.shaping_transition_glyphs(quarter_after);
    assert_eq!(after.len(), 1);
    assert_eq!(after[0].old.len(), old_before.len());
    for (index, side) in after[0].old.iter().enumerate() {
        assert!(
            (side.opacity - old_before[index].opacity).abs() < 1e-6,
            "old[{}] opacity 断裂：{} -> {}",
            index,
            old_before[index].opacity,
            side.opacity
        );
        assert!(
            (side.rect.w - old_before[index].rect.w).abs() < 1e-6
                && (side.rect.x - old_before[index].rect.x).abs() < 1e-6,
            "old[{}] 矩形断裂：({:.4},{:.4}) -> ({:.4},{:.4})",
            index,
            old_before[index].rect.x,
            old_before[index].rect.w,
            side.rect.x,
            side.rect.w
        );
    }
    assert_eq!(after[0].new.len(), new_before.len());
    for (index, side) in after[0].new.iter().enumerate() {
        assert!(
            (side.opacity - new_before[index].opacity).abs() < 1e-6,
            "new[{}] opacity 断裂：{} -> {}",
            index,
            new_before[index].opacity,
            side.opacity
        );
        assert!(
            (side.rect.w - new_before[index].rect.w).abs() < 1e-6
                && (side.rect.x - new_before[index].rect.x).abs() < 1e-6,
            "new[{}] 矩形断裂：({:.4},{:.4}) -> ({:.4},{:.4})",
            index,
            new_before[index].rect.x,
            new_before[index].rect.w,
            side.rect.x,
            side.rect.w
        );
    }
}

/// 阻塞 3：连续两次改变同一块 cluster，第二笔必须从第一笔的**当前帧**接手。
///
/// `af -> afi -> afij`：第一笔 40ms 时屏幕上是 `f`(opacity≈0.42, 8.75px) +
/// `fi`(opacity≈0.58, 16px) 两层。第二笔第一帧必须还是这两层，不能退回
/// 「完整 fi 作为 old 侧、opacity = 1」。
///
/// 旧实现用 `previous.owned_old_clusters == self.owned_old_clusters` 猜连续性：
/// 第一份是 `(1,2)`、第二份是 `(1,3)`，不相等 -> `started_at` 重置 -> 闪回全不透明。
#[test]
fn consecutive_mixed_edit_retargets_from_current_frame() {
    let now = Instant::now();
    let half = instant_at(now, HALF_MS);
    let quarter_after = instant_at(half, 40);
    let mut coord = LinuxEditorAnimationCoordinator::new();
    coord.set_typing_animation_duration_ms(DURATION_MS as u32);

    coord.begin_or_extend_edit_frontier(insert_request(
        a_snapshot(),
        af_snapshot(),
        "a",
        "af",
        vec![(1, 2)],
        OffsetMap::from_single_edit(1, (1, 1), 1),
        now,
    ));
    coord.begin_or_extend_edit_frontier(insert_request(
        af_snapshot(),
        afi_ligated_snapshot(),
        "af",
        "afi",
        vec![(2, 3)],
        OffsetMap::from_single_edit(2, (2, 2), 1),
        half,
    ));
    let before = coord.shaping_transition_glyphs(quarter_after);
    assert_eq!(before.len(), 1);
    assert_eq!(before[0].old.len(), 1, "第一段交接的 old 侧只有 f");
    assert_eq!(before[0].new.len(), 1, "第一段交接的 new 侧只有 fi");

    // 第三笔：再插一个 `j`，`fij` 合成一块 1..4。同一块 cluster 被再次改变。
    coord.begin_or_extend_edit_frontier(insert_request(
        afi_ligated_snapshot(),
        afij_ligated_snapshot(),
        "afi",
        "afij",
        vec![(3, 4)],
        OffsetMap::from_single_edit(3, (3, 3), 1),
        quarter_after,
    ));
    let after = coord.shaping_transition_glyphs(quarter_after);
    // 评论 28 ownership：历史 old `f` 不再被新 component 按 logical overlap 吸
    // 进来，而是留在 previous rest group 里继续淡 —— 所以是**两个** group：
    // 新 group `old=[fi] / new=[fij]`，previous rest `old=[f] / new=[]`。
    // 两层都还在画，但同一份像素只属于一个 group。
    assert_eq!(after.len(), 2, "新 group + previous rest 各一层");
    let after_old: Vec<&_> = after.iter().flat_map(|frame| frame.old.iter()).collect();
    // 屏幕上的两层都必须还在：f 继续淡出、fi 变成这轮的 old 侧继续淡出。
    assert_eq!(
        after_old.len(),
        2,
        "第二笔第一帧必须同时画出 f 与 fi 两层，不能把 f 弄丢"
    );

    // 按宽度认出哪一层是 f（8.75px）、哪一层是 fi（16px），逐项比对连续性。
    let mut matched = 0;
    for side in after_old {
        let previous = before[0]
            .old
            .iter()
            .chain(before[0].new.iter())
            .find(|candidate| (candidate.rect.w - side.rect.w).abs() < 1e-6);
        let Some(previous) = previous else {
            continue;
        };
        matched += 1;
        assert!(
            (side.opacity - previous.opacity).abs() < 1e-6,
            "宽度 {:.4} 那一层 opacity 断裂：{} -> {}",
            side.rect.w,
            previous.opacity,
            side.opacity
        );
        assert!(
            (side.rect.x - previous.rect.x).abs() < 1e-6,
            "宽度 {:.4} 那一层位置断裂：{} -> {}",
            side.rect.w,
            previous.rect.x,
            side.rect.x
        );
    }
    assert_eq!(matched, 2, "两层都必须能从上一帧找到对应，宽度不能变");
}

// ── 评论 25 阻塞 4：1:N / N:1 的不可拆区域 ───────────────────────────────────

/// 阻塞 4（一块 old 拆成两块 new）：旧 cluster `0..4` 删掉中间 `1..2` 之后
/// 还剩 `0..1` 与 `2..4` 两段，最新排版把它们 shaping 成两块 new cluster。
///
/// 旧实现 `untouched_part()` 只返回**第一个**片段，于是只会登记
/// `old 0..4 -> new A`，`new B` 不进交接层、被 canonical 直接放出来（跳变）。
#[test]
fn shaping_transition_supports_one_old_to_two_new_clusters() {
    let now = Instant::now();
    let mut coord = LinuxEditorAnimationCoordinator::new();
    coord.set_typing_animation_duration_ms(DURATION_MS as u32);

    let old = snapshot(vec![line(vec![cluster_sized(0, 4, 0.0, 40.0)])]);
    let new = snapshot(vec![line(vec![
        cluster_sized(0, 1, 0.0, 10.0),
        cluster_sized(1, 3, 10.0, 30.0),
    ])]);
    coord.begin_or_extend_edit_frontier(EditFrontierRequest {
        kind: EditorAnimationKind::Delete,
        base_snapshot: old,
        base_text: String::from("abcd"),
        target_snapshot: new,
        target_text: String::from("acd"),
        deleted_ranges: vec![(1, 2)],
        inserted_ranges: Vec::new(),
        offset_map: OffsetMap::from_single_edit(4, (1, 2), 0),
        conceal_direction: ConcealDirection::Forward,
        now,
    });

    let shaping = coord
        .active_shaping_transition
        .as_ref()
        .expect("一块 old 拆成两块 new 必须整组交接");
    assert_eq!(shaping.groups.len(), 1, "两段必须落在同一个 group 里");
    let group = &shaping.groups[0];
    assert_eq!(
        group
            .old_atoms
            .iter()
            .map(|atom| atom.cluster)
            .collect::<Vec<_>>(),
        vec![(0, 4)],
        "old 侧是那一整块不可拆 cluster"
    );
    assert_eq!(
        group
            .new_atoms
            .iter()
            .map(|atom| atom.cluster)
            .collect::<Vec<_>>(),
        vec![(0, 1), (1, 3)],
        "new 侧两块都要进来；只登记第一块会让第二块被 canonical 直接放出来"
    );
    assert_eq!(shaping.owned_old_clusters(), vec![(0, 4)]);
    assert_eq!(shaping.owned_new_clusters(), vec![(0, 1), (1, 3)]);

    // 旧侧一整块都被交接层占了，普通吞字不能碰。
    let frontier = coord.active_edit_frontier.as_ref().expect("删除后前沿仍在");
    assert!(
        frontier.conceal_glyphs.is_empty(),
        "old 0..4 整块归交接层，普通吞字不能拿它的 glyph"
    );
    assert!(frontier.conceal.regions.is_empty());
}

/// 阻塞 4（两块 old 合成一块 new）：反向同理。
#[test]
fn shaping_transition_supports_two_old_to_one_new_cluster() {
    let now = Instant::now();
    let mut coord = LinuxEditorAnimationCoordinator::new();
    coord.set_typing_animation_duration_ms(DURATION_MS as u32);

    let old = snapshot(vec![line(vec![
        cluster_sized(0, 2, 0.0, 20.0),
        cluster_sized(2, 4, 20.0, 20.0),
    ])]);
    let new = snapshot(vec![line(vec![cluster_sized(0, 3, 0.0, 30.0)])]);
    coord.begin_or_extend_edit_frontier(EditFrontierRequest {
        kind: EditorAnimationKind::Delete,
        base_snapshot: old,
        base_text: String::from("abcd"),
        target_snapshot: new,
        target_text: String::from("abc"),
        deleted_ranges: vec![(3, 4)],
        inserted_ranges: Vec::new(),
        offset_map: OffsetMap::from_single_edit(4, (3, 4), 0),
        conceal_direction: ConcealDirection::Backward,
        now,
    });

    let shaping = coord
        .active_shaping_transition
        .as_ref()
        .expect("两块 old 合成一块 new 必须整组交接");
    assert_eq!(shaping.groups.len(), 1);
    let group = &shaping.groups[0];
    assert_eq!(
        group
            .old_atoms
            .iter()
            .map(|atom| atom.cluster)
            .collect::<Vec<_>>(),
        vec![(0, 2), (2, 4)],
        "old 侧两块都要进来"
    );
    assert_eq!(
        group
            .new_atoms
            .iter()
            .map(|atom| atom.cluster)
            .collect::<Vec<_>>(),
        vec![(0, 3)],
        "new 侧是合成后的那一整块"
    );
    assert_eq!(shaping.owned_old_clusters(), vec![(0, 2), (2, 4)]);
    assert_eq!(shaping.owned_new_clusters(), vec![(0, 3)]);
}

/// `build()` 原本只看 `is_mixed`（changed range 是否只覆盖 cluster 一部分），
/// 于是漏掉这一类：逻辑 range 完整映射、cluster 边界也没变，但
/// `shaping_identity` 变了（复杂脚本里在别处插字符会改变旁边字的形态）。
///
/// `Reflow` 看到 `is_same_shaping` 为假就 `continue`，本层若也不接，
/// 这块字会直接跳成最新 canonical。
#[test]
fn context_only_shaping_change_enters_transition_even_without_changed_overlap() {
    let now = Instant::now();
    let mut coord = LinuxEditorAnimationCoordinator::new();
    coord.set_typing_animation_duration_ms(DURATION_MS as u32);

    // `ab`：a 0..1、b 1..2。
    let before = snapshot(vec![line(vec![cluster(0, 1, 0.0), cluster(1, 2, 10.0)])]);
    // `abX`：b 的 cluster 边界与逻辑 range 都没变，但字形形态变了。
    let after = snapshot(vec![line(vec![
        cluster(0, 1, 0.0),
        LineClusterSnapshot {
            shaping_identity: shaping_with(2),
            ..cluster(1, 2, 10.0)
        },
        cluster(2, 3, 20.0),
    ])]);
    coord.begin_or_extend_edit_frontier(insert_request(
        before,
        after,
        "ab",
        "abX",
        vec![(2, 3)],
        OffsetMap::from_single_edit(2, (2, 2), 1),
        now,
    ));

    let shaping = coord
        .active_shaping_transition
        .as_ref()
        .expect("只有 shaping_identity 变化也必须交接");
    assert_eq!(
        shaping.owned_clusters_for_test(),
        vec![((1, 2), (1, 2))],
        "b 的 range 完全没变、也没被 inserted range overlap，靠 identity 判据接住"
    );
    let frontier = coord.active_edit_frontier.as_ref().expect("前沿仍在");
    assert_eq!(
        frontier.shaping_new_owned,
        vec![(1, 2)],
        "b 整块退出前沿；X 仍然走普通 scalar reveal"
    );
    assert_eq!(
        frontier.new_ranges(),
        vec![(2, 3)],
        "只有新插入的 X 留在 scalar reveal path 里"
    );
}

// ── 视觉 owner 唯一性 ───────────────────────────────────────────────────────

/// 一块**视觉** cluster 每帧最多一个动画 owner。
///
/// 把 Frontier scalar、Reveal carry、Conceal、Reflow move、ShapingTransition
/// 当前这一帧真正引用的 cluster 全部汇总，按
/// `(layout revision, paragraph_id, line ordinal, cluster byte range)` 查重。
///
/// 关键是**按真实 cluster 查重**，而不是按各层自报的逻辑 range：两个不相邻
/// 的逻辑子 range 仍然可能引用同一块视觉 cluster（`fi 1..3` 被切成
/// `carry(1..2)` + `scalar(2..3)` 就是反例 A 的坏结果）。
#[test]
fn one_visual_cluster_has_exactly_one_animation_owner() {
    let now = Instant::now();
    let half = instant_at(now, HALF_MS);
    let mut coord = LinuxEditorAnimationCoordinator::new();
    coord.set_typing_animation_duration_ms(DURATION_MS as u32);

    // `ab` -> 在 `b` 前面插 `X`（`aXb`）：`X` 走 scalar reveal，`b` 被推后走 Reflow。
    let ab = snapshot(vec![line(vec![cluster(0, 1, 0.0), cluster(1, 2, 10.0)])]);
    let axb = snapshot(vec![line(vec![
        cluster(0, 1, 0.0),
        cluster(1, 2, 10.0),
        cluster(2, 3, 20.0),
    ])]);
    let axb_for_second = axb.clone();
    coord.begin_or_extend_edit_frontier(insert_request(
        ab,
        axb,
        "ab",
        "aXb",
        vec![(1, 2)],
        OffsetMap::from_single_edit(2, (1, 1), 1),
        now,
    ));

    // `aXb` -> 在 `X` 后面插 `i`；最新 shaping 把 `Xi` 合成一块 1..3。
    let axb_ligated = snapshot(vec![line(vec![
        cluster(0, 1, 0.0),
        cluster_sized(1, 3, 10.0, 16.0),
        cluster(3, 4, 30.0),
    ])]);
    coord.begin_or_extend_edit_frontier(insert_request(
        axb_for_second,
        axb_ligated,
        "aXb",
        "aXib",
        vec![(2, 3)],
        OffsetMap::from_single_edit(3, (2, 2), 1),
        half,
    ));

    let frontier = coord.active_edit_frontier.as_ref().expect("前沿仍在");
    let base = &frontier.base_snapshot;
    let target = &frontier.target_snapshot;
    let pool: Vec<&EditorLayoutSnapshot> = vec![base, target];

    // ── 汇总本帧的全部视觉 owner ──────────────────────────────────────
    let mut owners: Vec<(&'static str, VisualClusterKey)> = Vec::new();
    let mut record = |label: &'static str, id: LineSnapshotId, range: (usize, usize)| {
        for key in resolve_cluster_keys(&pool, id, range) {
            owners.push((label, key));
        }
    };

    for region in &frontier.reveal.regions {
        record("scalar_reveal", target.line_snapshots[0].id, region.range);
    }
    for carried in &frontier.reveal_carried {
        record("reveal_carry", carried.snapshot_id, carried.range);
    }
    for glyph in &frontier.conceal_glyphs {
        record("conceal", glyph.snapshot_id, glyph.range);
    }
    for range in coord.active_reflow_new_ranges() {
        record("reflow", target.line_snapshots[0].id, range);
    }
    if let Some(shaping) = coord.active_shaping_transition.as_ref() {
        for group in &shaping.groups {
            for atom in group.old_atoms.iter().chain(group.new_atoms.iter()) {
                // 评论 26 阻塞 3：一个 atom 可能有多段 survivor，每段都是一个
                // 独立的视觉身份，都必须确认没有第二个 owner。
                for key in &atom.handoff_keys {
                    record("shaping_transition", atom.snapshot_id, *key);
                }
            }
        }
    }

    for (index, (label, key)) in owners.iter().enumerate() {
        for (other_label, other_key) in owners.iter().skip(index + 1) {
            assert!(
                key != other_key || label == other_label,
                "视觉 cluster {key:?} 同时被 `{label}` 与 `{other_label}` 控制：\
                 同一块视觉 cluster 有两个动画 owner 就是重影/形状跳变"
            );
        }
    }

    // 反例 A 的具体后果：`Xi 1..3` 这块合成 cluster 的 owner 必须是且只能是
    // 交接层。它一旦落进 scalar reveal 或 carry，就是第二个 owner。
    //
    // 注意按**层**去重而不是按条目：交接层自己会为同一块 cluster 记两次
    // （old 侧淡出 + new 侧淡入），那是它内部的 cross-fade，不是双 owner。
    let mut mixed_layers: Vec<&str> = owners
        .iter()
        .filter(|(_, key)| key.cluster_start == 1 && key.cluster_end == 3)
        .map(|(label, _)| *label)
        .collect();
    mixed_layers.sort_unstable();
    mixed_layers.dedup();
    assert_eq!(
        mixed_layers,
        vec!["shaping_transition"],
        "Xi 1..3 只能由交接层拥有"
    );
}

/// 一块视觉 cluster 的身份：revision + 行 + cluster 的完整 byte range。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct VisualClusterKey {
    layout_revision: u64,
    paragraph_id: u64,
    visual_line_ordinal: u32,
    cluster_start: usize,
    cluster_end: usize,
}

/// 把「某个 snapshot 里、某个 byte range 实际引用到的视觉 cluster」解析出来。
///
/// 刻意用 **overlap** 语义：这正是「两个逻辑子 range 引用同一块视觉
/// cluster」能被抓住的原因。用 exact 语义就抓不到反例 A 的坏结果。
fn resolve_cluster_keys(
    pool: &[&EditorLayoutSnapshot],
    id: LineSnapshotId,
    range: (usize, usize),
) -> Vec<VisualClusterKey> {
    let mut out = Vec::new();
    for snapshot in pool {
        for line in &snapshot.line_snapshots {
            if line.id != id {
                continue;
            }
            for cluster in line.clusters_overlapping_range(range.0, range.1) {
                let key = VisualClusterKey {
                    layout_revision: line.id.layout_revision,
                    paragraph_id: line.id.paragraph_id,
                    visual_line_ordinal: line.id.visual_line_ordinal,
                    cluster_start: cluster.byte_start,
                    cluster_end: cluster.byte_end,
                };
                if !out.contains(&key) {
                    out.push(key);
                }
            }
        }
    }
    out
}

// ── 评论 26 阻塞 1：new 侧的终点必须是它自己的 canonical rect ────────────────

/// 阻塞 1：old cluster 在 x=10、new cluster 在 x=40 时，new 侧绝不能朝 x=10 跑。
///
/// 旧实现两侧共用一个 `toward`，new 侧被塞的是 old 区域包围盒：
///
/// - t=0：new 在 start_rect（x=40），opacity=0；
/// - t→1：越来越不透明，却越来越往 x=10 跑；
/// - 最后一帧几乎全不透明地待在 x=10；
/// - 下一帧 `is_finished()` 清层，静态 canonical 立刻跳回 x=40。
///
/// 肉眼就是动画末尾再跳一次。
#[test]
fn shaping_new_side_ends_at_canonical_rect_before_transition_finishes() {
    let now = Instant::now();
    let mut coord = LinuxEditorAnimationCoordinator::new();
    coord.set_typing_animation_duration_ms(DURATION_MS as u32);

    // `ab`：a 0..1 @x0、b 1..2 @x10。
    let before = snapshot(vec![line(vec![cluster(0, 1, 0.0), cluster(1, 2, 10.0)])]);
    // `abX`：b 的 range 没变、也没被 inserted overlap，但 shaping identity 变了，
    // 而且整块挪到了 x=40 —— 这正是「old x=10 / new x=40」的稳定反例。
    let after = snapshot(vec![line(vec![
        cluster(0, 1, 0.0),
        LineClusterSnapshot {
            shaping_identity: shaping_with(2),
            ..cluster_sized(1, 2, 40.0, 10.0)
        },
        cluster_sized(2, 3, 50.0, 10.0),
    ])]);
    coord.begin_or_extend_edit_frontier(insert_request(
        before.clone(),
        after.clone(),
        "ab",
        "abX",
        vec![(2, 3)],
        OffsetMap::from_single_edit(2, (2, 2), 1),
        now,
    ));

    let canonical_x = after.line_snapshots[0].clusters[1].source_rect.x;
    assert!(
        (canonical_x - 40.0).abs() < 1e-9,
        "前提：new cluster 的 canonical x 必须是 40，实际 {canonical_x}"
    );

    // 中段：new 侧必须已经待在自己的 canonical 位置，而不是朝 old 的 x=10 走。
    let mid = coord.shaping_transition_glyphs(instant_at(now, HALF_MS));
    assert_eq!(mid.len(), 1, "identity 变化必须进入交接层");
    assert_eq!(mid[0].new.len(), 1);
    let mid_x = mid[0].new[0].rect.x;
    assert!(
        (mid_x - 40.0).abs() < 1.0,
        "80ms 时 new 侧必须已经在 canonical x=40 附近，实际 {mid_x}：\
         朝 old 区域补间会让它越淡越往 x=10 跑"
    );

    // 临近结束：new 侧几乎全不透明，位置仍然必须是 canonical。
    let last = coord.shaping_transition_glyphs(instant_at(now, DURATION_MS - 1));
    assert_eq!(last.len(), 1);
    assert_eq!(last[0].new.len(), 1);
    let last_x = last[0].new[0].rect.x;
    let last_opacity = last[0].new[0].opacity;
    assert!(
        last_opacity > 0.9,
        "159ms 时 new 侧应当接近全不透明，实际 {last_opacity}"
    );
    assert!(
        (last_x - canonical_x).abs() < 1.0,
        "清层前最后一帧的 new 侧位置 {last_x} 必须等于 canonical {canonical_x}，\
         否则清层那一帧静态层会肉眼跳一次"
    );
    assert!(
        (last_x - 10.0).abs() > 10.0,
        "new 侧绝不能待在 old 的 x=10 附近，实际 {last_x}"
    );

    // 清层后静态 canonical 就在 x=40 —— 与最后一帧连续。
    let end = coord.shaping_transition_glyphs(instant_at(now, DURATION_MS + 1));
    for frame in &end {
        for side in &frame.new {
            assert!(
                (side.rect.x - canonical_x).abs() < 1.0,
                "清层前后 new 侧位置必须连续：{} vs canonical {}",
                side.rect.x,
                canonical_x
            );
        }
    }
}

// ── 评论 26 阻塞 2：source_rect 永远是 exact slice，不二次裁 ──────────────────

/// 阻塞 2：Conceal → Shaping 的 handoff 不得二次裁剪 UV。
///
/// 数字按评论原文：整块 source 100px、dest full 10px、当前只剩 2.5px
/// → exact slice = `100 * (2.5/10) = 25`。handoff 后 old side 的 source
/// 宽度必须仍是 **25**，再乘一次 `2.5/10` 得到的 6.25 就是二次裁剪。
#[test]
fn conceal_to_shaping_handoff_does_not_crop_source_twice() {
    // producer 侧的唯一算法：整块 source + 整字宽 + 已露宽度 → 精确 slice。
    let full = SourceRect {
        x: 3.0,
        y: 0.0,
        w: 100.0,
        h: 20.0,
    };
    let slice = visible_source_slice(&full, 10.0, 2.5);
    assert!(
        (slice.w - 25.0).abs() < 1e-9,
        "exact slice 必须是 25，实际 {}",
        slice.w
    );

    let now = Instant::now();
    let base = af_snapshot();
    let target = afi_ligated_snapshot();
    let handoff = CurrentVisualCluster {
        logical_range: (1, 2),
        visual_cluster_range: (1, 2),
        snapshot_id: base.line_snapshots[0].id,
        source_rect: slice,
        dest_rect: SourceRect {
            x: 10.0,
            y: 0.0,
            w: 10.0,
            h: 20.0,
        },
        opacity: 1.0,
        visible_clip: 2.5,
    };

    let state = ShapingTransitionState::build_or_retarget(
        None,
        &[handoff],
        &base,
        &target,
        &[],
        &[(2, 3)],
        &OffsetMap::from_single_edit(2, (2, 2), 1),
        now,
        DURATION_MS,
        String::from("afi"),
    );
    assert!(!state.is_empty(), "`f` 必须进入交接层");

    for probe_ms in [0u64, 40, 80, 120] {
        let frames = state.sample(instant_at(now, probe_ms));
        let old = &frames[0].old;
        assert_eq!(old.len(), 1, "old 侧只有 f 这一块");
        assert!(
            (old[0].source_rect.w - 25.0).abs() < 1e-9,
            "{probe_ms}ms 时 old 侧 source 宽度必须仍是 25（exact slice），实际 {}：\
             再乘 visible/rect.w 就是二次裁剪",
            old[0].source_rect.w
        );
        assert!(
            (old[0].rect.w - 2.5).abs() < 1e-9,
            "{probe_ms}ms 时 old 侧可见宽度仍是 2.5px，实际 {}",
            old[0].rect.w
        );
    }
}

/// 阻塞 2 的加长版：连续 retarget 两次，old 侧 UV 不能指数缩小。
///
/// 旧实现每接手一次就再乘一次 `visible / rect.w`，25 → 6.25 → 1.5625。
#[test]
fn repeated_shaping_retarget_does_not_shrink_old_uv_each_time() {
    let now = Instant::now();

    let full = SourceRect {
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 20.0,
    };
    let slice = visible_source_slice(&full, 10.0, 2.5);
    assert!((slice.w - 25.0).abs() < 1e-9);

    let base = af_snapshot();
    let target = afi_ligated_snapshot();
    let handoff = CurrentVisualCluster {
        logical_range: (1, 2),
        visual_cluster_range: (1, 2),
        snapshot_id: base.line_snapshots[0].id,
        source_rect: slice,
        dest_rect: SourceRect {
            x: 10.0,
            y: 0.0,
            w: 10.0,
            h: 20.0,
        },
        opacity: 1.0,
        visible_clip: 2.5,
    };

    // 第 1 笔：`af -> afi`。
    let mut state = ShapingTransitionState::build_or_retarget(
        None,
        &[handoff],
        &base,
        &target,
        &[],
        &[(2, 3)],
        &OffsetMap::from_single_edit(2, (2, 2), 1),
        now,
        DURATION_MS,
        String::from("afi"),
    );
    let old_source_of = |state: &ShapingTransitionState, cluster: (usize, usize)| -> Option<f64> {
        state
            .groups
            .iter()
            .flat_map(|group| &group.old_atoms)
            .find(|atom| atom.cluster == cluster)
            .map(|atom| atom.source_rect.w)
    };
    assert_eq!(
        old_source_of(&state, (1, 2)),
        Some(25.0),
        "第 1 笔之后 old `f` 的 source 仍是 25"
    );

    // 第 2、3 笔：各自只让那块 `fi` / `fij` 再变一次 shaping，
    // `f` 这一层必须一路 retarget 下来，source 不能被再裁。
    for round in 0..2u32 {
        let at = instant_at(now, 40);
        let visuals = state.current_visuals(at);
        let (old_snapshot, new_snapshot, target_text, inserted, map) = if round == 0 {
            (
                afi_ligated_snapshot(),
                afij_ligated_snapshot(),
                "afij",
                vec![(3, 4)],
                OffsetMap::from_single_edit(3, (3, 3), 1),
            )
        } else {
            (
                afij_ligated_snapshot(),
                snapshot(vec![line(vec![
                    cluster(0, 1, 0.0),
                    cluster_sized(1, 4, 10.0, 17.0),
                    cluster_sized(4, 5, 27.0, 9.0),
                ])]),
                "afijk",
                vec![(4, 5)],
                OffsetMap::from_single_edit(4, (4, 4), 1),
            )
        };
        state = ShapingTransitionState::build_or_retarget(
            Some(&state),
            &visuals,
            &old_snapshot,
            &new_snapshot,
            &[],
            &inserted,
            &map,
            at,
            DURATION_MS,
            String::from(target_text),
        );
        assert_eq!(
            old_source_of(&state, (1, 2)),
            Some(25.0),
            "第 {} 次 retarget 之后 old `f` 的 source 必须仍是 25，实际 {:?}：\
             每接手一次就再裁一次会让 UV 指数缩小",
            round + 2,
            old_source_of(&state, (1, 2))
        );
    }
}

// ── 评论 26 阻塞 3：1:N group 的下一笔必须能局部接管 ─────────────────────────

/// 阻塞 3：第一笔 `0..4 -> A + B` 跑到一半，第二笔只改 B。
///
/// 旧实现 `any absorbed => skip whole group`，于是：
///
/// - A 的上一帧半透明状态直接丢掉，canonical A 瞬间全亮；
/// - old O 也一起消失；
/// - 只有 B 被新 group 接住。
///
/// 现在必须按 atom 粒度拆：A 继续淡、old O 继续淡、只有 B 转入新 component。
#[test]
fn editing_one_child_of_one_to_many_transition_keeps_other_child_and_old_fade() {
    let now = Instant::now();
    let at = instant_at(now, HALF_MS / 2);
    let mut coord = LinuxEditorAnimationCoordinator::new();
    coord.set_typing_animation_duration_ms(DURATION_MS as u32);

    // 第 1 笔：old O = 0..4（40px 一块） -> new A = 0..1（10px）+ B = 1..3（30px）。
    let first_old = snapshot(vec![line(vec![cluster_sized(0, 4, 0.0, 40.0)])]);
    let first_new = snapshot(vec![line(vec![
        cluster_sized(0, 1, 0.0, 10.0),
        cluster_sized(1, 3, 10.0, 30.0),
    ])]);
    coord.begin_or_extend_edit_frontier(EditFrontierRequest {
        kind: EditorAnimationKind::Delete,
        base_snapshot: first_old,
        base_text: String::from("abcd"),
        target_snapshot: first_new.clone(),
        target_text: String::from("acd"),
        deleted_ranges: vec![(1, 2)],
        inserted_ranges: Vec::new(),
        offset_map: OffsetMap::from_single_edit(4, (1, 2), 0),
        conceal_direction: ConcealDirection::Forward,
        now,
    });

    let before = coord.shaping_transition_glyphs(at);
    assert_eq!(before.len(), 1, "第一笔只有一个 1:2 group");
    assert_eq!(before[0].old.len(), 1, "old O 在淡出");
    assert_eq!(before[0].new.len(), 2, "A 与 B 都在淡入");
    let a_before = before[0]
        .new
        .iter()
        .find(|side| (side.rect.w - 10.0).abs() < 1e-6)
        .expect("A 是那块 10px 的 new side")
        .clone();

    // 第 2 笔：只在 B 的中间插一个字，B 变成 mixed component `B -> C`。
    coord.begin_or_extend_edit_frontier(EditFrontierRequest {
        kind: EditorAnimationKind::Insert,
        base_snapshot: first_new,
        base_text: String::from("acd"),
        target_snapshot: snapshot(vec![line(vec![
            cluster_sized(0, 1, 0.0, 10.0),
            cluster_sized(1, 2, 10.0, 10.0),
            cluster_sized(2, 4, 20.0, 20.0),
        ])]),
        target_text: String::from("acZd"),
        deleted_ranges: Vec::new(),
        inserted_ranges: vec![(2, 3)],
        offset_map: OffsetMap::from_single_edit(3, (2, 2), 1),
        conceal_direction: ConcealDirection::Forward,
        now: at,
    });

    let shaping = coord
        .active_shaping_transition
        .as_ref()
        .expect("第二笔仍然有不可拆 cluster，交接层不能被清掉");
    assert_eq!(
        shaping.groups.len(),
        2,
        "被接管的 B 必须单独成组，A + old O 作为另一组继续淡"
    );

    let taken = shaping
        .groups
        .iter()
        .find(|group| group.new_atoms.iter().any(|atom| atom.cluster == (1, 2)))
        .expect("新 component 必须接住 B 的后代");
    assert_eq!(
        taken
            .old_atoms
            .iter()
            .map(|atom| atom.cluster)
            .collect::<Vec<_>>(),
        vec![(1, 3)],
        "只有 B 转入新 component"
    );

    let rest = shaping
        .groups
        .iter()
        .find(|group| group.new_atoms.iter().any(|atom| atom.cluster == (0, 1)))
        .expect("没被碰的兄弟 A 绝不能整组被跳过");
    assert!(
        rest.old_atoms.iter().any(|atom| atom.cluster == (0, 4)),
        "old O 仍在淡出，不能因为同组里 B 被接管就一起删掉"
    );

    // A 的像素事实必须连续：opacity / 位置 / 宽度都不能断。
    let after = coord.shaping_transition_glyphs(at);
    let rest_frame = after
        .iter()
        .find(|frame| {
            frame
                .old
                .iter()
                .any(|side| (side.rect.w - 40.0).abs() < 1e-6)
                && frame.new.len() == 1
        })
        .expect("rest group 这一帧必须同时画 old O 与 new A");
    let a_after = &rest_frame.new[0];
    assert!(
        (a_after.opacity - a_before.opacity).abs() < 1e-6,
        "A 的 opacity 断裂：{} -> {}",
        a_before.opacity,
        a_after.opacity
    );
    assert!(
        (a_after.rect.x - a_before.rect.x).abs() < 1e-6,
        "A 的位置断裂：{} -> {}",
        a_before.rect.x,
        a_after.rect.x
    );
    assert!(
        (a_after.rect.w - a_before.rect.w).abs() < 1e-6,
        "A 的宽度断裂：{} -> {}",
        a_before.rect.w,
        a_after.rect.w
    );
    assert!(
        a_after.opacity > 0.0 && a_after.opacity < 1.0,
        "A 仍在半透明淡入中，实际 {}",
        a_after.opacity
    );
    let old_o = rest_frame
        .old
        .iter()
        .find(|side| (side.rect.w - 40.0).abs() < 1e-6)
        .expect("old O 这一帧仍在画");
    assert!(
        old_o.opacity > 0.0,
        "old O 必须仍在淡出，实际 opacity = {}",
        old_o.opacity
    );
}

// ── 评论 26 阻塞 4：collect_components 不逐 byte 扫 ──────────────────────────

/// 结构回归：`OffsetMapEntry.length = 1_000_000`、snapshot 只有 3 个 cluster。
///
/// 逐 byte 实现会跑 100 万次；interval sweep 的探测量必须跟 cluster 数
/// 同量级。**不用 wall-clock** —— 计时在慢机器上会假绿。
#[test]
fn collect_components_probes_scale_with_cluster_count_not_text_length() {
    let make = || {
        snapshot(vec![line(vec![
            cluster(0, 1, 0.0),
            cluster(1, 2, 10.0),
            cluster(2, 3, 20.0),
        ])])
    };
    let old = make();
    let new = make();
    let map = OffsetMap {
        entries: vec![OffsetMapEntry {
            old_byte_offset: Utf8ByteOffset::unchecked(0),
            new_byte_offset: Utf8ByteOffset::unchecked(0),
            length: 1_000_000,
            kind: OffsetMapKind::Identity,
        }],
    };

    let old_side = ClusterIndex::build(&old);
    let new_side = ClusterIndex::build(&new);
    // 先把此前累计的探测量清掉，只测这一次调用。
    let _ = test_helpers::take_component_probe_count();
    let (components, pairs) = collect_components(&old_side, &new_side, &map);
    let probes = test_helpers::take_component_probe_count();

    assert_eq!(components.len(), 3, "3 个 cluster 应当连成 3 个分量");
    assert_eq!(pairs.len(), 3, "每个 old 各自配一个 new");
    assert!(
        probes <= 16,
        "探测量必须跟 cluster 数（3）同量级，实际 {probes}：\
         entry.length 是 1_000_000，逐 byte 扫会跑满一百万次"
    );
    assert!(probes >= 1, "总得真的做过区间探针");
}

/// 评论 27 阻塞 2：区间查询必须是 O(log N + k)，不能退化成 O(N²)。
///
/// old / new 各 10,000 个 cluster + 一条大 identity entry。上一版
/// `nodes_overlapping_interval` 每次都从 `sorted` 开头扫到 `first`，总访问量
/// `1+2+…+N ≈ 5000 万`；现在两端各二分一次，真正访问的只有实际相交的那批。
///
/// 计的是**实际检查的 new cluster 数**（`COMPONENT_CLUSTER_VISIT_COUNT`），
/// 不是 mapping-entry 探针 —— 只数探针会在 new 侧很大时假绿。
/// **不用 wall-clock**：计时在慢机器上会假绿。
#[test]
fn cluster_interval_query_visits_are_linear_not_quadratic() {
    const N: usize = 10_000;
    // 每个 cluster 10 字节：互不重叠、byte offset 全局递增（`byte_end` 也随之
    // 升序 —— 这正是区间二分下界依赖的不变量）。
    let make = || {
        let clusters: Vec<LineClusterSnapshot> = (0..N)
            .map(|i| cluster(i * 10, i * 10 + 10, (i as f64) * 10.0))
            .collect();
        snapshot(vec![line(clusters)])
    };
    let old = make();
    let new = make();
    let map = OffsetMap {
        entries: vec![OffsetMapEntry {
            old_byte_offset: Utf8ByteOffset::unchecked(0),
            new_byte_offset: Utf8ByteOffset::unchecked(0),
            length: 10 * N,
            kind: OffsetMapKind::Identity,
        }],
    };

    let old_side = ClusterIndex::build(&old);
    let new_side = ClusterIndex::build(&new);
    // 先清零，只统计这一次调用。
    let _ = test_helpers::take_component_cluster_visit_count();
    let _ = test_helpers::take_component_probe_count();
    let (components, pairs) = collect_components(&old_side, &new_side, &map);
    let visits = test_helpers::take_component_cluster_visit_count();
    let probes = test_helpers::take_component_probe_count();

    assert_eq!(components.len(), N, "identity 应当连成 N 个 1:1 分量");
    assert_eq!(pairs.len(), N, "每个 old 各自配一个 new");
    assert!(
        probes <= 4 * N,
        "mapping entry 探针也必须是 O(N)，实际 {probes}"
    );
    assert!(
        visits >= N,
        "每个 old cluster 至少要看一个 new cluster，实际 {visits}"
    );
    assert!(
        visits < 3 * N,
        "new cluster 访问量必须是 O(N) 量级（< 3N = {}），实际 {visits}：\
         从 sorted 开头重扫会跑成 1+2+…+N = {}",
        3 * N,
        N * (N + 1) / 2
    );
}

// ── 评论 27 阻塞 1 / 评论 28：视觉身份与 ownership ────────────────────────────

/// 评论 27/28 的 1:N 两笔 fixture 跑完后的产物。
struct OneToManySecondStroke {
    coord: LinuxEditorAnimationCoordinator,
    /// 第 1 笔 40ms 时 new A 的不透明度（第 2 笔 old 侧必须从它接手）。
    a_opacity: f64,
    /// 第 1 笔 40ms 时 old O 的不透明度（**不能**被当成接手值）。
    o_opacity: f64,
    first_old_id: LineSnapshotId,
    first_new_id: LineSnapshotId,
}

/// 第 1 笔 `old O 0..4 -> A 0..1 + B 1..3`，40ms 时第 2 笔只改 A。
///
/// 这个 fixture 的关键：old O 的 `primary_handoff_key()` 恰好是 `A_range`，
/// 所以同一帧里**历史 old O** 与**当前 new A** 的 `logical_range` 完全相同，
/// 只有视觉身份能区分它们。
fn one_to_many_then_edit_first_child() -> OneToManySecondStroke {
    let now = Instant::now();
    let at = instant_at(now, HALF_MS / 2);
    let mut coord = LinuxEditorAnimationCoordinator::new();
    coord.set_typing_animation_duration_ms(DURATION_MS as u32);

    // 第 1 笔：old O 0..4（40px 一块） -> new A 0..1（10px）+ B 1..3（30px）。
    let first_old = snapshot(vec![line(vec![cluster_sized(0, 4, 0.0, 40.0)])]);
    let first_old_id = first_old.line_snapshots[0].id;
    // target 必须是一份**独立的**行纹理：`line()` 恒用 `visual_line_id = 0`，
    // 两笔之间不换 id 的话视觉身份就只剩 `visual_cluster_range` 一半。
    let first_new = snapshot(vec![PreparedLineSnapshot::stub_for_tests(
        1,
        0.0,
        0,
        vec![
            cluster_sized(0, 1, 0.0, 10.0),
            cluster_sized(1, 3, 10.0, 30.0),
        ],
    )]);
    let first_new_id = first_new.line_snapshots[0].id;
    coord.begin_or_extend_edit_frontier(EditFrontierRequest {
        kind: EditorAnimationKind::Delete,
        base_snapshot: first_old,
        base_text: String::from("abcd"),
        target_snapshot: first_new.clone(),
        target_text: String::from("acd"),
        deleted_ranges: vec![(1, 2)],
        inserted_ranges: Vec::new(),
        offset_map: OffsetMap::from_single_edit(4, (1, 2), 0),
        conceal_direction: ConcealDirection::Forward,
        now,
    });

    // 前提必须显式成立，否则这些测试咬不住问题：
    // old O 的 primary handoff key 就是 A 的 range —— 两条 current_visual 的
    // logical_range 完全相同，只有视觉身份能区分它们。
    let first_state = coord
        .active_shaping_transition
        .as_ref()
        .expect("1:2 拆分必须进入交接层");
    let first_group = &first_state.groups[0];
    let o_atom = &first_group.old_atoms[0];
    let a_atom = first_group
        .new_atoms
        .iter()
        .find(|atom| atom.cluster == (0, 1))
        .expect("A = 0..1 必须在 new 侧");
    assert_eq!(
        o_atom.primary_handoff_key(),
        a_atom.cluster,
        "前提：old O 与 new A 共享同一个 logical_range = (0,1)"
    );
    assert_ne!(
        o_atom.snapshot_id, a_atom.snapshot_id,
        "前提：两份像素来自完全不同的行纹理"
    );
    assert_ne!(
        o_atom.visual_cluster_range, a_atom.visual_cluster_range,
        "前提：视觉身份必须不同（(0,4) vs (0,1)）"
    );

    let before = coord.shaping_transition_glyphs(at);
    assert_eq!(before.len(), 1);
    let a_opacity = before[0]
        .new
        .iter()
        .find(|side| (side.rect.w - 10.0).abs() < 1e-6)
        .expect("A 是那块 10px 的 new side")
        .opacity;
    let o_opacity = before[0].old[0].opacity;

    // 第 2 笔：只改 A —— 在 a 后插一个字，新 shaping 把 `aZ` 合成一块 0..2，
    // A 的 cluster 边界因此变了（`0..1 -> 0..2`），走 mixed/boundary 判据。
    coord.begin_or_extend_edit_frontier(EditFrontierRequest {
        kind: EditorAnimationKind::Insert,
        base_snapshot: first_new,
        base_text: String::from("acd"),
        target_snapshot: snapshot(vec![PreparedLineSnapshot::stub_for_tests(
            2,
            0.0,
            0,
            vec![
                cluster_sized(0, 2, 0.0, 20.0),
                cluster_sized(2, 4, 20.0, 20.0),
            ],
        )]),
        target_text: String::from("aZcd"),
        deleted_ranges: Vec::new(),
        inserted_ranges: vec![(1, 2)],
        offset_map: OffsetMap::from_single_edit(3, (1, 1), 1),
        conceal_direction: ConcealDirection::Forward,
        now: at,
    });

    OneToManySecondStroke {
        coord,
        a_opacity,
        o_opacity,
        first_old_id,
        first_new_id,
    }
}

/// 汇总**所有** shaping group 的 atom，按 `(snapshot_id, visual_cluster_range)`
/// 查重 —— 一份视觉像素一帧只能有一个 owner。
///
/// `one_visual_cluster_has_exactly_one_animation_owner` 那种「label 相同就跳过」
/// 的判据抓不到**ShapingTransition 内部两个 group 重复拥有同一份 old O**：
/// old/new cross-fade 正常情况下 snapshot / revision 或 visual cluster 本来就
/// 不同，只有视觉 key 完全相同还出现两次，才是实打实的重复画。
fn assert_no_duplicate_visual_atom(shaping: &ShapingTransitionState) {
    let mut seen: Vec<((LineSnapshotId, (usize, usize)), String)> = Vec::new();
    for (index, group) in shaping.groups.iter().enumerate() {
        for (side, atoms) in [("old", &group.old_atoms), ("new", &group.new_atoms)] {
            for atom in atoms {
                let key = (atom.snapshot_id, atom.visual_cluster_range);
                if let Some((_, previous)) = seen.iter().find(|(seen_key, _)| *seen_key == key) {
                    panic!(
                        "同一份视觉像素 (snapshot={:?}, cluster={:?}) 被两个 group 同时拥有：\
                         {} 与 groups[{index}].{side} —— render 会把同一帧画两遍",
                        key.0, key.1, previous
                    );
                }
                seen.push((key, format!("groups[{index}].{side}")));
            }
        }
    }
}

/// old O 与 new A 在同一帧里共享 `logical_range = A_range`，却来自两张完全
/// 不同的行纹理。第二笔只改 A 时：
///
/// - 新 component 的 old 侧只能是**正在淡入的 A**；
/// - 历史 old O 只能留在 previous rest group 里继续和 B 一起淡出。
///
/// 评论 27 修的是「old 侧抓错成 O」；评论 28 修的是「抓对了 A 之后 leftover
/// 又把 O 也吸进来」—— 那会让 O 同帧属于两个 group，违反「一份像素一个 owner」。
#[test]
fn editing_first_child_of_one_to_many_transition_uses_current_new_child_as_handoff() {
    let stroke = one_to_many_then_edit_first_child();
    let coord = stroke.coord;
    let shaping = coord
        .active_shaping_transition
        .as_ref()
        .expect("第二笔仍然有不可拆 cluster");
    assert_eq!(shaping.groups.len(), 2, "A 单独成组，old O + B 作为另一组");

    let taken = shaping
        .groups
        .iter()
        .find(|group| group.new_atoms.iter().any(|atom| atom.cluster == (0, 2)))
        .expect("A 的新 component 必须存在");
    assert_eq!(
        taken
            .old_atoms
            .iter()
            .map(|atom| atom.cluster)
            .collect::<Vec<_>>(),
        vec![(0, 1)],
        "评论 28：新 component 的 old 侧只能是它自己的 old cluster A，\
         历史 old O 不许被 logical overlap 顺手吸进来"
    );

    let primary = &taken.old_atoms[0];
    assert_eq!(
        primary.snapshot_id, stroke.first_new_id,
        "source 必须是 previous target 的 A"
    );
    assert_ne!(
        primary.snapshot_id, stroke.first_old_id,
        "绝不能从历史 old O 的纹理接手"
    );
    assert_eq!(
        primary.visual_cluster_range,
        (0, 1),
        "视觉身份也必须是 A 那块 cluster"
    );
    assert!(
        (primary.source_rect.w - 10.0).abs() < 1e-6,
        "source 必须是 A 的 10px 资源，实际 {}",
        primary.source_rect.w
    );
    assert!(
        (primary.start_rect.w - 10.0).abs() < 1e-6,
        "起步矩形必须是 A 的 10px，实际 {}",
        primary.start_rect.w
    );
    assert!(
        (primary.start_opacity - stroke.a_opacity).abs() < 1e-6,
        "start_opacity 必须等于上一帧 new A 的 {}，实际 {}",
        stroke.a_opacity,
        primary.start_opacity
    );
    assert!(
        (primary.start_opacity - stroke.o_opacity).abs() > 1e-6,
        "不能等于 old O 的 opacity {}",
        stroke.o_opacity
    );

    // 历史 old O 由 previous group 的拆分继续维护，只在 rest 这一组里。
    let rest = shaping
        .groups
        .iter()
        .find(|group| group.new_atoms.iter().any(|atom| atom.cluster == (2, 4)))
        .expect("B 必须继续淡入");
    assert_eq!(
        rest.old_atoms
            .iter()
            .map(|atom| atom.cluster)
            .collect::<Vec<_>>(),
        vec![(0, 4)],
        "old O 只留在 previous rest 组里淡出，不能整组被跳过"
    );
    let old_o = &rest.old_atoms[0];
    assert_eq!(
        old_o.snapshot_id, stroke.first_old_id,
        "rest 的 old 侧就是历史 old O"
    );
    assert_eq!(
        old_o.visual_cluster_range,
        (0, 4),
        "old O 的视觉身份必须是它自己那块 0..4"
    );
    assert!(
        (old_o.rect.w - 40.0).abs() < 1e-6,
        "old O 必须保持自己的 40px 几何，实际 {}",
        old_o.rect.w
    );
    assert!(
        (old_o.source_rect.w - 40.0).abs() < 1e-6,
        "old O 的 source 仍是它自己的 40px，实际 {}",
        old_o.source_rect.w
    );

    // old O 的 `(snapshot_id, visual_cluster_range)` 在所有 groups 里只能出现一次。
    assert_eq!(
        shaping
            .groups
            .iter()
            .flat_map(|group| group.old_atoms.iter())
            .filter(|atom| {
                atom.snapshot_id == stroke.first_old_id && atom.visual_cluster_range == (0, 4)
            })
            .count(),
        1,
        "历史 old O 在所有 groups 里只能出现一次"
    );
    assert_no_duplicate_visual_atom(shaping);
}

/// 评论 28：同一份视觉像素一帧只能有一个 owner —— 同一个
/// `(snapshot_id, visual_cluster_range)` 绝不能出现在两个 group 里。
#[test]
fn one_shaping_visual_atom_cannot_exist_in_two_groups() {
    let stroke = one_to_many_then_edit_first_child();
    let shaping = stroke
        .coord
        .active_shaping_transition
        .as_ref()
        .expect("第二笔仍然有不可拆 cluster");

    // 场景本身必须成立，否则这条测试退化成空转。
    assert_eq!(
        shaping
            .groups
            .iter()
            .flat_map(|group| group.old_atoms.iter())
            .filter(|atom| atom.snapshot_id == stroke.first_old_id)
            .count(),
        1,
        "历史 old O 必须且只能属于一个 group"
    );
    assert_eq!(
        shaping
            .groups
            .iter()
            .flat_map(|group| group.old_atoms.iter())
            .filter(|atom| atom.snapshot_id == stroke.first_new_id)
            .count(),
        1,
        "当前 new A 必须且只能属于一个 group"
    );

    assert_no_duplicate_visual_atom(shaping);
}
