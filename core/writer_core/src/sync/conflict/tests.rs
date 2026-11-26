use super::*;
use crate::starmap::store::meta::GraphMeta;
use crate::sync::SyncService;
use tempfile::TempDir;

/// 创建一个最小 GraphMeta JSON 用于测试。
fn make_graph_json(starmap_id: &str) -> String {
    let graph = GraphMeta {
        schema_version: "4".to_string(),
        starmap_id: starmap_id.to_string(),
        ..Default::default()
    };
    serde_json::to_string_pretty(&graph).unwrap()
}

/// 读取并解析 graph.json。
fn read_graph(sync_root: &Path, starmap_id: &str) -> GraphMeta {
    let graph_path = sync_root.join(format!("starmaps/{}/graph.json", starmap_id));
    let content = std::fs::read_to_string(&graph_path).unwrap();
    serde_json::from_str(&content).unwrap()
}

/// 在 sync_root 下创建 starmaps/<id>/graph.json。
fn write_graph(sync_root: &Path, starmap_id: &str, json: &str) {
    let graph_dir = sync_root.join(format!("starmaps/{}", starmap_id));
    std::fs::create_dir_all(&graph_dir).unwrap();
    std::fs::write(graph_dir.join("graph.json"), json).unwrap();
}

/// 构造最小合法星图对象 JSON bytes，用于 restore 测试的 remote_content。
///
/// 派生索引重建会解析对象 JSON，因此需要合法的领域对象。
fn make_object_bytes(object_type: &str, object_id: &str) -> Vec<u8> {
    let path = serde_json::json!({"starmapId": "sm_1", "target": {"type": "starmap"}});
    let obj = match object_type {
        "node" => serde_json::json!({
            "id": object_id,
            "title": "t",
            "kind": "Note",
            "position": {"x": 0.0, "y": 0.0},
            "createdAt": 0,
            "updatedAt": 0
        }),
        "edge" => serde_json::json!({
            "id": object_id,
            "from": path,
            "to": path,
            "kind": "RelatedTo",
            "createdAt": 0,
            "updatedAt": 0
        }),
        "embed" => serde_json::json!({
            "instanceId": object_id,
            "targetStarmapId": "sm_2",
            "position": {"x": 0.0, "y": 0.0},
            "hostPath": path,
            "provenance": {
                "source": "human",
                "sourceId": null,
                "generatedBy": null,
                "promptId": null,
                "reviewStatus": "accepted",
                "createdFromAnchor": null
            },
            "createdAt": 0,
            "updatedAt": 0
        }),
        "link" => serde_json::json!({
            "linkId": object_id,
            "source": path,
            "target": path,
            "createdAt": 0,
            "updatedAt": 0
        }),
        _ => serde_json::json!({}),
    };
    serde_json::to_vec(&obj).unwrap()
}

// =========================================================================
// 正常恢复：graph.json 不含对象 X → 调用后含 X
// =========================================================================

#[test]
fn test_restore_starmap_node_membership() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    write_graph(root, "sm_1", &make_graph_json("sm_1"));

    // 路径：starmaps/sm_1/nodes/00/node_abc.json
    let path = "starmaps/sm_1/nodes/00/node_abc.json";
    let content = make_object_bytes("node", "node_abc");
    SyncService::restore_starmap_object_membership(root, path, &content).unwrap();

    let graph = read_graph(root, "sm_1");
    assert!(
        graph.node_ids.iter().any(|x| x == "node_abc"),
        "node_abc should be in node_ids after restore"
    );
    assert_eq!(
        graph.node_revisions.get("node_abc"),
        Some(&graph.package_revision),
        "node_revisions should match package_revision"
    );
}

#[test]
fn test_restore_starmap_edge_membership() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    write_graph(root, "sm_1", &make_graph_json("sm_1"));

    let path = "starmaps/sm_1/edges/00/edge_xyz.json";
    let content = make_object_bytes("edge", "edge_xyz");
    SyncService::restore_starmap_object_membership(root, path, &content).unwrap();

    let graph = read_graph(root, "sm_1");
    assert!(
        graph.edge_ids.iter().any(|x| x == "edge_xyz"),
        "edge_xyz should be in edge_ids after restore"
    );
}

