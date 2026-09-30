//! Issue #805 评论 5907045450 — 删除星图后 LWW SyncState.tombstones 缺失导致
//! `snapshot_local_records_read_only` 拒绝伪造 delete 并打成 recoverable_error。
//!
//! ## 修复验证（原复现测试已转为回归守卫）
//!
//! 本测试原断言 **bug 存在**（tombstone 缺失 + 同步 Err）。修复后转为断言
//! **bug 已修复**（tombstone 补齐 + 同步 Ok），作为回归守卫。
//!
//! 修复方案（评论 5907045450 第 6/7/9 部分）：
//! - 新建 `core/writer_core/src/storage/journal/starmap_delete.rs`，把 StarMap
//!   删除做成 durable transaction（plan/apply）。
//! - 提取共用 `ensure_sync_tombstones_from_facts` 到 `workspace_change.rs`。
//! - `delete_starmap` / `delete_starmap_with_changes` 改走 plan/apply，在物理
//!   删除后写 `SyncState.tombstones`。
//!
//! 验证路径：
//! 1. 创建一个星图（含 node 等对象文件）
//! 2. 模拟同步完成：把星图文件路径加入 `SyncState.known_files`
//! 3. 删除该星图（走 `delete_starmap` / `delete_starmap_with_changes`）
//! 4. 断言 `SyncState.tombstones` 包含对应星图对象文件的记录（修复证据）
//! 5. 触发同步（`build_sync_plan` → `snapshot_local_records_read_only`），
//!    断言返回 Ok（不再因 "known file missing + no tombstone" 返回 Err）

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};

use tempfile::TempDir;
use writer_core::starmap;
use writer_core::starmap::semantic::{StarMapNodeContent, StarMapProvenance};
use writer_core::starmap::store::StarMapStore;
use writer_core::starmap::types::{StarMapNode, StarMapNodeKind};
use writer_core::sync::{SyncScope, SyncService, SyncState};

// ---------------------------------------------------------------------------
// 辅助
// ---------------------------------------------------------------------------

/// 构造一个最小可用的 StarMapNode（与 starmap/store/tests/mod.rs::make_test_node 同构）。
fn make_node(id: &str, title: &str) -> StarMapNode {
    StarMapNode {
        id: id.to_string(),
        title: title.to_string(),
        kind: StarMapNodeKind::Concept,
        payload: None,
        tags: vec![],
        content: StarMapNodeContent::Empty,
        anchors: vec![],
        portal: None,
        position: Default::default(),
        style: Default::default(),
        provenance: StarMapProvenance::default(),
        created_at: 0,
        updated_at: 0,
    }
}

/// 递归收集 `dir` 下所有文件相对于 `base` 的 POSIX 相对路径。
fn walk_dir(dir: &Path, base: &Path, out: &mut Vec<String>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            walk_dir(&path, base, out);
        } else {
            let rel = path.strip_prefix(base).unwrap();
            // 统一用 '/' 作为分隔符，与 sync scanner 的 relative_path 一致。
            let rel_str: PathBuf = rel
                .components()
                .map(|c| match c {
                    std::path::Component::Normal(s) => s.to_os_string(),
                    other => other.as_os_str().to_os_string(),
                })
                .collect();
            out.push(rel_str.to_string_lossy().to_string());
        }
    }
}

/// 收集星图相关的所有同步可见文件相对于 `app_data_root` 的路径：
/// - `starmaps/{id}.meta.json`
/// - `starmaps/{id}/` 目录下所有对象文件（nodes/edges/embeds/...）
fn collect_starmap_sync_files(app_data_root: &Path, starmap_id: &str) -> Vec<String> {
    let mut files = Vec::new();
    let meta_rel = format!("starmaps/{}.meta.json", starmap_id);
    files.push(meta_rel);
    let pkg_dir = app_data_root.join("starmaps").join(starmap_id);
    if pkg_dir.exists() {
        walk_dir(&pkg_dir, app_data_root, &mut files);
    }
    files
}

// ---------------------------------------------------------------------------
// 验证 Issue #805 评论 5907045450 修复
// ---------------------------------------------------------------------------

