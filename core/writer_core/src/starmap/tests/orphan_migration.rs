use crate::starmap::semantic::{
    StarMapNodeContent, StarMapPortal, StarMapProvenance, StarMapTargetDetail,
};
use crate::starmap::types::reference::StarMapTargetPath;
use crate::starmap::types::*;
use crate::starmap::*;
use tempfile::tempdir;

fn setup_temp_dir() -> tempfile::TempDir {
    let dir = tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    dir
}

/// 创建一个 Note 节点，带 portal 指向指定星图。
fn make_note_node_with_portal(
    node_id: &str,
    title: &str,
    destination_starmap_id: &str,
    created_at: u64,
) -> StarMapNode {
    StarMapNode {
        id: node_id.to_string(),
        title: title.to_string(),
        kind: StarMapNodeKind::Note,
        payload: None,
        tags: Vec::new(),
        content: StarMapNodeContent::Empty,
        anchors: Vec::new(),
        portal: Some(StarMapPortal {
            destination_starmap_id: destination_starmap_id.to_string(),
            destination_target: None,
        }),
        position: StarMapPoint::default(),
        style: StarMapNodeStyle::default(),
        provenance: StarMapProvenance::default(),
        created_at,
        updated_at: created_at,
    }
}

/// 创建一个 Note 节点，不带 portal。
fn make_note_node_no_portal(node_id: &str, title: &str, created_at: u64) -> StarMapNode {
    StarMapNode {
        id: node_id.to_string(),
        title: title.to_string(),
        kind: StarMapNodeKind::Note,
        payload: None,
        tags: Vec::new(),
        content: StarMapNodeContent::Empty,
        anchors: Vec::new(),
        portal: None,
        position: StarMapPoint::default(),
        style: StarMapNodeStyle::default(),
        provenance: StarMapProvenance::default(),
        created_at,
        updated_at: created_at,
    }
}

/// 将 graph 数据保存到磁盘。
fn save_graph(ws: &std::path::Path, starmap_id: &str, graph: &StarMapGraph) {
    let mut store = crate::starmap::store::StarMapStore::new(ws, starmap_id);
    store.load_full().unwrap();
    for node in &graph.nodes {
        store.upsert_node(node.clone());
    }
    for edge in &graph.edges {
        store.upsert_edge(edge.clone());
    }
    for embed in &graph.embeds {
        store.upsert_embed(embed.clone());
    }
    for link in &graph.links {
        store.upsert_link(link.clone());
    }
    for hl in &graph.hyperlinks {
        store.upsert_hyperlink(hl.clone());
    }
    store.flush().unwrap();
}

/// 从磁盘加载 graph 数据。
fn load_graph(ws: &std::path::Path, starmap_id: &str) -> StarMapGraph {
    let mut store = crate::starmap::store::StarMapStore::new(ws, starmap_id);
    store.load_full().unwrap();
    store.to_starmap_graph()
}

// ---------------------------------------------------------------------------
// identify_orphan_starmaps 纯函数测试
// ---------------------------------------------------------------------------

#[test]
fn test_identify_orphans_empty_starmap_with_matching_note() {
    let now = 100_000;
    let host_meta = StarMapMeta {
        starmap_id: "sm_host".to_string(),
        title: "Host".to_string(),
        description: String::new(),
        project_id: None,
        accent_color: "#7B8CDE".to_string(),
        created_at: now,
        updated_at: now,
    };
    let orphan_meta = StarMapMeta {
        starmap_id: "sm_orphan".to_string(),
        title: "Child Title".to_string(),
        description: String::new(),
        project_id: None,
        accent_color: "#7B8CDE".to_string(),
        created_at: now + 5_000, // 5 秒后创建，在 60 秒窗口内
        updated_at: now + 5_000,
    };

    let host_graph = StarMapGraph {
        schema_version: CURRENT_GRAPH_SCHEMA_VERSION,
        starmap_id: "sm_host".to_string(),
        nodes: vec![make_note_node_no_portal("n1", "Child Title", now)],
        edges: vec![],
        embeds: vec![],
        links: vec![],
        hyperlinks: vec![],
    };
    let orphan_graph = StarMapGraph::default();

    let all_starmaps = vec![host_meta, orphan_meta];
    let graphs = vec![host_graph, orphan_graph];

    let orphans = identify_orphan_starmaps(&all_starmaps, &graphs);
    assert_eq!(orphans.len(), 1);
    assert_eq!(orphans[0].starmap_id, "sm_orphan");
}

