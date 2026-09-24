use std::collections::HashMap;
use std::time::Instant;

use crate::sujian_editor_item::animated_slice::{AnimatedSlice, AnimatedSliceKind};
use crate::sujian_editor_item::layout_revision::LayoutRevision;
use crate::sujian_editor_item::layout_snapshot::{ShapingIdentity, SourceRect};
use crate::editor::layout::CanonicalDocumentVisualSnapshot;

use super::timeline::VisualUnitTiming;
use super::types::{PreparedTextVisualTransaction, PreparedVisualUnit};

impl PreparedTextVisualTransaction {
    /// Issue #738 评论 5789470425: 把活动事务的 Timed Reflow unit 从旧 canonical
    /// layout basis 重绑到当前 canonical layout。
    ///
    /// 只处理 `ReflowMove / ReflowCrossFade`（Timed unit）。CaretDriven unit
    ///（InsertReveal / DeleteConceal）由 caret owner/epoch 规则单独管理，不在此重绑。
    ///
    /// 流程：
    /// 1. 按本事务 `new_snapshot.virtual_text -> current_text` 建 `OffsetMap`。
    /// 2. ReflowMove：逐 anchor 映射 byte range，在当前 canonical 找相交 cluster 列表，
    ///    逐 cluster 校验 shaping identity。所有 anchor 都找到且 shaping 一致 → rebind；
    ///    任一 anchor 失效 → Remove unit。
    /// 3. ReflowCrossFade：以 `crossfade_group_id` 为单位成对重绑（crossfade_pair）。
    ///    同一个 `now` 采样 old/new 两侧当前帧；old side 保留旧纹理继续 fade-out，
    ///    只更新跟随的新目标几何；new side 绑定 current canonical 对应新几何继续 fade-in；
    ///    两侧使用相同 remaining duration。new-side anchor 无法映射/当前 shaping 又变化时，
    ///    整组一起结束并交给 canonical，不能只删一边。
    /// 4. 推进 `layout_basis_revision` 到当前 canonical revision。
    pub(crate) fn rebind_timed_units_to_canonical(
        &mut self,
        current_text: &str,
        canonical_snapshot: &CanonicalDocumentVisualSnapshot,
        current_layout_revision: LayoutRevision,
        now: Instant,
    ) {
        let new_snap_text = match self.new_snapshot.as_ref() {
            Some(s) => s.virtual_text.as_str(),
            None => {
                self.units.retain(|u| !is_timed_reflow_kind(u.slice.kind));
                // Issue #738 评论 5795950264 问题1: 仅当 CaretDriven 已 retire 或
                // 无 CaretDriven units 时才提升 layout_basis_revision，否则保持旧值
                // 让 basis 守卫继续 retire 旧 CaretDriven。
                if self.caret_motion_retired || !self.has_caret_driven_units() {
                    self.layout_basis_revision = current_layout_revision;
                }
                return;
            }
        };

        let offset_map = writer_core::editor::OffsetMap::build(new_snap_text, current_text);

        // Issue #738 评论 5793319451 问题2: CrossFade 多对多 group 重绑。
        // 按 crossfade_group_id 把 CrossFade units 分组，以 group 为单位处理。
        // 旧实现用 HashMap<u64, (Option<usize>, Option<usize>)> 每组只存最后一个 old 和
        // 最后一个 new，多对多 group 只留最后一对进 crossfade_pairs，其余成员被单独 Remove。
        // 改成 CrossFadeGroup { old_indices, new_indices } 收集 group 的所有成员，
        // 整组一起处理：任一侧为空或任一 new side anchor 失效 → 整组 Remove；
        // 全部有效 → 同一个 now 采样，所有 old fade-out，所有 new fade-in。
        struct CrossFadeGroup {
            old_indices: Vec<usize>,
            new_indices: Vec<usize>,
        }
        let mut crossfade_groups: HashMap<u64, CrossFadeGroup> = HashMap::new();
        for (i, unit) in self.units.iter().enumerate() {
            if !is_timed_reflow_kind(unit.slice.kind) {
                continue;
            }
            if unit.slice.kind != AnimatedSliceKind::ReflowCrossFade {
                continue;
            }
            if let Some(gid) = unit.slice.crossfade_group_id {
                let entry = crossfade_groups.entry(gid).or_insert(CrossFadeGroup {
                    old_indices: Vec::new(),
                    new_indices: Vec::new(),
                });
                match unit.slice.crossfade_side {
                    Some(crate::sujian_editor_item::animated_slice::CrossFadeSide::Old) => {
                        entry.old_indices.push(i)
                    }
                    Some(crate::sujian_editor_item::animated_slice::CrossFadeSide::New) => {
                        entry.new_indices.push(i)
                    }
                    None => {}
                }
            }
        }

        // Issue #738 评论 5794018647: RebindDecision 重构。
        // group 管"活不活"，unit 管"画到哪"。逐 anchor 保留自己的
        // current_rect/target_rect，不覆盖成总 frame/总 target。
        // - RebindMerged: ReflowMove merged 整体重绑，逐 anchor 保留 (current_rect, target_rect)。
        // - RebindNewSide: CrossFade new side 用自己的 canonical target 重绑。
        // - RebindOldSideFollow: CrossFade old side 一对一跟随对应 new side 的 target。
        // - RebindOldSideInPlace: CrossFade old side 多对多原位 fade-out（from=to=当前帧）。
        // - Split: movement vector 不一致时拆分（5792244119 问题 2）。
        enum RebindDecision {
            Keep,
            Remove,
            RebindMerged {
                union_current: SourceRect,
                union_target: SourceRect,
                anchor_rebinds: Vec<(SourceRect, SourceRect)>,
            },
            RebindNewSide {
                anchor_rebinds: Vec<(SourceRect, SourceRect)>,
            },
            RebindOldSideFollow {
                target: SourceRect,
            },
            RebindOldSideInPlace,
            Split(Vec<PreparedVisualUnit>),
        }
        let mut decisions: Vec<RebindDecision> = (0..self.units.len())
            .map(|_| RebindDecision::Keep)
            .collect();

        // ReflowMove: 逐 anchor 找新 canonical cluster，逐 cluster 校验 shaping。
        // Issue #738 评论 5792244119 问题 2: 各 anchor movement vector 不一致时拆回多个
        // PreparedVisualUnit，不再用 union_of_hits 合成跨行大矩形。
        for (i, unit) in self.units.iter().enumerate() {
            if !is_timed_reflow_kind(unit.slice.kind) {
                continue;
            }
            if unit.slice.kind == AnimatedSliceKind::ReflowCrossFade {
                continue;
            }
            // Issue #738 评论 5794018647: 逐 anchor 旧 from→to 按 visible 采样算 current_rect，
            // target_rect = 新 canonical cluster doc_rect，movement vector = target - current。
            let visible = unit.current_visible_fraction(now);
            let mut anchor_rebinds: Vec<(SourceRect, SourceRect)> = Vec::new();
            let mut all_anchors_ok = true;
            for anchor in &unit.slice.reflow_anchors {
                let (mapped_start, mapped_end) =
                    match offset_map.map_old_range_to_new(anchor.byte_start, anchor.byte_end) {
                        Some(r) => r,
                        None => {
                            all_anchors_ok = false;
                            break;
                        }
                    };
                let hits = find_clusters_in_canonical(canonical_snapshot, mapped_start, mapped_end);
                if hits.is_empty() {
                    all_anchors_ok = false;
                    break;
                }
                // 逐 cluster 校验 anchor shaping，取第一个 shaping 匹配的 hit 作为 target。
                let mut matched_hit: Option<&CanonicalClusterHit> = None;
                for hit in &hits {
                    if let Some(sid) = &anchor.shaping_identity {
                        if sid.is_same_shaping(&hit.shaping) {
                            matched_hit = Some(hit);
                            break;
                        }
                    }
                }
                let target_hit = match matched_hit {
                    Some(h) => h,
                    None => {
                        all_anchors_ok = false;
                        break;
                    }
                };
                // current_rect: anchor 旧 from→to 按 visible 采样（与 compute_frame 一致）。
                // Issue #738 评论 5796693007 问题2: ReflowMove 的 w/h 直接用
                // to_document_rect.w/h（与 compute_frame(ReflowMove) 第 481-494 行一致），
                // 不再误套 ReflowCrossFade 的四项插值。此处已跳过 ReflowCrossFade
                //（上方 `if unit.slice.kind == AnimatedSliceKind::ReflowCrossFade { continue; }`），
                // 一定是 ReflowMove，用共用 helper 按 kind 采 current rect。
                let current_rect = AnimatedSlice::sample_current_document_rect(
                    unit.slice.kind,
                    &anchor.from_document_rect,
                    &anchor.to_document_rect,
                    visible,
                );
                let target_rect = target_hit.doc_rect.clone();
                // (current_rect, target_rect)
                anchor_rebinds.push((current_rect, target_rect));
            }
            if !all_anchors_ok {
                decisions[i] = RebindDecision::Remove;
                continue;
            }
            // 计算各 anchor 的 movement vector (dx, dy) = target_rect - current_rect。
            let anchor_vectors: Vec<(f64, f64)> = anchor_rebinds
                .iter()
                .map(|(current, target)| (target.x - current.x, target.y - current.y))
                .collect();
            // 判断所有 anchor 的 movement vector 是否一致（容差 0.5）。
            let mut vectors_consistent = true;
            if anchor_vectors.len() > 1 {
                let (ref_dx, ref_dy) = anchor_vectors[0];
                for &(dx, dy) in &anchor_vectors[1..] {
                    if (dx - ref_dx).abs() > 0.5 || (dy - ref_dy).abs() > 0.5 {
                        vectors_consistent = false;
                        break;
                    }
                }
            }
            if vectors_consistent {
                // 整体重绑为 merged unit，逐 anchor 保留自己的 (current_rect, target_rect)。
                let union_current = anchor_rebinds
                    .iter()
                    .map(|(current, _)| current.clone())
                    .reduce(|acc, current| union_rect(&acc, &current))
                    .unwrap_or_else(|| unit.slice.from_document_rect.clone());
                let union_target = anchor_rebinds
                    .iter()
                    .map(|(_, target)| target.clone())
                    .reduce(|acc, target| union_rect(&acc, &target))
                    .unwrap_or_else(|| unit.slice.to_document_rect.clone());
                if rects_approx_equal(&unit.slice.to_document_rect, &union_target)
                    && rects_approx_equal(&unit.slice.from_document_rect, &union_current)
                {
                    decisions[i] = RebindDecision::Keep;
                } else {
                    decisions[i] = RebindDecision::RebindMerged {
                        union_current,
                        union_target,
                        anchor_rebinds: anchor_rebinds.clone(),
                    };
                }
            } else {
                // Issue #738 评论 5792244119 问题 2 + 5794018647: movement vector 不一致
                // → Split 拆回多个 PreparedVisualUnit，from = current_rect，to = target_rect。
                let replacement_units = build_split_replacement_units(unit, &anchor_rebinds, now);
                if replacement_units.is_empty() {
                    decisions[i] = RebindDecision::Remove;
                } else {
                    decisions[i] = RebindDecision::Split(replacement_units);
                }
            }
        }

        // Issue #738 评论 5793319451 问题2: CrossFade 多对多 group 重绑。
        // 以 group 为单位处理：old/new 任一侧为空 → 整组所有成员一起 Remove；
        // 逐个检查所有 new side 的 OffsetMap + shaping identity，任一失效 → 整组 Remove；
        // 全部有效 → 同一个 now 采样，所有 old fade-out（更新 to 跟随新目标几何），
        // 所有 new 绑定各自当前 canonical target fade-in。
        // Issue #738 评论 5792244119 问题 3 + 5794018647: group 管"活不活"，unit 管"画到哪"。
        // 缺 side 的 group 意味着 group 不完整，整组交给 canonical 接管。
        for (_gid, group) in &crossfade_groups {
            // old 或 new 任一侧为空 → 整组所有成员一起 Remove。
            if group.old_indices.is_empty() || group.new_indices.is_empty() {
                for &oi in &group.old_indices {
                    decisions[oi] = RebindDecision::Remove;
                }
                for &ni in &group.new_indices {
                    decisions[ni] = RebindDecision::Remove;
                }
                continue;
            }
            // 逐 new side 校验 shaping 并算出每个 unit 逐 anchor 的 (current_rect, target_rect)。
            // Issue #738 评论 5795183758 问题3: 不再把 merged unit 的总 frame/总 target 写给
            // 所有 anchors，逐 anchor 保留各自 current_rect/target_rect。各 anchor movement
            // vector 不一致时拆成多个 CrossFade units（保留 crossfade_group_id/crossfade_side）。
            let mut all_new_side_ok = true;
            let mut new_side_rebinds: Vec<(usize, Vec<(SourceRect, SourceRect)>, SourceRect)> =
                Vec::new();
            for &ni in &group.new_indices {
                let new_unit = &self.units[ni];
                let visible = new_unit.current_visible_fraction(now);
                let mut unit_anchor_rebinds: Vec<(SourceRect, SourceRect)> = Vec::new();
                if new_unit.slice.reflow_anchors.is_empty() {
                    // 无 reflow_anchors：用整体 byte range 的 hits union 作为 fallback，
                    // 构造单个 (current_rect, target_rect)。
                    let (ms, me) = match offset_map
                        .map_old_range_to_new(new_unit.slice.byte_start, new_unit.slice.byte_end)
                    {
                        Some(r) => r,
                        None => {
                            all_new_side_ok = false;
                            break;
                        }
                    };
                    let all_hits = find_clusters_in_canonical(canonical_snapshot, ms, me);
                    if all_hits.is_empty() {
                        all_new_side_ok = false;
                        break;
                    }
                    let target_rect = union_of_hits(&all_hits, &new_unit.slice.to_document_rect);
                    // Issue #738 评论 5795950264 问题2: w/h 按 visible 插值。
                    let current_rect = SourceRect {
                        x: new_unit.slice.from_document_rect.x
                            + (new_unit.slice.to_document_rect.x
                                - new_unit.slice.from_document_rect.x)
                                * visible,
                        y: new_unit.slice.from_document_rect.y
                            + (new_unit.slice.to_document_rect.y
                                - new_unit.slice.from_document_rect.y)
                                * visible,
                        w: new_unit.slice.from_document_rect.w
                            + (new_unit.slice.to_document_rect.w
                                - new_unit.slice.from_document_rect.w)
                                * visible,
                        h: new_unit.slice.from_document_rect.h
                            + (new_unit.slice.to_document_rect.h
                                - new_unit.slice.from_document_rect.h)
                                * visible,
                    };
                    unit_anchor_rebinds.push((current_rect, target_rect));
                } else {
                    for anchor in &new_unit.slice.reflow_anchors {
                        let (mapped_start, mapped_end) = match offset_map
                            .map_old_range_to_new(anchor.byte_start, anchor.byte_end)
                        {
                            Some(r) => r,
                            None => {
                                all_new_side_ok = false;
                                break;
                            }
                        };
                        let hits = find_clusters_in_canonical(
                            canonical_snapshot,
                            mapped_start,
                            mapped_end,
                        );
                        if hits.is_empty() {
                            all_new_side_ok = false;
                            break;
                        }
                        // 校验 new side anchor shaping identity（与 ReflowMove 对称）。
                        let mut anchor_shaping_match = false;
                        let mut matched_hit: Option<&CanonicalClusterHit> = None;
                        for hit in &hits {
                            if let Some(sid) = &anchor.shaping_identity {
                                if sid.is_same_shaping(&hit.shaping) {
                                    anchor_shaping_match = true;
                                    matched_hit = Some(hit);
                                    break;
                                }
                            }
                        }
                        if !anchor_shaping_match {
                            all_new_side_ok = false;
                            break;
                        }
                        let target_hit = match matched_hit {
                            Some(h) => h,
                            None => {
                                all_new_side_ok = false;
                                break;
                            }
                        };
                        // Issue #738 评论 5795183758 问题3: 逐 anchor 算 current_rect（旧
                        // from→to 按 visible 采样）和 target_rect（新 canonical cluster
                        // doc_rect），不再合并成 union 抹平逐 cluster 几何。
                        // Issue #738 评论 5795950264 问题2: w/h 按 visible 插值。
                        let current_rect = SourceRect {
                            x: anchor.from_document_rect.x
                                + (anchor.to_document_rect.x - anchor.from_document_rect.x)
                                    * visible,
                            y: anchor.from_document_rect.y
                                + (anchor.to_document_rect.y - anchor.from_document_rect.y)
                                    * visible,
                            w: anchor.from_document_rect.w
                                + (anchor.to_document_rect.w - anchor.from_document_rect.w)
                                    * visible,
                            h: anchor.from_document_rect.h
                                + (anchor.to_document_rect.h - anchor.from_document_rect.h)
                                    * visible,
                        };
                        let target_rect = target_hit.doc_rect.clone();
                        unit_anchor_rebinds.push((current_rect, target_rect));
                    }
                }
                if !all_new_side_ok {
                    break;
                }
                // target union 用于 old side 跟随和判断是否需要重绑。
                let target_union = unit_anchor_rebinds
                    .iter()
                    .map(|(_, target)| target.clone())
                    .reduce(|acc, target| union_rect(&acc, &target))
                    .unwrap_or_else(|| new_unit.slice.to_document_rect.clone());
                new_side_rebinds.push((ni, unit_anchor_rebinds, target_union));
            }
            if !all_new_side_ok {
                // 任一 new side anchor 失效：整组所有成员一起 Remove，交给 canonical。
                for &oi in &group.old_indices {
                    decisions[oi] = RebindDecision::Remove;
                }
                for &ni in &group.new_indices {
                    decisions[ni] = RebindDecision::Remove;
                }
                continue;
            }
            // 全部有效：每个 unit 用自己的 anchor_rebinds 重绑或拆分。
            // Issue #738 评论 5795950264 问题3: 记录 group 是否 Split，Split 时 old side 原位 fade-out。
            let mut group_has_split = false;
            for (ni, anchor_rebinds, target_union) in &new_side_rebinds {
                // Issue #738 评论 5795183758 问题3: 算各 anchor 的 movement vector，
                // 不一致时拆成多个 CrossFade units（保留 crossfade_group_id/crossfade_side）。
                let anchor_vectors: Vec<(f64, f64)> = anchor_rebinds
                    .iter()
                    .map(|(current, target)| (target.x - current.x, target.y - current.y))
                    .collect();
                let mut vectors_consistent = true;
                if anchor_vectors.len() > 1 {
                    let (ref_dx, ref_dy) = anchor_vectors[0];
                    for &(dx, dy) in &anchor_vectors[1..] {
                        if (dx - ref_dx).abs() > 0.5 || (dy - ref_dy).abs() > 0.5 {
                            vectors_consistent = false;
                            break;
                        }
                    }
                }
                if vectors_consistent {
                    if !rects_approx_equal(&self.units[*ni].slice.to_document_rect, target_union) {
                        decisions[*ni] = RebindDecision::RebindNewSide {
                            anchor_rebinds: anchor_rebinds.clone(),
                        };
                    } else {
                        decisions[*ni] = RebindDecision::Keep;
                    }
                } else {
                    // movement vector 不一致 → Split 拆成多个 CrossFade units，
                    // 保留 crossfade_group_id 和 crossfade_side，group 生命周期仍一起管理。
                    // Issue #738 评论 5795950264 问题2: 接住当前帧 opacity 避免拆分闪烁。
                    let split_unit = &self.units[*ni];
                    let split_visible = split_unit.current_visible_fraction(now);
                    let current_opacity = split_unit.slice.compute_frame(split_visible).opacity;
                    let replacement_units = build_crossfade_split_replacement_units(
                        split_unit,
                        anchor_rebinds,
                        now,
                        current_opacity,
                    );
                    if replacement_units.is_empty() {
                        decisions[*ni] = RebindDecision::Remove;
                    } else {
                        decisions[*ni] = RebindDecision::Split(replacement_units);
                        // Issue #738 评论 5795950264 问题3: 标记 Split。
                        group_has_split = true;
                    }
                }
            }
            // old side: 一对一跟随 new target；多对多/Split 原位 fade-out。
            // Issue #738 评论 5795950264 问题3: Split 时 old side 原位 fade-out。
            if !group_has_split && group.old_indices.len() == 1 && group.new_indices.len() == 1 {
                let target = &new_side_rebinds[0].2;
                for &oi in &group.old_indices {
                    if !rects_approx_equal(&self.units[oi].slice.to_document_rect, target) {
                        decisions[oi] = RebindDecision::RebindOldSideFollow {
                            target: target.clone(),
                        };
                    } else {
                        decisions[oi] = RebindDecision::Keep;
                    }
                }
            } else {
                for &oi in &group.old_indices {
                    decisions[oi] = RebindDecision::RebindOldSideInPlace;
                }
            }
        }

        // 应用 Rebind：逐 anchor 保留自己的几何（5794018647）。
        for (i, decision) in decisions.iter().enumerate() {
            match decision {
                RebindDecision::RebindNewSide { anchor_rebinds } => {
                    let unit = &mut self.units[i];
                    let visible = unit.current_visible_fraction(now);
                    let frame = unit.slice.compute_frame(visible);
                    // Issue #738 评论 5795183758 问题2: CrossFade 重绑只接了几何没有接住
                    // 当前透明度。compute_frame 在 ReflowCrossFade 分支返回 frame.opacity =
                    // opacity_from + (opacity_to - opacity_from) * visible，即当前帧插值
                    // 透明度。重绑后必须把 opacity_from 设为当前帧透明度，否则动画会从旧
                    // opacity（New=0.0）重新开始导致闪烁。opacity_to 保持 New=1（fade-in
                    // 终态）不变。AnimatedSliceFrame 无 scale 字段，故无需处理 scale_from。
                    unit.slice.opacity_from = frame.opacity;
                    // Issue #738 评论 5795183758 问题3: 逐 anchor 保留各自 current_rect/
                    // target_rect，不再把 merged unit 的总 frame/总 target 写给所有 anchors。
                    // slice 的 from/to 用所有 anchor 的 union 作为 merged unit 渲染代表几何。
                    let union_current = anchor_rebinds
                        .iter()
                        .map(|(current, _)| current.clone())
                        .reduce(|acc, current| union_rect(&acc, &current))
                        .unwrap_or_else(|| unit.slice.from_document_rect.clone());
                    let union_target = anchor_rebinds
                        .iter()
                        .map(|(_, target)| target.clone())
                        .reduce(|acc, target| union_rect(&acc, &target))
                        .unwrap_or_else(|| unit.slice.to_document_rect.clone());
                    unit.slice.from_document_rect = union_current.clone();
                    unit.slice.to_document_rect = union_target.clone();
                    unit.slice.static_hidden_document_rects = vec![union_target.clone()];
                    for (k, anchor) in unit.slice.reflow_anchors.iter_mut().enumerate() {
                        if let Some((current_rect, target_rect)) = anchor_rebinds.get(k) {
                            anchor.from_document_rect = current_rect.clone();
                            anchor.to_document_rect = target_rect.clone();
                        }
                    }
                    reset_timing(&mut unit.timing, now);
                }
                RebindDecision::RebindMerged {
                    union_current,
                    union_target,
                    anchor_rebinds,
                } => {
                    let unit = &mut self.units[i];
                    unit.slice.from_document_rect = union_current.clone();
                    unit.slice.to_document_rect = union_target.clone();
                    unit.slice.static_hidden_document_rects = vec![union_target.clone()];
                    for (k, anchor) in unit.slice.reflow_anchors.iter_mut().enumerate() {
                        if let Some((current_rect, target_rect)) = anchor_rebinds.get(k) {
                            anchor.from_document_rect = current_rect.clone();
                            anchor.to_document_rect = target_rect.clone();
                        }
                    }
                    reset_timing(&mut unit.timing, now);
                }
                RebindDecision::RebindOldSideFollow { target } => {
                    apply_old_side_rebind(&mut self.units[i], Some(target), now);
                }
                RebindDecision::RebindOldSideInPlace => {
                    apply_old_side_rebind(&mut self.units[i], None, now);
                }
                _ => {}
            }
        }

        // 移除 Remove 的 unit 并应用 Split（倒序保持索引稳定）。
        for i in (0..self.units.len()).rev() {
            match decisions.get(i) {
                Some(RebindDecision::Remove) => {
                    self.units.remove(i);
                }
                Some(RebindDecision::Split(replacement_units)) => {
                    let replacements = replacement_units.clone();
                    self.units.remove(i);
                    for new_unit in replacements {
                        self.units.insert(i, new_unit);
                    }
                }
                _ => {}
            }
        }

        // Issue #738 评论 5795950264 问题1: 只有当 CaretDriven 已 retire
        //（caret_motion_retired == true）或本事务没有 CaretDriven units 时才提升
        // layout_basis_revision。否则保持旧值，让 basis 守卫
        //（build_text_animation_plan_with_sample / find_cursor_transaction_for_target）
        // 继续 retire 旧 CaretDriven，防止旧 caret track 重新拿到 ownership 在新
        // canonical 上继续用旧布局几何。reconcile_active_transactions_with_canonical
        // 已在 rebind 之前先调 retire_caret_driven_units_for_transaction，所以含
        // CaretDriven 的事务进到这里时 caret_motion_retired 通常已是 true；此条件
        // 主要防御 retire 未覆盖的路径（如直接调 rebind 的测试/迁移代码）。
        if self.caret_motion_retired || !self.has_caret_driven_units() {
            self.layout_basis_revision = current_layout_revision;
        }
    }
}

