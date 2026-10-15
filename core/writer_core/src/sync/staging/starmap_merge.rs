//! # 星图语义合并器
//!
//! 对星图集中式文件（`starmaps/index.json` 和 `starmaps/<id>/graph.json`）
//! 做按对象 ID 的三方集合合并，而不是整文件 LWW。
//!
//! ## 背景
//!
//! 星图业务对象被当作普通 JSON 做整文件 LWW 时，双端并发编辑会丢失对象身份。
//! 本模块让这两个集中式文件走按对象 ID 的三方集合合并，保留互不冲突的新增，
//! 依据删除事实处理删除。
//!
//! ## 职责
//!
//! - 路径识别：判断 rel_path 是否是星图内容文件
//! - [`StarMapIndexRecord`] 三方合并（[`merge_starmap_index`]）
//! - [`GraphMeta`] + 对象文件统一合并（[`merge_starmap_graph_with_objects`]）
//!
//! 合并器是纯计算（不读磁盘、不写磁盘），只接受 bytes 输入，返回 bytes 输出。
//! 错误显式返回，不静默吞错。

use std::collections::{HashMap, HashSet};

use crate::error::{Error, Result};
use crate::starmap::store::meta::{
    DeletedSinceLastSync, DeletionEntry, GraphMeta, CURRENT_SCHEMA_VERSION,
};
use crate::starmap::store::relation_index::{
    EdgeRelationIndex, EmbedHostIndex, HyperlinkRelationIndex, LinkRelationIndex,
};
use crate::starmap::StarMapIndexRecord;

// ===========================================================================
// 路径识别
// ===========================================================================

/// 把路径中的反斜杠归一化为正斜杠，便于跨平台匹配。
fn normalize_path(rel_path: &str) -> String {
    rel_path.replace('\\', "/")
}

/// 判断 rel_path 是否是星图全局索引文件 `starmaps/index.json`。
pub(crate) fn is_starmap_index_path(rel_path: &str) -> bool {
    normalize_path(rel_path) == "starmaps/index.json"
}

/// 判断 rel_path 是否是星图 graph.json 文件 `starmaps/<id>/graph.json`。
///
/// 匹配模式：三段路径，第一段 `starmaps`，第三段 `graph.json`，中间段非空。
pub(crate) fn is_starmap_graph_meta_path(rel_path: &str) -> bool {
    let normalized = normalize_path(rel_path);
    let parts: Vec<&str> = normalized.split('/').collect();
    parts.len() == 3 && parts[0] == "starmaps" && !parts[1].is_empty() && parts[2] == "graph.json"
}

/// 判断 rel_path 是否是星图 meta 文件 `starmaps/<id>.meta.json`。
///
/// 匹配模式：两段路径，第一段 `starmaps`，第二段以 `.meta.json` 结尾且 `<id>` 非空。
pub(crate) fn is_starmap_meta_path(rel_path: &str) -> bool {
    let normalized = normalize_path(rel_path);
    let parts: Vec<&str> = normalized.split('/').collect();
    parts.len() == 2
        && parts[0] == "starmaps"
        && parts[1].ends_with(".meta.json")
        && parts[1].len() > ".meta.json".len()
}

/// 判断字符是否是小写 hex 数字（`0-9a-f`）。
fn is_hex_digit(c: char) -> bool {
    matches!(c, '0'..='9' | 'a'..='f')
}

/// 判断 bucket 是否是有效的 hex bucket（2 个小写 hex 字符）。
///
/// 磁盘布局中 bucket = hex 高 4 bit（`00-0f`）。此处验证格式为 2 个 hex 字符，
/// 不限制 `00-0f` 范围——路径识别关注格式，bucket 合法性由其他逻辑验证。
fn is_valid_bucket(bucket: &str) -> bool {
    bucket.len() == 2 && bucket.chars().all(is_hex_digit)
}

/// 判断 rel_path 是否是星图对象文件
/// `starmaps/<id>/{nodes,edges,embeds,links,hyperlinks}/<bucket>/<obj_id>.json`。
pub(crate) fn is_starmap_object_file_path(rel_path: &str) -> bool {
    let normalized = normalize_path(rel_path);
    let parts: Vec<&str> = normalized.split('/').collect();
    parts.len() == 5
        && parts[0] == "starmaps"
        && !parts[1].is_empty()
        && matches!(
            parts[2],
            "nodes" | "edges" | "embeds" | "links" | "hyperlinks"
        )
        && is_valid_bucket(parts[3])
        && parts[4].ends_with(".json")
        && parts[4].len() > ".json".len()
}

/// 判断 rel_path 是否是任何星图内容文件。
///
/// 包括：index.json、graph.json、meta.json、对象文件。
pub(crate) fn is_starmap_content_path(rel_path: &str) -> bool {
    is_starmap_index_path(rel_path)
        || is_starmap_graph_meta_path(rel_path)
        || is_starmap_meta_path(rel_path)
        || is_starmap_object_file_path(rel_path)
}

// ===========================================================================
// 合并结果
// ===========================================================================

/// 星图语义合并结果。
#[derive(Debug, Clone)]
pub(crate) enum StarmapMergeResult {
    /// 合并后内容与 local 相同，无需写回（keep_local）。
    KeepLocal,
    /// 合并后内容与 incoming 相同，应用 incoming。
    ApplyIncoming(Vec<u8>),
    /// 合并产生新内容（三方都有变化但能语义合并），写回合并结果。
    ApplyMerged(Vec<u8>),
    /// 合并后内容与 base 相同且 local/incoming 都未改，无需操作（noop）。
    NoOp,
}

/// 对象级三方合并决策结果。
///
/// 用于星图对象文件（`nodes/edges/embeds/links/hyperlinks/<bucket>/<id>.json`）
/// 的按内容三方比较，替代整文件 LWW，识别双端同改冲突而非静默覆盖。
#[derive(Debug, Clone)]
pub(crate) enum ObjectMergeDecision {
    /// 保留本地版本（本地新增 / 本地修改 / 远端删除本地保留等）。
    KeepLocal,
    /// 应用远端版本（远端新增 / 远端修改 / 本地删除远端保留等）。
    ApplyIncoming(Vec<u8>),
    /// 无操作（双方相同，或双方都未改）。
    NoOp,
    /// 冲突：双侧同改且内容不同，或双侧新增不同内容。
    Conflict(crate::sync::types::SyncConflictKind),
    /// 双侧都删除（base 有，local 和 incoming 都不存在）。
    Delete,
}

/// 对象级三方合并：按内容比较 base/local/incoming，决定最终操作。
///
/// 纯计算，不读磁盘。`base`/`local`/`incoming` 各为 `Option<&[u8]>`
///（`None` = 文件不存在）。
///
/// # 决策规则
///
/// | base | local | incoming | 条件 | 决策 |
/// |------|-------|----------|------|------|
/// | None | None  | None     | —    | NoOp |
/// | None | Some  | None     | —    | KeepLocal（本地新增）|
/// | None | None  | Some     | —    | ApplyIncoming（远端新增）|
/// | None | Some  | Some     | l==i | NoOp |
/// | None | Some  | Some     | l!=i | Conflict(BothChanged)（双侧新增不同内容）|
/// | Some | None  | None     | —    | Delete（双侧都删）|
/// | Some | Some  | None     | l==b | Delete（远端删除，local 未改 → 接受删除）|
/// | Some | Some  | None     | l!=b | Conflict(RemoteDeleted)（删除-修改冲突）|
/// | Some | None  | Some     | i==b | Delete（本地删除，incoming 未改 → 接受删除）|
/// | Some | None  | Some     | i!=b | Conflict(RemoteDeleted)（删除-修改冲突）|
/// | Some | Some  | Some     | l==i | NoOp |
/// | Some | Some  | Some     | l==b, i!=b | ApplyIncoming |
/// | Some | Some  | Some     | l!=b, i==b | KeepLocal |
/// | Some | Some  | Some     | l!=b, i!=b, l!=i | Conflict(BothChanged) |
pub(crate) fn merge_starmap_object(
    base: Option<&[u8]>,
    local: Option<&[u8]>,
    incoming: Option<&[u8]>,
) -> ObjectMergeDecision {
    match (base, local, incoming) {
        // --- base 不存在 ---
        (None, None, None) => ObjectMergeDecision::NoOp,
        (None, Some(_), None) => ObjectMergeDecision::KeepLocal,
        (None, None, Some(iv)) => ObjectMergeDecision::ApplyIncoming(iv.to_vec()),
        (None, Some(lv), Some(iv)) => {
            if lv == iv {
                ObjectMergeDecision::NoOp
            } else {
                ObjectMergeDecision::Conflict(crate::sync::types::SyncConflictKind::BothChanged)
            }
        }
        // --- base 存在 ---
        (Some(_), None, None) => ObjectMergeDecision::Delete,
        // base 存在、local 存在、incoming 不存在（远端删除）：
        // - local == base（local 未改）→ 接受远端删除
        // - local != base（local 修改了）→ 删除-修改冲突
        (Some(bv), Some(lv), None) => {
            if lv == bv {
                ObjectMergeDecision::Delete
            } else {
                ObjectMergeDecision::Conflict(crate::sync::types::SyncConflictKind::RemoteDeleted)
            }
        }
        // base 存在、local 不存在、incoming 存在（本地删除）：
        // - incoming == base（incoming 未改）→ 接受本地删除
        // - incoming != base（incoming 修改了）→ 删除-修改冲突
        (Some(bv), None, Some(iv)) => {
            if iv == bv {
                ObjectMergeDecision::Delete
            } else {
                ObjectMergeDecision::Conflict(crate::sync::types::SyncConflictKind::RemoteDeleted)
            }
        }
        (Some(bv), Some(lv), Some(iv)) => {
            if lv == iv {
                ObjectMergeDecision::NoOp
            } else if lv == bv {
                // local 未改，incoming 改了 → ApplyIncoming
                ObjectMergeDecision::ApplyIncoming(iv.to_vec())
            } else if iv == bv {
                // incoming 未改，local 改了 → KeepLocal
                ObjectMergeDecision::KeepLocal
            } else {
                // 双端都改且内容不同 → 冲突
                ObjectMergeDecision::Conflict(crate::sync::types::SyncConflictKind::BothChanged)
            }
        }
    }
}

