//! 贡献事务崩溃恢复的对象级协调。
//!
//! 事务在 `Prepared` 阶段崩溃时，对象可能只写入了一部分，崩溃后用户还可能
//! 继续编辑。旧实现删除整棵 source 再复制 backup，会抹掉备份之后产生的
//! 正文、新对象和删除重建结果。这里逐对象比较 source 与 backup 的
//! generation / revision / content_sha256 / digest，只有能证明 source 仍是
//! 备份基线（或比备份更旧、需要回滚一次删除）时才从备份恢复：
//!
//! - source 版本更新、同 revision 不同摘要、generation 改变 → 保留 source；
//! - source 已不存在且有持久删除事实（本地变更账本 / 同步 tombstone）→
//!   保留删除；
//! - source 已不存在且没有删除事实 → 恢复备份对象（回滚事务删除）；
//! - backup 中不存在的对象是新对象 → 一律保留，绝不整树删除。
//!
//! 删除重建按最近的 `chapter.meta.json` 作为对象版本上下文：章节 generation
//! 变化后，旧备份里该章节目录下的其它文件也不能再恢复，否则会把旧正文/
//! 旧附属文件塞回新章节。无法证明的恢复保留 source 与事务目录（含
//! `recovery-conflicts.json`），由下一次启动/人工诊断处理。

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::error::{Error, Result};
use crate::sync::local_mutations::{self, LocalMutationRecord};
use crate::sync::types::{SyncScope, SyncState};
use crate::sync::SyncService;

use super::{BackupRoot, ContributionTransactionManifest};

mod facts;

use facts::{backup_path_for, decide, read_object_facts, Decision, ObjectFacts};

/// 一次恢复的结果。存在冲突时调用方必须保留事务目录与备份材料。
#[derive(Debug, Default)]
pub(super) struct RestoreReport {
    /// 已从备份恢复的对象。
    pub restored: Vec<PathBuf>,
    /// 因 source 更新、删除事实明确或对象新建而保留的对象。
    pub preserved: Vec<PreservedObject>,
    /// 无法证明如何协调的对象；source 与备份都不动。
    pub conflicts: Vec<RestoreConflict>,
}

#[derive(Debug)]
pub(super) struct PreservedObject {
    pub path: PathBuf,
    pub reason: String,
}

#[derive(Debug)]
pub(super) struct RestoreConflict {
    pub path: PathBuf,
    pub reason: String,
}

impl RestoreReport {
    pub(super) fn has_conflicts(&self) -> bool {
        !self.conflicts.is_empty()
    }

    pub(super) fn conflict_message(&self) -> String {
        self.conflicts
            .iter()
            .map(|conflict| format!("{}: {}", conflict.path.display(), conflict.reason))
            .collect::<Vec<_>>()
            .join("; ")
    }
}

/// 恢复一笔 `Prepared` 贡献事务。
///
/// 备份材料缺失仍然是硬错误（无法回滚）；对象级冲突不是硬错误：保留 source
/// 与事务目录并把冲突返回给调用方，避免因为一次崩溃后的正常编辑导致工作区
/// 无法打开。恢复前会先校验所有备份都存在，再逐个对象决策、执行。
pub(super) fn restore_manifest(
    app_data_root: &Path,
    tx_dir: &Path,
    manifest: &ContributionTransactionManifest,
) -> Result<RestoreReport> {
    for root in &manifest.roots {
        if root.existed && (root.backup.is_empty() || !Path::new(&root.backup).exists()) {
            return Err(Error::Other(format!(
                "contribution backup missing during recovery: source={} backup={}",
                root.source, root.backup
            )));
        }
    }

    let mut report = RestoreReport::default();
    let mut deletion_facts = DeletionFacts::new(app_data_root, manifest);
    for root in &manifest.roots {
        if let Err(reason) = restore_root(&mut deletion_facts, root, manifest, &mut report) {
            report.conflicts.push(RestoreConflict {
                path: PathBuf::from(&root.source),
                reason,
            });
        }
    }

    if !report.conflicts.is_empty() {
        if let Err(error) = write_conflict_report(tx_dir, manifest, &report) {
            log::warn!(
                "contribution recovery: could not persist conflict report for tx {}: {error}",
                manifest.transaction_id
            );
        }
    } else {
        let report_path = tx_dir.join("recovery-conflicts.json");
        if report_path.exists() {
            let _ = fs::remove_file(report_path);
        }
    }
    Ok(report)
}

