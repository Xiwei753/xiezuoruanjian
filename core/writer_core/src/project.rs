//! # 项目管理（Core 层）
//!
//! 负责作品（Project）的 CRUD、统计、排序、重命名、删除。
//!
//! ## 职责边界
//!
//! - **做**：项目创建/列表/重命名/删除/排序/统计
//! - **不做**：卷和章节管理（由 `volume.rs` / `chapter.rs` 负责）
//! - **删除安全**：所有删除操作经过 `delete_guard` 验证，删除后移入 trash 目录并记录 tombstone
//!
//! ## 目录结构
//!
//! ```text
//! projects/
//!   {project_id}/
//!     project.json          # 项目元数据（id、title、order、时间戳）
//!     volumes/              # 所有卷
//!     characters/           # 角色数据（预留）
//! ```
//!
//! #645 评论第 1 点：一个工作区一个 Git 仓库。作品目录不再各自初始化 `.git/`，
//! Git 仓库由 workspace 级别统一管理（见 `sync::staging`）。

use crate::error::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use uuid::Uuid;

/// 项目元数据结构体。
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Project {
    pub id: String,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub order: i32,
}

pub fn list_projects(projects_root: &Path) -> Result<Vec<Project>> {
    list_projects_inner(projects_root)
}

/// 纯读取项目元数据，不调用 `ensure_project_repo_with_layout` / `ensure_project_repo`。
///
/// #644 评论 5493295108 问题1：冷启动/同步不能把 Core 卡住——列表/摘要不能顺手迁移
/// 所有旧作品。迁移职责移到 `sync::staging::prepare_staging_runs`（已释放 Core 写锁之后）。
/// #645 评论第 1 点：作品目录不再各自养 Git 仓库，list 永远只读 `project.json`。
#[allow(
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    clippy::excessive_nesting,
    clippy::too_many_arguments,
    clippy::type_complexity
)]
fn list_projects_inner(projects_root: &Path) -> Result<Vec<Project>> {
    let projects_dir = projects_root;
    if !projects_dir.exists() {
        return Ok(Vec::new());
    }

    let mut projects = Vec::new();
    for entry in fs::read_dir(projects_dir)? {
        let entry = entry?;
        let ft = entry.file_type()?;
        let is_dir = ft.is_dir() || (ft.is_symlink() && entry.path().is_dir());
        if is_dir {
            let meta_path = entry.path().join("project.json");
            match fs::read_to_string(&meta_path) {
                Ok(content) => {
                    if let Ok(project) = serde_json::from_str::<Project>(&content) {
                        // #644 评论 5493295108 问题1：纯读取，不触发迁移。
                        // 旧作品（无 .git/）的 Git 迁移由 `sync::staging::prepare_staging_runs`
                        // 在已释放 Core 写锁之后执行，不堵住冷启动卷章读取。
                        // #645 评论第 1 点：作品不再各自养 Git 仓库，list 永远只读元数据。
                        projects.push(project);
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {}
                Err(e) => return Err(e.into()),
            }
        }
    }
    projects.sort_by_key(|p| p.order);
    Ok(projects)
}

/// 项目统计摘要。
///
/// `total_word_count` 为所有章节字数之和，`volume_count`/`chapter_count` 为直接子项计数。
/// 计算需要遍历所有卷和章节目录，对大型项目有一定 I/O 开销。
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ProjectStats {
    pub total_word_count: u32,
    pub volume_count: u32,
    pub chapter_count: u32,
}

/// 项目摘要 — 元数据 + 统计一次性返回（#625 第二段）。
///
/// 作品卡片要显示字数，需要在列表时一次拿到所有项目的 summary，
/// 避免端侧逐卡跨 FFI 调 `get_project_stats`（N 次 FFI + N 次遍历）。
/// `total_word_count`/`volume_count`/`chapter_count` 复用 `get_project_stats` 逻辑。
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ProjectSummary {
    pub id: String,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
    pub total_word_count: u32,
    pub volume_count: u32,
    pub chapter_count: u32,
}

#[derive(Deserialize)]
struct ChapterWordCount {
    #[serde(default)]
    word_count: u32,
}

#[allow(
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    clippy::excessive_nesting,
    clippy::too_many_arguments,
    clippy::type_complexity
)]
pub fn get_project_stats(project_root: &Path) -> Result<ProjectStats> {
    let mut stats = ProjectStats {
        total_word_count: 0,
        volume_count: 0,
        chapter_count: 0,
    };

    let volumes_dir = project_root.join("volumes");

    if !volumes_dir.exists() {
        return Ok(stats);
    }

    for vol_entry in fs::read_dir(&volumes_dir)? {
        let vol_entry = vol_entry?;

        if !vol_entry.file_type()?.is_dir() {
            continue;
        }

        let vol_path = vol_entry.path();
        if vol_path.join("volume.json").exists() {
            stats.volume_count += 1;

            let chapters_dir = vol_path.join("chapters");
            if let Ok(chap_iter) = fs::read_dir(&chapters_dir) {
                for chap_entry in chap_iter {
                    let chap_entry = chap_entry?;

                    if !chap_entry.file_type()?.is_dir() {
                        continue;
                    }

                    let meta_path = chap_entry.path().join("chapter.meta.json");
                    if let Ok(content) = fs::read(&meta_path) {
                        stats.chapter_count += 1;
                        if let Ok(meta) = serde_json::from_slice::<ChapterWordCount>(&content) {
                            stats.total_word_count += meta.word_count;
                        }
                    }
                }
            }
        }
    }

    Ok(stats)
}

