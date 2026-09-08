use crate::recent_edits::{
    flush_recent_edits, get_recent_edits, record_recent_edit, RecentEdit, RecentEditsCacheKey,
    RECENT_EDITS_CACHE,
};
use std::fs;
use tempfile::tempdir;

/// 在 `projects_root` 下写一份最小的 `chapter.meta.json`，让
/// `resolve_current_chapter_owner` 能把 `(project_id, volume_id, chapter_id)`
/// 识别为当前作品树中真实存在的章节。
fn write_chapter_meta(
    projects_root: &std::path::Path,
    project_id: &str,
    volume_id: &str,
    chapter_id: &str,
) {
    let dir = projects_root
        .join(project_id)
        .join("volumes")
        .join(volume_id)
        .join("chapters")
        .join(chapter_id);
    fs::create_dir_all(&dir).unwrap();
    let meta = serde_json::json!({
        "id": chapter_id,
        "title": chapter_id,
        "created_at": "2026-01-01T00:00:00Z",
        "updated_at": "2026-01-01T00:00:00Z",
        "order": 0,
        "word_count": 0u32,
        "hash": "",
    });
    fs::write(dir.join("chapter.meta.json"), meta.to_string()).unwrap();
}

#[test]
fn test_record_recent_edit_corrupt_json() {
    let dir = tempdir().unwrap();
    let app_data_root = dir.path();
    let projects_root = dir.path().join("projects");

    let recent_path = app_data_root.join("recent_edits.json");
    fs::write(&recent_path, "{ corrupted data ]}").unwrap();

    record_recent_edit(app_data_root, &projects_root, "p1", "v1", "c1").unwrap();

    let edits = get_recent_edits(app_data_root, &projects_root).unwrap();
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].project_id, "p1");
    assert_eq!(edits[0].volume_id, "v1");
    assert_eq!(edits[0].chapter_id, "c1");
}

#[test]
fn test_flush_recent_edits() {
    let dir = tempdir().unwrap();
    let app_data_root = dir.path();
    let projects_root = dir.path().join("projects");

    // Initially no cache, flushing shouldn't create file
    flush_recent_edits(app_data_root, &projects_root).unwrap();
    let recent_path = app_data_root.join("recent_edits.json");
    assert!(
        !recent_path.exists(),
        "Should not create file if no cached edits"
    );

    // Record edit (this populates cache and may write file depending on global debounce)
    record_recent_edit(app_data_root, &projects_root, "p1", "v1", "c1").unwrap();

    // Delete file to simulate unflushed cache, if the debounce happened to write it
    if recent_path.exists() {
        fs::remove_file(&recent_path).unwrap();
    }
    assert!(!recent_path.exists());

    // Flush edits to write cache back to file
    flush_recent_edits(app_data_root, &projects_root).unwrap();
    assert!(
        recent_path.exists(),
        "File should be re-created by flush_recent_edits"
    );

    // Verify content
    let content = fs::read_to_string(&recent_path).unwrap();
    let edits: Vec<RecentEdit> = serde_json::from_str(&content).unwrap();
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].project_id, "p1");
    assert_eq!(edits[0].volume_id, "v1");
    assert_eq!(edits[0].chapter_id, "c1");
}

#[test]
fn test_record_recent_edit_limit_20() {
    let dir = tempdir().unwrap();
    let app_data_root = dir.path();
    let projects_root = dir.path().join("projects");

    // 20 different projects, each with one chapter
    for i in 1..=20 {
        let project_id = format!("proj_{}", i);
        let chapter_id = format!("ch_{}", i);
        record_recent_edit(
            app_data_root,
            &projects_root,
            &project_id,
            "vol_1",
            &chapter_id,
        )
        .unwrap();
    }

    let edits = get_recent_edits(app_data_root, &projects_root).unwrap();
    assert_eq!(edits.len(), 20);
    // Most recent project is proj_20
    assert_eq!(edits[0].project_id, "proj_20");
    assert_eq!(edits[19].project_id, "proj_1");

    // Now edit proj_1's another chapter — should move to top, total still 20
    record_recent_edit(app_data_root, &projects_root, "proj_1", "vol_1", "ch_1b").unwrap();
    let edits = get_recent_edits(app_data_root, &projects_root).unwrap();
    assert_eq!(edits.len(), 20);
    assert_eq!(edits[0].project_id, "proj_1");
    assert_eq!(edits[0].chapter_id, "ch_1b");
}

/// #630 评论 5312333045 项1: 每个 project 只保留最后一次编辑的 chapter。
#[test]
fn test_record_recent_edit_per_project_dedup() {
    let dir = tempdir().unwrap();
    let app_data_root = dir.path();
    let projects_root = dir.path().join("projects");

    // Record two chapters for same project
    record_recent_edit(app_data_root, &projects_root, "p1", "v1", "c1").unwrap();
    record_recent_edit(app_data_root, &projects_root, "p1", "v1", "c2").unwrap();

    let edits = get_recent_edits(app_data_root, &projects_root).unwrap();
    assert_eq!(edits.len(), 1, "same project should only have one entry");
    assert_eq!(edits[0].project_id, "p1");
    assert_eq!(edits[0].chapter_id, "c2", "latest chapter should be kept");
}