fn write_conflict_report(
    tx_dir: &Path,
    manifest: &ContributionTransactionManifest,
    report: &RestoreReport,
) -> Result<()> {
    let conflicts: Vec<Value> = report
        .conflicts
        .iter()
        .map(|conflict| {
            serde_json::json!({
                "path": conflict.path.to_string_lossy(),
                "reason": conflict.reason,
            })
        })
        .collect();
    let document = serde_json::json!({
        "transaction_id": manifest.transaction_id,
        "restored": report
            .restored
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect::<Vec<_>>(),
        "preserved": report
            .preserved
            .iter()
            .map(|preserved| serde_json::json!({
                "path": preserved.path.to_string_lossy(),
                "reason": preserved.reason,
            }))
            .collect::<Vec<_>>(),
        "conflicts": conflicts,
    });
    crate::storage::atomic_write_string(
        &tx_dir.join("recovery-conflicts.json"),
        &serde_json::to_string_pretty(&document)?,
    )?;
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PathKind {
    Missing,
    File,
    Directory,
}

impl PathKind {
    fn of(path: &Path) -> Self {
        match fs::symlink_metadata(path) {
            Ok(metadata) if metadata.is_dir() => Self::Directory,
            Ok(_) => Self::File,
            Err(_) => Self::Missing,
        }
    }
}

fn restore_root(
    deletion_facts: &mut DeletionFacts<'_>,
    root: &BackupRoot,
    manifest: &ContributionTransactionManifest,
    report: &mut RestoreReport,
) -> std::result::Result<(), String> {
    let source = Path::new(&root.source);
    let backup = Path::new(&root.backup);
    let backup_available = root.existed && !root.backup.is_empty() && backup.exists();
    if root.existed && !backup_available {
        return Err(format!(
            "backup missing during recovery: source={} backup={}",
            root.source, root.backup
        ));
    }

    let source_kind = PathKind::of(source);
    let backup_kind = if backup_available {
        PathKind::of(backup)
    } else {
        PathKind::Missing
    };
    if matches!(
        (source_kind, backup_kind),
        (PathKind::File, PathKind::Directory) | (PathKind::Directory, PathKind::File)
    ) {
        report.conflicts.push(RestoreConflict {
            path: source.to_path_buf(),
            reason: "source and backup object kinds differ; refusing tree replacement".to_string(),
        });
        return Ok(());
    }

    if source_kind == PathKind::Directory || backup_kind == PathKind::Directory {
        let current = walk_files(source)?;
        let previous = if backup_available {
            walk_files(backup)?
        } else {
            BTreeMap::new()
        };
        let contexts = ChapterContexts::scan(source, backup, &current, &previous, deletion_facts)?;
        let mut relative_paths = BTreeSet::new();
        relative_paths.extend(current.keys().cloned());
        relative_paths.extend(previous.keys().cloned());
        for relative in relative_paths {
            let source_path = join_relative(source, &relative);
            let backup_path = join_relative(backup, &relative);
            let context = contexts.for_path(&relative);
            restore_object(
                deletion_facts,
                context,
                &source_path,
                &backup_path,
                backup_available,
                None,
                report,
            );
        }
        return Ok(());
    }

    let context = single_file_chapter_context(manifest, source, backup, deletion_facts)?;
    let backup_meta = single_file_backup_meta(manifest, source, backup);
    restore_object(
        deletion_facts,
        context.as_ref(),
        source,
        backup,
        backup_available,
        backup_meta.as_deref(),
        report,
    );
    Ok(())
}

fn join_relative(root: &Path, relative: &str) -> PathBuf {
    if relative.is_empty() {
        root.to_path_buf()
    } else {
        root.join(relative)
    }
}

fn walk_files(root: &Path) -> std::result::Result<BTreeMap<String, PathBuf>, String> {
    let mut files = BTreeMap::new();
    if !root.exists() {
        return Ok(files);
    }
    if root.is_file() {
        files.insert(String::new(), root.to_path_buf());
        return Ok(files);
    }
    for entry in walkdir::WalkDir::new(root).follow_links(false) {
        let entry = entry.map_err(|error| {
            format!(
                "walk contribution backup root {} failed: {error}",
                root.display()
            )
        })?;
        if !entry.file_type().is_file() {
            continue;
        }
        let relative = entry
            .path()
            .strip_prefix(root)
            .map_err(|error| format!("normalize contribution path failed: {error}"))?
            .to_string_lossy()
            .replace('\\', "/");
        files.insert(relative, entry.path().to_path_buf());
    }
    Ok(files)
}

/// 章节对象的版本上下文：同一章节目录内的文件必须按章节 meta 的
/// generation/revision 一起裁决，不能一边恢复旧正文一边保留新 meta。
#[derive(Debug)]
enum ChapterContext {
    /// source 仍是备份基线或更旧：可以按对象恢复备份成员。
    Rollback,
    /// source 章节已更新/删除重建：不恢复该章节目录下的旧备份成员。
    Keep(String),
    /// 章节删除事实明确：保留删除。
    Deleted(String),
    /// 章节版本无法证明：保留 source。
    Conflict(String),
}

struct ChapterContexts {
    dirs: BTreeMap<String, ChapterContext>,
}

impl ChapterContexts {
    fn scan(
        source_root: &Path,
        backup_root: &Path,
        current: &BTreeMap<String, PathBuf>,
        previous: &BTreeMap<String, PathBuf>,
        deletion_facts: &mut DeletionFacts<'_>,
    ) -> std::result::Result<Self, String> {
        let mut dirs = BTreeSet::new();
        for relative in current.keys().chain(previous.keys()) {
            if let Some(directory) = relative.strip_suffix("chapter.meta.json") {
                dirs.insert(directory.trim_end_matches('/').to_string());
            }
        }
        let mut contexts = BTreeMap::new();
        for directory in dirs {
            let source_meta =
                join_relative(source_root, &chapter_file(&directory, "chapter.meta.json"));
            let backup_meta =
                join_relative(backup_root, &chapter_file(&directory, "chapter.meta.json"));
            let context = chapter_context(deletion_facts, &source_meta, &backup_meta)?;
            contexts.insert(directory, context);
        }
        Ok(Self { dirs: contexts })
    }

    fn for_path(&self, relative: &str) -> Option<&ChapterContext> {
        let mut current = Path::new(relative).parent();
        while let Some(directory) = current {
            let key = directory.to_string_lossy().replace('\\', "/");
            if let Some(context) = self.dirs.get(&key) {
                return Some(context);
            }
            current = directory.parent();
        }
        None
    }
}

fn chapter_file(directory: &str, name: &str) -> String {
    if directory.is_empty() {
        name.to_string()
    } else {
        format!("{directory}/{name}")
    }
}

fn single_file_chapter_context(
    manifest: &ContributionTransactionManifest,
    source: &Path,
    backup: &Path,
    deletion_facts: &mut DeletionFacts<'_>,
) -> std::result::Result<Option<ChapterContext>, String> {
    let Some(name) = source.file_name().and_then(|name| name.to_str()) else {
        return Ok(None);
    };
    if name != "chapter.md" && name != "chapter.meta.json" {
        return Ok(None);
    }
    let Some(directory) = source.parent() else {
        return Ok(None);
    };
    let source_meta = directory.join("chapter.meta.json");
    let Some(backup_meta) = single_file_backup_meta(manifest, source, backup) else {
        // 单文件根没有可证明的备份身份：source 存在就保留，source 缺失才
        // 走无版本信息的默认恢复逻辑。
        if source_meta.is_file() {
            return Ok(Some(ChapterContext::Keep(
                "single-file chapter without backup meta; preserving source".to_string(),
            )));
        }
        return Ok(None);
    };
    chapter_context(deletion_facts, &source_meta, &backup_meta).map(Some)
}

/// 单文件根没有目录结构可继承，按清单中的兄弟 `chapter.meta.json` 根定位
/// 它的备份，才能比较同一章节的 generation/revision/content_sha256。
fn single_file_backup_meta(
    manifest: &ContributionTransactionManifest,
    source: &Path,
    backup: &Path,
) -> Option<PathBuf> {
    let name = source.file_name()?.to_str()?;
    if name != "chapter.md" && name != "chapter.meta.json" {
        return None;
    }
    let directory = source.parent()?;
    let source_meta = directory.join("chapter.meta.json");
    backup_path_for(manifest, &source_meta)
        .or_else(|| {
            backup
                .parent()
                .map(|parent| parent.join("chapter.meta.json"))
        })
        .filter(|path| path.is_file())
}

fn chapter_context(
    deletion_facts: &mut DeletionFacts<'_>,
    source_meta: &Path,
    backup_meta: &Path,
) -> std::result::Result<ChapterContext, String> {
    let source_facts = read_object_facts(source_meta, None)?;
    let backup_facts = read_object_facts(backup_meta, None)?;
    if !source_facts.exists && !backup_facts.exists {
        return Ok(ChapterContext::Rollback);
    }
    if !backup_facts.exists {
        return Ok(ChapterContext::Keep(
            "chapter object created after backup; keeping new object".to_string(),
        ));
    }
    if !source_facts.exists {
        if let Some(reason) = deletion_facts.deletion_fact(source_meta)? {
            return Ok(ChapterContext::Deleted(format!(
                "chapter meta has durable delete fact: {reason}"
            )));
        }
        let source_body = source_meta
            .parent()
            .map(|directory| directory.join("chapter.md"))
            .unwrap_or_else(|| PathBuf::from("chapter.md"));
        if let Some(reason) = deletion_facts.deletion_fact(&source_body)? {
            return Ok(ChapterContext::Deleted(format!(
                "chapter body has durable delete fact: {reason}"
            )));
        }
        return Ok(ChapterContext::Rollback);
    }
    Ok(match decide(&source_facts, &backup_facts, None) {
        Decision::Restore(_) | Decision::KeepBaseline | Decision::Skip => ChapterContext::Rollback,
        Decision::KeepNewer(reason) | Decision::KeepCreated(reason) => ChapterContext::Keep(reason),
        Decision::KeepDeleted(reason) => ChapterContext::Deleted(reason),
        Decision::Conflict(reason) => ChapterContext::Conflict(reason),
    })
}

fn restore_object(
    deletion_facts: &mut DeletionFacts<'_>,
    context: Option<&ChapterContext>,
    source: &Path,
    backup: &Path,
    backup_available: bool,
    backup_meta: Option<&Path>,
    report: &mut RestoreReport,
) {
    let source_facts = match read_object_facts(source, None) {
        Ok(facts) => facts,
        Err(reason) => {
            report.conflicts.push(RestoreConflict {
                path: source.to_path_buf(),
                reason,
            });
            return;
        }
    };
    let backup_facts = if backup_available {
        match read_object_facts(backup, backup_meta) {
            Ok(facts) => facts,
            Err(reason) => {
                report.conflicts.push(RestoreConflict {
                    path: source.to_path_buf(),
                    reason,
                });
                return;
            }
        }
    } else {
        ObjectFacts::default()
    };
    let deletion_fact = if !source_facts.exists && backup_facts.exists {
        match deletion_facts.deletion_fact(source) {
            Ok(fact) => fact,
            Err(reason) => {
                report.conflicts.push(RestoreConflict {
                    path: source.to_path_buf(),
                    reason,
                });
                return;
            }
        }
    } else {
        None
    };

    let mut decision = decide(&source_facts, &backup_facts, deletion_fact.as_deref());
    if let Some(context) = context {
        decision =
            apply_chapter_context(decision, context, source_facts.exists, backup_facts.exists);
    }

    match decision {
        Decision::Skip | Decision::KeepBaseline => {}
        Decision::Restore(reason) => match restore_backup_object(backup, source) {
            Ok(()) => {
                log::info!(
                    "contribution recovery: restored {} from backup ({reason})",
                    source.display()
                );
                report.restored.push(source.to_path_buf());
            }
            Err(error) => {
                report.conflicts.push(RestoreConflict {
                    path: source.to_path_buf(),
                    reason: format!("restore from backup failed: {error}"),
                });
            }
        },
        Decision::KeepNewer(reason)
        | Decision::KeepCreated(reason)
        | Decision::KeepDeleted(reason) => {
            log::warn!(
                "contribution recovery: preserved {} ({reason})",
                source.display()
            );
            report.preserved.push(PreservedObject {
                path: source.to_path_buf(),
                reason,
            });
        }
        Decision::Conflict(reason) => {
            report.conflicts.push(RestoreConflict {
                path: source.to_path_buf(),
                reason,
            });
        }
    }
}

fn apply_chapter_context(
    decision: Decision,
    context: &ChapterContext,
    source_exists: bool,
    backup_exists: bool,
) -> Decision {
    match context {
        ChapterContext::Rollback => decision,
        ChapterContext::Keep(reason) | ChapterContext::Conflict(reason) => {
            if !source_exists && backup_exists {
                // 章节对象已经是新版本（或版本无法证明）：旧备份成员不能
                // 恢复进新章节；保持删除是唯一不污染新对象的做法。
                Decision::KeepDeleted(format!("chapter context kept: {reason}"))
            } else if matches!(decision, Decision::Restore(_)) {
                Decision::Conflict(format!("chapter context blocks restore: {reason}"))
            } else {
                decision
            }
        }
        ChapterContext::Deleted(reason) => {
            if !source_exists && backup_exists {
                Decision::KeepDeleted(reason.clone())
            } else {
                decision
            }
        }
    }
}

fn restore_backup_object(backup: &Path, source: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(source);
    if matches!(&metadata, Ok(metadata) if metadata.is_dir()) {
        return Err(Error::Other(format!(
            "refusing to restore backup over directory: {}",
            source.display()
        )));
    }
    if let Some(parent) = source.parent() {
        fs::create_dir_all(parent)?;
    }
    crate::storage::durable_copy_file(backup, source)
}

/// 删除事实解析器：读取本地变更账本与同步 tombstone，并区分“事务自己写的
/// 删除事实”。贡献事务在 `Prepared` 阶段可能已经改了 sync state 文件；如果
/// 该文件相对备份已变化，里面的 tombstone 不能当作事务之后的外部删除，
/// 否则会阻止回滚事务自己的删除。
type LedgerCache =
    HashMap<PathBuf, std::result::Result<HashMap<String, LocalMutationRecord>, String>>;
type StateCache = HashMap<(PathBuf, Option<String>), std::result::Result<SyncState, String>>;

struct DeletionFacts<'a> {
    app_data_root: &'a Path,
    manifest: &'a ContributionTransactionManifest,
    created_at_ms: i64,
    ledgers: LedgerCache,
    states: StateCache,
    state_changes: HashMap<PathBuf, bool>,
}