#[test]
fn test_identify_orphans_not_orphan_if_embed_target() {
    let now = 100_000;
    let host_meta = StarMapMeta {
        starmap_id: "sm_host".to_string(),
        title: "Host".to_string(),
        description: String::new(),
        project_id: None,
        accent_color: "#7B8CDE".to_string(),
        created_at: now,
        updated_at: now,
    };
    let child_meta = StarMapMeta {
        starmap_id: "sm_child".to_string(),
        title: "Child Title".to_string(),
        description: String::new(),
        project_id: None,
        accent_color: "#7B8CDE".to_string(),
        created_at: now + 5_000,
        updated_at: now + 5_000,
    };

    // Host graph has an embed pointing to child — child is NOT an orphan.
    let embed = StarMapEmbed {
        instance_id: "em_1".to_string(),
        target_starmap_id: "sm_child".to_string(),
        label: None,
        position: StarMapPoint::default(),
        host_path: StarMapTargetPath {
            starmap_id: "sm_host".to_string(),
            segments: vec![],
            target: StarMapTargetDetail::Starmap,
        },
        provenance: StarMapProvenance::default(),
        created_at: now,
        updated_at: now,
    };
    let host_graph = StarMapGraph {
        schema_version: CURRENT_GRAPH_SCHEMA_VERSION,
        starmap_id: "sm_host".to_string(),
        nodes: vec![make_note_node_no_portal("n1", "Child Title", now)],
        edges: vec![],
        embeds: vec![embed],
        links: vec![],
        hyperlinks: vec![],
    };
    let child_graph = StarMapGraph::default();

    let all_starmaps = vec![host_meta, child_meta];
    let graphs = vec![host_graph, child_graph];

    let orphans = identify_orphan_starmaps(&all_starmaps, &graphs);
    assert!(orphans.is_empty(), "Embed target should not be orphan");
}

#[test]
fn test_identify_orphans_not_orphan_if_portal_target() {
    let now = 100_000;
    let host_meta = StarMapMeta {
        starmap_id: "sm_host".to_string(),
        title: "Host".to_string(),
        description: String::new(),
        project_id: None,
        accent_color: "#7B8CDE".to_string(),
        created_at: now,
        updated_at: now,
    };
    let child_meta = StarMapMeta {
        starmap_id: "sm_child".to_string(),
        title: "Child Title".to_string(),
        description: String::new(),
        project_id: None,
        accent_color: "#7B8CDE".to_string(),
        created_at: now + 5_000,
        updated_at: now + 5_000,
    };

    // Host graph has a Note node with portal pointing to child — child is NOT an orphan.
    let host_graph = StarMapGraph {
        schema_version: CURRENT_GRAPH_SCHEMA_VERSION,
        starmap_id: "sm_host".to_string(),
        nodes: vec![make_note_node_with_portal(
            "n1",
            "Child Title",
            "sm_child",
            now,
        )],
        edges: vec![],
        embeds: vec![],
        links: vec![],
        hyperlinks: vec![],
    };
    let child_graph = StarMapGraph::default();

    let all_starmaps = vec![host_meta, child_meta];
    let graphs = vec![host_graph, child_graph];

    let orphans = identify_orphan_starmaps(&all_starmaps, &graphs);
    assert!(orphans.is_empty(), "Portal target should not be orphan");
}

