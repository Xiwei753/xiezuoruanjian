use crate::chapter::{self, Chapter, ChapterContent, ChapterSaveReceipt};
use crate::error::Result;
use crate::project::{self, Project, ProjectSummary};
use crate::volume::{self, Volume};

impl super::WriterCore {
    /// #644 评论 5491531984 问题1：通过 layout 确定 Git 物理位置。
    pub fn list_projects(&self) -> Result<Vec<Project>> {
        match &self.git_metadata_root {
            Some(root) => {
                let root = root.clone();
                let projects_root = self.projects_root.clone();
                let layout_fn: project::GitLayoutFn = Box::new(move |project_id: &str| {
                    crate::storage::git_repo_layout::GitRepoLayout::with_external_git_dir(
                        projects_root.join(project_id),
                        root.join(project_id),
                    )
                });
                project::list_projects_with_layout(&self.projects_root, Some(layout_fn))
            }
            None => project::list_projects(&self.projects_root),
        }
    }

    /// #625 第二段：批量返回项目摘要（元数据 + 统计）。
    /// #644 评论 5492740265 问题4：`git_metadata_root=Some` 时走 layout factory，
    /// 不会在共享存储重新制造 `.git`。
    pub fn list_project_summaries(&self) -> Result<Vec<ProjectSummary>> {
        match &self.git_metadata_root {
            Some(root) => {
                let root = root.clone();
                let projects_root = self.projects_root.clone();
                let layout_fn: project::GitLayoutFn = Box::new(move |project_id: &str| {
                    crate::storage::git_repo_layout::GitRepoLayout::with_external_git_dir(
                        projects_root.join(project_id),
                        root.join(project_id),
                    )
                });
                project::list_project_summaries_with_layout(&self.projects_root, Some(layout_fn))
            }
            None => project::list_project_summaries(&self.projects_root),
        }
    }

    /// #644 评论 5492740265 问题4：创建作品时走 layout factory。
    ///
    /// `git_metadata_root=Some` 时，新作品从出生开始就直接使用外部 git_dir，
    /// 不会先在共享存储建 `.git` 再等下一次列表/同步搬家。
    pub fn create_project(&self, title: &str) -> Result<Project> {
        match &self.git_metadata_root {
            Some(root) => {
                let root = root.clone();
                let projects_root = self.projects_root.clone();
                let layout_fn: project::GitLayoutFn = Box::new(move |project_id: &str| {
                    crate::storage::git_repo_layout::GitRepoLayout::with_external_git_dir(
                        projects_root.join(project_id),
                        root.join(project_id),
                    )
                });
                project::create_project_with_layout_factory(
                    &self.projects_root,
                    title,
                    Some(layout_fn),
                )
            }
            None => project::create_project(&self.projects_root, title),
        }
    }

    pub fn list_volumes(&self, project_id: &str) -> Result<Vec<Volume>> {
        let project_root = self.project_root(project_id);
        volume::list_volumes(&project_root)
    }

    pub fn create_volume(&self, project_id: &str, title: &str) -> Result<Volume> {
        let project_root = self.project_root(project_id);
        volume::create_volume(&project_root, title)
    }

    pub fn list_valid_chapter_ids(
        &self,
        project_id: &str,
    ) -> Result<std::collections::HashSet<String>> {
        let project_root = self.project_root(project_id);
        chapter::list_valid_chapter_ids(&project_root)
    }

    pub fn list_chapters(&self, project_id: &str, volume_id: &str) -> Result<Vec<Chapter>> {
        let project_root = self.project_root(project_id);
        chapter::list_chapters(&project_root, volume_id)
    }

    pub fn calculate_word_count(&self, text: &str) -> u32 {
        chapter::calculate_word_count(text)
    }

    pub fn create_chapter(
        &self,
        project_id: &str,
        volume_id: &str,
        title: &str,
    ) -> Result<Chapter> {
        let project_root = self.project_root(project_id);
        chapter::create_chapter(&project_root, volume_id, title)
    }

    pub fn get_project_stats(&self, project_id: &str) -> Result<crate::project::ProjectStats> {
        let project_root = self.project_root(project_id);
        crate::project::get_project_stats(&project_root)
    }

    pub fn read_chapter(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
    ) -> Result<ChapterContent> {
        let project_root = self.project_root(project_id);
        chapter::read_chapter(&project_root, volume_id, chapter_id)
    }

    pub fn open_chapter(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
    ) -> Result<super::ChapterOpenResult> {
        let content = self.read_chapter(project_id, volume_id, chapter_id)?;
        Ok(super::ChapterOpenResult {
            meta: content.meta,
            content: content.content,
        })
    }

    pub fn write_chapter(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        content: &str,
    ) -> Result<()> {
        let project_root = self.project_root(project_id);
        chapter::save_chapter(&project_root, volume_id, chapter_id, content)
    }

    pub fn clear_chapter_content(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
    ) -> Result<()> {
        let project_root = self.project_root(project_id);
        chapter::clear_chapter_content(&project_root, volume_id, chapter_id)
    }

    pub fn clear_chapter_content_verified(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
    ) -> Result<ChapterSaveReceipt> {
        let project_root = self.project_root(project_id);
        chapter::clear_chapter_content_verified(&project_root, volume_id, chapter_id)
    }

    pub fn write_chapter_verified(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        content: &str,
    ) -> Result<ChapterSaveReceipt> {
        let project_root = self.project_root(project_id);
        chapter::save_chapter_verified(&project_root, volume_id, chapter_id, content)
    }

