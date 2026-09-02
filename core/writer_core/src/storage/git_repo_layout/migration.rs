use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use super::{GitRepoLayout, RepoOpenResult, try_open_repo};

/// RAII 守卫，保证临时目录在 drop 时删除。
struct MigrateTmpDirGuard(Option<PathBuf>);

#[allow(clippy::expect_used)]
impl MigrateTmpDirGuard {
    fn new(path: PathBuf) -> Self {
        Self(Some(path))
    }

    fn path(&self) -> &Path {
        self.0
            .as_ref()
            .expect("MigrateTmpDirGuard already disarmed")
    }

    fn disarm(&mut self) -> PathBuf {
        self.0.take().expect("MigrateTmpDirGuard already disarmed")
    }
}

impl Drop for MigrateTmpDirGuard {
    fn drop(&mut self) {
        if let Some(path) = self.0.take() {
            let _ = std::fs::remove_dir_all(&path);
        }
    }
}

/// 递归复制目录（durable copy）。
fn migrate_copy_dir_recursive(src: &Path, dst: &Path) -> crate::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            migrate_copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            crate::storage::durable_copy_file(&src_path, &dst_path)?;
        }
    }
    crate::storage::sync_dir(dst)?;
    Ok(())
}

/// 迁移阶段枚举。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MigrationPhase {
    Prepared,
    SourceClaimed,
    TargetPrepared,
    TargetInstalled,
    SourceCleaned,
}

const LAYOUT_MIGRATIONS_DIR: &str = ".layout-migrations";
const LAYOUT_MIGRATION_JOURNAL_NAME: &str = ".sujian-layout-migration";

/// 迁移 journal 内容。
#[derive(Debug, Clone, Serialize, Deserialize)]
struct LayoutMigrationJournal {
    owner: String,
    worktree_canonical: String,
    original_source: String,
    claimed_source: String,
    target_tmp: String,
    target_git_dir: String,
    phase: MigrationPhase,
}

fn canonicalize_or_lossy(path: &Path) -> String {
    std::fs::canonicalize(path)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| path.to_string_lossy().into_owned())
}

fn migrations_dir(target_git_dir: &Path) -> Option<PathBuf> {
    target_git_dir
        .parent()
        .map(|p| p.join(LAYOUT_MIGRATIONS_DIR))
}

fn journal_path(target_git_dir: &Path, owner: &str) -> Option<PathBuf> {
    migrations_dir(target_git_dir).map(|dir| dir.join(format!("{owner}.json")))
}

fn legacy_journal_path(target_git_dir: &Path) -> PathBuf {
    target_git_dir.join(LAYOUT_MIGRATION_JOURNAL_NAME)
}

fn write_migration_journal(
    target_git_dir: &Path,
    journal: &LayoutMigrationJournal,
) -> crate::Result<()> {
    let path = journal_path(target_git_dir, &journal.owner).ok_or_else(|| {
        crate::Error::Io(std::io::Error::other(format!(
            "write_migration_journal: target_git_dir has no parent: {}",
            target_git_dir.display(),
        )))
    })?;
    let content = serde_json::to_vec(journal).map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "write_migration_journal: serialize: {e}"
        )))
    })?;
    crate::storage::atomic_write_bytes(&path, &content)
}

#[allow(clippy::excessive_nesting)]
fn scan_migration_journals(target_git_dir: &Path) -> crate::Result<Vec<LayoutMigrationJournal>> {
    let Some(dir) = migrations_dir(target_git_dir) else {
        return Ok(Vec::new());
    };
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut journals = Vec::new();
    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "json") {
            match std::fs::read(&path) {
                Ok(content) => match serde_json::from_slice(&content) {
                    Ok(journal) => journals.push(journal),
                    Err(e) => {
                        return Err(crate::Error::Io(std::io::Error::other(format!(
                            "scan_migration_journals: corrupted journal {}: {}",
                            path.display(), e,
                        ))));
                    }
                },
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(crate::Error::Io(e)),
            }
        }
    }
    Ok(journals)
}

fn remove_migration_journal(target_git_dir: &Path, owner: &str) -> crate::Result<()> {
    let path = match journal_path(target_git_dir, owner) {
        Some(p) => p,
        None => return Ok(()),
    };
    if !path.exists() {
        return Ok(());
    }
    std::fs::remove_file(&path).map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "remove_migration_journal: remove {}: {e}",
            path.display(),
        )))
    })?;
    if let Some(dir) = migrations_dir(target_git_dir) {
        if dir.exists() {
            crate::storage::sync_dir(&dir)?;
        }
    }
    Ok(())
}