/// #632: normalize_recent_edits 在加载旧数据时按当前作品树校正章节归属，
/// 并按校正后的 project_id 去重。
#[test]
fn test_normalize_on_load() {
    let dir = tempdir().unwrap();
    let app_data_root = dir.path();
    let projects_root = dir.path().join("projects");

    // 在当前作品树下，c1 和 c2 都属于 p1/v1
    write_chapter_meta(&projects_root, "p1", "v1", "c1");
    write_chapter_meta(&projects_root, "p1", "v1", "c2");

    // Write raw JSON with two entries for the same project
    let raw = serde_json::to_string_pretty(&vec![
        RecentEdit {
            project_id: "p1".to_string(),
            volume_id: "v1".to_string(),
            chapter_id: "c1".to_string(),
            timestamp: "2026-01-01T00:00:00Z".to_string(),
        },
        RecentEdit {
            project_id: "p1".to_string(),
            volume_id: "v1".to_string(),
            chapter_id: "c2".to_string(),
            timestamp: "2026-01-02T00:00:00Z".to_string(),
        },
    ])
    .unwrap();
    fs::write(app_data_root.join("recent_edits.json"), &raw).unwrap();

    let edits = get_recent_edits(app_data_root, &projects_root).unwrap();
    assert_eq!(edits.len(), 1, "normalize should deduplicate by project_id");
    assert_eq!(edits[0].chapter_id, "c2", "latest timestamp wins");
}

/// #630: normalize 保留不同项目的条目。
#[test]
fn test_normalize_keeps_different_projects() {
    let dir = tempdir().unwrap();
    let app_data_root = dir.path();
    let projects_root = dir.path().join("projects");

    write_chapter_meta(&projects_root, "p1", "v1", "c1");
    write_chapter_meta(&projects_root, "p2", "v2", "c3");

    let raw = serde_json::to_string_pretty(&vec![
        RecentEdit {
            project_id: "p1".to_string(),
            volume_id: "v1".to_string(),
            chapter_id: "c1".to_string(),
            timestamp: "2026-01-01T00:00:00Z".to_string(),
        },
        RecentEdit {
            project_id: "p2".to_string(),
            volume_id: "v2".to_string(),
            chapter_id: "c3".to_string(),
            timestamp: "2026-01-02T00:00:00Z".to_string(),
        },
    ])
    .unwrap();
    fs::write(app_data_root.join("recent_edits.json"), &raw).unwrap();

    let edits = get_recent_edits(app_data_root, &projects_root).unwrap();
    assert_eq!(edits.len(), 2, "different projects should both be kept");
}

/// #632: 磁盘里残留的 alias（同一章节被记成两个不同 project_id）在读取时
/// 被当前作品树校正为同一个 canonical project_id，再按 project 去重。
#[test]
fn test_normalize_collapses_alias_via_current_tree() {
    let dir = tempdir().unwrap();
    let app_data_root = dir.path();
    let projects_root = dir.path().join("projects");

    // 真实树里章节 c1 属于 p_real/v_real
    write_chapter_meta(&projects_root, "p_real", "v_real", "c1");

    // 磁盘里残留两条 alias：一条指向不存在的 p_alias，一条指向真实 p_real
    let raw = serde_json::to_string_pretty(&vec![
        RecentEdit {
            project_id: "p_alias".to_string(),
            volume_id: "v_alias".to_string(),
            chapter_id: "c1".to_string(),
            timestamp: "2026-01-01T00:00:00Z".to_string(),
        },
        RecentEdit {
            project_id: "p_real".to_string(),
            volume_id: "v_real".to_string(),
            chapter_id: "c1".to_string(),
            timestamp: "2026-01-02T00:00:00Z".to_string(),
        },
    ])
    .unwrap();
    fs::write(app_data_root.join("recent_edits.json"), &raw).unwrap();

    let edits = get_recent_edits(app_data_root, &projects_root).unwrap();
    assert_eq!(
        edits.len(),
        1,
        "alias should collapse to canonical project_id"
    );
    assert_eq!(edits[0].project_id, "p_real");
    assert_eq!(edits[0].volume_id, "v_real");
    assert_eq!(edits[0].chapter_id, "c1");
}

