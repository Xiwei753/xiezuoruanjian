use crate::project::{create_project, list_projects};
use tempfile::tempdir;

#[test]
fn test_create_and_list_project() {
    let dir = tempdir().unwrap();
    let data_root = dir.path();
    std::fs::create_dir_all(data_root.join("projects")).unwrap();

    let projects = list_projects(&data_root.join("projects")).unwrap();
    assert_eq!(projects.len(), 0);

    let project = create_project(&data_root.join("projects"), "Test Project").unwrap();
    assert_eq!(project.title, "Test Project");

    let projects = list_projects(&data_root.join("projects")).unwrap();
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].title, "Test Project");

    let volumes =
        crate::volume::list_volumes(&data_root.join("projects").join(&project.id)).unwrap();
    assert_eq!(volumes.len(), 1);
    assert_eq!(volumes[0].title, "第一卷");
}

#[test]
fn test_create_project_does_not_initialize_per_project_git_repo() {
    let dir = tempdir().unwrap();
    let data_root = dir.path();
    std::fs::create_dir_all(data_root.join("projects")).unwrap();

    let project = create_project(&data_root.join("projects"), "Test Git Repo").unwrap();
    let project_dir = data_root.join("projects").join(&project.id);

    // #645 评论第 1 点：一个工作区一个 Git 仓库。作品目录不再各自初始化 `.git/`，
    // Git 仓库由 workspace 级别统一管理。create_project 后作品目录不应含 `.git/`。
    assert!(
        !project_dir.join(".git").exists(),
        "create_project 后作品目录不应含 .git/ — workspace 单一 Git 仓库模型"
    );
    // 作品目录也不是一个可打开的 Git 仓库。
    assert!(
        git2::Repository::open(&project_dir).is_err(),
        "作品目录不应是可打开的 Git 仓库 — Git 仓库由 workspace 级别管理"
    );
}

#[test]
fn test_list_projects_never_creates_per_project_git_repo() {
    let dir = tempdir().unwrap();
    let data_root = dir.path();
    let projects_root = data_root.join("projects");
    std::fs::create_dir_all(&projects_root).unwrap();

    // 手工构造旧版作品：只有 project.json，没有 .git/。
    let legacy_id = "legacy-project-without-git";
    let legacy_dir = projects_root.join(legacy_id);
    std::fs::create_dir_all(&legacy_dir).unwrap();
    let legacy = crate::project::Project {
        id: legacy_id.to_string(),
        title: "Legacy".to_string(),
        created_at: "2026-01-01T00:00:00+00:00".to_string(),
        updated_at: "2026-01-01T00:00:00+00:00".to_string(),
        order: 0,
    };
    std::fs::write(
        legacy_dir.join("project.json"),
        serde_json::to_string_pretty(&legacy).unwrap(),
    )
    .unwrap();
    assert!(!legacy_dir.join(".git").exists());

    // list_projects 只纯读取项目元数据，不触发迁移。
    let projects = list_projects(&projects_root).unwrap();
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].id, legacy_id);
    // list 不应触发迁移，.git 仍不应存在。
    assert!(!legacy_dir.join(".git").exists());

    // create_project 也不应创建 per-project .git。
    let new_project = create_project(&projects_root, "New Project").unwrap();
    let new_dir = projects_root.join(&new_project.id);
    assert!(
        !new_dir.join(".git").exists(),
        "create_project 不应创建 per-project .git — Git 仓库由 workspace 级别管理"
    );
}

#[test]
fn test_rename_project_duplicate_title() {
    let dir = tempdir().unwrap();
    let data_root = dir.path();
    std::fs::create_dir_all(data_root.join("projects")).unwrap();

    let _project1 = create_project(&data_root.join("projects"), "Project A").unwrap();
    let project2 = create_project(&data_root.join("projects"), "Project B").unwrap();

    let result =
        crate::project::rename_project(&data_root.join("projects"), &project2.id, "Project A");
    assert!(result.is_err());
    if let Err(crate::error::Error::Other(msg)) = result {
        assert_eq!(msg, "Project title already exists");
    } else {
        panic!("Expected Error::Other(\"Project title already exists\")");
    }
}

#[test]
fn test_get_project_stats_empty() {
    let dir = tempdir().unwrap();
    let data_root = dir.path();
    std::fs::create_dir_all(data_root.join("projects")).unwrap();

    let project = create_project(&data_root.join("projects"), "Test Stats Empty").unwrap();

    let stats =
        crate::project::get_project_stats(&data_root.join("projects").join(&project.id)).unwrap();
    assert_eq!(stats.volume_count, 1); // create_project automatically generates a default volume
    assert_eq!(stats.chapter_count, 0);
    assert_eq!(stats.total_word_count, 0);
}