/// 列出所有项目摘要（元数据 + 统计），#625 第二段新增批量 API。
///
/// 复用 `list_projects_inner`（纯读取元数据 + 排序）和 `get_project_stats`（字数累加），
/// 一次遍历返回带 `total_word_count`/`volume_count`/`chapter_count` 的列表。
/// 不破坏现有 `list_projects` 4 字段契约。
///
/// 错误处理：磁盘/IO 错误显式向上传播，不用 unwrap/expect。
///
/// #645 评论第 1 点：作品不再各自养 Git 仓库，摘要路径只读 `project.json` + 统计。
pub fn list_project_summaries(projects_root: &Path) -> Result<Vec<ProjectSummary>> {
    let projects = list_projects_inner(projects_root)?;
    let mut summaries = Vec::with_capacity(projects.len());
    for project in projects {
        let project_dir = projects_root.join(&project.id);
        let stats = get_project_stats(&project_dir)?;
        summaries.push(ProjectSummary {
            id: project.id,
            title: project.title,
            created_at: project.created_at,
            updated_at: project.updated_at,
            total_word_count: stats.total_word_count,
            volume_count: stats.volume_count,
            chapter_count: stats.chapter_count,
        });
    }
    Ok(summaries)
}

/// 创建项目并自动创建"第一卷"。
///
/// `order` 字段取现有项目最大 order + 1，保证新项目排在最后。
/// 自动调用 `volume::create_volume` 创建默认卷，保持产品一致性。
///
/// #645 评论第 1 点：一个工作区一个 Git 仓库。`create_project` 只创建作品目录、
/// `project.json`、`volumes/`、`characters/` 和默认卷，**不再初始化作品级 `.git/`**。
/// Git 仓库由 workspace 级别统一管理（见 `sync::staging`）。
pub fn create_project(projects_root: &Path, title: &str) -> Result<Project> {
    let projects = list_projects_inner(projects_root)?;
    let order = projects
        .iter()
        .map(|p| p.order)
        .max()
        .map(|m| m + 1)
        .unwrap_or(0);

    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let project = Project {
        id: id.clone(),
        title: title.to_string(),
        created_at: now.clone(),
        updated_at: now,
        order,
    };

    let project_dir = projects_root.join(&id);
    fs::create_dir_all(&project_dir)?;
    // #645 评论第 1 点：一个工作区一个 Git 仓库。作品目录不再各自初始化 `.git/`，
    // Git 仓库由 workspace 级别统一管理（见 `sync::staging`）。
    fs::create_dir_all(project_dir.join("volumes"))?;
    fs::create_dir_all(project_dir.join("characters"))?;

    let meta_path = project_dir.join("project.json");
    let content = serde_json::to_string_pretty(&project)?;
    crate::storage::atomic_write_string(&meta_path, &content)?;

    // Create a default volume to maintain consistency with product requirements
    let _ = crate::volume::create_volume(&project_dir, "第一卷")?;

    Ok(project)
}

