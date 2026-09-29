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
const CHILD_EMBED_JOURNAL_PREFIX: &str = ".sujian-child-embed-journal-";

/// 子嵌入 journal 所在目录（app_meta 下）。
const CHILD_EMBED_JOURNALS_DIR: &str = "app-meta/child-embed-journals";

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
/// 根据 phase 和实际状态决定下一步：
/// - `Pending`：child 还没创建，清 journal（无操作可恢复）
/// - `ChildCreated`：child 已存在但 Embed 可能还没添加，补 Embed
/// - `EmbedAdded`：Embed 已添加，推进到 Completed 并清 journal
/// - `Completed`：直接清 journal
pub fn recover_pending_child_embed_transactions(app_data_root: &Path) -> Result<()> {
    let journals_dir = app_data_root.join(CHILD_EMBED_JOURNALS_DIR);
    if !journals_dir.exists() {
        return Ok(());
    }

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

        if let Err(e) = recover_single_child_embed_journal(&path, app_data_root) {
            log::error!(
                "[recover_pending_child_embed_transactions] failed to recover {}: {}",
                path.display(),
                e
            );
        }
    }
    Ok(())
}

/// 恢复单个子嵌入 journal。
fn recover_single_child_embed_journal(journal_path: &Path, app_data_root: &Path) -> Result<()> {
    let content = fs::read(journal_path)?;
    let journal: StarMapChildEmbedJournal = serde_json::from_slice(&content).map_err(|e| {
        crate::error::Error::Io(std::io::Error::other(format!(
            "recover_single_child_embed_journal: parse {}: {e}",
            journal_path.display()
        )))
    })?;

    match journal.phase {
        StarMapChildEmbedPhase::Pending => {
            // journal 刚写盘，child 还没创建。清 journal（无操作可恢复）。
            log::info!(
                "[recover_single_child_embed_journal] Pending phase — child not yet created, \
                 clearing journal tx_id={}",
                journal.tx_id
            );
            fs::remove_file(journal_path)?;
            if let Some(parent) = journal_path.parent() {
                crate::storage::sync_dir(parent)?;
            }
            Ok(())
        }
        StarMapChildEmbedPhase::ChildCreated => {
            // child 已创建，检查 Embed 是否存在。
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
                return Ok(());
            }

            // child 已存在，检查 Embed 是否已在宿主图中。
            let embed_exists = check_embed_exists(
                app_data_root,
                &journal.host_starmap_id,
                &journal.child_starmap_id,
            )?;

            if embed_exists {
                // Embed 已存在，推进到 Completed 并清 journal。
                log::info!(
                    "[recover_single_child_embed_journal] ChildCreated and embed already exists, \
                     completing tx_id={}",
                    journal.tx_id
                );
                let mut tx = StarMapChildEmbedTransaction {
                    journal,
                    journal_path: journal_path.to_path_buf(),
                    completed: false,
                };
                tx.complete()?;
                tx.cleanup_journal()?;
            } else {
                // Embed 不存在，需要补 Embed。
                log::info!(
                    "[recover_single_child_embed_journal] ChildCreated but embed missing, \
                     repairing tx_id={}",
                    journal.tx_id
                );
                repair_embed_for_child(app_data_root, &journal)?;
                let mut tx = StarMapChildEmbedTransaction {
                    journal,
                    journal_path: journal_path.to_path_buf(),
                    completed: false,
                };
                tx.complete()?;
                tx.cleanup_journal()?;
            }
            Ok(())
        }
        StarMapChildEmbedPhase::EmbedAdded => {
            // Embed 已添加，推进到 Completed 并清 journal。
            log::info!(
                "[recover_single_child_embed_journal] EmbedAdded, completing tx_id={}",
                journal.tx_id
            );
            let mut tx = StarMapChildEmbedTransaction {
                journal,
                journal_path: journal_path.to_path_buf(),
                completed: false,
            };
            tx.complete()?;
            tx.cleanup_journal()?;
            Ok(())
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
            Ok(())
        }
    }
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
fn repair_embed_for_child(app_data_root: &Path, journal: &StarMapChildEmbedJournal) -> Result<()> {
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
    store.flush()?;
    Ok(())
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

        // 没有任何 journal 文件，恢复应该成功且无操作
        recover_pending_child_embed_transactions(app_data_root).unwrap();
    }

    #[test]
    fn test_recover_pending_phase_clears_journal() {
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

        // 模拟崩溃：直接恢复，不推进 phase
        recover_pending_child_embed_transactions(app_data_root).unwrap();

        // Pending phase 应该清除 journal（child 未创建）
        assert!(!tx.journal_path.exists());
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

        recover_pending_child_embed_transactions(app_data_root).unwrap();
        assert!(!tx.journal_path.exists());
    }

    #[test]
    fn test_recover_child_created_embed_exists() {
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

        // 恢复：embed 已存在，应该完成并清 journal
        recover_pending_child_embed_transactions(app_data_root).unwrap();
        assert!(!tx.journal_path.exists());
    }

    #[test]
    fn test_recover_child_created_embed_missing_repairs() {
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

        // 恢复：embed 不存在，应该补建 embed
        recover_pending_child_embed_transactions(app_data_root).unwrap();
        assert!(!tx.journal_path.exists());

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

        // 恢复：child meta 不存在，应该清 journal
        recover_pending_child_embed_transactions(app_data_root).unwrap();
        assert!(!tx.journal_path.exists());
    }
}