#[test]
fn test_restore_starmap_embed_membership() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    write_graph(root, "sm_1", &make_graph_json("sm_1"));

    let path = "starmaps/sm_1/embeds/00/emb_001.json";
    let content = make_object_bytes("embed", "emb_001");
    SyncService::restore_starmap_object_membership(root, path, &content).unwrap();

    let graph = read_graph(root, "sm_1");
    assert!(
        graph.embed_instance_ids.iter().any(|x| x == "emb_001"),
        "emb_001 should be in embed_instance_ids after restore"
    );
}

#[test]
fn test_restore_starmap_link_membership() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    write_graph(root, "sm_1", &make_graph_json("sm_1"));

    // 新契约（Issue #1060）：链接不是星图包内对象，唯一事实在 annotations
    // 仓库；同步冲突恢复不会再向 graph.json 的派生成员表 link_ids 添加链接。
    let path = "starmaps/sm_1/links/00/link_001.json";
    let content = make_object_bytes("link", "link_001");
    SyncService::restore_starmap_object_membership(root, path, &content).unwrap();

    let graph = read_graph(root, "sm_1");
    assert!(
        graph.link_ids.is_empty(),
        "links are not package objects: restore must not derive link_ids membership"
    );
}

// =========================================================================
// 已存在不重复添加
// =========================================================================

#[test]
fn test_restore_starmap_membership_no_duplicate() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    // graph.json 已含 node_abc
    let mut graph = GraphMeta {
        schema_version: "4".to_string(),
        starmap_id: "sm_1".to_string(),
        ..Default::default()
    };
    graph.node_ids.push("node_abc".to_string());
    graph.package_revision = 5;
    let json = serde_json::to_string_pretty(&graph).unwrap();
    write_graph(root, "sm_1", &json);

    let path = "starmaps/sm_1/nodes/00/node_abc.json";
    let content = make_object_bytes("node", "node_abc");
    SyncService::restore_starmap_object_membership(root, path, &content).unwrap();

    let updated = read_graph(root, "sm_1");
    let count = updated.node_ids.iter().filter(|x| x == &"node_abc").count();
    assert_eq!(count, 1, "node_abc should not be duplicated in node_ids");
}

// =========================================================================
// 非星图路径跳过
// =========================================================================

#[test]
fn test_restore_starmap_membership_skip_non_starmap_path() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    write_graph(root, "sm_1", &make_graph_json("sm_1"));

    // 普通正文路径，不是星图对象文件
    let path = "chapters/ch_001/text.txt";
    // 非星图路径：防御性返回 Ok(())，不修改 graph。
    let content = b"some text".to_vec();
    SyncService::restore_starmap_object_membership(root, path, &content).unwrap();

    let graph = read_graph(root, "sm_1");
    assert!(
        graph.node_ids.is_empty(),
        "non-starmap path should not modify graph"
    );
}

// =========================================================================
// graph.json 不存在：返回 Err，不宣告成功
// =========================================================================

#[test]
fn test_restore_starmap_membership_no_graph_json() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    // 不创建 graph.json，函数应返回 Err（不再 best-effort）
    let path = "starmaps/sm_1/nodes/00/node_abc.json";
    let content = make_object_bytes("node", "node_abc");
    let result = SyncService::restore_starmap_object_membership(root, path, &content);
    assert!(
        result.is_err(),
        "restore should return Err when graph.json does not exist"
    );

    // graph.json 不应被创建
    assert!(
        !root.join("starmaps/sm_1/graph.json").exists(),
        "graph.json should not be created when it didn't exist"
    );
}

// =========================================================================
// graph.json 内容损坏：返回 Err，不宣告成功
// =========================================================================

#[test]
fn test_restore_starmap_membership_corrupt_graph_json() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    write_graph(root, "sm_1", "not valid json {{{");

    let path = "starmaps/sm_1/nodes/00/node_abc.json";
    // 损坏的 graph.json 应返回 Err，不宣告成功
    let content = make_object_bytes("node", "node_abc");
    let result = SyncService::restore_starmap_object_membership(root, path, &content);
    assert!(
        result.is_err(),
        "restore should return Err when graph.json is corrupt"
    );
}

// =========================================================================
// package_revision 递增验证
// =========================================================================