    pub fn write_chapter_verified_with_allow_empty_overwrite(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        content: &str,
        allow_empty_overwrite: bool,
    ) -> Result<ChapterSaveReceipt> {
        let project_root = self.project_root(project_id);
        chapter::save_chapter_verified_with_allow_empty_overwrite(
            &project_root,
            volume_id,
            chapter_id,
            content,
            allow_empty_overwrite,
        )
    }

    pub fn update_chapter_note(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        note: &str,
    ) -> Result<()> {
        let project_root = self.project_root(project_id);
        chapter::update_chapter_note(&project_root, volume_id, chapter_id, note)
    }

    /// #644 评论 5492740265 问题4：重命名作品时走 layout factory。
    pub fn rename_project(&self, project_id: &str, new_title: &str) -> crate::error::Result<()> {
        match &self.git_metadata_root {
            Some(root) => {
                let root = root.clone();
                let projects_root = self.projects_root.clone();
                let layout_fn: project::GitLayoutFn = Box::new(move |project_id: &str| {
                    crate::storage::git_repo_layout::GitRepoLayout::with_external_git_dir(
                        projects_root.join(project_id),
                        root.join(project_id),
                    )
                });
                crate::project::rename_project_with_layout(
                    &self.projects_root,
                    project_id,
                    new_title,
                    Some(layout_fn),
                )
            }
            None => crate::project::rename_project(&self.projects_root, project_id, new_title),
        }
    }

    /// #644 评论 5493295108 问题4：删除作品时同时清理 private git_dir。
    ///
    /// `git_metadata_root=Some` 时，把 private git_dir 也移进 trash
    /// （在 private 根下建 `trash/<delete-token>/`，同文件系统 rename）。
    /// worktree trash 和 private Git trash 用同一个 delete token 关联，
    /// 之后跟同一份删除生命周期清理。
    pub fn delete_project(&self, project_id: &str) -> crate::error::Result<()> {
        match &self.git_metadata_root {
            Some(_root) => {
                let layout = self.project_git_layout(project_id);
                crate::project::delete_project_with_layout(
                    &self.projects_root,
                    project_id,
                    &self.app_data_root,
                    Some(&layout),
                )
            }
            None => {
                crate::project::delete_project(&self.projects_root, project_id, &self.app_data_root)
            }
        }
    }

    /// #644 评论 5492740265 问题4：重排作品时走 layout factory。
    pub fn reorder_projects(&self, ordered_ids: &[String]) -> crate::error::Result<()> {
        match &self.git_metadata_root {
            Some(root) => {
                let root = root.clone();
                let projects_root = self.projects_root.clone();
                let layout_fn: project::GitLayoutFn = Box::new(move |project_id: &str| {
                    crate::storage::git_repo_layout::GitRepoLayout::with_external_git_dir(
                        projects_root.join(project_id),
                        root.join(project_id),
                    )
                });
                crate::project::reorder_projects_with_layout(
                    &self.projects_root,
                    ordered_ids,
                    Some(layout_fn),
                )
            }
            None => crate::project::reorder_projects(&self.projects_root, ordered_ids),
        }
    }

    pub fn rename_volume(
        &self,
        project_id: &str,
        volume_id: &str,
        new_title: &str,
    ) -> crate::error::Result<()> {
        let project_root = self.project_root(project_id);
        crate::volume::rename_volume(&project_root, volume_id, new_title)
    }

    pub fn delete_volume(&self, project_id: &str, volume_id: &str) -> crate::error::Result<()> {
        let project_root = self.project_root(project_id);
        crate::volume::delete_volume(&project_root, volume_id, &self.app_data_root)
    }

    pub fn reorder_volumes(
        &self,
        project_id: &str,
        ordered_ids: &[String],
    ) -> crate::error::Result<()> {
        let project_root = self.project_root(project_id);
        crate::volume::reorder_volumes(&project_root, ordered_ids)
    }

    pub fn rename_chapter(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        new_title: &str,
    ) -> crate::error::Result<()> {
        let project_root = self.project_root(project_id);
        crate::chapter::rename_chapter(&project_root, volume_id, chapter_id, new_title)
    }

    pub fn delete_chapter(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
    ) -> crate::error::Result<()> {
        let project_root = self.project_root(project_id);
        crate::chapter::delete_chapter(&project_root, volume_id, chapter_id, &self.app_data_root)
    }

    pub fn reorder_chapters(
        &self,
        project_id: &str,
        volume_id: &str,
        ordered_ids: &[String],
    ) -> crate::error::Result<()> {
        let project_root = self.project_root(project_id);
        crate::chapter::reorder_chapters(&project_root, volume_id, ordered_ids)
    }

    /// 从子章节聚合获取 volume 的最近更新时间。
    pub fn get_volume_updated_at_aggregated(
        &self,
        project_id: &str,
        volume_id: &str,
    ) -> crate::error::Result<String> {
        let project_root = self.project_root(project_id);
        crate::project::get_volume_updated_at_aggregated(&project_root, volume_id)
    }

    /// 从子章节聚合获取 project 的最近更新时间。
    pub fn get_project_updated_at_aggregated(
        &self,
        project_id: &str,
    ) -> crate::error::Result<String> {
        let project_root = self.project_root(project_id);
        crate::project::get_project_updated_at_aggregated(&project_root)
    }
}
