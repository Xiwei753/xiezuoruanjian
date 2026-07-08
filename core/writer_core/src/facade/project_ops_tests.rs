use super::WriterCore;
use tempfile::tempdir;

#[test]
fn test_facade_project_ops_project_flow() {
    let temp_dir = tempdir().unwrap();
    let core = WriterCore::new(temp_dir.path());
    core.create_workspace().unwrap();

    // Test project creation
    let project = core.create_project("Facade Project").unwrap();
    assert_eq!(project.title, "Facade Project");

    // Test list projects
    let projects = core.list_projects().unwrap();
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].id, project.id);
    assert_eq!(projects[0].title, "Facade Project");

    // Test rename project
    core.rename_project(&project.id, "Renamed Facade Project").unwrap();
    let projects = core.list_projects().unwrap();
    assert_eq!(projects[0].title, "Renamed Facade Project");

    // Test reorder projects
    let project2 = core.create_project("Project 2").unwrap();
    let projects = core.list_projects().unwrap();
    assert_eq!(projects[0].id, project.id);
    assert_eq!(projects[1].id, project2.id);

    core.reorder_projects(&[project2.id.clone(), project.id.clone()]).unwrap();
    let projects = core.list_projects().unwrap();
    assert_eq!(projects[0].id, project2.id);
    assert_eq!(projects[1].id, project.id);

    // Test get aggregated dates
    let project_date = core.get_project_updated_at_aggregated(&project.id).unwrap();
    assert!(!project_date.is_empty());

    // Test get project stats
    let stats = core.get_project_stats(&project.id).unwrap();
    assert_eq!(stats.volume_count, 1);
    assert_eq!(stats.chapter_count, 0);
    assert_eq!(stats.total_word_count, 0);

    // Test delete project
    core.delete_project(&project.id).unwrap();
    let projects = core.list_projects().unwrap();
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].id, project2.id);
}