/// Issue #738 评论 5792244119 问题 2: 为 ReflowMove merged unit 构造拆分后的
/// replacement PreparedVisualUnit 列表。
///
/// 当各 anchor 的 movement vector 不一致（拆行/目标不连续）时，逐 anchor 构造独立
/// AnimatedSlice（ReflowMove），每个保留自己的 snapshot_id/source_rect/from rect/
/// to rect，共享同一个 now 采样结果和 remaining duration。
///
/// `anchor_rebinds[i]` = (current_rect, target_rect)，与 reflow_anchors[i] 一一对应。
/// Issue #738 评论 5793319451 问题3A: `now` 由外层显式传入，共用同一时间采样。
fn build_split_replacement_units(
    unit: &PreparedVisualUnit,
    anchor_rebinds: &[(SourceRect, SourceRect)],
    now: Instant,
) -> Vec<PreparedVisualUnit> {
    if unit.slice.reflow_anchors.is_empty() || anchor_rebinds.is_empty() {
        return Vec::new();
    }
    let remaining_duration_ms = match &unit.timing {
        VisualUnitTiming::Timed {
            started_at,
            duration_ms,
            ..
        } => {
            let remaining = match *started_at {
                Some(start) => {
                    duration_ms.saturating_sub(now.duration_since(start).as_millis() as u64)
                }
                None => *duration_ms,
            };
            remaining.max(1)
        }
        _ => 1,
    };
    let mut replacements: Vec<PreparedVisualUnit> = Vec::new();
    for (anchor_idx, anchor) in unit.slice.reflow_anchors.iter().enumerate() {
        if anchor_idx >= anchor_rebinds.len() {
            break;
        }
        // Issue #738 评论 5794018647: anchor_rebinds[i] = (current_rect, target_rect)。
        // current_rect 已是旧 from→to 按 visible 采样的当前帧，不再内部 from + (to - from) * current_visible 插值。
        let (current_rect, target_rect) = &anchor_rebinds[anchor_idx];
        // 逐 anchor 构造独立 ReflowMove slice，保留自己的 snapshot_id/source_rect。
        // Issue #738 评论 5795183758 问题1: replacement slice 和它内部唯一 anchor
        // 必须从创建那一刻就是同一 layout basis。旧代码直接把 anchor.clone() 放进
        // reflow_anchors vec，其 from_document_rect/to_document_rect 仍是 Split 之前
        // 那一代 basis，下一次 rebind 会从 stale anchor 的旧 from/to 采 current_rect
        // 导致二次 reconcile 跳变。
        // 修复：先 clone anchor，再把 from/to 显式覆盖成 current_rect/target_rect，
        // 与 new_slice 的 from_document_rect/to_document_rect 保持同一 basis。
        let mut new_anchor = anchor.clone();
        new_anchor.from_document_rect = current_rect.clone();
        new_anchor.to_document_rect = target_rect.clone();
        let new_slice = AnimatedSlice {
            kind: AnimatedSliceKind::ReflowMove,
            snapshot_id: anchor.snapshot_id,
            source_rect: anchor.source_rect.clone(),
            from_document_rect: current_rect.clone(),
            to_document_rect: target_rect.clone(),
            opacity_from: unit.slice.opacity_from,
            opacity_to: unit.slice.opacity_to,
            scale_from: unit.slice.scale_from,
            scale_to: unit.slice.scale_to,
            byte_start: anchor.byte_start,
            byte_end: anchor.byte_end,
            shaping_identity: anchor.shaping_identity.clone(),
            conceal_to_left_edge: unit.slice.conceal_to_left_edge,
            visual_line_id: anchor.visual_line_id,
            start_fraction: 0.0,
            static_hidden_document_rects: vec![target_rect.clone()],
            crossfade_group_id: None,
            crossfade_side: None,
            reflow_anchors: vec![new_anchor], // Split 后每个 replacement 只有一个 anchor，与 slice 同 basis
        };
        let mut new_unit = PreparedVisualUnit::wrap(new_slice, remaining_duration_ms);
        if let VisualUnitTiming::Timed {
            start_fraction,
            started_at,
            duration_ms,
            ..
        } = &mut new_unit.timing
        {
            *start_fraction = 0.0;
            *started_at = Some(now);
            *duration_ms = remaining_duration_ms;
        }
        replacements.push(new_unit);
    }
    replacements
}