// ===========================================================================
// 三方集合合并辅助函数
// ===========================================================================

/// 三方集合合并：保留互不冲突的新增，依据删除事实处理删除。
///
/// `base`/`local`/`incoming` 各为 `Option<&Vec<String>>`（`None` = 不存在 = 空集）。
/// `local_modified`/`incoming_modified` 分别表示 local/incoming 端修改了的对象 ID
///（revision 与 base 不同）。返回合并后的 `Vec<String>`，保持插入顺序，去重。
///
/// # 三方删除判定
///
/// 对 `base` 中存在的 ID：
/// - 两侧都保留（local 和 incoming 都仍含此 ID）→ 保留
/// - 一侧删除、另一侧保留且**未修改**（不在 modified 集合中）→ 接受删除（不保留）
/// - 一侧删除、另一侧保留且**修改了**（在 modified 集合中）→ 保留对象（删除-修改冲突）
/// - 两侧都删除 → 删除
///
/// 对非 `base` 的新增 ID（local-only 或 incoming-only）：保留（互不冲突的新增）。
///
/// # 输出顺序
///
/// 先按 `base` 原始顺序输出保留的 base ID，再按 `local` 顺序输出 local 新增的，
/// 再按 `incoming` 顺序输出 incoming 新增的（去重）。
fn three_way_merge_id_set(
    base: Option<&Vec<String>>,
    local: Option<&Vec<String>>,
    incoming: Option<&Vec<String>>,
    local_modified: &HashSet<String>,
    incoming_modified: &HashSet<String>,
) -> Vec<String> {
    let base_vec: &[String] = base.map(|v| v.as_slice()).unwrap_or(&[]);
    let local_vec: &[String] = local.map(|v| v.as_slice()).unwrap_or(&[]);
    let incoming_vec: &[String] = incoming.map(|v| v.as_slice()).unwrap_or(&[]);

    let base_set: HashSet<&String> = base_vec.iter().collect();
    let local_set: HashSet<&String> = local_vec.iter().collect();
    let incoming_set: HashSet<&String> = incoming_vec.iter().collect();

    let mut result: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    // 1. 按 base 顺序处理 base 中的 ID：
    //    - 两侧都保留 → 保留
    //    - 一侧删除、另一侧保留且未修改 → 接受删除（不保留）
    //    - 一侧删除、另一侧保留且修改了 → 保留对象（删除-修改冲突）
    //    - 两侧都删除 → 不保留
    for id in base_vec {
        let in_local = local_set.contains(id);
        let in_incoming = incoming_set.contains(id);

        if in_local && in_incoming {
            // 两侧都保留 → 保留
            if seen.insert(id.clone()) {
                result.push(id.clone());
            }
        } else if in_local && !in_incoming {
            // incoming 删除，local 保留
            if local_modified.contains(id) && seen.insert(id.clone()) {
                // local 修改了该对象 → 删除-修改冲突，保留对象
                result.push(id.clone());
            }
            // local 未修改 → 接受删除（不保留）
        } else if !in_local && in_incoming {
            // local 删除，incoming 保留
            if incoming_modified.contains(id) && seen.insert(id.clone()) {
                // incoming 修改了该对象 → 删除-修改冲突，保留对象
                result.push(id.clone());
            }
            // incoming 未修改 → 接受删除（不保留）
        }
        // 两侧都删除 → 不保留
    }

    // 2. 按 local 顺序输出 local 新增的（local - base）
    for id in local_vec {
        if !base_set.contains(id) && seen.insert(id.clone()) {
            result.push(id.clone());
        }
    }

    // 3. 按 incoming 顺序输出 incoming 新增的（incoming - base）
    for id in incoming_vec {
        if !base_set.contains(id) && seen.insert(id.clone()) {
            result.push(id.clone());
        }
    }

    result
}

/// 合并 `main_starmap_by_project` 映射的单个 key 的值。
///
/// 返回 `Some(value)` 表示保留，`None` 表示删除。
fn merge_map_value(
    base: Option<&String>,
    local: Option<&String>,
    incoming: Option<&String>,
    local_updated_at: u64,
    incoming_updated_at: u64,
) -> Option<String> {
    match (base, local, incoming) {
        // --- base 没有 ---
        (None, None, None) => None,
        (None, Some(lv), None) => Some(lv.clone()),
        (None, None, Some(iv)) => Some(iv.clone()),
        (None, Some(lv), Some(iv)) => {
            pick_by_updated_at(lv, iv, local_updated_at, incoming_updated_at)
        }
        // --- base 有 ---
        (Some(_), None, None) => None,                 // 删除达成共识
        (Some(_), Some(lv), None) => Some(lv.clone()), // local 保留（删除未达成共识）
        (Some(_), None, Some(iv)) => Some(iv.clone()), // incoming 保留
        (Some(bv), Some(lv), Some(iv)) => {
            let local_changed = lv != bv;
            let incoming_changed = iv != bv;
            match (local_changed, incoming_changed) {
                (false, false) => Some(bv.clone()),
                (true, false) => Some(lv.clone()),
                (false, true) => Some(iv.clone()),
                (true, true) => pick_by_updated_at(lv, iv, local_updated_at, incoming_updated_at),
            }
        }
    }
}

/// 取 updated_at 较大方的值（同 updated_at 取 incoming）。
fn pick_by_updated_at(
    local: &str,
    incoming: &str,
    local_updated_at: u64,
    incoming_updated_at: u64,
) -> Option<String> {
    if local_updated_at > incoming_updated_at {
        Some(local.to_string())
    } else {
        Some(incoming.to_string())
    }
}

/// 合并 `main_starmap_by_project` 映射。
fn merge_main_starmap_by_project(
    base: &HashMap<String, String>,
    local: &HashMap<String, String>,
    incoming: &HashMap<String, String>,
    local_updated_at: u64,
    incoming_updated_at: u64,
) -> HashMap<String, String> {
    let mut all_keys: HashSet<&str> = HashSet::new();
    all_keys.extend(base.keys().map(String::as_str));
    all_keys.extend(local.keys().map(String::as_str));
    all_keys.extend(incoming.keys().map(String::as_str));

    let mut result: HashMap<String, String> = HashMap::new();
    for key in all_keys {
        let merged = merge_map_value(
            base.get(key),
            local.get(key),
            incoming.get(key),
            local_updated_at,
            incoming_updated_at,
        );
        if let Some(v) = merged {
            result.insert(key.to_string(), v);
        }
    }
    result
}

/// 合并单个对象 ID 的 revision 值。
fn merge_revision_value(
    base: Option<u64>,
    local: Option<u64>,
    incoming: Option<u64>,
) -> Option<u64> {
    match (base, local, incoming) {
        (None, None, None) => None,
        (None, Some(lv), None) => Some(lv),
        (None, None, Some(iv)) => Some(iv),
        (None, Some(lv), Some(iv)) => Some(lv.max(iv)),
        (Some(bv), None, None) => Some(bv),
        (Some(_), Some(lv), None) => Some(lv),
        (Some(_), None, Some(iv)) => Some(iv),
        (Some(bv), Some(lv), Some(iv)) => {
            let local_changed = lv != bv;
            let incoming_changed = iv != bv;
            match (local_changed, incoming_changed) {
                (false, false) => Some(bv),
                (true, false) => Some(lv),
                (false, true) => Some(iv),
                (true, true) => Some(lv.max(iv)),
            }
        }
    }
}

/// 合并 revisions HashMap，只保留 `member_ids` 中存在的对象。
fn merge_revisions(
    base: Option<&HashMap<String, u64>>,
    local: Option<&HashMap<String, u64>>,
    incoming: Option<&HashMap<String, u64>>,
    member_ids: &HashSet<String>,
) -> HashMap<String, u64> {
    let mut all_keys: HashSet<&String> = HashSet::new();
    if let Some(b) = base {
        all_keys.extend(b.keys());
    }
    if let Some(l) = local {
        all_keys.extend(l.keys());
    }
    if let Some(i) = incoming {
        all_keys.extend(i.keys());
    }

    let mut result: HashMap<String, u64> = HashMap::new();
    for key in all_keys {
        if !member_ids.contains(key) {
            continue;
        }
        let base_val = base.and_then(|m| m.get(key)).copied();
        let local_val = local.and_then(|m| m.get(key)).copied();
        let incoming_val = incoming.and_then(|m| m.get(key)).copied();
        if let Some(rev) = merge_revision_value(base_val, local_val, incoming_val) {
            result.insert(key.clone(), rev);
        }
    }
    result
}

/// 解析 starmap_id 并校验三方一致性。
fn resolve_starmap_id(
    base: Option<&str>,
    local: Option<&str>,
    incoming: Option<&str>,
) -> Result<String> {
    let ids: Vec<&str> = [base, local, incoming].into_iter().flatten().collect();
    if ids.is_empty() {
        return Ok(String::new());
    }
    let first = ids[0];
    for id in &ids[1..] {
        if *id != first {
            return Err(Error::Other(format!(
                "starmap_id mismatch: base={:?}, local={:?}, incoming={:?}",
                base, local, incoming
            )));
        }
    }
    Ok(first.to_string())
}

/// 合并后成员 ID 集合，用于 revisions 和 deletions 过滤。
pub(crate) struct MemberIdSets {
    node_ids: HashSet<String>,
    edge_ids: HashSet<String>,
    embed_ids: HashSet<String>,
    link_ids: HashSet<String>,
    hyperlink_ids: HashSet<String>,
}

