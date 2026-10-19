use super::*;
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

#[test]
fn staging_run_create_and_cleanup() {
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    let run = StagingRun::create(tmp.path(), live).unwrap();
    assert!(run.base_root().exists());
    assert!(run.staging_root().exists());
    run.cleanup();
    assert!(!run.run_root().exists());
}

#[test]
fn base_snapshot_hardlink_or_copy() {
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    fs::create_dir_all(&live).unwrap();
    fs::write(live.join("a.txt"), "hello").unwrap();
    fs::create_dir_all(live.join("sub")).unwrap();
    fs::write(live.join("sub/b.txt"), "world").unwrap();

    let run = StagingRun::create(tmp.path(), live.clone()).unwrap();
    run.build_base_snapshot_from_live(
        &live,
        &[
            PathBuf::from("a.txt"),
            PathBuf::from("sub/b.txt"),
            PathBuf::from("missing.txt"),
        ],
    )
    .unwrap();

    assert_eq!(
        fs::read_to_string(run.base_root().join("a.txt")).unwrap(),
        "hello"
    );
    assert_eq!(
        fs::read_to_string(run.base_root().join("sub/b.txt")).unwrap(),
        "world"
    );
    assert!(!run.base_root().join("missing.txt").exists());
}

#[test]
fn commit_plan_local_eq_base_applies_incoming() {
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    fs::create_dir_all(&live).unwrap();
    fs::write(live.join("f.txt"), "base").unwrap();

    let run = StagingRun::create(tmp.path(), live.clone()).unwrap();
    run.build_base_snapshot_from_live(&live, &[PathBuf::from("f.txt")])
        .unwrap();
    // local 仍是 base（没动），incoming 改了。
    fs::write(run.staging_root().join("f.txt"), "incoming").unwrap();

    let plan = run.compute_commit_plan(&live).unwrap();
    assert_eq!(plan.content_actions.len(), 1);
    assert!(plan.keep_local.is_empty());
    assert!(plan.conflict.is_empty());
}

#[test]
fn commit_plan_incoming_eq_base_keeps_local() {
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    fs::create_dir_all(&live).unwrap();
    fs::write(live.join("f.txt"), "base").unwrap();

    let run = StagingRun::create(tmp.path(), live.clone()).unwrap();
    run.build_base_snapshot_from_live(&live, &[PathBuf::from("f.txt")])
        .unwrap();
    // local 改了（走 atomic_write rename 替换，hard-link 的 base 保留旧 inode），
    // incoming == base。
    crate::storage::atomic_write_string(&live.join("f.txt"), "local-changed").unwrap();
    fs::write(run.staging_root().join("f.txt"), "base").unwrap();

    let plan = run.compute_commit_plan(&live).unwrap();
    assert_eq!(plan.keep_local.len(), 1);
    assert!(plan.content_actions.is_empty());
    assert!(plan.conflict.is_empty());
}

#[test]
fn seed_from_live_copies_all_files_to_base_and_staging() {
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    fs::create_dir_all(live.join("sub")).unwrap();
    fs::write(live.join("a.txt"), "hello").unwrap();
    fs::write(live.join("sub/b.txt"), "world").unwrap();
    // 内部目录应被跳过
    fs::create_dir_all(live.join(".git/objects")).unwrap();
    fs::write(live.join(".git/HEAD"), "ref: refs/heads/main").unwrap();
    fs::create_dir_all(live.join("app-meta/transactions/tx1")).unwrap();
    fs::write(live.join("app-meta/transactions/tx1/staged"), "tmp").unwrap();

    let run = StagingRun::create(tmp.path(), live.clone()).unwrap();
    run.seed_from_live(&live).unwrap();

    // base 有业务文件
    assert_eq!(
        fs::read_to_string(run.base_root().join("a.txt")).unwrap(),
        "hello"
    );
    assert_eq!(
        fs::read_to_string(run.base_root().join("sub/b.txt")).unwrap(),
        "world"
    );
    // staging 也有业务文件
    assert_eq!(
        fs::read_to_string(run.staging_root().join("a.txt")).unwrap(),
        "hello"
    );
    assert_eq!(
        fs::read_to_string(run.staging_root().join("sub/b.txt")).unwrap(),
        "world"
    );
    // 内部目录被跳过
    assert!(!run.base_root().join(".git").exists());
    assert!(!run.staging_root().join(".git").exists());
    assert!(!run.base_root().join("app-meta/transactions").exists());
    assert!(!run.staging_root().join("app-meta/transactions").exists());
}

