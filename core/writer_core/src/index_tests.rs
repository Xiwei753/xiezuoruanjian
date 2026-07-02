use std::fs;
use std::path::Path;
use crate::index::{SearchIndex, SearchOptions};
use tempfile::tempdir;

fn setup_test_workspace(dir: &Path) {
    crate::workspace::create_workspace(dir).unwrap();
    let projects_dir = dir.join("projects");
    fs::create_dir_all(&projects_dir).unwrap();

    // Create a test project with chapters
    let proj_id = "proj1";
    let vol_id = "vol1";
    let ch_id = "ch1";

    let ch_dir = projects_dir
        .join(proj_id)
        .join("volumes")
        .join(vol_id)
        .join("chapters")
        .join(ch_id);
    fs::create_dir_all(&ch_dir).unwrap();

    fs::write(
        ch_dir.join("chapter.md"),
        "????????\n???????\n???????\n??????\n????????",
    )
    .unwrap();

    fs::write(
        ch_dir.join("chapter.meta.json"),
        r#"{"id": "ch1", "title": "??? ??", "created_at": 0, "updated_at": 0}"#,
    )
    .unwrap();

    // Second chapter
    let ch2_id = "ch2";
    let ch2_dir = projects_dir
        .join(proj_id)
        .join("volumes")
        .join(vol_id)
        .join("chapters")
        .join(ch2_id);
    fs::create_dir_all(&ch2_dir).unwrap();

    fs::write(
        ch2_dir.join("chapter.md"),
        "??????\n???????\n?????????\n????????",
    )
    .unwrap();

    fs::write(
        ch2_dir.join("chapter.meta.json"),
        r#"{"id": "ch2", "title": "??? ??", "created_at": 0, "updated_at": 0}"#,
    )
    .unwrap();
}

#[test]
fn test_build_index_and_search() {
    let dir = tempdir().unwrap();
    setup_test_workspace(dir.path());

    let index = SearchIndex::build(dir.path()).unwrap();
    let stats = index.stats();
    assert_eq!(stats.chapter_count, 2);

    let options = SearchOptions::default();
    let hits = index.search("??", &options);
    assert_eq!(hits.len(), 2); // line 2 ch1, line 2 ch2
}

#[test]
fn test_search_case_insensitive() {
    let dir = tempdir().unwrap();
    setup_test_workspace(dir.path());

    let index = SearchIndex::build(dir.path()).unwrap();
    let options = SearchOptions::default();
    let hits = index.search("??", &options);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].chapter_title, "??? ??");
    assert_eq!(hits[0].line_number, 2);
}

#[test]
fn test_search_with_context() {
    let dir = tempdir().unwrap();
    setup_test_workspace(dir.path());

    let index = SearchIndex::build(dir.path()).unwrap();
    let options = SearchOptions {
        context_lines: 1,
        ..Default::default()
    };
    let hits = index.search("??", &options);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].context_before.len(), 1);
    assert_eq!(hits[0].context_before[0], "???????");
    assert_eq!(hits[0].context_after.len(), 1);
    assert_eq!(hits[0].context_after[0], "??????");
}

#[test]
fn test_search_max_results() {
    let dir = tempdir().unwrap();
    setup_test_workspace(dir.path());

    let index = SearchIndex::build(dir.path()).unwrap();
    let options = SearchOptions {
        max_results: 1,
        ..Default::default()
    };
    let hits = index.search("??", &options);
    assert_eq!(hits.len(), 1);
}

#[test]
fn test_search_in_project() {
    let dir = tempdir().unwrap();
    setup_test_workspace(dir.path());

    let index = SearchIndex::build(dir.path()).unwrap();
    let options = SearchOptions::default();
    let hits = index.search_in_project("proj1", "??", &options);
    assert_eq!(hits.len(), 2);

    let hits_empty = index.search_in_project("nonexistent", "??", &options);
    assert_eq!(hits_empty.len(), 0);
}

#[test]
fn test_search_empty_query() {
    let dir = tempdir().unwrap();
    setup_test_workspace(dir.path());

    let index = SearchIndex::build(dir.path()).unwrap();
    let options = SearchOptions::default();
    let hits = index.search("", &options);
    assert_eq!(hits.len(), 0);
}

#[test]
fn test_empty_workspace() {
    let dir = tempdir().unwrap();
    crate::workspace::create_workspace(dir.path()).unwrap();

    let index = SearchIndex::build(dir.path()).unwrap();
    let stats = index.stats();
    assert_eq!(stats.chapter_count, 0);

    let options = SearchOptions::default();
    let hits = index.search("test", &options);
    assert_eq!(hits.len(), 0);
}


#[test]
fn test_build_missing_projects_dir() {
    let dir = tempdir().unwrap();
    // Skip creating the workspace, so "projects" dir is missing.
    let index = SearchIndex::build(dir.path()).unwrap();
    assert_eq!(index.stats().chapter_count, 0);
}

#[test]
fn test_build_missing_subdirectories() {
    let dir = tempdir().unwrap();
    let projects_dir = dir.path().join("projects");
    fs::create_dir_all(&projects_dir).unwrap();

    // Project without volumes dir
    let proj_no_volumes = projects_dir.join("proj_no_volumes");
    fs::create_dir_all(&proj_no_volumes).unwrap();

    // Project with volumes but no chapters dir
    let proj_no_chapters = projects_dir.join("proj_no_chapters");
    let vol_no_chapters = proj_no_chapters.join("volumes").join("vol1");
    fs::create_dir_all(&vol_no_chapters).unwrap();

    // Project with chapters but no chapter.md
    let proj_no_md = projects_dir.join("proj_no_md");
    let ch_no_md = proj_no_md.join("volumes").join("vol1").join("chapters").join("ch1");
    fs::create_dir_all(&ch_no_md).unwrap();

    let index = SearchIndex::build(dir.path()).unwrap();
    assert_eq!(index.stats().chapter_count, 0);
}

#[test]
fn test_build_invalid_utf8_chapter() {
    let dir = tempdir().unwrap();
    let projects_dir = dir.path().join("projects");
    let ch_dir = projects_dir.join("proj1").join("volumes").join("vol1").join("chapters").join("ch1");
    fs::create_dir_all(&ch_dir).unwrap();

    // Write invalid UTF-8 bytes to trigger `.ok()?` failure in read_to_string
    fs::write(ch_dir.join("chapter.md"), b"\xFF\xFE\xFD").unwrap();

    let index = SearchIndex::build(dir.path()).unwrap();
    assert_eq!(index.stats().chapter_count, 0);
}

#[test]
fn test_build_happy_path() {
    let dir = tempdir().unwrap();
    setup_test_workspace(dir.path());

    let index = SearchIndex::build(dir.path()).unwrap();
    let stats = index.stats();

    // Verify stats
    assert_eq!(stats.chapter_count, 2);
    assert!(stats.total_lines > 0);
    assert!(stats.total_words > 0);

    // Verify index entries metadata contents
    assert_eq!(index.entries.len(), 2);

    let ch1 = index.entries.iter().find(|e| e.chapter_id == "ch1").unwrap();
    assert_eq!(ch1.project_id, "proj1");
    assert_eq!(ch1.volume_id, "vol1");
    assert_eq!(ch1.chapter_title, "??? ??");
    assert_eq!(ch1.relative_path, "projects/proj1/volumes/vol1/chapters/ch1/chapter.md");
}