#[test]
fn test_identify_orphans_not_orphan_if_time_window_exceeded() {
    let now = 100_000;
    let host_meta = StarMapMeta {
        starmap_id: "sm_host".to_string(),
        title: "Host".to_string(),
        description: String::new(),
        project_id: None,
        accent_color: "#7B8CDE".to_string(),
        created_at: now,
        updated_at: now,
    };
    let orphan_meta = StarMapMeta {
        starmap_id: "sm_orphan".to_string(),
        title: "Child Title".to_string(),
        description: String::new(),
        project_id: None,
        accent_color: "#7B8CDE".to_string(),
        created_at: now + 120_000, // 120 秒后创建，超过 60 秒窗口
        updated_at: now + 120_000,
    };

    let host_graph = StarMapGraph {
        schema_version: CURRENT_GRAPH_SCHEMA_VERSION,
        starmap_id: "sm_host".to_string(),
        nodes: vec![make_note_node_no_portal("n1", "Child Title", now)],
        edges: vec![],
        embeds: vec![],
        links: vec![],
        hyperlinks: vec![],
    };
    let orphan_graph = StarMapGraph::default();

    let all_starmaps = vec![host_meta, orphan_meta];
    let graphs = vec![host_graph, orphan_graph];

    let orphans = identify_orphan_starmaps(&all_starmaps, &graphs);
    assert!(
        orphans.is_empty(),
        "Time window exceeded should not be orphan"
    );
}

#[test]
fn test_identify_orphans_not_orphan_if_has_content() {
    let now = 100_000;
    let host_meta = StarMapMeta {
        starmap_id: "sm_host".to_string(),
        title: "Host".to_string(),
        description: String::new(),
        project_id: None,
        accent_color: "#7B8CDE".to_string(),
        created_at: now,
        updated_at: now,
    };
    let child_meta = StarMapMeta {
        starmap_id: "sm_child".to_string(),
        title: "Child Title".to_string(),
        description: String::new(),
        project_id: None,
        accent_color: "#7B8CDE".to_string(),
        created_at: now + 5_000,
        updated_at: now + 5_000,
    };

    // Child graph has a node — not empty, so not an orphan.
    let child_graph = StarMapGraph {
        schema_version: CURRENT_GRAPH_SCHEMA_VERSION,
        starmap_id: "sm_child".to_string(),
        nodes: vec![StarMapNode {
            id: "n_child".to_string(),
            title: "Something".to_string(),
            kind: StarMapNodeKind::Character,
            payload: None,
            tags: Vec::new(),
            content: StarMapNodeContent::Empty,
            anchors: Vec::new(),
            portal: None,
            position: StarMapPoint::default(),
            style: StarMapNodeStyle::default(),
            provenance: StarMapProvenance::default(),
            created_at: now + 5_000,
            updated_at: now + 5_000,
        }],
        edges: vec![],
        embeds: vec![],
        links: vec![],
        hyperlinks: vec![],
    };

    let host_graph = StarMapGraph {
        schema_version: CURRENT_GRAPH_SCHEMA_VERSION,
        starmap_id: "sm_host".to_string(),
        nodes: vec![make_note_node_no_portal("n1", "Child Title", now)],
        edges: vec![],
        embeds: vec![],
        links: vec![],
        hyperlinks: vec![],
    };

    let all_starmaps = vec![host_meta, child_meta];
    let graphs = vec![host_graph, child_graph];

    let orphans = identify_orphan_starmaps(&all_starmaps, &graphs);
    assert!(orphans.is_empty(), "Non-empty starmap should not be orphan");
}

