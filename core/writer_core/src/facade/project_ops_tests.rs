use super::WriterCore;
use tempfile::tempdir;

#[test]
fn test_project_ops() {
    let temp_dir = tempdir().unwrap();
    let core = WriterCore::new(temp_dir.path());
    core.create_workspace().unwrap();

    let p = core.create_project("Project 1").unwrap();
    assert_eq!(p.title, "Project 1");
    let projects = core.list_projects().unwrap();
    assert_eq!(projects.len(), 1);

    core.rename_project(&p.id, "Project 1 Renamed").unwrap();
    let projects = core.list_projects().unwrap();
    assert_eq!(projects[0].title, "Project 1 Renamed");

    let p2 = core.create_project("Project 2").unwrap();
    core.reorder_projects(&[p2.id.clone(), p.id.clone()]).unwrap();
    let projects = core.list_projects().unwrap();
    assert_eq!(projects[0].id, p2.id);

    let v = core.create_volume(&p.id, "Vol 1").unwrap();
    assert_eq!(v.title, "Vol 1");
    core.rename_volume(&p.id, &v.id, "Vol 1 Renamed").unwrap();

    let v_default = &core.list_volumes(&p.id).unwrap()[0];

    let v2 = core.create_volume(&p.id, "Vol 2").unwrap();
    core.reorder_volumes(&p.id, &[v2.id.clone(), v_default.id.clone(), v.id.clone()]).unwrap();

    let c = core.create_chapter(&p.id, &v.id, "Ch 1").unwrap();
    assert_eq!(c.title, "Ch 1");
    core.rename_chapter(&p.id, &v.id, &c.id, "Ch 1 Renamed").unwrap();

    let c2 = core.create_chapter(&p.id, &v.id, "Ch 2").unwrap();
    core.reorder_chapters(&p.id, &v.id, &[c2.id.clone(), c.id.clone()]).unwrap();

    core.write_chapter(&p.id, &v.id, &c.id, "test content").unwrap();

    let wc = core.calculate_word_count("hello world");
    assert_eq!(wc, 10);

    core.update_chapter_note(&p.id, &v.id, &c.id, "some note").unwrap();

    let valid_ids = core.list_valid_chapter_ids(&p.id).unwrap();
    assert!(valid_ids.contains(&c.id));
    assert!(valid_ids.contains(&c2.id));

    let stats = core.get_project_stats(&p.id).unwrap();
    assert!(stats.total_word_count > 0);

    let time1 = core.get_volume_updated_at_aggregated(&p.id, &v.id).unwrap();
    assert!(!time1.is_empty());

    let time2 = core.get_project_updated_at_aggregated(&p.id).unwrap();
    assert!(!time2.is_empty());

    core.clear_chapter_content(&p.id, &v.id, &c.id).unwrap();
    let content = core.read_chapter(&p.id, &v.id, &c.id).unwrap();
    assert_eq!(content.content, "");

    core.delete_chapter(&p.id, &v.id, &c.id).unwrap();
    let chs = core.list_chapters(&p.id, &v.id).unwrap();
    assert_eq!(chs.len(), 1);

    core.delete_volume(&p.id, &v.id).unwrap();
    let vols = core.list_volumes(&p.id).unwrap();
    assert_eq!(vols.len(), 2);

    core.delete_project(&p.id).unwrap();
    let projects = core.list_projects().unwrap();
    assert_eq!(projects.len(), 1);
}