fn project_location(app_data_root: &Path, relative: &str) -> Option<(PathBuf, String, SyncScope)> {
    let rest = relative.strip_prefix("projects/")?;
    let (project_id, tail) = rest.split_once('/')?;
    if project_id.is_empty() || tail.is_empty() {
        return None;
    }
    Some((
        app_data_root.join("projects").join(project_id),
        tail.to_string(),
        SyncScope::Project,
    ))
}

fn external_project_location(path: &Path) -> Option<(PathBuf, String, SyncScope)> {
    let normalized = path.to_string_lossy().replace('\\', "/");
    let index = normalized.rfind("/projects/")?;
    let after = &normalized[index + "/projects/".len()..];
    let (project_id, tail) = after.split_once('/')?;
    if project_id.is_empty() || tail.is_empty() {
        return None;
    }
    let project_root = Path::new(&normalized[..index])
        .join("projects")
        .join(project_id);
    Some((project_root, tail.to_string(), SyncScope::Project))
}

impl<'a> DeletionFacts<'a> {
    fn new(app_data_root: &'a Path, manifest: &'a ContributionTransactionManifest) -> Self {
        Self {
            app_data_root,
            manifest,
            created_at_ms: manifest.created_at_ms,
            ledgers: HashMap::new(),
            states: HashMap::new(),
            state_changes: HashMap::new(),
        }
    }