/// 重命名项目。
///
/// 同一作品根下不允许重名（title 唯一性检查）。
/// 如果新标题已被其他项目使用，返回 `Error::Other`。
///
/// #645 评论第 1 点：重命名只改 `project.json`，不涉及 Git 仓库。
pub fn rename_project(projects_root: &Path, project_id: &str, new_title: &str) -> Result<()> {
    let projects = list_projects_inner(projects_root)?;
    if projects
        .iter()
        .any(|p| p.title == new_title && p.id != project_id)
    {
        return Err(crate::error::Error::Other(
            "Project title already exists".to_string(),
        ));
    }

    let project_dir = projects_root.join(project_id);
    let meta_path = project_dir.join("project.json");

    if !meta_path.exists() {
        return Err(crate::error::Error::ProjectNotFound);
    }

    let meta_str = fs::read_to_string(&meta_path)?;
    let mut meta: Project = serde_json::from_str(&meta_str)?;

    meta.title = new_title.to_string();
    meta.updated_at = Utc::now().to_rfc3339();

    let updated_meta_str = serde_json::to_string_pretty(&meta)?;
    crate::storage::atomic_write_string(&meta_path, &updated_meta_str)?;

    Ok(())
}

/// 删除作品。
///
/// 使用 durable delete transaction 解决"正文删了，private Git 还活着"的分裂状态：
/// 1. 在第一次 rename 前先写 `ProjectDeleteJournal`（含 from/trash 路径 + phase）
/// 2. 每次 rename 后 fsync 对应父目录并推进 phase
/// 3. #644 评论 5495945801 问题3：tombstone 收进事务（`tx.write_tombstone`），
///    错误直接返回不吞；journal 保留到 tombstone/worktree trash 都完成以后
///
/// #645 评论第 1 点：一个工作区一个 Git 仓库。Git 仓库是 workspace 级别共享的，
/// 删除单个作品只移 worktree 进 trash，**不移动共享 git_dir**。因此不再传
/// `git_dir_from`/`git_dir_trash_root`（传 None），也不再调 `tx.move_git()`。
#[allow(
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    clippy::excessive_nesting,
    clippy::too_many_arguments,
    clippy::type_complexity
)]
pub fn delete_project(projects_root: &Path, project_id: &str, app_data_root: &Path) -> Result<()> {
    let project_id = crate::delete_guard::validate_id_segment(project_id)?;
    let project_dir = projects_root.join(project_id);
    let target_canon =
        crate::delete_guard::validate_delete_target(projects_root, &project_dir, "project.json")?;

    // #644 评论 5495945801 问题2：只传 trash root，token 在事务内部生成。
    let worktree_trash_root = app_data_root.join("sync/trash");

    // #645 评论第 1 点：workspace 共享 git_dir 不因删除单个作品而移动。
    // 创建 durable delete transaction，不传 private git_dir。
    let mut tx = crate::storage::journal::project_delete::ProjectDeleteTransaction::new(
        project_id,
        &target_canon,
        &worktree_trash_root,
        None,
        None,
        projects_root,
        app_data_root,
    );

    // 1. 准备阶段：写 journal 到 app_meta/delete-journals/。
    //    在第一次 rename 前先写 journal，确保崩溃恢复能看到待删除状态。
    tx.prepare()?;

    // 2. 移动 worktree 到 trash。
    tx.move_worktree()?;

    // 3. #644 评论 5495945801 问题3：生成 tombstone（收进事务，错误直接返回不吞）。
    //    删除原来的 if let Ok / let _ = tombstone 代码块。
    tx.write_tombstone()?;

    // 4. 完成删除：推进 phase 到 Completed。
    tx.complete()?;

    // 5. 清理 journal。
    tx.cleanup_journal()?;

    Ok(())
}

/// 从子章节聚合获取 volume 的最近更新时间。
///
/// 遍历该 volume 下所有 chapter.meta.json，取最大的 updated_at。
/// 如果没有子章节，返回 volume.json 的 created_at 作为 fallback。
pub fn get_volume_updated_at_aggregated(project_root: &Path, volume_id: &str) -> Result<String> {
    let chapters = crate::chapter::list_chapters(project_root, volume_id)?;

    if let Some(max_updated) = chapters.iter().map(|c| c.updated_at.as_str()).max() {
        return Ok(max_updated.to_string());
    }

    // Fallback: no chapters, use volume.json created_at
    let volume_dir = project_root.join("volumes").join(volume_id);
    let meta_path = volume_dir.join("volume.json");
    if meta_path.exists() {
        let raw = fs::read_to_string(&meta_path)?;
        if let Ok(vol) = serde_json::from_str::<crate::volume::Volume>(&raw) {
            return Ok(vol.created_at);
        }
    }

    Ok(Utc::now().to_rfc3339())
}

