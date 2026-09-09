use super::*;
use crate::api::types::{
    ChapterMetaDto, RestoreChapterInputDto, RestoreProjectInputDto, RestoreVolumeInputDto,
};
use tempfile::tempdir;
use uuid::Uuid;

/// 构造一个合法的 UUID 字符串。
fn new_uuid() -> String {
    Uuid::new_v4().to_string()
}

/// 构造一个完整的 RestoreProjectInputDto，包含 2 卷，每卷 2 章节，正文非空。
fn make_full_input() -> RestoreProjectInputDto {
    let project_id = new_uuid();
    let vol1_id = new_uuid();
    let vol2_id = new_uuid();
    let ch1_id = new_uuid();
    let ch2_id = new_uuid();
    let ch3_id = new_uuid();
    let ch4_id = new_uuid();

    RestoreProjectInputDto {
        project_id,
        title: "恢复测试作品".to_string(),
        order: 0,
        volumes: vec![
            RestoreVolumeInputDto {
                volume_id: vol1_id,
                title: "第一卷".to_string(),
                order: 0,
                chapters: vec![
                    RestoreChapterInputDto {
                        chapter_id: ch1_id,
                        title: "第一章".to_string(),
                        order: 0,
                        content: "第一章正文内容。".to_string(),
                    },
                    RestoreChapterInputDto {
                        chapter_id: ch2_id,
                        title: "第二章".to_string(),
                        order: 1,
                        content: "第二章正文内容。".to_string(),
                    },
                ],
            },
            RestoreVolumeInputDto {
                volume_id: vol2_id,
                title: "第二卷".to_string(),
                order: 1,
                chapters: vec![
                    RestoreChapterInputDto {
                        chapter_id: ch3_id,
                        title: "第三章".to_string(),
                        order: 0,
                        content: "第三章正文内容。".to_string(),
                    },
                    RestoreChapterInputDto {
                        chapter_id: ch4_id,
                        title: "第四章".to_string(),
                        order: 1,
                        content: "".to_string(),
                    },
                ],
            },
        ],
    }
}

/// 创建测试用 WriterCoreApi 实例（含初始化 workspace Git 仓库）。
fn make_api() -> (tempfile::TempDir, WriterCoreApi) {
    let temp_dir = tempdir().unwrap();
    std::fs::create_dir_all(temp_dir.path().join("projects")).unwrap();
    let api = WriterCoreApi::new(temp_dir.path(), temp_dir.path().join("projects"));
    // 初始化 workspace Git 仓库，让 record_workspace_change_set_history 可用
    let layout = crate::storage::git_repo_layout::GitRepoLayout::new(temp_dir.path().to_path_buf());
    crate::storage::workspace_git::ensure_workspace_repo(&layout).unwrap();
    api.set_workspace_git_layout(layout);
    (temp_dir, api)
}

#[test]
fn restore_project_tree_success_creates_full_tree() {
    let (_dir, api) = make_api();
    let input = make_full_input();

    let expected_project_id = input.project_id.clone();
    let expected_vol_ids: Vec<String> = input.volumes.iter().map(|v| v.volume_id.clone()).collect();
    let expected_chapters: Vec<(String, String, String)> = input
        .volumes
        .iter()
        .flat_map(|v| {
            v.chapters
                .iter()
                .map(|c| (v.volume_id.clone(), c.chapter_id.clone(), c.content.clone()))
        })
        .collect();

    let result = api.restore_project_tree(&input).unwrap();
    assert_eq!(result.id, expected_project_id);
    assert_eq!(result.title, "恢复测试作品");

    // 验证卷和章节都按指定 ID 创建
    let volumes = api.list_volumes(&expected_project_id).unwrap();
    assert_eq!(volumes.len(), 2);
    let actual_vol_ids: Vec<String> = volumes.iter().map(|v| v.id.clone()).collect();
    for expected_id in &expected_vol_ids {
        assert!(
            actual_vol_ids.contains(expected_id),
            "volume {} should exist",
            expected_id
        );
    }
    // 验证卷 order 连续（0, 1）
    let mut sorted_vols = volumes.clone();
    sorted_vols.sort_by_key(|v| v.order);
    assert_eq!(sorted_vols[0].order, 0);
    assert_eq!(sorted_vols[1].order, 1);

    // 验证章节和正文
    for (vol_id, ch_id, expected_content) in &expected_chapters {
        let chapters = api.list_chapters(&expected_project_id, vol_id).unwrap();
        let chapter: &ChapterMetaDto = chapters
            .iter()
            .find(|c| &c.id == ch_id)
            .unwrap_or_else(|| panic!("chapter {} should exist", ch_id));

        let opened = api
            .open_chapter(&expected_project_id, vol_id, ch_id)
            .unwrap();
        assert_eq!(opened.meta.id, chapter.id);
        assert_eq!(opened.content, *expected_content);
    }
}