#[test]
fn commit_plan_both_changed_is_conflict() {
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    fs::create_dir_all(&live).unwrap();
    // UserTextDocument 双方都改才冲突。
    // 用 note.md（正文类）而非 .txt（GeneratedCache 走 LWW 不冲突）。
    fs::write(live.join("note.md"), "base").unwrap();

    let run = StagingRun::create(tmp.path(), live.clone()).unwrap();
    run.build_base_snapshot_from_live(&live, &[PathBuf::from("note.md")])
        .unwrap();
    // local 改了（atomic_write rename 替换，base 保留旧 inode）。
    crate::storage::atomic_write_string(&live.join("note.md"), "local-changed").unwrap();
    fs::write(run.staging_root().join("note.md"), "incoming-changed").unwrap();

    let plan = run.compute_commit_plan(&live).unwrap();
    assert_eq!(plan.conflict.len(), 1);
    assert!(plan.content_actions.is_empty());
    assert!(plan.keep_local.is_empty());
}

#[test]
fn commit_plan_remote_delete_local_unchanged_produces_delete() {
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    fs::create_dir_all(&live).unwrap();
    fs::write(live.join("f.txt"), "base").unwrap();

    let run = StagingRun::create(tmp.path(), live.clone()).unwrap();
    run.build_base_snapshot_from_live(&live, &[PathBuf::from("f.txt")])
        .unwrap();
    // 远端删除：staging 中没有 f.txt（incoming=None），local 没改。
    // base ∪ staging = {f.txt}（来自 base），三方比较：local==base, incoming=None → Delete。

    let plan = run.compute_commit_plan(&live).unwrap();
    assert_eq!(plan.content_actions.len(), 1);
    assert!(matches!(
        plan.content_actions[0],
        CommitAction::Delete { .. }
    ));
    assert!(plan.conflict.is_empty());
    assert!(plan.keep_local.is_empty());
}

#[test]
fn commit_plan_remote_new_local_none_applies() {
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    fs::create_dir_all(&live).unwrap();
    // live 没有 f.txt（local=None），base 也没有（base=None）。
    // staging 有 f.txt（incoming=Some）。
    let run = StagingRun::create(tmp.path(), live.clone()).unwrap();
    // base 为空（没有 build_base_snapshot）
    fs::write(run.staging_root().join("f.txt"), "new-from-remote").unwrap();

    let plan = run.compute_commit_plan(&live).unwrap();
    // base=None, local=None, incoming=Some → local==base (both None), incoming!=base → Apply
    assert_eq!(plan.content_actions.len(), 1);
    assert!(plan.conflict.is_empty());
}

#[test]
fn commit_plan_local_changed_remote_deleted_is_conflict() {
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    fs::create_dir_all(&live).unwrap();
    // UserTextDocument local 改了 远端删除 → 冲突。
    // 用 note.md（正文类）而非 .txt（GeneratedCache 走 LWW：Apply Delete 不冲突）。
    fs::write(live.join("note.md"), "base").unwrap();

    let run = StagingRun::create(tmp.path(), live.clone()).unwrap();
    run.build_base_snapshot_from_live(&live, &[PathBuf::from("note.md")])
        .unwrap();
    // local 改了，远端删除（staging 没有 note.md）。
    crate::storage::atomic_write_string(&live.join("note.md"), "local-changed").unwrap();

    let plan = run.compute_commit_plan(&live).unwrap();
    // local!=base, incoming=None → local!=incoming → Conflict
    assert_eq!(plan.conflict.len(), 1);
    assert!(plan.content_actions.is_empty());
    assert!(plan.keep_local.is_empty());
}

