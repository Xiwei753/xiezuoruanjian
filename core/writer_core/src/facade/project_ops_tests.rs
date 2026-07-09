use crate::facade::WriterCore;
use tempfile::tempdir;

#[test]
fn test_project_crud_operations() {
    let temp_dir = tempdir().unwrap();
    let core = WriterCore::new(temp_dir.path());
    core.create_workspace().unwrap();

    let p1 = core.create_project("Project 1").unwrap();
    let p2 = core.create_project("Project 2").unwrap();

    let projects = core.list_projects().unwrap();
    assert_eq!(projects.len(), 2);

    core.rename_project(&p1.id, "Project 1 Renamed").unwrap();
    let projects = core.list_projects().unwrap();
    assert_eq!(projects.iter().find(|p| p.id == p1.id).unwrap().title, "Project 1 Renamed");

    core.reorder_projects(&[p2.id.clone(), p1.id.clone()]).unwrap();
    let projects = core.list_projects().unwrap();
    assert_eq!(projects[0].id, p2.id);
    assert_eq!(projects[1].id, p1.id);

    core.delete_project(&p2.id).unwrap();
    let projects = core.list_projects().unwrap();
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].id, p1.id);
}

#[test]
fn test_volume_crud_operations() {
    let temp_dir = tempdir().unwrap();
    let core = WriterCore::new(temp_dir.path());
    core.create_workspace().unwrap();

    let p = core.create_project("Project 1").unwrap();
    let v1 = core.create_volume(&p.id, "Volume 1").unwrap();
    let v2 = core.create_volume(&p.id, "Volume 2").unwrap();

    let volumes = core.list_volumes(&p.id).unwrap();
    // Default volume + 2 created volumes
    assert_eq!(volumes.len(), 3);

    core.rename_volume(&p.id, &v1.id, "Volume 1 Renamed").unwrap();
    let volumes = core.list_volumes(&p.id).unwrap();
    assert_eq!(volumes.iter().find(|v| v.id == v1.id).unwrap().title, "Volume 1 Renamed");

    let mut vol_ids: Vec<String> = volumes.into_iter().map(|v| v.id).collect();
    vol_ids.reverse();
    core.reorder_volumes(&p.id, &vol_ids).unwrap();
    let volumes = core.list_volumes(&p.id).unwrap();
    assert_eq!(volumes.into_iter().map(|v| v.id).collect::<Vec<_>>(), vol_ids);

    core.delete_volume(&p.id, &v2.id).unwrap();
    let volumes = core.list_volumes(&p.id).unwrap();
    assert_eq!(volumes.len(), 2);
}

#[test]
fn test_chapter_crud_and_content_operations() {
    let temp_dir = tempdir().unwrap();
    let core = WriterCore::new(temp_dir.path());
    core.create_workspace().unwrap();

    let p = core.create_project("Project 1").unwrap();
    let v = core.create_volume(&p.id, "Volume 1").unwrap();

    let c1 = core.create_chapter(&p.id, &v.id, "Chapter 1").unwrap();
    let c2 = core.create_chapter(&p.id, &v.id, "Chapter 2").unwrap();

    let chapters = core.list_chapters(&p.id, &v.id).unwrap();
    assert_eq!(chapters.len(), 2);

    let valid_ids = core.list_valid_chapter_ids(&p.id).unwrap();
    assert!(valid_ids.contains(&c1.id));
    assert!(valid_ids.contains(&c2.id));

    core.rename_chapter(&p.id, &v.id, &c1.id, "Chapter 1 Renamed").unwrap();
    let chapters = core.list_chapters(&p.id, &v.id).unwrap();
    assert_eq!(chapters.iter().find(|c| c.id == c1.id).unwrap().title, "Chapter 1 Renamed");

    core.reorder_chapters(&p.id, &v.id, &[c2.id.clone(), c1.id.clone()]).unwrap();
    let chapters = core.list_chapters(&p.id, &v.id).unwrap();
    assert_eq!(chapters[0].id, c2.id);
    assert_eq!(chapters[1].id, c1.id);

    core.write_chapter(&p.id, &v.id, &c1.id, "Hello World").unwrap();
    let content = core.read_chapter(&p.id, &v.id, &c1.id).unwrap();
    assert_eq!(content.content, "Hello World");

    assert_eq!(core.calculate_word_count("Hello World"), 10);

    core.clear_chapter_content(&p.id, &v.id, &c1.id).unwrap();
    let content = core.read_chapter(&p.id, &v.id, &c1.id).unwrap();
    assert_eq!(content.content, "");

    core.write_chapter_verified(&p.id, &v.id, &c1.id, "Verified Content").unwrap();
    let open_res = core.open_chapter(&p.id, &v.id, &c1.id).unwrap();
    assert_eq!(open_res.content, "Verified Content");

    let receipt = core.write_chapter_verified_with_allow_empty_overwrite(&p.id, &v.id, &c1.id, "", true).unwrap();
    assert_eq!(receipt.word_count, 0);

    core.update_chapter_note(&p.id, &v.id, &c1.id, "My Note").unwrap();
    let chapters = core.list_chapters(&p.id, &v.id).unwrap();
    assert_eq!(chapters.iter().find(|c| c.id == c1.id).unwrap().note.as_deref(), Some("My Note"));

    core.delete_chapter(&p.id, &v.id, &c2.id).unwrap();
    let chapters = core.list_chapters(&p.id, &v.id).unwrap();
    assert_eq!(chapters.len(), 1);
}

#[test]
fn test_project_and_volume_aggregated_update_time() {
    let temp_dir = tempdir().unwrap();
    let core = WriterCore::new(temp_dir.path());
    core.create_workspace().unwrap();

    let p = core.create_project("Project").unwrap();
    let v = core.create_volume(&p.id, "Volume").unwrap();
    let c = core.create_chapter(&p.id, &v.id, "Chapter").unwrap();

    core.write_chapter_verified(&p.id, &v.id, &c.id, "Hello").unwrap();

    let vol_updated = core.get_volume_updated_at_aggregated(&p.id, &v.id).unwrap();
    let proj_updated = core.get_project_updated_at_aggregated(&p.id).unwrap();

    assert!(!vol_updated.is_empty());
    assert!(!proj_updated.is_empty());

    let stats = core.get_project_stats(&p.id).unwrap();
    // assert_eq!(stats.project_id, p.id);
    assert!(stats.chapter_count > 0);
}