/// Issue #738 评论 5795183758 问题3: CrossFade new side merged unit 各 anchor movement
/// vector 不一致时，逐 anchor 构造独立 ReflowCrossFade replacement PreparedVisualUnit。
///
/// 与 `build_split_replacement_units` 对称，但保留 `ReflowCrossFade` kind 和原 unit 的
/// `crossfade_group_id`/`crossfade_side`，使拆分后的多个 new side units 仍属于同一
/// CrossFade group，group 生命周期一起管理。`opacity_to` 保留原值（New side: 1 淡入终态），
/// `opacity_from` 用 `current_opacity`（当前帧透明度）接住当前屏幕透明度，避免拆分第一帧闪烁。
///
/// `anchor_rebinds[i]` = (current_rect, target_rect)，与 reflow_anchors[i] 一一对应。
/// Issue #738 评论 5795950264 问题2: `current_opacity` 由外层用同一 visible/now 采样
/// `unit.slice.compute_frame(visible).opacity` 算出传入。
fn build_crossfade_split_replacement_units(
    unit: &PreparedVisualUnit,
    anchor_rebinds: &[(SourceRect, SourceRect)],
    now: Instant,
    current_opacity: f64,
) -> Vec<PreparedVisualUnit> {
    if unit.slice.reflow_anchors.is_empty() || anchor_rebinds.is_empty() {
        return Vec::new();
    }
    let remaining_duration_ms = match &unit.timing {
        VisualUnitTiming::Timed {
            started_at,
            duration_ms,
            ..
        } => {
            let remaining = match *started_at {
                Some(start) => {
                    duration_ms.saturating_sub(now.duration_since(start).as_millis() as u64)
                }
                None => *duration_ms,
            };
            remaining.max(1)
        }
        _ => 1,
    };
    let mut replacements: Vec<PreparedVisualUnit> = Vec::new();
    for (anchor_idx, anchor) in unit.slice.reflow_anchors.iter().enumerate() {
        if anchor_idx >= anchor_rebinds.len() {
            break;
        }
        let (current_rect, target_rect) = &anchor_rebinds[anchor_idx];
        // 逐 anchor 构造独立 ReflowCrossFade slice，保留 crossfade_group_id/crossfade_side。
        // replacement slice 和它内部唯一 anchor 同一 layout basis（5795183758 问题1 对称修复）。
        let mut new_anchor = anchor.clone();
        new_anchor.from_document_rect = current_rect.clone();
        new_anchor.to_document_rect = target_rect.clone();
        let new_slice = AnimatedSlice {
            kind: AnimatedSliceKind::ReflowCrossFade,
            snapshot_id: anchor.snapshot_id,
            source_rect: anchor.source_rect.clone(),
            from_document_rect: current_rect.clone(),
            to_document_rect: target_rect.clone(),
            // Issue #738 评论 5795950264 问题2: opacity_from 用当前帧透明度接住，
            // 不再用 unit.slice.opacity_from（New side 原值 0.0），避免拆分第一帧闪。
            opacity_from: current_opacity,
            opacity_to: unit.slice.opacity_to,
            scale_from: unit.slice.scale_from,
            scale_to: unit.slice.scale_to,
            byte_start: anchor.byte_start,
            byte_end: anchor.byte_end,
            shaping_identity: anchor.shaping_identity.clone(),
            conceal_to_left_edge: unit.slice.conceal_to_left_edge,
            visual_line_id: anchor.visual_line_id,
            start_fraction: 0.0,
            static_hidden_document_rects: vec![target_rect.clone()],
            crossfade_group_id: unit.slice.crossfade_group_id,
            crossfade_side: unit.slice.crossfade_side,
            reflow_anchors: vec![new_anchor],
        };
        let mut new_unit = PreparedVisualUnit::wrap(new_slice, remaining_duration_ms);
        if let VisualUnitTiming::Timed {
            start_fraction,
            started_at,
            duration_ms,
            ..
        } = &mut new_unit.timing
        {
            *start_fraction = 0.0;
            *started_at = Some(now);
            *duration_ms = remaining_duration_ms;
        }
        replacements.push(new_unit);
    }
    replacements
}