/// 从子章节聚合获取 project 的最近更新时间。
///
/// 遍历该 project 下所有 volume 下所有 chapter.meta.json，取最大的 updated_at。
/// 如果没有子章节，返回 project.json 的 created_at 作为 fallback。
pub fn get_project_updated_at_aggregated(project_root: &Path) -> Result<String> {
    let volumes = crate::volume::list_volumes(project_root)?;

    let mut max_updated: Option<String> = None;

    for vol in &volumes {
        let chapters = crate::chapter::list_chapters(project_root, &vol.id)?;
        for ch in &chapters {
            match &max_updated {
                Some(current) if ch.updated_at.as_str() > current.as_str() => {
                    max_updated = Some(ch.updated_at.clone());
                }
                None => {
                    max_updated = Some(ch.updated_at.clone());
                }
                _ => {}
            }
        }
    }

    if let Some(updated) = max_updated {
        return Ok(updated);
    }

    // Fallback: no chapters in any volume, use project.json created_at
    let project_dir = project_root;
    let meta_path = project_dir.join("project.json");
    if meta_path.exists() {
        let raw = fs::read_to_string(&meta_path)?;
        if let Ok(proj) = serde_json::from_str::<Project>(&raw) {
            return Ok(proj.created_at);
        }
    }

    Ok(Utc::now().to_rfc3339())
}

/// #645 评论第 1 点：重排只改各 `project.json` 的 `order` 字段，不涉及 Git 仓库。
#[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
pub fn reorder_projects(projects_root: &Path, ordered_ids: &[String]) -> Result<()> {
    let mut projects = list_projects_inner(projects_root)?;
    let mut projects_map = std::collections::HashMap::new();
    for p in projects.drain(..) {
        projects_map.insert(p.id.clone(), p);
    }

    let existing_ids: std::collections::HashSet<_> = projects_map.keys().cloned().collect();
    let new_ids: std::collections::HashSet<_> = ordered_ids.iter().cloned().collect();

    if existing_ids.len() != new_ids.len()
        || existing_ids != new_ids
        || ordered_ids.len() != new_ids.len()
    {
        return Err(crate::error::Error::Other(
            "Invalid ordered_ids for reorder".to_string(),
        ));
    }

    let now = Utc::now().to_rfc3339();

    for (index, id) in ordered_ids.iter().enumerate() {
        if let Some(meta) = projects_map.get_mut(id) {
            if meta.order != index as i32 {
                meta.order = index as i32;
                meta.updated_at = now.clone();
                let project_dir = projects_root.join(id);
                let meta_path = project_dir.join("project.json");
                let updated_meta_str = serde_json::to_string_pretty(&meta)?;
                crate::storage::atomic_write_string(&meta_path, &updated_meta_str)?;
            }
        } else {
            return Err(crate::error::Error::ProjectNotFound);
        }
    }
    Ok(())
}

#[cfg(test)]
mod inline_tests {
    use super::*;
    use tempfile::tempdir;

    /// 验证聚合查询：有子章节时返回最大的 chapter updated_at。
    #[test]
    fn test_aggregated_volume_updated_at_with_chapters() {
        let temp_dir = tempdir().unwrap();
        let data_root = temp_dir.path();

        std::fs::create_dir_all(data_root.join("projects")).unwrap();
        let project =
            crate::project::create_project(&data_root.join("projects"), "TestProject").unwrap();
        let volumes =
            crate::volume::list_volumes(&data_root.join("projects").join(&project.id)).unwrap();
        let volume = &volumes[0];

        // Create two chapters
        let ch1 = crate::chapter::create_chapter(
            &data_root.join("projects").join(&project.id),
            &volume.id,
            "Ch1",
        )
        .unwrap();
        let _ch2 = crate::chapter::create_chapter(
            &data_root.join("projects").join(&project.id),
            &volume.id,
            "Ch2",
        )
        .unwrap();

        // Save ch1 with content (updates its updated_at)
        crate::chapter::save_chapter_verified(
            &data_root.join("projects").join(&project.id),
            &volume.id,
            &ch1.id,
            "Some content",
        )
        .unwrap();

        // The aggregated updated_at should be the max of all chapter updated_at values
        let aggregated = get_volume_updated_at_aggregated(
            &data_root.join("projects").join(&project.id),
            &volume.id,
        )
        .unwrap();
        assert!(
            !aggregated.is_empty(),
            "aggregated updated_at should not be empty"
        );

        // Verify it matches the latest chapter's updated_at
        let chapters = crate::chapter::list_chapters(
            &data_root.join("projects").join(&project.id),
            &volume.id,
        )
        .unwrap();
        let max_updated = chapters
            .iter()
            .map(|c| c.updated_at.as_str())
            .max()
            .unwrap();
        assert_eq!(aggregated, max_updated);
    }

