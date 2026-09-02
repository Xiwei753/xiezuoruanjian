//! # 项目删除事务（Durable Delete Transaction）
//!
//! 解决 "正文删了，private Git 还活着" 的分裂状态问题。
//!
//! ## 问题背景
//!
//! 旧 `delete_project_with_layout()` 先移动 worktree trash，再移动 private git_dir。
//! 如果进程死在两次 rename 中间，或 private trash 创建失败被 `let _ = ...` 吞掉，
//! 会出现 worktree 已删除但 git_dir 仍在原活动位置的分裂状态。
//!
//! ## 解决方案
//!
//! 在第一次 rename 前先写 `ProjectDeleteJournal`（含 from/trash 路径 + phase），
//! 每次 rename 后 fsync 对应父目录并推进 phase。private rename/create_dir 的错误
//! 直接返回，不能 warn 后当删除成功。
//!
//! 启动时恢复入口看到 journal 后，根据 from/trash 的 old/new 状态继续完成删除；
//! 已经完成两边后再清 journal。
//!
//! ## 状态机
//!
//! ```text
//! Prepared → WorktreeMoved → GitMoved → Completed
//!                (可以恢复)    (可以恢复)
//! ```
//!
//! - `Prepared`: journal 已落盘，尚未移动任何内容
//! - `WorktreeMoved`: worktree 已移入 trash，private git_dir 尚未移动
//! - `GitMoved`: 两边都已移入 trash，等待 tombstone 生成
//! - `Completed`: tombstone 已生成，可以清理 journal
//!
//! ## 崩溃恢复
//!
//! 启动时调用 `recover_pending_delete_transactions`：
//! - 遍历 app_meta/delete-journals/ 下所有 journal
//! - 根据 phase 和 from/trash 路径实际存在状态决定下一步
//! - 已经完成两边后再清 journal

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

use crate::error::Result;

/// 删除 journal 文件名前缀。
const DELETE_JOURNAL_PREFIX: &str = ".sujian-delete-journal-";

/// 删除 journal 所在目录（app_meta 下）。
const DELETE_JOURNALS_DIR: &str = "app-meta/delete-journals";

/// 项目删除事务阶段。
///
/// 写入 journal，供崩溃恢复判断删除进度：
/// - `Prepared`: journal 已落盘，尚未移动任何内容
/// - `WorktreeMoved`: worktree 已移入 trash，private git_dir 尚未移动
/// - `GitMoved`: 两边都已移入 trash，等待 tombstone 生成
/// - `Completed`: tombstone 已生成，可以清理 journal
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectDeletePhase {
    Prepared,
    WorktreeMoved,
    GitMoved,
    Completed,
}

/// 项目删除 journal。
///
/// 在第一次 rename 前先写到应用私有目录（app_meta/delete-journals/），
/// 每次 rename 后 fsync 对应父目录并推进 phase。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectDeleteJournal {
    /// 本次删除的唯一 token（关联 worktree trash 和 private Git trash）。
    pub token: String,
    /// 被删除的项目 ID。
    pub project_id: String,
    /// worktree 原始路径（projects_root/{project_id}）。
    pub worktree_from: String,
    /// worktree trash 路径（sync/trash/{token}）。
    pub worktree_trash: String,
    /// private git_dir 原始路径（可选，None 表示标准布局无 private git）。
    pub git_dir_from: Option<String>,
    /// private git_dir trash 路径（可选）。
    pub git_dir_trash: Option<String>,
    /// #644 评论 5495945801 问题3：projects_root，用于 strip_prefix 算 rel_project_dir。
    pub projects_root: String,
    /// #644 评论 5495945801 问题3：app_data_root，用于 strip_prefix 算 rel_trash_path。
    pub app_data_root: String,
    /// 当前删除阶段。
    pub phase: ProjectDeletePhase,
}

/// 项目删除事务。
///
/// 生命周期：`new` → `prepare` → `move_worktree` → `move_git` → `complete` → `cleanup_journal`。
/// 未完成的 journal 留给 `recover_pending_delete_transactions` 处理。
pub struct ProjectDeleteTransaction {
    journal: ProjectDeleteJournal,
    journal_path: PathBuf,
    completed: bool,
}