#[test]
fn walk_commit_candidates_skips_git_sujian_migrate_source() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    // 创建测试目录结构
    fs::create_dir_all(root.join("normal")).unwrap();
    fs::write(root.join("normal/file.txt"), "normal").unwrap();
    // .git.sujian-migrate-source-* 目录应被跳过
    fs::create_dir_all(root.join(".git.sujian-migrate-source-abc/objects")).unwrap();
    fs::write(
        root.join(".git.sujian-migrate-source-abc/HEAD"),
        "ref: main",
    )
    .unwrap();
    fs::write(root.join(".git.sujian-migrate-source-abc/config"), "[core]").unwrap();
    // .git.sujian-tmp-* 目录也应被跳过
    fs::create_dir_all(root.join(".git.sujian-tmp-tmp123")).unwrap();
    fs::write(root.join(".git.sujian-tmp-tmp123/tmp"), "tmp").unwrap();
    // .git 目录也应被跳过
    fs::create_dir_all(root.join(".git/objects")).unwrap();
    fs::write(root.join(".git/HEAD"), "ref: main").unwrap();

    let mut out = Vec::new();
    walk_commit_candidates(root, root, &mut out).unwrap();

    // 只应看到 normal/file.txt
    assert_eq!(out.len(), 1);
    assert!(out.contains(&PathBuf::from("normal/file.txt")));
    // 不应包含任何内部 Git 工件
    assert!(!out.iter().any(|p| p.to_string_lossy().contains(".git")));
}

#[test]
fn classify_staging_commit_path_skips_git_sujian_migrate_source() {
    // .git.sujian-migrate-source-* 应被分类为 Skip
    assert_eq!(
        classify_staging_commit_path(".git.sujian-migrate-source-abc"),
        StagingCommitClass::Skip
    );
    assert_eq!(
        classify_staging_commit_path(".git.sujian-migrate-source-abc/objects/HEAD"),
        StagingCommitClass::Skip
    );
    // Windows 路径格式也应正确处理
    assert_eq!(
        classify_staging_commit_path(".git.sujian-migrate-source-abc\\objects\\HEAD"),
        StagingCommitClass::Skip
    );

    // .git.sujian-tmp-* 也应被分类为 Skip
    assert_eq!(
        classify_staging_commit_path(".git.sujian-tmp-tmp123"),
        StagingCommitClass::Skip
    );
    assert_eq!(
        classify_staging_commit_path(".git.sujian-tmp-tmp123/tmp"),
        StagingCommitClass::Skip
    );

    // .git 也应被分类为 Skip
    assert_eq!(
        classify_staging_commit_path(".git"),
        StagingCommitClass::Skip
    );
    assert_eq!(
        classify_staging_commit_path(".git/objects/HEAD"),
        StagingCommitClass::Skip
    );

    // 普通内容应被分类为 Content
    assert_eq!(
        classify_staging_commit_path("normal/file.txt"),
        StagingCommitClass::Content
    );
}

