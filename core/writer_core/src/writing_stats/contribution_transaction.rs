//! 对象变更与来源事件的跨文件事务。
//!
//! 星图对象和项目注释落在不同根目录，单个 `SaveTransaction` 无法同时覆盖；
//! 这里在修改对象前备份受影响目录，并把待写事件写入同一事务清单。只有对象
//! 已经持久化后才把清单推进到 `object_applied`，随后幂等追加事件并标记
//! `committed`。进程在提交前退出时，恢复逻辑会回到修改前的对象状态；已经
//! 推进到 `object_applied` 的事务则补写事件，避免“对象已改、来源丢失”。

use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::storage::atomic_write_string;
use crate::writing_stats::contribution::ContributionEvent;
use crate::writing_stats::contribution_store::ContributionStore;

const TRANSACTIONS_ROOT: &str = "app-meta/transactions/contributions";
const MANIFEST_FILE: &str = "manifest.json";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum TransactionPhase {
    BackingUp,
    Prepared,
    ObjectApplied,
    Committed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BackupRoot {
    source: String,
    backup: String,
    existed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ContributionTransactionManifest {
    transaction_id: String,
    phase: TransactionPhase,
    roots: Vec<BackupRoot>,
    events: Vec<ContributionEvent>,
}

thread_local! {
    static ACTIVE_EVENTS: RefCell<Option<Vec<ContributionEvent>>> = const { RefCell::new(None) };
}

pub(crate) struct ContributionTransaction {
    app_data_root: PathBuf,
    transaction_id: String,
    dir: PathBuf,
    roots: Vec<BackupRoot>,
    phase: TransactionPhase,
    owns_transaction: bool,
    /// 本次事务修改过的画布。文件回滚后，API 层据此直接淘汰已加载 Store，
    /// 防止内存继续暴露失败修改；提交成功时保留 Store 的最终状态。
    affected_starmaps: Vec<String>,
}

impl ContributionTransaction {
    pub(crate) fn begin(app_data_root: &Path, roots: Vec<PathBuf>) -> Result<Self> {
        let nested = ACTIVE_EVENTS.with(|slot| slot.borrow().is_some());
        if nested {
            return Ok(Self::noop(app_data_root));
        }

        let transaction_id = uuid::Uuid::new_v4().to_string();
        let dir = app_data_root.join(TRANSACTIONS_ROOT).join(&transaction_id);
        fs::create_dir_all(&dir)?;

        let roots: Vec<BackupRoot> = roots
            .into_iter()
            .enumerate()
            .map(|(index, source)| BackupRoot {
                source: source.to_string_lossy().into_owned(),
                backup: dir
                    .join("backup")
                    .join(index.to_string())
                    .to_string_lossy()
                    .into_owned(),
                existed: source.exists(),
            })
            .collect();

        let mut transaction = Self {
            app_data_root: app_data_root.to_path_buf(),
            transaction_id,
            dir,
            roots,
            phase: TransactionPhase::BackingUp,
            owns_transaction: true,
            affected_starmaps: Vec::new(),
        };
        transaction.write_manifest(Vec::new())?;
        transaction.copy_backups()?;
        transaction.phase = TransactionPhase::Prepared;
        transaction.write_manifest(Vec::new())?;
        ACTIVE_EVENTS.with(|slot| {
            *slot.borrow_mut() = Some(Vec::new());
        });
        Ok(transaction)
    }

    fn noop(app_data_root: &Path) -> Self {
        Self {
            app_data_root: app_data_root.to_path_buf(),
            transaction_id: String::new(),
            dir: PathBuf::new(),
            roots: Vec::new(),
            phase: TransactionPhase::Committed,
            owns_transaction: false,
            affected_starmaps: Vec::new(),
        }
    }

    /// 登记本次事务会修改内存与磁盘的画布集合。回滚时由 API 层统一淘汰这些
    /// Store；事务成功提交后则保留它们的最终内存状态。
    pub(crate) fn register_affected_starmaps(&mut self, starmap_ids: &[String]) {
        if !self.owns_transaction {
            return;
        }
        self.affected_starmaps.extend(starmap_ids.iter().cloned());
        self.affected_starmaps.sort();
        self.affected_starmaps.dedup();
    }

    pub(crate) fn affected_starmaps(&self) -> &[String] {
        &self.affected_starmaps
    }

    fn write_manifest(&self, events: Vec<ContributionEvent>) -> Result<()> {
        self.write_manifest_for_phase(self.phase, events)
    }

    fn write_manifest_for_phase(
        &self,
        phase: TransactionPhase,
        events: Vec<ContributionEvent>,
    ) -> Result<()> {
        let manifest = ContributionTransactionManifest {
            transaction_id: self.transaction_id.clone(),
            phase,
            roots: self.roots.clone(),
            events,
        };
        fs::create_dir_all(&self.dir)?;
        atomic_write_string(
            &self.dir.join(MANIFEST_FILE),
            &serde_json::to_string(&manifest)?,
        )
    }

    fn copy_backups(&self) -> Result<()> {
        for root in &self.roots {
            if root.existed {
                copy_path(Path::new(&root.source), Path::new(&root.backup))?;
                if !Path::new(&root.backup).exists() {
                    return Err(Error::Other(format!(
                        "contribution backup missing after copy: {}",
                        root.backup
                    )));
                }
            }
        }
        Ok(())
    }

    /// 延迟准备事务：先建立 Prepared 空清单，等对象变更写入前再登记具体文件。
    pub(crate) fn begin_deferred(app_data_root: &Path) -> Result<Self> {
        Self::begin(app_data_root, Vec::new())
    }

    /// 在真正写对象文件前登记具体路径，只备份这些文件/目录，不复制整棵图。
    pub(crate) fn prepare_paths(&mut self, paths: &[PathBuf]) -> Result<()> {
        if !self.owns_transaction || self.phase != TransactionPhase::Prepared {
            return Ok(());
        }
        let mut changed = false;
        for path in paths {
            let source = path.to_string_lossy().into_owned();
            if self.roots.iter().any(|root| root.source == source) {
                continue;
            }
            // 已被某个目录根覆盖的路径无需重复备份。
            if self.roots.iter().any(|root| {
                let existing = Path::new(&root.source);
                existing != path && path.starts_with(existing)
            }) {
                continue;
            }
            let existed = path.exists();
            let backup = if existed {
                let backup = self.dir.join("backup").join(self.roots.len().to_string());
                copy_path(path, &backup)?;
                backup.to_string_lossy().into_owned()
            } else {
                String::new()
            };
            self.roots.push(BackupRoot {
                source,
                backup,
                existed,
            });
            changed = true;
        }
        if changed {
            self.write_manifest(Vec::new())?;
        }
        Ok(())
    }

    fn restore(&self) -> Result<()> {
        for root in &self.roots {
            if root.existed && (root.backup.is_empty() || !Path::new(&root.backup).exists()) {
                return Err(Error::Other(format!(
                    "contribution backup missing during rollback: {}",
                    root.source
                )));
            }
        }
        for root in &self.roots {
            remove_path(Path::new(&root.source))?;
            if root.existed {
                copy_path(Path::new(&root.backup), Path::new(&root.source))?;
            }
        }
        Ok(())
    }

    /// Prepared 阶段主动回滚。返回 `true` 表示文件已恢复到事务开始前，调用方
    /// 必须同步淘汰受影响的已加载 Store；`ObjectApplied` 阶段不在这里回滚，
    /// 继续沿用“对象已生效、事件待恢复”的既有语义。
    pub(crate) fn rollback_prepared(&mut self) -> Result<bool> {
        if !self.owns_transaction {
            return Ok(false);
        }
        ACTIVE_EVENTS.with(|slot| {
            slot.borrow_mut().take();
        });
        if self.phase != TransactionPhase::Prepared {
            return Ok(false);
        }
        self.restore()?;
        let _ = fs::remove_dir_all(&self.dir);
        self.owns_transaction = false;
        Ok(true)
    }

    /// 对象已经持久化后调用：先写 `object_applied` 清单，再幂等追加事件。
    /// 事件追加失败时清单保留，下次工作区写入通过恢复流程补写；对象本身不回滚。
    pub(crate) fn commit(&mut self) -> Result<()> {
        if !self.owns_transaction {
            return Ok(());
        }
        let events = ACTIVE_EVENTS
            .with(|slot| slot.borrow_mut().take())
            .unwrap_or_default();
        // 只有 ObjectApplied 清单真正落盘后才能把内存阶段推进；否则写盘失败时
        // Drop 仍必须按 Prepared 回滚对象，不能因为内存阶段提前改变而漏回滚。
        self.write_manifest_for_phase(TransactionPhase::ObjectApplied, events.clone())?;
        self.phase = TransactionPhase::ObjectApplied;

        let store = ContributionStore::new(&self.app_data_root);
        if let Err(error) = store.record_events(events.clone()) {
            log::warn!(
                "contribution transaction {} could not append events yet: {error}",
                self.transaction_id
            );
            self.owns_transaction = false;
            return Err(error);
        }

        self.write_manifest_for_phase(TransactionPhase::Committed, events)?;
        self.phase = TransactionPhase::Committed;
        let _ = fs::remove_dir_all(&self.dir);
        self.owns_transaction = false;
        Ok(())
    }
}

impl Drop for ContributionTransaction {
    fn drop(&mut self) {
        if let Err(error) = self.rollback_prepared() {
            log::error!(
                "contribution transaction {} rollback failed: {error}",
                self.transaction_id
            );
        }
    }
}

/// 当前事务内暂存事件；没有活动事务时返回 `false`。
pub(crate) fn queue_active_events(events: Vec<ContributionEvent>) -> bool {
    ACTIVE_EVENTS.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(queued) = slot.as_mut() else {
            return false;
        };
        queued.extend(events);
        true
    })
}

