#![allow(clippy::unwrap_used)]
use criterion::{criterion_group, criterion_main, Criterion};
use std::fs;
use tempfile::tempdir;
use writer_core::starmap::store::{GraphMeta, StarMapStore};
use writer_core::starmap::semantic::{
    StarMapDisplayPolicy, StarMapNodeContent, StarMapOpenBehavior, StarMapProvenance,
};
use writer_core::starmap::types::*;

fn bench_load_starmap(c: &mut Criterion) {
    let dir = tempdir().unwrap();
    let workspace = dir.path();
    let meta = writer_core::starmap::create_starmap(workspace, "Bench", "", None).unwrap();
    let real_id = meta.starmap_id.clone();
    let starmap_dir = workspace.join("app-meta").join("starmaps").join(&real_id);

    let gmeta = GraphMeta {
        schema_version: "2".to_string(),
        starmap_id: real_id.clone(),
        title: "Bench".to_string(),
        node_ids: (0..1000).map(|i| format!("node_{}", i)).collect(),
        edge_ids: vec![],
        embed_instance_ids: vec![],
        link_ids: vec![],
        hyperlink_ids: vec![],
        edge_relation_index: vec![],
        embed_host_index: vec![],
        link_relation_index: vec![],
        hyperlink_relation_index: vec![],
        node_kind_counts: std::collections::HashMap::new(),
        package_revision: 0,
        updated_at: 0,
        deleted_since_last_sync: writer_core::starmap::store::DeletedSinceLastSync::default(),
    };
    fs::create_dir_all(&starmap_dir).unwrap();
    fs::write(
        starmap_dir.join("graph.json"),
        serde_json::to_string(&gmeta).unwrap(),
    )
    .unwrap();

    let nodes_dir = starmap_dir.join("nodes");
    fs::create_dir_all(&nodes_dir).unwrap();
    for i in 0..1000 {
        let node = StarMapNode {
            id: format!("node_{}", i),
            title: format!("Node {}", i),
            kind: StarMapNodeKind::Character,
            payload: None,
            tags: vec![],
            content: StarMapNodeContent::Empty,
            anchors: vec![],
            portal: None,
            display_policy: StarMapDisplayPolicy::default(),
            open_behavior: StarMapOpenBehavior::default(),
            provenance: StarMapProvenance::default(),
            created_at: 0,
            updated_at: 0,
        };
        let path = nodes_dir.join(format!("{}.json", node.id));
        fs::write(&path, serde_json::to_string(&node).unwrap()).unwrap();
    }

    c.bench_function("load_starmap_1000_nodes_baseline", |b| {
        b.iter(|| {
            let mut store = StarMapStore::new(workspace, &real_id);
            let _ = store.load_full();
        })
    });
}

criterion_group!(benches, bench_load_starmap);
criterion_main!(benches);