/// 合并后的五个成员 ID 列表及其 HashSet 集合。
pub(crate) struct MergedMemberIds {
    node_ids: Vec<String>,
    edge_ids: Vec<String>,
    embed_ids: Vec<String>,
    link_ids: Vec<String>,
    hyperlink_ids: Vec<String>,
    sets: MemberIdSets,
}

/// 合并后的五个 revisions HashMap。
struct MergedRevisions {
    node: HashMap<String, u64>,
    edge: HashMap<String, u64>,
    embed: HashMap<String, u64>,
    link: HashMap<String, u64>,
    hyperlink: HashMap<String, u64>,
}

/// 判断 tombstone 对应的对象是否仍在成员表中。
fn is_object_still_exists(object_type: &str, object_id: &str, sets: &MemberIdSets) -> bool {
    match object_type {
        "node" => sets.node_ids.contains(object_id),
        "edge" => sets.edge_ids.contains(object_id),
        "embed" => sets.embed_ids.contains(object_id),
        "link" => sets.link_ids.contains(object_id),
        "hyperlink" => sets.hyperlink_ids.contains(object_id),
        _ => false, // 未知类型视为已删除，保留 tombstone（保守策略）
    }
}

/// 合并 `deleted_since_last_sync` tombstone。
///
/// 按 `(object_type, object_id)` 去重，取最大 `deleted_at_revision`。
///
/// # tombstone 保留语义
///
/// tombstone 记录删除事实，用于后续同步传播删除到其他设备。只有当对象确实在
/// 最终成员表中存在（即被重新创建）时才清理对应 tombstone；对象已删除（不在
/// 最终成员表中）则保留 tombstone 用于同步传播。
///
/// 这里"对象在最终成员表中存在"由 `member_sets`（合并后的成员 ID 集合）判断。
/// 由于 [`three_way_merge_id_set`] 已修复删除判定，被删除的对象不会复活到成员表，
/// 因此其 tombstone 会被正确保留。
fn merge_deletions(
    base: Option<&DeletedSinceLastSync>,
    local: Option<&DeletedSinceLastSync>,
    incoming: Option<&DeletedSinceLastSync>,
    member_sets: &MemberIdSets,
) -> DeletedSinceLastSync {
    // 按 (object_type, object_id) 去重，取最大 deleted_at_revision
    let mut tombstones: HashMap<(String, String), u64> = HashMap::new();
    for source in [base, local, incoming].into_iter().flatten() {
        for entry in &source.entries {
            let key = (entry.object_type.clone(), entry.object_id.clone());
            let current = tombstones.get(&key).copied().unwrap_or(0);
            if entry.deleted_at_revision > current {
                tombstones.insert(key, entry.deleted_at_revision);
            }
        }
    }

    // 对象被重新创建（在最终成员表中存在）→ 清理对应 tombstone；
    // 对象已删除（不在最终成员表中）→ 保留 tombstone 用于同步传播。
    let mut entries: Vec<DeletionEntry> = Vec::new();
    for ((object_type, object_id), deleted_at_revision) in tombstones {
        if !is_object_still_exists(&object_type, &object_id, member_sets) {
            entries.push(DeletionEntry {
                object_type,
                object_id,
                deleted_at_revision,
            });
        }
    }

    // 稳定排序，保证序列化确定性
    entries.sort_by(|a, b| {
        a.deleted_at_revision
            .cmp(&b.deleted_at_revision)
            .then_with(|| a.object_type.cmp(&b.object_type))
            .then_with(|| a.object_id.cmp(&b.object_id))
    });

    DeletedSinceLastSync { entries }
}

/// 根据合并后 bytes 与 base/local/incoming 的关系决定返回类型。
fn finalize_merge_result(
    merged_bytes: &[u8],
    base: Option<&[u8]>,
    local: Option<&[u8]>,
    incoming: Option<&[u8]>,
) -> StarmapMergeResult {
    if local.is_none() && incoming.is_none() {
        return StarmapMergeResult::NoOp;
    }

    if Some(merged_bytes) == local {
        if Some(merged_bytes) == incoming && Some(merged_bytes) == base {
            StarmapMergeResult::NoOp
        } else {
            StarmapMergeResult::KeepLocal
        }
    } else if Some(merged_bytes) == incoming {
        StarmapMergeResult::ApplyIncoming(merged_bytes.to_vec())
    } else {
        StarmapMergeResult::ApplyMerged(merged_bytes.to_vec())
    }
}

// ===========================================================================
// StarMapIndexRecord 三方合并
// ===========================================================================

/// 构造空 `StarMapIndexRecord`（文件不存在时的等价表示）。
fn empty_index_record() -> StarMapIndexRecord {
    StarMapIndexRecord {
        schema_version: 1,
        starmap_ids: vec![],
        root_starmap_ids: vec![],
        main_starmap_by_project: HashMap::new(),
        updated_at: 0,
    }
}

/// 解析 `StarMapIndexRecord`。`None` 表示文件不存在，等价于空记录。
fn parse_index(bytes: Option<&[u8]>) -> Result<StarMapIndexRecord> {
    match bytes {
        Some(b) => Ok(serde_json::from_slice(b)?),
        None => Ok(empty_index_record()),
    }
}

/// 过滤 ID 列表，只保留存在于 `existing` 集合中的 ID，保持顺序与去重。
///
/// 用于清理 `root_starmap_ids` 中指向已删除星图的 ID。
fn retain_ids_in_set(ids: Vec<String>, existing: &HashSet<String>) -> Vec<String> {
    let mut result: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for id in ids {
        if existing.contains(&id) && seen.insert(id.clone()) {
            result.push(id);
        }
    }
    result
}

/// 过滤 `main_starmap_by_project`，只保留 value 存在于 `existing` 集合中的映射。
///
/// 用于清理指向已删除星图的 project→starmap 映射，避免继续指向已删除图。
fn retain_main_by_existing(
    map: HashMap<String, String>,
    existing: &HashSet<String>,
) -> HashMap<String, String> {
    map.into_iter()
        .filter(|(_, v)| existing.contains(v))
        .collect()
}

/// `StarMapIndexRecord` 三方语义合并。
///
/// `base`/`local`/`incoming` 各为 `Option<&[u8]>`（`None` = 文件不存在 = 空集合）。
/// 反序列化失败返回 `Err`（不静默吞错）。
///
/// # 合并语义
///
/// - `starmap_ids`：三方集合合并（一侧删除一侧保留 base 原样 → 接受删除）
/// - `root_starmap_ids`：三方集合合并，再清理指向已删除星图的 ID
/// - `main_starmap_by_project`：按 project_id key 做三方变更判断，再清理指向
///   已删除星图的映射
/// - `schema_version`：取 `max(base, local, incoming)`
/// - `updated_at`：取 `max(local, incoming)`（base 不参与）
pub(crate) fn merge_starmap_index(
    base: Option<&[u8]>,
    local: Option<&[u8]>,
    incoming: Option<&[u8]>,
) -> Result<StarmapMergeResult> {
    let base_rec = parse_index(base)?;
    let local_rec = parse_index(local)?;
    let incoming_rec = parse_index(incoming)?;

    // StarMapIndexRecord 没有 revision 信息，无法区分"保留 base 原样"和"修改了"，
    // 因此传入空 modified 集合，保持原有行为（一侧删除一侧保留 base 原样 → 接受删除）。
    let empty_modified: HashSet<String> = HashSet::new();

    let merged_starmap_ids = three_way_merge_id_set(
        Some(&base_rec.starmap_ids),
        Some(&local_rec.starmap_ids),
        Some(&incoming_rec.starmap_ids),
        &empty_modified,
        &empty_modified,
    );
    // 最终存在的星图集合：合并后的 starmap_ids。
    // root_starmap_ids 和 main_starmap_by_project 中指向已删除图的映射需清理，
    // 避免继续指向已删除图。
    let existing_starmap_ids: HashSet<String> = merged_starmap_ids.iter().cloned().collect();

    let merged_root_starmap_ids = three_way_merge_id_set(
        Some(&base_rec.root_starmap_ids),
        Some(&local_rec.root_starmap_ids),
        Some(&incoming_rec.root_starmap_ids),
        &empty_modified,
        &empty_modified,
    );
    let merged_root_starmap_ids = retain_ids_in_set(merged_root_starmap_ids, &existing_starmap_ids);

    let merged_main = merge_main_starmap_by_project(
        &base_rec.main_starmap_by_project,
        &local_rec.main_starmap_by_project,
        &incoming_rec.main_starmap_by_project,
        local_rec.updated_at,
        incoming_rec.updated_at,
    );
    let merged_main = retain_main_by_existing(merged_main, &existing_starmap_ids);

    let merged = StarMapIndexRecord {
        schema_version: base_rec
            .schema_version
            .max(local_rec.schema_version)
            .max(incoming_rec.schema_version),
        starmap_ids: merged_starmap_ids,
        root_starmap_ids: merged_root_starmap_ids,
        main_starmap_by_project: merged_main,
        updated_at: local_rec.updated_at.max(incoming_rec.updated_at),
    };

    let merged_bytes = serde_json::to_vec_pretty(&merged)?;
    Ok(finalize_merge_result(&merged_bytes, base, local, incoming))
}

// ===========================================================================
// GraphMeta 三方合并
// ===========================================================================

/// 解析 `GraphMeta`。`None` 表示文件不存在。
fn parse_graph_meta(bytes: Option<&[u8]>) -> Result<Option<GraphMeta>> {
    match bytes {
        Some(b) => Ok(Some(serde_json::from_slice(b)?)),
        None => Ok(None),
    }
}

