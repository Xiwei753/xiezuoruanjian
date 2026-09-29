//! # 星图子嵌入事务（Crash-safe Child Embed Transaction）
//!
//! 解决 "子星图已写盘、父 Embed 还没落盘" 的孤儿根星图问题。
//!
//! ## 问题背景
//!
//! 旧 `create_starmap_child_embed()` 依次调用 `create_starmap()` 和
//! `add_starmap_embed()`，不是 crash-safe 的：如果进程死在"子图已写盘、
//! 父 Embed 还没落盘"之间，会留下孤儿根星图。
//!
//! ## 解决方案
//!
//! 在创建子星图之前先写 pending journal（含 child_starmap_id / host_starmap_id /
//! title / position / phase），然后执行操作，完成后标 completed / 删除 journal。
//! 启动恢复时发现 pending journal：
//! - child 已存在、Embed 不存在：按 journal 补 Embed
//! - Embed 已存在：清 journal
//! - child 根本没创建出来：清 journal
//!
//! ## 状态机
//!
//! ```text
//! Pending → ChildCreated → EmbedAdded → Completed
//! ```
//!
//! - `Pending`: journal 已落盘，尚未创建子星图
//! - `ChildCreated`: 子星图 meta/index 已写盘，尚未添加 Embed
//! - `EmbedAdded`: Embed 已添加到宿主图并 flush
//! - `Completed`: 可以清理 journal

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use crate::error::Result;

/// 子嵌入 journal 文件名前缀。
pub const CHILD_EMBED_JOURNAL_PREFIX: &str = ".sujian-child-embed-journal-";

/// 子嵌入 journal 所在目录（app_meta 下）。
pub const CHILD_EMBED_JOURNALS_DIR: &str = "app-meta/child-embed-journals";

/// 星图子嵌入事务阶段。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StarMapChildEmbedPhase {
    /// journal 已落盘，尚未创建子星图。
    Pending,
    /// 子星图 meta/index 已写盘，尚未添加 Embed。
    ChildCreated,
    /// Embed 已添加到宿主图并 flush。
    EmbedAdded,
    /// 可以清理 journal。
    Completed,
}

/// 星图子嵌入 journal。
///
/// 在创建子星图之前先写到应用私有目录（app_meta/child-embed-journals/），
/// 确保崩溃恢复能看到待完成状态。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StarMapChildEmbedJournal {
    /// 本次事务的唯一 ID。
    pub tx_id: String,
    /// 宿主星图 ID（父图）。
    pub host_starmap_id: String,
    /// 子星图 ID（预先生成，用于 create_starmap_with_id）。
    pub child_starmap_id: String,
    /// 子星图标题。
    pub title: String,
    /// Embed 在宿主图中的位置。
    pub position: crate::starmap::types::StarMapPoint,
    /// 当前事务阶段。
    pub phase: StarMapChildEmbedPhase,
}

/// 崩溃恢复后待补 history 的子嵌入结果。
///
/// `recover_pending_child_embed_transactions` 返回 `Vec<RecoveredStarMapChildEmbed>`，
/// 每个元素对应一个推进到 `EmbedAdded` 但未记 history 的子嵌入事务。
/// bootstrap 用 `changes` 调 `record_workspace_change_set` 写本地 history，
/// 成功后推进 journal 到 `Completed` 并清 journal。
#[derive(Debug, Clone)]
pub struct RecoveredStarMapChildEmbed {
    /// 本次子嵌入事务的 journal tx_id（用于 ack 推进 journal）。
    pub journal_token: String,
    /// 待补记到 workspace Git history 的变更集。
    pub changes: crate::storage::workspace_git::WorkspaceChangeSet,
}

/// 星图子嵌入事务。
///
/// 生命周期：`new` → `prepare` → `create_child` → `add_embed` → `complete` →
/// `cleanup_journal`。未完成的 journal 留给 `recover_pending_child_embed_transactions`
/// 处理。
pub struct StarMapChildEmbedTransaction {
    journal: StarMapChildEmbedJournal,
    journal_path: PathBuf,
    completed: bool,
}

impl StarMapChildEmbedTransaction {
    /// 创建新的子嵌入事务。
    ///
    /// 接收预先生成的 `child_starmap_id`（格式 `sm_{uuid}`），在 journal 中记录
    /// 宿主星图、子星图、标题和位置信息。
    pub fn new(
        host_starmap_id: &str,
        child_starmap_id: &str,
        title: &str,
        position: crate::starmap::types::StarMapPoint,
        app_data_root: &Path,
    ) -> Self {
        let tx_id = format!(
            "{}_{}",
            chrono::Utc::now().timestamp_millis(),
            uuid::Uuid::new_v4()
        );

        let journal = StarMapChildEmbedJournal {
            tx_id: tx_id.clone(),
            host_starmap_id: host_starmap_id.to_string(),
            child_starmap_id: child_starmap_id.to_string(),
            title: title.to_string(),
            position,
            phase: StarMapChildEmbedPhase::Pending,
        };

        let journal_path = app_data_root
            .join(CHILD_EMBED_JOURNALS_DIR)
            .join(format!("{}{}", CHILD_EMBED_JOURNAL_PREFIX, tx_id));

        Self {
            journal,
            journal_path,
            completed: false,
        }
    }