/// #632: 章节已不存在时，normalize 直接丢弃该条目。
#[test]
fn test_normalize_drops_orphan_chapter() {
    let dir = tempdir().unwrap();
    let app_data_root = dir.path();
    let projects_root = dir.path().join("projects");

    // 只在树里放 c1，不放 c_orphan
    write_chapter_meta(&projects_root, "p1", "v1", "c1");

    let raw = serde_json::to_string_pretty(&vec![
        RecentEdit {
            project_id: "p1".to_string(),
            volume_id: "v1".to_string(),
            chapter_id: "c1".to_string(),
            timestamp: "2026-01-01T00:00:00Z".to_string(),
        },
        RecentEdit {
            project_id: "p_orphan".to_string(),
            volume_id: "v_orphan".to_string(),
            chapter_id: "c_orphan".to_string(),
            timestamp: "2026-01-02T00:00:00Z".to_string(),
        },
    ])
    .unwrap();
    fs::write(app_data_root.join("recent_edits.json"), &raw).unwrap();

    let edits = get_recent_edits(app_data_root, &projects_root).unwrap();
    assert_eq!(edits.len(), 1, "orphan chapter should be dropped");
    assert_eq!(edits[0].chapter_id, "c1");
}

#[test]
fn test_flush_recent_edits_manual_cache() {
    let dir = tempdir().unwrap();
    let app_data_root = dir.path();
    let projects_root = dir.path().join("projects");

    // Populate in-memory cache
    {
        let edits = get_recent_edits(app_data_root, &projects_root).unwrap(); // initializes cache
        assert!(edits.is_empty());
    }

    {
        let mutex = RECENT_EDITS_CACHE.get().unwrap();
        let mut cache = mutex.lock().unwrap();
        cache.insert(
            RecentEditsCacheKey {
                app_data_root: app_data_root.to_path_buf(),
                projects_root: projects_root.to_path_buf(),
            },
            vec![RecentEdit {
                project_id: "proj_1".to_string(),
                volume_id: "vol_1".to_string(),
                chapter_id: "ch_1".to_string(),
                timestamp: chrono::Utc::now().to_rfc3339(),
            }],
        );
    }

    let recent_path = app_data_root.join("recent_edits.json");
    if recent_path.exists() {
        fs::remove_file(&recent_path).unwrap();
    }

    // Flush cache to disk
    flush_recent_edits(app_data_root, &projects_root).unwrap();

    // Assert file exists
    assert!(recent_path.exists());

    // Verify contents
    let edits = get_recent_edits(app_data_root, &projects_root).unwrap();
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].project_id, "proj_1");
    assert_eq!(edits[0].volume_id, "vol_1");
    assert_eq!(edits[0].chapter_id, "ch_1");
}

#[test]
fn test_flush_recent_edits_empty_cache() {
    let dir = tempdir().unwrap();
    let app_data_root = dir.path();
    let projects_root = dir.path().join("projects");

    let recent_path = app_data_root.join("recent_edits.json");
    assert!(!recent_path.exists());

    // Flushing when cache has nothing for this app data root shouldn't fail
    flush_recent_edits(app_data_root, &projects_root).unwrap();

    // And it shouldn't create the file if it had no data
    assert!(!recent_path.exists());
}

#[test]
fn test_flush_recent_edits_creates_directory() {
    let dir = tempdir().unwrap();
    let app_data_root = dir.path();
    let projects_root = dir.path().join("projects");

    let recent_path = app_data_root.join("recent_edits.json");
    assert!(!recent_path.exists());

    // Populate in-memory cache directly
    {
        let _ = get_recent_edits(app_data_root, &projects_root); // initializes cache
        let mutex = RECENT_EDITS_CACHE.get().unwrap();
        let mut cache = mutex.lock().unwrap();
        cache.insert(
            RecentEditsCacheKey {
                app_data_root: app_data_root.to_path_buf(),
                projects_root: projects_root.to_path_buf(),
            },
            vec![RecentEdit {
                project_id: "proj_1".to_string(),
                volume_id: "vol_1".to_string(),
                chapter_id: "ch_1".to_string(),
                timestamp: chrono::Utc::now().to_rfc3339(),
            }],
        );
    }

    assert!(!recent_path.exists());

    // Now flush cache to disk. It should create the missing directories.
    flush_recent_edits(app_data_root, &projects_root).unwrap();

    // Assert file exists
    assert!(recent_path.exists());

    // Verify contents from memory cache
    let edits = get_recent_edits(app_data_root, &projects_root).unwrap();
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].project_id, "proj_1");
    assert_eq!(edits[0].volume_id, "vol_1");
    assert_eq!(edits[0].chapter_id, "ch_1");

    // Verify that flush actually wrote the expected content to disk
    let file_content = fs::read_to_string(&recent_path).unwrap();
    let parsed_edits: Vec<serde_json::Value> = serde_json::from_str(&file_content).unwrap();
    assert_eq!(parsed_edits.len(), 1);
    assert_eq!(parsed_edits[0]["project_id"], "proj_1");
    assert_eq!(parsed_edits[0]["volume_id"], "vol_1");
    assert_eq!(parsed_edits[0]["chapter_id"], "ch_1");
    assert!(parsed_edits[0]["timestamp"].is_string());
}
