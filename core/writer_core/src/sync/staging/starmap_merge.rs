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
//! - [`GraphMeta`] 三方合并（[`merge_graph_meta`]）
//!
//! 合并器是纯计算（不读磁盘、不写磁盘），只接受 bytes 输入，返回 bytes 输出。
//! 错误显式返回，不静默吞错。

use std::collections::{HashMap, HashSet};

use crate::error::{Error, Result};
use crate::starmap::store::meta::{
    DeletedSinceLastSync, DeletionEntry, GraphMeta, CURRENT_SCHEMA_VERSION,
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
/// | Some | Some  | None     | —    | KeepLocal（远端删除，本地保留）|
/// | Some | None  | Some     | —    | ApplyIncoming（本地删除，远端保留）|
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
        (Some(_), Some(_), None) => ObjectMergeDecision::KeepLocal,
        (Some(_), None, Some(iv)) => ObjectMergeDecision::ApplyIncoming(iv.to_vec()),
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
/// 返回合并后的 `Vec<String>`，保持插入顺序，去重。
///
/// # 三方删除判定
///
/// 对 `base` 中存在的 ID：
/// - 两侧都保留（local 和 incoming 都仍含此 ID）→ 保留
/// - 一侧删除、另一侧保留 base 原样（仍含此 ID 且未变）→ 接受删除（不保留）
/// - 两侧都删除 → 删除
///
/// 对非 `base` 的新增 ID（local-only 或 incoming-only）：保留（互不冲突的新增）。
///
/// # 输出顺序
///
/// 先按 `base` 原始顺序输出保留的 base ID，再按 `local` 顺序输出 local 新增的，
/// 再按 `incoming` 顺序输出 incoming 新增的（去重）。
///
/// # 局限
///
/// 本函数只看 ID 集合不看对象内容。"一侧删除一侧对该对象做实质修改（ID 仍在但
/// 其他属性变了）"的冲突检测在对象级决策（`merge_starmap_object`）中处理。
/// 对于 graph meta 的成员 ID 列表，调用方需结合 `deleted_since_last_sync`
/// tombstone 判断是否是真正的删除操作（有 tombstone 记录）还是仅仅是集合缺失。
fn three_way_merge_id_set(
    base: Option<&Vec<String>>,
    local: Option<&Vec<String>>,
    incoming: Option<&Vec<String>>,
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
    //    - 至少一侧删除 → 接受删除（不保留），避免已删除对象因"任意一端存在"而复活
    for id in base_vec {
        let in_local = local_set.contains(id);
        let in_incoming = incoming_set.contains(id);
        if in_local && in_incoming && seen.insert(id.clone()) {
            result.push(id.clone());
        }
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
struct MemberIdSets {
    node_ids: HashSet<String>,
    edge_ids: HashSet<String>,
    embed_ids: HashSet<String>,
    link_ids: HashSet<String>,
    hyperlink_ids: HashSet<String>,
}

/// 合并后的五个成员 ID 列表及其 HashSet 集合。
struct MergedMemberIds {
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

    let merged_starmap_ids = three_way_merge_id_set(
        Some(&base_rec.starmap_ids),
        Some(&local_rec.starmap_ids),
        Some(&incoming_rec.starmap_ids),
    );
    // 最终存在的星图集合：合并后的 starmap_ids。
    // root_starmap_ids 和 main_starmap_by_project 中指向已删除图的映射需清理，
    // 避免继续指向已删除图。
    let existing_starmap_ids: HashSet<String> = merged_starmap_ids.iter().cloned().collect();

    let merged_root_starmap_ids = three_way_merge_id_set(
        Some(&base_rec.root_starmap_ids),
        Some(&local_rec.root_starmap_ids),
        Some(&incoming_rec.root_starmap_ids),
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

/// 合并五个成员 ID 列表，返回合并后的 ID 和对应的 HashSet 集合。
fn merge_member_ids(
    base_meta: Option<&GraphMeta>,
    local_meta: Option<&GraphMeta>,
    incoming_meta: Option<&GraphMeta>,
) -> MergedMemberIds {
    let node_ids = three_way_merge_id_set(
        base_meta.map(|m| &m.node_ids),
        local_meta.map(|m| &m.node_ids),
        incoming_meta.map(|m| &m.node_ids),
    );
    let edge_ids = three_way_merge_id_set(
        base_meta.map(|m| &m.edge_ids),
        local_meta.map(|m| &m.edge_ids),
        incoming_meta.map(|m| &m.edge_ids),
    );
    let embed_ids = three_way_merge_id_set(
        base_meta.map(|m| &m.embed_instance_ids),
        local_meta.map(|m| &m.embed_instance_ids),
        incoming_meta.map(|m| &m.embed_instance_ids),
    );
    let link_ids = three_way_merge_id_set(
        base_meta.map(|m| &m.link_ids),
        local_meta.map(|m| &m.link_ids),
        incoming_meta.map(|m| &m.link_ids),
    );
    let hyperlink_ids = three_way_merge_id_set(
        base_meta.map(|m| &m.hyperlink_ids),
        local_meta.map(|m| &m.hyperlink_ids),
        incoming_meta.map(|m| &m.hyperlink_ids),
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

/// 比较两个 `Option<&[u8]>` 是否相等。
fn opt_slice_eq(a: Option<&[u8]>, b: Option<&[u8]>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => a == b,
        (None, None) => true,
        _ => false,
    }
}

/// 提升新出现/变更对象的 revision 到至少 `new_pr`。
///
/// - 不在 `base_member_ids` 中的对象（新增）→ revision = max(当前, new_pr)
/// - 在 base 中且 revision 有变化（与 `base_revisions` 不同）→ revision = max(当前, new_pr)
/// - 在 base 中且 revision 无变化 → 保持当前 revision
///
/// 跨设备 revision 不能简单 max 当作全局版本语义。合并产生新内容时分配新单调
/// package revision，并确保新出现/变更对象的 revision >= new_pr，使增量快照
/// `since_revision = old_pr` 能准确发出这些对象。
fn bump_object_revisions(
    merged: &mut HashMap<String, u64>,
    base_revisions: Option<&HashMap<String, u64>>,
    base_member_ids: &HashSet<String>,
    new_pr: u64,
) {
    for (id, rev) in merged.iter_mut() {
        let in_base = base_member_ids.contains(id);
        let base_rev = base_revisions.and_then(|m| m.get(id)).copied();
        if !in_base {
            // 新增对象 → 至少 new_pr
            *rev = (*rev).max(new_pr);
        } else if let Some(br) = base_rev {
            // 在 base 中且 revision 有变化 → 至少 new_pr；无变化则保持
            if *rev != br {
                *rev = (*rev).max(new_pr);
            }
        } else {
            // 在 base 成员表但无 revision 记录 → 视为变更
            *rev = (*rev).max(new_pr);
        }
    }
}

/// 从 `Option<&GraphMeta>` 提取指定成员 ID 列表为 HashSet。
fn meta_member_set(meta: Option<&GraphMeta>, f: fn(&GraphMeta) -> &Vec<String>) -> HashSet<String> {
    meta.map(|m| f(m).iter().cloned().collect())
        .unwrap_or_default()
}

/// 对五种对象类型统一提升新出现/变更对象的 revision。
///
/// `base_meta` 用于提取 base 的成员表和 revisions，判断对象是否新增/变更。
fn bump_all_revisions(revisions: &mut MergedRevisions, base_meta: Option<&GraphMeta>, new_pr: u64) {
    let base_node_ids = meta_member_set(base_meta, |m| &m.node_ids);
    let base_edge_ids = meta_member_set(base_meta, |m| &m.edge_ids);
    let base_embed_ids = meta_member_set(base_meta, |m| &m.embed_instance_ids);
    let base_link_ids = meta_member_set(base_meta, |m| &m.link_ids);
    let base_hyperlink_ids = meta_member_set(base_meta, |m| &m.hyperlink_ids);

    bump_object_revisions(
        &mut revisions.node,
        base_meta.map(|m| &m.node_revisions),
        &base_node_ids,
        new_pr,
    );
    bump_object_revisions(
        &mut revisions.edge,
        base_meta.map(|m| &m.edge_revisions),
        &base_edge_ids,
        new_pr,
    );
    bump_object_revisions(
        &mut revisions.embed,
        base_meta.map(|m| &m.embed_revisions),
        &base_embed_ids,
        new_pr,
    );
    bump_object_revisions(
        &mut revisions.link,
        base_meta.map(|m| &m.link_revisions),
        &base_link_ids,
        new_pr,
    );
    bump_object_revisions(
        &mut revisions.hyperlink,
        base_meta.map(|m| &m.hyperlink_revisions),
        &base_hyperlink_ids,
        new_pr,
    );
}

/// `GraphMeta` 三方语义合并。
///
/// `base`/`local`/`incoming` 各为 `Option<&[u8]>`（`None` = 文件不存在）。
/// 反序列化失败返回 `Err`。
///
/// # 合并语义
///
/// - `starmap_id`：三者必须一致，不一致返回 `Err`
/// - `schema_version`：取 max（字符串比较），local/incoming 高于 `CURRENT_SCHEMA_VERSION` 返回 `Err`
/// - 五个成员 ID 列表：各自三方集合合并
/// - 五个 revisions HashMap：按对象 ID 三方合并，只保留成员表中存在的对象；
///   合并有变化时提升新出现/变更对象的 revision 到新 package_revision
/// - `deleted_since_last_sync`：按 `(object_type, object_id)` 去重取最大 revision，
///   只保留已删除对象的 tombstone
/// - 派生缓存（edge_relation_index 等）：置空，由 Store 层从对象文件重建
/// - `package_revision`：无变化时保持 `max(local, incoming)`；有变化时
///   `max(local, incoming) + 1`（新单调 revision，使增量快照能准确发出新对象）
/// - `updated_at`：取 `max(local, incoming)`
pub(crate) fn merge_graph_meta(
    base: Option<&[u8]>,
    local: Option<&[u8]>,
    incoming: Option<&[u8]>,
) -> Result<StarmapMergeResult> {
    let base_meta = parse_graph_meta(base)?;
    let local_meta = parse_graph_meta(local)?;
    let incoming_meta = parse_graph_meta(incoming)?;

    let (merged_starmap_id, merged_schema_version) = resolve_graph_meta_header(
        base_meta.as_ref(),
        local_meta.as_ref(),
        incoming_meta.as_ref(),
    )?;

    let member_ids = merge_member_ids(
        base_meta.as_ref(),
        local_meta.as_ref(),
        incoming_meta.as_ref(),
    );

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

    // 派生缓存: 取 local 版本（local 不存在取 incoming）
    let local_pr = local_meta.as_ref().map(|m| m.package_revision).unwrap_or(0);
    let incoming_pr = incoming_meta
        .as_ref()
        .map(|m| m.package_revision)
        .unwrap_or(0);
    let local_ua = local_meta.as_ref().map(|m| m.updated_at).unwrap_or(0);
    let incoming_ua = incoming_meta.as_ref().map(|m| m.updated_at).unwrap_or(0);

    // 文件双方都不存在 → 无操作（文件已删除，无需写入）
    if local.is_none() && incoming.is_none() {
        return Ok(StarmapMergeResult::NoOp);
    }

    // 判断是否完全无变化（三者 bytes 相同）。bytes 相同意味着成员表、revisions、
    // 派生缓存、package_revision 都相同，无需任何操作。
    let no_change = opt_slice_eq(local, base) && opt_slice_eq(incoming, base);

    // 合并产生新内容时分配新单调 package revision（max(local_pr, incoming_pr) + 1），
    // 使增量快照 since_revision = max(local_pr, incoming_pr) 能准确发出新对象。
    // 跨设备 revision 不能简单 max 当作全局版本语义。无变化时保持原 revision。
    let package_revision = if no_change {
        local_pr.max(incoming_pr)
    } else {
        local_pr.max(incoming_pr) + 1
    };

    // 有变化时提升新出现/变更对象的 revision 到新 package_revision
    if !no_change {
        bump_all_revisions(&mut revisions, base_meta.as_ref(), package_revision);
    }

    // 派生缓存：合并器是纯计算（不读磁盘），无法从对象文件重建派生索引。
    // 合并后的 GraphMeta 将派生缓存字段置空，由 Store 层在下次加载时从对象文件
    // 重建（meta.rs 注释已说明这些是派生缓存）。run.rs 写入时派生缓存为空可接受。
    let merged = GraphMeta {
        schema_version: merged_schema_version,
        starmap_id: merged_starmap_id,
        node_ids: member_ids.node_ids,
        edge_ids: member_ids.edge_ids,
        embed_instance_ids: member_ids.embed_ids,
        link_ids: member_ids.link_ids,
        hyperlink_ids: member_ids.hyperlink_ids,
        edge_relation_index: vec![],
        embed_host_index: vec![],
        link_relation_index: vec![],
        hyperlink_relation_index: vec![],
        node_kind_counts: HashMap::new(),
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
    Ok(finalize_merge_result(&merged_bytes, base, local, incoming))
}

// ===========================================================================
// 单元测试
// ===========================================================================

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

    fn make_graph_meta_bytes(
        starmap_id: &str,
        node_ids: &[&str],
        node_revisions: &[(&str, u64)],
        updated_at: u64,
    ) -> Vec<u8> {
        make_graph_meta_bytes_with_pr(starmap_id, node_ids, node_revisions, 0, updated_at)
    }

    fn make_graph_meta_bytes_with_pr(
        starmap_id: &str,
        node_ids: &[&str],
        node_revisions: &[(&str, u64)],
        package_revision: u64,
        updated_at: u64,
    ) -> Vec<u8> {
        let meta = GraphMeta {
            schema_version: CURRENT_SCHEMA_VERSION.to_string(),
            starmap_id: starmap_id.to_string(),
            node_ids: node_ids.iter().map(|s| s.to_string()).collect(),
            edge_ids: vec![],
            embed_instance_ids: vec![],
            link_ids: vec![],
            hyperlink_ids: vec![],
            edge_relation_index: vec![],
            embed_host_index: vec![],
            link_relation_index: vec![],
            hyperlink_relation_index: vec![],
            node_kind_counts: HashMap::new(),
            node_revisions: node_revisions
                .iter()
                .map(|(k, v)| (k.to_string(), *v))
                .collect(),
            edge_revisions: HashMap::new(),
            embed_revisions: HashMap::new(),
            link_revisions: HashMap::new(),
            hyperlink_revisions: HashMap::new(),
            package_revision,
            updated_at,
            deleted_since_last_sync: DeletedSinceLastSync::default(),
        };
        serde_json::to_vec_pretty(&meta).expect("serialize graph meta")
    }

    fn extract_starmap_ids(result: &StarmapMergeResult) -> Vec<String> {
        let bytes = match result {
            StarmapMergeResult::ApplyIncoming(b) | StarmapMergeResult::ApplyMerged(b) => b,
            _ => return vec![],
        };
        let record: StarMapIndexRecord = serde_json::from_slice(bytes).expect("deserialize index");
        record.starmap_ids
    }

    fn extract_node_ids(result: &StarmapMergeResult) -> Vec<String> {
        let bytes = match result {
            StarmapMergeResult::ApplyIncoming(b) | StarmapMergeResult::ApplyMerged(b) => b,
            _ => return vec![],
        };
        let meta: GraphMeta = serde_json::from_slice(bytes).expect("deserialize graph meta");
        meta.node_ids
    }

    fn extract_node_revisions(result: &StarmapMergeResult) -> HashMap<String, u64> {
        let bytes = match result {
            StarmapMergeResult::ApplyIncoming(b) | StarmapMergeResult::ApplyMerged(b) => b,
            _ => return HashMap::new(),
        };
        let meta: GraphMeta = serde_json::from_slice(bytes).expect("deserialize graph meta");
        meta.node_revisions
    }

    fn extract_package_revision(result: &StarmapMergeResult) -> u64 {
        let bytes = match result {
            StarmapMergeResult::ApplyIncoming(b) | StarmapMergeResult::ApplyMerged(b) => b,
            _ => return 0,
        };
        let meta: GraphMeta = serde_json::from_slice(bytes).expect("deserialize graph meta");
        meta.package_revision
    }

    fn extract_graph_meta(result: &StarmapMergeResult) -> Option<GraphMeta> {
        let bytes = match result {
            StarmapMergeResult::ApplyIncoming(b) | StarmapMergeResult::ApplyMerged(b) => b,
            _ => return None,
        };
        Some(serde_json::from_slice(bytes).expect("deserialize graph meta"))
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

    // --- 测试 4: 双端新增 node ---

    #[test]
    fn test_merge_graph_meta_both_add_node() {
        let base = make_graph_meta_bytes("sm_1", &["x"], &[], 0);
        let local = make_graph_meta_bytes("sm_1", &["x", "n1"], &[], 100);
        let incoming = make_graph_meta_bytes("sm_1", &["x", "n2"], &[], 200);

        let result = merge_graph_meta(Some(&base), Some(&local), Some(&incoming))
            .expect("merge should succeed");
        let ids = extract_node_ids(&result);
        assert_eq!(ids, vec!["x", "n1", "n2"]);
    }

    // --- 测试 5: revisions 三方合并 ---

    #[test]
    fn test_merge_graph_meta_revisions() {
        let base = make_graph_meta_bytes("sm_1", &["x"], &[("x", 1)], 0);
        let local = make_graph_meta_bytes("sm_1", &["x"], &[("x", 2)], 100);
        let incoming = make_graph_meta_bytes("sm_1", &["x"], &[("x", 3)], 200);

        let result = merge_graph_meta(Some(&base), Some(&local), Some(&incoming))
            .expect("merge should succeed");
        let revs = extract_node_revisions(&result);
        assert_eq!(revs.get("x"), Some(&3));
    }

    // --- 测试 6: starmap_id 不一致 → Err ---

    #[test]
    fn test_merge_graph_meta_starmap_id_mismatch() {
        let local = make_graph_meta_bytes("A", &["x"], &[], 100);
        let incoming = make_graph_meta_bytes("B", &["x"], &[], 200);

        let result = merge_graph_meta(None, Some(&local), Some(&incoming));
        assert!(result.is_err(), "starmap_id mismatch should return Err");
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

    // --- 测试 14: graph meta 成员删除不复活 + tombstone 保留 ---

    #[test]
    fn test_merge_graph_meta_one_delete_one_keep() {
        // base 有节点 x、y。local 删 y，incoming 保留 y 无变化。
        // 接受删除后 y 不在 node_ids。
        let base = make_graph_meta_bytes("sm_1", &["x", "y"], &[("x", 1), ("y", 1)], 0);
        let local = make_graph_meta_bytes("sm_1", &["x"], &[("x", 1)], 100); // local 删 y
        let incoming = make_graph_meta_bytes("sm_1", &["x", "y"], &[("x", 1), ("y", 1)], 200);

        let result = merge_graph_meta(Some(&base), Some(&local), Some(&incoming))
            .expect("merge should succeed");
        let ids = extract_node_ids(&result);
        // 一侧删除一侧保留 base 原样 → 接受删除，y 不复活
        assert_eq!(ids, vec!["x".to_string()]);
    }

    // --- 测试 15: 双端新增节点 → package_revision = max(local_pr, incoming_pr) + 1 ---

    #[test]
    fn test_merge_graph_meta_bump_package_revision_on_both_add() {
        // base: pr=7, nodes=[x]. local: pr=8, nodes=[x,n1]. incoming: pr=8, nodes=[x,n2].
        // 双端从 pr=7 分别新增 n1/n2。合并后 pr 应为 9（max(8,8)+1），
        // 使增量快照 since_revision=8 能发出 n1、n2。
        let base = make_graph_meta_bytes_with_pr("sm_1", &["x"], &[("x", 7)], 7, 0);
        let local =
            make_graph_meta_bytes_with_pr("sm_1", &["x", "n1"], &[("x", 7), ("n1", 8)], 8, 100);
        let incoming =
            make_graph_meta_bytes_with_pr("sm_1", &["x", "n2"], &[("x", 7), ("n2", 8)], 8, 200);

        let result = merge_graph_meta(Some(&base), Some(&local), Some(&incoming))
            .expect("merge should succeed");
        let pr = extract_package_revision(&result);
        // max(8, 8) + 1 = 9
        assert_eq!(
            pr, 9,
            "package_revision should be max(local_pr, incoming_pr) + 1"
        );
    }

    // --- 测试 16: 新增节点的 revision 应等于新 package_revision ---

    #[test]
    fn test_merge_graph_meta_new_object_revision_bumped() {
        let base = make_graph_meta_bytes_with_pr("sm_1", &["x"], &[("x", 7)], 7, 0);
        let local =
            make_graph_meta_bytes_with_pr("sm_1", &["x", "n1"], &[("x", 7), ("n1", 8)], 8, 100);
        let incoming =
            make_graph_meta_bytes_with_pr("sm_1", &["x", "n2"], &[("x", 7), ("n2", 8)], 8, 200);

        let result = merge_graph_meta(Some(&base), Some(&local), Some(&incoming))
            .expect("merge should succeed");
        let revs = extract_node_revisions(&result);
        let pr = extract_package_revision(&result);
        // n1、n2 是新增对象，revision 应提升到新 package_revision（9）
        assert_eq!(
            revs.get("n1"),
            Some(&pr),
            "n1 revision should be bumped to new pr"
        );
        assert_eq!(
            revs.get("n2"),
            Some(&pr),
            "n2 revision should be bumped to new pr"
        );
        // x 在 base 中且 revision 无变化（7→7），保持原 revision
        assert_eq!(
            revs.get("x"),
            Some(&7),
            "x revision unchanged should stay at 7"
        );
    }

    // --- 测试 17: 派生缓存字段应为空 ---

    #[test]
    fn test_merge_graph_meta_derived_cache_cleared() {
        let base = make_graph_meta_bytes("sm_1", &["x"], &[], 0);
        let local = make_graph_meta_bytes("sm_1", &["x", "n1"], &[], 100);
        let incoming = make_graph_meta_bytes("sm_1", &["x", "n2"], &[], 200);

        let result = merge_graph_meta(Some(&base), Some(&local), Some(&incoming))
            .expect("merge should succeed");
        let meta = extract_graph_meta(&result).expect("should have merged meta");
        // 派生缓存字段应为空，由 Store 层重建
        assert!(
            meta.edge_relation_index.is_empty(),
            "edge_relation_index should be empty"
        );
        assert!(
            meta.embed_host_index.is_empty(),
            "embed_host_index should be empty"
        );
        assert!(
            meta.link_relation_index.is_empty(),
            "link_relation_index should be empty"
        );
        assert!(
            meta.hyperlink_relation_index.is_empty(),
            "hyperlink_relation_index should be empty"
        );
        assert!(
            meta.node_kind_counts.is_empty(),
            "node_kind_counts should be empty"
        );
    }

    // --- 测试 18: 无变化时 package_revision 不变 ---

    #[test]
    fn test_merge_graph_meta_no_change_preserves_revision() {
        // 三者完全相同 → NoOp，package_revision 不变
        let base = make_graph_meta_bytes_with_pr("sm_1", &["x"], &[("x", 5)], 5, 100);
        let local = base.clone();
        let incoming = base.clone();

        let result = merge_graph_meta(Some(&base), Some(&local), Some(&incoming))
            .expect("merge should succeed");
        // 无变化 → NoOp
        assert!(
            matches!(result, StarmapMergeResult::NoOp),
            "no change should return NoOp, got {:?}",
            result
        );
    }

    // --- 测试 19: 单端修改 → package_revision bump ---

    #[test]
    fn test_merge_graph_meta_single_side_change_bumps_pr() {
        // local 新增节点，incoming 无变化 → 有变化，pr bump
        let base = make_graph_meta_bytes_with_pr("sm_1", &["x"], &[("x", 5)], 5, 0);
        let local =
            make_graph_meta_bytes_with_pr("sm_1", &["x", "n1"], &[("x", 5), ("n1", 6)], 6, 100);
        let incoming = make_graph_meta_bytes_with_pr("sm_1", &["x"], &[("x", 5)], 5, 0);

        let result = merge_graph_meta(Some(&base), Some(&local), Some(&incoming))
            .expect("merge should succeed");
        let pr = extract_package_revision(&result);
        // 有变化 → max(6, 5) + 1 = 7
        assert_eq!(pr, 7, "single side change should bump pr");
        let revs = extract_node_revisions(&result);
        // n1 是新增对象 → revision 提升到 7
        assert_eq!(
            revs.get("n1"),
            Some(&7),
            "n1 revision should be bumped to new pr"
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
        let d = merge_starmap_object(Some(b"base"), Some(b"local"), None);
        assert!(matches!(d, ObjectMergeDecision::KeepLocal), "got {:?}", d);
    }

    #[test]
    fn test_merge_object_local_delete_remote_keep() {
        let d = merge_starmap_object(Some(b"base"), None, Some(b"incoming"));
        match d {
            ObjectMergeDecision::ApplyIncoming(c) => assert_eq!(c, b"incoming"),
            other => panic!("expected ApplyIncoming, got {:?}", other),
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
}