/// Issue #757：staging "远端删除 + 本地修改" 必须记成 RemoteDeleted，
/// 不再被 three_way_resolve 的 BothChanged 笼统覆盖。
///
/// 真实 staging 流程：base 有 note.md=A，live 改成 B，staging 删除 note.md
/// （incoming=None）。compute_commit_plan → conflict[0].kind == RemoteDeleted。
/// record_staging_conflicts 持久化后 load_conflict_preview 必须 remote_deleted=true。
/// resolve_conflict_take_remote 必须立即把 live 移入 trash、返回 applied_live=true，
/// 且 pending_take_remote 不含此 path（不排队下载）。
#[test]
fn staging_remote_deleted_local_modified_records_remote_deleted_kind() {
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    fs::create_dir_all(&live).unwrap();
    // 用 note.md（正文类路径，走 three_way_resolve）。
    let note_rel = "note.md";
    let note_abs = live.join(note_rel);
    // base 有正文 A。
    fs::write(&note_abs, "base-A").unwrap();

    let run = StagingRun::create(tmp.path(), live.clone()).unwrap();
    run.build_base_snapshot_from_live(&live, &[PathBuf::from(note_rel)])
        .unwrap();
    // live 改成 B（atomic_write rename 替换，base 保留旧 inode）。
    crate::storage::atomic_write_string(&note_abs, "local-B").unwrap();
    // staging 不放 note.md（模拟远端删除，incoming=None）。

    // 1. compute_commit_plan → conflict[0].kind == RemoteDeleted。
    let plan = run.compute_commit_plan(&live).unwrap();
    assert_eq!(
        plan.conflict.len(),
        1,
        "remote-deleted + local-modified should produce exactly one conflict"
    );
    assert_eq!(
        plan.conflict[0].kind,
        crate::sync::types::SyncConflictKind::RemoteDeleted,
        "conflict kind must be RemoteDeleted, not BothChanged"
    );
    assert_eq!(plan.conflict[0].rel_path, PathBuf::from(note_rel));
    // RemoteDeleted 不保存快照。
    assert!(plan.conflict[0].remote_snapshot_path.is_none());

    // cleanup staging（模拟 commit_helpers 的 run.cleanup()）。
    run.cleanup();

    // 2. record_staging_conflicts 持久化（sync_root = live 目录）。
    let merged = crate::sync::conflict::record_staging_conflicts(
        &live,
        "projects/test",
        &plan.conflict,
        &[],
    )
    .unwrap();
    assert_eq!(merged.len(), 1);
    assert_eq!(
        merged[0].kind,
        crate::sync::types::SyncConflictKind::RemoteDeleted
    );
    assert!(merged[0].remote_snapshot_path.is_none());

    // 3. load_conflict_preview 必须 remote_deleted=true。
    let preview = crate::sync::SyncService::load_conflict_preview(&live, note_rel).unwrap();
    assert!(
        preview.remote_deleted,
        "preview.remote_deleted must be true for RemoteDeleted conflict"
    );
    assert_eq!(
        preview.kind,
        crate::sync::types::SyncConflictKind::RemoteDeleted
    );
    assert!(preview.remote_content.is_none());
    assert_eq!(preview.local_content, "local-B");

    // 4. resolve_conflict_take_remote 必须立即把 live 移入 trash、返回 applied_live=true。
    let applied_live =
        crate::sync::SyncService::resolve_conflict_take_remote(&live, note_rel).unwrap();
    assert!(
        applied_live,
        "take_remote for RemoteDeleted must return applied_live=true (move to trash immediately)"
    );
    // live 正文已被移入 trash，note.md 不再存在于 live。
    assert!(
        !note_abs.exists(),
        "live note.md must be moved to trash after take_remote"
    );

    // 5. SyncState.pending_take_remote 不含此 path。
    let state_after = crate::sync::SyncService::load_sync_state(&live).unwrap();
    assert!(
        !state_after.pending_take_remote.contains(note_rel),
        "pending_take_remote must NOT contain the path for RemoteDeleted take_remote"
    );
    assert!(!state_after.conflicted_files.contains(note_rel));
    assert!(state_after.conflicts.is_empty());
}

// ── 星图语义合并集成测试（issue #902 第八轮复核）──

/// 构造 StarMapIndexRecord 的 JSON bytes。
fn make_index_json(starmap_ids: &[&str], updated_at: u64) -> Vec<u8> {
    let record = serde_json::json!({
        "schemaVersion": 1,
        "starmapIds": starmap_ids,
        "rootStarmapIds": [],
        "mainStarmapByProject": {},
        "updatedAt": updated_at,
    });
    serde_json::to_vec_pretty(&record).expect("serialize index")
}

/// 构造 GraphMeta 的 JSON bytes（只填必要字段）。
fn make_graph_meta_json(starmap_id: &str, node_ids: &[&str], updated_at: u64) -> Vec<u8> {
    let record = serde_json::json!({
        "schemaVersion": "4",
        "starmapId": starmap_id,
        "nodeIds": node_ids,
        "edgeIds": [],
        "embedInstanceIds": [],
        "linkIds": [],
        "hyperlinkIds": [],
        "packageRevision": 0,
        "updatedAt": updated_at,
    });
    serde_json::to_vec_pretty(&record).expect("serialize graph meta")
}

