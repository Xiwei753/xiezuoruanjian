//! # 项目删除事务（Durable Delete Transaction）
//!
//! 解决 "正文删了，private Git 还活着" 的分裂状态问题。
//!
//! ## 问题背景
//!
//! 旧 `delete_project()` 先移动 worktree trash，再移动 private git_dir。
//! 如果进程死在两次 rename 中间，或 private trash 创建失败被 `let _ = ...` 吞掉，
//! 会出现 worktree 已删除但 git_dir 仍在原活动位置的分裂状态。
//!
//! #645 评论第 1 点：一个工作区一个 Git 仓库后，删除单个作品不再移动共享 git_dir，
//! 事务只覆盖 worktree 移动 + tombstone 生成。
//!
//! #645 评论 5504296097 问题2：StarMap 解绑也收进本事务，避免
//! "解绑成功但作品删除失败" 或 "解绑失败但作品删除成功" 的半状态。
//!
//! ## 解决方案
//!
//! 在第一次 rename 前先写 `ProjectDeleteJournal`（含 from/trash 路径 + phase +
//! 待解绑 starmap_ids），每次 rename 后 fsync 对应父目录并推进 phase。
//! private rename/create_dir 的错误直接返回，不能 warn 后当删除成功。
//!
//! 启动时恢复入口看到 journal 后，根据 from/trash 的 old/new 状态继续完成删除；
//! 已经完成两边后再清 journal。
//!
//! ## 状态机
//!
//! ```text
//! Prepared → WorktreeMoved → GitMoved → TombstoneWritten → StarMapsUnbound → HistoryRecorded → RemoteDeleteQueued → Completed
//!                (可以恢复)    (可以恢复)      (可以恢复)          (可以恢复)        (可以恢复)          (可以恢复)
//! ```
//!
//! - `Prepared`: journal 已落盘，尚未移动任何内容
//! - `WorktreeMoved`: worktree 已移入 trash，private git_dir 尚未移动
//! - `GitMoved`: 两边都已移入 trash，等待 tombstone 生成
//! - `TombstoneWritten`: tombstone 已生成并持久化（#645 评论 5504296097 缺口3）
//! - `StarMapsUnbound`: starmap 解绑已完成
//! - `HistoryRecorded`: workspace Git history 已记录（#645 评论 5504296097 缺口2）
//! - `RemoteDeleteQueued`: PendingDeletedTarget 已落盘到
//!   `app-meta/sync/pending_deleted_targets.json`（#645 评论 5504296097 问题1），
//!   下次 `prepare_full_sync` 会为该 target 生成 deleted_project plan 清理远端前缀
//! - `Completed`: 可以清理 journal
//!
//! ## 崩溃恢复
//!
//! 启动时调用 `recover_pending_delete_transactions`：
//! - 遍历 app_meta/delete-journals/ 下所有 journal
//! - 根据 phase 和 from/trash 路径实际存在状态决定下一步
//! - 推进到 `StarMapsUnbound` 后**不** complete/cleanup，返回 `RecoveredProjectDelete`
//!   （含 change-set）供 bootstrap 写 workspace Git history
//! - bootstrap 记 history 成功后调 `ack_project_delete_history` 推进到
//!   `HistoryRecorded` → `RemoteDeleteQueued` → `Completed` 并清 journal
//!   （`RemoteDeleteQueued` 阶段幂等写 PendingDeletedTarget）

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
/// - `TombstoneWritten`: tombstone 已生成并持久化（#645 评论 5504296097 缺口3）
/// - `StarMapsUnbound`: starmap 解绑已完成
/// - `HistoryRecorded`: workspace Git history 已记录（#645 评论 5504296097 缺口2）
/// - `RemoteDeleteQueued`: PendingDeletedTarget 已落盘（#645 评论 5504296097 问题1）
/// - `Completed`: 可以清理 journal
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectDeletePhase {
    Prepared,
    WorktreeMoved,
    GitMoved,
    /// #645 评论 5504296097 缺口3：tombstone 已生成并持久化。
    ///
    /// 在 `GitMoved` 之后、`StarMapsUnbound` 之前。`write_tombstone` 成功后
    /// 立刻推进到此 phase，使重放幂等——即使死在"tombstone 已写、phase 尚未
    /// 持久化"的窗口，重放也只追加不重复（配合 `generate_tombstones` 的 upsert/skip）。
    TombstoneWritten,
    /// #645 评论 5504296097 问题2：starmap 解绑已完成。
    ///
    /// 在 `TombstoneWritten` 之后、`HistoryRecorded` 之前。把 unbind 收进事务，
    /// 避免"解绑成功但作品删除失败"或"解绑失败但作品删除成功"的半状态。
    StarMapsUnbound,
    /// #645 评论 5504296097 缺口2：workspace Git history 已记录。
    ///
    /// 在 `StarMapsUnbound` 之后、`RemoteDeleteQueued` 之前。API/bootstrap 调
    /// `record_workspace_change_set` 成功后通过 `ack_project_delete_history`
    /// 推进到此 phase。history 失败时 journal 保留在 `StarMapsUnbound`，
    /// 下次启动 recover 补记。
    HistoryRecorded,
    /// #645 评论 5504296097 问题1：PendingDeletedTarget 已落盘到
    /// `app-meta/sync/pending_deleted_targets.json`。
    ///
    /// 在 `HistoryRecorded` 之后、`Completed` 之前。`ack_project_delete_history`
    /// 推进到 `HistoryRecorded` 后，幂等调 `record_pending_deleted_target` 写
    /// PendingDeletedTarget，成功后推进到此 phase。写失败时 journal 保留在
    /// `HistoryRecorded`，下次启动 recover 补写——不让 pending target 丢失。
    RemoteDeleteQueued,
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
    /// #645 评论 5504296097 问题2：本次需要解绑的 starmap ids。
    ///
    /// 在 `StarMapsUnbound` phase 逐个执行幂等 unbind。
    /// `#[serde(default)]` 保持向后兼容：旧 journal 文件没有这个字段，
    /// 反序列化时得到空 Vec，恢复时跳过 unbind（旧 journal 的 starmap
    /// 已在旧 API 层 best-effort loop 里解绑过）。
    #[serde(default)]
    pub starmap_ids: Vec<String>,
    /// #645 评论 5504296097 问题3：发起删除的设备 ID，参与 LWW 平局决胜。
    ///
    /// `ack_project_delete_history` 和 `recover_single_journal` 用此字段
    /// 构造 `PendingDeletedTarget`，使 target tombstone 的 device_id 与
    /// `resolve_lww_path` 的 tie-break 规则一致。
    /// `#[serde(default)]` 保持向后兼容：旧 journal 没有此字段时反序列化
    /// 为空字符串（LWW tie-break 中空字符串 < 任何非空 device_id）。
    #[serde(default)]
    pub device_id: String,
    /// #645 评论 5504296097 问题2修复：删除发起来源 — 必须进入 durable journal。
    ///
    /// `ack_project_delete_history` 和 `recover_single_journal` 按 origin 分流：
    /// - `User` → 推进到 `RemoteDeleteQueued`（写 PendingDeletedTarget）；
    /// - `RemoteLifecycle` → 跳过 `RemoteDeleteQueued`，直接 `Completed`
    ///   （远端已删，不反向排队删远端）。
    /// `#[serde(default)]` 保持向后兼容：旧 journal 反序列化为 `User`。
    #[serde(default)]
    pub origin: crate::project::ProjectDeleteOrigin,
    /// 当前删除阶段。
    pub phase: ProjectDeletePhase,
}