fn migrate_legacy_journal(
    target_git_dir: &Path,
    _worktree_root: &Path,
) -> crate::Result<Option<LayoutMigrationJournal>> {
    let legacy_path = legacy_journal_path(target_git_dir);
    if !legacy_path.exists() {
        return Ok(None);
    }
    let content = std::fs::read(&legacy_path)?;
    #[derive(Debug, Clone, Deserialize)]
    struct LegacyJournal {
        migration_uuid: String,
        worktree_canonical: String,
        original_source: String,
        claimed_source: String,
        #[serde(default)]
        target_tmp: String,
        target_git_dir: String,
        phase: String,
    }
    let legacy: LegacyJournal = serde_json::from_slice(&content).map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "migrate_legacy_journal: parse {}: {e}",
            legacy_path.display(),
        )))
    })?;
    let owner = legacy.migration_uuid;
    let phase = match legacy.phase.as_str() {
        "copied" => MigrationPhase::TargetPrepared,
        "finalized" => MigrationPhase::SourceCleaned,
        _ => {
            log::warn!(
                "[git_repo_layout] legacy journal has unknown phase {}; treating as TargetPrepared",
                legacy.phase,
            );
            MigrationPhase::TargetPrepared
        }
    };
    let target_tmp = if !legacy.target_tmp.is_empty() {
        legacy.target_tmp
    } else {
        let target_git_dir_path = PathBuf::from(&legacy.target_git_dir);
        match target_git_dir_path.parent() {
            Some(parent) => parent
                .join(format!(".git.sujian-migrate-{}", owner))
                .to_string_lossy()
                .into_owned(),
            None => legacy.target_git_dir.clone(),
        }
    };
    let journal = LayoutMigrationJournal {
        owner: owner.clone(),
        worktree_canonical: legacy.worktree_canonical,
        original_source: legacy.original_source,
        claimed_source: legacy.claimed_source,
        target_tmp,
        target_git_dir: legacy.target_git_dir,
        phase,
    };
    write_migration_journal(target_git_dir, &journal)?;
    std::fs::remove_file(&legacy_path)?;
    if let Some(parent) = legacy_path.parent() {
        crate::storage::sync_dir(parent)?;
    }
    Ok(Some(journal))
}

pub(crate) fn complete_migration_with_journal(
    private_git_dir: &Path,
    embedded_git_dir: &Path,
    worktree_root: &Path,
) -> crate::Result<()> {
    let journals = match migrate_legacy_journal(private_git_dir, worktree_root) {
        Ok(Some(j)) => vec![j],
        Ok(None) => scan_migration_journals(private_git_dir)?,
        Err(e) => {
            return Err(crate::Error::Io(std::io::Error::other(format!(
                "complete_migration_with_journal: migrate legacy journal: {e}"
            ))));
        }
    };
    if journals.is_empty() {
        log::warn!(
            "[git_repo_layout] dual repo coexist but no migration journal in private {}; \
             keeping embedded .git at {}",
            private_git_dir.display(),
            embedded_git_dir.display(),
        );
        return Ok(());
    }
    for journal in journals {
        let current_worktree_canonical = canonicalize_or_lossy(worktree_root);
        if journal.worktree_canonical != current_worktree_canonical {
            log::warn!(
                "[git_repo_layout] migration journal worktree mismatch; keeping embedded .git at {}",
                embedded_git_dir.display(),
            );
            continue;
        }
        let claimed_source_path = PathBuf::from(&journal.claimed_source);
        if claimed_source_path.exists() {
            std::fs::remove_dir_all(&claimed_source_path).map_err(|e| {
                crate::Error::Io(std::io::Error::other(format!(
                    "complete_migration_with_journal: remove claimed_source {}: {e}",
                    claimed_source_path.display(),
                )))
            })?;
            if let Some(parent) = worktree_root.parent() {
                crate::storage::sync_dir(parent)?;
            }
            crate::storage::sync_dir(worktree_root)?;
        }
        remove_migration_journal(private_git_dir, &journal.owner)?;
    }
    Ok(())
}