#[test]
fn test_get_project_stats_with_data() {
    let dir = tempdir().unwrap();
    let data_root = dir.path();
    std::fs::create_dir_all(data_root.join("projects")).unwrap();

    let project = create_project(&data_root.join("projects"), "Test Stats Data").unwrap();

    // project already has 1 default volume "第一卷"
    let volumes =
        crate::volume::list_volumes(&data_root.join("projects").join(&project.id)).unwrap();
    assert_eq!(volumes.len(), 1);
    let vol1_id = &volumes[0].id;

    // Add second volume
    let vol2 =
        crate::volume::create_volume(&data_root.join("projects").join(&project.id), "第二卷")
            .unwrap();

    // Add chapters
    let c1 = crate::chapter::create_chapter(
        &data_root.join("projects").join(&project.id),
        vol1_id,
        "Chapter 1",
    )
    .unwrap();
    let c2 = crate::chapter::create_chapter(
        &data_root.join("projects").join(&project.id),
        vol1_id,
        "Chapter 2",
    )
    .unwrap();
    let c3 = crate::chapter::create_chapter(
        &data_root.join("projects").join(&project.id),
        &vol2.id,
        "Chapter 3",
    )
    .unwrap();

    // Write content
    let content1 = "This is a test content.";
    crate::chapter::save_chapter(
        &data_root.join("projects").join(&project.id),
        vol1_id,
        &c1.id,
        content1,
    )
    .unwrap();

    let content2 = "More content.";
    crate::chapter::save_chapter(
        &data_root.join("projects").join(&project.id),
        vol1_id,
        &c2.id,
        content2,
    )
    .unwrap();

    let content3 = "Even more testing content.";
    crate::chapter::save_chapter(
        &data_root.join("projects").join(&project.id),
        &vol2.id,
        &c3.id,
        content3,
    )
    .unwrap();

    // Calculate exact word counts using the core function for robust testing

    let stats =
        crate::project::get_project_stats(&data_root.join("projects").join(&project.id)).unwrap();
    assert_eq!(stats.volume_count, 2);
    assert_eq!(stats.chapter_count, 3);

    let expected_word_count = crate::chapter::calculate_word_count(content1)
        + crate::chapter::calculate_word_count(content2)
        + crate::chapter::calculate_word_count(content3);

    assert_eq!(stats.total_word_count, expected_word_count);
}

#[test]
fn test_get_project_stats_non_existent() {
    let dir = tempdir().unwrap();
    let data_root = dir.path();
    std::fs::create_dir_all(data_root.join("projects")).unwrap();

    let stats =
        crate::project::get_project_stats(&data_root.join("projects").join("non-existent-id"))
            .unwrap();
    assert_eq!(stats.volume_count, 0);
    assert_eq!(stats.chapter_count, 0);
    assert_eq!(stats.total_word_count, 0);
}

#[test]
fn test_reorder_projects_success_and_mismatch_error() {
    let dir = tempdir().unwrap();
    let data_root = dir.path();
    std::fs::create_dir_all(data_root.join("projects")).unwrap();

    let _project1 = create_project(&data_root.join("projects"), "Project 1").unwrap();
    let _project2 = create_project(&data_root.join("projects"), "Project 2").unwrap();

    let projects = list_projects(&data_root.join("projects")).unwrap();
    assert!(projects.len() >= 2);

    let mut ordered_ids = projects.iter().map(|p| p.id.clone()).collect::<Vec<_>>();

    ordered_ids.reverse();
    let result = crate::project::reorder_projects(&data_root.join("projects"), &ordered_ids);
    assert!(result.is_ok());

    let new_projects = list_projects(&data_root.join("projects")).unwrap();
    let new_ids = new_projects
        .iter()
        .map(|p| p.id.clone())
        .collect::<Vec<_>>();
    assert_eq!(new_ids, ordered_ids);

    let missing_ids = vec![ordered_ids[0].clone()];
    let result = crate::project::reorder_projects(&data_root.join("projects"), &missing_ids);
    match result {
        Err(crate::error::Error::Other(msg)) => {
            assert_eq!(msg, "Invalid ordered_ids for reorder")
        }
        _ => panic!("Expected Error::Other for missing IDs"),
    }

    let mut extra_ids = ordered_ids.clone();
    extra_ids.push("non-existent-id".to_string());
    let result = crate::project::reorder_projects(&data_root.join("projects"), &extra_ids);
    match result {
        Err(crate::error::Error::Other(msg)) => {
            assert_eq!(msg, "Invalid ordered_ids for reorder")
        }
        _ => panic!("Expected Error::Other for extra non-existent IDs"),
    }
}

#[cfg(test)]
mod tests_facade {
    use crate::facade::WriterCore;
    use tempfile::tempdir;

    #[test]
    fn test_facade_create_project_updates_project_tree() {
        let dir = tempdir().unwrap();
        let data_root = dir.path();

        let core = WriterCore::new(data_root, data_root.join("projects"));
        std::fs::create_dir_all(dir.path().join("projects")).unwrap();

        let tree_before = core.list_projects().unwrap();
        assert_eq!(tree_before.len(), 0, "Tree should be empty initially");

        let _proj = core.create_project("Test Project Facade").unwrap();

        let tree_after = core.list_projects().unwrap();
        assert_eq!(
            tree_after.len(),
            1,
            "Tree should have 1 project after creation"
        );
        assert_eq!(tree_after[0].title, "Test Project Facade");
    }
}
