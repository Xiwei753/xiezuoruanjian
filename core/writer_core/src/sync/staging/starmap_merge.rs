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
use crate::starmap::store::relation_index::{EdgeRelationIndex, EmbedHostIndex, LinkRelationIndex};
use crate::starmap::StarMapIndexRecord;
use crate::sync::content_class::{resolve_lww, LwwRecord, LwwWinner};

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
/// `starmaps/<id>/{nodes,edges,embeds,links}/<bucket>/<obj_id>.json`。
pub(crate) fn is_starmap_object_file_path(rel_path: &str) -> bool {
    let normalized = normalize_path(rel_path);
    let parts: Vec<&str> = normalized.split('/').collect();
    parts.len() == 5
        && parts[0] == "starmaps"
        && !parts[1].is_empty()
        && matches!(parts[2], "nodes" | "edges" | "embeds" | "links")
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
    /// 远端删除了整个 graph（incoming 不存在）且 local 未改 → 删除 graph.json。
    /// 调用方（run.rs）应发出 `CommitAction::Delete` 删除 live 的 graph.json。
    /// 修复 issue #902 第十一轮复核：此前整图删除会写出空成员列表的 GraphMeta，
    /// 若 index.json 已删除对应星图 ID，磁盘会保留未登记的空 graph。
    Delete,
}

/// 对象级三方合并决策结果。
///
/// 用于星图对象文件（`nodes/edges/embeds/links/<bucket>/<id>.json`）
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
/// | Some | None  | Some     | i!=b | Conflict(BothChanged)（本地删除+远端修改，incoming 存在）|
/// | Some | Some  | Some     | l==i | NoOp |
/// | Some | Some  | Some     | l==b, i!=b | ApplyIncoming |
/// | Some | Some  | Some     | l!=b, i==b | KeepLocal |
/// | Some | Some  | Some     | l!=b, i!=b, l!=i | Conflict(BothChanged) |
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn merge_starmap_object(
    base: Option<&[u8]>,
    local: Option<&[u8]>,
    incoming: Option<&[u8]>,
) -> ObjectMergeDecision {
    merge_starmap_object_with_versions(base, local, incoming, None, None)
}

/// 带持久版本记录的对象级三方合并。
///
/// 内容三方比较能确定“只有一侧修改”时仍直接采用该侧；只有真正无法由
/// 内容判断的双侧修改（含删除-修改）才用两侧 manifest 的持久
/// `updated_at_ms` / `deleted_at_ms` + `device_id` 做统一 LWW 决胜。
/// 记录缺失时保留原有冲突行为，不凭空制造版本。
pub(crate) fn merge_starmap_object_with_versions(
    base: Option<&[u8]>,
    local: Option<&[u8]>,
    incoming: Option<&[u8]>,
    local_record: Option<&LwwRecord>,
    incoming_record: Option<&LwwRecord>,
) -> ObjectMergeDecision {
    let decision = merge_starmap_object_by_content(base, local, incoming);
    if !matches!(decision, ObjectMergeDecision::Conflict(_)) {
        return decision;
    }
    let (Some(local_record), Some(incoming_record)) = (local_record, incoming_record) else {
        return decision;
    };
    match resolve_lww(local_record, incoming_record) {
        LwwWinner::Remote => match incoming {
            Some(content) => ObjectMergeDecision::ApplyIncoming(content.to_vec()),
            None => ObjectMergeDecision::Delete,
        },
        LwwWinner::Local => {
            if local.is_some() {
                ObjectMergeDecision::KeepLocal
            } else {
                ObjectMergeDecision::Delete
            }
        }
        LwwWinner::Tie => ObjectMergeDecision::NoOp,
    }
}

