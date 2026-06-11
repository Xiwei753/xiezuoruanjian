use criterion::{black_box, criterion_group, criterion_main, Criterion};
use tempfile::tempdir;
use std::fs;
use writer_core::chapter::list_chapters;

fn setup_large_workspace(dir: &std::path::Path) -> (String, String) {
    writer_core::workspace::create_workspace(dir).unwrap();
    let projects_dir = dir.join("projects");
    fs::create_dir_all(&projects_dir).unwrap();

    let proj_id = "proj_bench";
    let vol_id = "vol_bench";

    // Create 1000 chapters
    for i in 0..1000 {
        let ch_id = format!("ch_{}", i);
        let ch_dir = projects_dir
            .join(proj_id)
            .join("volumes")
            .join(vol_id)
            .join("chapters")
            .join(&ch_id);
        fs::create_dir_all(&ch_dir).unwrap();

        fs::write(
            ch_dir.join("chapter.meta.json"),
            format!(r#"{{"id": "{}", "title": "Chapter {}", "created_at": "0", "updated_at": "0", "word_count": 0, "hash": ""}}"#, ch_id, i),
        ).unwrap();
    }

    (proj_id.to_string(), vol_id.to_string())
}

fn bench_list_chapters(c: &mut Criterion) {
    let dir = tempdir().unwrap();
    let (proj_id, vol_id) = setup_large_workspace(dir.path());

    c.bench_function("list_chapters", |b| {
        b.iter(|| {
            let chapters = list_chapters(black_box(dir.path()), black_box(&proj_id), black_box(&vol_id)).unwrap();
            black_box(chapters);
        })
    });
}

criterion_group!(benches, bench_list_chapters);
criterion_main!(benches);
