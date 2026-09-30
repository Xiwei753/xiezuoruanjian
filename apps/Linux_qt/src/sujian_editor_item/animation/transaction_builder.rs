use writer_core::editor::OffsetMap;

// 仅 `process_transaction` 的单元测试需要下列导入：生产路径已改为
// `build_prepared_transaction` 直接构造、`rebind_timed_units_to_canonical` 重绑，
// 因此这些名字在非测试构建里没有使用者，必须用 `#[cfg(test)]` 收起来，
// 否则 `--bin sujian-linux-qt` 的 clippy（-D warnings）会报 unused import。
#[cfg(test)]
use std::time::Instant;

use super::coordinator::LinuxEditorAnimationCoordinator;
#[cfg(test)]
use crate::editor::layout::compute_affected_paragraph_ranges;
use crate::sujian_editor_item::animated_slice::{AnimatedSlice, AnimatedSliceKind};
use crate::sujian_editor_item::animation::cursor_motion::build_cursor_visual_track;
use crate::sujian_editor_item::animation::rebase::{
    match_rebase_frames, PreparedRebaseHandoff, RebaseCaretHandoff,
};
use crate::sujian_editor_item::animation::{
    PreparedTextVisualTransaction, PreparedVisualUnit, RebaseFrame, TextVisualOperationKind,
    TextVisualTransactionState, TransactionTimeline,
};
#[cfg(test)]
use crate::sujian_editor_item::animation_mode::AnimationMode;
#[cfg(test)]
use crate::sujian_editor_item::edit_motion::{diff_plain_text, EditorAnimationKind};
use crate::sujian_editor_item::edit_motion::{CursorRect, PreparedEditMotion};
use crate::sujian_editor_item::editor_animation_debug_log;
use crate::sujian_editor_item::layout_revision::LayoutRevision;
use crate::sujian_editor_item::layout_snapshot::{
    ClusterInsertRelation, EditorLayoutSnapshot, SourceRect,
};
use crate::sujian_editor_item::transaction_key::VisualTransactionKey;

pub(crate) fn operation_kind_label(kind: TextVisualOperationKind) -> &'static str {
    match kind {
        TextVisualOperationKind::Insert => "Insert",
        TextVisualOperationKind::Delete => "Delete",
        TextVisualOperationKind::CompositionUpdate => "CompositionUpdate",
        TextVisualOperationKind::CompositionCommitOrCancel => "CompositionCommitOrCancel",
    }
}

pub(crate) fn unit_kind_labels(units: &[PreparedVisualUnit]) -> Vec<String> {
    units
        .iter()
        .map(|u| format!("{:?}", u.slice.kind))
        .collect()
}

pub(crate) fn emit_transaction_diagnostic(
    tx: &PreparedTextVisualTransaction,
    event: &str,
    reason: &str,
) {
    crate::sujian_editor_item::editor_animation_diagnostic_event(
        event,
        &tx.key,
        operation_kind_label(tx.operation_kind),
        tx.old_cursor_rect.as_ref().map(|r| (r.x, r.top)),
        tx.new_cursor_rect.as_ref().map(|r| (r.x, r.top)),
        &unit_kind_labels(&tx.units).join(","),
        tx.timeline.first_render_wall_ms,
        reason,
    );
}

/// Issue #747 评论 5813540976: Composition commit 特殊 crossfade 规格（preedit→candidate 形变动画）。
/// 仅在 operation_kind == CompositionCommitOrCancel 且 is_commit 且 !visual_text_unchanged 时有值。
pub(crate) struct CompositionCommitCrossfadeSpec {
    pub(crate) preedit_byte_start: usize,
    pub(crate) preedit_byte_end: usize,
    pub(crate) candidate_byte_start: usize,
    pub(crate) candidate_byte_end: usize,
}

/// Issue #747 评论 5805324575 / 5813540976: 统一视觉事务构造的「归一化编辑事件」。
///
/// 所有编辑来源（普通 Insert/Delete、IME composition update/commit）先把原始编辑
/// 状态归一化成 `VisualEditSpec`，再交给 [`build_prepared_transaction`] 这唯一一处
/// 创建 `PreparedTextVisualTransaction`。`composition.rs` 只负责把 IME 状态归一化成
/// `VisualEditSpec` 并调用同一个事务构造器，不再维护第二套事务创建算法。
///
/// Issue #747 评论 5813540976: spec 只携带归一化输入（`offset_map`、cursor line info、
/// `text_animation_enabled` / `caret_animation_enabled`、`composition_commit_crossfade`），
/// 不再携带 `units` 与 `cursor_visual_track`——它们是 builder 的输出，
/// 由 [`build_prepared_transaction`] 内部统一构造。
pub(crate) struct VisualEditSpec {
    pub(crate) key: VisualTransactionKey,
    pub(crate) operation_kind: TextVisualOperationKind,
    pub(crate) old_snapshot: EditorLayoutSnapshot,
    pub(crate) new_snapshot: EditorLayoutSnapshot,
    pub(crate) inserted_ranges: Vec<(usize, usize)>,
    pub(crate) deleted_ranges: Vec<(usize, usize)>,
    pub(crate) offset_map: OffsetMap,
    pub(crate) old_cursor_rect: Option<CursorRect>,
    pub(crate) new_cursor_rect: Option<CursorRect>,
    pub(crate) old_cursor_visual_line_id: Option<usize>,
    pub(crate) new_cursor_visual_line_id: Option<usize>,
    pub(crate) old_cursor_line_top: f64,
    pub(crate) old_cursor_line_bottom: f64,
    pub(crate) new_cursor_line_top: f64,
    pub(crate) new_cursor_line_bottom: f64,
    pub(crate) cursor_owner_epoch: u64,
    pub(crate) layout_basis_revision: LayoutRevision,
    pub(crate) rebase_frames: Vec<RebaseFrame>,
    pub(crate) caret_handoff: Option<RebaseCaretHandoff>,
    pub(crate) visual_affected_byte_range_old: Option<(usize, usize)>,
    pub(crate) visual_affected_byte_range_new: Option<(usize, usize)>,
    /// Issue #756 评论 5821042551: 文字 unit（InsertReveal/DeleteConceal/Reflow）的时长。
    /// Issue #808: 文字始终拥有独立 timeline，协同时也不再共享。
    pub(crate) text_duration_ms: u64,
    /// Issue #756 评论 5821042551: cursor visual track 的时长。
    /// Issue #808: 光标始终拥有独立 timeline，协同时也不再共享。
    pub(crate) caret_duration_ms: u64,
    /// Issue #756: 文字动画开关（ReflowMove/ReflowCrossFade + InsertReveal/DeleteConceal）。
    /// coordinated=true 或 typing_animation_enabled=true 时为 true。
    pub(crate) text_animation_enabled: bool,
    /// Issue #756: 光标动画开关（caret motion track）。
    /// coordinated=true 或 smooth_cursor_enabled=true 时为 true。
    pub(crate) caret_animation_enabled: bool,
    /// Issue #756: 协同动画显式模式。决定吞吐字（InsertReveal/DeleteConceal）的遮罩
    /// 锚点是否取自 caret 位置。Issue #808 后协同不再把文字与光标绑死：
    /// - coordinated=true：吞吐字遮罩从 caret 位置展开/收拢（视觉上从光标处吐出/被光标吞进），
    ///   但文字动画按自己的 timeline + easing 推进，不消费 caret frame。
    /// - coordinated=false：吞吐字用 typing timeline 自己推进，遮罩锚点取默认值。
    /// 三种语义彻底分开：文字动画、光标动画、协同动画（遮罩锚点选择）。
    pub(crate) coordinated_animation_enabled: bool,
    pub(crate) composition_commit_crossfade: Option<CompositionCommitCrossfadeSpec>,
}