/// 从 `Option<&GraphMeta>` 提取字段引用为 `Option`。
fn meta_field_str(meta: Option<&GraphMeta>, f: fn(&GraphMeta) -> &str) -> &str {
    meta.map(f).unwrap_or("")
}

/// 解析并校验 graph meta 的 starmap_id 和 schema_version。
///
/// 返回 `(merged_starmap_id, merged_schema_version)`。
fn resolve_graph_meta_header(
    base_meta: Option<&GraphMeta>,
    local_meta: Option<&GraphMeta>,
    incoming_meta: Option<&GraphMeta>,
) -> Result<(String, String)> {
    let merged_starmap_id = resolve_starmap_id(
        base_meta.map(|m| m.starmap_id.as_str()),
        local_meta.map(|m| m.starmap_id.as_str()),
        incoming_meta.map(|m| m.starmap_id.as_str()),
    )?;

    let base_sv = meta_field_str(base_meta, |m| m.schema_version.as_str());
    let local_sv = meta_field_str(local_meta, |m| m.schema_version.as_str());
    let incoming_sv = meta_field_str(incoming_meta, |m| m.schema_version.as_str());

    if local_sv > CURRENT_SCHEMA_VERSION || incoming_sv > CURRENT_SCHEMA_VERSION {
        return Err(Error::UnsupportedVersion {
            version: std::cmp::max(local_sv, incoming_sv).to_string(),
        });
    }
    let merged_schema_version =
        std::cmp::max(base_sv, std::cmp::max(local_sv, incoming_sv)).to_string();

    Ok((merged_starmap_id, merged_schema_version))
}

/// 从 revisions HashMap 中提取修改了的对象 ID 集合（revision 与 base 不同）。
fn extract_modified_ids(
    base_revisions: Option<&HashMap<String, u64>>,
    side_revisions: Option<&HashMap<String, u64>>,
) -> HashSet<String> {
    let empty: HashMap<String, u64> = HashMap::new();
    let base_rev = base_revisions.unwrap_or(&empty);
    let side_rev = side_revisions.unwrap_or(&empty);
    let mut modified: HashSet<String> = HashSet::new();
    for (id, rev) in side_rev {
        let base_val = base_rev.get(id).copied();
        if base_val != Some(*rev) {
            modified.insert(id.clone());
        }
    }
    modified
}

/// 合并五个成员 ID 列表，返回合并后的 ID 和对应的 HashSet 集合。
///
/// 从 revisions 中提取 `local_modified`/`incoming_modified` 集合，传入
/// `three_way_merge_id_set` 处理删除-修改冲突。
fn merge_member_ids(
    base_meta: Option<&GraphMeta>,
    local_meta: Option<&GraphMeta>,
    incoming_meta: Option<&GraphMeta>,
) -> MergedMemberIds {
    // 提取各对象类型的 local_modified / incoming_modified 集合
    let node_local_mod = extract_modified_ids(
        base_meta.map(|m| &m.node_revisions),
        local_meta.map(|m| &m.node_revisions),
    );
    let node_incoming_mod = extract_modified_ids(
        base_meta.map(|m| &m.node_revisions),
        incoming_meta.map(|m| &m.node_revisions),
    );
    let edge_local_mod = extract_modified_ids(
        base_meta.map(|m| &m.edge_revisions),
        local_meta.map(|m| &m.edge_revisions),
    );
    let edge_incoming_mod = extract_modified_ids(
        base_meta.map(|m| &m.edge_revisions),
        incoming_meta.map(|m| &m.edge_revisions),
    );
    let embed_local_mod = extract_modified_ids(
        base_meta.map(|m| &m.embed_revisions),
        local_meta.map(|m| &m.embed_revisions),
    );
    let embed_incoming_mod = extract_modified_ids(
        base_meta.map(|m| &m.embed_revisions),
        incoming_meta.map(|m| &m.embed_revisions),
    );
    let link_local_mod = extract_modified_ids(
        base_meta.map(|m| &m.link_revisions),
        local_meta.map(|m| &m.link_revisions),
    );
    let link_incoming_mod = extract_modified_ids(
        base_meta.map(|m| &m.link_revisions),
        incoming_meta.map(|m| &m.link_revisions),
    );
    let hyperlink_local_mod = extract_modified_ids(
        base_meta.map(|m| &m.hyperlink_revisions),
        local_meta.map(|m| &m.hyperlink_revisions),
    );
    let hyperlink_incoming_mod = extract_modified_ids(
        base_meta.map(|m| &m.hyperlink_revisions),
        incoming_meta.map(|m| &m.hyperlink_revisions),
    );

    let node_ids = three_way_merge_id_set(
        base_meta.map(|m| &m.node_ids),
        local_meta.map(|m| &m.node_ids),
        incoming_meta.map(|m| &m.node_ids),
        &node_local_mod,
        &node_incoming_mod,
    );
    let edge_ids = three_way_merge_id_set(
        base_meta.map(|m| &m.edge_ids),
        local_meta.map(|m| &m.edge_ids),
        incoming_meta.map(|m| &m.edge_ids),
        &edge_local_mod,
        &edge_incoming_mod,
    );
    let embed_ids = three_way_merge_id_set(
        base_meta.map(|m| &m.embed_instance_ids),
        local_meta.map(|m| &m.embed_instance_ids),
        incoming_meta.map(|m| &m.embed_instance_ids),
        &embed_local_mod,
        &embed_incoming_mod,
    );
    let link_ids = three_way_merge_id_set(
        base_meta.map(|m| &m.link_ids),
        local_meta.map(|m| &m.link_ids),
        incoming_meta.map(|m| &m.link_ids),
        &link_local_mod,
        &link_incoming_mod,
    );
    let hyperlink_ids = three_way_merge_id_set(
        base_meta.map(|m| &m.hyperlink_ids),
        local_meta.map(|m| &m.hyperlink_ids),
        incoming_meta.map(|m| &m.hyperlink_ids),
        &hyperlink_local_mod,
        &hyperlink_incoming_mod,
    );

    let sets = MemberIdSets {
        node_ids: node_ids.iter().cloned().collect(),
        edge_ids: edge_ids.iter().cloned().collect(),
        embed_ids: embed_ids.iter().cloned().collect(),
        link_ids: link_ids.iter().cloned().collect(),
        hyperlink_ids: hyperlink_ids.iter().cloned().collect(),
    };

    MergedMemberIds {
        node_ids,
        edge_ids,
        embed_ids,
        link_ids,
        hyperlink_ids,
        sets,
    }
}

/// 从 `Option<&GraphMeta>` 提取 revisions HashMap 引用。
fn meta_revisions<'a>(
    meta: Option<&'a GraphMeta>,
    f: fn(&'a GraphMeta) -> &'a HashMap<String, u64>,
) -> Option<&'a HashMap<String, u64>> {
    meta.map(f)
}

/// 合并五个 revisions HashMap。
fn merge_all_revisions(
    base_meta: Option<&GraphMeta>,
    local_meta: Option<&GraphMeta>,
    incoming_meta: Option<&GraphMeta>,
    sets: &MemberIdSets,
) -> MergedRevisions {
    let node = merge_revisions(
        meta_revisions(base_meta, |m| &m.node_revisions),
        meta_revisions(local_meta, |m| &m.node_revisions),
        meta_revisions(incoming_meta, |m| &m.node_revisions),
        &sets.node_ids,
    );
    let edge = merge_revisions(
        meta_revisions(base_meta, |m| &m.edge_revisions),
        meta_revisions(local_meta, |m| &m.edge_revisions),
        meta_revisions(incoming_meta, |m| &m.edge_revisions),
        &sets.edge_ids,
    );
    let embed = merge_revisions(
        meta_revisions(base_meta, |m| &m.embed_revisions),
        meta_revisions(local_meta, |m| &m.embed_revisions),
        meta_revisions(incoming_meta, |m| &m.embed_revisions),
        &sets.embed_ids,
    );
    let link = merge_revisions(
        meta_revisions(base_meta, |m| &m.link_revisions),
        meta_revisions(local_meta, |m| &m.link_revisions),
        meta_revisions(incoming_meta, |m| &m.link_revisions),
        &sets.link_ids,
    );
    let hyperlink = merge_revisions(
        meta_revisions(base_meta, |m| &m.hyperlink_revisions),
        meta_revisions(local_meta, |m| &m.hyperlink_revisions),
        meta_revisions(incoming_meta, |m| &m.hyperlink_revisions),
        &sets.hyperlink_ids,
    );
    MergedRevisions {
        node,
        edge,
        embed,
        link,
        hyperlink,
    }
}

// ===========================================================================
// 派生索引重建
// ===========================================================================

/// 派生索引重建结果：从对象文件内容重建的派生缓存字段。
#[derive(Debug, Clone, Default)]
pub(crate) struct DerivedIndexes {
    pub edge_relation_index: Vec<EdgeRelationIndex>,
    pub embed_host_index: Vec<EmbedHostIndex>,
    pub link_relation_index: Vec<LinkRelationIndex>,
    pub hyperlink_relation_index: Vec<HyperlinkRelationIndex>,
    pub node_kind_counts: HashMap<String, u32>,
}

