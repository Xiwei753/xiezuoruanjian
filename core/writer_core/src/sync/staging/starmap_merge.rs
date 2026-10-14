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

// ===========================================================================
// 三方集合合并辅助函数
// ===========================================================================

/// 三方集合合并：保留互不冲突的新增，依据删除事实处理删除。
///
/// `base`/`local`/`incoming` 各为 `Option<&Vec<String>>`（`None` = 不存在 = 空集）。
/// 返回合并后的 `Vec<String>`，保持插入顺序，去重。
///
/// # 逻辑
///
/// 1. 新增 = `(local - base) ∪ (incoming - base)`
/// 2. 保留 = `base` 中在 `local` 或 `incoming` 中仍存在的
/// 3. 结果 = 保留 ∪ 新增
/// 4. 输出顺序：先按 `base` 原始顺序输出保留的，再按 `local` 顺序输出 local 新增的，
///    再按 `incoming` 顺序输出 incoming 新增的（去重）
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

    // 1. 按 base 顺序输出保留的（base 中在 local 或 incoming 中仍存在的）
    for id in base_vec {
        if (local_set.contains(id) || incoming_set.contains(id)) && seen.insert(id.clone()) {
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
/// 合并后只保留成员表中已删除对象的 tombstone。
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

    // 只保留成员表中已删除对象的 tombstone
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

/// `StarMapIndexRecord` 三方语义合并。
///
/// `base`/`local`/`incoming` 各为 `Option<&[u8]>`（`None` = 文件不存在 = 空集合）。
/// 反序列化失败返回 `Err`（不静默吞错）。
///
/// # 合并语义
///
/// - `starmap_ids`：三方集合合并
/// - `root_starmap_ids`：三方集合合并
/// - `main_starmap_by_project`：按 project_id key 做三方变更判断
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
    let merged_root_starmap_ids = three_way_merge_id_set(
        Some(&base_rec.root_starmap_ids),
        Some(&local_rec.root_starmap_ids),
        Some(&incoming_rec.root_starmap_ids),
    );
    let merged_main = merge_main_starmap_by_project(
        &base_rec.main_starmap_by_project,
        &local_rec.main_starmap_by_project,
        &incoming_rec.main_starmap_by_project,
        local_rec.updated_at,
        incoming_rec.updated_at,
    );

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
/// - 五个 revisions HashMap：按对象 ID 三方合并，只保留成员表中存在的对象
/// - `deleted_since_last_sync`：按 `(object_type, object_id)` 去重取最大 revision，只保留已删除对象
/// - 派生缓存：取 local 版本（local 不存在取 incoming）
/// - `package_revision`/`updated_at`：取 `max(local, incoming)`
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

    let revisions = merge_all_revisions(
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
    let derived = local_meta.as_ref().or(incoming_meta.as_ref());
    let local_pr = local_meta.as_ref().map(|m| m.package_revision).unwrap_or(0);
    let incoming_pr = incoming_meta
        .as_ref()
        .map(|m| m.package_revision)
        .unwrap_or(0);
    let local_ua = local_meta.as_ref().map(|m| m.updated_at).unwrap_or(0);
    let incoming_ua = incoming_meta.as_ref().map(|m| m.updated_at).unwrap_or(0);

    let merged = GraphMeta {
        schema_version: merged_schema_version,
        starmap_id: merged_starmap_id,
        node_ids: member_ids.node_ids,
        edge_ids: member_ids.edge_ids,
        embed_instance_ids: member_ids.embed_ids,
        link_ids: member_ids.link_ids,
        hyperlink_ids: member_ids.hyperlink_ids,
        edge_relation_index: derived
            .map(|m| m.edge_relation_index.clone())
            .unwrap_or_default(),
        embed_host_index: derived
            .map(|m| m.embed_host_index.clone())
            .unwrap_or_default(),
        link_relation_index: derived
            .map(|m| m.link_relation_index.clone())
            .unwrap_or_default(),
        hyperlink_relation_index: derived
            .map(|m| m.hyperlink_relation_index.clone())
            .unwrap_or_default(),
        node_kind_counts: derived
            .map(|m| m.node_kind_counts.clone())
            .unwrap_or_default(),
        node_revisions: revisions.node,
        edge_revisions: revisions.edge,
        embed_revisions: revisions.embed,
        link_revisions: revisions.link,
        hyperlink_revisions: revisions.hyperlink,
        package_revision: local_pr.max(incoming_pr),
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
            package_revision: 0,
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

    // --- 测试 3: 一端删除一端保留 → 保留（删除未达成共识）---

    #[test]
    fn test_merge_index_one_delete_one_keep() {
        let base = make_index_bytes(&["A", "B"], &[], &[], 0);
        let local = make_index_bytes(&["A"], &[], &[], 100); // local 删 B
        let incoming = make_index_bytes(&["A", "B"], &[], &[], 200); // incoming 保留 B

        let result = merge_starmap_index(Some(&base), Some(&local), Some(&incoming))
            .expect("merge should succeed");
        let ids = extract_starmap_ids(&result);
        assert_eq!(ids, vec!["A", "B"]);
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
        let base = make_index_bytes(&[], &[], &[("p", "A")], 0);
        let local = make_index_bytes(&[], &[], &[("p", "B")], 100);
        let incoming = make_index_bytes(&[], &[], &[("p", "C")], 200);

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
}