#[test]
fn restore_project_tree_project_id_conflict_returns_err_and_no_partial() {
    let (_dir, api) = make_api();

    // 先创建一个项目（标题和结构都与 input 不同）
    let existing = api.create_project("已有作品").unwrap();

    // 尝试用相同 project_id 但不同内容恢复 → 应报冲突
    let mut input = make_full_input();
    input.project_id = existing.id.clone();

    let err = api.restore_project_tree(&input).unwrap_err();
    assert!(
        matches!(err, WriterError::Other(ref msg) if msg.contains("differs from restore input")),
        "expected 'differs from restore input' error, got: {:?}",
        err,
    );

    // 验证原有项目仍然完好（没有被破坏）
    let projects = api.list_projects().unwrap();
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].id, existing.id);

    // 验证原有项目的卷仍然存在（create_project 会创建默认卷）
    let volumes = api.list_volumes(&existing.id).unwrap();
    assert!(!volumes.is_empty());
}

#[test]
fn restore_project_tree_idempotent_when_project_matches() {
    let (_dir, api) = make_api();
    let input = make_full_input();

    // 第一次恢复：成功创建
    let first = api.restore_project_tree(&input).unwrap();
    assert_eq!(first.id, input.project_id);

    // 第二次恢复：内容完全一致 → 幂等返回，不报错
    let second = api.restore_project_tree(&input).unwrap();
    assert_eq!(second.id, input.project_id);
    assert_eq!(second.title, input.title);

    // 验证项目仍然只有一个（没有重复创建）
    let projects = api.list_projects().unwrap();
    assert_eq!(projects.len(), 1);
}

#[test]
fn restore_project_tree_non_idempotent_when_content_differs() {
    let (_dir, api) = make_api();
    let input = make_full_input();

    // 第一次恢复：成功创建
    api.restore_project_tree(&input).unwrap();

    // 第二次恢复：同一 project_id 但 title 不同 → 冲突
    let mut input2 = input.clone();
    input2.title = "不同标题".to_string();

    let err = api.restore_project_tree(&input2).unwrap_err();
    assert!(
        matches!(err, WriterError::Other(ref msg) if msg.contains("differs from restore input")),
        "expected 'differs from restore input' error for title mismatch, got: {:?}",
        err,
    );
}

#[test]
fn restore_project_tree_duplicate_volume_id_returns_err_and_rolls_back() {
    let (_dir, api) = make_api();

    let dup_id = new_uuid();
    let mut input = make_full_input();
    // 让两个卷用同一个 volume_id
    input.volumes[0].volume_id = dup_id.clone();
    input.volumes[1].volume_id = dup_id.clone();

    let err = api.restore_project_tree(&input).unwrap_err();
    assert!(matches!(err, WriterError::Other(ref msg) if msg.contains("duplicate volume_id")));

    // 验证没有留下半成品：项目不应存在
    let projects = api.list_projects().unwrap();
    assert!(
        projects.is_empty(),
        "no partial project should remain after rollback"
    );
}

#[test]
fn restore_project_tree_duplicate_chapter_id_returns_err_and_rolls_back() {
    let (_dir, api) = make_api();

    let dup_id = new_uuid();
    let mut input = make_full_input();
    // 让同一卷下两个章节用同一个 chapter_id
    input.volumes[0].chapters[0].chapter_id = dup_id.clone();
    input.volumes[0].chapters[1].chapter_id = dup_id.clone();

    let err = api.restore_project_tree(&input).unwrap_err();
    assert!(matches!(err, WriterError::Other(ref msg) if msg.contains("duplicate chapter_id")));

    // 验证没有留下半成品
    let projects = api.list_projects().unwrap();
    assert!(
        projects.is_empty(),
        "no partial project should remain after rollback"
    );
}

#[test]
fn restore_project_tree_empty_volumes_creates_project_only() {
    let (_dir, api) = make_api();

    let project_id = new_uuid();
    let input = RestoreProjectInputDto {
        project_id: project_id.clone(),
        title: "空作品".to_string(),
        order: 0,
        volumes: vec![],
    };

    let result = api.restore_project_tree(&input).unwrap();
    assert_eq!(result.id, project_id);
    assert_eq!(result.title, "空作品");

    // 验证项目存在但无卷
    let volumes = api.list_volumes(&project_id).unwrap();
    assert!(volumes.is_empty(), "no volumes should exist");
}