    /// 获取预先生成的子星图 ID。
    pub fn child_starmap_id(&self) -> &str {
        &self.journal.child_starmap_id
    }

    /// 获取事务 ID。
    pub fn tx_id(&self) -> &str {
        &self.journal.tx_id
    }

    /// 准备阶段：写 journal 到 app_meta/child-embed-journals/。
    ///
    /// 在创建子星图之前先写 journal，确保崩溃恢复能看到待完成状态。
    pub fn prepare(&mut self) -> Result<()> {
        let content = serde_json::to_vec(&self.journal).map_err(|e| {
            crate::error::Error::Io(std::io::Error::other(format!(
                "StarMapChildEmbedTransaction::prepare: serialize: {e}"
            )))
        })?;
        crate::storage::atomic_write_bytes(&self.journal_path, &content)?;
        Ok(())
    }

    /// 推进 phase 到 ChildCreated（子星图已创建）。
    ///
    /// 调用方在 `create_starmap_with_id()` 成功后调用此方法。
    pub fn mark_child_created(&mut self) -> Result<()> {
        self.advance_phase(StarMapChildEmbedPhase::ChildCreated)?;
        Ok(())
    }

    /// 推进 phase 到 EmbedAdded（Embed 已添加并 flush）。
    ///
    /// 调用方在 `add_starmap_embed()` + flush 成功后调用此方法。
    pub fn mark_embed_added(&mut self) -> Result<()> {
        self.advance_phase(StarMapChildEmbedPhase::EmbedAdded)?;
        Ok(())
    }

    /// 完成事务：推进 phase 到 Completed。
    pub fn complete(&mut self) -> Result<()> {
        self.advance_phase(StarMapChildEmbedPhase::Completed)?;
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
            if let Some(parent) = self.journal_path.parent() {
                crate::storage::sync_dir(parent)?;
            }
        }
        Ok(())
    }

    /// 推进 phase 并持久化 journal。
    fn advance_phase(&mut self, phase: StarMapChildEmbedPhase) -> Result<()> {
        self.journal.phase = phase;
        let content = serde_json::to_vec(&self.journal).map_err(|e| {
            crate::error::Error::Io(std::io::Error::other(format!(
                "StarMapChildEmbedTransaction::advance_phase: serialize: {e}"
            )))
        })?;
        crate::storage::atomic_write_bytes(&self.journal_path, &content)?;
        Ok(())
    }
}

/// 恢复所有待处理的子嵌入事务。
///
/// 启动时调用，遍历 app_meta/child-embed-journals/ 下所有 journal，
/// 根据 phase 和磁盘事实决定下一步：
/// - `Pending`：检查磁盘事实——child meta/index 是否已存在
/// - `ChildCreated`：child 已存在但 Embed 可能还没添加，补 Embed
/// - `EmbedAdded`：Embed 已添加，返回 change-set 供 bootstrap 补 history
/// - `Completed`：直接清 journal
///
/// 返回 `Vec<RecoveredStarMapChildEmbed>`，
/// 每个元素含待补 history 的 change-set。恢复时推进到 `EmbedAdded`
/// 但**不** complete/cleanup——把 change-set 返回给 bootstrap，由 bootstrap
/// 调 `record_workspace_change_set` 写 history 后再推进 journal 到 `Completed`
/// 并清 journal。
///
/// `Completed` phase 的 journal 直接清理（history 已记）。
pub fn recover_pending_child_embed_transactions(
    app_data_root: &Path,
) -> Result<Vec<RecoveredStarMapChildEmbed>> {
    let journals_dir = app_data_root.join(CHILD_EMBED_JOURNALS_DIR);
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
        if !file_name.starts_with(CHILD_EMBED_JOURNAL_PREFIX) {
            continue;
        }

        match recover_single_child_embed_journal(&path, app_data_root) {
            Ok(Some(recovered)) => {
                recovered_list.push(recovered);
            }
            Ok(None) => {
                // 无需补 history（已 Completed，已清理）。
            }
            Err(e) => {
                // 恢复失败，保留 journal，下次重启继续。
                log::error!(
                    "[recover_pending_child_embed_transactions] failed to recover {}: {}",
                    path.display(),
                    e
                );
            }
        }
    }
    Ok(recovered_list)
}