/// 工作区锁内恢复未完成事务。
pub(crate) fn recover_pending(app_data_root: &Path) -> Result<()> {
    let root = app_data_root.join(TRANSACTIONS_ROOT);
    if !root.is_dir() {
        return Ok(());
    }
    let mut dirs: Vec<PathBuf> = fs::read_dir(&root)?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    dirs.sort();
    for dir in dirs {
        let manifest_path = dir.join(MANIFEST_FILE);
        let content = match fs::read_to_string(&manifest_path) {
            Ok(content) => content,
            Err(_) => {
                let _ = fs::remove_dir_all(&dir);
                continue;
            }
        };
        let manifest: ContributionTransactionManifest = match serde_json::from_str(&content) {
            Ok(manifest) => manifest,
            Err(error) => {
                log::error!(
                    "skip malformed contribution transaction {}: {error}",
                    dir.display()
                );
                continue;
            }
        };
        match manifest.phase {
            TransactionPhase::BackingUp => {
                // 备份尚未完成，对象还没有开始修改；只清理不完整备份，
                // 绝不能拿不完整备份覆盖原始对象。
                let _ = fs::remove_dir_all(&dir);
            }
            TransactionPhase::Prepared => {
                restore_manifest(&manifest)?;
                let _ = fs::remove_dir_all(&dir);
            }
            TransactionPhase::ObjectApplied => {
                let store = ContributionStore::new(app_data_root);
                store.record_events(manifest.events)?;
                let _ = fs::remove_dir_all(&dir);
            }
            TransactionPhase::Committed => {
                let _ = fs::remove_dir_all(&dir);
            }
        }
    }
    Ok(())
}

fn restore_manifest(manifest: &ContributionTransactionManifest) -> Result<()> {
    // 先确认所有需要恢复的备份都存在，再删除任何原始路径；避免备份缺失时
    // 把原始对象删掉却无法恢复。
    for root in &manifest.roots {
        if root.existed && !Path::new(&root.backup).exists() {
            return Err(Error::Other(format!(
                "contribution backup missing during recovery: {}",
                root.backup
            )));
        }
    }
    for root in &manifest.roots {
        remove_path(Path::new(&root.source))?;
        if root.existed {
            copy_path(Path::new(&root.backup), Path::new(&root.source))?;
        }
    }
    Ok(())
}

fn copy_path(source: &Path, target: &Path) -> Result<()> {
    if source.is_dir() {
        fs::create_dir_all(target)?;
        for entry in fs::read_dir(source)? {
            let entry = entry?;
            copy_path(&entry.path(), &target.join(entry.file_name()))?;
        }
    } else {
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(source, target)?;
    }
    Ok(())
}

fn remove_path(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => {
            fs::remove_dir_all(path)?;
        }
        Ok(_) => {
            fs::remove_file(path)?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    Ok(())
}