    /// 验证聚合查询：无子章节时 fallback 到 volume.json 的 created_at。
    #[test]
    fn test_aggregated_volume_updated_at_no_chapters() {
        let temp_dir = tempdir().unwrap();
        let data_root = temp_dir.path();

        std::fs::create_dir_all(data_root.join("projects")).unwrap();
        let project =
            crate::project::create_project(&data_root.join("projects"), "TestProject").unwrap();
        let volumes =
            crate::volume::list_volumes(&data_root.join("projects").join(&project.id)).unwrap();
        let volume = &volumes[0];

        // No chapters created — should fallback to volume's created_at
        let aggregated = get_volume_updated_at_aggregated(
            &data_root.join("projects").join(&project.id),
            &volume.id,
        )
        .unwrap();
        assert_eq!(aggregated, volume.created_at);
    }

    /// 验证聚合查询：project 级别，有子章节时返回最大的 chapter updated_at。
    #[test]
    fn test_aggregated_project_updated_at_with_chapters() {
        let temp_dir = tempdir().unwrap();
        let data_root = temp_dir.path();

        std::fs::create_dir_all(data_root.join("projects")).unwrap();
        let project =
            crate::project::create_project(&data_root.join("projects"), "TestProject").unwrap();
        let volumes =
            crate::volume::list_volumes(&data_root.join("projects").join(&project.id)).unwrap();
        let volume = &volumes[0];

        let ch1 = crate::chapter::create_chapter(
            &data_root.join("projects").join(&project.id),
            &volume.id,
            "Ch1",
        )
        .unwrap();
        crate::chapter::save_chapter_verified(
            &data_root.join("projects").join(&project.id),
            &volume.id,
            &ch1.id,
            "Project level content",
        )
        .unwrap();

        let aggregated =
            get_project_updated_at_aggregated(&data_root.join("projects").join(&project.id))
                .unwrap();
        assert!(!aggregated.is_empty());

        // Verify it matches the latest chapter's updated_at across all volumes
        let chapters = crate::chapter::list_chapters(
            &data_root.join("projects").join(&project.id),
            &volume.id,
        )
        .unwrap();
        let max_updated = chapters
            .iter()
            .map(|c| c.updated_at.as_str())
            .max()
            .unwrap();
        assert_eq!(aggregated, max_updated);
    }

    /// 验证聚合查询：project 级别，无子章节时 fallback 到 project.json 的 created_at。
    #[test]
    fn test_aggregated_project_updated_at_no_chapters() {
        let temp_dir = tempdir().unwrap();
        let data_root = temp_dir.path();

        std::fs::create_dir_all(data_root.join("projects")).unwrap();
        let project =
            crate::project::create_project(&data_root.join("projects"), "TestProject").unwrap();

        // No chapters — should fallback to project's created_at
        let aggregated =
            get_project_updated_at_aggregated(&data_root.join("projects").join(&project.id))
                .unwrap();
        assert_eq!(aggregated, project.created_at);
    }

    #[test]
    fn test_delete_project_success() {
        let temp_dir = tempdir().unwrap();
        let data_root = temp_dir.path();

        std::fs::create_dir_all(data_root.join("projects")).unwrap();
        let project =
            crate::project::create_project(&data_root.join("projects"), "TestProjectToDelete")
                .unwrap();

        let project_dir = data_root.join("projects").join(&project.id);
        assert!(project_dir.exists());

        let result = delete_project(&data_root.join("projects"), &project.id, data_root);
        assert!(result.is_ok());

        assert!(!project_dir.exists());

        let trash_dir = data_root.join("sync/trash");
        assert!(trash_dir.exists());

        // Trash should have something
        let trash_contents: Vec<_> = std::fs::read_dir(&trash_dir).unwrap().collect();
        assert!(!trash_contents.is_empty());

        // Verify we can't find it
        let list_res = list_projects(&data_root.join("projects")).unwrap();
        assert!(list_res.iter().find(|p| p.id == project.id).is_none());
    }