/// Issue #747 评论 5813540976: 全仓库唯一创建 `PreparedTextVisualTransaction` 的完整入口。
///
/// 接收归一化后的 [`VisualEditSpec`]，内部统一完成 slice 构造、unit wrap、rebase 匹配、
/// cursor track 构建、timeline 初始化。其它模块（含 `composition.rs` 与普通 Insert/Delete
/// 路径）都经由本函数创建事务，从而保证「只允许这里创建 `PreparedTextVisualTransaction`」。
pub(crate) fn build_prepared_transaction(spec: VisualEditSpec) -> PreparedTextVisualTransaction {
    let mut slices: Vec<AnimatedSlice> = Vec::new();

    // 1a. InsertReveal / DeleteConceal（文字动画）
    //
    // Issue #756: 吞吐字是否存在由 text_animation_enabled 决定（coordinated || typing）。
    // Issue #808: 吞吐字始终用 Timed timing（独立 timeline + 自己的 easing）。
    // - coordinated=true：遮罩锚点取 caret 位置（视觉上从光标处吐出/被光标吞进），
    //   但文字 progress 不消费 caret frame，文字与光标不再绑死。
    // - coordinated=false：遮罩锚点取默认值，文字用 typing timeline 自己推进。
    // 三种语义彻底分开：文字动画（timeline+easing）、光标动画（timeline+easing）、
    // 协同动画（遮罩锚点选择）。
    if spec.text_animation_enabled {
        for &(i_start, i_end) in &spec.inserted_ranges {
            slices.extend(build_insert_reveal_slices(
                spec.key,
                &spec.new_snapshot,
                (i_start, i_end),
                // Issue #808: 传旧 caret 作为吐字遮罩锚点。
                spec.old_cursor_rect.as_ref(),
                // Issue #808 评论 5916391891 修改 4: coordinated 决定是否用 caret 锚点做遮罩。
                spec.coordinated_animation_enabled,
                spec.old_cursor_visual_line_id,
            ));
        }
        for &(d_start, d_end) in &spec.deleted_ranges {
            slices.extend(build_delete_conceal_slices(
                spec.key,
                &spec.old_snapshot,
                (d_start, d_end),
                spec.old_cursor_rect.as_ref(),
                // Issue #808: 传新 caret 作为吞字遮罩锚点。
                spec.new_cursor_rect.as_ref(),
                spec.coordinated_animation_enabled,
                spec.new_cursor_visual_line_id,
            ));
        }
    }

    // 1b. Composition commit 特殊 crossfade（preedit→candidate 形变）
    //
    // Issue #756 评论 5821793349: crossfade 文字 unit（DeleteConceal/InsertReveal/
    // ReflowCrossFade/ReflowMove）也受 text_animation_enabled 控制。
    // 非协同 smooth-only commit（coordinated=false + typing=false + smooth=true）只保留
    // cursor_visual_track，tx.units 必须为空，不再生成文字动画。
    // typing-only / typing+smooth / coordinated 模式继续保留 composition crossfade 文字动画。
    if spec.text_animation_enabled {
        if let Some(crossfade) = &spec.composition_commit_crossfade {
            slices.extend(build_composition_commit_crossfade_slices(
                spec.key,
                &spec.old_snapshot,
                &spec.new_snapshot,
                &spec.offset_map,
                crossfade.preedit_byte_start,
                crossfade.preedit_byte_end,
                crossfade.candidate_byte_start,
                crossfade.candidate_byte_end,
                spec.old_cursor_rect.as_ref(),
                spec.new_cursor_rect.as_ref(),
            ));
        }
    }

    // 1c. Reflow（unchanged material）
    //
    // Issue #756: Reflow 是文字动画的一部分，由 text_animation_enabled 决定
    //（coordinated=true 或 typing_animation_enabled=true）。typing 关闭且非协同时
    // 只有光标动画，不生成文字 unit。
    let mut excluded_old: Vec<(usize, usize)> = spec.deleted_ranges.clone();
    let mut excluded_new: Vec<(usize, usize)> = spec.inserted_ranges.clone();
    if let Some(crossfade) = &spec.composition_commit_crossfade {
        excluded_old.push((crossfade.preedit_byte_start, crossfade.preedit_byte_end));
        excluded_new.push((crossfade.candidate_byte_start, crossfade.candidate_byte_end));
    }
    if spec.text_animation_enabled {
        slices.extend(build_cluster_reflow_slices(
            spec.key,
            &spec.old_snapshot,
            &spec.new_snapshot,
            &spec.offset_map,
            &excluded_old,
            &excluded_new,
            spec.old_cursor_rect.as_ref(),
            spec.new_cursor_rect.as_ref(),
            spec.visual_affected_byte_range_old,
            spec.visual_affected_byte_range_new,
        ));
    }

    // 2. Wrap units
    //
    // Issue #756: InsertReveal/DeleteConceal 的 timing 由 coordinated_animation_enabled
    // 决定（coordinated=true → CaretDriven，coordinated=false → Timed）。
    // ReflowMove/ReflowCrossFade 永远 Timed，与 coordinated 无关。
    let mut units: Vec<PreparedVisualUnit> = slices
        .into_iter()
        .map(|s| match s.kind {
            AnimatedSliceKind::InsertReveal | AnimatedSliceKind::DeleteConceal => {
                PreparedVisualUnit::wrap_with_coordinated(
                    s,
                    spec.text_duration_ms,
                    spec.coordinated_animation_enabled,
                )
            }
            AnimatedSliceKind::ReflowMove | AnimatedSliceKind::ReflowCrossFade => {
                PreparedVisualUnit::wrap(s, spec.text_duration_ms)
            }
        })
        .collect();

    // 3. Rebase frame 匹配
    match_rebase_frames(&spec.rebase_frames, &mut units, &spec.offset_map);

    // 4. Cursor visual track
    //
    // Issue #756: caret motion track 就是正文编辑期间的光标动画，由
    // caret_animation_enabled 决定（coordinated=true 或 smooth_cursor_enabled=true）。
    // 关闭时本事务不拥有 caret motion，光标位置由 canonical caret 接管（Snap），
    // 不会在用户关掉"平滑光标"后仍然沿 track 滑动。
    let cursor_visual_track = if spec.caret_animation_enabled {
        build_cursor_visual_track(
            spec.old_cursor_rect.as_ref(),
            spec.new_cursor_rect.as_ref(),
            spec.old_cursor_visual_line_id,
            spec.new_cursor_visual_line_id,
            spec.old_cursor_line_top,
            spec.old_cursor_line_bottom,
            spec.new_cursor_line_top,
            spec.new_cursor_line_bottom,
            spec.caret_handoff.clone(),
            spec.caret_duration_ms,
        )
    } else {
        None
    };

    // 5. 诊断日志
    editor_animation_debug_log(&format!(
        "anim_spec: op={:?} units={} inserted={} deleted={} rebased={} handoff={} epoch={} \
         text_anim={} caret_anim={} coordinated_anim={}",
        spec.operation_kind,
        units.len(),
        spec.inserted_ranges.len(),
        spec.deleted_ranges.len(),
        spec.rebase_frames.len(),
        spec.caret_handoff.is_some(),
        spec.cursor_owner_epoch,
        spec.text_animation_enabled,
        spec.caret_animation_enabled,
        spec.coordinated_animation_enabled,
    ));

    // Issue #785 评论 5857451442: 把 InsertReveal 未生成的诊断从 env-gated debug log
    // 升级为正式 editor.anim.* 诊断事件（writer_diagnostics），普通诊断包（不带 debug
    // 环境变量）即可在 zip 中看到，满足"诊断包直接看出原因"的要求。
    // 字段带 inserted range、snapshot revision、相交 line 数、相交 cluster 数、
    // 空白/控制字符跳过数，便于定位是注入链遗漏还是全部 cluster 被当空白跳过。
    //
    // Issue #785 评论 5857873894 修改 5: 正式 Warn 只针对 inserted range 确实含可见字符
    //（非空白、非控制字符）但最终 InsertReveal 为 0 的情况。空格、tab、换行继续正常跳过，
    // 不报异常。如果所有 inserted range 都只含空白/控制字符，则不记 Warn（这是正常跳过）。
    if spec.text_animation_enabled && !spec.inserted_ranges.is_empty() {
        let insert_reveal_count = units
            .iter()
            .filter(|u| u.slice.kind == AnimatedSliceKind::InsertReveal)
            .count();
        if insert_reveal_count == 0 {
            let mut intersecting_lines = 0usize;
            let mut intersecting_clusters = 0usize;
            let mut whitespace_skip_count = 0usize;
            for &(i_start, i_end) in &spec.inserted_ranges {
                for new_line in spec.new_snapshot.line_snapshots.iter() {
                    if new_line.byte_start < i_end && new_line.byte_end > i_start {
                        intersecting_lines += 1;
                    }
                    for new_cluster in new_line.clusters.iter() {
                        if new_cluster.byte_start < i_end && new_cluster.byte_end > i_start {
                            intersecting_clusters += 1;
                            if let Some(text) = spec
                                .new_snapshot
                                .virtual_text
                                .get(new_cluster.byte_start..new_cluster.byte_end)
                            {
                                if text.chars().all(|c| c.is_whitespace() || c.is_control()) {
                                    whitespace_skip_count += 1;
                                }
                            }
                        }
                    }
                }
            }
            // Issue #785 评论 5858151780: 是否属于"可见字符 Insert"直接检查
            // new_snapshot.virtual_text[inserted_range]，不用"相交 cluster 是否全是 whitespace"
            // 反推。Partial cluster 可能同时包含旧可见字符和本次插入的空白，按整个 cluster
            // 判断会把正常空白输入误报成 InsertReveal 丢失。
            let all_inserted_is_whitespace =
                spec.inserted_ranges.iter().all(|&(i_start, i_end)| {
                    let text = spec
                        .new_snapshot
                        .virtual_text
                        .get(i_start..i_end)
                        .unwrap_or("");
                    text.chars().all(|c| c.is_whitespace() || c.is_control())
                });
            if !all_inserted_is_whitespace {
                {
                    use std::collections::BTreeMap;
                    let mut fields = BTreeMap::new();
                    fields.insert(
                        "transaction_id".to_string(),
                        serde_json::json!(spec.key.transaction_id),
                    );
                    fields.insert(
                        "generation".to_string(),
                        serde_json::json!(spec.key.generation),
                    );
                    fields.insert(
                        "operation_kind".to_string(),
                        serde_json::Value::String(
                            operation_kind_label(spec.operation_kind).to_string(),
                        ),
                    );
                    fields.insert(
                        "inserted_ranges".to_string(),
                        serde_json::json!(spec.inserted_ranges),
                    );
                    fields.insert(
                        "snapshot_revision".to_string(),
                        serde_json::json!(spec.new_snapshot.revision.0),
                    );
                    fields.insert(
                        "intersecting_line_count".to_string(),
                        serde_json::json!(intersecting_lines),
                    );
                    fields.insert(
                        "intersecting_cluster_count".to_string(),
                        serde_json::json!(intersecting_clusters),
                    );
                    fields.insert(
                        "whitespace_skip_count".to_string(),
                        serde_json::json!(whitespace_skip_count),
                    );
                    fields.insert(
                        "unit_kinds".to_string(),
                        serde_json::Value::String(unit_kind_labels(&units).join(",")),
                    );
                    writer_diagnostics::record_event(writer_diagnostics::DiagnosticEvent {
                        timestamp_ms: chrono::Utc::now().timestamp_millis(),
                        sequence: 0,
                        session_id: String::new(),
                        level: writer_diagnostics::DiagnosticLevel::Warn,
                        origin: writer_diagnostics::DiagnosticOrigin::App,
                        event: "editor.anim.insert_reveal_not_generated".to_string(),
                        target: "editor.anim".to_string(),
                        message: Some(
                            "inserted range has visible chars but InsertReveal count is 0"
                                .to_string(),
                        ),
                        fields,
                    });
                }
                // 保留 env-gated debug log 作为开发时辅助，正式诊断走上面的 writer_diagnostics 事件。
                editor_animation_debug_log(&format!(
                    "anim_diagnostic: InsertReveal_not_generated inserted_ranges={:?} \
                     snapshot_revision={} intersecting_lines={} intersecting_clusters={} \
                     whitespace_skip={} — possible causes: animation visuals injection missed \
                     the inserted line, no non-whitespace cluster in range, or all clusters \
                     skipped as whitespace",
                    spec.inserted_ranges,
                    spec.new_snapshot.revision.0,
                    intersecting_lines,
                    intersecting_clusters,
                    whitespace_skip_count,
                ));
            }
        }
    }

    // 6. 唯一 PreparedTextVisualTransaction struct literal
    PreparedTextVisualTransaction {
        key: spec.key,
        state: TextVisualTransactionState::Pending,
        operation_kind: spec.operation_kind,
        timeline: TransactionTimeline::new(if spec.text_animation_enabled {
            spec.text_duration_ms
        } else {
            spec.caret_duration_ms
        }),
        units,
        old_cursor_rect: spec.old_cursor_rect,
        new_cursor_rect: spec.new_cursor_rect,
        cursor_visual_track,
        cancel_reason: None,
        texture_prepared: false,
        old_snapshot: Some(spec.old_snapshot),
        new_snapshot: Some(spec.new_snapshot),
        cursor_owner_epoch: spec.cursor_owner_epoch,
        caret_motion_retired: false,
        visual_affected_byte_range_old: spec.visual_affected_byte_range_old,
        visual_affected_byte_range_new: spec.visual_affected_byte_range_new,
        layout_basis_revision: spec.layout_basis_revision,
    }
}