#[test]
fn restore_project_tree_volume_with_empty_chapters() {
    let (_dir, api) = make_api();

    let project_id = new_uuid();
    let vol_id = new_uuid();
    let input = RestoreProjectInputDto {
        project_id: project_id.clone(),
        title: "空卷作品".to_string(),
        order: 0,
        volumes: vec![RestoreVolumeInputDto {
            volume_id: vol_id.clone(),
            title: "空卷".to_string(),
            order: 0,
            chapters: vec![],
        }],
    };

    let result = api.restore_project_tree(&input).unwrap();
    assert_eq!(result.id, project_id);

    // 验证卷存在但无章节
    let volumes = api.list_volumes(&project_id).unwrap();
    assert_eq!(volumes.len(), 1);
    assert_eq!(volumes[0].id, vol_id);

    let chapters = api.list_chapters(&project_id, &vol_id).unwrap();
    assert!(chapters.is_empty(), "no chapters should exist");
}

#[test]
fn restore_project_tree_empty_project_id_returns_err() {
    let (_dir, api) = make_api();

    let mut input = make_full_input();
    input.project_id = "".to_string();

    let err = api.restore_project_tree(&input).unwrap_err();
    assert!(
        matches!(err, WriterError::Other(ref msg) if msg.contains("project_id must not be empty"))
    );
}

#[test]
fn restore_project_tree_invalid_project_id_format_returns_err() {
    let (_dir, api) = make_api();

    let mut input = make_full_input();
    input.project_id = "not-a-uuid".to_string();

    let err = api.restore_project_tree(&input).unwrap_err();
    assert!(
        matches!(err, WriterError::Other(ref msg) if msg.contains("invalid project_id format"))
    );
}

#[test]
fn restore_project_tree_invalid_volume_id_format_returns_err() {
    let (_dir, api) = make_api();

    let mut input = make_full_input();
    input.volumes[0].volume_id = "bad-volume-id".to_string();

    let err = api.restore_project_tree(&input).unwrap_err();
    assert!(matches!(err, WriterError::Other(ref msg) if msg.contains("invalid volume_id format")));
}

#[test]
fn restore_project_tree_invalid_chapter_id_format_returns_err() {
    let (_dir, api) = make_api();

    let mut input = make_full_input();
    input.volumes[0].chapters[0].chapter_id = "bad-chapter-id".to_string();

    let err = api.restore_project_tree(&input).unwrap_err();
    assert!(
        matches!(err, WriterError::Other(ref msg) if msg.contains("invalid chapter_id format"))
    );
}

/// 验证 build_restore_workspace_change_set 包含
/// 空正文章节的 chapter.md。
#[test]
fn build_restore_change_set_includes_chapter_md_for_empty_content() {
    let input = make_full_input();
    // make_full_input 的第四章 content 为空
    assert!(
        input.volumes[1].chapters[1].content.is_empty(),
        "test precondition: chapter 4 content should be empty"
    );

    let cs = WriterCoreApi::build_restore_workspace_change_set(&input);

    // 展开所有 Upsert 路径
    let paths: Vec<std::path::PathBuf> = cs
        .changes
        .iter()
        .filter_map(|c| match c {
            crate::storage::workspace_git::WorkspaceHistoryChange::Upsert(p) => Some(p.clone()),
            _ => None,
        })
        .collect();

    // 每个章节应该有 chapter.meta.json 和 chapter.md
    for vol in &input.volumes {
        for ch in &vol.chapters {
            let meta_path = std::path::PathBuf::from("projects")
                .join(&input.project_id)
                .join("volumes")
                .join(&vol.volume_id)
                .join("chapters")
                .join(&ch.chapter_id)
                .join("chapter.meta.json");
            let content_path = std::path::PathBuf::from("projects")
                .join(&input.project_id)
                .join("volumes")
                .join(&vol.volume_id)
                .join("chapters")
                .join(&ch.chapter_id)
                .join("chapter.md");
            assert!(
                paths.contains(&meta_path),
                "change set should contain {}",
                meta_path.display()
            );
            assert!(
                paths.contains(&content_path),
                "change set should contain {} (even for empty content)",
                content_path.display()
            );
        }
    }
}