/// Issue #738 评论 5794018647: 重置 unit timing，算 remaining duration 并重置
/// start_fraction/started_at/duration。所有 Rebind 变体共用同一时间线语义。
fn reset_timing(timing: &mut VisualUnitTiming, now: Instant) {
    if let VisualUnitTiming::Timed {
        start_fraction,
        started_at,
        duration_ms,
        ..
    } = timing
    {
        let remaining = match *started_at {
            Some(start) => duration_ms.saturating_sub(now.duration_since(start).as_millis() as u64),
            None => *duration_ms,
        };
        *start_fraction = 0.0;
        *started_at = Some(now);
        *duration_ms = remaining.max(1);
    }
}

/// Issue #738 评论 5794018647: CrossFade old side 重绑辅助。target = Some 时跟随
/// new target，None 时原位 fade-out（from=to=当前帧）。不更新 static_hidden。
fn apply_old_side_rebind(unit: &mut PreparedVisualUnit, target: Option<&SourceRect>, now: Instant) {
    let visible = unit.current_visible_fraction(now);
    let frame = unit.slice.compute_frame(visible);
    let frame_rect = SourceRect {
        x: frame.x,
        y: frame.y,
        w: frame.w,
        h: frame.h,
    };
    unit.slice.from_document_rect = frame_rect.clone();
    // Issue #738 评论 5795183758 问题2: CrossFade 重绑只接了几何没有接住当前透明度。
    // compute_frame 在 ReflowCrossFade 分支返回 frame.opacity = opacity_from +
    // (opacity_to - opacity_from) * visible（animated_slice.rs:505），即当前帧插值透明度。
    // 重绑后必须把 opacity_from 设为当前帧透明度，否则动画会从旧 opacity（Old=1.0/New=0.0）
    // 重新开始导致闪烁。opacity_to 保持 Old=0/New=1（fade-out/fade-in 终态）不变。
    // AnimatedSliceFrame 无 scale 字段，故无需处理 scale_from。
    unit.slice.opacity_from = frame.opacity;
    let to = match target {
        Some(t) => t.clone(),
        None => frame_rect.clone(),
    };
    unit.slice.to_document_rect = to.clone();
    for anchor in &mut unit.slice.reflow_anchors {
        anchor.from_document_rect = frame_rect.clone();
        anchor.to_document_rect = to.clone();
    }
    reset_timing(&mut unit.timing, now);
}