// 四类动画 slice 的构造按编辑语义拆到 slices.rs，本文件保留事务装配、
// slice 合并与 coordinator 方法。
pub(crate) mod slices;

pub(crate) use slices::{
    build_cluster_reflow_slices, build_composition_commit_crossfade_slices,
    build_delete_conceal_slices, build_insert_reveal_slices,
};

pub(crate) fn merge_adjacent_slices(slices: Vec<AnimatedSlice>) -> Vec<AnimatedSlice> {
    if slices.len() <= 1 {
        return slices;
    }

    let mut result: Vec<AnimatedSlice> = Vec::with_capacity(slices.len());
    let mut current = slices[0].clone();

    for next in &slices[1..] {
        if can_merge(&current, next) {
            current = merge_two(&current, next);
        } else {
            result.push(current);
            current = next.clone();
        }
    }
    result.push(current);
    result
}

fn can_merge(a: &AnimatedSlice, b: &AnimatedSlice) -> bool {
    // 条件 1：相同 kind
    if a.kind != b.kind {
        return false;
    }
    // 条件 2：相同 snapshot_id
    if a.snapshot_id != b.snapshot_id {
        return false;
    }
    // 条件 3：相邻 byte range
    if a.byte_end != b.byte_start {
        return false;
    }
    // 条件 4：同方向
    match a.kind {
        AnimatedSliceKind::InsertReveal => {
            // 同一行吐字：from_document_rect 的 y 相同
            (a.from_document_rect.y - b.from_document_rect.y).abs() < 0.5
        }
        AnimatedSliceKind::DeleteConceal => {
            // 同一行吞字且同方向：from_document_rect 的 y 相同，conceal_to_left_edge 相同
            (a.from_document_rect.y - b.from_document_rect.y).abs() < 0.5
                && a.conceal_to_left_edge == b.conceal_to_left_edge
        }
        AnimatedSliceKind::ReflowMove | AnimatedSliceKind::ReflowCrossFade => {
            // 移动向量相同：dx = to.x - from.x, dy = to.y - from.y
            let a_dx = a.to_document_rect.x - a.from_document_rect.x;
            let a_dy = a.to_document_rect.y - a.from_document_rect.y;
            let b_dx = b.to_document_rect.x - b.from_document_rect.x;
            let b_dy = b.to_document_rect.y - b.from_document_rect.y;
            let same_vector = (a_dx - b_dx).abs() < 0.5 && (a_dy - b_dy).abs() < 0.5;
            // Issue #738 评论 5789470425 问题3: CrossFade 只能合并同 group 同 side。
            // old 侧和 new 侧不能互相合并；不同 group 不能合并。
            let same_crossfade_group = a.crossfade_group_id == b.crossfade_group_id
                && a.crossfade_side == b.crossfade_side;
            same_vector && same_crossfade_group
        }
    }
}