impl ProjectDeleteTransaction {
    /// 创建新的删除事务。
    ///
    /// #644 评论 5495945801 问题2：接收 trash **根目录**而非最终 trash 路径。
    /// 在内部生成一次 token 后统一得到：
    /// - `worktree_trash = worktree_trash_root.join(&token)`
    /// - `git_dir_trash = git_dir_trash_root.map(|root| root.join(&token))`
    ///
    /// journal 里记录的 worktree_trash / git_dir_trash 就是这两个最终路径。
    pub fn new(
        project_id: &str,
        worktree_from: &Path,
        worktree_trash_root: &Path,
        git_dir_from: Option<&Path>,
        git_dir_trash_root: Option<&Path>,
        projects_root: &Path,
        app_data_root: &Path,
    ) -> Self {
        let token = format!(
            "{}_{}_{}",
            chrono::Utc::now().timestamp_millis(),
            Uuid::new_v4(),
            project_id
        );

        // #644 评论 5495945801 问题2：用 token 统一拼 trash 路径，不用 project_id。
        let worktree_trash = worktree_trash_root.join(&token);
        let git_dir_trash = git_dir_trash_root.map(|root| root.join(&token));

        let journal = ProjectDeleteJournal {
            token: token.clone(),
            project_id: project_id.to_string(),
            worktree_from: worktree_from.to_string_lossy().into_owned(),
            worktree_trash: worktree_trash.to_string_lossy().into_owned(),
            git_dir_from: git_dir_from.map(|p| p.to_string_lossy().into_owned()),
            git_dir_trash: git_dir_trash
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned()),
            projects_root: projects_root.to_string_lossy().into_owned(),
            app_data_root: app_data_root.to_string_lossy().into_owned(),
            phase: ProjectDeletePhase::Prepared,
        };

        let journal_path = app_data_root
            .join(DELETE_JOURNALS_DIR)
            .join(format!("{}{}", DELETE_JOURNAL_PREFIX, token));