/// 恢复 pending 迁移的统一入口。
#[allow(clippy::too_many_lines, clippy::excessive_nesting)]
pub(crate) fn resume_layout_migration(layout: &GitRepoLayout) -> crate::Result<()> {
    let is_external = layout.git_dir != layout.worktree_root.join(".git");
    if !is_external {
        return Ok(());
    }
    let journals = match migrate_legacy_journal(&layout.git_dir, &layout.worktree_root) {
        Ok(Some(j)) => {
            let owner_tag: &str = &j.owner;
            log::debug!("[git_repo_layout] resume: migrated legacy journal, owner_tag={}", owner_tag);
            vec![j]
        }
        Ok(None) => {
            let scanned = scan_migration_journals(&layout.git_dir)?;
            log::debug!("[git_repo_layout] resume: scanned {} journals", scanned.len());
            scanned
        }
        Err(e) => {
            return Err(crate::Error::Io(std::io::Error::other(format!(
                "resume_layout_migration: migrate legacy journal: {e}"
            ))));
        }
    };
    if journals.is_empty() {
        return Ok(());
    }
    for journal in journals {
        let current_worktree_canonical = canonicalize_or_lossy(&layout.worktree_root);
        if journal.worktree_canonical != current_worktree_canonical {
            log::warn!("[git_repo_layout] resume: journal worktree mismatch; skipping");
            continue;
        }
        let mut current_journal = journal;
        loop {
            let target_path = PathBuf::from(&current_journal.target_git_dir);
            let target_open = try_open_repo(&target_path)?;
            let claimed_source_path = PathBuf::from(&current_journal.claimed_source);
            let claimed_exists = claimed_source_path.exists();

            match current_journal.phase {
                MigrationPhase::Prepared => {
                    let original_source = PathBuf::from(&current_journal.original_source);
                    let original_exists = original_source.exists();
                    match (original_exists, claimed_exists, &target_open) {
                        (true, false, RepoOpenResult::Missing) => {
                            std::fs::rename(&original_source, &claimed_source_path).map_err(|e| {
                                crate::Error::Io(std::io::Error::other(format!(
                                    "resume_layout_migration: Prepared phase rename: {e}",
                                )))
                            })?;
                            if let Some(parent) = layout.worktree_root.parent() {
                                crate::storage::sync_dir(parent)?;
                            }
                            crate::storage::sync_dir(&layout.worktree_root)?;
                            current_journal = LayoutMigrationJournal { phase: MigrationPhase::SourceClaimed, ..current_journal };
                            write_migration_journal(&target_path, &current_journal)?;
                            continue;
                        }
                        (false, true, RepoOpenResult::Missing) => {
                            current_journal = LayoutMigrationJournal { phase: MigrationPhase::SourceClaimed, ..current_journal };
                            write_migration_journal(&target_path, &current_journal)?;
                            continue;
                        }
                        (_, _, RepoOpenResult::Corrupt(e)) => {
                            return Err(crate::Error::Io(std::io::Error::other(format!(
                                "resume_layout_migration: Prepared phase but target corrupt: {e}",
                            ))));
                        }
                        (_, _, RepoOpenResult::Valid) => {
                            return Err(crate::Error::Io(std::io::Error::other(format!(
                                "resume_layout_migration: Prepared phase but target already valid",
                            ))));
                        }
                        (true, true, _) => {
                            return Err(crate::Error::Io(std::io::Error::other(format!(
                                "resume_layout_migration: Prepared phase ambiguous ownership",
                            ))));
                        }
                        (false, false, RepoOpenResult::Missing) => {
                            return Err(crate::Error::Io(std::io::Error::other(format!(
                                "resume_layout_migration: Prepared phase but both sources missing",
                            ))));
                        }
                    }
                }
                MigrationPhase::SourceClaimed => {
                    let target_tmp_path = PathBuf::from(&current_journal.target_tmp);
                    match &target_open {
                        RepoOpenResult::Valid => {
                            return Err(crate::Error::Io(std::io::Error::other(format!(
                                "resume_layout_migration: SourceClaimed phase but target already valid",
                            ))));
                        }
                        RepoOpenResult::Corrupt(e) => {
                            return Err(crate::Error::Io(std::io::Error::other(format!(
                                "resume_layout_migration: SourceClaimed phase but target corrupt: {e}",
                            ))));
                        }
                        RepoOpenResult::Missing => {}
                    }
                    if claimed_exists {
                        if target_tmp_path.exists() {
                            std::fs::remove_dir_all(&target_tmp_path).map_err(|e| {
                                crate::Error::Io(std::io::Error::other(format!(
                                    "resume_layout_migration: remove stale target_tmp: {e}",
                                )))
                            })?;
                        }
                        if let Some(parent) = target_tmp_path.parent() {
                            std::fs::create_dir_all(parent)?;
                        }
                        migrate_copy_dir_recursive(&claimed_source_path, &target_tmp_path)?;
                        { let tmp_repo = git2::Repository::open(&target_tmp_path).map_err(|e| { crate::Error::Io(std::io::Error::other(format!("resume_layout_migration: open copied repo: {e}"))) })?; let _ = tmp_repo.head(); let _ = tmp_repo.find_reference("HEAD"); }
                        current_journal = LayoutMigrationJournal { phase: MigrationPhase::TargetPrepared, ..current_journal };
                        write_migration_journal(&target_path, &current_journal)?;
                        continue;
                    } else {
                        return Err(crate::Error::Io(std::io::Error::other(format!(
                            "resume_layout_migration: SourceClaimed phase but claimed_source missing",
                        ))));
                    }
                }
                MigrationPhase::TargetPrepared => {
                    let target_tmp_path = PathBuf::from(&current_journal.target_tmp);
                    let target_tmp_exists = target_tmp_path.exists();
                    match (&target_open, target_tmp_exists) {
                        (RepoOpenResult::Valid, false) => {
                            let repo = git2::Repository::open(&target_path).map_err(|e| { crate::Error::Io(std::io::Error::other(format!("resume_layout_migration: TargetPrepared phase open repo: {e}"))) })?;
                            repo.set_workdir(&layout.worktree_root, false).map_err(|e| { crate::Error::Io(std::io::Error::other(format!("resume_layout_migration: set_workdir: {e}"))) })?;
                            let _ = repo.head();
                            if let Ok(mut index) = repo.index() { let _ = index.read(true); }
                            current_journal = LayoutMigrationJournal { phase: MigrationPhase::TargetInstalled, ..current_journal };
                            write_migration_journal(&target_path, &current_journal)?;
                            continue;
                        }
                        (RepoOpenResult::Valid, true) => {
                            return Err(crate::Error::Io(std::io::Error::other(format!(
                                "resume_layout_migration: TargetPrepared phase ownership ambiguity",
                            ))));
                        }
                        (RepoOpenResult::Corrupt(e), _) => {
                            return Err(crate::Error::Io(std::io::Error::other(format!(
                                "resume_layout_migration: TargetPrepared phase but target corrupt: {e}",
                            ))));
                        }
                        (RepoOpenResult::Missing, true) => {
                            if let Some(parent) = target_path.parent() { std::fs::create_dir_all(parent)?; }
                            std::fs::rename(&target_tmp_path, &target_path).map_err(|e| { crate::Error::Io(std::io::Error::other(format!("resume_layout_migration: rename target_tmp: {e}"))) })?;
                            if let Some(parent) = target_path.parent() { crate::storage::sync_dir(parent)?; }
                            let repo = git2::Repository::open(&target_path).map_err(|e| { crate::Error::Io(std::io::Error::other(format!("resume_layout_migration: open final repo: {e}"))) })?;
                            repo.set_workdir(&layout.worktree_root, false).map_err(|e| { crate::Error::Io(std::io::Error::other(format!("resume_layout_migration: set_workdir: {e}"))) })?;
                            let _ = repo.head();
                            if let Ok(mut index) = repo.index() { let _ = index.read(true); }
                            current_journal = LayoutMigrationJournal { phase: MigrationPhase::TargetInstalled, ..current_journal };
                            write_migration_journal(&target_path, &current_journal)?;
                            continue;
                        }
                        (RepoOpenResult::Missing, false) => {
                            return Err(crate::Error::Io(std::io::Error::other(format!(
                                "resume_layout_migration: TargetPrepared phase but both target and tmp missing",
                            ))));
                        }
                    }
                }
                MigrationPhase::TargetInstalled => {
                    if claimed_exists {
                        std::fs::remove_dir_all(&claimed_source_path).map_err(|e| {
                            crate::Error::Io(std::io::Error::other(format!(
                                "resume_layout_migration: TargetInstalled phase remove claimed_source: {e}",
                            )))
                        })?;
                        if let Some(parent) = layout.worktree_root.parent() { crate::storage::sync_dir(parent)?; }
                        crate::storage::sync_dir(&layout.worktree_root)?;
                    }
                    current_journal = LayoutMigrationJournal { phase: MigrationPhase::SourceCleaned, ..current_journal };
                    write_migration_journal(&target_path, &current_journal)?;
                    continue;
                }
                MigrationPhase::SourceCleaned => {
                    remove_migration_journal(&layout.git_dir, &current_journal.owner)?;
                    break;
                }
            }
        }
    }
    Ok(())
}

