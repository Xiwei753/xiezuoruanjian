use criterion::{black_box, criterion_group, criterion_main, Criterion};
use tempfile::tempdir;
use std::fs;
use writer_core::index::{SearchIndex, SearchOptions};

fn setup_large_workspace(dir: &std::path::Path) {
    writer_core::workspace::create_workspace(dir).unwrap();
    let projects_dir = dir.join("projects");
    fs::create_dir_all(&projects_dir).unwrap();

    let proj_id = "proj_bench";
    let vol_id = "vol_bench";

    // Create 100 chapters with 100 lines each
    for i in 0..100 {
        let ch_id = format!("ch_{}", i);
        let ch_dir = projects_dir
            .join(proj_id)
            .join("volumes")
            .join(vol_id)
            .join("chapters")
            .join(&ch_id);
        fs::create_dir_all(&ch_dir).unwrap();

        let mut content = String::new();
        for j in 0..100 {
            if j == 50 && i % 10 == 0 {
                content.push_str("这是一个包含目标关键字的行\n");
            } else {
                content.push_str(&format!("这是第 {} 章的第 {} 行普通的文本内容\n", i, j));
            }
        }

        fs::write(ch_dir.join("chapter.md"), content).unwrap();
        fs::write(
            ch_dir.join("chapter.meta.json"),
            format!(r#"{{"id": "{}", "title": "Chapter {}", "created_at": 0, "updated_at": 0}}"#, ch_id, i),
        ).unwrap();
    }
}

fn bench_search(c: &mut Criterion) {
    let dir = tempdir().unwrap();
    setup_large_workspace(dir.path());
    let index = SearchIndex::build(dir.path()).unwrap();

    let mut group = c.benchmark_group("Search");

    group.bench_function("case_sensitive", |b| {
        let options = SearchOptions {
            case_sensitive: true,
            ..Default::default()
        };
        b.iter(|| {
            let hits = index.search(black_box("目标关键字"), black_box(&options));
            black_box(hits);
        })
    });

    group.bench_function("case_insensitive", |b| {
        let options = SearchOptions {
            case_sensitive: false,
            ..Default::default()
        };
        b.iter(|| {
            let hits = index.search(black_box("目标关键字"), black_box(&options));
            black_box(hits);
        })
    });

    group.finish();
}

criterion_group!(benches, bench_search);
criterion_main!(benches);