        Self {
            journal,
            journal_path,
            completed: false,
        }
    }

    /// 获取当前 delete token。
    pub fn token(&self) -> &str {
        &self.journal.token
    }

    /// 获取 journal 路径（供调用方写 trash 时参考）。
    pub fn worktree_trash_path(&self) -> &Path {
        Path::new(&self.journal.worktree_trash)
    }

    /// 获取 private git_dir trash 路径（可选）。
    pub fn git_dir_trash_path(&self) -> Option<&Path> {
        self.journal.git_dir_trash.as_deref().map(Path::new)
    }

    /// 准备阶段：写 journal 到 app_meta/delete-journals/。
    ///
    /// 在第一次 rename 前先写 journal，确保崩溃恢复能看到待删除状态。
    pub fn prepare(&mut self) -> Result<()> {
        let content = serde_json::to_vec(&self.journal).map_err(|e| {
            crate::error::Error::Io(std::io::Error::other(format!(
                "ProjectDeleteTransaction::prepare: serialize: {e}"
            )))
        })?;
        crate::storage::atomic_write_bytes(&self.journal_path, &content)?;
        Ok(())
    }

    /// 移动 worktree 到 trash。
    ///
    /// 移动后 fsync worktree trash 父目录并推进 phase 到 WorktreeMoved。
    /// 失败时返回 Err，journal 保留，下次恢复。
    pub fn move_worktree(&mut self) -> Result<()> {
        let worktree_from = PathBuf::from(&self.journal.worktree_from);
        let worktree_trash = PathBuf::from(&self.journal.worktree_trash);

        // 如果 worktree 已经不存在（之前已移动），直接推进 phase。
        if !worktree_from.exists() {
            self.advance_phase(ProjectDeletePhase::WorktreeMoved)?;
            return Ok(());
        }

        // 确保 trash 父目录存在。
        if let Some(parent) = worktree_trash.parent() {
            fs::create_dir_all(parent)?;
        }

        // 移动 worktree 到 trash。
        fs::rename(&worktree_from, &worktree_trash)?;

        // fsync worktree trash 父目录，持久化 rename 的目录项。
        if let Some(parent) = worktree_trash.parent() {
            crate::storage::sync_dir(parent)?;
        }

        self.advance_phase(ProjectDeletePhase::WorktreeMoved)?;
        Ok(())
    }

    /// 移动 private git_dir 到 trash。
    ///
    /// 移动后 fsync git_dir trash 父目录并推进 phase 到 GitMoved。
    /// private rename/create_dir 的错误直接返回，不能 warn 后当删除成功。
    pub fn move_git(&mut self) -> Result<()> {
        let Some(git_dir_from) = self.journal.git_dir_from.as_ref() else {
            // 无 private git_dir，直接推进 phase 到 GitMoved。
            self.advance_phase(ProjectDeletePhase::GitMoved)?;
            return Ok(());
        };

        let git_dir_from = PathBuf::from(git_dir_from);
        let git_dir_trash_str = self.journal.git_dir_trash.as_ref().ok_or_else(|| {
            crate::error::Error::Io(std::io::Error::other(
                "ProjectDeleteTransaction::move_git: git_dir_trash missing while git_dir_from present",
            ))
        })?;
        let git_dir_trash = PathBuf::from(git_dir_trash_str);

        // 如果 git_dir 已经不存在（之前已移动），直接推进 phase。
        if !git_dir_from.exists() {
            self.advance_phase(ProjectDeletePhase::GitMoved)?;
            return Ok(());
        }

        // 确保 trash 父目录存在。
        if let Some(parent) = git_dir_trash.parent() {
            fs::create_dir_all(parent)?;
        }

        // 移动 private git_dir 到 trash。
        fs::rename(&git_dir_from, &git_dir_trash)?;

        // fsync git_dir trash 父目录，持久化 rename 的目录项。
        if let Some(parent) = git_dir_trash.parent() {
            crate::storage::sync_dir(parent)?;
        }

        self.advance_phase(ProjectDeletePhase::GitMoved)?;
        Ok(())
    }

    /// #644 评论 5495945801 问题3：生成 tombstone 并保存 sync state。
    ///
    /// 正常删除和崩溃恢复共用这一份。load/save_sync_state 的错误直接返回，不吞。
    ///
    /// 调用时机：`move_git` 成功（phase == GitMoved）之后、`complete` 之前。
    /// 失败时返回 Err，journal 保留下次恢复继续。
    pub fn write_tombstone(&self) -> Result<()> {
        let worktree_trash = PathBuf::from(&self.journal.worktree_trash);
        let projects_root = PathBuf::from(&self.journal.projects_root);
        let app_data_root = PathBuf::from(&self.journal.app_data_root);
        let project_dir = PathBuf::from(&self.journal.worktree_from);

        // load_sync_state 错误直接返回，不吞。
        // 保持与原 project.rs 一致的传参：sync_root = worktree_trash。
        let mut state = crate::sync::SyncService::load_sync_state(&worktree_trash)?;

        let rel_project_dir = project_dir
            .strip_prefix(&projects_root)
            .unwrap_or(&project_dir)
            .to_string_lossy()
            .replace("\\", "/");

        let rel_trash_path = match worktree_trash.strip_prefix(&app_data_root) {
            Ok(p) => p,
            Err(_) => &worktree_trash,
        }
        .to_string_lossy()
        .replace("\\", "/");

        crate::trash::generate_tombstones(
            &mut state,
            &worktree_trash,
            &rel_project_dir,
            &rel_trash_path,
        );

        // save_sync_state 错误直接返回，不吞。
        crate::sync::SyncService::save_sync_state(&worktree_trash, &state)?;

        Ok(())
    }

    /// 完成删除：推进 phase 到 Completed。
    ///
    /// tombstone 由 `write_tombstone` 独立处理，调用方应在 `write_tombstone` 成功后
    /// 再调用 `complete`。journal 要保留到 tombstone/两边 trash 都完成以后。
    pub fn complete(&mut self) -> Result<()> {
        self.advance_phase(ProjectDeletePhase::Completed)?;
        self.completed = true;
        Ok(())
    }

    /// 清理 journal（删除 journal 文件 + fsync 父目录）。
    ///
    /// 仅在 completed == true 时调用。
    pub fn cleanup_journal(self) -> Result<()> {
        if !self.completed {
            return Ok(());
        }

        if self.journal_path.exists() {
            fs::remove_file(&self.journal_path)?;
            // fsync journal 父目录，持久化删除的目录项。
            if let Some(parent) = self.journal_path.parent() {
                crate::storage::sync_dir(parent)?;
            }
        }
        Ok(())
    }

    /// 推进 phase 并持久化 journal。
    fn advance_phase(&mut self, phase: ProjectDeletePhase) -> Result<()> {
        self.journal.phase = phase;
        let content = serde_json::to_vec(&self.journal).map_err(|e| {
            crate::error::Error::Io(std::io::Error::other(format!(
                "ProjectDeleteTransaction::advance_phase: serialize: {e}"
            )))
        })?;
        crate::storage::atomic_write_bytes(&self.journal_path, &content)?;
        Ok(())
    }
}

/// 恢复所有待处理的删除事务。
///
/// 启动时调用，遍历 app_meta/delete-journals/ 下所有 journal，
/// 根据 phase 和 from/trash 路径实际存在状态决定下一步。
///
/// 返回完成的事务数量。
pub fn recover_pending_delete_transactions(app_data_root: &Path) -> Result<usize> {
    let journals_dir = app_data_root.join(DELETE_JOURNALS_DIR);
    if !journals_dir.exists() {
        return Ok(0);
    }

    let mut recovered = 0;
    for entry in fs::read_dir(&journals_dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(file_name) = path.file_name() else {
            continue;
        };
        let file_name = file_name.to_string_lossy();
        if !file_name.starts_with(DELETE_JOURNAL_PREFIX) {
            continue;
        }

        match recover_single_journal(app_data_root, &path) {
            Ok(true) => {
                recovered += 1;
            }
            Ok(false) => {
                // 恢复未完成，journal 保留。
            }
            Err(e) => {
                // 恢复失败，保留 journal，下次重启继续。
                log::error!(
                    "[recover_pending_delete_transactions] failed to recover {}: {}",
                    path.display(),
                    e
                );
            }
        }
    }
    Ok(recovered)
}