/// 跨文件系统安全的 .git 迁移（write-ahead journal 状态机版）。
#[allow(clippy::too_many_lines, clippy::excessive_nesting)]
pub(crate) fn migrate_embedded_git(
    default_git_dir: &Path,
    target_git_dir: &Path,
    worktree_root: &Path,
) -> crate::Result<()> {
    let owner = uuid::Uuid::new_v4().to_string();
    let owned_source_name = format!(".git.sujian-migrate-source-{}", owner);
    let owned_source_path = worktree_root.join(&owned_source_name);
    let target_parent = target_git_dir.parent().ok_or_else(|| {
        crate::Error::Io(std::io::Error::other(format!(
            "migrate_embedded_git: target_git_dir has no parent: {}",
            target_git_dir.display(),
        )))
    })?;
    let tmp_git = target_parent.join(format!(".git.sujian-migrate-{}", owner));

    let journal = LayoutMigrationJournal {
        owner: owner.clone(),
        worktree_canonical: canonicalize_or_lossy(worktree_root),
        original_source: default_git_dir.to_string_lossy().into_owned(),
        claimed_source: owned_source_path.to_string_lossy().into_owned(),
        target_tmp: tmp_git.to_string_lossy().into_owned(),
        target_git_dir: target_git_dir.to_string_lossy().into_owned(),
        phase: MigrationPhase::Prepared,
    };
    write_migration_journal(target_git_dir, &journal)?;

    std::fs::rename(default_git_dir, &owned_source_path).map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "migrate_embedded_git: rename source: {e}",
        )))
    })?;
    if let Some(parent) = worktree_root.parent() { crate::storage::sync_dir(parent)?; }
    crate::storage::sync_dir(worktree_root)?;

    let journal = LayoutMigrationJournal { phase: MigrationPhase::SourceClaimed, ..journal };
    write_migration_journal(target_git_dir, &journal)?;

    let mut guard = MigrateTmpDirGuard::new(tmp_git);
    migrate_copy_dir_recursive(&owned_source_path, guard.path())?;
    { let tmp_repo = git2::Repository::open(guard.path()).map_err(|e| { crate::Error::Io(std::io::Error::other(format!("migrate_embedded_git: open tmp repo: {e}"))) })?; let _ = tmp_repo.head(); let _ = tmp_repo.find_reference("HEAD"); }

    let journal = LayoutMigrationJournal { phase: MigrationPhase::TargetPrepared, ..journal };
    write_migration_journal(target_git_dir, &journal)?;

    if let Some(parent) = target_git_dir.parent() { std::fs::create_dir_all(parent)?; }
    let guard_path = guard.disarm();
    std::fs::rename(&guard_path, target_git_dir).map_err(|e| {
        let _ = std::fs::remove_dir_all(&guard_path);
        crate::Error::Io(std::io::Error::other(format!("migrate_embedded_git: rename: {e}")))
    })?;
    if let Some(parent) = target_git_dir.parent() { crate::storage::sync_dir(parent)?; }

    let repo = git2::Repository::open(target_git_dir).map_err(|e| { crate::Error::Io(std::io::Error::other(format!("migrate_embedded_git: open migrated repo: {e}"))) })?;
    repo.set_workdir(worktree_root, false).map_err(|e| { crate::Error::Io(std::io::Error::other(format!("migrate_embedded_git: set_workdir: {e}"))) })?;
    let _ = repo.head();
    if let Ok(mut index) = repo.index() { let _ = index.read(true); }

    let journal = LayoutMigrationJournal { phase: MigrationPhase::TargetInstalled, ..journal };
    write_migration_journal(target_git_dir, &journal)?;

    std::fs::remove_dir_all(&owned_source_path).map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!("migrate_embedded_git: remove owned source: {e}")))
    })?;
    if let Some(parent) = worktree_root.parent() { crate::storage::sync_dir(parent)?; }
    crate::storage::sync_dir(worktree_root)?;

    let journal = LayoutMigrationJournal { phase: MigrationPhase::SourceCleaned, ..journal };
    write_migration_journal(target_git_dir, &journal)?;

    remove_migration_journal(target_git_dir, &owner)?;
    Ok(())
}
