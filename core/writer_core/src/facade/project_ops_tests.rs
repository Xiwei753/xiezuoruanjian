use super::*;
use tempfile::tempdir;

#[test]
fn test_project_lifecycle() {
    let temp_dir = tempdir().unwrap();
    let core = WriterCore::new(temp_dir.path());
    core.create_workspace().unwrap();

    let p1 = core.create_project("Project 1").unwrap();
    let p2 = core.create_project("Project 2").unwrap();

    let projects = core.list_projects().unwrap();
    assert_eq!(projects.len(), 2);

    core.rename_project(&p1.id, "Project 1 Renamed").unwrap();
    let projects = core.list_projects().unwrap();
    assert_eq!(
        projects.iter().find(|p| p.id == p1.id).unwrap().title,
        "Project 1 Renamed"
    );

    core.reorder_projects(&[p2.id.clone(), p1.id.clone()])
        .unwrap();
    let projects = core.list_projects().unwrap();
    assert_eq!(projects[0].id, p2.id);
    assert_eq!(projects[1].id, p1.id);

    core.delete_project(&p2.id).unwrap();
    let projects = core.list_projects().unwrap();
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].id, p1.id);
}

#[test]
fn test_volume_lifecycle() {
    let temp_dir = tempdir().unwrap();
    let core = WriterCore::new(temp_dir.path());
    core.create_workspace().unwrap();

    let p1 = core.create_project("Project 1").unwrap();
    let v1 = core.create_volume(&p1.id, "Volume 1").unwrap();
    let v2 = core.create_volume(&p1.id, "Volume 2").unwrap();

    let volumes = core.list_volumes(&p1.id).unwrap();
    // Default volume + v1 + v2
    assert_eq!(volumes.len(), 3);

    core.rename_volume(&p1.id, &v1.id, "Volume 1 Renamed")
        .unwrap();
    let volumes = core.list_volumes(&p1.id).unwrap();
    assert_eq!(
        volumes.iter().find(|v| v.id == v1.id).unwrap().title,
        "Volume 1 Renamed"
    );

    core.reorder_volumes(&p1.id, &[volumes[0].id.clone(), v2.id.clone(), v1.id.clone()])
        .unwrap();
    let volumes = core.list_volumes(&p1.id).unwrap();
    assert_eq!(volumes[1].id, v2.id);

    core.delete_volume(&p1.id, &v2.id).unwrap();
    let volumes = core.list_volumes(&p1.id).unwrap();
    assert_eq!(volumes.len(), 2);
}

#[test]
fn test_chapter_lifecycle() {
    let temp_dir = tempdir().unwrap();
    let core = WriterCore::new(temp_dir.path());
    core.create_workspace().unwrap();

    let p1 = core.create_project("Project 1").unwrap();
    let v1 = core.create_volume(&p1.id, "Volume 1").unwrap();

    let c1 = core.create_chapter(&p1.id, &v1.id, "Chapter 1").unwrap();
    let c2 = core.create_chapter(&p1.id, &v1.id, "Chapter 2").unwrap();

    let chapters = core.list_chapters(&p1.id, &v1.id).unwrap();
    assert_eq!(chapters.len(), 2);

    core.rename_chapter(&p1.id, &v1.id, &c1.id, "Chapter 1 Renamed")
        .unwrap();
    let chapters = core.list_chapters(&p1.id, &v1.id).unwrap();
    assert_eq!(
        chapters.iter().find(|c| c.id == c1.id).unwrap().title,
        "Chapter 1 Renamed"
    );

    core.reorder_chapters(&p1.id, &v1.id, &[c2.id.clone(), c1.id.clone()])
        .unwrap();
    let chapters = core.list_chapters(&p1.id, &v1.id).unwrap();
    assert_eq!(chapters[0].id, c2.id);
    assert_eq!(chapters[1].id, c1.id);

    core.delete_chapter(&p1.id, &v1.id, &c2.id).unwrap();
    let chapters = core.list_chapters(&p1.id, &v1.id).unwrap();
    assert_eq!(chapters.len(), 1);

    let valid_ids = core.list_valid_chapter_ids(&p1.id).unwrap();
    assert!(valid_ids.contains(&c1.id));
    assert!(!valid_ids.contains(&c2.id));
}

#[test]
fn test_chapter_content_operations() {
    let temp_dir = tempdir().unwrap();
    let core = WriterCore::new(temp_dir.path());
    core.create_workspace().unwrap();

    let p1 = core.create_project("Project 1").unwrap();
    let v1 = core.create_volume(&p1.id, "Volume 1").unwrap();
    let c1 = core.create_chapter(&p1.id, &v1.id, "Chapter 1").unwrap();

    assert_eq!(core.calculate_word_count("hello world"), 10);

    core.write_chapter(&p1.id, &v1.id, &c1.id, "First content")
        .unwrap();
    let content = core.read_chapter(&p1.id, &v1.id, &c1.id).unwrap();
    assert_eq!(content.content, "First content");

    let open_result = core.open_chapter(&p1.id, &v1.id, &c1.id).unwrap();
    assert_eq!(open_result.content, "First content");
    assert_eq!(open_result.meta.id, c1.id);

    core.update_chapter_note(&p1.id, &v1.id, &c1.id, "My note")
        .unwrap();
    let open_result_with_note = core.open_chapter(&p1.id, &v1.id, &c1.id).unwrap();
    assert_eq!(open_result_with_note.meta.note, Some("My note".to_string()));

    core.clear_chapter_content(&p1.id, &v1.id, &c1.id).unwrap();
    let content_cleared = core.read_chapter(&p1.id, &v1.id, &c1.id).unwrap();
    assert_eq!(content_cleared.content, "");

    let receipt1 = core
        .write_chapter_verified(&p1.id, &v1.id, &c1.id, "Second content")
        .unwrap();
    assert_eq!(receipt1.word_count, 13);

    let receipt2 = core
        .write_chapter_verified_with_allow_empty_overwrite(&p1.id, &v1.id, &c1.id, "", true)
        .unwrap();
    assert_eq!(receipt2.word_count, 0);

    let receipt3 = core
        .clear_chapter_content_verified(&p1.id, &v1.id, &c1.id)
        .unwrap();
    assert_eq!(receipt3.word_count, 0);
}

#[test]
fn test_aggregated_stats_and_time() {
    let temp_dir = tempdir().unwrap();
    let core = WriterCore::new(temp_dir.path());
    core.create_workspace().unwrap();

    let p1 = core.create_project("Project 1").unwrap();
    let v1 = core.create_volume(&p1.id, "Volume 1").unwrap();
    let c1 = core.create_chapter(&p1.id, &v1.id, "Chapter 1").unwrap();

    core.write_chapter(&p1.id, &v1.id, &c1.id, "Hello world")
        .unwrap();

    let vol_updated = core
        .get_volume_updated_at_aggregated(&p1.id, &v1.id)
        .unwrap();
    assert!(!vol_updated.is_empty());

    let proj_updated = core.get_project_updated_at_aggregated(&p1.id).unwrap();
    assert!(!proj_updated.is_empty());

    let stats = core.get_project_stats(&p1.id).unwrap();
    assert_eq!(stats.total_word_count, 10);
}