#[test]
fn test_identify_orphans_not_orphan_if_title_mismatch() {
    let now = 100_000;
    let host_meta = StarMapMeta {
        starmap_id: "sm_host".to_string(),
        title: "Host".to_string(),
        description: String::new(),
        project_id: None,
        accent_color: "#7B8CDE".to_string(),
        created_at: now,
        updated_at: now,
    };
    let other_meta = StarMapMeta {
        starmap_id: "sm_other".to_string(),
        title: "Different Title".to_string(),
        description: String::new(),
        project_id: None,
        accent_color: "#7B8CDE".to_string(),
        created_at: now + 5_000,
        updated_at: now + 5_000,
    };

    let host_graph = StarMapGraph {
        schema_version: CURRENT_GRAPH_SCHEMA_VERSION,
        starmap_id: "sm_host".to_string(),
        nodes: vec![make_note_node_no_portal("n1", "Child Title", now)],
        edges: vec![],
        embeds: vec![],
        links: vec![],
        hyperlinks: vec![],
    };
    let other_graph = StarMapGraph::default();

    let all_starmaps = vec![host_meta, other_meta];
    let graphs = vec![host_graph, other_graph];

    let orphans = identify_orphan_starmaps(&all_starmaps, &graphs);
    assert!(orphans.is_empty(), "Title mismatch should not be orphan");
}

#[test]
fn test_identify_orphans_multiple_orphans() {
    let now = 100_000;
    let host_meta = StarMapMeta {
        starmap_id: "sm_host".to_string(),
        title: "Host".to_string(),
        description: String::new(),
        project_id: None,
        accent_color: "#7B8CDE".to_string(),
        created_at: now,
        updated_at: now,
    };
    let orphan1_meta = StarMapMeta {
        starmap_id: "sm_orphan1".to_string(),
        title: "Child A".to_string(),
        description: String::new(),
        project_id: None,
        accent_color: "#7B8CDE".to_string(),
        created_at: now + 3_000,
        updated_at: now + 3_000,
    };
    let orphan2_meta = StarMapMeta {
        starmap_id: "sm_orphan2".to_string(),
        title: "Child B".to_string(),
        description: String::new(),
        project_id: None,
        accent_color: "#7B8CDE".to_string(),
        created_at: now + 10_000,
        updated_at: now + 10_000,
    };

    let host_graph = StarMapGraph {
        schema_version: CURRENT_GRAPH_SCHEMA_VERSION,
        starmap_id: "sm_host".to_string(),
        nodes: vec![
            make_note_node_no_portal("n1", "Child A", now),
            make_note_node_no_portal("n2", "Child B", now + 8_000),
        ],
        edges: vec![],
        embeds: vec![],
        links: vec![],
        hyperlinks: vec![],
    };
    let orphan1_graph = StarMapGraph::default();
    let orphan2_graph = StarMapGraph::default();

    let all_starmaps = vec![host_meta, orphan1_meta, orphan2_meta];
    let graphs = vec![host_graph, orphan1_graph, orphan2_graph];

    let orphans = identify_orphan_starmaps(&all_starmaps, &graphs);
    assert_eq!(orphans.len(), 2);
    let orphan_ids: Vec<String> = orphans.iter().map(|o| o.starmap_id.clone()).collect();
    assert!(orphan_ids.contains(&"sm_orphan1".to_string()));
    assert!(orphan_ids.contains(&"sm_orphan2".to_string()));
}

// ---------------------------------------------------------------------------
// safe_migrate_orphan_starmaps 集成测试
// ---------------------------------------------------------------------------

#[test]
fn test_safe_migrate_no_orphans() {
    let dir = setup_temp_dir();
    let _sm = create_starmap(dir.path(), "Normal Map", "", None).unwrap();

    let migrated = safe_migrate_orphan_starmaps(dir.path()).unwrap();
    assert!(migrated.is_empty());
}