/// 恢复单个 journal。
///
/// 返回 `Ok(true)` 表示 journal 已可以清理（Completed 或已无事可做），
/// `Ok(false)` 表示 journal 仍保留（恢复未完成）。
fn recover_single_journal(_app_data_root: &Path, journal_path: &Path) -> Result<bool> {
    let content = fs::read(journal_path)?;
    let journal: ProjectDeleteJournal = serde_json::from_slice(&content).map_err(|e| {
        crate::error::Error::Io(std::io::Error::other(format!(
            "recover_single_journal: parse {}: {e}",
            journal_path.display()
        )))
    })?;

    let worktree_from = PathBuf::from(&journal.worktree_from);

    match journal.phase {
        ProjectDeletePhase::Prepared => {
            // journal 刚写盘，还没开始移动。
            // 检查 worktree_from 是否存在，存在则继续移动。
            if worktree_from.exists() {
                // 继续移动 worktree。
                let mut tx = ProjectDeleteTransaction {
                    journal,
                    journal_path: journal_path.to_path_buf(),
                    completed: false,
                };
                tx.move_worktree()?;
                // worktree 已移动，继续处理 git。
                tx.move_git()?;
                if tx.journal.phase == ProjectDeletePhase::GitMoved {
                    // #644 评论 5495945801 问题3：git 已移动，生成 tombstone 后推进到 Completed。
                    tx.write_tombstone()?;
                    tx.complete()?;
                    tx.cleanup_journal()?;
                    return Ok(true);
                }
                // git 移动失败或不存在，保留 journal 下次恢复。
                return Ok(false);
            }
            // worktree 不存在，说明已经移到 trash（可能是崩溃在 move_worktree 之前但 rename 已成功）。
            // 推进 phase 到 WorktreeMoved，继续处理 git。
            let mut tx = ProjectDeleteTransaction {
                journal,
                journal_path: journal_path.to_path_buf(),
                completed: false,
            };
            tx.advance_phase(ProjectDeletePhase::WorktreeMoved)?;
            // 继续处理 git。
            tx.move_git()?;
            if tx.journal.phase == ProjectDeletePhase::GitMoved {
                // #644 评论 5495945801 问题3：git 已移动，生成 tombstone 后推进到 Completed。
                tx.write_tombstone()?;
                tx.complete()?;
                tx.cleanup_journal()?;
                return Ok(true);
            }
            Ok(false)
        }
        ProjectDeletePhase::WorktreeMoved => {
            // worktree 已移到 trash，private git_dir 尚未移动。
            // 继续移动 git。
            let mut tx = ProjectDeleteTransaction {
                journal,
                journal_path: journal_path.to_path_buf(),
                completed: false,
            };
            tx.move_git()?;
            if tx.journal.phase == ProjectDeletePhase::GitMoved {
                // #644 评论 5495945801 问题3：git 已移动，生成 tombstone 后推进到 Completed。
                tx.write_tombstone()?;
                tx.complete()?;
                tx.cleanup_journal()?;
                return Ok(true);
            }
            Ok(false)
        }
        ProjectDeletePhase::GitMoved => {
            // 两边都已移到 trash，等待 tombstone 生成。
            // #644 评论 5495945801 问题3：必须重新生成 tombstone，不能直接跳到 Completed。
            // 如果进程死在"两边 rename 完成、tombstone 尚未保存"之间，
            // 重启后必须在这里补上 tombstone，否则永久漏掉。
            let mut tx = ProjectDeleteTransaction {
                journal,
                journal_path: journal_path.to_path_buf(),
                completed: false,
            };
            tx.write_tombstone()?;
            tx.complete()?;
            tx.cleanup_journal()?;
            Ok(true)
        }
        ProjectDeletePhase::Completed => {
            // #644 评论 5496728184 缺陷2修复：Completed 必须用 durable cleanup 范式
            // （与 cleanup_journal 第314-327行一致），不能吞删除错误。
            // 删除失败时返回 Err，不能声称"已恢复完成"。
            if journal_path.exists() {
                fs::remove_file(journal_path)?;
                // fsync journal 父目录，持久化删除的目录项。
                if let Some(parent) = journal_path.parent() {
                    crate::storage::sync_dir(parent)?;
                }
            }
            Ok(true)
        }
    }
}

#[cfg(test)]
mod tests;