fn merged_byte_range(a: (usize, usize), b: (usize, usize)) -> (usize, usize) {
    (a.0.min(b.0), a.1.max(b.1))
}

fn merge_two(a: &AnimatedSlice, b: &AnimatedSlice) -> AnimatedSlice {
    let (byte_start, byte_end) =
        merged_byte_range((a.byte_start, a.byte_end), (b.byte_start, b.byte_end));
    // Issue #738 评论 5789470425 问题2: 合并 reflow_anchors 列表，不丢子 cluster 身份。
    let merged_anchors: Vec<crate::sujian_editor_item::animated_slice::ReflowAnchor> = a
        .reflow_anchors
        .iter()
        .chain(&b.reflow_anchors)
        .cloned()
        .collect();
    // merged unit 的 from/to/source 取各 anchor 的 union（连续矩形做整体插值）。
    // 用 inline min/max 计算而非 bounding_box helper，强调 reflow_anchors 才是逐 cluster 真相。
    let merged_from = union_source_rect(&a.from_document_rect, &b.from_document_rect);
    let merged_to = union_source_rect(&a.to_document_rect, &b.to_document_rect);
    let merged_source = union_source_rect(&a.source_rect, &b.source_rect);
    // shaping_identity 取首个 anchor 的代表值；逐 cluster 真实 shaping 在 reflow_anchors。
    let head_shaping = a.shaping_identity.clone();
    AnimatedSlice {
        kind: a.kind,
        snapshot_id: a.snapshot_id,
        source_rect: merged_source,
        from_document_rect: merged_from,
        to_document_rect: merged_to,
        opacity_from: a.opacity_from,
        opacity_to: a.opacity_to,
        scale_from: a.scale_from,
        scale_to: a.scale_to,
        byte_start,
        byte_end,
        shaping_identity: head_shaping,
        conceal_to_left_edge: a.conceal_to_left_edge,
        // Issue #808: 合并后的 slice 取首个 slice 的 caret 锚点。
        // 同一行的相邻 slice 共享同一 caret 锚点（行内插入/删除），取首个即可。
        caret_anchor_x: a.caret_anchor_x,
        caret_anchor_y: a.caret_anchor_y,
        is_caret_line: a.is_caret_line,
        visual_line_id: a.visual_line_id,
        start_fraction: a.start_fraction.min(b.start_fraction),
        static_hidden_document_rects: a
            .static_hidden_document_rects
            .iter()
            .chain(&b.static_hidden_document_rects)
            .cloned()
            .collect(),
        crossfade_group_id: a.crossfade_group_id,
        crossfade_side: a.crossfade_side,
        reflow_anchors: merged_anchors,
    }
}

