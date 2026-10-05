//! Issue #826 评论 24：不可拆 shaping cluster 的原子视觉交接 —— 回归测试。
//!
//! 三条测试咬的是同一条底层规则：
//!
//! **Core 的 range 可以按字符切；Qt 的视觉 owner 绝不能切开 shaping cluster。**
//!
//! 所有 cluster 都是 synthetic 的（`fi` 直接构造成 byte range `1..3` 的一块
//! cluster），不依赖系统字体是否真的启用 fi ligature —— 否则这台机器上关掉
//! ligature 就会让测试假绿。

use std::time::{Duration, Instant};

use writer_core::editor::OffsetMap;

use crate::editor::layout::{CaretAffinity, LayoutSnapshot};
use crate::sujian_editor_item::animation::coordinator::EditFrontierRequest;
use crate::sujian_editor_item::animation::coordinator::LinuxEditorAnimationCoordinator;
use crate::sujian_editor_item::animation::edit_frontier::ConcealDirection;
use crate::sujian_editor_item::edit_motion::EditorAnimationKind;
use crate::sujian_editor_item::layout_snapshot::{
    EditorLayoutSnapshot, LineClusterSnapshot, PreparedLineSnapshot, ShapingIdentity, SourceRect,
};

const DURATION_MS: u64 = 160;
const HALF_MS: u64 = 80;

fn instant_at(base: Instant, ms: u64) -> Instant {
    base + Duration::from_millis(ms)
}

fn shaping() -> ShapingIdentity {
    ShapingIdentity {
        text_content_hash: 1,
        raw_font_fingerprint: String::from("test-font"),
        glyph_indexes_hash: 1,
        cluster_glyph_count: 1,
        direction_rtl: false,
        format_fingerprint: 1,
    }
}