/// 删除星图后 LWW SyncState.tombstones 已补齐，`snapshot_local_records_read_only`
/// 不再拒绝，`build_sync_plan` 返回 Ok。
///
/// 步骤：
/// 1. 创建星图 + node 对象文件；
/// 2. 模拟同步完成：把星图文件路径加入 `SyncState.known_files` 并保存；
/// 3. `delete_starmap` 删除星图（文件移到 trash，tombstone 补齐）；
/// 4. 断言 `SyncState.tombstones` 包含所有星图文件记录（修复证据）；
/// 5. `SyncService::build_sync_plan(App scope)` → Ok。
#[test]
fn verify_issue_805_delete_starmap_tombstone_present_sync_ok() {
    let tmp = TempDir::new().unwrap();
    let app_data_root = tmp.path().to_path_buf();
    std::fs::create_dir_all(app_data_root.join("projects")).unwrap();

    // ── 1. 创建星图（含 node 对象文件） ──
    let meta =
        starmap::create_starmap(&app_data_root, "测试星图", "验证 #805 修复", None).unwrap();
    let starmap_id = meta.starmap_id.clone();

    let mut store = StarMapStore::new(&app_data_root, &starmap_id);
    store.upsert_node(make_node("n1", "节点1"));
    store.flush().unwrap();

    // 收集星图相关文件相对路径（meta + 对象文件）。
    let starmap_files = collect_starmap_sync_files(&app_data_root, &starmap_id);
    assert!(
        !starmap_files.is_empty(),
        "前置：应至少有 meta 文件"
    );
    // 确认对象文件确实落盘（含 node）。
    let has_node_file = starmap_files.iter().any(|p| p.contains("/nodes/"));
    assert!(
        has_node_file,
        "前置：应存在 node 对象文件，实际文件: {:?}",
        starmap_files
    );

    // ── 2. 模拟同步完成：把星图文件路径加入 SyncState.known_files ──
    //    known_files 记录上次同步后的共识哈希；这是同步成功的正常基线状态。
    let mut state = SyncState::default();
    state.device_id = "test-device-805".to_string();
    for rel in &starmap_files {
        state
            .known_files
            .insert(rel.clone(), "dummyhash000000000000000000000000".to_string());
        state.known_files_updated_at.insert(rel.clone(), 1000);
    }
    SyncService::save_sync_state(&app_data_root, &state).unwrap();

    // ── 3. 删除星图（走 delete_starmap，现在走 plan/apply 事务） ──
    starmap::delete_starmap(&app_data_root, &starmap_id).unwrap();

    // 确认星图文件已从磁盘删除（移到 trash，原路径不存在）。
    for rel in &starmap_files {
        let full = app_data_root.join(rel);
        assert!(
            !full.exists(),
            "前置：删除星图后 {} 应不存在（已移到 trash）",
            rel
        );
    }

    // ── 4. 断言 SyncState.tombstones 包含所有星图文件记录（修复证据） ──
    //    修复前这里缺失（bug 根因）。修复后 plan/apply 事务补齐 tombstone。
    let state_after_delete = SyncService::load_sync_state(&app_data_root).unwrap();
    let missing_tombstones: Vec<_> = starmap_files
        .iter()
        .filter(|rel| {
            !state_after_delete
                .tombstones
                .iter()
                .any(|t| t.original_path == **rel)
        })
        .collect();
    assert!(
        missing_tombstones.is_empty(),
        "验证 #805 修复：删除星图后 SyncState.tombstones 应包含所有星图文件记录，\
         但仍缺失: {:?}\n\
         starmap_files: {:?}\n\
         tombstones: {:?}",
        missing_tombstones,
        starmap_files,
        state_after_delete.tombstones
    );

    // ── 5. 触发同步（build_sync_plan → snapshot_local_records_read_only） ──
    //    星图在 App scope 下同步（is_app_whitelisted_path 包含 starmaps/）。
    //    修复前：因 "known file missing + no SyncState tombstone" 返回 Err。
    //    修复后：tombstone 已补齐，返回 Ok。
    let plan_result = SyncService::build_sync_plan(&app_data_root, SyncScope::App);

    assert!(
        plan_result.is_ok(),
        "验证 #805 修复：build_sync_plan 应返回 Ok（tombstone 已补齐），\
         实际 Err: {:?}",
        plan_result.err()
    );

    // 保留 tmp 防止提前清理。
    drop(tmp);
}

/// 单独验证：`delete_starmap_with_changes` 路径同样补齐 tombstone 且同步 Ok
/// （issue 描述提到 delete_starmap / delete_starmap_with_changes 两条路径都受影响）。
#[test]
fn verify_issue_805_delete_starmap_with_changes_tombstone_present_sync_ok() {
    let tmp = TempDir::new().unwrap();
    let app_data_root = tmp.path().to_path_buf();
    std::fs::create_dir_all(app_data_root.join("projects")).unwrap();

    let meta =
        starmap::create_starmap(&app_data_root, "测试星图2", "验证 #805 with_changes", None)
            .unwrap();
    let starmap_id = meta.starmap_id.clone();

    let mut store = StarMapStore::new(&app_data_root, &starmap_id);
    store.upsert_node(make_node("n2", "节点2"));
    store.flush().unwrap();

    let starmap_files = collect_starmap_sync_files(&app_data_root, &starmap_id);
    assert!(!starmap_files.is_empty());

    // 模拟同步基线。
    let mut state = SyncState::default();
    state.device_id = "test-device-805b".to_string();
    for rel in &starmap_files {
        state
            .known_files
            .insert(rel.clone(), "dummyhash000000000000000000000000".to_string());
        state.known_files_updated_at.insert(rel.clone(), 1000);
    }
    SyncService::save_sync_state(&app_data_root, &state).unwrap();

    // 走 delete_starmap_with_changes 路径（现在走 plan/apply 事务）。
    let change_set =
        starmap::delete_starmap_with_changes(&app_data_root, &starmap_id).unwrap();
    // 变更集应包含 meta 删除 + 对象目录删除树。
    assert!(
        !change_set.is_empty(),
        "前置：delete_starmap_with_changes 应返回非空变更集"
    );

    // tombstone 应全部补齐。
    let state_after = SyncService::load_sync_state(&app_data_root).unwrap();
    let missing: Vec<_> = starmap_files
        .iter()
        .filter(|rel| {
            !state_after
                .tombstones
                .iter()
                .any(|t| t.original_path == **rel)
        })
        .collect();
    assert!(
        missing.is_empty(),
        "验证 #805 修复（with_changes）：tombstone 应全部补齐，仍缺失: {:?}\n\
         tombstones: {:?}",
        missing,
        state_after.tombstones
    );

    // 同步应成功。
    let plan_result = SyncService::build_sync_plan(&app_data_root, SyncScope::App);
    assert!(
        plan_result.is_ok(),
        "验证 #805 修复（with_changes）：build_sync_plan 应返回 Ok，\
         实际 Err: {:?}",
        plan_result.err()
    );

    drop(tmp);
}