/// 纯内容三方比较（不读取版本记录）。
fn merge_starmap_object_by_content(
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
        //   incoming 实际存在，用 BothChanged 而非 RemoteDeleted：
        //   save_conflict_snapshots 只给 BothChanged 保存 incoming snapshot，
        //   take_remote 的 BothChanged 分支会正确写入远端文件。
        //   修复 issue #902 第十一轮复核。
        (Some(bv), None, Some(iv)) => {
            if iv == bv {
                ObjectMergeDecision::Delete
            } else {
                ObjectMergeDecision::Conflict(crate::sync::types::SyncConflictKind::BothChanged)
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
}

/// 合并后的四个成员 ID 列表及其 HashSet 集合。
pub(crate) struct MergedMemberIds {
    node_ids: Vec<String>,
    edge_ids: Vec<String>,
    embed_ids: Vec<String>,
    link_ids: Vec<String>,
    sets: MemberIdSets,
}

/// 合并后的四个 revisions HashMap。
struct MergedRevisions {
    node: HashMap<String, u64>,
    edge: HashMap<String, u64>,
    embed: HashMap<String, u64>,
    link: HashMap<String, u64>,
}

/// 判断 tombstone 对应的对象是否仍在成员表中。
fn is_object_still_exists(object_type: &str, object_id: &str, sets: &MemberIdSets) -> bool {
    match object_type {
        "node" => sets.node_ids.contains(object_id),
        "edge" => sets.edge_ids.contains(object_id),
        "embed" => sets.embed_ids.contains(object_id),
        "link" => sets.link_ids.contains(object_id),
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
///
/// # 删除-修改冲突保留（issue #902 第十二轮复核）
///
/// `graph_lifecycle_decisions` 传入各星图的整图生命周期决策。若某星图是
/// `DeleteModifyConflict`（一端删除整图，另一端修改了 graph），且 live 图仍存在
/// （base 或 local 的 starmap_ids 含此 ID），则该 ID 不会被从全局索引移除，
/// 避免 graph 成为未登记的孤儿。root_starmap_ids 和 main_starmap_by_project
/// 中的对应关联同理保留。
pub(crate) fn merge_starmap_index(
    base: Option<&[u8]>,
    local: Option<&[u8]>,
    incoming: Option<&[u8]>,
    graph_lifecycle_decisions: &std::collections::HashMap<String, GraphLifecycleDecision>,
) -> Result<StarmapMergeResult> {
    let base_rec = parse_index(base)?;
    let local_rec = parse_index(local)?;
    let incoming_rec = parse_index(incoming)?;

    // StarMapIndexRecord 没有 revision 信息，无法区分"保留 base 原样"和"修改了"，
    // 因此传入空 modified 集合，保持原有行为（一侧删除一侧保留 base 原样 → 接受删除）。
    let empty_modified: HashSet<String> = HashSet::new();

    let mut merged_starmap_ids = three_way_merge_id_set(
        Some(&base_rec.starmap_ids),
        Some(&local_rec.starmap_ids),
        Some(&incoming_rec.starmap_ids),
        &empty_modified,
        &empty_modified,
    );

    // --- 删除-修改冲突星图保留（issue #902 第十二/十三轮复核）---
    // 仅远端删除、本地修改（is_local_deleted: false）时 live graph 仍存在于磁盘，
    // 全局索引不能删掉该星图 ID，否则 graph 成为未登记的孤儿。只要该 ID 在 base
    // 或 local 的 starmap_ids 中（说明 live 图存在），就将其加回 merged_starmap_ids。
    // 本地删除、远端修改（is_local_deleted: true）时本地已删除该图，live graph.json
    // 不存在，不应把图重新登记到全局 index（否则 index 指向 A、live 却没有 A/graph.json）。
    let base_starmap_set: HashSet<&String> = base_rec.starmap_ids.iter().collect();
    let local_starmap_set: HashSet<&String> = local_rec.starmap_ids.iter().collect();
    let mut conflict_retained_ids: HashSet<String> = HashSet::new();
    for (id, decision) in graph_lifecycle_decisions {
        if matches!(
            decision,
            GraphLifecycleDecision::DeleteModifyConflict {
                is_local_deleted: false
            }
        ) && (base_starmap_set.contains(id) || local_starmap_set.contains(id))
        {
            if !merged_starmap_ids.iter().any(|x| x == id) {
                merged_starmap_ids.push(id.clone());
            }
            conflict_retained_ids.insert(id.clone());
        }
    }

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
    let mut merged_root_starmap_ids =
        retain_ids_in_set(merged_root_starmap_ids, &existing_starmap_ids);
    // 删除-修改冲突的根星图 ID 也需保留：如果该 ID 在 base 或 local 的
    // root_starmap_ids 中，且已被加回 existing_starmap_ids，则加回 root 列表。
    let base_root_set: HashSet<&String> = base_rec.root_starmap_ids.iter().collect();
    let local_root_set: HashSet<&String> = local_rec.root_starmap_ids.iter().collect();
    for id in &conflict_retained_ids {
        if (base_root_set.contains(id) || local_root_set.contains(id))
            && !merged_root_starmap_ids.iter().any(|x| x == id)
        {
            merged_root_starmap_ids.push(id.clone());
        }
    }

    let merged_main = merge_main_starmap_by_project(
        &base_rec.main_starmap_by_project,
        &local_rec.main_starmap_by_project,
        &incoming_rec.main_starmap_by_project,
        local_rec.updated_at,
        incoming_rec.updated_at,
    );
    let mut merged_main = retain_main_by_existing(merged_main, &existing_starmap_ids);
    // 删除-修改冲突的 main_starmap_by_project 映射也需保留：
    // 如果 base 或 local 有 project→id 映射，且 id 已被加回，则加回该映射。
    for id in &conflict_retained_ids {
        for (proj, sid) in &base_rec.main_starmap_by_project {
            if sid == id {
                merged_main
                    .entry(proj.clone())
                    .or_insert_with(|| id.clone());
            }
        }
        for (proj, sid) in &local_rec.main_starmap_by_project {
            if sid == id {
                merged_main
                    .entry(proj.clone())
                    .or_insert_with(|| id.clone());
            }
        }
    }

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

/// 合并四个成员 ID 列表，返回合并后的 ID 和对应的 HashSet 集合。
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

    let sets = MemberIdSets {
        node_ids: node_ids.iter().cloned().collect(),
        edge_ids: edge_ids.iter().cloned().collect(),
        embed_ids: embed_ids.iter().cloned().collect(),
        link_ids: link_ids.iter().cloned().collect(),
    };

    MergedMemberIds {
        node_ids,
        edge_ids,
        embed_ids,
        link_ids,
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

/// 合并四个 revisions HashMap。
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
    MergedRevisions {
        node,
        edge,
        embed,
        link,
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
/// - `node_kind_counts`：从 node 对象的 `kind` 字段统计计数
#[allow(clippy::too_many_lines)]
pub(crate) fn rebuild_derived_indexes(
    _starmap_id: &str,
    member_ids: &MergedMemberIds,
    object_contents: &HashMap<(String, String), Vec<u8>>,
) -> Result<DerivedIndexes> {
    use crate::starmap::types::{StarMapEdge, StarMapEmbed, StarMapLink};

    let mut edge_relation_index: Vec<EdgeRelationIndex> = Vec::new();
    let mut embed_host_index: Vec<EmbedHostIndex> = Vec::new();
    let mut link_relation_index: Vec<LinkRelationIndex> = Vec::new();
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
        _ => None,
    }
}

/// 从对象文件路径提取 `(object_type, object_id)`。
///
/// 路径格式：`starmaps/<id>/<type_dir>/<bucket>/<obj_id>.json`。
/// `object_type` 是单数形式（`node`/`edge`/`embed`/`link`），
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
///
/// # 最终成员表驱动
///
/// 修复 issue #902 第十一轮复核：此前只遍历**现有 `revisions` 的键**提升
/// ApplyIncoming；如果 GraphMeta 成员列表里有新增对象，却尚无对应的
/// `node_revisions[id]`，其 ID 根本不进 map。`snapshot.rs` 在增量模式用
/// `node_revs.get(id).unwrap_or(0) > since_revision`，新增节点会被漏发。
///
/// 现在从 `final_member_ids`（最终存在的成员 ID 集合）驱动：
/// 1. 删除不在最终成员表中的条目
/// 2. 为最终存在的成员确保有 revision 条目（ApplyIncoming 提升到 new_pr，
///    其余保留当前 revision 或补记 new_pr）
fn apply_revision_by_decisions(
    revisions: &mut HashMap<String, u64>,
    decisions: &HashMap<String, &ObjectMergeDecision>,
    final_member_ids: &HashSet<String>,
    new_pr: u64,
) {
    // 1. 删除不在最终成员表中的条目
    revisions.retain(|id, _| final_member_ids.contains(id));

    // 2. 为最终存在的成员确保有 revision 条目
    for id in final_member_ids {
        match decisions.get(id) {
            Some(ObjectMergeDecision::ApplyIncoming(_)) => {
                // 新增或修改的对象，revision 提升到 new_pr
                revisions.insert(id.clone(), new_pr);
            }
            Some(ObjectMergeDecision::Delete) => {
                // 已在 retain 中移除，不补记
            }
            _ => {
                // KeepLocal/NoOp/Conflict 或无决策：保留当前 revision，
                // 如果没有则补记 new_pr（确保最终成员都有 revision 条目）
                revisions.entry(id.clone()).or_insert(new_pr);
            }
        }
    }
}

/// 整图生命周期决策（issue #902 第十二轮复核）。
///
/// 对称处理 base/local/incoming 三方的整图存在性，覆盖所有组合，
/// 不再只处理"远端删除"单边场景。
#[derive(Debug, Clone)]
pub(crate) enum GraphLifecycleDecision {
    /// 正常合并：graph 存在（至少一方有），走常规 graph+对象合并。
    Normal,
    /// 整图删除：双端删除，或一端删除另一端未改。
    Delete,
    /// 删除-修改冲突：一端删除整图，另一端修改了 graph。
    /// `is_local_deleted=true`：本地删除远端修改；`false`：远端删除本地修改。
    DeleteModifyConflict { is_local_deleted: bool },
}

/// 星图 graph.json + 对象文件统一合并输出。
#[derive(Debug)]
pub(crate) struct StarmapGraphMergeOutput {
    pub graph_meta_result: StarmapMergeResult,
    pub object_decisions: Vec<(std::path::PathBuf, ObjectMergeDecision)>,
    /// 整图生命周期决策（issue #902 第十二轮复核）。
    /// `merge_starmap_index` 据此决定是否保留删除-修改冲突星图的全局关联。
    pub lifecycle_decision: GraphLifecycleDecision,
    /// graph 级冲突事实（`DeleteModifyConflict` 时产生）。
    /// run.rs 据此在 `plan.conflict` 中添加 graph.json 的 `StagingConflict`，
    /// 不再只靠对象级 `Conflict(RemoteDeleted)` 隐式传递整图冲突。
    pub graph_conflict: Option<crate::sync::types::SyncConflictKind>,
}

/// 解析整图生命周期决策（issue #902 第十二轮复核）。
///
/// 对称处理 base/local/incoming 三方整图存在性，覆盖所有组合，
/// 不再只处理"远端删除"单边场景（原 `check_entire_graph_delete` 的局限）。
///
/// **issue #902 第十七轮复核**：整图实体 = graph + meta 的组合。
/// "整图存在" = graph.is_some() OR meta.is_some()；
/// "整图未修改" = graph 和 meta 都未改。一侧删除、另一侧对任一组成部分
/// 有实际变更 → DeleteModifyConflict，避免只改 meta 被误判为"未修改"而删除。
///
/// # 决策规则
///
/// "存在" 指 graph 或 meta 任一存在。
///
/// | base | local | incoming | 条件 | 决策 |
/// |------|-------|----------|------|------|
/// | 否   | *     | *        | —    | Normal（新建图）|
/// | 是   | 是    | 是       | —    | Normal |
/// | 是   | 是    | 否       | local 未改(graph+meta都未改) | Delete |
/// | 是   | 是    | 否       | local 修改了 | DeleteModifyConflict{is_local_deleted:false}|
/// | 是   | 否    | 是       | incoming 未改 | Delete |
/// | 是   | 否    | 是       | incoming 修改了 | DeleteModifyConflict{is_local_deleted:true}|
/// | 是   | 否    | 否       | —    | Delete（双端删除）|
pub(crate) fn resolve_graph_lifecycle(
    graph_base: Option<&[u8]>,
    graph_local: Option<&[u8]>,
    graph_incoming: Option<&[u8]>,
    meta_base: Option<&[u8]>,
    meta_local: Option<&[u8]>,
    meta_incoming: Option<&[u8]>,
) -> GraphLifecycleDecision {
    // 整图存在性 = graph 或 meta 任一存在
    let base_exists = graph_base.is_some() || meta_base.is_some();
    let local_exists = graph_local.is_some() || meta_local.is_some();
    let incoming_exists = graph_incoming.is_some() || meta_incoming.is_some();

    // base 无 → Normal（新建图，走常规合并）
    if !base_exists {
        return GraphLifecycleDecision::Normal;
    }
    // base 有、local 有、incoming 有 → Normal
    if local_exists && incoming_exists {
        return GraphLifecycleDecision::Normal;
    }
    // base 有、local 有、incoming 无 → 远端删除
    if local_exists && !incoming_exists {
        // local 未修改 = graph 和 meta 都未改
        let graph_unchanged = graph_local == graph_base;
        let meta_unchanged = meta_local == meta_base;
        if graph_unchanged && meta_unchanged {
            // local 未改 → 接受远端删除
            GraphLifecycleDecision::Delete
        } else {
            // local 修改了（graph 或 meta 任一改了）→ 远端删除本地修改冲突
            GraphLifecycleDecision::DeleteModifyConflict {
                is_local_deleted: false,
            }
        }
    } else if !local_exists && incoming_exists {
        // base 有、local 无、incoming 有 → 本地删除
        // incoming 未修改 = graph 和 meta 都未改
        let graph_unchanged = graph_incoming == graph_base;
        let meta_unchanged = meta_incoming == meta_base;
        if graph_unchanged && meta_unchanged {
            // incoming 未改 → 接受本地删除
            GraphLifecycleDecision::Delete
        } else {
            // incoming 修改了 → 本地删除远端修改冲突
            GraphLifecycleDecision::DeleteModifyConflict {
                is_local_deleted: true,
            }
        }
    } else {
        // base 有、local 无、incoming 无 → 双端删除
        GraphLifecycleDecision::Delete
    }
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
/// - `meta_base`/`meta_local`/`meta_incoming`：meta.json 的三方 bytes
///   （issue #902 第十七轮复核：纳入整图生命周期判断，避免只改 meta 被误判为"未修改"）
/// - `objects`：每个对象文件的 `(rel_path, object_type, object_id, base, local, incoming)`
///
/// 纯计算，不读磁盘。调用方（`run.rs`）负责读取文件并组装输入。
#[allow(
    clippy::type_complexity,
    clippy::too_many_lines,
    clippy::too_many_arguments
)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn merge_starmap_graph_with_objects(
    starmap_id: &str,
    graph_base: Option<&[u8]>,
    graph_local: Option<&[u8]>,
    graph_incoming: Option<&[u8]>,
    meta_base: Option<&[u8]>,
    meta_local: Option<&[u8]>,
    meta_incoming: Option<&[u8]>,
    objects: &[(
        std::path::PathBuf,
        String,
        String,
        Option<Vec<u8>>,
        Option<Vec<u8>>,
        Option<Vec<u8>>,
    )],
) -> Result<StarmapGraphMergeOutput> {
    merge_starmap_graph_with_object_versions(
        starmap_id,
        graph_base,
        graph_local,
        graph_incoming,
        meta_base,
        meta_local,
        meta_incoming,
        objects,
        &HashMap::new(),
    )
}

/// 带对象版本记录的星图 graph 合并。
///
/// `object_versions` 以对象文件相对路径为键，值为 `(local 记录, incoming 记录)`。
/// 对象内容无法判断胜负时使用记录做 LWW；记录缺失时仍按内容冲突处理。
pub(crate) fn merge_starmap_graph_with_object_versions(
    starmap_id: &str,
    graph_base: Option<&[u8]>,
    graph_local: Option<&[u8]>,
    graph_incoming: Option<&[u8]>,
    meta_base: Option<&[u8]>,
    meta_local: Option<&[u8]>,
    meta_incoming: Option<&[u8]>,
    objects: &[(
        std::path::PathBuf,
        String,
        String,
        Option<Vec<u8>>,
        Option<Vec<u8>>,
        Option<Vec<u8>>,
    )],
    object_versions: &HashMap<String, (Option<LwwRecord>, Option<LwwRecord>)>,
) -> Result<StarmapGraphMergeOutput> {
    // --- 1. 解析 graph 成员并确定对象保留边界 ---
    let base_meta = parse_graph_meta(graph_base)?;
    let local_meta = parse_graph_meta(graph_local)?;
    let incoming_meta = parse_graph_meta(graph_incoming)?;

    let lifecycle_decision = resolve_graph_lifecycle(
        graph_base,
        graph_local,
        graph_incoming,
        meta_base,
        meta_local,
        meta_incoming,
    );

    // --- 整图生命周期决策（issue #902 第十二轮复核）---
    // 删除或删除-修改冲突必须优先于对象级决策，不能用孤儿对象复活整图。
    match &lifecycle_decision {
        GraphLifecycleDecision::Delete => {
            let decisions = objects
                .iter()
                .map(|(rel, _, _, _, _, _)| (rel.clone(), ObjectMergeDecision::Delete))
                .collect();
            return Ok(StarmapGraphMergeOutput {
                graph_meta_result: StarmapMergeResult::Delete,
                object_decisions: decisions,
                lifecycle_decision,
                graph_conflict: None,
            });
        }
        GraphLifecycleDecision::DeleteModifyConflict { is_local_deleted } => {
            let decisions = objects
                .iter()
                .map(|(rel, _, _, _, _, _)| (rel.clone(), ObjectMergeDecision::NoOp))
                .collect();
            let conflict_kind = if *is_local_deleted {
                crate::sync::types::SyncConflictKind::BothChanged
            } else {
                crate::sync::types::SyncConflictKind::RemoteDeleted
            };
            return Ok(StarmapGraphMergeOutput {
                graph_meta_result: StarmapMergeResult::NoOp,
                object_decisions: decisions,
                lifecycle_decision,
                graph_conflict: Some(conflict_kind),
            });
        }
        GraphLifecycleDecision::Normal => {}
    }

    // The graph membership lists are authoritative. Files left behind after a
    // member is removed are orphans and must not be appended back into graph.json.
    let initial_members = merge_member_ids(
        base_meta.as_ref(),
        local_meta.as_ref(),
        incoming_meta.as_ref(),
    );

    // --- 2. 对每个对象做三方决策，收集最终胜出 bytes ---
    let mut object_decisions: Vec<(std::path::PathBuf, ObjectMergeDecision)> = Vec::new();
    let mut final_objects: HashMap<(String, String), Vec<u8>> = HashMap::new();
    let mut node_decisions: HashMap<String, ObjectMergeDecision> = HashMap::new();
    let mut edge_decisions: HashMap<String, ObjectMergeDecision> = HashMap::new();
    let mut embed_decisions: HashMap<String, ObjectMergeDecision> = HashMap::new();
    let mut link_decisions: HashMap<String, ObjectMergeDecision> = HashMap::new();

    for (rel_path, object_type, object_id, base, local, incoming) in objects {
        let version_key = rel_path.to_string_lossy().to_string();
        let (local_record, incoming_record) = object_versions
            .get(&version_key)
            .map(|(local_record, incoming_record)| {
                (local_record.as_ref(), incoming_record.as_ref())
            })
            .unwrap_or((None, None));
        let decision = if is_object_still_exists(object_type, object_id, &initial_members.sets) {
            merge_starmap_object_with_versions(
                base.as_deref(),
                local.as_deref(),
                incoming.as_deref(),
                local_record,
                incoming_record,
            )
        } else {
            ObjectMergeDecision::Delete
        };

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
            _ => {}
        }

        object_decisions.push((rel_path.clone(), decision));
    }

    // 安全网：三方 graph 都不存在时没有任何成员引用对象。保留对象级 Delete
    // 决策，让调用方清理遗留文件；不创建凭空存在的 graph.json。
    if graph_base.is_none() && graph_local.is_none() && graph_incoming.is_none() {
        return Ok(StarmapGraphMergeOutput {
            graph_meta_result: StarmapMergeResult::NoOp,
            object_decisions,
            lifecycle_decision,
            graph_conflict: None,
        });
    }

    let (merged_starmap_id, merged_schema_version) = resolve_graph_meta_header(
        base_meta.as_ref(),
        local_meta.as_ref(),
        incoming_meta.as_ref(),
    )?;

    // --- 3. 从 final_objects 构建按类型分组的最终对象 ID 集合 ---
    let mut final_node: HashSet<String> = HashSet::new();
    let mut final_edge: HashSet<String> = HashSet::new();
    let mut final_embed: HashSet<String> = HashSet::new();
    let mut final_link: HashSet<String> = HashSet::new();
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
            _ => {}
        }
    }

    // --- 5. 用对象决策修正成员表 ---
    let node_ids = reconcile_member_list(&initial_members.node_ids, &final_node);
    let edge_ids = reconcile_member_list(&initial_members.edge_ids, &final_edge);
    let embed_ids = reconcile_member_list(&initial_members.embed_ids, &final_embed);
    let link_ids = reconcile_member_list(&initial_members.link_ids, &final_link);

    // --- 6. 检查一致性：成员表中的每个 ID 必须有对应胜出对象 bytes ---
    check_member_object_consistency("node", &node_ids, &final_objects)?;
    check_member_object_consistency("edge", &edge_ids, &final_objects)?;
    check_member_object_consistency("embed", &embed_ids, &final_objects)?;
    check_member_object_consistency("link", &link_ids, &final_objects)?;

    let sets = MemberIdSets {
        node_ids: node_ids.iter().cloned().collect(),
        edge_ids: edge_ids.iter().cloned().collect(),
        embed_ids: embed_ids.iter().cloned().collect(),
        link_ids: link_ids.iter().cloned().collect(),
    };
    let member_ids = MergedMemberIds {
        node_ids: node_ids.clone(),
        edge_ids: edge_ids.clone(),
        embed_ids: embed_ids.clone(),
        link_ids: link_ids.clone(),
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

    // revision 补记：无条件根据最终成员表整理 revision（issue #902 第十二轮复核）。
    // 此前只在 has_actual_change 时才调用 apply_revision_by_decisions，导致
    // 纯 KeepLocal/NoOp/Conflict 的合并不会补记缺失的 revision 条目，
    // snapshot.rs 增量模式会漏发这些对象。
    // package_revision 的提升仍基于 has_actual_change。
    {
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
        apply_revision_by_decisions(
            &mut revisions.node,
            &node_refs,
            &member_ids.sets.node_ids,
            package_revision,
        );
        apply_revision_by_decisions(
            &mut revisions.edge,
            &edge_refs,
            &member_ids.sets.edge_ids,
            package_revision,
        );
        apply_revision_by_decisions(
            &mut revisions.embed,
            &embed_refs,
            &member_ids.sets.embed_ids,
            package_revision,
        );
        apply_revision_by_decisions(
            &mut revisions.link,
            &link_refs,
            &member_ids.sets.link_ids,
            package_revision,
        );
    }

    // --- 10. 构造最终 GraphMeta ---
    let merged = GraphMeta {
        schema_version: merged_schema_version,
        starmap_id: merged_starmap_id,
        node_ids,
        edge_ids,
        embed_instance_ids: embed_ids,
        link_ids,
        edge_relation_index: derived.edge_relation_index,
        embed_host_index: derived.embed_host_index,
        link_relation_index: derived.link_relation_index,
        node_kind_counts: derived.node_kind_counts,
        node_revisions: revisions.node,
        edge_revisions: revisions.edge,
        embed_revisions: revisions.embed,
        link_revisions: revisions.link,
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
        lifecycle_decision,
        graph_conflict: None,
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

        let result =
            merge_starmap_index(Some(&base), Some(&local), Some(&incoming), &HashMap::new())
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

        let result =
            merge_starmap_index(Some(&base), Some(&local), Some(&incoming), &HashMap::new())
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

        let result =
            merge_starmap_index(Some(&base), Some(&local), Some(&incoming), &HashMap::new())
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

        let result =
            merge_starmap_index(Some(&base), Some(&local), Some(&incoming), &HashMap::new())
                .expect("merge should succeed");
        let bytes = match result {
            StarmapMergeResult::ApplyMerged(b) | StarmapMergeResult::ApplyIncoming(b) => b,
            StarmapMergeResult::KeepLocal => local,
            StarmapMergeResult::NoOp | StarmapMergeResult::Delete => base,
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

        let result =
            merge_starmap_index(Some(&base), Some(&local), Some(&incoming), &HashMap::new())
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

        let result =
            merge_starmap_index(Some(&base), Some(&local), Some(&incoming), &HashMap::new())
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

        let result =
            merge_starmap_index(Some(&base), Some(&local), Some(&incoming), &HashMap::new())
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

        let result =
            merge_starmap_index(Some(&base), Some(&local), Some(&incoming), &HashMap::new())
                .expect("merge should succeed");
        let bytes = match result {
            StarmapMergeResult::ApplyMerged(b) | StarmapMergeResult::ApplyIncoming(b) => b,
            StarmapMergeResult::KeepLocal => local,
            StarmapMergeResult::NoOp | StarmapMergeResult::Delete => base,
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

        let result =
            merge_starmap_index(Some(&base), Some(&local), Some(&incoming), &HashMap::new())
                .expect("merge should succeed");
        let bytes = match result {
            StarmapMergeResult::ApplyMerged(b) | StarmapMergeResult::ApplyIncoming(b) => b,
            StarmapMergeResult::KeepLocal => local,
            StarmapMergeResult::NoOp | StarmapMergeResult::Delete => base,
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
        // base 存在、local 不存在、incoming != base（incoming 修改了）
        // → 删除-修改冲突。incoming 实际存在，用 BothChanged 而非 RemoteDeleted
        // （issue #902 第十一轮复核）
        let d = merge_starmap_object(Some(b"base"), None, Some(b"modified"));
        match d {
            ObjectMergeDecision::Conflict(crate::sync::types::SyncConflictKind::BothChanged) => {}
            other => panic!("expected Conflict(BothChanged), got {:?}", other),
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

    // =========================================================================
    // issue #902 第十一轮复核回归测试
    // =========================================================================

    /// 构造 GraphMeta bytes 用于 merge_starmap_graph_with_objects 测试。
    fn make_graph_meta_bytes(
        starmap_id: &str,
        node_ids: &[&str],
        node_revisions: &[(&str, u64)],
        package_revision: u64,
        updated_at: u64,
    ) -> Vec<u8> {
        use crate::starmap::store::meta::{DeletedSinceLastSync, GraphMeta};
        let mut node_revs: HashMap<String, u64> = HashMap::new();
        for (id, rev) in node_revisions {
            node_revs.insert(id.to_string(), *rev);
        }
        let meta = GraphMeta {
            schema_version: crate::starmap::store::meta::CURRENT_SCHEMA_VERSION.to_string(),
            starmap_id: starmap_id.to_string(),
            node_ids: node_ids.iter().map(|s| s.to_string()).collect(),
            edge_ids: vec![],
            embed_instance_ids: vec![],
            link_ids: vec![],
            edge_relation_index: vec![],
            embed_host_index: vec![],
            link_relation_index: vec![],
            node_kind_counts: HashMap::new(),
            node_revisions: node_revs,
            edge_revisions: HashMap::new(),
            embed_revisions: HashMap::new(),
            link_revisions: HashMap::new(),
            package_revision,
            updated_at,
            deleted_since_last_sync: DeletedSinceLastSync::default(),
        };
        serde_json::to_vec_pretty(&meta).expect("serialize graph meta")
    }

    // --- 问题1 回归：对象 Delete 决策（live 仍有文件时 run.rs 应发 Delete 动作）---

    #[test]
    fn test_graph_merge_object_delete_when_live_present() {
        // base 有对象 n1，local 仍有 n1（未改），incoming 删除 n1
        // → object_decisions 中 n1 的决策应为 Delete
        // run.rs 根据 obj_local.is_some() 发出 CommitAction::Delete
        let graph_base = make_graph_meta_bytes("sm1", &["n1"], &[("n1", 1)], 1, 100);
        let graph_local = graph_base.clone();
        let graph_incoming = make_graph_meta_bytes("sm1", &[], &[], 2, 200);

        let node_base = br#"{"id":"n1","kind":"text"}"#.to_vec();
        let node_local = node_base.clone();

        let objects = vec![(
            std::path::PathBuf::from("starmaps/sm1/nodes/00/n1.json"),
            "node".to_string(),
            "n1".to_string(),
            Some(node_base),
            Some(node_local),
            None,
        )];

        let output = merge_starmap_graph_with_objects(
            "sm1",
            Some(&graph_base),
            Some(&graph_local),
            Some(&graph_incoming),
            None,
            None,
            None,
            &objects,
        )
        .expect("merge should succeed");

        let n1_decision = output
            .object_decisions
            .iter()
            .find(|(rel, _)| rel.to_string_lossy().ends_with("n1.json"))
            .map(|(_, d)| d)
            .expect("n1 decision should exist");
        assert!(
            matches!(n1_decision, ObjectMergeDecision::Delete),
            "expected Delete, got {:?}",
            n1_decision
        );
    }

    // --- 问题2 回归：本地删除+远端修改 → BothChanged（非 RemoteDeleted）---

    #[test]
    fn test_issue902_local_delete_remote_modify_is_both_changed() {
        // base 存在、local 不存在（本地删除）、incoming != base（远端修改）
        // → Conflict(BothChanged)，不是 Conflict(RemoteDeleted)
        let d = merge_starmap_object(Some(b"base"), None, Some(b"modified"));
        match d {
            ObjectMergeDecision::Conflict(crate::sync::types::SyncConflictKind::BothChanged) => {}
            other => panic!("expected Conflict(BothChanged), got {:?}", other),
        }
    }

    #[test]
    fn test_issue902_remote_delete_local_modify_is_remote_deleted() {
        // base 存在、local != base（本地修改）、incoming 不存在（远端删除）
        // → Conflict(RemoteDeleted)（远端确实不存在文件）
        let d = merge_starmap_object(Some(b"base"), Some(b"modified"), None);
        match d {
            ObjectMergeDecision::Conflict(crate::sync::types::SyncConflictKind::RemoteDeleted) => {}
            other => panic!("expected Conflict(RemoteDeleted), got {:?}", other),
        }
    }

    // --- 问题3 回归：整图删除 → StarmapMergeResult::Delete ---

    #[test]
    fn test_issue902_entire_graph_delete_local_unchanged() {
        // base 有 graph，local 未改（graph_local == graph_base），
        // incoming 删除了整个 graph（graph_incoming = None）
        // → StarmapMergeResult::Delete，所有对象也是 Delete
        let graph_base = make_graph_meta_bytes("sm1", &["n1"], &[("n1", 1)], 1, 100);
        let graph_local = graph_base.clone();

        let node_base = br#"{"id":"n1","kind":"text"}"#.to_vec();
        let node_local = node_base.clone();

        let objects = vec![(
            std::path::PathBuf::from("starmaps/sm1/nodes/00/n1.json"),
            "node".to_string(),
            "n1".to_string(),
            Some(node_base),
            Some(node_local),
            None,
        )];

        let output = merge_starmap_graph_with_objects(
            "sm1",
            Some(&graph_base),
            Some(&graph_local),
            None,
            None,
            None,
            None,
            &objects,
        )
        .expect("merge should succeed");

        assert!(
            matches!(output.graph_meta_result, StarmapMergeResult::Delete),
            "expected Delete, got {:?}",
            output.graph_meta_result
        );
        for (_, decision) in &output.object_decisions {
            assert!(
                matches!(decision, ObjectMergeDecision::Delete),
                "expected object Delete, got {:?}",
                decision
            );
        }
    }

    #[test]
    fn test_issue902_entire_graph_delete_local_modified_is_conflict() {
        // base 有 graph，local 修改了（graph_local != graph_base），
        // incoming 删除了整个 graph（graph_incoming = None）
        // → 删除-修改冲突：StarmapMergeResult::NoOp，graph_conflict=Some(RemoteDeleted)
        // 整图生命周期冲突未裁决：所有子对象决策为 NoOp，不执行任何 Apply/Delete，
        // 等用户选择整图版本后再基于胜出整图生成完整 CommitPlan。
        // issue #902 第十四轮复核：对象不再被提前写盘
        let graph_base = make_graph_meta_bytes("sm1", &["n1"], &[("n1", 1)], 1, 100);
        let graph_local =
            make_graph_meta_bytes("sm1", &["n1", "n2"], &[("n1", 1), ("n2", 2)], 2, 200);

        let node_base = br#"{"id":"n1","kind":"text"}"#.to_vec();
        let node_local = node_base.clone();

        let objects = vec![(
            std::path::PathBuf::from("starmaps/sm1/nodes/00/n1.json"),
            "node".to_string(),
            "n1".to_string(),
            Some(node_base),
            Some(node_local),
            None,
        )];

        let output = merge_starmap_graph_with_objects(
            "sm1",
            Some(&graph_base),
            Some(&graph_local),
            None,
            None,
            None,
            None,
            &objects,
        )
        .expect("merge should succeed");

        assert!(
            matches!(output.graph_meta_result, StarmapMergeResult::NoOp),
            "expected NoOp for delete-modify conflict, got {:?}",
            output.graph_meta_result
        );
        assert!(
            matches!(
                output.graph_conflict,
                Some(crate::sync::types::SyncConflictKind::RemoteDeleted)
            ),
            "expected graph_conflict Some(RemoteDeleted), got {:?}",
            output.graph_conflict
        );
        for (_, decision) in &output.object_decisions {
            assert!(
                matches!(decision, ObjectMergeDecision::NoOp),
                "expected object NoOp (graph lifecycle conflict unresolved, no sub-object action), got {:?}",
                decision
            );
        }
    }

    // --- 问题3 第十三轮复核回归：整图删除-修改冲突对象使用真实三方合并结果 ---
    // issue #902 第十四轮复核：整图生命周期冲突未裁决时，子对象全部 NoOp。

    #[test]
    fn test_issue902_local_delete_remote_modify_object_real_merge() {
        // 本地删除整图、远端修改 graph 但同时删除了某些节点。
        // 对象 n1: base=Some, local=None, incoming=None（远端也删了 n1）
        // 整图生命周期冲突未裁决：子对象全部 NoOp，不执行任何 Apply/Delete。
        // 修改前（第十三轮）：返回真实三方结果 Delete；但正常 Commit 会立即删除
        // n1.json，而 graph.json 仍引用 n1，下次加载找不到文件。
        let graph_base = make_graph_meta_bytes("sm1", &["n1"], &[("n1", 1)], 1, 100);
        let graph_incoming = make_graph_meta_bytes("sm1", &[], &[], 2, 200);

        let node_base = br#"{"id":"n1","kind":"text"}"#.to_vec();

        let objects = vec![(
            std::path::PathBuf::from("starmaps/sm1/nodes/00/n1.json"),
            "node".to_string(),
            "n1".to_string(),
            Some(node_base),
            None,
            None,
        )];

        let output = merge_starmap_graph_with_objects(
            "sm1",
            Some(&graph_base),
            None,
            Some(&graph_incoming),
            None,
            None,
            None,
            &objects,
        )
        .expect("merge should succeed");

        assert!(
            matches!(output.graph_meta_result, StarmapMergeResult::NoOp),
            "expected NoOp for delete-modify conflict, got {:?}",
            output.graph_meta_result
        );
        assert!(
            matches!(
                output.graph_conflict,
                Some(crate::sync::types::SyncConflictKind::BothChanged)
            ),
            "expected graph_conflict Some(BothChanged), got {:?}",
            output.graph_conflict
        );
        for (_, decision) in &output.object_decisions {
            assert!(
                matches!(decision, ObjectMergeDecision::NoOp),
                "expected object NoOp (graph lifecycle conflict unresolved, no sub-object action), got {:?}",
                decision
            );
        }
    }

    #[test]
    fn test_issue902_remote_delete_local_modify_unmodified_object_real_merge() {
        // 远端删除整图、本地修改了 graph（加了 n2）但 n1 本身未改。
        // 整图生命周期冲突未裁决：子对象全部 NoOp，不执行任何 Apply/Delete。
        // 修改前（第十三轮）：n1→Delete、n2→KeepLocal；但正常 Commit 会立即对
        // n1 执行 Delete 删除 n1.json，而 graph.json 仍包含 n1，下次加载找不到文件。
        // issue #902 第十四轮复核：子对象全部 NoOp，等用户裁决整图后再处理。
        let graph_base = make_graph_meta_bytes("sm1", &["n1"], &[("n1", 1)], 1, 100);
        let graph_local =
            make_graph_meta_bytes("sm1", &["n1", "n2"], &[("n1", 1), ("n2", 2)], 2, 200);

        let node1_base = br#"{"id":"n1","kind":"text"}"#.to_vec();
        let node1_local = node1_base.clone(); // n1 未改
        let node2_local = br#"{"id":"n2","kind":"text"}"#.to_vec(); // n2 本地新增

        let objects = vec![
            (
                std::path::PathBuf::from("starmaps/sm1/nodes/00/n1.json"),
                "node".to_string(),
                "n1".to_string(),
                Some(node1_base),
                Some(node1_local),
                None,
            ),
            (
                std::path::PathBuf::from("starmaps/sm1/nodes/00/n2.json"),
                "node".to_string(),
                "n2".to_string(),
                None,
                Some(node2_local),
                None,
            ),
        ];

        let output = merge_starmap_graph_with_objects(
            "sm1",
            Some(&graph_base),
            Some(&graph_local),
            None,
            None,
            None,
            None,
            &objects,
        )
        .expect("merge should succeed");

        assert!(
            matches!(output.graph_meta_result, StarmapMergeResult::NoOp),
            "expected NoOp for delete-modify conflict, got {:?}",
            output.graph_meta_result
        );
        assert!(
            matches!(
                output.graph_conflict,
                Some(crate::sync::types::SyncConflictKind::RemoteDeleted)
            ),
            "expected graph_conflict Some(RemoteDeleted), got {:?}",
            output.graph_conflict
        );

        let n1_decision = output
            .object_decisions
            .iter()
            .find(|(rel, _)| rel.to_string_lossy().ends_with("n1.json"))
            .map(|(_, d)| d)
            .expect("n1 decision should exist");
        assert!(
            matches!(n1_decision, ObjectMergeDecision::NoOp),
            "expected n1 NoOp (graph lifecycle conflict unresolved, no sub-object action), got {:?}",
            n1_decision
        );

        let n2_decision = output
            .object_decisions
            .iter()
            .find(|(rel, _)| rel.to_string_lossy().ends_with("n2.json"))
            .map(|(_, d)| d)
            .expect("n2 decision should exist");
        assert!(
            matches!(n2_decision, ObjectMergeDecision::NoOp),
            "expected n2 NoOp (graph lifecycle conflict unresolved, no sub-object action), got {:?}",
            n2_decision
        );
    }

    // --- issue #902 第十四轮复核回归：整图删除-修改冲突时子对象全部 NoOp ---
    // 反例：base 图有 n1；本地为图新增 n2（graph 已修改），远端删除整图。
    // graph 级标记 RemoteDeleted 并保留本地 graph；但未修改过的 n1 若产生 Delete
    // 并在 Commit 立即删除 n1.json，本地 graph.json 仍包含 n1，下次加载找不到文件。
    // 修复后：DeleteModifyConflict 分支所有 object_decisions 为 NoOp。

    #[test]
    fn test_issue902_r14_delete_modify_conflict_all_objects_noop_remote_deleted() {
        // 远端删除整图、本地修改 graph（新增 n2），base 有 n1（未改）。
        // 期望：graph_conflict=Some(RemoteDeleted)，所有对象 NoOp。
        let graph_base = make_graph_meta_bytes("sm1", &["n1"], &[("n1", 1)], 1, 100);
        let graph_local =
            make_graph_meta_bytes("sm1", &["n1", "n2"], &[("n1", 1), ("n2", 2)], 2, 200);

        let node1_base = br#"{"id":"n1","kind":"text"}"#.to_vec();
        let node1_local = node1_base.clone(); // n1 未改
        let node2_local = br#"{"id":"n2","kind":"text"}"#.to_vec(); // n2 本地新增

        let objects = vec![
            (
                std::path::PathBuf::from("starmaps/sm1/nodes/00/n1.json"),
                "node".to_string(),
                "n1".to_string(),
                Some(node1_base),
                Some(node1_local),
                None,
            ),
            (
                std::path::PathBuf::from("starmaps/sm1/nodes/00/n2.json"),
                "node".to_string(),
                "n2".to_string(),
                None,
                Some(node2_local),
                None,
            ),
        ];

        let output = merge_starmap_graph_with_objects(
            "sm1",
            Some(&graph_base),
            Some(&graph_local),
            None,
            None,
            None,
            None,
            &objects,
        )
        .expect("merge should succeed");

        assert!(
            matches!(output.graph_meta_result, StarmapMergeResult::NoOp),
            "expected NoOp for delete-modify conflict, got {:?}",
            output.graph_meta_result
        );
        assert!(
            matches!(
                output.graph_conflict,
                Some(crate::sync::types::SyncConflictKind::RemoteDeleted)
            ),
            "expected graph_conflict Some(RemoteDeleted), got {:?}",
            output.graph_conflict
        );
        // 关键断言：所有子对象决策为 NoOp，不产生任何 Apply/Delete
        assert_eq!(
            output.object_decisions.len(),
            objects.len(),
            "object_decisions count should match objects count"
        );
        for (rel, decision) in &output.object_decisions {
            assert!(
                matches!(decision, ObjectMergeDecision::NoOp),
                "expected NoOp for {} (graph lifecycle conflict unresolved), got {:?}",
                rel.display(),
                decision
            );
        }
    }

    #[test]
    fn test_issue902_r14_delete_modify_conflict_all_objects_noop_local_deleted() {
        // 本地删除整图、远端修改 graph（新增 n2），base 有 n1。
        // 期望：graph_conflict=Some(BothChanged)，所有对象 NoOp。
        let graph_base = make_graph_meta_bytes("sm1", &["n1"], &[("n1", 1)], 1, 100);
        let graph_incoming =
            make_graph_meta_bytes("sm1", &["n1", "n2"], &[("n1", 1), ("n2", 2)], 2, 200);

        let node1_base = br#"{"id":"n1","kind":"text"}"#.to_vec();
        let node1_incoming = node1_base.clone(); // n1 远端未改
        let node2_incoming = br#"{"id":"n2","kind":"text"}"#.to_vec(); // n2 远端新增

        let objects = vec![
            (
                std::path::PathBuf::from("starmaps/sm1/nodes/00/n1.json"),
                "node".to_string(),
                "n1".to_string(),
                Some(node1_base),
                None,
                Some(node1_incoming),
            ),
            (
                std::path::PathBuf::from("starmaps/sm1/nodes/00/n2.json"),
                "node".to_string(),
                "n2".to_string(),
                None,
                None,
                Some(node2_incoming),
            ),
        ];

        let output = merge_starmap_graph_with_objects(
            "sm1",
            Some(&graph_base),
            None,
            Some(&graph_incoming),
            None,
            None,
            None,
            &objects,
        )
        .expect("merge should succeed");

        assert!(
            matches!(output.graph_meta_result, StarmapMergeResult::NoOp),
            "expected NoOp for delete-modify conflict, got {:?}",
            output.graph_meta_result
        );
        assert!(
            matches!(
                output.graph_conflict,
                Some(crate::sync::types::SyncConflictKind::BothChanged)
            ),
            "expected graph_conflict Some(BothChanged), got {:?}",
            output.graph_conflict
        );
        // 关键断言：所有子对象决策为 NoOp，不产生任何 Apply/Delete
        assert_eq!(
            output.object_decisions.len(),
            objects.len(),
            "object_decisions count should match objects count"
        );
        for (rel, decision) in &output.object_decisions {
            assert!(
                matches!(decision, ObjectMergeDecision::NoOp),
                "expected NoOp for {} (graph lifecycle conflict unresolved), got {:?}",
                rel.display(),
                decision
            );
        }
    }

    // --- 问题4 回归：新对象缺 revision 时 apply_revision_by_decisions 补记 ---

    #[test]
    fn test_issue902_apply_revision_new_member_gets_revision() {
        // node_ids=[n2] 但 revision map 缺 n2
        // ApplyIncoming 后 revision map 中 n2 的 revision 等于 new_pr
        let mut revisions: HashMap<String, u64> = HashMap::new();
        // n2 不在 revisions 中（新增对象缺 revision）

        let incoming_content = br#"{"id":"n2","kind":"text"}"#.to_vec();
        let decision = ObjectMergeDecision::ApplyIncoming(incoming_content);
        let decisions: HashMap<String, &ObjectMergeDecision> =
            HashMap::from([("n2".to_string(), &decision)]);

        let mut final_member_ids: HashSet<String> = HashSet::new();
        final_member_ids.insert("n2".to_string());

        let new_pr = 10u64;
        apply_revision_by_decisions(&mut revisions, &decisions, &final_member_ids, new_pr);

        assert_eq!(
            revisions.get("n2"),
            Some(&new_pr),
            "new member n2 should have revision == new_pr"
        );
    }

    #[test]
    fn test_issue902_apply_revision_removes_deleted_members() {
        // revisions 有 n1 和 n2，n1 被删除（不在 final_member_ids 中）
        // → n1 的 revision 被移除，n2 保留
        let mut revisions: HashMap<String, u64> =
            HashMap::from([("n1".to_string(), 5), ("n2".to_string(), 8)]);

        let decisions: HashMap<String, &ObjectMergeDecision> = HashMap::new();

        let mut final_member_ids: HashSet<String> = HashSet::new();
        final_member_ids.insert("n2".to_string()); // n1 不在最终成员表中

        apply_revision_by_decisions(&mut revisions, &decisions, &final_member_ids, 10);

        assert!(
            !revisions.contains_key("n1"),
            "deleted member n1 should be removed from revisions"
        );
        assert!(
            revisions.contains_key("n2"),
            "retained member n2 should still have revision"
        );
    }

    #[test]
    fn test_issue902_apply_revision_keep_local_preserves_revision() {
        // KeepLocal 决策：保留当前 revision，如果没有则补记 new_pr
        let mut revisions: HashMap<String, u64> = HashMap::from([("n1".to_string(), 7)]);

        let decision = ObjectMergeDecision::KeepLocal;
        let decisions: HashMap<String, &ObjectMergeDecision> =
            HashMap::from([("n1".to_string(), &decision)]);

        let mut final_member_ids: HashSet<String> = HashSet::new();
        final_member_ids.insert("n1".to_string());

        let new_pr = 10u64;
        apply_revision_by_decisions(&mut revisions, &decisions, &final_member_ids, new_pr);

        // KeepLocal 保留当前 revision（7），不提升到 new_pr
        assert_eq!(
            revisions.get("n1"),
            Some(&7),
            "KeepLocal should preserve existing revision"
        );
    }

    #[test]
    fn test_issue902_apply_revision_keep_local_missing_gets_new_pr() {
        // KeepLocal 决策但 revision map 中没有该 ID → 补记 new_pr
        let mut revisions: HashMap<String, u64> = HashMap::new();

        let decision = ObjectMergeDecision::KeepLocal;
        let decisions: HashMap<String, &ObjectMergeDecision> =
            HashMap::from([("n1".to_string(), &decision)]);

        let mut final_member_ids: HashSet<String> = HashSet::new();
        final_member_ids.insert("n1".to_string());

        let new_pr = 10u64;
        apply_revision_by_decisions(&mut revisions, &decisions, &final_member_ids, new_pr);

        assert_eq!(
            revisions.get("n1"),
            Some(&new_pr),
            "KeepLocal with missing revision should get new_pr"
        );
    }

    // --- resolve_graph_lifecycle 单元测试（issue #902 第十二轮复核）---
    // issue #902 第十七轮复核：签名扩展纳入 meta bytes，现有测试传 None 保持原语义。

    #[test]
    fn test_resolve_graph_lifecycle_base_none_new_graph() {
        // base 不存在 → Normal（新建图，走常规合并）
        let d = resolve_graph_lifecycle(None, Some(b"local"), Some(b"incoming"), None, None, None);
        assert!(matches!(d, GraphLifecycleDecision::Normal), "got {:?}", d);
    }

    #[test]
    fn test_resolve_graph_lifecycle_base_none_local_none() {
        // base 不存在、local 也不存在 → Normal
        let d = resolve_graph_lifecycle(None, None, Some(b"incoming"), None, None, None);
        assert!(matches!(d, GraphLifecycleDecision::Normal), "got {:?}", d);
    }

    #[test]
    fn test_resolve_graph_lifecycle_all_present() {
        // base、local、incoming 都存在 → Normal
        let d = resolve_graph_lifecycle(
            Some(b"base"),
            Some(b"local"),
            Some(b"incoming"),
            None,
            None,
            None,
        );
        assert!(matches!(d, GraphLifecycleDecision::Normal), "got {:?}", d);
    }

    #[test]
    fn test_resolve_graph_lifecycle_remote_delete_local_unchanged() {
        // base 存在、local == base（未改）、incoming 不存在（远端删除）→ Delete
        let d = resolve_graph_lifecycle(Some(b"base"), Some(b"base"), None, None, None, None);
        assert!(matches!(d, GraphLifecycleDecision::Delete), "got {:?}", d);
    }

    #[test]
    fn test_resolve_graph_lifecycle_remote_delete_local_modified() {
        // base 存在、local != base（修改了）、incoming 不存在（远端删除）
        // → DeleteModifyConflict{is_local_deleted: false}
        let d = resolve_graph_lifecycle(Some(b"base"), Some(b"modified"), None, None, None, None);
        match d {
            GraphLifecycleDecision::DeleteModifyConflict {
                is_local_deleted: false,
            } => {}
            other => panic!(
                "expected DeleteModifyConflict{{is_local_deleted: false}}, got {:?}",
                other
            ),
        }
    }

    #[test]
    fn test_resolve_graph_lifecycle_local_delete_incoming_unchanged() {
        // base 存在、local 不存在（本地删除）、incoming == base（未改）→ Delete
        let d = resolve_graph_lifecycle(Some(b"base"), None, Some(b"base"), None, None, None);
        assert!(matches!(d, GraphLifecycleDecision::Delete), "got {:?}", d);
    }

    #[test]
    fn test_resolve_graph_lifecycle_local_delete_incoming_modified() {
        // base 存在、local 不存在（本地删除）、incoming != base（远端修改）
        // → DeleteModifyConflict{is_local_deleted: true}
        let d = resolve_graph_lifecycle(Some(b"base"), None, Some(b"modified"), None, None, None);
        match d {
            GraphLifecycleDecision::DeleteModifyConflict {
                is_local_deleted: true,
            } => {}
            other => panic!(
                "expected DeleteModifyConflict{{is_local_deleted: true}}, got {:?}",
                other
            ),
        }
    }

    #[test]
    fn test_resolve_graph_lifecycle_both_delete() {
        // base 存在、local 和 incoming 都不存在（双端删除）→ Delete
        let d = resolve_graph_lifecycle(Some(b"base"), None, None, None, None, None);
        assert!(matches!(d, GraphLifecycleDecision::Delete), "got {:?}", d);
    }

    // --- issue #902 第十七轮复核问题2 新增测试 ---
    // 整图实体 = graph + meta，只改 meta 也应触发 DeleteModifyConflict。

    #[test]
    fn test_resolve_graph_lifecycle_remote_delete_local_meta_only_modified() {
        // base 有 graph+meta、local graph 未改但 meta 改了、incoming 无（远端删除）
        // → DeleteModifyConflict{is_local_deleted: false}
        // 此前只看 graph bytes 会误判为"未修改"→ Delete，丢失本地 meta 变更。
        let d = resolve_graph_lifecycle(
            Some(b"graph"),
            Some(b"graph"),
            None,
            Some(b"meta_base"),
            Some(b"meta_modified"),
            None,
        );
        match d {
            GraphLifecycleDecision::DeleteModifyConflict {
                is_local_deleted: false,
            } => {}
            other => panic!(
                "expected DeleteModifyConflict{{is_local_deleted: false}} (meta modified), got {:?}",
                other
            ),
        }
    }

    #[test]
    fn test_resolve_graph_lifecycle_local_delete_incoming_meta_only_modified() {
        // base 有 graph+meta、local 无（本地删除）、incoming graph 未改但 meta 改了
        // → DeleteModifyConflict{is_local_deleted: true}
        let d = resolve_graph_lifecycle(
            Some(b"graph"),
            None,
            Some(b"graph"),
            Some(b"meta_base"),
            None,
            Some(b"meta_modified"),
        );
        match d {
            GraphLifecycleDecision::DeleteModifyConflict {
                is_local_deleted: true,
            } => {}
            other => panic!(
                "expected DeleteModifyConflict{{is_local_deleted: true}} (meta modified), got {:?}",
                other
            ),
        }
    }

    #[test]
    fn test_resolve_graph_lifecycle_meta_only_no_graph_all_present() {
        // graph 全 None、meta 三方都存在 → Normal（整图存在）
        let d = resolve_graph_lifecycle(
            None,
            None,
            None,
            Some(b"meta_base"),
            Some(b"meta_local"),
            Some(b"meta_incoming"),
        );
        assert!(matches!(d, GraphLifecycleDecision::Normal), "got {:?}", d);
    }

    #[test]
    fn test_resolve_graph_lifecycle_meta_only_remote_delete_unchanged() {
        // graph 全 None、base+local 有 meta 且未改、incoming 无 meta → Delete
        let d = resolve_graph_lifecycle(None, None, None, Some(b"meta"), Some(b"meta"), None);
        assert!(matches!(d, GraphLifecycleDecision::Delete), "got {:?}", d);
    }

    #[test]
    fn test_resolve_graph_lifecycle_meta_only_remote_delete_modified() {
        // graph 全 None、base+local 有 meta 且 local 改了、incoming 无 meta
        // → DeleteModifyConflict{is_local_deleted: false}
        let d = resolve_graph_lifecycle(
            None,
            None,
            None,
            Some(b"meta_base"),
            Some(b"meta_modified"),
            None,
        );
        match d {
            GraphLifecycleDecision::DeleteModifyConflict {
                is_local_deleted: false,
            } => {}
            other => panic!(
                "expected DeleteModifyConflict{{is_local_deleted: false}} (meta-only), got {:?}",
                other
            ),
        }
    }

    // =========================================================================
    // merge_starmap_index with graph_lifecycle_decisions 测试
    // （issue #902 第十二轮复核）
    // =========================================================================

    /// 从 StarmapMergeResult 提取完整的 StarMapIndexRecord。
    fn extract_index_record(
        result: &StarmapMergeResult,
        local: &[u8],
        base: &[u8],
    ) -> StarMapIndexRecord {
        let bytes = match result {
            StarmapMergeResult::ApplyMerged(b) | StarmapMergeResult::ApplyIncoming(b) => b,
            StarmapMergeResult::KeepLocal => local,
            StarmapMergeResult::NoOp | StarmapMergeResult::Delete => base,
        };
        serde_json::from_slice(bytes).expect("deserialize index")
    }

    // --- 测试 1: DeleteModifyConflict 时星图 ID 保留在 starmap_ids ---

    #[test]
    fn test_merge_index_delete_modify_conflict_retains_starmap_ids() {
        // base 含 A，local 含 A，incoming 不含 A（远端删除）
        // graph_lifecycle_decisions 中 A = DeleteModifyConflict{is_local_deleted: false}
        // 合并后 A 仍在 merged starmap_ids 中
        let base = make_index_bytes(&["A"], &[], &[], 0);
        let local = make_index_bytes(&["A"], &[], &[], 100);
        let incoming = make_index_bytes(&[], &[], &[], 200); // 远端删除 A

        let mut decisions = HashMap::new();
        decisions.insert(
            "A".to_string(),
            GraphLifecycleDecision::DeleteModifyConflict {
                is_local_deleted: false,
            },
        );

        let result = merge_starmap_index(Some(&base), Some(&local), Some(&incoming), &decisions)
            .expect("merge should succeed");
        let record = extract_index_record(&result, &local, &base);

        assert!(
            record.starmap_ids.contains(&"A".to_string()),
            "DeleteModifyConflict should retain A in starmap_ids, got {:?}",
            record.starmap_ids
        );
    }

    // --- 测试 2: DeleteModifyConflict 时星图 ID 保留在 root_starmap_ids ---

    #[test]
    fn test_merge_index_delete_modify_conflict_retains_root_starmap_ids() {
        // base/local 的 root_starmap_ids 含 A，incoming 不含 A
        // graph_lifecycle_decisions 中 A = DeleteModifyConflict{is_local_deleted: false}
        // 合并后 A 仍在 merged root_starmap_ids 中
        let base = make_index_bytes(&["A"], &["A"], &[], 0);
        let local = make_index_bytes(&["A"], &["A"], &[], 100);
        let incoming = make_index_bytes(&[], &[], &[], 200); // 远端删除 A

        let mut decisions = HashMap::new();
        decisions.insert(
            "A".to_string(),
            GraphLifecycleDecision::DeleteModifyConflict {
                is_local_deleted: false,
            },
        );

        let result = merge_starmap_index(Some(&base), Some(&local), Some(&incoming), &decisions)
            .expect("merge should succeed");
        let record = extract_index_record(&result, &local, &base);

        assert!(
            record.starmap_ids.contains(&"A".to_string()),
            "DeleteModifyConflict should retain A in starmap_ids, got {:?}",
            record.starmap_ids
        );
        assert!(
            record.root_starmap_ids.contains(&"A".to_string()),
            "DeleteModifyConflict should retain A in root_starmap_ids, got {:?}",
            record.root_starmap_ids
        );
    }

    // --- 测试 3: DeleteModifyConflict 时星图 ID 保留在 main_starmap_by_project ---

    #[test]
    fn test_merge_index_delete_modify_conflict_retains_main_starmap_by_project() {
        // base/local 的 main_starmap_by_project 有 "proj" → "A"，incoming 没有
        // graph_lifecycle_decisions 中 A = DeleteModifyConflict{is_local_deleted: false}
        // 合并后 "proj" → "A" 仍在 merged main_starmap_by_project 中
        let base = make_index_bytes(&["A"], &[], &[("proj", "A")], 0);
        let local = make_index_bytes(&["A"], &[], &[("proj", "A")], 100);
        let incoming = make_index_bytes(&[], &[], &[], 200); // 远端删除 A

        let mut decisions = HashMap::new();
        decisions.insert(
            "A".to_string(),
            GraphLifecycleDecision::DeleteModifyConflict {
                is_local_deleted: false,
            },
        );

        let result = merge_starmap_index(Some(&base), Some(&local), Some(&incoming), &decisions)
            .expect("merge should succeed");
        let record = extract_index_record(&result, &local, &base);

        assert!(
            record.starmap_ids.contains(&"A".to_string()),
            "DeleteModifyConflict should retain A in starmap_ids, got {:?}",
            record.starmap_ids
        );
        assert_eq!(
            record.main_starmap_by_project.get("proj"),
            Some(&"A".to_string()),
            "DeleteModifyConflict should retain proj→A in main_starmap_by_project, got {:?}",
            record.main_starmap_by_project
        );
    }

    // --- 测试 4: Normal/Delete 时不受 graph_lifecycle_decisions 影响 ---

    #[test]
    fn test_merge_index_normal_delete_decisions_no_effect() {
        // graph_lifecycle_decisions 中 A = Normal 或 Delete
        // 合并结果与不传 graph_lifecycle_decisions 时一致
        let base = make_index_bytes(&["A", "B"], &[], &[], 0);
        let local = make_index_bytes(&["A"], &[], &[], 100); // local 删 B
        let incoming = make_index_bytes(&["A", "B"], &[], &[], 200); // incoming 保留 B

        // 不传 decisions 的基准结果
        let baseline =
            merge_starmap_index(Some(&base), Some(&local), Some(&incoming), &HashMap::new())
                .expect("merge should succeed");
        let baseline_ids = extract_starmap_ids(&baseline);

        // Normal decision → 不影响结果
        let mut normal_decisions = HashMap::new();
        normal_decisions.insert("B".to_string(), GraphLifecycleDecision::Normal);
        let result_normal = merge_starmap_index(
            Some(&base),
            Some(&local),
            Some(&incoming),
            &normal_decisions,
        )
        .expect("merge should succeed");
        let normal_ids = extract_starmap_ids(&result_normal);
        assert_eq!(
            normal_ids, baseline_ids,
            "Normal decision should not affect merge result"
        );

        // Delete decision → 不影响结果
        let mut delete_decisions = HashMap::new();
        delete_decisions.insert("B".to_string(), GraphLifecycleDecision::Delete);
        let result_delete = merge_starmap_index(
            Some(&base),
            Some(&local),
            Some(&incoming),
            &delete_decisions,
        )
        .expect("merge should succeed");
        let delete_ids = extract_starmap_ids(&result_delete);
        assert_eq!(
            delete_ids, baseline_ids,
            "Delete decision should not affect merge result"
        );
    }

    // --- 测试 5: 本地删除远端修改的 DeleteModifyConflict{is_local_deleted: true} ---
    // issue #902 第十三轮复核：is_local_deleted=true 时不应把图重新登记到全局 index

    #[test]
    fn test_merge_index_delete_modify_conflict_local_deleted_not_retained() {
        // base 含 A，local 不含 A（本地删除），incoming 含 A（远端保留/修改）
        // graph_lifecycle_decisions 中 A = DeleteModifyConflict{is_local_deleted: true}
        // 合并后 A 不应在 merged starmap_ids 中：本地已删除该图，live graph.json
        // 不存在，不应把图重新登记到全局 index（否则 index 指向 A、live 却没有
        // A/graph.json）。
        let base = make_index_bytes(&["A"], &[], &[], 0);
        let local = make_index_bytes(&[], &[], &[], 100); // 本地删除 A
        let incoming = make_index_bytes(&["A"], &[], &[], 200); // 远端保留 A

        let mut decisions = HashMap::new();
        decisions.insert(
            "A".to_string(),
            GraphLifecycleDecision::DeleteModifyConflict {
                is_local_deleted: true,
            },
        );

        let result = merge_starmap_index(Some(&base), Some(&local), Some(&incoming), &decisions)
            .expect("merge should succeed");
        let record = extract_index_record(&result, &local, &base);

        assert!(
            !record.starmap_ids.contains(&"A".to_string()),
            "DeleteModifyConflict{{is_local_deleted: true}} should NOT retain A in starmap_ids \
             (local deleted, live graph absent), got {:?}",
            record.starmap_ids
        );
    }
}