#[test]
fn test_restore_starmap_membership_increments_revision() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    let graph = GraphMeta {
        schema_version: "4".to_string(),
        starmap_id: "sm_1".to_string(),
        package_revision: 10,
        ..Default::default()
    };
    let json = serde_json::to_string_pretty(&graph).unwrap();
    write_graph(root, "sm_1", &json);

    let path = "starmaps/sm_1/nodes/00/node_new.json";
    let content = make_object_bytes("node", "node_new");
    SyncService::restore_starmap_object_membership(root, path, &content).unwrap();

    let updated = read_graph(root, "sm_1");
    assert_eq!(
        updated.package_revision, 11,
        "package_revision should increment by 1"
    );
    assert_eq!(
        updated.node_revisions.get("node_new"),
        Some(&11u64),
        "node_revisions should be set to new package_revision"
    );
}

// =========================================================================
// issue #902 第十四轮复核：read_member_files 返回 Err on 缺失对象
// =========================================================================

/// 用 bucket_for_id 构造对象文件相对路径。
fn object_rel(starmap_id: &str, type_dir: &str, object_id: &str) -> String {
    use crate::starmap::package_storage::bucket_for_id;
    let bucket = bucket_for_id(object_id);
    format!(
        "starmaps/{}/{}/{}/{}.json",
        starmap_id, type_dir, bucket, object_id
    )
}

/// 写对象文件到正确 bucket 路径。
fn write_object_file(root: &Path, starmap_id: &str, type_dir: &str, object_id: &str, bytes: &[u8]) {
    let rel = object_rel(starmap_id, type_dir, object_id);
    let path = root.join(&rel);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(&path, bytes).unwrap();
}

/// 写最小 index.json。
fn write_index(root: &Path, starmap_ids: &[&str]) {
    let idx = crate::starmap::StarMapIndexRecord {
        schema_version: 4,
        starmap_ids: starmap_ids.iter().map(|s| s.to_string()).collect(),
        root_starmap_ids: starmap_ids.iter().map(|s| s.to_string()).collect(),
        main_starmap_by_project: std::collections::HashMap::new(),
        updated_at: 0,
    };
    let dir = root.join("starmaps");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("index.json"),
        serde_json::to_string_pretty(&idx).unwrap(),
    )
    .unwrap();
}

/// 读 index.json。
fn read_index(root: &Path) -> crate::starmap::StarMapIndexRecord {
    let content = std::fs::read_to_string(root.join("starmaps/index.json")).unwrap_or_default();
    serde_json::from_str(&content).unwrap_or_else(|_| crate::starmap::StarMapIndexRecord {
        schema_version: 4,
        starmap_ids: vec![],
        root_starmap_ids: vec![],
        main_starmap_by_project: std::collections::HashMap::new(),
        updated_at: 0,
    })
}

#[test]
fn test_read_member_files_err_on_missing_object() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    // graph.json 含 node_n1，但不创建 n1 的对象文件
    let mut graph = GraphMeta {
        schema_version: "4".to_string(),
        starmap_id: "sm_1".to_string(),
        ..Default::default()
    };
    graph.node_ids.push("n1".to_string());
    write_graph(root, "sm_1", &serde_json::to_string_pretty(&graph).unwrap());

    // collect_all_member_object_contents 应返回 Err（n1 对象文件缺失）
    let result = SyncService::collect_all_member_object_contents(root, "sm_1", &graph);
    assert!(
        result.is_err(),
        "collect_all_member_object_contents 应在成员对象缺失时返回 Err"
    );
}

// =========================================================================
// issue #902 第十四轮复核：apply_starmap_object_remote_delete（问题 3）
// =========================================================================

