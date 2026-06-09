use criterion::{black_box, criterion_group, criterion_main, Criterion};
use tempfile::tempdir;
use writer_core::facade::WriterCore;
use writer_core::mind_map::graph::{MindMapGraph, MindMapGraphNode};
use writer_core::mind_map::validation::validate_graph;

fn criterion_benchmark(c: &mut Criterion) {
    let temp_dir = tempdir().unwrap();
    writer_core::workspace::create_workspace(temp_dir.path()).unwrap();
    let core = WriterCore::new(temp_dir.path());
    let proj = core.create_project("Bench Project").unwrap();

    // Create 5 volumes with 100 chapters each (500 chapters total) to simulate a large project
    for v in 0..5 {
        let vol = core.create_volume(&proj.id, &format!("Vol {}", v)).unwrap();
        for c in 0..100 {
            core.create_chapter(&proj.id, &vol.id, &format!("Chap {}", c)).unwrap();
        }
    }

    let graph = MindMapGraph {
        schema_version: 2,
        id: "g1".into(),
        project_id: proj.id.clone(),
        title: "Test".into(),
        nodes: vec![MindMapGraphNode {
            id: "n1".into(),
            title: "Node 1".into(),
            kind: writer_core::mind_map::graph::MindMapNodeKind::Note,
            payload: None,
            tags: vec![],
            created_at: 0,
            updated_at: 0,
        }],
        edges: vec![],
        anchors: vec![],
        links: vec![],
        created_at: 0,
        updated_at: 0,
    };

    c.bench_function("validate_graph", |b| {
        b.iter(|| validate_graph(black_box(&graph), black_box(&core)).unwrap())
    });
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