#[test]
fn test_safe_migrate_orphan_creates_embed() {
    let dir = setup_temp_dir();

    // 创建宿主星图。
    let host = create_starmap(dir.path(), "Host", "", None).unwrap();

    // 在宿主星图中添加一个 Note 节点（标题为 "Child"）。
    let now = now_epoch();
    let host_graph = StarMapGraph {
        schema_version: CURRENT_GRAPH_SCHEMA_VERSION,
        starmap_id: host.starmap_id.clone(),
        nodes: vec![make_note_node_no_portal("n1", "Child", now)],
        edges: vec![],
        embeds: vec![],
        links: vec![],
        hyperlinks: vec![],
    };
    save_graph(dir.path(), &host.starmap_id, &host_graph);

    // 创建一个空白的"孤儿"星图，标题与 Note 节点一致，创建时间接近。
    let orphan = StarMapMeta {
        starmap_id: format!("sm_{}", uuid::Uuid::new_v4()),
        title: "Child".to_string(),
        description: String::new(),
        project_id: None,
        accent_color: "#7B8CDE".to_string(),
        created_at: now + 2_000,
        updated_at: now + 2_000,
    };
    save_starmap_meta(dir.path(), &orphan).unwrap();
    let mut idx = load_index(dir.path()).unwrap();
    idx.starmap_ids.push(orphan.starmap_id.clone());
    idx.updated_at = now_epoch();
    save_index(dir.path(), &idx).unwrap();

    // 运行迁移。
    let migrated = safe_migrate_orphan_starmaps(dir.path()).unwrap();
    assert_eq!(migrated.len(), 1);
    assert_eq!(migrated[0], orphan.starmap_id);

    // 验证宿主星图现在有 embed 指向孤儿星图。
    let host_graph_after = load_graph(dir.path(), &host.starmap_id);
    assert_eq!(host_graph_after.embeds.len(), 1);
    assert_eq!(
        host_graph_after.embeds[0].target_starmap_id,
        orphan.starmap_id
    );

    // 验证孤儿星图不再出现在根列表中。
    let roots = list_root_starmaps(dir.path()).unwrap();
    assert!(
        !roots.iter().any(|r| r.starmap_id == orphan.starmap_id),
        "Orphan should not appear in root list after migration"
    );
}

#[test]
fn test_safe_migrate_does_not_touch_non_empty_starmap() {
    let dir = setup_temp_dir();

    let host = create_starmap(dir.path(), "Host", "", None).unwrap();
    let now = now_epoch();

    let host_graph = StarMapGraph {
        schema_version: CURRENT_GRAPH_SCHEMA_VERSION,
        starmap_id: host.starmap_id.clone(),
        nodes: vec![make_note_node_no_portal("n1", "Child", now)],
        edges: vec![],
        embeds: vec![],
        links: vec![],
        hyperlinks: vec![],
    };
    save_graph(dir.path(), &host.starmap_id, &host_graph);

    // 创建一个非空白的星图，标题与 Note 节点一致。
    let non_empty = StarMapMeta {
        starmap_id: format!("sm_{}", uuid::Uuid::new_v4()),
        title: "Child".to_string(),
        description: String::new(),
        project_id: None,
        accent_color: "#7B8CDE".to_string(),
        created_at: now + 2_000,
        updated_at: now + 2_000,
    };
    save_starmap_meta(dir.path(), &non_empty).unwrap();
    let mut idx = load_index(dir.path()).unwrap();
    idx.starmap_ids.push(non_empty.starmap_id.clone());
    idx.updated_at = now_epoch();
    save_index(dir.path(), &idx).unwrap();

    // 给这个星图添加一个节点，使其非空白。
    let non_empty_graph = StarMapGraph {
        schema_version: CURRENT_GRAPH_SCHEMA_VERSION,
        starmap_id: non_empty.starmap_id.clone(),
        nodes: vec![StarMapNode {
            id: "n_real".to_string(),
            title: "Real Content".to_string(),
            kind: StarMapNodeKind::Character,
            payload: None,
            tags: Vec::new(),
            content: StarMapNodeContent::Empty,
            anchors: Vec::new(),
            portal: None,
            position: StarMapPoint::default(),
            style: StarMapNodeStyle::default(),
            provenance: StarMapProvenance::default(),
            created_at: now + 2_000,
            updated_at: now + 2_000,
        }],
        edges: vec![],
        embeds: vec![],
        links: vec![],
        hyperlinks: vec![],
    };
    save_graph(dir.path(), &non_empty.starmap_id, &non_empty_graph);

    // 运行迁移 — 不应迁移非空白星图。
    let migrated = safe_migrate_orphan_starmaps(dir.path()).unwrap();
    assert!(migrated.is_empty(), "Non-empty starmap should not migrate");

    // 验证非空白星图仍然出现在根列表中。
    let roots = list_root_starmaps(dir.path()).unwrap();
    assert!(
        roots.iter().any(|r| r.starmap_id == non_empty.starmap_id),
        "Non-empty starmap should still be in root list"
    );
}