/// Ack 子嵌入 journal 的 history 已记录，推进到 `Completed` 并清理 journal。
///
/// bootstrap 在 `record_workspace_change_set` 成功后调用此函数，
/// 把 journal 从 `EmbedAdded` 推进到 `Completed` 并删除 journal 文件。
///
/// 幂等：journal 已不存在（已清理）时返回 `Ok(())`。
///
/// journal 文件名是 `.sujian-child-embed-journal-{tx_id}`，
/// 在 `app-meta/child-embed-journals/` 下。
pub fn ack_child_embed_history(app_data_root: &Path, journal_token: &str) -> Result<()> {
    let journal_path = app_data_root
        .join(CHILD_EMBED_JOURNALS_DIR)
        .join(format!("{}{}", CHILD_EMBED_JOURNAL_PREFIX, journal_token));
    if !journal_path.exists() {
        // journal 已清理（可能 recover 已处理或已 ack），幂等返回 Ok。
        return Ok(());
    }
    let content = fs::read(&journal_path)?;
    let journal: StarMapChildEmbedJournal = serde_json::from_slice(&content).map_err(|e| {
        crate::error::Error::Io(std::io::Error::other(format!(
            "ack_child_embed_history: parse {}: {e}",
            journal_path.display()
        )))
    })?;

    let mut tx = StarMapChildEmbedTransaction {
        journal,
        journal_path,
        completed: false,
    };
    // 推进到 Completed 并清理 journal。
    tx.complete()?;
    tx.cleanup_journal()?;
    Ok(())
}

/// 恢复单个子嵌入 journal。
///
/// 返回 `Ok(Some(recovered))` 表示 journal 已推进到 `EmbedAdded`，
/// 调用方需用 `recovered.changes` 补记 history 后推进 journal 到 `Completed` 并清 journal。
/// 返回 `Ok(None)` 表示 journal 已清理（`Completed`），无需补 history。
fn recover_single_child_embed_journal(
    journal_path: &Path,
    app_data_root: &Path,
) -> Result<Option<RecoveredStarMapChildEmbed>> {
    let content = fs::read(journal_path)?;
    let journal: StarMapChildEmbedJournal = serde_json::from_slice(&content).map_err(|e| {
        crate::error::Error::Io(std::io::Error::other(format!(
            "recover_single_child_embed_journal: parse {}: {e}",
            journal_path.display()
        )))
    })?;

    match journal.phase {
        StarMapChildEmbedPhase::Pending => {
            // journal 刚写盘，但 phase 是提示，磁盘事实才是恢复依据。
            // 检查磁盘上 child 是否已经创建。
            let child_meta_path = app_data_root
                .join("starmaps")
                .join(format!("{}.meta.json", journal.child_starmap_id));
            let child_meta_exists = child_meta_path.exists();
            let index_has_child = check_index_has_child(app_data_root, &journal.child_starmap_id)?;

            if !child_meta_exists && !index_has_child {
                // child meta 不存在、index 也没有 child：child 确实没创建，清 journal。
                log::info!(
                    "[recover_single_child_embed_journal] Pending — child not created \
                     (no meta, no index entry), clearing journal tx_id={}",
                    journal.tx_id
                );
                fs::remove_file(journal_path)?;
                if let Some(parent) = journal_path.parent() {
                    crate::storage::sync_dir(parent)?;
                }
                return Ok(None);
            }

            if child_meta_exists {
                // child meta 已存在：child 已经创建，需要补 Embed。
                // 推进 phase 到 ChildCreated，然后走 ChildCreated 的恢复逻辑。
                log::info!(
                    "[recover_single_child_embed_journal] Pending but child meta exists, \
                     advancing to ChildCreated and repairing tx_id={}",
                    journal.tx_id
                );
                let mut tx = StarMapChildEmbedTransaction {
                    journal,
                    journal_path: journal_path.to_path_buf(),
                    completed: false,
                };
                tx.advance_phase(StarMapChildEmbedPhase::ChildCreated)?;
                return recover_child_created_or_embed_added(
                    app_data_root,
                    &tx.journal,
                    journal_path,
                );
            }

            // index 有 child、meta 不存在：半状态，修正掉 index 中的 child 引用。
            log::warn!(
                "[recover_single_child_embed_journal] Pending — index has child {} but meta \
                 missing, removing stale index entry, clearing journal tx_id={}",
                journal.child_starmap_id,
                journal.tx_id
            );
            remove_child_from_index(app_data_root, &journal.child_starmap_id)?;
            fs::remove_file(journal_path)?;
            if let Some(parent) = journal_path.parent() {
                crate::storage::sync_dir(parent)?;
            }
            Ok(None)
        }
        StarMapChildEmbedPhase::ChildCreated => {
            // child 已创建（phase 提示），检查 Embed 是否存在。
            let child_meta_path = app_data_root
                .join("starmaps")
                .join(format!("{}.meta.json", journal.child_starmap_id));

            if !child_meta_path.exists() {
                // child meta 不存在——child 根本没创建出来，清 journal。
                log::info!(
                    "[recover_single_child_embed_journal] ChildCreated but child meta missing, \
                     clearing journal tx_id={}",
                    journal.tx_id
                );
                fs::remove_file(journal_path)?;
                if let Some(parent) = journal_path.parent() {
                    crate::storage::sync_dir(parent)?;
                }
                return Ok(None);
            }

            recover_child_created_or_embed_added(app_data_root, &journal, journal_path)
        }
        StarMapChildEmbedPhase::EmbedAdded => {
            // Embed 已添加，返回 change-set 供 bootstrap 补 history。
            // 不 complete/cleanup，由 bootstrap 记 history 后推进。
            log::info!(
                "[recover_single_child_embed_journal] EmbedAdded, returning change-set \
                 for history tx_id={}",
                journal.tx_id
            );
            let changes = build_child_embed_change_set(app_data_root, &journal, Vec::new());
            Ok(Some(RecoveredStarMapChildEmbed {
                journal_token: journal.tx_id.clone(),
                changes,
            }))
        }
        StarMapChildEmbedPhase::Completed => {
            // 已完成，清 journal。
            log::info!(
                "[recover_single_child_embed_journal] Completed, clearing journal tx_id={}",
                journal.tx_id
            );
            fs::remove_file(journal_path)?;
            if let Some(parent) = journal_path.parent() {
                crate::storage::sync_dir(parent)?;
            }
            Ok(None)
        }
    }
}