/// 构造 StarMapNode 的 JSON bytes（只填反序列化必填字段）。
///
/// `rebuild_derived_indexes` 会把节点对象反序列化为 `StarMapNode`，
/// 需要 id/title/kind/position/createdAt/updatedAt 字段。
fn make_node_json(node_id: &str) -> Vec<u8> {
    let record = serde_json::json!({
        "id": node_id,
        "title": node_id,
        "kind": "note",
        "position": { "x": 0.0, "y": 0.0 },
        "createdAt": 0,
        "updatedAt": 0,
    });
    serde_json::to_vec_pretty(&record).expect("serialize node")
}

/// issue #902 第八轮复核：starmaps/index.json 走语义合并而非整文件 LWW。
/// 双端并发创建不同星图时，合并后应保留双方新增的星图 ID。
#[test]
fn starmap_index_semantic_merge_both_add() {
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    let starmaps_dir = live.join("starmaps");
    fs::create_dir_all(&starmaps_dir).unwrap();

    let index_rel = "starmaps/index.json";
    let index_abs = live.join(index_rel);

    // base: index { starmapIds: ["A"] }
    fs::write(&index_abs, make_index_json(&["A"], 0)).unwrap();

    let run = StagingRun::create(tmp.path(), live.clone()).unwrap();
    run.build_base_snapshot_from_live(&live, &[PathBuf::from(index_rel)])
        .unwrap();

    // local: index { starmapIds: ["A", "B"] }（本地新增 B）
    // 用 atomic_write rename 替换，hard-link 的 base 保留旧 inode。
    let local_bytes = make_index_json(&["A", "B"], 100);
    crate::storage::atomic_write_bytes(&index_abs, &local_bytes).unwrap();

    // incoming(staging): index { starmapIds: ["A", "C"] }（远端新增 C）
    let staging_starmaps_dir = run.staging_root().join("starmaps");
    fs::create_dir_all(&staging_starmaps_dir).unwrap();
    fs::write(
        run.staging_root().join(index_rel),
        make_index_json(&["A", "C"], 200),
    )
    .unwrap();

    let plan = run.compute_commit_plan(&live).unwrap();

    // 合并后: index { starmapIds: ["A", "B", "C"] }
    // 应产生一个 content_action（Apply），不冲突，不 keep_local。
    assert_eq!(
        plan.content_actions.len(),
        1,
        "semantic merge should produce exactly one content action"
    );
    assert!(
        plan.conflict.is_empty(),
        "semantic merge should not produce conflicts"
    );

    // 验证合并后的内容包含 A, B, C。
    let merged_bytes = match &plan.content_actions[0] {
        CommitAction::Apply { content, .. } => content,
        CommitAction::Delete { .. } => panic!("expected Apply, got Delete"),
    };
    let merged: serde_json::Value =
        serde_json::from_slice(merged_bytes).expect("deserialize merged index");
    let merged_ids: Vec<String> = merged["starmapIds"]
        .as_array()
        .expect("starmapIds should be array")
        .iter()
        .map(|v| v.as_str().expect("id should be string").to_string())
        .collect();
    assert_eq!(
        merged_ids,
        vec!["A".to_string(), "B".to_string(), "C".to_string()],
        "merged starmapIds should contain all three IDs"
    );
}