#[test]
fn test_apply_starmap_object_remote_delete_removes_member() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    // graph.json 含 node_n1
    let mut graph = GraphMeta {
        schema_version: "4".to_string(),
        starmap_id: "sm_1".to_string(),
        package_revision: 5,
        ..Default::default()
    };
    graph.node_ids.push("n1".to_string());
    graph.node_revisions.insert("n1".to_string(), 5);
    let json = serde_json::to_string_pretty(&graph).unwrap();
    write_graph(root, "sm_1", &json);

    // 创建 n1 对象文件
    let n1_bytes = make_object_bytes("node", "n1");
    write_object_file(root, "sm_1", "nodes", "n1", &n1_bytes);

    let path = object_rel("sm_1", "nodes", "n1");
    SyncService::apply_starmap_object_remote_delete(root, &path).unwrap();

    // 验证 n1 从 node_ids 移除
    let updated = read_graph(root, "sm_1");
    assert!(
        !updated.node_ids.iter().any(|x| x == "n1"),
        "n1 应从 node_ids 移除"
    );
    // 验证 tombstone 添加
    assert!(
        updated
            .deleted_since_last_sync
            .entries
            .iter()
            .any(|e| e.object_type == "node" && e.object_id == "n1"),
        "应在 deleted_since_last_sync 添加 n1 tombstone"
    );
    // 验证 package_revision 递增
    assert_eq!(updated.package_revision, 6);
    // 验证对象文件已删除
    assert!(!root.join(&path).exists(), "对象文件应被删除");
}

#[test]
fn test_apply_starmap_object_remote_delete_cascade_removes_dangling_edge() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    // graph.json 含 node_n1 和 edge_e1（e1 引用 n1）
    let mut graph = GraphMeta {
        schema_version: "4".to_string(),
        starmap_id: "sm_1".to_string(),
        ..Default::default()
    };
    graph.node_ids.push("n1".to_string());
    graph.edge_ids.push("e1".to_string());
    let json = serde_json::to_string_pretty(&graph).unwrap();
    write_graph(root, "sm_1", &json);

    // 创建 n1 和 e1 对象文件
    let n1_bytes = make_object_bytes("node", "n1");
    write_object_file(root, "sm_1", "nodes", "n1", &n1_bytes);

    // e1 的 from/to 引用 n1
    let node_path = serde_json::json!({
        "starmapId": "sm_1",
        "target": {"type": "node", "node_id": "n1"}
    });
    let edge_e1 = serde_json::json!({
        "id": "e1",
        "from": node_path,
        "to": node_path,
        "kind": "RelatedTo",
        "createdAt": 0,
        "updatedAt": 0
    });
    write_object_file(
        root,
        "sm_1",
        "edges",
        "e1",
        &serde_json::to_vec(&edge_e1).unwrap(),
    );

    let path = object_rel("sm_1", "nodes", "n1");
    SyncService::apply_starmap_object_remote_delete(root, &path).unwrap();

    // 验证 n1 和 e1 都从成员表移除（级联清理）
    let updated = read_graph(root, "sm_1");
    assert!(!updated.node_ids.iter().any(|x| x == "n1"), "n1 应移除");
    assert!(
        !updated.edge_ids.iter().any(|x| x == "e1"),
        "e1 应级联移除（引用已删 node n1）"
    );
    // 验证两个对象文件都删除
    assert!(!root.join(object_rel("sm_1", "nodes", "n1")).exists());
    assert!(!root.join(object_rel("sm_1", "edges", "e1")).exists());
}

// =========================================================================
// issue #902 第十四轮复核：apply_starmap_graph_remote_delete（问题 2a）
// =========================================================================

#[test]
fn test_apply_starmap_graph_remote_delete_removes_graph_and_index() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    // 创建 graph.json + 对象文件 + index.json
    let mut graph = GraphMeta {
        schema_version: "4".to_string(),
        starmap_id: "sm_1".to_string(),
        ..Default::default()
    };
    graph.node_ids.push("n1".to_string());
    write_graph(root, "sm_1", &serde_json::to_string_pretty(&graph).unwrap());
    write_object_file(
        root,
        "sm_1",
        "nodes",
        "n1",
        &make_object_bytes("node", "n1"),
    );
    write_index(root, &["sm_1", "sm_2"]);

    let graph_path = "starmaps/sm_1/graph.json";
    SyncService::apply_starmap_graph_remote_delete(root, graph_path).unwrap();

    // 验证 graph.json 删除
    assert!(!root.join(graph_path).exists(), "graph.json 应删除");
    // 验证子对象删除
    assert!(
        !root.join(object_rel("sm_1", "nodes", "n1")).exists(),
        "子对象应删除"
    );
    // 验证 index.json 移除 sm_1
    let idx = read_index(root);
    assert!(
        !idx.starmap_ids.iter().any(|x| x == "sm_1"),
        "index 应移除 sm_1"
    );
    assert!(
        idx.starmap_ids.iter().any(|x| x == "sm_2"),
        "index 应保留 sm_2"
    );
}