    fn deletion_fact(&mut self, path: &Path) -> std::result::Result<Option<String>, String> {
        let Some((sync_root, relative, scope)) = self.sync_location(path) else {
            return Ok(None);
        };
        if !local_mutations::is_sync_business_path(&relative, scope) {
            return Ok(None);
        }

        let ledger = self.ledger(&sync_root)?;
        if let Some(record) = ledger.get(&relative) {
            if record.is_delete()
                && (self.created_at_ms <= 0 || record.updated_at_ms >= self.created_at_ms)
            {
                return Ok(Some(format!(
                    "local mutation delete at {} (transaction {})",
                    record.updated_at_ms, record.transaction_id
                )));
            }
        }

        if self.state_file_changed(&sync_root)? {
            return Ok(None);
        }
        let state = self.state(&sync_root, scope, &relative)?;
        if let Some(tombstone) = state.tombstones.iter().find(|tombstone| {
            tombstone.original_path == relative
                && (self.created_at_ms <= 0
                    || tombstone
                        .deleted_at
                        .saturating_mul(1000)
                        .saturating_add(1000)
                        >= self.created_at_ms)
        }) {
            return Ok(Some(format!(
                "sync tombstone kind={} deleted_at={}",
                tombstone.kind, tombstone.deleted_at
            )));
        }
        if state.deleted_files.contains(&relative) {
            return Ok(Some("sync deleted_files fact".to_string()));
        }
        Ok(None)
    }