/// issue #902 第八轮复核：starmaps/<id>/graph.json 走语义合并。
/// 双端并发创建不同节点时，合并后应保留双方新增的节点 ID。
///
/// 第十轮复核：统一合并路径 `merge_starmap_graph_with_objects` 用 `final_objects`
/// （从对象文件决策得到的最终对象集合）驱动成员表。仅有 graph.json 无对象文件时
/// `final_objects` 为空，`reconcile_member_list` 返回空成员表。因此测试需要同时
/// 设置节点对象文件（`starmaps/<id>/nodes/<bucket>/<obj_id>.json`）。
#[test]
fn starmap_graph_meta_semantic_merge_both_add_node() {
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    let graph_dir = live.join("starmaps").join("sm_x");
    fs::create_dir_all(&graph_dir).unwrap();

    let graph_rel = "starmaps/sm_x/graph.json";
    let graph_abs = live.join(graph_rel);

    // 节点对象文件路径：starmaps/sm_x/nodes/00/<node_id>.json
    // bucket 必须是 2 个小写 hex 字符（见 is_valid_bucket）。
    let node_x_rel = "starmaps/sm_x/nodes/00/x.json";
    let node_x_abs = live.join(node_x_rel);
    fs::create_dir_all(node_x_abs.parent().unwrap()).unwrap();

    // base: graph { nodeIds: ["x"] } + 节点对象 x
    fs::write(&graph_abs, make_graph_meta_json("sm_x", &["x"], 0)).unwrap();
    fs::write(&node_x_abs, make_node_json("x")).unwrap();

    let run = StagingRun::create(tmp.path(), live.clone()).unwrap();
    run.build_base_snapshot_from_live(
        &live,
        &[PathBuf::from(graph_rel), PathBuf::from(node_x_rel)],
    )
    .unwrap();

    // local: graph { nodeIds: ["x", "n1"] }（本地新增 n1）+ 节点对象 n1
    let local_bytes = make_graph_meta_json("sm_x", &["x", "n1"], 100);
    crate::storage::atomic_write_bytes(&graph_abs, &local_bytes).unwrap();
    let node_n1_abs = live.join("starmaps/sm_x/nodes/00/n1.json");
    fs::write(&node_n1_abs, make_node_json("n1")).unwrap();

    // incoming: graph { nodeIds: ["x", "n2"] }（远端新增 n2）+ 节点对象 x, n2
    let staging_graph_dir = run.staging_root().join("starmaps").join("sm_x");
    fs::create_dir_all(&staging_graph_dir).unwrap();
    fs::write(
        run.staging_root().join(graph_rel),
        make_graph_meta_json("sm_x", &["x", "n2"], 200),
    )
    .unwrap();
    let staging_node_dir = run
        .staging_root()
        .join("starmaps")
        .join("sm_x")
        .join("nodes")
        .join("00");
    fs::create_dir_all(&staging_node_dir).unwrap();
    fs::write(staging_node_dir.join("x.json"), make_node_json("x")).unwrap();
    fs::write(staging_node_dir.join("n2.json"), make_node_json("n2")).unwrap();

    let plan = run.compute_commit_plan(&live).unwrap();

    // 合并后: graph { nodeIds: ["x", "n1", "n2"] }
    assert!(
        plan.conflict.is_empty(),
        "semantic merge should not produce conflicts"
    );

    // 统一合并会产生 graph.json 的 ApplyMerged + n2.json 的 ApplyIncoming。
    // 从 content_actions 中找到 graph.json 的合并结果。
    let merged_bytes = plan
        .content_actions
        .iter()
        .find_map(|action| match action {
            CommitAction::Apply { rel_path, content } if rel_path == &PathBuf::from(graph_rel) => {
                Some(content.clone())
            }
            _ => None,
        })
        .expect("graph.json should be in content_actions with merged content");
    let merged: serde_json::Value =
        serde_json::from_slice(&merged_bytes).expect("deserialize merged graph meta");
    let merged_ids: Vec<String> = merged["nodeIds"]
        .as_array()
        .expect("nodeIds should be array")
        .iter()
        .map(|v| v.as_str().expect("id should be string").to_string())
        .collect();
    assert_eq!(
        merged_ids,
        vec!["x".to_string(), "n1".to_string(), "n2".to_string()],
        "merged nodeIds should contain all three IDs"
    );
}