// =========================================================================
// issue #902 第十四轮复核：apply_starmap_graph_remote_modify（问题 2b）
// =========================================================================

#[test]
fn test_apply_starmap_graph_remote_modify_restores_graph() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    // 远端 graph.json 含 node_rn1
    let mut remote_graph = GraphMeta {
        schema_version: "4".to_string(),
        starmap_id: "sm_1".to_string(),
        ..Default::default()
    };
    remote_graph.node_ids.push("rn1".to_string());
    let remote_graph_bytes = serde_json::to_vec_pretty(&remote_graph).unwrap();

    // 保存远端 graph snapshot
    let snapshot_rel = "starmaps/sm_1/graph.json.remote-conflict-20260101-000000";
    std::fs::create_dir_all(root.join("starmaps/sm_1")).unwrap();
    std::fs::write(root.join(snapshot_rel), &remote_graph_bytes).unwrap();

    // 保存远端子对象快照
    let snapshot_dir = ".conflict-snapshot/starmaps/sm_1/20260101-000000";
    let rn1_bytes = make_object_bytes("node", "rn1");
    let snap_obj_rel = format!("{}/{}", snapshot_dir, object_rel("sm_1", "nodes", "rn1"));
    let snap_obj_path = root.join(&snap_obj_rel);
    if let Some(parent) = snap_obj_path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(&snap_obj_path, &rn1_bytes).unwrap();

    // issue #902 第十七轮复核问题4：快照必须包含 meta.json，
    // 否则 read_remote_meta_snapshot 返回 Err。
    let snap_meta_rel = format!("{}/starmaps/sm_1.meta.json", snapshot_dir);
    let snap_meta_path = root.join(&snap_meta_rel);
    if let Some(parent) = snap_meta_path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    let meta_bytes = serde_json::json!({
        "starmapId": "sm_1",
        "title": "remote meta",
        "description": "",
        "projectId": null,
        "accentColor": "#000000",
        "createdAt": 0,
        "updatedAt": 0
    });
    std::fs::write(
        &snap_meta_path,
        serde_json::to_vec_pretty(&meta_bytes).unwrap(),
    )
    .unwrap();

    // index.json 不含 sm_1（本地已删除）
    write_index(root, &["sm_2"]);

    // issue #902 第十九轮复核问题3：快照必须包含 index_facts.json，
    // 否则 load_starmap_identity_facts 返回 Err。本测试中 sm_1 不是 root，
    // 也不是任何 project 的 main starmap。
    let facts = serde_json::json!({
        "is_root": false,
        "main_for_projects": []
    });
    let facts_rel = format!("{}/index_facts.json", snapshot_dir);
    let facts_path = root.join(&facts_rel);
    if let Some(parent) = facts_path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(&facts_path, serde_json::to_vec_pretty(&facts).unwrap()).unwrap();

    let conflict = SyncConflict {
        local_path: "starmaps/sm_1/graph.json".to_string(),
        remote_path: "remote/starmaps/sm_1/graph.json".to_string(),
        local_hash: "local".to_string(),
        remote_hash: "remote".to_string(),
        base_hash: "base".to_string(),
        created_at: 0,
        description: "graph both changed".to_string(),
        kind: SyncConflictKind::BothChanged,
        remote_snapshot_path: Some(snapshot_rel.to_string()),
        remote_graph_snapshot_dir: Some(snapshot_dir.to_string()),
    };

    SyncService::apply_starmap_graph_remote_modify(root, "starmaps/sm_1/graph.json", &conflict)
        .unwrap();

    // 验证 graph.json 恢复
    let restored = read_graph(root, "sm_1");
    assert!(
        restored.node_ids.iter().any(|x| x == "rn1"),
        "graph.json 应含远端 node rn1"
    );
    // 验证子对象恢复
    assert!(
        root.join(object_rel("sm_1", "nodes", "rn1")).exists(),
        "子对象 rn1 应恢复"
    );
    // 验证 index.json 含 sm_1
    let idx = read_index(root);
    assert!(
        idx.starmap_ids.iter().any(|x| x == "sm_1"),
        "index 应恢复 sm_1"
    );
}