    fn sync_location(&self, path: &Path) -> Option<(PathBuf, String, SyncScope)> {
        // 标准布局：作品根位于 app_data_root/projects/<pid>。
        if let Ok(relative) = path.strip_prefix(self.app_data_root) {
            let relative = relative.to_string_lossy().replace('\\', "/");
            if relative.is_empty() {
                return None;
            }
            if let Some(location) = project_location(self.app_data_root, &relative) {
                return Some(location);
            }
            return Some((self.app_data_root.to_path_buf(), relative, SyncScope::App));
        }
        // Android 等平台的作品根可能在 app_data_root 之外；仍按
        // `.../projects/<pid>/...` 定位该作品自己的同步状态/账本。
        external_project_location(path)
    }

    fn ledger(
        &mut self,
        sync_root: &Path,
    ) -> std::result::Result<HashMap<String, LocalMutationRecord>, String> {
        if let Some(cached) = self.ledgers.get(sync_root) {
            return cached.clone();
        }
        let result = local_mutations::load_latest(sync_root).map_err(|error| {
            format!(
                "read local mutation ledger {} failed: {error}",
                sync_root
                    .join("app-meta/sync/local-mutations.jsonl")
                    .display()
            )
        });
        self.ledgers.insert(sync_root.to_path_buf(), result.clone());
        result
    }