/// 从合并后的对象文件内容重建派生索引。
///
/// 输入是合并后的 GraphMeta（包含最终成员 ID 列表）和每个对象文件的内容。
/// `object_contents` 的 key 是 `(object_type, object_id)`，value 是对象 JSON bytes。
///
/// 纯计算，不读磁盘。调用方（`run.rs`）负责读取对象文件并组装 `object_contents`。
///
/// # 重建规则
///
/// - `edge_relation_index`：从 edge 对象的 `from`/`to` 构建
/// - `embed_host_index`：从 embed 对象的 `host_path` 构建
/// - `link_relation_index`：从 link 对象的 `source`/`target` 构建
/// - `hyperlink_relation_index`：从 hyperlink 对象的 `source` 提取 `source_node_id`
/// - `node_kind_counts`：从 node 对象的 `kind` 字段统计计数
#[allow(clippy::too_many_lines)]
pub(crate) fn rebuild_derived_indexes(
    starmap_id: &str,
    member_ids: &MergedMemberIds,
    object_contents: &HashMap<(String, String), Vec<u8>>,
) -> Result<DerivedIndexes> {
    use crate::starmap::types::{StarMapEdge, StarMapEmbed, StarMapHyperlink, StarMapLink};

    let mut edge_relation_index: Vec<EdgeRelationIndex> = Vec::new();
    let mut embed_host_index: Vec<EmbedHostIndex> = Vec::new();
    let mut link_relation_index: Vec<LinkRelationIndex> = Vec::new();
    let mut hyperlink_relation_index: Vec<HyperlinkRelationIndex> = Vec::new();
    let mut node_kind_counts: HashMap<String, u32> = HashMap::new();

    // 重建 edge_relation_index
    for edge_id in &member_ids.edge_ids {
        let key = ("edge".to_string(), edge_id.clone());
        if let Some(bytes) = object_contents.get(&key) {
            let edge: StarMapEdge = serde_json::from_slice(bytes)?;
            edge_relation_index.push(EdgeRelationIndex {
                edge_id: edge.id.clone(),
                from: edge.from.clone(),
                to: edge.to.clone(),
            });
        }
    }

    // 重建 embed_host_index
    for instance_id in &member_ids.embed_ids {
        let key = ("embed".to_string(), instance_id.clone());
        if let Some(bytes) = object_contents.get(&key) {
            let embed: StarMapEmbed = serde_json::from_slice(bytes)?;
            embed_host_index.push(EmbedHostIndex {
                instance_id: embed.instance_id.clone(),
                host_path: embed.host_path.clone(),
            });
        }
    }

    // 重建 link_relation_index
    for link_id in &member_ids.link_ids {
        let key = ("link".to_string(), link_id.clone());
        if let Some(bytes) = object_contents.get(&key) {
            let link: StarMapLink = serde_json::from_slice(bytes)?;
            link_relation_index.push(LinkRelationIndex {
                link_id: link.link_id.clone(),
                source: link.source.clone(),
                target: link.target.clone(),
            });
        }
    }

    // 重建 hyperlink_relation_index
    for hl_id in &member_ids.hyperlink_ids {
        let key = ("hyperlink".to_string(), hl_id.clone());
        if let Some(bytes) = object_contents.get(&key) {
            let hl: StarMapHyperlink = serde_json::from_slice(bytes)?;
            let source_node_id =
                crate::starmap::store::relation_index::target_path_node_id(&hl.source, starmap_id)
                    .unwrap_or_default()
                    .to_string();
            hyperlink_relation_index.push(HyperlinkRelationIndex {
                hyperlink_id: hl.hyperlink_id.clone(),
                source_node_id,
            });
        }
    }

    // 重建 node_kind_counts
    for node_id in &member_ids.node_ids {
        let key = ("node".to_string(), node_id.clone());
        if let Some(bytes) = object_contents.get(&key) {
            let node: crate::starmap::types::StarMapNode = serde_json::from_slice(bytes)?;
            *node_kind_counts
                .entry(format!("{:?}", node.kind))
                .or_insert(0) += 1;
        }
    }

    Ok(DerivedIndexes {
        edge_relation_index,
        embed_host_index,
        link_relation_index,
        hyperlink_relation_index,
        node_kind_counts,
    })
}

// ===========================================================================
// 统一对象级合并
// ===========================================================================

/// 把路径中的对象目录名（复数）转为对象类型（单数）。
///
/// `nodes` → `node`，`edges` → `edge`，等。未知目录返回 `None`。
fn dir_to_object_type(dir: &str) -> Option<&'static str> {
    match dir {
        "nodes" => Some("node"),
        "edges" => Some("edge"),
        "embeds" => Some("embed"),
        "links" => Some("link"),
        "hyperlinks" => Some("hyperlink"),
        _ => None,
    }
}

/// 从对象文件路径提取 `(object_type, object_id)`。
///
/// 路径格式：`starmaps/<id>/<type_dir>/<bucket>/<obj_id>.json`。
/// `object_type` 是单数形式（`node`/`edge`/`embed`/`link`/`hyperlink`），
/// 与 [`rebuild_derived_indexes`] 和 `DeletionEntry.object_type` 一致。
/// 返回 `None` 如果路径不符合对象文件格式。
pub(crate) fn parse_object_path(rel_path: &std::path::Path) -> Option<(String, String)> {
    let rel_str = rel_path.to_string_lossy().replace('\\', "/");
    let parts: Vec<&str> = rel_str.split('/').collect();
    if parts.len() == 5 {
        if let Some(obj_type) = dir_to_object_type(parts[2]) {
            let file_name = parts[4];
            if file_name.ends_with(".json") && file_name.len() > ".json".len() {
                let obj_id = &file_name[..file_name.len() - ".json".len()];
                return Some((obj_type.to_string(), obj_id.to_string()));
            }
        }
    }
    None
}

/// 用最终对象集合修正成员表。
///
/// 先按 `initial` 顺序保留在 `final_set` 中的 ID，再追加 `final_set` 中
/// 不在 `initial` 里的 ID（按字典序排序，保证确定性）。
fn reconcile_member_list(initial: &[String], final_set: &HashSet<String>) -> Vec<String> {
    let mut result: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    // 1. 按 initial 顺序保留在 final_set 中的 ID
    for id in initial {
        if final_set.contains(id) && seen.insert(id.clone()) {
            result.push(id.clone());
        }
    }
    // 2. 追加 final_set 中不在 initial 里的 ID（字典序排序保证确定性）
    let mut extras: Vec<&String> = final_set.iter().filter(|id| !seen.contains(*id)).collect();
    extras.sort();
    for id in extras {
        result.push(id.clone());
    }
    result
}

/// 检查成员表中的每个 ID 都有对应胜出对象 bytes。
///
/// 成员表有 ID 但 `final_objects` 中没有对应 bytes → 数据不一致，返回 `Err`。
fn check_member_object_consistency(
    object_type: &str,
    member_ids: &[String],
    final_objects: &HashMap<(String, String), Vec<u8>>,
) -> Result<()> {
    for id in member_ids {
        let key = (object_type.to_string(), id.clone());
        if !final_objects.contains_key(&key) {
            return Err(Error::Other(format!(
                "starmap member '{}' (type '{}') has no corresponding object file",
                id, object_type
            )));
        }
    }
    Ok(())
}

/// 根据对象决策调整 revisions。
///
/// - `ApplyIncoming` → revision 提升到 `new_pr`（实际写入新内容）
/// - `Delete` → 从 revisions 中移除（对象已删除）
/// - `KeepLocal`/`NoOp`/`Conflict` → 保留当前 revision
///
/// `decisions` 的 key 是 object_id。
fn apply_revision_by_decisions(
    revisions: &mut HashMap<String, u64>,
    decisions: &HashMap<String, &ObjectMergeDecision>,
    new_pr: u64,
) {
    // 收集需要移除的 ID（Delete 决策）
    let to_remove: Vec<String> = revisions
        .keys()
        .filter(|id| matches!(decisions.get(*id), Some(ObjectMergeDecision::Delete)))
        .cloned()
        .collect();
    for id in to_remove {
        revisions.remove(&id);
    }

    // ApplyIncoming → 提升到 new_pr
    for (id, rev) in revisions.iter_mut() {
        if let Some(ObjectMergeDecision::ApplyIncoming(_)) = decisions.get(id) {
            *rev = (*rev).max(new_pr);
        }
    }
}

/// 星图 graph.json + 对象文件统一合并输出。
#[derive(Debug)]
pub(crate) struct StarmapGraphMergeOutput {
    pub graph_meta_result: StarmapMergeResult,
    pub object_decisions: Vec<(std::path::PathBuf, ObjectMergeDecision)>,
}

