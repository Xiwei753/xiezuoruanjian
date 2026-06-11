use criterion::{criterion_group, criterion_main, Criterion};
use std::fs;
use std::path::Path;
use uuid::Uuid;
use writer_core::project::{create_project, list_projects};
use tempfile::tempdir;

fn bench_list_projects(c: &mut Criterion) {
    let dir = tempdir().unwrap();
    let workspace_path = dir.path();

    // Create 100 dummy projects
    for i in 0..100 {
        create_project(workspace_path, &format!("Project {}", i)).unwrap();
    }

    c.bench_function("list_projects_100", |b| {
        b.iter(|| {
            list_projects(workspace_path).unwrap()
        })
    });
}

criterion_group!(benches, bench_list_projects);
criterion_main!(benches);
