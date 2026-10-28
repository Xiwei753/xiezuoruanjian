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

use crate::error::Result;
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
}

impl ContributionTransaction {
    pub(crate) fn begin(app_data_root: &Path, roots: Vec<PathBuf>) -> Result<Self> {
        let nested = ACTIVE_EVENTS.with(|slot| slot.borrow().is_some());
        if nested {
            return Ok(Self::noop(app_data_root));
        }

        let transaction_id = uuid::Uuid::new_v4().to_string();
        let dir = app_data_root
            .join(TRANSACTIONS_ROOT)
            .join(&transaction_id);
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
        }
    }

    fn write_manifest(&self, events: Vec<ContributionEvent>) -> Result<()> {
        let manifest = ContributionTransactionManifest {
            transaction_id: self.transaction_id.clone(),
            phase: self.phase,
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
            }
        }
        Ok(())
    }

    fn restore(&self) -> Result<()> {
        for root in &self.roots {
            remove_path(Path::new(&root.source))?;
            if root.existed {
                copy_path(Path::new(&root.backup), Path::new(&root.source))?;
            }
        }
        Ok(())
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
        self.phase = TransactionPhase::ObjectApplied;
        self.write_manifest(events.clone())?;

        let store = ContributionStore::new(&self.app_data_root);
        if let Err(error) = store.record_events(events.clone()) {
            log::warn!(
                "contribution transaction {} could not append events yet: {error}",
                self.transaction_id
            );
            self.owns_transaction = false;
            return Ok(());
        }

        self.phase = TransactionPhase::Committed;
        self.write_manifest(events)?;
        let _ = fs::remove_dir_all(&self.dir);
        self.owns_transaction = false;
        Ok(())
    }
}

impl Drop for ContributionTransaction {
    fn drop(&mut self) {
        if !self.owns_transaction {
            return;
        }
        ACTIVE_EVENTS.with(|slot| {
            slot.borrow_mut().take();
        });
        if self.phase == TransactionPhase::Prepared {
            if let Err(error) = self.restore() {
                log::error!(
                    "contribution transaction {} rollback failed: {error}",
                    self.transaction_id
                );
                return;
            }
            let _ = fs::remove_dir_all(&self.dir);
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
            TransactionPhase::BackingUp | TransactionPhase::Prepared => {
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