// =========================================================================
// issue #902 第十五轮复核：apply_starmap_object_remote_delete 返回删除路径集合
// =========================================================================

#[test]
fn test_apply_starmap_object_remote_delete_returns_all_deleted_paths() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    // graph.json 含 node_n1 和 edge_e1（e1 引用 n1）
    let mut graph = GraphMeta {
        schema_version: "4".to_string(),
        starmap_id: "sm_1".to_string(),
        ..Default::default()
    };
    graph.node_ids.push("n1".to_string());
    graph.edge_ids.push("e1".to_string());
    let json = serde_json::to_string_pretty(&graph).unwrap();
    write_graph(root, "sm_1", &json);

    // 创建 n1 对象文件
    let n1_bytes = make_object_bytes("node", "n1");
    write_object_file(root, "sm_1", "nodes", "n1", &n1_bytes);

    // e1 的 from/to 引用 n1
    let node_path = serde_json::json!({
        "starmapId": "sm_1",
        "target": {"type": "node", "node_id": "n1"}
    });
    let edge_e1 = serde_json::json!({
        "id": "e1",
        "from": node_path,
        "to": node_path,
        "kind": "RelatedTo",
        "createdAt": 0,
        "updatedAt": 0
    });
    write_object_file(
        root,
        "sm_1",
        "edges",
        "e1",
        &serde_json::to_vec(&edge_e1).unwrap(),
    );

    let path = object_rel("sm_1", "nodes", "n1");
    let deleted_paths = SyncService::apply_starmap_object_remote_delete(root, &path).unwrap();

    // 验证返回的路径集合包含 n1 和 e1 的路径
    let n1_rel = object_rel("sm_1", "nodes", "n1");
    let e1_rel = object_rel("sm_1", "edges", "e1");
    assert!(
        deleted_paths.iter().any(|p| p == &n1_rel),
        "删除路径集合应包含 n1 路径"
    );
    assert!(
        deleted_paths.iter().any(|p| p == &e1_rel),
        "删除路径集合应包含级联删除的 e1 路径"
    );

    // 构造 SyncState，对每个删除路径添加 tombstone
    let mut state = crate::sync::types::SyncState::default();
    for del_path in &deleted_paths {
        SyncService::record_remote_delete_tombstone(&mut state, del_path);
    }
    // 验证 tombstones 包含所有删除路径
    for del_path in &deleted_paths {
        assert!(
            state
                .tombstones
                .iter()
                .any(|t| t.original_path == *del_path),
            "tombstones 应包含删除路径 '{}'",
            del_path
        );
    }
}

// =========================================================================
// issue #902 第十五轮复核：apply_starmap_graph_remote_modify 恢复 root/main 身份
// =========================================================================

