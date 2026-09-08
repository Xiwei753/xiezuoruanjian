use super::composition::OffsetMap;
use super::rebase::{RebaseContinuation, RebaseReason, RebaseSliceMapping};
use super::visual::AnimatedSliceRole;

/// #639 评论 5420317382：判断旧 slice 角色是否属于"当前屏幕上已经可见的新出现文字"。
fn is_emergence_role(role: AnimatedSliceRole) -> bool {
    matches!(
        role,
        AnimatedSliceRole::Move | AnimatedSliceRole::Insert | AnimatedSliceRole::CrossfadeNew
    )
}

/// #639 评论 5421085782：对新事务中的 `CrossfadeOld + CrossfadeNew` pair 建索引。
fn build_crossfade_pair_index(
    new_slice_roles: &[AnimatedSliceRole],
    new_slice_byte_ranges: &[(usize, usize)],
    offset_map: Option<&OffsetMap>,
) -> std::collections::HashMap<(usize, usize), (usize, usize)> {
    use AnimatedSliceRole::*;
    let mut old_by_range: std::collections::HashMap<(usize, usize), usize> =
        std::collections::HashMap::new();
    let mut new_by_range: std::collections::HashMap<(usize, usize), usize> =
        std::collections::HashMap::new();
    for (idx, (role, &range)) in new_slice_roles
        .iter()
        .zip(new_slice_byte_ranges.iter())
        .enumerate()
    {
        match *role {
            CrossfadeOld => {
                old_by_range.entry(range).or_insert(idx);
            }
            CrossfadeNew => {
                new_by_range.entry(range).or_insert(idx);
            }
            _ => {}
        }
    }
    let mut pairs = std::collections::HashMap::new();
    for (&old_range, &old_idx) in old_by_range.iter() {
        let new_lookup_range = match offset_map {
            Some(map) => map
                .map_old_range_to_new(old_range.0, old_range.1)
                .unwrap_or(old_range),
            None => old_range,
        };
        if let Some(&new_idx) = new_by_range.get(&new_lookup_range) {
            pairs.insert(old_range, (old_idx, new_idx));
        }
    }
    pairs
}

/// #606: rebase slice 角色兼容性
fn compatible_rebase_roles(new_role: AnimatedSliceRole, old_role: AnimatedSliceRole) -> bool {
    use AnimatedSliceRole::*;
    matches!(
        (new_role, old_role),
        (Move, Move)
            | (Move, Insert)
            | (Move, CrossfadeNew)
            | (Insert, Move)
            | (Insert, Insert)
            | (Insert, CrossfadeNew)
            | (CrossfadeNew, CrossfadeNew)
            | (CrossfadeNew, Move)
            | (CrossfadeNew, Insert)
            | (Delete, Delete)
            | (Delete, CrossfadeOld)
            | (CrossfadeOld, CrossfadeOld)
            | (CrossfadeOld, Delete)
    )
}

/// 尝试为单个旧 slice 找到匹配的新 slice。
fn try_match_slice(
    old_role: AnimatedSliceRole,
    (old_start, old_end): (usize, usize),
    new_slice_roles: &[AnimatedSliceRole],
    new_slice_byte_ranges: &[(usize, usize)],
    used_new: &std::collections::HashSet<usize>,
    offset_map: Option<&OffsetMap>,
) -> Option<(usize, RebaseReason)> {
    for (new_idx, (new_role, &(new_start, new_end))) in new_slice_roles
        .iter()
        .zip(new_slice_byte_ranges.iter())
        .enumerate()
    {
        if used_new.contains(&new_idx) || !compatible_rebase_roles(*new_role, old_role) {
            continue;
        }
        if old_start == new_start && old_end == new_end {
            return Some((new_idx, RebaseReason::SameByteRange));
        }
        let Some(map) = offset_map else {
            continue;
        };
        if let Some((mapped_start, mapped_end)) = map.map_old_range_to_new(old_start, old_end) {
            if mapped_start == new_start && mapped_end == new_end {
                return Some((new_idx, RebaseReason::OffsetMapMatched));
            }
        }
    }
    None
}

/// #606: rebase slice 匹配输入
#[derive(Debug, Clone, Copy)]
pub struct SliceMatchInput<'a> {
    pub old_slice_roles: &'a [AnimatedSliceRole],
    pub old_slice_byte_ranges: &'a [(usize, usize)],
    pub new_slice_roles: &'a [AnimatedSliceRole],
    pub new_slice_byte_ranges: &'a [(usize, usize)],
    pub offset_map: Option<&'a OffsetMap>,
}

/// #606: 计算旧事务逻辑 slice → 新事务逻辑 slice 的对应关系。
pub fn compute_rebase_slice_mappings(input: SliceMatchInput) -> Vec<RebaseSliceMapping> {
    let mut mappings = Vec::new();
    let mut used_new = std::collections::HashSet::new();
    let crossfade_pairs = build_crossfade_pair_index(
        input.new_slice_roles,
        input.new_slice_byte_ranges,
        input.offset_map,
    );
    for (old_idx, (old_role, &(old_start, old_end))) in input
        .old_slice_roles
        .iter()
        .zip(input.old_slice_byte_ranges.iter())
        .enumerate()
    {
        let mut matched: Option<(usize, RebaseReason)> = None;
        if is_emergence_role(*old_role) {
            matched = crossfade_pairs
                .get(&(old_start, old_end))
                .filter(|&&(idx, _)| !used_new.contains(&idx))
                .map(|&(idx, _)| (idx, RebaseReason::SameByteRange));
        }
        if matched.is_none() {
            matched = try_match_slice(
                *old_role,
                (old_start, old_end),
                input.new_slice_roles,
                input.new_slice_byte_ranges,
                &used_new,
                input.offset_map,
            );
        }
        if let Some((new_idx, reason)) = matched {
            mappings.push(RebaseSliceMapping {
                old_slice_index: old_idx,
                new_slice_index: new_idx,
                continuation: RebaseContinuation::Continue,
                reason,
            });
            used_new.insert(new_idx);
        }
    }
    mappings
}