fn union_source_rect(a: &SourceRect, b: &SourceRect) -> SourceRect {
    let min_x = a.x.min(b.x);
    let min_y = a.y.min(b.y);
    let max_right = (a.x + a.w).max(b.x + b.w);
    let max_bottom = (a.y + a.h).max(b.y + b.h);
    SourceRect {
        x: min_x,
        y: min_y,
        w: max_right - min_x,
        h: max_bottom - min_y,
    }
}

impl LinuxEditorAnimationCoordinator {
    pub(crate) fn create_transaction_from_prepared_handoff(
        &mut self,
        prepared: Option<PreparedRebaseHandoff>,
        vt: &PreparedEditMotion,
        // Issue #756: 文字动画开关（coordinated || typing）与光标动画开关
        // （coordinated || smooth）由调用方按同一份设置算出，两者互相独立。
        text_animation_enabled: bool,
        caret_animation_enabled: bool,
        // Issue #756: 协同动画显式模式。决定吞吐字是否由 caret 驱动。
        coordinated_animation_enabled: bool,
        old_cursor_rect: Option<CursorRect>,
        new_cursor_rect: Option<CursorRect>,
        old_cursor_visual_line_id: Option<usize>,
        new_cursor_visual_line_id: Option<usize>,
        old_cursor_line_top: f64,
        old_cursor_line_bottom: f64,
        new_cursor_line_top: f64,
        new_cursor_line_bottom: f64,
        old_snapshot: &EditorLayoutSnapshot,
        new_snapshot: &EditorLayoutSnapshot,
        cursor_owner_epoch: u64,
        layout_basis_revision: LayoutRevision,
    ) -> Option<VisualTransactionKey> {
        let prepared = prepared?;
        match prepared {
            PreparedRebaseHandoff::Insert {
                rebase_frames,
                caret_handoff,
                range_start,
                range_end,
                insert_offset_map,
                visual_affected_byte_range_old,
                visual_affected_byte_range_new,
            } => {
                let key = self.alloc_key();
                let inserted_range_tuple = (range_start, range_end);
                // Issue #747 评论 5813540976: 只归一化 spec，统一由 build_prepared_transaction 构造。
                // build_prepared_transaction 内部调 build_insert_reveal_slices / build_cluster_reflow_slices
                // / match_rebase_frames / build_cursor_visual_track 完成全部 slice/unit/track 构造。
                // Issue #687: Insert 事务 changed range 由 Core 显式拥有，reflow 排除 inserted_range。
                // Issue #756: InsertReveal 生成由 text_animation_enabled + caret_animation_enabled 决定，
                // 不再由 smooth_cursor_enabled 单独决定，也不再把 typing && smooth 当成协同。
                // Issue #710 评论 5732160521 问题 1/3: Insert 事务 old 侧是插入点
                // (range_start, range_start)，new 侧是 inserted_range。
                let carried_rebase = rebase_frames.len();
                let spec = VisualEditSpec {
                    key,
                    operation_kind: TextVisualOperationKind::Insert,
                    old_snapshot: old_snapshot.clone(),
                    new_snapshot: new_snapshot.clone(),
                    inserted_ranges: vec![inserted_range_tuple],
                    deleted_ranges: vec![],
                    offset_map: insert_offset_map,
                    old_cursor_rect,
                    new_cursor_rect,
                    old_cursor_visual_line_id,
                    new_cursor_visual_line_id,
                    old_cursor_line_top,
                    old_cursor_line_bottom,
                    new_cursor_line_top,
                    new_cursor_line_bottom,
                    cursor_owner_epoch,
                    layout_basis_revision,
                    rebase_frames,
                    caret_handoff,
                    visual_affected_byte_range_old,
                    visual_affected_byte_range_new,
                    text_duration_ms: vt.text_duration_ms,
                    caret_duration_ms: vt.caret_duration_ms,
                    text_animation_enabled,
                    caret_animation_enabled,
                    coordinated_animation_enabled,
                    composition_commit_crossfade: None,
                };
                let prepared_tx = build_prepared_transaction(spec);

                // Issue #690 评论 5675007226 步骤 5: 每笔动画一条紧凑事件进正式诊断包。
                emit_transaction_diagnostic(&prepared_tx, "editor.anim.create", "created");
                editor_animation_debug_log(&format!(
                    "anim_event: key={:?} op=Insert inserted={:?} unit_kinds={:?} carried_rebase={}",
                    key,
                    inserted_range_tuple,
                    unit_kind_labels(&prepared_tx.units),
                    carried_rebase,
                ));

                self.prepared_queue.enqueue(prepared_tx);

                Some(key)
            }
            PreparedRebaseHandoff::Delete {
                rebase_frames,
                caret_handoff,
                deleted_ranges,
                delete_offset_map,
                visual_affected_byte_range_old,
                visual_affected_byte_range_new,
            } => {
                let key = self.alloc_key();

                // Issue #747 评论 5813540976: 只归一化 spec，统一由 build_prepared_transaction 构造。
                // build_prepared_transaction 内部调 build_cluster_reflow_slices(key, old, new,
                // offset_map, &deleted_ranges, &[], ...) 排除 deleted_range，
                // Issue #687: changed range 由 Core 显式拥有。
                // Issue #756: DeleteConceal 生成由 text_animation_enabled + caret_animation_enabled 决定，
                // 不再由 smooth_cursor_enabled 单独决定，也不再把 typing && smooth 当成协同。
                // Issue #710 评论 5732160521 问题 1/3: Delete 事务 old 侧是 deleted_range，
                // new 侧是删除后落点 (rebase_byte_start, rebase_byte_start)。
                let carried_rebase = rebase_frames.len();
                let deleted_ranges_log = deleted_ranges.clone();
                let spec = VisualEditSpec {
                    key,
                    operation_kind: TextVisualOperationKind::Delete,
                    old_snapshot: old_snapshot.clone(),
                    new_snapshot: new_snapshot.clone(),
                    inserted_ranges: vec![],
                    deleted_ranges,
                    offset_map: delete_offset_map,
                    old_cursor_rect,
                    new_cursor_rect,
                    old_cursor_visual_line_id,
                    new_cursor_visual_line_id,
                    old_cursor_line_top,
                    old_cursor_line_bottom,
                    new_cursor_line_top,
                    new_cursor_line_bottom,
                    cursor_owner_epoch,
                    layout_basis_revision,
                    rebase_frames,
                    caret_handoff,
                    visual_affected_byte_range_old,
                    visual_affected_byte_range_new,
                    text_duration_ms: vt.text_duration_ms,
                    caret_duration_ms: vt.caret_duration_ms,
                    text_animation_enabled,
                    caret_animation_enabled,
                    coordinated_animation_enabled,
                    composition_commit_crossfade: None,
                };
                let prepared_tx = build_prepared_transaction(spec);

                // Issue #690 评论 5675007226 步骤 5: 每笔动画一条紧凑事件进正式诊断包。
                emit_transaction_diagnostic(&prepared_tx, "editor.anim.create", "created");
                editor_animation_debug_log(&format!(
                    "anim_event: key={:?} op=Delete deleted={:?} unit_kinds={:?} carried_rebase={}",
                    key,
                    deleted_ranges_log,
                    unit_kind_labels(&prepared_tx.units),
                    carried_rebase,
                ));

                self.prepared_queue.enqueue(prepared_tx);

                Some(key)
            }
        }
    }