/// Issue #738 评论 5789470425 问题2: 把 find_clusters_in_canonical 返回的 hits 合成
/// 代表 to_rect（各 cluster doc_rect 的 union）。逐 cluster 真相在 hits 里，
/// 此 union 仅供 merged unit 渲染插值的代表几何。
fn union_of_hits(hits: &[CanonicalClusterHit], fallback: &SourceRect) -> SourceRect {
    if hits.is_empty() {
        return fallback.clone();
    }
    let mut acc = hits[0].doc_rect.clone();
    for hit in &hits[1..] {
        acc = union_rect(&acc, &hit.doc_rect);
    }
    acc
}

/// Issue #738 评论 5789470425 问题2: 两个 SourceRect 的 union。
fn union_rect(a: &SourceRect, b: &SourceRect) -> SourceRect {
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

/// Issue #738 评论 5787277777: 判断 AnimatedSliceKind 是否为 Timed Reflow
///（ReflowMove / ReflowCrossFade），即需要 rebind 到 canonical 的 unit 类型。
fn is_timed_reflow_kind(kind: AnimatedSliceKind) -> bool {
    matches!(
        kind,
        AnimatedSliceKind::ReflowMove | AnimatedSliceKind::ReflowCrossFade
    )
}

/// Issue #738 评论 5789470425 问题2: 在 canonical snapshot 里按 byte range 找所有相交 cluster，
/// 逐 cluster 返回（byte range、document rect、shaping identity），不做 bounding box，
/// 不只取第一个 cluster 的 shaping。
///
/// reconcile 时逐 anchor 调此函数，逐 cluster 校验 shaping；拆行或各 anchor 新移动
/// 向量不同时拆回多个 Timed unit，而不是做一个跨行 bounding rect。
#[derive(Clone, Debug)]
struct CanonicalClusterHit {
    byte_start: usize,
    byte_end: usize,
    doc_rect: SourceRect,
    shaping: ShapingIdentity,
}

fn find_clusters_in_canonical(
    snapshot: &CanonicalDocumentVisualSnapshot,
    byte_start: usize,
    byte_end: usize,
) -> Vec<CanonicalClusterHit> {
    let dpr = snapshot.dpr.max(0.001);
    let mut hits: Vec<CanonicalClusterHit> = Vec::new();
    for para in &snapshot.paragraphs {
        for line in &para.lines {
            for cluster in &line.clusters {
                // 相交判定：cluster 与 [byte_start, byte_end) 有重叠。
                if cluster.document_byte_start < byte_end && cluster.document_byte_end > byte_start
                {
                    let vline = snapshot.visual_lines.iter().find(|vl| {
                        vl.byte_start <= cluster.document_byte_start
                            && vl.byte_end >= cluster.document_byte_end
                    });
                    let (line_y, line_x) = match vline {
                        Some(vl) => (vl.y, vl.x),
                        None => (0.0, line.x_pos),
                    };
                    let doc_rect = SourceRect {
                        x: cluster.source_rect_x / dpr + line_x,
                        y: line_y + cluster.source_rect_y / dpr,
                        w: cluster.source_rect_w / dpr,
                        h: cluster.source_rect_h / dpr,
                    };
                    let shaping = ShapingIdentity {
                        text_content_hash: hash_str_for_shaping(&cluster.cluster_text),
                        raw_font_fingerprint: cluster.raw_font_fingerprint.clone(),
                        glyph_indexes_hash: hash_u32_for_shaping(&[cluster.first_glyph_index]),
                        cluster_glyph_count: cluster.glyph_count,
                        direction_rtl: cluster.is_rtl,
                        format_fingerprint: 0,
                    };
                    hits.push(CanonicalClusterHit {
                        byte_start: cluster.document_byte_start,
                        byte_end: cluster.document_byte_end,
                        doc_rect,
                        shaping,
                    });
                }
            }
        }
    }
    hits
}

/// Issue #738 评论 5787277777: 两个 SourceRect 是否在容差内相等（几何没变判断）。
fn rects_approx_equal(a: &SourceRect, b: &SourceRect) -> bool {
    const EPS: f64 = 0.5;
    (a.x - b.x).abs() < EPS
        && (a.y - b.y).abs() < EPS
        && (a.w - b.w).abs() < EPS
        && (a.h - b.h).abs() < EPS
}

/// Issue #738 评论 5787277777: 与 line_snapshot_builder 一致的哈希函数，
/// 用于从 CanonicalClusterSnapshot 构造 ShapingIdentity 做比较。
fn hash_str_for_shaping(data: &str) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    data.hash(&mut hasher);
    hasher.finish()
}

fn hash_u32_for_shaping(data: &[u32]) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    data.hash(&mut hasher);
    hasher.finish()
}