    fn state(
        &mut self,
        sync_root: &Path,
        scope: SyncScope,
        relative: &str,
    ) -> std::result::Result<SyncState, String> {
        let group = if scope == SyncScope::App {
            crate::sync::config_store::app_data_group_for_path(relative)
        } else {
            None
        };
        let key = (
            sync_root.to_path_buf(),
            group.as_ref().map(|group| group.id()),
        );
        if let Some(cached) = self.states.get(&key) {
            return cached.clone();
        }
        let result = match group.as_ref() {
            Some(group) => SyncService::load_sync_state_read_only_for_group(sync_root, group, None),
            None => SyncService::load_sync_state_read_only(sync_root, None),
        }
        .map_err(|error| {
            format!(
                "read sync state {} failed: {error}",
                sync_root.join("app-meta/sync/state.local.json").display()
            )
        });
        self.states.insert(key, result.clone());
        result
    }

    fn state_file_changed(&mut self, sync_root: &Path) -> std::result::Result<bool, String> {
        if let Some(changed) = self.state_changes.get(sync_root) {
            return Ok(*changed);
        }
        let changed = self.compute_state_file_changed(sync_root)?;
        self.state_changes.insert(sync_root.to_path_buf(), changed);
        Ok(changed)
    }

    fn compute_state_file_changed(&self, sync_root: &Path) -> std::result::Result<bool, String> {
        let state_path = sync_root.join("app-meta/sync/state.local.json");
        let covering = self.manifest.roots.iter().find(|root| {
            let source = Path::new(&root.source);
            let source_is_directory =
                source.is_dir() || (root.existed && Path::new(&root.backup).is_dir());
            source == state_path || (source_is_directory && state_path.starts_with(source))
        });
        let Some(root) = covering else {
            return Ok(false);
        };
        if !root.existed || root.backup.is_empty() {
            return Ok(true);
        }
        if !state_path.exists() {
            return Ok(true);
        }
        let Ok(relative) = state_path.strip_prefix(Path::new(&root.source)) else {
            return Ok(true);
        };
        let backup_path = Path::new(&root.backup).join(relative);
        match (fs::read(&state_path), fs::read(&backup_path)) {
            (Ok(current), Ok(previous)) => Ok(current != previous),
            // 无法比较时按“事务已改过”处理，避免把事务自己的未提交删除事实
            // 当成外部事实。
            _ => Ok(true),
        }
    }
}