    // process_transaction 只服务本文件的单元测试：正文事务的生产路径已经改成
    // `build_prepared_transaction` 直接构造，rebase 走 `rebind_timed_units_to_canonical`，
    // 没有任何生产调用方。整段 cfg(test)，避免在正常构建里被 dead_code 判死。
    #[cfg(test)]
    pub fn process_transaction(
        &mut self,
        vt: &PreparedEditMotion,
        typing_animation_enabled: bool,
        smooth_cursor_enabled: bool,
        coordinated_animation_enabled: bool,
        is_scrolling: bool,
        is_loading: bool,
        is_applying_format: bool,
        old_cursor_rect: Option<CursorRect>,
        new_cursor_rect: Option<CursorRect>,
        old_cursor_visual_line_id: Option<usize>,
        new_cursor_visual_line_id: Option<usize>,
        old_cursor_line_top: f64,
        old_cursor_line_bottom: f64,
        new_cursor_line_top: f64,
        new_cursor_line_bottom: f64,
        old_snapshot: &EditorLayoutSnapshot,
        new_snapshot: &EditorLayoutSnapshot,
        cursor_owner_epoch: u64,
        layout_basis_revision: LayoutRevision,
    ) -> Option<VisualTransactionKey> {
        // Issue #756: 删除把"两个独立开关同时开启"等价成"协同动画"的逻辑。
        // - coordinated=true 时：文字与光标绑死，要求有效 caret motion，否则不创建事务
        //   （文字动画也不启动）。
        // - coordinated=false 时：typing_animation_enabled 只决定文字动画
        //   （Reflow + InsertReveal/DeleteConceal），smooth_cursor_enabled 只决定光标动画
        //   （caret motion track）。两者互相独立，同时为 true 不等于协同：
        //   只有 coordinated_animation_enabled 才走协同路径。
        let text_animation_enabled = coordinated_animation_enabled || typing_animation_enabled;
        let caret_animation_enabled = coordinated_animation_enabled || smooth_cursor_enabled;
        if (!text_animation_enabled && !caret_animation_enabled)
            || is_scrolling
            || is_loading
            || is_applying_format
        {
            return None;
        }

        // Issue #756: valid_caret_motion_track 检查。
        // - coordinated=true 时：文字和光标绑死，必须有有效 caret motion，否则不创建事务。
        // - coordinated=false 时：不把缺少 caret motion 当成"整笔不播"——文字动画（Reflow）
        //   与 cursor track 各自按自己的开关决定（无 caret motion 时只是没有 CaretDriven
        //   units 与 cursor track，与 Issue #727 约束 5 一致）。
        let valid_caret_motion_track = old_cursor_rect.is_some() && new_cursor_rect.is_some();
        if coordinated_animation_enabled && !valid_caret_motion_track {
            return None;
        }

        let mode = AnimationMode::from_context(is_scrolling, is_loading, is_applying_format);
        if !mode.should_create_transaction() {
            return None;
        }

        match vt.kind {
            EditorAnimationKind::Insert => {
                if let Some(range) = vt.inserted_range {
                    let range_start = range.start().value();
                    let range_end = range.end().value();
                    let insert_offset_map = OffsetMap::build(&vt.old_text, &vt.new_text);
                    // Issue #710 评论 5733109905: 冲突检测用 current-old 坐标系。
                    // 先计算 visual_affected_byte_range 得到 old-side range (old_s, old_e)，
                    // 再用 old_s/old_e 查冲突。insert_offset_map 仍保留用于 rebase。
                    let (visual_affected_byte_range_old, visual_affected_byte_range_new) = {
                        let (old_s, old_e, new_s, new_e) = compute_affected_paragraph_ranges(
                            &vt.old_text,
                            &vt.new_text,
                            (range_start, range_start),
                            (range_start, range_end),
                        );
                        (Some((old_s, old_e)), Some((new_s, new_e)))
                    };
                    let (conflict_old_start, conflict_old_end) =
                        visual_affected_byte_range_old.unwrap_or((range_start, range_start));
                    let conflicting = self.prepared_queue.find_conflicting_transaction(
                        &vt.old_text,
                        conflict_old_start,
                        conflict_old_end,
                    );
                    // 纯插入在 old 文档里就是 range_start 这一个位置点。
                    let now = Instant::now();
                    let (rebase_frames, caret_handoff) = self.take_rebase_frames(
                        &conflicting,
                        "rebased_by_insert",
                        now,
                        Some((&[(range_start, range_start)], &insert_offset_map)),
                        &vt.old_text,
                        cursor_owner_epoch,
                    );

                    let key = self.alloc_key();
                    let inserted_range_tuple = (range_start, range_end);
                    // Issue #747 评论 5813540976: 只归一化 spec，统一由 build_prepared_transaction 构造。
                    // build_prepared_transaction 内部调 build_cluster_reflow_slices(key, old, new,
                    // offset_map, &[], &[inserted_range_tuple], ...) 排除 inserted_range，
                    // Issue #687: changed range 由 Core 显式拥有。
                    // Issue #756: InsertReveal 生成由 text_animation_enabled + caret_animation_enabled 决定，
                    // 不再把 typing && smooth 当成协同。
                    // Issue #710 评论 5732160521 问题 1/3: Insert 事务 old 侧是插入点
                    // (range_start, range_start)，new 侧是 inserted_range。
                    let carried_rebase = rebase_frames.len();
                    let spec = VisualEditSpec {
                        key,
                        operation_kind: TextVisualOperationKind::Insert,
                        old_snapshot: old_snapshot.clone(),
                        new_snapshot: new_snapshot.clone(),
                        inserted_ranges: vec![inserted_range_tuple],
                        deleted_ranges: vec![],
                        offset_map: insert_offset_map,
                        old_cursor_rect,
                        new_cursor_rect,
                        old_cursor_visual_line_id,
                        new_cursor_visual_line_id,
                        old_cursor_line_top,
                        old_cursor_line_bottom,
                        new_cursor_line_top,
                        new_cursor_line_bottom,
                        cursor_owner_epoch,
                        layout_basis_revision,
                        rebase_frames,
                        caret_handoff,
                        visual_affected_byte_range_old,
                        visual_affected_byte_range_new,
                        text_duration_ms: vt.text_duration_ms,
                        caret_duration_ms: vt.caret_duration_ms,
                        text_animation_enabled,
                        caret_animation_enabled,
                        coordinated_animation_enabled,
                        composition_commit_crossfade: None,
                    };
                    let prepared = build_prepared_transaction(spec);

                    // Issue #690 评论 5675007226 步骤 5: 每笔动画一条紧凑事件进正式诊断包。
                    emit_transaction_diagnostic(&prepared, "editor.anim.create", "created");
                    editor_animation_debug_log(&format!(
                        "anim_event: key={:?} op=Insert inserted={:?} unit_kinds={:?} carried_rebase={}",
                        key,
                        inserted_range_tuple,
                        unit_kind_labels(&prepared.units),
                        carried_rebase,
                    ));

                    self.prepared_queue.enqueue(prepared);

                    return Some(key);
                }
            }
            EditorAnimationKind::Delete => {
                let deleted_ranges: Vec<(usize, usize)> = if let Some(range) = vt.deleted_range {
                    vec![(range.start().value(), range.end().value())]
                } else {
                    let changes = diff_plain_text(&vt.old_text, &vt.new_text);
                    let mut ranges = Vec::new();
                    for change in &changes {
                        if let writer_core::editor::EditorChange::Delete { index, text } = change {
                            let range_start = index.value();
                            let range_end = range_start + text.len();
                            ranges.push((range_start, range_end));
                        }
                    }
                    ranges
                };

                let rebase_byte_start = deleted_ranges.first().map(|(s, _)| *s).unwrap_or(0);
                let rebase_byte_end = deleted_ranges.last().map(|(_, e)| *e).unwrap_or(0);
                let delete_offset_map = OffsetMap::build(&vt.old_text, &vt.new_text);
                // Issue #710 评论 5733109905: 冲突检测用 current-old 坐标系。
                // 先计算 visual_affected_byte_range 得到 old-side range (old_s, old_e)，
                // 再用 old_s/old_e 查冲突。delete_offset_map 仍保留用于 rebase。
                let (visual_affected_byte_range_old, visual_affected_byte_range_new) = {
                    let (old_s, old_e, new_s, new_e) = compute_affected_paragraph_ranges(
                        &vt.old_text,
                        &vt.new_text,
                        (rebase_byte_start, rebase_byte_end),
                        (rebase_byte_start, rebase_byte_start),
                    );
                    (Some((old_s, old_e)), Some((new_s, new_e)))
                };
                let (conflict_old_start, conflict_old_end) =
                    visual_affected_byte_range_old.unwrap_or((rebase_byte_start, rebase_byte_end));
                let conflicting = self.prepared_queue.find_conflicting_transaction(
                    &vt.old_text,
                    conflict_old_start,
                    conflict_old_end,
                );
                let now = Instant::now();
                let (rebase_frames, caret_handoff) = self.take_rebase_frames(
                    &conflicting,
                    "rebased_by_delete",
                    now,
                    Some((&deleted_ranges, &delete_offset_map)),
                    &vt.old_text,
                    cursor_owner_epoch,
                );

                let key = self.alloc_key();

                // Issue #747 评论 5813540976: 只归一化 spec，统一由 build_prepared_transaction 构造。
                // build_prepared_transaction 内部调 build_cluster_reflow_slices(key, old, new,
                // offset_map, &deleted_ranges, &[], ...) 排除 deleted_range，
                // Issue #687: changed range 由 Core 显式拥有。
                // Issue #756: DeleteConceal 生成由 text_animation_enabled + caret_animation_enabled 决定，
                // 不再由 smooth_cursor_enabled 单独决定，也不再把 typing && smooth 当成协同。
                // Issue #710 评论 5732160521 问题 1/3: Delete 事务 old 侧是 deleted_range，
                // new 侧是删除后落点 (rebase_byte_start, rebase_byte_start)。
                let carried_rebase = rebase_frames.len();
                let spec = VisualEditSpec {
                    key,
                    operation_kind: TextVisualOperationKind::Delete,
                    old_snapshot: old_snapshot.clone(),
                    new_snapshot: new_snapshot.clone(),
                    inserted_ranges: vec![],
                    deleted_ranges: deleted_ranges.clone(),
                    offset_map: delete_offset_map,
                    old_cursor_rect,
                    new_cursor_rect,
                    old_cursor_visual_line_id,
                    new_cursor_visual_line_id,
                    old_cursor_line_top,
                    old_cursor_line_bottom,
                    new_cursor_line_top,
                    new_cursor_line_bottom,
                    cursor_owner_epoch,
                    layout_basis_revision,
                    rebase_frames,
                    caret_handoff,
                    visual_affected_byte_range_old,
                    visual_affected_byte_range_new,
                    text_duration_ms: vt.text_duration_ms,
                    caret_duration_ms: vt.caret_duration_ms,
                    text_animation_enabled,
                    caret_animation_enabled,
                    coordinated_animation_enabled,
                    composition_commit_crossfade: None,
                };
                let prepared = build_prepared_transaction(spec);

                // Issue #690 评论 5675007226 步骤 5: 每笔动画一条紧凑事件进正式诊断包。
                emit_transaction_diagnostic(&prepared, "editor.anim.create", "created");
                editor_animation_debug_log(&format!(
                    "anim_event: key={:?} op=Delete deleted={:?} unit_kinds={:?} carried_rebase={}",
                    key,
                    deleted_ranges,
                    unit_kind_labels(&prepared.units),
                    carried_rebase,
                ));

                self.prepared_queue.enqueue(prepared);

                return Some(key);
            }
            EditorAnimationKind::Cursor => {
                // Issue #702: 删除"纯光标移动创建空 Cursor 文字事务"的结构。
                // 纯光标移动直接维护 CursorAnimationState（由 rendering.rs
                // update_cursor_visual_position → build_cursor_plan → apply_plan
                // 构造），用 Scene Graph 当前帧 frame_now 推进 from→to 动画，
                // 不再伪装成文字事务（units=空）。
                // 此分支不再创建任何事务，返回 None。
                return None;
            }
        }
        None
    }
}

#[cfg(test)]
mod tests;