/// 处理 ChildCreated 阶段的恢复逻辑（也用于 Pending 阶段发现 child 已存在时）。
///
/// 检查 Embed 是否已在宿主图中：
/// - Embed 已存在：推进到 EmbedAdded，返回 change-set（不 complete/cleanup）
/// - Embed 不存在：补建 Embed，推进到 EmbedAdded，返回 change-set（不 complete/cleanup）
fn recover_child_created_or_embed_added(
    app_data_root: &Path,
    journal: &StarMapChildEmbedJournal,
    journal_path: &Path,
) -> Result<Option<RecoveredStarMapChildEmbed>> {
    let embed_exists = check_embed_exists(
        app_data_root,
        &journal.host_starmap_id,
        &journal.child_starmap_id,
    )?;

    let mut tx = StarMapChildEmbedTransaction {
        journal: journal.clone(),
        journal_path: journal_path.to_path_buf(),
        completed: false,
    };

    if embed_exists {
        // Embed 已存在，推进到 EmbedAdded，返回 change-set。
        log::info!(
            "[recover_child_created_or_embed_added] embed already exists, advancing to \
             EmbedAdded tx_id={}",
            journal.tx_id
        );
        tx.advance_phase(StarMapChildEmbedPhase::EmbedAdded)?;
        let changes = build_child_embed_change_set(app_data_root, journal, Vec::new());
        Ok(Some(RecoveredStarMapChildEmbed {
            journal_token: journal.tx_id.clone(),
            changes,
        }))
    } else {
        // Embed 不存在，需要补 Embed。
        log::info!(
            "[recover_child_created_or_embed_added] embed missing, repairing tx_id={}",
            journal.tx_id
        );
        let flush_paths = repair_embed_for_child(app_data_root, journal)?;
        tx.advance_phase(StarMapChildEmbedPhase::EmbedAdded)?;
        let changes = build_child_embed_change_set(app_data_root, journal, flush_paths);
        Ok(Some(RecoveredStarMapChildEmbed {
            journal_token: journal.tx_id.clone(),
            changes,
        }))
    }
}

/// 构造子嵌入事务的 workspace 变更集。
///
/// 包含：
/// - child meta 路径：`starmaps/{child_starmap_id}.meta.json`
/// - starmap index 路径：`starmaps/index.json`
/// - host store flush 后的真实文件路径（strip prefix app_data_root 转为相对路径）
fn build_child_embed_change_set(
    app_data_root: &Path,
    journal: &StarMapChildEmbedJournal,
    flush_paths: Vec<PathBuf>,
) -> crate::storage::workspace_git::WorkspaceChangeSet {
    let mut change_set = crate::storage::workspace_git::WorkspaceChangeSet::new()
        .add_upsert(
            PathBuf::from("starmaps").join(format!("{}.meta.json", journal.child_starmap_id)),
        )
        .add_upsert(PathBuf::from("starmaps").join("index.json"));

    for path in flush_paths {
        let rel_path = path
            .strip_prefix(app_data_root)
            .unwrap_or(&path)
            .to_path_buf();
        change_set = change_set.add_upsert(rel_path);
    }

    change_set
}