/// 项目删除事务。
///
/// 生命周期：`new` → `prepare` → `move_worktree` → `move_git` → `write_tombstone`
/// → `unbind_starmaps` → `complete` → `cleanup_journal`。
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
    ///
    /// #645 评论 5504296097 问题2：接收 `starmap_ids`，在 `StarMapsUnbound` phase
    /// 逐个执行幂等 unbind。把 unbind 收进事务，避免半状态。
    ///
    /// #645 评论 5504296097 问题2修复：接收 `origin`，写入 durable journal，
    /// ack/recover 按 origin 分流（RemoteLifecycle 不生成 PendingDeletedTarget）。
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        project_id: &str,
        worktree_from: &Path,
        worktree_trash_root: &Path,
        git_dir_from: Option<&Path>,
        git_dir_trash_root: Option<&Path>,
        projects_root: &Path,
        app_data_root: &Path,
        starmap_ids: Vec<String>,
        device_id: &str,
        origin: crate::project::ProjectDeleteOrigin,
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
            starmap_ids,
            device_id: device_id.to_string(),
            origin,
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
    /// 调用时机：`move_git` 成功（phase == GitMoved）之后、`unbind_starmaps` 之前。
    /// 失败时返回 Err，journal 保留下次恢复继续。
    ///
    /// #645 评论 5504296097 缺口3修复：成功后推进 phase 到 `TombstoneWritten`，
    /// 使重放幂等——重放时 `generate_tombstones` 的 upsert/skip 保证不重复追加，
    /// phase 推进保证下次重放从 `TombstoneWritten` 继续。
    pub fn write_tombstone(&mut self) -> Result<()> {
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

        // #645 评论 5504296097 缺口3：tombstone 已写盘，推进 phase 到 TombstoneWritten。
        self.advance_phase(ProjectDeletePhase::TombstoneWritten)?;

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

    /// #645 评论 5504296097 问题2：解除本次删除作品的所有 StarMap 绑定。
    ///
    /// 在 `write_tombstone` 成功（phase == GitMoved）之后、`complete` 之前调用。
    /// 逐个执行幂等 unbind（`unbind_starmap_from_project` 已是幂等的：
    /// 把 `project_id` 设为 None，如果已经是 None 则无变化）。
    /// 失败时返回 Err，journal 保留下次恢复继续。
    ///
    /// 把 unbind 收进事务，避免 API 层 best-effort unbind loop 产生的半状态：
    /// - 情况 A：解绑成功，作品删除失败 → starmap 已解绑但 project 还在
    /// - 情况 B：某个解绑失败，作品删除成功 → 悬空引用
    pub fn unbind_starmaps(&mut self) -> Result<()> {
        let app_data_root = PathBuf::from(&self.journal.app_data_root);
        for sm_id in &self.journal.starmap_ids {
            // 幂等 unbind：如果 starmap 已解绑则跳过。
            // unbind 错误直接返回，不吞——半状态比保留 journal 下次恢复更糟。
            crate::starmap::unbind_starmap_from_project(&app_data_root, sm_id)?;
        }
        self.advance_phase(ProjectDeletePhase::StarMapsUnbound)?;
        Ok(())
    }

    /// #645 评论 5504296097 问题2：获取本次需要解绑的 starmap ids。
    ///
    /// 供调用方构造 change_set：每个被解绑的 starmap 都会产生
    /// `Upsert(starmaps/{id}.meta.json) + Upsert(starmaps/index.json)`。
    pub fn starmap_ids(&self) -> &[String] {
        &self.journal.starmap_ids
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

/// #645 评论 5504296097 缺口2修复：崩溃恢复后待补 history 的删除结果。
///
/// `recover_pending_delete_transactions` 返回 `Vec<RecoveredProjectDelete>`，
/// 每个元素对应一个推进到 `StarMapsUnbound` 但未记 history 的删除事务。
/// bootstrap 用 `changes` 调 `record_workspace_change_set` 写本地 history，
/// 成功后调 `ack_project_delete_history` 推进 journal 到 `HistoryRecorded` →
/// `Completed` 并清 journal。
#[derive(Debug, Clone)]
pub struct RecoveredProjectDelete {
    /// 本次删除的 journal token（用于 ack 推进 journal）。
    pub journal_token: String,
    /// 待补记到 workspace Git history 的变更集。
    pub changes: crate::storage::workspace_git::WorkspaceChangeSet,
    /// 本次被解绑的 starmap ids（供调用方刷搜索索引等）。
    pub unbound_starmap_ids: Vec<String>,
}

/// #645 评论 5504296097 缺口2修复：构造项目删除的 workspace 变更集。
///
/// `DeleteTree(projects/{project_id})` + 每个被解绑 starmap 的 meta +
/// `index.json`（若有解绑）。正常删除和崩溃恢复共用此 helper，保证两条
/// 路径产生的 change-set 一致。
pub(crate) fn build_project_delete_change_set(
    project_id: &str,
    unbound_starmap_ids: &[String],
) -> crate::storage::workspace_git::WorkspaceChangeSet {
    let project_tree_path = PathBuf::from("projects").join(project_id);
    let mut change_set =
        crate::storage::workspace_git::WorkspaceChangeSet::new().add_delete_tree(project_tree_path);
    for sm_id in unbound_starmap_ids {
        let meta_rel = PathBuf::from("starmaps").join(format!("{}.meta.json", sm_id));
        change_set = change_set.add_upsert(meta_rel);
    }
    if !unbound_starmap_ids.is_empty() {
        change_set = change_set.add_upsert(PathBuf::from("starmaps").join("index.json"));
    }
    change_set
}

/// 恢复所有待处理的删除事务。
///
/// 启动时调用，遍历 app_meta/delete-journals/ 下所有 journal，
/// 根据 phase 和 from/trash 路径实际存在状态决定下一步。
///
/// #645 评论 5504296097 缺口2修复：返回 `Vec<RecoveredProjectDelete>`，
/// 每个元素含待补 history 的 change-set。恢复时推进到 `StarMapsUnbound`
/// 但**不** complete/cleanup——把 change-set 返回给 bootstrap，由 bootstrap
/// 调 `record_workspace_change_set` 写 history 后再调 `ack_project_delete_history`
/// 推进 journal 到 `HistoryRecorded` → `Completed` 并清 journal。
///
/// `HistoryRecorded` / `Completed` phase 的 journal 直接清理（history 已记）。
pub fn recover_pending_delete_transactions(
    app_data_root: &Path,
) -> Result<Vec<RecoveredProjectDelete>> {
    let journals_dir = app_data_root.join(DELETE_JOURNALS_DIR);
    if !journals_dir.exists() {
        return Ok(Vec::new());
    }

    let mut recovered_list = Vec::new();
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

        match recover_single_journal(&path) {
            Ok(Some(recovered)) => {
                recovered_list.push(recovered);
            }
            Ok(None) => {
                // 无需补 history（已 HistoryRecorded/Completed，已清理）。
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
    Ok(recovered_list)
}

/// #645 评论 5504296097 缺口2/问题1修复：API/bootstrap 记 history 成功后推进 journal。
///
/// 读取 journal、推进 phase 到 `HistoryRecorded`，幂等写 PendingDeletedTarget
/// 到 `app-meta/sync/pending_deleted_targets.json`，推进到 `RemoteDeleteQueued` →
/// `Completed`、清 journal。
///
/// journal 文件名是 `.sujian-delete-journal-{token}`，在 `app-meta/delete-journals/` 下。
///
/// 幂等：journal 已不存在（已清理）时返回 `Ok(())`。
///
/// #645 评论 5504296097 问题1：`record_pending_deleted_target` 失败时返回 Err，
/// journal 保留在 `HistoryRecorded`，下次启动 recover 补写——不让 pending target 丢失。
pub fn ack_project_delete_history(app_data_root: &Path, journal_token: &str) -> Result<()> {
    let journal_path = app_data_root
        .join(DELETE_JOURNALS_DIR)
        .join(format!("{}{}", DELETE_JOURNAL_PREFIX, journal_token));
    if !journal_path.exists() {
        // journal 已清理（可能 recover 已处理或已 ack），幂等返回 Ok。
        return Ok(());
    }
    let content = fs::read(&journal_path)?;
    let journal: ProjectDeleteJournal = serde_json::from_slice(&content).map_err(|e| {
        crate::error::Error::Io(std::io::Error::other(format!(
            "ack_project_delete_history: parse {}: {e}",
            journal_path.display()
        )))
    })?;

    let mut tx = ProjectDeleteTransaction {
        journal,
        journal_path,
        completed: false,
    };
    // 推进到 HistoryRecorded。
    tx.advance_phase(ProjectDeletePhase::HistoryRecorded)?;

    // #645 评论 5504296097 问题2修复：按 origin 分流。
    // - User → 写 PendingDeletedTarget → RemoteDeleteQueued → Completed
    //   （下次同步清理远端）；
    // - RemoteLifecycle → 跳过 PendingDeletedTarget，直接 Completed
    //   （远端已删，不反向排队删远端）。
    match tx.journal.origin {
        crate::project::ProjectDeleteOrigin::User => {
            // #645 评论 5504296097 问题1：幂等写 PendingDeletedTarget，让 prepare_full_sync
            // 能为已删除作品生成 deleted_project target，run_transfer 走 target-delete 计划
            // 清理远端 projects/<id>/ 下所有对象。写失败返回 Err，journal 保留在
            // HistoryRecorded，下次启动 recover 补写。
            //
            // deleted_at_ms 用 journal 的 token 时间戳前缀（token 格式：{ts}_{uuid}_{pid}），
            // 解析失败时退回当前时间。provider-neutral，不写 GitHub 专用逻辑。
            let deleted_at_ms = tx
                .journal
                .token
                .split('_')
                .next()
                .and_then(|s| s.parse::<i64>().ok())
                .unwrap_or_else(|| chrono::Utc::now().timestamp_millis());
            let pending_deleted = crate::sync::types::PendingDeletedTarget::for_project(
                &tx.journal.project_id,
                deleted_at_ms,
                journal_token,
                &tx.journal.device_id,
            );
            crate::sync::pending_deleted::record_pending_deleted_target(
                app_data_root,
                pending_deleted,
            )?;

            // PendingDeletedTarget 已落盘，推进到 RemoteDeleteQueued → Completed → cleanup。
            tx.advance_phase(ProjectDeletePhase::RemoteDeleteQueued)?;
            tx.complete()?;
            tx.cleanup_journal()?;
        }
        crate::project::ProjectDeleteOrigin::RemoteLifecycle => {
            // RemoteLifecycle 永远不进入 RemoteDeleteQueued（远端已删，
            // 不反向排队删远端）。直接推进到 Completed 并清 journal。
            log::info!(
                "[ack_project_delete_history] RemoteLifecycle origin — \
                 skipping PendingDeletedTarget, advancing to Completed"
            );
            tx.complete()?;
            tx.cleanup_journal()?;
        }
    }
    Ok(())
}

/// 恢复单个 journal。
///
/// 返回 `Ok(Some(recovered))` 表示 journal 已推进到 `StarMapsUnbound`，
/// 调用方需用 `recovered.changes` 补记 history 后调 `ack_project_delete_history`。
/// 返回 `Ok(None)` 表示 journal 已清理（`HistoryRecorded`/`Completed`），无需补 history。
fn recover_single_journal(journal_path: &Path) -> Result<Option<RecoveredProjectDelete>> {
    let content = fs::read(journal_path)?;
    let journal: ProjectDeleteJournal = serde_json::from_slice(&content).map_err(|e| {
        crate::error::Error::Io(std::io::Error::other(format!(
            "recover_single_journal: parse {}: {e}",
            journal_path.display()
        )))
    })?;

    let worktree_from = PathBuf::from(&journal.worktree_from);
    let mut tx = ProjectDeleteTransaction {
        journal,
        journal_path: journal_path.to_path_buf(),
        completed: false,
    };

    match tx.journal.phase {
        ProjectDeletePhase::Prepared => {
            // journal 刚写盘，还没开始移动。
            if worktree_from.exists() {
                tx.move_worktree()?;
            } else {
                // worktree 不存在，说明已经移到 trash。
                tx.advance_phase(ProjectDeletePhase::WorktreeMoved)?;
            }
            tx.move_git()?;
            tx.write_tombstone()?;
            tx.unbind_starmaps()?;
            Ok(Some(make_recovered(&tx)))
        }
        ProjectDeletePhase::WorktreeMoved => {
            // worktree 已移到 trash，继续移动 git、写 tombstone、解绑 starmap。
            tx.move_git()?;
            tx.write_tombstone()?;
            tx.unbind_starmaps()?;
            Ok(Some(make_recovered(&tx)))
        }
        ProjectDeletePhase::GitMoved => {
            // 两边都已移到 trash，等待 tombstone 生成。
            // #645 评论 5504296097 缺口3：write_tombstone 幂等，重放不重复追加。
            tx.write_tombstone()?;
            tx.unbind_starmaps()?;
            Ok(Some(make_recovered(&tx)))
        }
        ProjectDeletePhase::TombstoneWritten => {
            // #645 评论 5504296097 缺口3：tombstone 已写，继续解绑 starmap。
            // write_tombstone 已推进到 TombstoneWritten，unbind_starmaps 幂等。
            tx.unbind_starmaps()?;
            Ok(Some(make_recovered(&tx)))
        }
        ProjectDeletePhase::StarMapsUnbound => {
            // #645 评论 5504296097 缺口2：tombstone 已生成、starmap 已解绑，
            // 但 history 还没记。返回 change-set 供 bootstrap 补记。
            Ok(Some(make_recovered(&tx)))
        }
        ProjectDeletePhase::HistoryRecorded => {
            // #645 评论 5504296097 问题1：history 已记，但 PendingDeletedTarget
            // 可能还没落盘（ack 在写 pending target 前崩溃）。
            // #645 评论 5504296097 问题2修复：按 origin 分流。
            // - User → 幂等补写 PendingDeletedTarget → RemoteDeleteQueued → Completed；
            // - RemoteLifecycle → 跳过 PendingDeletedTarget，直接 Completed
            //   （远端已删，不反向排队删远端）。
            // 写失败返回 Err，journal 保留在 HistoryRecorded，下次启动 recover 补写。
            let app_data_root = PathBuf::from(&tx.journal.app_data_root);
            match tx.journal.origin {
                crate::project::ProjectDeleteOrigin::User => {
                    let deleted_at_ms = tx
                        .journal
                        .token
                        .split('_')
                        .next()
                        .and_then(|s| s.parse::<i64>().ok())
                        .unwrap_or_else(|| chrono::Utc::now().timestamp_millis());
                    let pending_deleted = crate::sync::types::PendingDeletedTarget::for_project(
                        &tx.journal.project_id,
                        deleted_at_ms,
                        &tx.journal.token,
                        &tx.journal.device_id,
                    );
                    crate::sync::pending_deleted::record_pending_deleted_target(
                        &app_data_root,
                        pending_deleted,
                    )?;
                    tx.advance_phase(ProjectDeletePhase::RemoteDeleteQueued)?;
                    tx.complete()?;
                    tx.cleanup_journal()?;
                }
                crate::project::ProjectDeleteOrigin::RemoteLifecycle => {
                    log::info!(
                        "[recover_single_journal] RemoteLifecycle origin — \
                         skipping PendingDeletedTarget, advancing to Completed"
                    );
                    tx.complete()?;
                    tx.cleanup_journal()?;
                }
            }
            Ok(None)
        }
        ProjectDeletePhase::RemoteDeleteQueued => {
            // #645 评论 5504296097 问题1：PendingDeletedTarget 已落盘，
            // 推进到 Completed 并清 journal。
            tx.complete()?;
            tx.cleanup_journal()?;
            Ok(None)
        }
        ProjectDeletePhase::Completed => {
            // #644 评论 5496728184 缺陷2修复：Completed 必须用 durable cleanup 范式
            // （与 cleanup_journal 一致），不能吞删除错误。
            // 删除失败时返回 Err，不能声称"已恢复完成"。
            if journal_path.exists() {
                fs::remove_file(journal_path)?;
                // fsync journal 父目录，持久化删除的目录项。
                if let Some(parent) = journal_path.parent() {
                    crate::storage::sync_dir(parent)?;
                }
            }
            Ok(None)
        }
    }
}

/// 从已推进到 `StarMapsUnbound` 的事务构造 `RecoveredProjectDelete`。
fn make_recovered(tx: &ProjectDeleteTransaction) -> RecoveredProjectDelete {
    let unbound_starmap_ids = tx.journal.starmap_ids.clone();
    let changes = build_project_delete_change_set(&tx.journal.project_id, &unbound_starmap_ids);
    RecoveredProjectDelete {
        journal_token: tx.journal.token.clone(),
        changes,
        unbound_starmap_ids,
    }
}

#[cfg(test)]
mod tests;