#[test]
fn test_safe_migrate_does_not_touch_time_window_exceeded() {
    let dir = setup_temp_dir();

    let host = create_starmap(dir.path(), "Host", "", None).unwrap();
    let now = now_epoch();

    let host_graph = StarMapGraph {
        schema_version: CURRENT_GRAPH_SCHEMA_VERSION,
        starmap_id: host.starmap_id.clone(),
        nodes: vec![make_note_node_no_portal("n1", "Child", now)],
        edges: vec![],
        embeds: vec![],
        links: vec![],
        hyperlinks: vec![],
    };
    save_graph(dir.path(), &host.starmap_id, &host_graph);

    // 创建一个空白星图，标题匹配但创建时间远超窗口。
    let far_future = StarMapMeta {
        starmap_id: format!("sm_{}", uuid::Uuid::new_v4()),
        title: "Child".to_string(),
        description: String::new(),
        project_id: None,
        accent_color: "#7B8CDE".to_string(),
        created_at: now + 200_000, // 200 秒后，远超 60 秒窗口
        updated_at: now + 200_000,
    };
    save_starmap_meta(dir.path(), &far_future).unwrap();
    let mut idx = load_index(dir.path()).unwrap();
    idx.starmap_ids.push(far_future.starmap_id.clone());
    idx.updated_at = now_epoch();
    save_index(dir.path(), &idx).unwrap();

    // 运行迁移 — 不应迁移时间窗口外的星图。
    let migrated = safe_migrate_orphan_starmaps(dir.path()).unwrap();
    assert!(
        migrated.is_empty(),
        "Time-window-exceeded starmap should not migrate"
    );

    // 该星图仍然在根列表中。
    let roots = list_root_starmaps(dir.path()).unwrap();
    assert!(
        roots.iter().any(|r| r.starmap_id == far_future.starmap_id),
        "Time-window-exceeded starmap should still be in root list"
    );
}

#[test]
fn test_safe_migrate_idempotent() {
    let dir = setup_temp_dir();

    let host = create_starmap(dir.path(), "Host", "", None).unwrap();
    let now = now_epoch();

    let host_graph = StarMapGraph {
        schema_version: CURRENT_GRAPH_SCHEMA_VERSION,
        starmap_id: host.starmap_id.clone(),
        nodes: vec![make_note_node_no_portal("n1", "Child", now)],
        edges: vec![],
        embeds: vec![],
        links: vec![],
        hyperlinks: vec![],
    };
    save_graph(dir.path(), &host.starmap_id, &host_graph);

    let orphan = StarMapMeta {
        starmap_id: format!("sm_{}", uuid::Uuid::new_v4()),
        title: "Child".to_string(),
        description: String::new(),
        project_id: None,
        accent_color: "#7B8CDE".to_string(),
        created_at: now + 2_000,
        updated_at: now + 2_000,
    };
    save_starmap_meta(dir.path(), &orphan).unwrap();
    let mut idx = load_index(dir.path()).unwrap();
    idx.starmap_ids.push(orphan.starmap_id.clone());
    idx.updated_at = now_epoch();
    save_index(dir.path(), &idx).unwrap();

    // 第一次迁移。
    let migrated1 = safe_migrate_orphan_starmaps(dir.path()).unwrap();
    assert_eq!(migrated1.len(), 1);

    // 第二次迁移 — 不应再有孤儿。
    let migrated2 = safe_migrate_orphan_starmaps(dir.path()).unwrap();
    assert!(
        migrated2.is_empty(),
        "Second migration should find no orphans"
    );
}