/// 检查 starmaps/index.json 中是否包含指定的 child_starmap_id。
fn check_index_has_child(app_data_root: &Path, child_starmap_id: &str) -> Result<bool> {
    let index_path = app_data_root.join("starmaps").join("index.json");
    if !index_path.exists() {
        return Ok(false);
    }
    let content = fs::read_to_string(&index_path)?;
    let idx: crate::starmap::StarMapIndexRecord = serde_json::from_str(&content)?;
    Ok(idx.starmap_ids.iter().any(|id| id == child_starmap_id))
}

/// 从 starmaps/index.json 中移除指定的 child_starmap_id。
fn remove_child_from_index(app_data_root: &Path, child_starmap_id: &str) -> Result<()> {
    let index_path = app_data_root.join("starmaps").join("index.json");
    if !index_path.exists() {
        return Ok(());
    }
    let content = fs::read_to_string(&index_path)?;
    let mut idx: crate::starmap::StarMapIndexRecord = serde_json::from_str(&content)?;
    idx.starmap_ids.retain(|id| id != child_starmap_id);
    idx.updated_at = crate::starmap::now_epoch();
    let new_content = serde_json::to_string_pretty(&idx)?;
    crate::storage::atomic_write_string(&index_path, &new_content)?;
    Ok(())
}

/// 检查宿主星图中是否已有指向 child_starmap_id 的 Embed。
fn check_embed_exists(
    app_data_root: &Path,
    host_starmap_id: &str,
    child_starmap_id: &str,
) -> Result<bool> {
    let mut store = crate::starmap::store::StarMapStore::new(app_data_root, host_starmap_id);
    store.load_full()?;
    let graph = store.to_starmap_graph();
    Ok(graph
        .embeds
        .iter()
        .any(|e| e.target_starmap_id == child_starmap_id))
}