/// 一个 cluster，宽度固定 10px（`stub_for_tests` 用最小 x 当 `visual_x`，
/// 所以文档 x 要靠行内放一个 `x = 0` 的 cluster 钉住偏移）。
fn cluster(byte_start: usize, byte_end: usize, x: f64) -> LineClusterSnapshot {
    LineClusterSnapshot {
        byte_start,
        byte_end,
        source_rect: SourceRect {
            x,
            y: 0.0,
            w: 10.0,
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

/// `af`：两个单字符 cluster —— `a` 0..1 @x0，`f` 1..2 @x10。
fn af_snapshot() -> EditorLayoutSnapshot {
    snapshot(vec![PreparedLineSnapshot::stub_for_tests(
        0,
        0.0,
        0,
        vec![cluster(0, 1, 0.0), cluster(1, 2, 10.0)],
    )])
}

/// `afi`：**合成后**的正文 —— `a` 0..1 @x0，`fi` 1..3 @x10。
///
/// 注意最新正文里 `f` 与 `i` 已经是一块 16px 宽的视觉 cluster，不是两块。
/// 这正是评论 24 的核心事实：Core 只知道「在 `f` 后面插了一个 `i`」，
/// Qt 却已经把三个字形合成了一块资源。
fn afi_ligated_snapshot() -> EditorLayoutSnapshot {
    snapshot(vec![PreparedLineSnapshot::stub_for_tests(
        0,
        0.0,
        0,
        vec![cluster(0, 1, 0.0), cluster(1, 3, 10.0)],
    )])
}

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
    coord.begin_or_extend_edit_frontier(EditFrontierRequest {
        kind: EditorAnimationKind::Insert,
        base_snapshot: snapshot(vec![PreparedLineSnapshot::stub_for_tests(
            0,
            0.0,
            0,
            vec![cluster(0, 1, 0.0)],
        )]),
        target_snapshot: af_snapshot(),
        deleted_ranges: Vec::new(),
        inserted_ranges: vec![(1, 2)],
        offset_map: OffsetMap::from_single_edit(1, (1, 1), 1),
        base_text: String::from("a"),
        target_text: String::from("af"),
        conceal_direction: ConcealDirection::Forward,
        now,
    });
    // 80ms：scalar reveal 走了 8.75px（ease_out_cubic(0.5) = 0.875）。
    let pending_before = coord.active_reveal_pending_ranges_for_test(half);
    assert_eq!(
        pending_before,
        vec![(1, 2)],
        "第一笔 80ms 时 f 还没吐完，仍归 scalar reveal"
    );

    // 第二笔：在 `f` 后面插 `i`。Core 逻辑上 f 仍映射 1..2、新 i 是 2..3，
    // 但最新正文里只有一块 1..3 的 fi cluster。
    coord.begin_or_extend_edit_frontier(EditFrontierRequest {
        kind: EditorAnimationKind::Insert,
        base_snapshot: af_snapshot(),
        target_snapshot: afi_ligated_snapshot(),
        deleted_ranges: Vec::new(),
        inserted_ranges: vec![(2, 3)],
        offset_map: OffsetMap::from_single_edit(2, (2, 2), 1),
        base_text: String::from("af"),
        target_text: String::from("afi"),
        conceal_direction: ConcealDirection::Forward,
        now: half,
    });

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
        target_snapshot: snapshot(vec![PreparedLineSnapshot::stub_for_tests(
            0,
            0.0,
            0,
            vec![cluster(0, 1, 0.0), cluster(1, 2, 10.0)],
        )]),
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

/// 每块视觉 cluster 每帧最多一个动画 owner。
///
/// 把 Frontier scalar、Reveal carry、Reflow move、Conceal、ShapingTransition
/// 当前这一帧的 owner 全部汇总，按 (snapshot revision, line id, cluster range)
/// 查重。反例 A 的坏结果在这里会被直接抓到：`fi 1..3` 同时出现在
/// `carry` 和 `scalar reveal` 里。
#[test]
fn one_visual_cluster_has_exactly_one_animation_owner() {
    let now = Instant::now();
    let half = instant_at(now, HALF_MS);
    let mut coord = LinuxEditorAnimationCoordinator::new();
    coord.set_typing_animation_duration_ms(DURATION_MS as u32);

    // `ab` -> 在 `b` 前面插 `X`（`aXb`）：`X` 走 scalar reveal，`b` 被推后走 Reflow。
    let ab = snapshot(vec![PreparedLineSnapshot::stub_for_tests(
        0,
        0.0,
        0,
        vec![cluster(0, 1, 0.0), cluster(1, 2, 10.0)],
    )]);
    let axb = snapshot(vec![PreparedLineSnapshot::stub_for_tests(
        0,
        0.0,
        0,
        vec![cluster(0, 1, 0.0), cluster(1, 2, 10.0), cluster(2, 3, 20.0)],
    )]);
    coord.begin_or_extend_edit_frontier(EditFrontierRequest {
        kind: EditorAnimationKind::Insert,
        base_snapshot: ab.clone(),
        target_snapshot: axb.clone(),
        deleted_ranges: Vec::new(),
        inserted_ranges: vec![(1, 2)],
        offset_map: OffsetMap::from_single_edit(2, (1, 1), 1),
        base_text: String::from("ab"),
        target_text: String::from("aXb"),
        conceal_direction: ConcealDirection::Forward,
        now,
    });

    // `aXb` -> 在 `X` 后面插 `i`；最新 shaping 把 `Xi` 合成一块 1..3。
    let axb_ligated = snapshot(vec![PreparedLineSnapshot::stub_for_tests(
        0,
        0.0,
        0,
        vec![cluster(0, 1, 0.0), cluster(1, 3, 10.0), cluster(3, 4, 30.0)],
    )]);
    coord.begin_or_extend_edit_frontier(EditFrontierRequest {
        kind: EditorAnimationKind::Insert,
        base_snapshot: axb,
        target_snapshot: axb_ligated,
        deleted_ranges: Vec::new(),
        inserted_ranges: vec![(2, 3)],
        offset_map: OffsetMap::from_single_edit(3, (2, 2), 1),
        base_text: String::from("aXb"),
        target_text: String::from("aXib"),
        conceal_direction: ConcealDirection::Forward,
        now: half,
    });

    // ── 汇总本帧的全部视觉 owner ──────────────────────────────────────
    //
    // 每层各自报出「这一帧我正在控制的 byte range」。凡是两层报了同一块
    // range，就是同一块视觉 cluster 有了第二个 owner。
    let mut owners: Vec<(&'static str, (usize, usize))> = Vec::new();

    if let Some(frontier) = coord.active_edit_frontier.as_ref() {
        for region in &frontier.reveal.regions {
            owners.push(("scalar_reveal", region.range));
        }
        for carried in &frontier.reveal_carried {
            owners.push(("reveal_carry", carried.range));
        }
        for region in &frontier.conceal.regions {
            owners.push(("conceal", region.range));
        }
    }
    for range in coord.active_reflow_new_ranges() {
        owners.push(("reflow", range));
    }
    if let Some(shaping) = coord.active_shaping_transition.as_ref() {
        for (_old_cluster, new_cluster) in shaping.owned_clusters_for_test() {
            owners.push(("shaping_transition", new_cluster));
        }
    }

    // 同名 range 出现两次就是双 owner。
    for (index, (label, range)) in owners.iter().enumerate() {
        for (other_label, other_range) in owners.iter().skip(index + 1) {
            assert!(
                range != other_range,
                "byte range {range:?} 同时被 `{label}` 与 `{other_label}` 控制：\
                 同一块视觉 cluster 有两个动画 owner 就是重影/形状跳变"
            );
        }
    }

    // 反例 A 的具体后果：`Xi` 1..3 这块合成 cluster 的 owner 必须是且只能是
    // 交接层。它一旦落进 scalar reveal 或 carry，就会被第二层同时控制。
    let mixed_owners: Vec<&str> = owners
        .iter()
        .filter(|(_, range)| *range == (1, 3))
        .map(|(label, _)| *label)
        .collect();
    assert_eq!(
        mixed_owners,
        vec!["shaping_transition"],
        "Xi 1..3 只能由交接层拥有"
    );
}
