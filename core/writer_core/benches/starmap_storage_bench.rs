#![allow(clippy::unwrap_used)]
//! 星图包全量加载基准。
//!
//! 旧实现手写 `workspace/app-meta/starmaps/<id>/nodes/<id>.json`，既不是真实
//! 路径（真实布局是 `workspace/starmaps/<id>/nodes/<bucket>/<id>.json`），
//! 也不是当前 schema（`GraphMeta::CURRENT_SCHEMA_VERSION == "4"`，旧基准写
//! `"2"`；节点还缺 `asset_id` / `definition_revision`，且已经没有 `tags`）。
//!
//! 现在全部通过公开 API 造真实数据：
//! `create_starmap` 建画布，`library::save_node_definition` 写共享本体，
//! `StarMapStore::set_node_asset` + `upsert_node` 建画布实例，
//! `StarMapStore::flush` 落盘实例与 `graph.json`，
//! `library::save_reference_index` 登记实例到本体的反向引用。
//! 数据准备全部在计时区外；计时闭包只新建 reader 并做 `load_full`。

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use std::fs;
use std::path::Path;
use tempfile::tempdir;
use writer_core::starmap::create_starmap;
use writer_core::starmap::library::{save_node_definition, save_reference_index};
use writer_core::starmap::semantic::{StarMapNodeContent, StarMapProvenance};
use writer_core::starmap::store::meta::CURRENT_SCHEMA_VERSION;
use writer_core::starmap::store::{GraphMeta, StarMapStore};
use writer_core::starmap::types::{
    StarMapNodeDefinition, StarMapNodeKind, StarMapNodeReference, StarMapNodeStyle, StarMapPoint,
    StarMapReferenceIndex,
};

const NODE_COUNT: usize = 1000;

/// 固定时间戳（毫秒，与 `now_epoch` 同单位），让种子数据可重复；不影响加载路径。
const FIXED_TIMESTAMP_MS: u64 = 1_717_800_000_000;

/// 用公开 API 生成 1000 个当前 schema 的真实节点，返回星图 ID。
fn seed_starmap(workspace: &Path) -> String {
    let meta = create_starmap(workspace, "Bench", "", None).expect("create bench starmap");
    let starmap_id = meta.starmap_id;
    let mut store = StarMapStore::new(workspace, &starmap_id);
    let mut references = StarMapReferenceIndex::default();

    for index in 0..NODE_COUNT {
        let instance_id = format!("node_{index:04}");
        let asset_id = format!("asset_{index:04}");

        // 共享标题/内容只存素材库本体，画布实例只引用 asset_id。
        let definition = StarMapNodeDefinition {
            asset_id: asset_id.clone(),
            title: format!("Node {index}"),
            kind: StarMapNodeKind::Character,
            payload: None,
            content: StarMapNodeContent::Empty,
            anchors: Vec::new(),
            portal: None,
            style: StarMapNodeStyle::default(),
            provenance: StarMapProvenance::default(),
            created_at: FIXED_TIMESTAMP_MS,
            updated_at: FIXED_TIMESTAMP_MS,
        };
        save_node_definition(workspace, &definition).expect("seed shared node definition");

        let position = StarMapPoint {
            x: (index % 40) as f64 * 48.0,
            y: (index / 40) as f64 * 48.0,
        };
        store.set_node_asset(&instance_id, &asset_id);
        store.upsert_node(definition.instantiate(&instance_id, position));

        references.nodes.push(StarMapNodeReference {
            asset_id,
            starmap_id: starmap_id.clone(),
            instance_id,
            project_id: None,
        });
    }

    // flush 写节点实例（nodes/<bucket>/<id>.json）与 graph.json（当前 schema 4）。
    store.flush().expect("flush seeded starmap package");
    // 反向引用是节点身份解码的一部分：先落盘，加载时就不必在计时区补索引。
    save_reference_index(workspace, &references).expect("seed node reference index");

    // 落盘后校验种子包确实是当前 schema 且成员完整，防止基准悄悄跑在旧格式上。
    let graph_json = workspace
        .join("starmaps")
        .join(&starmap_id)
        .join("graph.json");
    let graph_meta: GraphMeta =
        serde_json::from_str(&fs::read_to_string(&graph_json).expect("read seeded graph.json"))
            .expect("parse seeded graph.json");
    assert_eq!(graph_meta.schema_version, CURRENT_SCHEMA_VERSION);
    assert_eq!(graph_meta.node_ids.len(), NODE_COUNT);

    starmap_id
}

fn bench_load_starmap(c: &mut Criterion) {
    let dir = tempdir().unwrap();
    let workspace = dir.path();
    let starmap_id = seed_starmap(workspace);

    // 预热一次全量加载：同时确认种子数据能被当前 reader 读通、不触发迁移写盘。
    let mut warmup = StarMapStore::new(workspace, &starmap_id);
    let warmup_result = warmup.load_full().expect("warm-up load of seeded starmap");
    assert_eq!(warmup_result.loaded_node_count, NODE_COUNT);
    black_box(warmup_result);

    c.bench_function("load_starmap_1000_nodes", |b| {
        b.iter(|| {
            // 每轮新建 reader，测的是冷启动全量加载，不是复用缓存。
            let mut store = StarMapStore::new(workspace, &starmap_id);
            let result = store.load_full().expect("load seeded 1000-node starmap");
            assert_eq!(result.loaded_node_count, NODE_COUNT);
            black_box(result);
        });
    });
}

criterion_group!(benches, bench_load_starmap);
criterion_main!(benches);