#[test]
fn test_apply_starmap_graph_remote_modify_restores_root_identity() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    // 远端 graph.json 含 node_rn1
    let mut remote_graph = GraphMeta {
        schema_version: "4".to_string(),
        starmap_id: "sm_1".to_string(),
        ..Default::default()
    };
    remote_graph.node_ids.push("rn1".to_string());
    let remote_graph_bytes = serde_json::to_vec_pretty(&remote_graph).unwrap();

    // 保存远端 graph snapshot
    let snapshot_rel = "starmaps/sm_1/graph.json.remote-conflict-20260101-000000";
    std::fs::create_dir_all(root.join("starmaps/sm_1")).unwrap();
    std::fs::write(root.join(snapshot_rel), &remote_graph_bytes).unwrap();

    // 保存远端子对象快照
    let snapshot_dir = ".conflict-snapshot/starmaps/sm_1/20260101-000000";
    let rn1_bytes = make_object_bytes("node", "rn1");
    let snap_obj_rel = format!("{}/{}", snapshot_dir, object_rel("sm_1", "nodes", "rn1"));
    let snap_obj_path = root.join(&snap_obj_rel);
    if let Some(parent) = snap_obj_path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(&snap_obj_path, &rn1_bytes).unwrap();

    // issue #902 第十七轮复核问题4：快照必须包含 meta.json，
    // 否则 read_remote_meta_snapshot 返回 Err。
    let snap_meta_rel = format!("{}/starmaps/sm_1.meta.json", snapshot_dir);
    let snap_meta_path = root.join(&snap_meta_rel);
    if let Some(parent) = snap_meta_path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    let meta_bytes = serde_json::json!({
        "starmapId": "sm_1",
        "title": "remote meta",
        "description": "",
        "projectId": null,
        "accentColor": "#000000",
        "createdAt": 0,
        "updatedAt": 0
    });
    std::fs::write(
        &snap_meta_path,
        serde_json::to_vec_pretty(&meta_bytes).unwrap(),
    )
    .unwrap();

    // 保存 index_facts.json：sm_1 是 root，且是 proj1 的 main starmap
    let facts = serde_json::json!({
        "is_root": true,
        "main_for_projects": ["proj1"]
    });
    let facts_rel = format!("{}/index_facts.json", snapshot_dir);
    let facts_path = root.join(&facts_rel);
    if let Some(parent) = facts_path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(&facts_path, serde_json::to_vec_pretty(&facts).unwrap()).unwrap();

    // 本地 index：sm_1 在 starmap_ids 但不在 root_starmap_ids
    let idx = crate::starmap::StarMapIndexRecord {
        schema_version: 4,
        starmap_ids: vec!["sm_1".to_string(), "sm_2".to_string()],
        root_starmap_ids: vec!["sm_2".to_string()],
        main_starmap_by_project: std::collections::HashMap::new(),
        updated_at: 0,
    };
    let dir = root.join("starmaps");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("index.json"),
        serde_json::to_string_pretty(&idx).unwrap(),
    )
    .unwrap();

    let conflict = SyncConflict {
        local_path: "starmaps/sm_1/graph.json".to_string(),
        remote_path: "remote/starmaps/sm_1/graph.json".to_string(),
        local_hash: "local".to_string(),
        remote_hash: "remote".to_string(),
        base_hash: "base".to_string(),
        created_at: 0,
        description: "graph both changed".to_string(),
        kind: SyncConflictKind::BothChanged,
        remote_snapshot_path: Some(snapshot_rel.to_string()),
        remote_graph_snapshot_dir: Some(snapshot_dir.to_string()),
    };

    SyncService::apply_starmap_graph_remote_modify(root, "starmaps/sm_1/graph.json", &conflict)
        .unwrap();

    // 验证恢复后 sm_1 在 root_starmap_ids 中
    let idx = read_index(root);
    assert!(
        idx.root_starmap_ids.iter().any(|x| x == "sm_1"),
        "sm_1 应恢复到 root_starmap_ids"
    );
    // 验证 main_starmap_by_project["proj1"] = "sm_1"
    assert_eq!(
        idx.main_starmap_by_project.get("proj1"),
        Some(&"sm_1".to_string()),
        "main_starmap_by_project['proj1'] 应为 'sm_1'"
    );
}

// =========================================================================
// issue #902 第十五轮复核：cascade_remove_references 传播解析错误
// =========================================================================

#[test]
fn test_cascade_remove_references_propagates_parse_error() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    // graph.json 含 node_n1 和 edge_e1
    let mut graph = GraphMeta {
        schema_version: "4".to_string(),
        starmap_id: "sm_1".to_string(),
        ..Default::default()
    };
    graph.node_ids.push("n1".to_string());
    graph.edge_ids.push("e1".to_string());
    let json = serde_json::to_string_pretty(&graph).unwrap();
    write_graph(root, "sm_1", &json);

    // 创建 n1 对象文件（合法 JSON）
    let n1_bytes = make_object_bytes("node", "n1");
    write_object_file(root, "sm_1", "nodes", "n1", &n1_bytes);

    // e1 的对象文件内容是损坏的 JSON
    write_object_file(root, "sm_1", "edges", "e1", b"{invalid");

    let path = object_rel("sm_1", "nodes", "n1");
    let result = SyncService::apply_starmap_object_remote_delete(root, &path);

    // 验证返回 Err（而不是成功删除）
    assert!(
        result.is_err(),
        "apply_starmap_object_remote_delete 应在 edge 对象文件损坏时返回 Err，\
             而不是吞掉解析错误"
    );
}