/// #955：本机统计设备组在 seed 之前完成旧事件库迁移，迁移结果进入 staging。
#[test]
fn prepare_staging_runs_migrates_local_stats_group_before_seed() {
    use crate::sync::full_sync::{FullSyncPlan, PlannedTarget};
    use crate::sync::types::{AppDataGroup, PlannedTargetKind, SyncPolicy, SyncTarget};

    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("app");
    let legacy_dir = live.join("app-meta/stats/events.local");
    fs::create_dir_all(&legacy_dir).unwrap();
    let event = crate::writing_stats::WritingInputEvent::with_identity(
        "evt-1".to_string(),
        1_714_550_400_000,
        "device_1",
        crate::writing_stats::Platform::Desktop,
        "desktop",
        "proj",
        "vol",
        "chap",
        crate::writing_stats::EventSource::HumanTyped,
        10,
        0,
        0,
        0,
        0,
        "session",
    );
    fs::write(
        legacy_dir.join("2024-05-01.events.jsonl"),
        format!("{}\n", serde_json::to_string(&event).unwrap()),
    )
    .unwrap();
    crate::settings::ensure_device_info(&live, "", "", Some("device_1")).unwrap();

    let planned = PlannedTarget {
        target: SyncTarget::app_data_group(AppDataGroup::StatsDevice("device_1".to_string())),
        local_root: live.clone(),
        staging_root: None,
        target_kind: PlannedTargetKind::App,
        project_id: None,
        target_live_root: live.clone(),
        deleted_journal_token: None,
        deleted_lww: None,
        live_lww: None,
        expected_delete_lww: None,
    };
    let mut plan = FullSyncPlan {
        sync_policy: SyncPolicy {
            enabled: true,
            ..Default::default()
        },
        force_sync: false,
        targets: vec![planned],
        app_data_root: tmp.path().to_path_buf(),
        remote_catalog_snapshot: crate::sync::full_sync::tests::test_empty_catalog_snapshot(),
    };

    let runs = prepare_staging_runs(&mut plan).unwrap();
    assert_eq!(runs.len(), 1);
    assert!(runs[0].prepare_error().is_none());
    // 迁移写进 live 的新分区，随后 seed 进入 staging。
    assert!(live
        .join("stats/events/device_1/2024-05-01.events.jsonl")
        .is_file());
    assert!(runs[0]
        .staging_root()
        .join("stats/events/device_1/2024-05-01.events.jsonl")
        .is_file());
    assert!(plan.targets[0].staging_root.is_some());
}

/// #955：本机统计迁移失败只让该组带上 prepare_error，不终止整轮同步。
#[test]
fn prepare_staging_runs_records_stats_migration_failure_per_group() {
    use crate::sync::full_sync::{FullSyncPlan, PlannedTarget};
    use crate::sync::types::{AppDataGroup, PlannedTargetKind, SyncPolicy, SyncTarget};

    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("app");
    let legacy_dir = live.join("app-meta/stats/events.local");
    fs::create_dir_all(&legacy_dir).unwrap();
    // 损坏的旧事件行让迁移失败。
    fs::write(legacy_dir.join("2024-05-01.events.jsonl"), "{\"bad\":}\n").unwrap();
    crate::settings::ensure_device_info(&live, "", "", Some("device_1")).unwrap();

    let stats_target = PlannedTarget {
        target: SyncTarget::app_data_group(AppDataGroup::StatsDevice("device_1".to_string())),
        local_root: live.clone(),
        staging_root: None,
        target_kind: PlannedTargetKind::App,
        project_id: None,
        target_live_root: live.clone(),
        deleted_journal_token: None,
        deleted_lww: None,
        live_lww: None,
        expected_delete_lww: None,
    };
    let global_target = PlannedTarget {
        target: SyncTarget::app_data_group(AppDataGroup::Global),
        local_root: live.clone(),
        staging_root: None,
        target_kind: PlannedTargetKind::App,
        project_id: None,
        target_live_root: live.clone(),
        deleted_journal_token: None,
        deleted_lww: None,
        live_lww: None,
        expected_delete_lww: None,
    };
    let mut plan = FullSyncPlan {
        sync_policy: SyncPolicy {
            enabled: true,
            ..Default::default()
        },
        force_sync: false,
        targets: vec![stats_target, global_target],
        app_data_root: tmp.path().to_path_buf(),
        remote_catalog_snapshot: crate::sync::full_sync::tests::test_empty_catalog_snapshot(),
    };

    let runs = prepare_staging_runs(&mut plan).unwrap();
    assert_eq!(runs.len(), 2);
    let error = runs[0]
        .prepare_error()
        .expect("stats group must record migration failure");
    assert!(
        error.starts_with("migration: "),
        "unexpected error: {error}"
    );
    // 迁移失败不写新分区，该组 staging 也不会带上半迁移数据。
    assert!(!live.join("stats/events").exists());
    assert!(!runs[0].staging_root().join("stats/events").exists());
    // 其它独立组照常 seed，不受影响。
    assert!(runs[1].prepare_error().is_none());
}