    #[test]
    fn test_delete_project_not_found() {
        let temp_dir = tempdir().unwrap();
        let data_root = temp_dir.path();
        std::fs::create_dir_all(data_root.join("projects")).unwrap();

        let result = delete_project(&data_root.join("projects"), "non_existent_id", data_root);
        assert!(result.is_err());
        match result {
            Err(crate::error::Error::InvalidDeleteTarget(_)) => {}
            _ => panic!("Expected InvalidDeleteTarget error for non-existent project"),
        }
    }

    /// #625 第二段：空目录返回空 summary 列表。
    #[test]
    fn test_list_project_summaries_empty_dir() {
        let temp_dir = tempdir().unwrap();
        let data_root = temp_dir.path();
        std::fs::create_dir_all(data_root.join("projects")).unwrap();

        let summaries = list_project_summaries(&data_root.join("projects")).unwrap();
        assert!(summaries.is_empty());
    }

    /// #625 第二段：有项目时返回正确 summary，含 total_word_count/volume_count/chapter_count。
    #[test]
    fn test_list_project_summaries_with_stats() {
        let temp_dir = tempdir().unwrap();
        let data_root = temp_dir.path();
        let projects_root = data_root.join("projects");
        std::fs::create_dir_all(&projects_root).unwrap();

        // 项目 1：自动创建"第一卷"，再加一个章节并写入正文。
        let project1 = create_project(&projects_root, "ProjectOne").unwrap();
        let volumes1 = crate::volume::list_volumes(&projects_root.join(&project1.id)).unwrap();
        assert_eq!(volumes1.len(), 1);
        let volume1 = &volumes1[0];
        let ch1 =
            crate::chapter::create_chapter(&projects_root.join(&project1.id), &volume1.id, "Ch1")
                .unwrap();
        // "Hello World" 非空白字符数 = 10（calculate_word_count 按非空白字符计）。
        crate::chapter::save_chapter_verified(
            &projects_root.join(&project1.id),
            &volume1.id,
            &ch1.id,
            "Hello World",
        )
        .unwrap();

        // 项目 2：只有自动创建的"第一卷"，无章节。
        let project2 = create_project(&projects_root, "ProjectTwo").unwrap();

        let summaries = list_project_summaries(&projects_root).unwrap();
        assert_eq!(summaries.len(), 2);

        // 按 order 排序，project1 先创建（order=0），project2 后创建（order=1）。
        let s1 = &summaries[0];
        let s2 = &summaries[1];
        assert_eq!(s1.id, project1.id);
        assert_eq!(s1.title, "ProjectOne");
        assert_eq!(s1.volume_count, 1);
        assert_eq!(s1.chapter_count, 1);
        assert_eq!(s1.total_word_count, 10);

        assert_eq!(s2.id, project2.id);
        assert_eq!(s2.title, "ProjectTwo");
        assert_eq!(s2.volume_count, 1);
        assert_eq!(s2.chapter_count, 0);
        assert_eq!(s2.total_word_count, 0);

        // 元数据字段应与 Project 一致。
        assert_eq!(s1.created_at, project1.created_at);
        assert_eq!(s1.updated_at, project1.updated_at);
    }

    /// #625 第二段：list_project_summaries 与 list_projects 排序一致（按 order）。
    #[test]
    fn test_list_project_summaries_order_consistent_with_list_projects() {
        let temp_dir = tempdir().unwrap();
        let data_root = temp_dir.path();
        let projects_root = data_root.join("projects");
        std::fs::create_dir_all(&projects_root).unwrap();

        let p1 = create_project(&projects_root, "P1").unwrap();
        let p2 = create_project(&projects_root, "P2").unwrap();
        let p3 = create_project(&projects_root, "P3").unwrap();

        let projects = list_projects(&projects_root).unwrap();
        let summaries = list_project_summaries(&projects_root).unwrap();

        assert_eq!(projects.len(), summaries.len());
        for (p, s) in projects.iter().zip(summaries.iter()) {
            assert_eq!(p.id, s.id);
            assert_eq!(p.title, s.title);
        }
        // 顺序应一致。
        assert_eq!(summaries[0].id, p1.id);
        assert_eq!(summaries[1].id, p2.id);
        assert_eq!(summaries[2].id, p3.id);
    }
}

#[cfg(test)]
mod tests;