/// 统一处理 graph.json 和对象文件的合并，做一致性协调。
///
/// # 一致性协调
///
/// 1. 先对每个对象做三方决策（[`merge_starmap_object`]）
/// 2. 根据对象决策确定最终存在的对象集合
/// 3. 用最终对象集合驱动 graph 成员表（而不是 graph 成员表独立合并）
/// 4. graph 说删除 Y 但对象决策说 ApplyIncoming Y → 以对象决策为准，保留 Y
/// 5. graph 说保留 Y 但对象决策说 Delete Y → 以对象决策为准，移除 Y
/// 6. 重建派生索引（[`rebuild_derived_indexes`]）
/// 7. revision 只对实际写入/删除的对象提升，冲突保留本地
///
/// # 输入
///
/// - `starmap_id`：星图 ID（用于派生索引重建）
/// - `graph_base`/`graph_local`/`graph_incoming`：graph.json 的三方 bytes
/// - `objects`：每个对象文件的 `(rel_path, object_type, object_id, base, local, incoming)`
///
/// 纯计算，不读磁盘。调用方（`run.rs`）负责读取文件并组装输入。
#[allow(clippy::type_complexity, clippy::too_many_lines)]
pub(crate) fn merge_starmap_graph_with_objects(
    starmap_id: &str,
    graph_base: Option<&[u8]>,
    graph_local: Option<&[u8]>,
    graph_incoming: Option<&[u8]>,
    objects: &[(
        std::path::PathBuf,
        String,
        String,
        Option<Vec<u8>>,
        Option<Vec<u8>>,
        Option<Vec<u8>>,
    )],
) -> Result<StarmapGraphMergeOutput> {
    // --- 1. 对每个对象做三方决策，收集最终胜出 bytes ---
    let mut object_decisions: Vec<(std::path::PathBuf, ObjectMergeDecision)> = Vec::new();
    let mut final_objects: HashMap<(String, String), Vec<u8>> = HashMap::new();
    let mut node_decisions: HashMap<String, ObjectMergeDecision> = HashMap::new();
    let mut edge_decisions: HashMap<String, ObjectMergeDecision> = HashMap::new();
    let mut embed_decisions: HashMap<String, ObjectMergeDecision> = HashMap::new();
    let mut link_decisions: HashMap<String, ObjectMergeDecision> = HashMap::new();
    let mut hyperlink_decisions: HashMap<String, ObjectMergeDecision> = HashMap::new();

    for (rel_path, object_type, object_id, base, local, incoming) in objects {
        let decision = merge_starmap_object(base.as_deref(), local.as_deref(), incoming.as_deref());

        // 收集胜出 bytes：ApplyIncoming/ApplyMerged 用新内容，KeepLocal/Conflict 用 local，
        // NoOp 用 local（== incoming），Delete 无 bytes。
        let winning = match &decision {
            ObjectMergeDecision::NoOp => local.clone().or(incoming.clone()),
            ObjectMergeDecision::KeepLocal => local.clone(),
            ObjectMergeDecision::ApplyIncoming(c) => Some(c.clone()),
            ObjectMergeDecision::Delete => None,
            ObjectMergeDecision::Conflict(_) => local.clone(),
        };
        if let Some(bytes) = winning {
            final_objects.insert((object_type.clone(), object_id.clone()), bytes);
        }

        // 按类型记录决策（用于 revision 绑定）
        match object_type.as_str() {
            "node" => {
                node_decisions.insert(object_id.clone(), decision.clone());
            }
            "edge" => {
                edge_decisions.insert(object_id.clone(), decision.clone());
            }
            "embed" => {
                embed_decisions.insert(object_id.clone(), decision.clone());
            }
            "link" => {
                link_decisions.insert(object_id.clone(), decision.clone());
            }
            "hyperlink" => {
                hyperlink_decisions.insert(object_id.clone(), decision.clone());
            }
            _ => {}
        }

        object_decisions.push((rel_path.clone(), decision));
    }

    // --- 2. 解析三方 graph meta ---
    let base_meta = parse_graph_meta(graph_base)?;
    let local_meta = parse_graph_meta(graph_local)?;
    let incoming_meta = parse_graph_meta(graph_incoming)?;

    // 文件双方都不存在 → 无操作
    if graph_local.is_none() && graph_incoming.is_none() {
        return Ok(StarmapGraphMergeOutput {
            graph_meta_result: StarmapMergeResult::NoOp,
            object_decisions,
        });
    }

    let (merged_starmap_id, merged_schema_version) = resolve_graph_meta_header(
        base_meta.as_ref(),
        local_meta.as_ref(),
        incoming_meta.as_ref(),
    )?;

    // --- 3. 用 graph 成员表三方合并得到初始成员表（保持顺序） ---
    let initial_members = merge_member_ids(
        base_meta.as_ref(),
        local_meta.as_ref(),
        incoming_meta.as_ref(),
    );

    // --- 4. 从 final_objects 构建按类型分组的最终对象 ID 集合 ---
    let mut final_node: HashSet<String> = HashSet::new();
    let mut final_edge: HashSet<String> = HashSet::new();
    let mut final_embed: HashSet<String> = HashSet::new();
    let mut final_link: HashSet<String> = HashSet::new();
    let mut final_hyperlink: HashSet<String> = HashSet::new();
    for (obj_type, obj_id) in final_objects.keys() {
        match obj_type.as_str() {
            "node" => {
                final_node.insert(obj_id.clone());
            }
            "edge" => {
                final_edge.insert(obj_id.clone());
            }
            "embed" => {
                final_embed.insert(obj_id.clone());
            }
            "link" => {
                final_link.insert(obj_id.clone());
            }
            "hyperlink" => {
                final_hyperlink.insert(obj_id.clone());
            }
            _ => {}
        }
    }

    // --- 5. 用对象决策修正成员表 ---
    let node_ids = reconcile_member_list(&initial_members.node_ids, &final_node);
    let edge_ids = reconcile_member_list(&initial_members.edge_ids, &final_edge);
    let embed_ids = reconcile_member_list(&initial_members.embed_ids, &final_embed);
    let link_ids = reconcile_member_list(&initial_members.link_ids, &final_link);
    let hyperlink_ids = reconcile_member_list(&initial_members.hyperlink_ids, &final_hyperlink);

    // --- 6. 检查一致性：成员表中的每个 ID 必须有对应胜出对象 bytes ---
    check_member_object_consistency("node", &node_ids, &final_objects)?;
    check_member_object_consistency("edge", &edge_ids, &final_objects)?;
    check_member_object_consistency("embed", &embed_ids, &final_objects)?;
    check_member_object_consistency("link", &link_ids, &final_objects)?;
    check_member_object_consistency("hyperlink", &hyperlink_ids, &final_objects)?;

    let sets = MemberIdSets {
        node_ids: node_ids.iter().cloned().collect(),
        edge_ids: edge_ids.iter().cloned().collect(),
        embed_ids: embed_ids.iter().cloned().collect(),
        link_ids: link_ids.iter().cloned().collect(),
        hyperlink_ids: hyperlink_ids.iter().cloned().collect(),
    };
    let member_ids = MergedMemberIds {
        node_ids: node_ids.clone(),
        edge_ids: edge_ids.clone(),
        embed_ids: embed_ids.clone(),
        link_ids: link_ids.clone(),
        hyperlink_ids: hyperlink_ids.clone(),
        sets,
    };

    // --- 7. 合并 revisions 和 deletions ---
    let mut revisions = merge_all_revisions(
        base_meta.as_ref(),
        local_meta.as_ref(),
        incoming_meta.as_ref(),
        &member_ids.sets,
    );

    let merged_deletions = merge_deletions(
        base_meta.as_ref().map(|m| &m.deleted_since_last_sync),
        local_meta.as_ref().map(|m| &m.deleted_since_last_sync),
        incoming_meta.as_ref().map(|m| &m.deleted_since_last_sync),
        &member_ids.sets,
    );

    // --- 8. 重建派生索引 ---
    let derived = rebuild_derived_indexes(starmap_id, &member_ids, &final_objects)?;

    // --- 9. revision 绑定对象决策 + package_revision ---
    let local_pr = local_meta.as_ref().map(|m| m.package_revision).unwrap_or(0);
    let incoming_pr = incoming_meta
        .as_ref()
        .map(|m| m.package_revision)
        .unwrap_or(0);
    let local_ua = local_meta.as_ref().map(|m| m.updated_at).unwrap_or(0);
    let incoming_ua = incoming_meta.as_ref().map(|m| m.updated_at).unwrap_or(0);

    // 基于对象操作清单判断是否有实际变更：
    // ApplyIncoming（实际写入）或 Delete（实际删除）算变更；
    // KeepLocal/NoOp/Conflict 不算成功合并的变更。
    let has_actual_change = object_decisions.iter().any(|(_, d)| {
        matches!(
            d,
            ObjectMergeDecision::ApplyIncoming(_) | ObjectMergeDecision::Delete
        )
    });

    let package_revision = if has_actual_change {
        local_pr.max(incoming_pr) + 1
    } else {
        local_pr.max(incoming_pr)
    };

    // 只对实际变更的对象提升 revision；冲突保留本地 revision
    if has_actual_change {
        let node_refs: HashMap<String, &ObjectMergeDecision> =
            node_decisions.iter().map(|(k, v)| (k.clone(), v)).collect();
        let edge_refs: HashMap<String, &ObjectMergeDecision> =
            edge_decisions.iter().map(|(k, v)| (k.clone(), v)).collect();
        let embed_refs: HashMap<String, &ObjectMergeDecision> = embed_decisions
            .iter()
            .map(|(k, v)| (k.clone(), v))
            .collect();
        let link_refs: HashMap<String, &ObjectMergeDecision> =
            link_decisions.iter().map(|(k, v)| (k.clone(), v)).collect();
        let hyperlink_refs: HashMap<String, &ObjectMergeDecision> = hyperlink_decisions
            .iter()
            .map(|(k, v)| (k.clone(), v))
            .collect();
        apply_revision_by_decisions(&mut revisions.node, &node_refs, package_revision);
        apply_revision_by_decisions(&mut revisions.edge, &edge_refs, package_revision);
        apply_revision_by_decisions(&mut revisions.embed, &embed_refs, package_revision);
        apply_revision_by_decisions(&mut revisions.link, &link_refs, package_revision);
        apply_revision_by_decisions(&mut revisions.hyperlink, &hyperlink_refs, package_revision);
    }

    // --- 10. 构造最终 GraphMeta ---
    let merged = GraphMeta {
        schema_version: merged_schema_version,
        starmap_id: merged_starmap_id,
        node_ids,
        edge_ids,
        embed_instance_ids: embed_ids,
        link_ids,
        hyperlink_ids,
        edge_relation_index: derived.edge_relation_index,
        embed_host_index: derived.embed_host_index,
        link_relation_index: derived.link_relation_index,
        hyperlink_relation_index: derived.hyperlink_relation_index,
        node_kind_counts: derived.node_kind_counts,
        node_revisions: revisions.node,
        edge_revisions: revisions.edge,
        embed_revisions: revisions.embed,
        link_revisions: revisions.link,
        hyperlink_revisions: revisions.hyperlink,
        package_revision,
        updated_at: local_ua.max(incoming_ua),
        deleted_since_last_sync: merged_deletions,
    };

    let merged_bytes = serde_json::to_vec_pretty(&merged)?;
    let graph_meta_result =
        finalize_merge_result(&merged_bytes, graph_base, graph_local, graph_incoming);

    Ok(StarmapGraphMergeOutput {
        graph_meta_result,
        object_decisions,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- 测试辅助构造 ---

    fn make_index_bytes(
        starmap_ids: &[&str],
        root_starmap_ids: &[&str],
        main_starmap_by_project: &[(&str, &str)],
        updated_at: u64,
    ) -> Vec<u8> {
        let record = StarMapIndexRecord {
            schema_version: 1,
            starmap_ids: starmap_ids.iter().map(|s| s.to_string()).collect(),
            root_starmap_ids: root_starmap_ids.iter().map(|s| s.to_string()).collect(),
            main_starmap_by_project: main_starmap_by_project
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            updated_at,
        };
        serde_json::to_vec_pretty(&record).expect("serialize index")
    }

    fn extract_starmap_ids(result: &StarmapMergeResult) -> Vec<String> {
        let bytes = match result {
            StarmapMergeResult::ApplyIncoming(b) | StarmapMergeResult::ApplyMerged(b) => b,
            _ => return vec![],
        };
        let record: StarMapIndexRecord = serde_json::from_slice(bytes).expect("deserialize index");
        record.starmap_ids
    }

    // --- 测试 1: 双端新增 starmap ---

    #[test]
    fn test_merge_index_both_add_starmap() {
        let base = make_index_bytes(&["A"], &[], &[], 0);
        let local = make_index_bytes(&["A", "B"], &[], &[], 100);
        let incoming = make_index_bytes(&["A", "C"], &[], &[], 200);

        let result = merge_starmap_index(Some(&base), Some(&local), Some(&incoming))
            .expect("merge should succeed");
        let ids = extract_starmap_ids(&result);
        assert_eq!(ids, vec!["A", "B", "C"]);
    }

    // --- 测试 2: 双端删除 starmap ---

    #[test]
    fn test_merge_index_both_delete() {
        let base = make_index_bytes(&["A", "B"], &[], &[], 0);
        let local = make_index_bytes(&["A"], &[], &[], 100);
        let incoming = make_index_bytes(&["A"], &[], &[], 200);

        let result = merge_starmap_index(Some(&base), Some(&local), Some(&incoming))
            .expect("merge should succeed");
        let ids = extract_starmap_ids(&result);
        assert_eq!(ids, vec!["A"]);
    }

    // --- 测试 3: 一端删除一端保留 base 原样 → 接受删除（不复活）---

    #[test]
    fn test_merge_index_one_delete_one_keep() {
        let base = make_index_bytes(&["A", "B"], &[], &[], 0);
        let local = make_index_bytes(&["A"], &[], &[], 100); // local 删 B
        let incoming = make_index_bytes(&["A", "B"], &[], &[], 200); // incoming 保留 B（无变化）

        let result = merge_starmap_index(Some(&base), Some(&local), Some(&incoming))
            .expect("merge should succeed");
        let ids = extract_starmap_ids(&result);
        // 一侧删除一侧保留 base 原样 → 接受删除，结果为 [A]，不复活 B
        assert_eq!(ids, vec!["A"]);
    }

    // --- 测试 7: main_starmap_by_project 冲突 → 取 updated_at 较大方的值 ---

    #[test]
    fn test_merge_index_main_starmap_conflict() {
        // 真实数据约束：main_starmap_by_project 的 value 必须是 starmap_ids 中
        // 存在的图。base 有 A，local 新增 B 并把 main 指向 B，incoming 新增 C
        // 并把 main 指向 C。
        let base = make_index_bytes(&["A"], &[], &[("p", "A")], 0);
        let local = make_index_bytes(&["A", "B"], &[], &[("p", "B")], 100);
        let incoming = make_index_bytes(&["A", "C"], &[], &[("p", "C")], 200);

        let result = merge_starmap_index(Some(&base), Some(&local), Some(&incoming))
            .expect("merge should succeed");
        let bytes = match result {
            StarmapMergeResult::ApplyMerged(b) | StarmapMergeResult::ApplyIncoming(b) => b,
            StarmapMergeResult::KeepLocal => local,
            StarmapMergeResult::NoOp => base,
        };
        let record: StarMapIndexRecord = serde_json::from_slice(&bytes).expect("deserialize index");
        // incoming.updated_at (200) > local.updated_at (100) → 取 incoming 的值 "C"
        assert_eq!(
            record.main_starmap_by_project.get("p"),
            Some(&"C".to_string())
        );
    }

    // --- 测试 8: 合并结果 == local → KeepLocal ---

    #[test]
    fn test_merge_result_keep_local() {
        // local 新增 B，incoming 没改 → 合并结果 == local
        let base = make_index_bytes(&["A"], &[], &[], 0);
        let local = make_index_bytes(&["A", "B"], &[], &[], 100);
        let incoming = make_index_bytes(&["A"], &[], &[], 0);

        let result = merge_starmap_index(Some(&base), Some(&local), Some(&incoming))
            .expect("merge should succeed");
        assert!(matches!(result, StarmapMergeResult::KeepLocal));
    }

    // --- 测试 9: 合并结果 == incoming → ApplyIncoming ---

    #[test]
    fn test_merge_result_apply_incoming() {
        // incoming 新增 B，local 没改 → 合并结果 == incoming
        let base = make_index_bytes(&["A"], &[], &[], 0);
        let local = make_index_bytes(&["A"], &[], &[], 0);
        let incoming = make_index_bytes(&["A", "B"], &[], &[], 100);

        let result = merge_starmap_index(Some(&base), Some(&local), Some(&incoming))
            .expect("merge should succeed");
        match result {
            StarmapMergeResult::ApplyIncoming(b) => assert_eq!(b, incoming),
            other => panic!("expected ApplyIncoming, got {:?}", other),
        }
    }

    // --- 测试 10: base == local == incoming → NoOp ---

    #[test]
    fn test_merge_result_noop() {
        let base = make_index_bytes(&["A", "B"], &[], &[], 100);
        let local = base.clone();
        let incoming = base.clone();

        let result = merge_starmap_index(Some(&base), Some(&local), Some(&incoming))
            .expect("merge should succeed");
        assert!(matches!(result, StarmapMergeResult::NoOp));
    }

    // --- 测试 11: 路径识别 ---

    #[test]
    fn test_path_recognition() {
        // is_starmap_index_path
        assert!(is_starmap_index_path("starmaps/index.json"));
        assert!(is_starmap_index_path("starmaps\\index.json")); // backslash 归一化
        assert!(!is_starmap_index_path("starmaps/sm_1/index.json"));
        assert!(!is_starmap_index_path("starmaps/sm_1.meta.json"));
        assert!(!is_starmap_index_path("projects/index.json"));

        // is_starmap_graph_meta_path
        assert!(is_starmap_graph_meta_path("starmaps/sm_1/graph.json"));
        assert!(is_starmap_graph_meta_path("starmaps\\sm_1\\graph.json"));
        assert!(!is_starmap_graph_meta_path("starmaps/index.json"));
        assert!(!is_starmap_graph_meta_path("starmaps/sm_1/nodes/00/x.json"));
        assert!(!is_starmap_graph_meta_path("starmaps/graph.json")); // 缺少 <id> 段

        // is_starmap_meta_path
        assert!(is_starmap_meta_path("starmaps/sm_1.meta.json"));
        assert!(is_starmap_meta_path("starmaps\\sm_1.meta.json"));
        assert!(!is_starmap_meta_path("starmaps/index.json"));
        assert!(!is_starmap_meta_path("starmaps/.meta.json")); // id 为空
        assert!(!is_starmap_meta_path("starmaps/sm_1/graph.json"));

        // is_starmap_object_file_path
        assert!(is_starmap_object_file_path(
            "starmaps/sm_1/nodes/00/node_a.json"
        ));
        assert!(is_starmap_object_file_path(
            "starmaps/sm_1/edges/0f/edge_b.json"
        ));
        assert!(is_starmap_object_file_path(
            "starmaps/sm_1/embeds/05/inst_c.json"
        ));
        assert!(is_starmap_object_file_path(
            "starmaps/sm_1/links/0a/link_d.json"
        ));
        assert!(is_starmap_object_file_path(
            "starmaps/sm_1/hyperlinks/00/hl_e.json"
        ));
        assert!(is_starmap_object_file_path(
            "starmaps\\sm_1\\nodes\\00\\node_a.json"
        ));
        assert!(!is_starmap_object_file_path(
            "starmaps/sm_1/nodes/gg/node_a.json"
        )); // bucket 非 hex
        assert!(!is_starmap_object_file_path(
            "starmaps/sm_1/nodes/0/node_a.json"
        )); // bucket 长度不对
        assert!(!is_starmap_object_file_path(
            "starmaps/sm_1/unknown/00/x.json"
        )); // 未知对象类型
        assert!(!is_starmap_object_file_path("starmaps/sm_1/nodes/00/.json")); // obj_id 为空
        assert!(!is_starmap_object_file_path(
            "starmaps/sm_1/nodes/00/node_a.txt"
        )); // 非 .json

        // is_starmap_content_path
        assert!(is_starmap_content_path("starmaps/index.json"));
        assert!(is_starmap_content_path("starmaps/sm_1/graph.json"));
        assert!(is_starmap_content_path("starmaps/sm_1.meta.json"));
        assert!(is_starmap_content_path(
            "starmaps/sm_1/nodes/00/node_a.json"
        ));
        assert!(!is_starmap_content_path("projects/p_1/meta.json"));
        assert!(!is_starmap_content_path("settings.json"));
    }

    // --- 测试 12: root_starmap_ids 指向已删除图 → 清理 ---

    #[test]
    fn test_merge_index_root_starmap_cleanup_on_delete() {
        // base 有 A、B，B 同时是 root。local 删 B（从 starmap_ids 和 root 都移除），
        // incoming 保留 B 无变化。接受删除后 B 不在 starmap_ids，root 中指向 B 的
        // ID 也应被清理。
        let base = make_index_bytes(&["A", "B"], &["A", "B"], &[], 0);
        let local = make_index_bytes(&["A"], &["A"], &[], 100); // local 删 B
        let incoming = make_index_bytes(&["A", "B"], &["A", "B"], &[], 200); // incoming 保留

        let result = merge_starmap_index(Some(&base), Some(&local), Some(&incoming))
            .expect("merge should succeed");
        let bytes = match result {
            StarmapMergeResult::ApplyMerged(b) | StarmapMergeResult::ApplyIncoming(b) => b,
            StarmapMergeResult::KeepLocal => local,
            StarmapMergeResult::NoOp => base,
        };
        let record: StarMapIndexRecord = serde_json::from_slice(&bytes).expect("deserialize index");
        assert_eq!(record.starmap_ids, vec!["A".to_string()]);
        // root_starmap_ids 中 B 已被清理（指向已删除图）
        assert_eq!(record.root_starmap_ids, vec!["A".to_string()]);
    }

    // --- 测试 13: main_starmap_by_project 指向已删除图 → 清理 ---

    #[test]
    fn test_merge_index_main_starmap_cleanup_on_delete() {
        // base 有 A、B，main={"p":"B"}。local 删 B，incoming 保留 B 无变化。
        // 接受删除后 B 不在 starmap_ids，main 中指向 B 的映射也应被清理。
        let base = make_index_bytes(&["A", "B"], &[], &[("p", "B")], 0);
        let local = make_index_bytes(&["A"], &[], &[("p", "A")], 100); // local 删 B，main 改指 A
        let incoming = make_index_bytes(&["A", "B"], &[], &[("p", "B")], 200); // incoming 保留

        let result = merge_starmap_index(Some(&base), Some(&local), Some(&incoming))
            .expect("merge should succeed");
        let bytes = match result {
            StarmapMergeResult::ApplyMerged(b) | StarmapMergeResult::ApplyIncoming(b) => b,
            StarmapMergeResult::KeepLocal => local,
            StarmapMergeResult::NoOp => base,
        };
        let record: StarMapIndexRecord = serde_json::from_slice(&bytes).expect("deserialize index");
        assert_eq!(record.starmap_ids, vec!["A".to_string()]);
        // main_starmap_by_project 中指向已删除图 B 的映射被清理；
        // local 把 main 改指 A（A 仍存在）→ 保留 "p"→"A"。
        assert_eq!(
            record.main_starmap_by_project.get("p"),
            Some(&"A".to_string())
        );
    }

    // --- 测试 20-31: merge_starmap_object 对象级三方决策 ---

    #[test]
    fn test_merge_object_all_none() {
        let d = merge_starmap_object(None, None, None);
        assert!(matches!(d, ObjectMergeDecision::NoOp), "got {:?}", d);
    }

    #[test]
    fn test_merge_object_local_add() {
        let d = merge_starmap_object(None, Some(b"local"), None);
        assert!(matches!(d, ObjectMergeDecision::KeepLocal), "got {:?}", d);
    }

    #[test]
    fn test_merge_object_incoming_add() {
        let d = merge_starmap_object(None, None, Some(b"incoming"));
        match d {
            ObjectMergeDecision::ApplyIncoming(c) => assert_eq!(c, b"incoming"),
            other => panic!("expected ApplyIncoming, got {:?}", other),
        }
    }

    #[test]
    fn test_merge_object_both_add_same() {
        let d = merge_starmap_object(None, Some(b"x"), Some(b"x"));
        assert!(matches!(d, ObjectMergeDecision::NoOp), "got {:?}", d);
    }

    #[test]
    fn test_merge_object_both_add_different() {
        let d = merge_starmap_object(None, Some(b"local"), Some(b"incoming"));
        match d {
            ObjectMergeDecision::Conflict(crate::sync::types::SyncConflictKind::BothChanged) => {}
            other => panic!("expected Conflict(BothChanged), got {:?}", other),
        }
    }

    #[test]
    fn test_merge_object_both_delete() {
        let d = merge_starmap_object(Some(b"base"), None, None);
        assert!(matches!(d, ObjectMergeDecision::Delete), "got {:?}", d);
    }

    #[test]
    fn test_merge_object_remote_delete_local_keep() {
        // base 存在、local == base、incoming 不存在 → 接受远端删除
        let d = merge_starmap_object(Some(b"base"), Some(b"base"), None);
        assert!(matches!(d, ObjectMergeDecision::Delete), "got {:?}", d);
    }

    #[test]
    fn test_merge_object_remote_delete_local_modify_conflict() {
        // base 存在、local != base（local 修改了）、incoming 不存在 → 删除-修改冲突
        let d = merge_starmap_object(Some(b"base"), Some(b"modified"), None);
        match d {
            ObjectMergeDecision::Conflict(crate::sync::types::SyncConflictKind::RemoteDeleted) => {}
            other => panic!("expected Conflict(RemoteDeleted), got {:?}", other),
        }
    }

    #[test]
    fn test_merge_object_local_delete_remote_keep() {
        // base 存在、local 不存在、incoming == base → 接受本地删除
        let d = merge_starmap_object(Some(b"base"), None, Some(b"base"));
        assert!(matches!(d, ObjectMergeDecision::Delete), "got {:?}", d);
    }

    #[test]
    fn test_merge_object_local_delete_remote_modify_conflict() {
        // base 存在、local 不存在、incoming != base（incoming 修改了）→ 删除-修改冲突
        let d = merge_starmap_object(Some(b"base"), None, Some(b"modified"));
        match d {
            ObjectMergeDecision::Conflict(crate::sync::types::SyncConflictKind::RemoteDeleted) => {}
            other => panic!("expected Conflict(RemoteDeleted), got {:?}", other),
        }
    }

    #[test]
    fn test_merge_object_both_same() {
        let d = merge_starmap_object(Some(b"base"), Some(b"same"), Some(b"same"));
        assert!(matches!(d, ObjectMergeDecision::NoOp), "got {:?}", d);
    }

    #[test]
    fn test_merge_object_only_incoming_changed() {
        // local==base, incoming!=base → ApplyIncoming
        let d = merge_starmap_object(Some(b"base"), Some(b"base"), Some(b"new"));
        match d {
            ObjectMergeDecision::ApplyIncoming(c) => assert_eq!(c, b"new"),
            other => panic!("expected ApplyIncoming, got {:?}", other),
        }
    }

    #[test]
    fn test_merge_object_only_local_changed() {
        // local!=base, incoming==base → KeepLocal
        let d = merge_starmap_object(Some(b"base"), Some(b"new"), Some(b"base"));
        assert!(matches!(d, ObjectMergeDecision::KeepLocal), "got {:?}", d);
    }

    #[test]
    fn test_merge_object_both_changed_conflict() {
        // local!=base, incoming!=base, local!=incoming → Conflict(BothChanged)
        let d = merge_starmap_object(Some(b"base"), Some(b"local"), Some(b"incoming"));
        match d {
            ObjectMergeDecision::Conflict(crate::sync::types::SyncConflictKind::BothChanged) => {}
            other => panic!("expected Conflict(BothChanged), got {:?}", other),
        }
    }

    // --- 测试 36: three_way_merge_id_set 直接测试 ---

    #[test]
    fn test_three_way_merge_id_set_delete_modify_conflict() {
        let base = vec!["a".to_string(), "b".to_string()];
        let local = vec!["a".to_string()]; // 删 b
        let incoming = vec!["a".to_string(), "b".to_string()]; // 保留 b
        let local_mod = HashSet::new();
        let mut incoming_mod = HashSet::new();
        incoming_mod.insert("b".to_string()); // incoming 修改了 b

        let result = three_way_merge_id_set(
            Some(&base),
            Some(&local),
            Some(&incoming),
            &local_mod,
            &incoming_mod,
        );
        // 删除-修改冲突 → 保留 b
        assert!(
            result.contains(&"b".to_string()),
            "delete-modify conflict should preserve b"
        );
    }

    #[test]
    fn test_three_way_merge_id_set_delete_keep_unmodified() {
        let base = vec!["a".to_string(), "b".to_string()];
        let local = vec!["a".to_string()]; // 删 b
        let incoming = vec!["a".to_string(), "b".to_string()]; // 保留 b（未修改）
        let local_mod = HashSet::new();
        let incoming_mod = HashSet::new();

        let result = three_way_merge_id_set(
            Some(&base),
            Some(&local),
            Some(&incoming),
            &local_mod,
            &incoming_mod,
        );
        // 接受删除 → b 不保留
        assert!(
            !result.contains(&"b".to_string()),
            "delete-keep-unmodified should accept deletion"
        );
    }

    #[test]
    fn test_three_way_merge_id_set_both_delete() {
        let base = vec!["a".to_string(), "b".to_string()];
        let local = vec!["a".to_string()]; // 删 b
        let incoming = vec!["a".to_string()]; // 删 b
        let local_mod = HashSet::new();
        let incoming_mod = HashSet::new();

        let result = three_way_merge_id_set(
            Some(&base),
            Some(&local),
            Some(&incoming),
            &local_mod,
            &incoming_mod,
        );
        // 两侧都删 → b 不保留
        assert!(
            !result.contains(&"b".to_string()),
            "both delete should remove b"
        );
    }

    #[test]
    fn test_three_way_merge_id_set_both_keep() {
        let base = vec!["a".to_string(), "b".to_string()];
        let local = vec!["a".to_string(), "b".to_string()];
        let incoming = vec!["a".to_string(), "b".to_string()];
        let local_mod = HashSet::new();
        let incoming_mod = HashSet::new();

        let result = three_way_merge_id_set(
            Some(&base),
            Some(&local),
            Some(&incoming),
            &local_mod,
            &incoming_mod,
        );
        // 两侧都保留 → b 保留
        assert!(
            result.contains(&"b".to_string()),
            "both keep should preserve b"
        );
    }
}