/// 为已创建的子星图补建 Embed 关系。
///
/// 在宿主星图中创建一个指向 child 的 Embed，使用 journal 中记录的 title 和 position。
///
/// 返回 `store.flush()` 产生的真实文件路径列表，供调用方构造 `WorkspaceChangeSet`。
fn repair_embed_for_child(
    app_data_root: &Path,
    journal: &StarMapChildEmbedJournal,
) -> Result<Vec<PathBuf>> {
    let now = crate::starmap::now_epoch();
    let embed = crate::starmap::types::StarMapEmbed {
        instance_id: format!("em_{}", uuid::Uuid::new_v4()),
        target_starmap_id: journal.child_starmap_id.clone(),
        label: Some(journal.title.clone()),
        position: journal.position.clone(),
        host_path: crate::starmap::types::StarMapTargetPath {
            starmap_id: journal.host_starmap_id.clone(),
            segments: Vec::new(),
            target: crate::starmap::semantic::StarMapTargetDetail::Starmap,
        },
        provenance: crate::starmap::semantic::StarMapProvenance::default(),
        created_at: now,
        updated_at: now,
    };

    let mut store =
        crate::starmap::store::StarMapStore::new(app_data_root, &journal.host_starmap_id);
    store.load_full()?;
    store.upsert_embed(embed);
    let changed_paths = store.flush()?;
    Ok(changed_paths)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn setup_temp_dir() -> tempfile::TempDir {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("starmaps")).unwrap();
        dir
    }

    #[test]
    fn test_journal_prepare_and_cleanup() {
        let dir = setup_temp_dir();
        let app_data_root = dir.path();

        let mut tx = StarMapChildEmbedTransaction::new(
            "sm_host",
            "sm_child",
            "Child Title",
            crate::starmap::types::StarMapPoint::default(),
            app_data_root,
        );

        // prepare 写 journal 到磁盘
        tx.prepare().unwrap();
        let journal_path = tx.journal_path.clone();
        assert!(journal_path.exists());

        // complete + cleanup 删除 journal
        tx.complete().unwrap();
        tx.cleanup_journal().unwrap();
        assert!(!journal_path.exists());
    }

    #[test]
    fn test_journal_advance_phase_persists() {
        let dir = setup_temp_dir();
        let app_data_root = dir.path();

        let mut tx = StarMapChildEmbedTransaction::new(
            "sm_host",
            "sm_child",
            "Child Title",
            crate::starmap::types::StarMapPoint::default(),
            app_data_root,
        );

        tx.prepare().unwrap();
        tx.mark_child_created().unwrap();

        // 重新读取 journal 验证 phase 已持久化
        let content = fs::read(&tx.journal_path).unwrap();
        let journal: StarMapChildEmbedJournal = serde_json::from_slice(&content).unwrap();
        assert_eq!(journal.phase, StarMapChildEmbedPhase::ChildCreated);

        tx.mark_embed_added().unwrap();
        let content = fs::read(&tx.journal_path).unwrap();
        let journal: StarMapChildEmbedJournal = serde_json::from_slice(&content).unwrap();
        assert_eq!(journal.phase, StarMapChildEmbedPhase::EmbedAdded);

        tx.complete().unwrap();
        tx.cleanup_journal().unwrap();
    }

    #[test]
    fn test_recover_pending_no_journals() {
        let dir = setup_temp_dir();
        let app_data_root = dir.path();

        // 没有任何 journal 文件，恢复应该成功且返回空列表
        let recovered = recover_pending_child_embed_transactions(app_data_root).unwrap();
        assert!(recovered.is_empty());
    }

    #[test]
    fn test_recover_pending_phase_clears_journal_when_no_child() {
        let dir = setup_temp_dir();
        let app_data_root = dir.path();

        let mut tx = StarMapChildEmbedTransaction::new(
            "sm_host",
            "sm_child",
            "Child Title",
            crate::starmap::types::StarMapPoint::default(),
            app_data_root,
        );
        tx.prepare().unwrap();
        assert!(tx.journal_path.exists());

        // 模拟崩溃：直接恢复，不推进 phase。
        // Pending + 磁盘上无 child meta、无 index → 清 journal。
        let recovered = recover_pending_child_embed_transactions(app_data_root).unwrap();
        assert!(recovered.is_empty());

        // Pending phase 且磁盘上无 child → 清除 journal
        assert!(!tx.journal_path.exists());
    }

    #[test]
    fn test_recover_pending_phase_with_child_exists_repairs_embed() {
        let dir = setup_temp_dir();
        let app_data_root = dir.path();

        // 先创建宿主星图和子星图（模拟崩溃窗口：journal Pending 但 child 已落盘）
        let host_meta = crate::starmap::create_starmap(app_data_root, "Host", "", None).unwrap();
        let child_meta = crate::starmap::create_starmap(app_data_root, "Child", "", None).unwrap();

        // 写一个 Pending phase 的 journal（模拟崩溃在 mark_child_created 之前）
        let mut tx = StarMapChildEmbedTransaction::new(
            &host_meta.starmap_id,
            &child_meta.starmap_id,
            "Child",
            crate::starmap::types::StarMapPoint::default(),
            app_data_root,
        );
        tx.prepare().unwrap();
        // 不调 mark_child_created，phase 仍是 Pending

        // 恢复：Pending 但磁盘上 child 已存在 → 补 Embed，返回 RecoveredStarMapChildEmbed
        let recovered = recover_pending_child_embed_transactions(app_data_root).unwrap();
        assert_eq!(recovered.len(), 1);
        assert_eq!(recovered[0].journal_token, tx.tx_id());

        // journal 不应被删除（推进到 EmbedAdded，等待 bootstrap 补 history）
        assert!(tx.journal_path.exists());

        // 验证 journal phase 已推进到 EmbedAdded
        let content = fs::read(&tx.journal_path).unwrap();
        let journal: StarMapChildEmbedJournal = serde_json::from_slice(&content).unwrap();
        assert_eq!(journal.phase, StarMapChildEmbedPhase::EmbedAdded);

        // 验证 embed 已被补建
        let mut store =
            crate::starmap::store::StarMapStore::new(app_data_root, &host_meta.starmap_id);
        store.load_full().unwrap();
        let graph = store.to_starmap_graph();
        assert_eq!(graph.embeds.len(), 1);
        assert_eq!(graph.embeds[0].target_starmap_id, child_meta.starmap_id);
    }

    #[test]
    fn test_recover_pending_phase_index_has_child_but_no_meta() {
        let dir = setup_temp_dir();
        let app_data_root = dir.path();

        // 创建宿主星图
        let host_meta = crate::starmap::create_starmap(app_data_root, "Host", "", None).unwrap();

        // 手动在 index.json 中加入一个不存在的 child id（模拟半状态）
        let index_path = app_data_root.join("starmaps").join("index.json");
        let content = fs::read_to_string(&index_path).unwrap();
        let mut idx: crate::starmap::StarMapIndexRecord = serde_json::from_str(&content).unwrap();
        idx.starmap_ids.push("sm_half_child".to_string());
        let new_content = serde_json::to_string_pretty(&idx).unwrap();
        crate::storage::atomic_write_string(&index_path, &new_content).unwrap();

        // 写一个 Pending phase 的 journal
        let mut tx = StarMapChildEmbedTransaction::new(
            &host_meta.starmap_id,
            "sm_half_child",
            "Child",
            crate::starmap::types::StarMapPoint::default(),
            app_data_root,
        );
        tx.prepare().unwrap();

        // 恢复：Pending + index 有 child 但 meta 不存在 → 修正 index，清 journal
        let recovered = recover_pending_child_embed_transactions(app_data_root).unwrap();
        assert!(recovered.is_empty());
        assert!(!tx.journal_path.exists());

        // 验证 index 中的半状态 child 引用已被移除
        let content = fs::read_to_string(&index_path).unwrap();
        let idx: crate::starmap::StarMapIndexRecord = serde_json::from_str(&content).unwrap();
        assert!(!idx.starmap_ids.iter().any(|id| id == "sm_half_child"));
    }

    #[test]
    fn test_recover_completed_clears_journal() {
        let dir = setup_temp_dir();
        let app_data_root = dir.path();

        let mut tx = StarMapChildEmbedTransaction::new(
            "sm_host",
            "sm_child",
            "Child Title",
            crate::starmap::types::StarMapPoint::default(),
            app_data_root,
        );
        tx.prepare().unwrap();
        tx.complete().unwrap();
        // 不调 cleanup_journal，模拟崩溃

        let recovered = recover_pending_child_embed_transactions(app_data_root).unwrap();
        assert!(recovered.is_empty());
        assert!(!tx.journal_path.exists());
    }

    #[test]
    fn test_recover_child_created_embed_exists_returns_change_set() {
        let dir = setup_temp_dir();
        let app_data_root = dir.path();

        // 先创建宿主星图和子星图
        let host_meta = crate::starmap::create_starmap(app_data_root, "Host", "", None).unwrap();
        let child_meta = crate::starmap::create_starmap(app_data_root, "Child", "", None).unwrap();

        // 在宿主图中添加 embed 指向 child
        let now = crate::starmap::now_epoch();
        let embed = crate::starmap::types::StarMapEmbed {
            instance_id: format!("em_{}", uuid::Uuid::new_v4()),
            target_starmap_id: child_meta.starmap_id.clone(),
            label: Some("Child".to_string()),
            position: crate::starmap::types::StarMapPoint::default(),
            host_path: crate::starmap::types::StarMapTargetPath {
                starmap_id: host_meta.starmap_id.clone(),
                segments: Vec::new(),
                target: crate::starmap::semantic::StarMapTargetDetail::Starmap,
            },
            provenance: crate::starmap::semantic::StarMapProvenance::default(),
            created_at: now,
            updated_at: now,
        };
        let mut store =
            crate::starmap::store::StarMapStore::new(app_data_root, &host_meta.starmap_id);
        store.load_full().unwrap();
        store.upsert_embed(embed);
        store.flush().unwrap();

        // 写一个 ChildCreated phase 的 journal
        let mut tx = StarMapChildEmbedTransaction::new(
            &host_meta.starmap_id,
            &child_meta.starmap_id,
            "Child",
            crate::starmap::types::StarMapPoint::default(),
            app_data_root,
        );
        tx.prepare().unwrap();
        tx.mark_child_created().unwrap();

        // 恢复：embed 已存在 → 推进到 EmbedAdded，返回 change-set，不删 journal
        let recovered = recover_pending_child_embed_transactions(app_data_root).unwrap();
        assert_eq!(recovered.len(), 1);
        assert_eq!(recovered[0].journal_token, tx.tx_id());

        // journal 不应被删除（等待 bootstrap 补 history）
        assert!(tx.journal_path.exists());

        // 验证 journal phase 已推进到 EmbedAdded
        let content = fs::read(&tx.journal_path).unwrap();
        let journal: StarMapChildEmbedJournal = serde_json::from_slice(&content).unwrap();
        assert_eq!(journal.phase, StarMapChildEmbedPhase::EmbedAdded);
    }

    #[test]
    fn test_recover_child_created_embed_missing_repairs_and_returns_change_set() {
        let dir = setup_temp_dir();
        let app_data_root = dir.path();

        // 创建宿主星图和子星图，但不添加 embed
        let host_meta = crate::starmap::create_starmap(app_data_root, "Host", "", None).unwrap();
        let child_meta = crate::starmap::create_starmap(app_data_root, "Child", "", None).unwrap();

        // 写一个 ChildCreated phase 的 journal
        let mut tx = StarMapChildEmbedTransaction::new(
            &host_meta.starmap_id,
            &child_meta.starmap_id,
            "Child",
            crate::starmap::types::StarMapPoint::default(),
            app_data_root,
        );
        tx.prepare().unwrap();
        tx.mark_child_created().unwrap();

        // 恢复：embed 不存在 → 补建 embed，推进到 EmbedAdded，返回 change-set
        let recovered = recover_pending_child_embed_transactions(app_data_root).unwrap();
        assert_eq!(recovered.len(), 1);
        assert_eq!(recovered[0].journal_token, tx.tx_id());

        // journal 不应被删除（等待 bootstrap 补 history）
        assert!(tx.journal_path.exists());

        // 验证 journal phase 已推进到 EmbedAdded
        let content = fs::read(&tx.journal_path).unwrap();
        let journal: StarMapChildEmbedJournal = serde_json::from_slice(&content).unwrap();
        assert_eq!(journal.phase, StarMapChildEmbedPhase::EmbedAdded);

        // 验证 embed 已被补建
        let mut store =
            crate::starmap::store::StarMapStore::new(app_data_root, &host_meta.starmap_id);
        store.load_full().unwrap();
        let graph = store.to_starmap_graph();
        assert_eq!(graph.embeds.len(), 1);
        assert_eq!(graph.embeds[0].target_starmap_id, child_meta.starmap_id);
    }

    #[test]
    fn test_recover_child_created_child_meta_missing_clears() {
        let dir = setup_temp_dir();
        let app_data_root = dir.path();

        // 只创建宿主星图，不创建子星图
        let host_meta = crate::starmap::create_starmap(app_data_root, "Host", "", None).unwrap();

        // 写一个 ChildCreated phase 的 journal（模拟 child 创建失败）
        let mut tx = StarMapChildEmbedTransaction::new(
            &host_meta.starmap_id,
            "sm_nonexistent_child",
            "Child",
            crate::starmap::types::StarMapPoint::default(),
            app_data_root,
        );
        tx.prepare().unwrap();
        tx.mark_child_created().unwrap();

        // 恢复：child meta 不存在 → 清 journal
        let recovered = recover_pending_child_embed_transactions(app_data_root).unwrap();
        assert!(recovered.is_empty());
        assert!(!tx.journal_path.exists());
    }

    #[test]
    fn test_recover_embed_added_returns_change_set() {
        let dir = setup_temp_dir();
        let app_data_root = dir.path();

        // 创建宿主星图和子星图，并添加 embed
        let host_meta = crate::starmap::create_starmap(app_data_root, "Host", "", None).unwrap();
        let child_meta = crate::starmap::create_starmap(app_data_root, "Child", "", None).unwrap();

        let now = crate::starmap::now_epoch();
        let embed = crate::starmap::types::StarMapEmbed {
            instance_id: format!("em_{}", uuid::Uuid::new_v4()),
            target_starmap_id: child_meta.starmap_id.clone(),
            label: Some("Child".to_string()),
            position: crate::starmap::types::StarMapPoint::default(),
            host_path: crate::starmap::types::StarMapTargetPath {
                starmap_id: host_meta.starmap_id.clone(),
                segments: Vec::new(),
                target: crate::starmap::semantic::StarMapTargetDetail::Starmap,
            },
            provenance: crate::starmap::semantic::StarMapProvenance::default(),
            created_at: now,
            updated_at: now,
        };
        let mut store =
            crate::starmap::store::StarMapStore::new(app_data_root, &host_meta.starmap_id);
        store.load_full().unwrap();
        store.upsert_embed(embed);
        store.flush().unwrap();

        // 写一个 EmbedAdded phase 的 journal（模拟崩溃在 complete 之前）
        let mut tx = StarMapChildEmbedTransaction::new(
            &host_meta.starmap_id,
            &child_meta.starmap_id,
            "Child",
            crate::starmap::types::StarMapPoint::default(),
            app_data_root,
        );
        tx.prepare().unwrap();
        tx.mark_child_created().unwrap();
        tx.mark_embed_added().unwrap();

        // 恢复：EmbedAdded → 返回 change-set，不删 journal
        let recovered = recover_pending_child_embed_transactions(app_data_root).unwrap();
        assert_eq!(recovered.len(), 1);
        assert_eq!(recovered[0].journal_token, tx.tx_id());

        // journal 不应被删除
        assert!(tx.journal_path.exists());
    }

    #[test]
    fn test_recovered_change_set_contains_correct_paths() {
        let dir = setup_temp_dir();
        let app_data_root = dir.path();

        // 创建宿主星图和子星图，但不添加 embed
        let host_meta = crate::starmap::create_starmap(app_data_root, "Host", "", None).unwrap();
        let child_meta = crate::starmap::create_starmap(app_data_root, "Child", "", None).unwrap();

        // 写一个 ChildCreated phase 的 journal
        let mut tx = StarMapChildEmbedTransaction::new(
            &host_meta.starmap_id,
            &child_meta.starmap_id,
            "Child",
            crate::starmap::types::StarMapPoint::default(),
            app_data_root,
        );
        tx.prepare().unwrap();
        tx.mark_child_created().unwrap();

        // 恢复：补建 embed，返回 change-set
        let recovered = recover_pending_child_embed_transactions(app_data_root).unwrap();
        assert_eq!(recovered.len(), 1);

        let changes = &recovered[0].changes;
        let flat_paths = changes.to_flat_paths();

        // 验证 change-set 包含 child meta 路径
        let child_meta_rel =
            PathBuf::from("starmaps").join(format!("{}.meta.json", child_meta.starmap_id));
        assert!(
            flat_paths.iter().any(|p| p == &child_meta_rel),
            "change-set should contain child meta path: {:?}",
            child_meta_rel
        );

        // 验证 change-set 包含 index.json 路径
        let index_rel = PathBuf::from("starmaps").join("index.json");
        assert!(
            flat_paths.iter().any(|p| p == &index_rel),
            "change-set should contain index.json path: {:?}",
            index_rel
        );
    }
}
