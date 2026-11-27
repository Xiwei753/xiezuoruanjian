//! Manifest 分片存储。
//!
//! 单文件 `app-meta/sync/manifest.sync.json` 每次合并都要重写一轮；记录变多后，
//! 任意一条业务对象变化都会让整份清单重新序列化、重新落盘、重新上传。这里把
//! 权威清单按"对象顶层命名空间"切成若干分片（与旧清单同目录下的
//! `manifest-shards/`）：
//!
//! - `volumes/...` → `manifest-shards/volumes.json`
//! - `starmaps/...`、`settings/...`、`app-meta/...` 同理
//! - 顶层散文件（如 `project.json`）→ `manifest-shards/_root.json`
//!
//! 合并/确认时只重写受影响分片，其余分片保持逐字节不变。旧单文件在首次
//! 读取时迁移为分片集合（迁移失败保留旧文件并返回错误，不丢记录）；迁移后
//! 分片是唯一权威，旧文件不再写回。

use crate::sync::path::ValidatedSyncPath;
use crate::sync::types::{ManifestFileRecord, SyncManifest};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// 分片目录名（与旧清单文件同级）。
pub(crate) const SHARDS_DIR_NAME: &str = "manifest-shards";

/// 顶层散文件使用的分片键。
const ROOT_SHARD: &str = "_root";
/// 分片文件后缀。
const SHARD_SUFFIX: &str = ".json";

/// 路径 → 分片键：取顶层命名空间；顶层散文件归入 `_root`。
pub(crate) fn shard_key(path: &str) -> String {
    if !path.contains('/') {
        return ROOT_SHARD.to_string();
    }
    let top = path.split('/').next().unwrap_or_default();
    let mut key: String = top
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .take(64)
        .collect();
    if key.is_empty() {
        key = ROOT_SHARD.to_string();
    }
    key
}

fn shard_file_name(key: &str) -> String {
    format!("{key}{SHARD_SUFFIX}")
}

fn manifest_parent(manifest_rel_path: &str) -> String {
    Path::new(manifest_rel_path)
        .parent()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default()
}

/// 分片目录：与给定清单文件同级的 `manifest-shards/`。
pub(crate) fn shards_dir(sync_root: &Path, manifest_rel_path: &str) -> PathBuf {
    let parent = manifest_parent(manifest_rel_path);
    if parent.is_empty() {
        sync_root.join(SHARDS_DIR_NAME)
    } else {
        sync_root.join(parent).join(SHARDS_DIR_NAME)
    }
}

fn shard_rel_path(manifest_rel_path: &str, key: &str) -> String {
    let parent = manifest_parent(manifest_rel_path);
    if parent.is_empty() {
        format!("{SHARDS_DIR_NAME}/{}", shard_file_name(key))
    } else {
        format!("{parent}/{SHARDS_DIR_NAME}/{}", shard_file_name(key))
    }
}

/// 分片目录本身是否属于清单存储路径（扫描/白名单排除用）。
pub(crate) fn is_manifest_storage_path(rel_path: &str) -> bool {
    let rel_path = rel_path.replace('\\', "/");
    rel_path.starts_with(&format!("{SHARDS_DIR_NAME}/"))
        || rel_path.contains(&format!("/{SHARDS_DIR_NAME}/"))
}

fn read_shard(path: &Path) -> crate::Result<SyncManifest> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| {
            crate::Error::Io(std::io::Error::other(format!(
                "manifest shard {} is unreadable: {e}",
                path.display()
            )))
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(SyncManifest::default()),
        Err(e) => Err(crate::Error::Io(e)),
    }
}

fn list_shard_keys(sync_root: &Path, manifest_rel_path: &str) -> crate::Result<Vec<String>> {
    let dir = shards_dir(sync_root, manifest_rel_path);
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(crate::Error::Io(e)),
    };
    let mut keys = Vec::new();
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if let Some(key) = name.strip_suffix(SHARD_SUFFIX) {
            keys.push(key.to_string());
        }
    }
    keys.sort();
    Ok(keys)
}

/// 分片目录是否已经存在（存在即视为分片是权威存储）。
pub(crate) fn shards_present(sync_root: &Path, manifest_rel_path: &str) -> bool {
    shards_dir(sync_root, manifest_rel_path).is_dir()
}

/// 读取全部分片并合并成完整清单。
///
/// 分片目录不存在返回 `None`（调用方回退旧单文件）；任意分片损坏都返回错误，
/// 绝不把损坏分片当成空清单——那会让后续合并误以为这些对象不存在。
pub(crate) fn load_sharded(
    sync_root: &Path,
    manifest_rel_path: &str,
) -> crate::Result<Option<SyncManifest>> {
    if !shards_present(sync_root, manifest_rel_path) {
        return Ok(None);
    }
    let mut by_path: BTreeMap<String, ManifestFileRecord> = BTreeMap::new();
    for key in list_shard_keys(sync_root, manifest_rel_path)? {
        let shard =
            read_shard(&shards_dir(sync_root, manifest_rel_path).join(shard_file_name(&key)))?;
        for record in shard.files {
            if ValidatedSyncPath::new(&record.path).is_err() {
                return Err(crate::Error::Io(std::io::Error::other(format!(
                    "manifest shard {key} contains invalid path {}",
                    record.path
                ))));
            }
            by_path.insert(record.path.clone(), record);
        }
    }
    Ok(Some(SyncManifest {
        files: by_path.into_values().collect(),
    }))
}

/// 读取权威清单：优先分片，回退旧单文件（只读兼容）。
///
/// 旧单文件存在而分片目录不存在时执行一次性迁移：成功写入全部分片后才
/// 认为分片权威；迁移失败保留旧文件并返回错误。
pub(crate) fn load_manifest(
    sync_root: &Path,
    manifest_rel_path: &str,
) -> crate::Result<SyncManifest> {
    if let Some(manifest) = load_sharded(sync_root, manifest_rel_path)? {
        return Ok(manifest);
    }
    let legacy = sync_root.join(manifest_rel_path);
    let manifest = match std::fs::read(&legacy) {
        Ok(bytes) => {
            serde_json::from_slice::<SyncManifest>(&bytes).map_err(|e| {
                crate::Error::SyncRemoteError {
                    category: "local_error".to_string(),
                    context: format!("invalid local manifest {}: {e}", legacy.display()),
                    status: 0,
                    body_preview: String::new(),
                }
            })?
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(SyncManifest::default()),
        Err(e) => return Err(crate::Error::Io(e)),
    };
    write_full(sync_root, manifest_rel_path, &manifest)?;
    Ok(manifest)
}

/// 用完整清单覆盖写全部分片，并删除不再需要的分片。
pub(crate) fn write_full(
    sync_root: &Path,
    manifest_rel_path: &str,
    manifest: &SyncManifest,
) -> crate::Result<()> {
    let plan = plan_persist(sync_root, manifest_rel_path, manifest)?;
    for (rel, bytes) in &plan.writes {
        let path = sync_root.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        crate::storage::transaction::atomic_write_bytes(&path, bytes)?;
    }
    for rel in &plan.deletes {
        match std::fs::remove_file(sync_root.join(rel)) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(crate::Error::Io(e)),
        }
    }
    std::fs::create_dir_all(shards_dir(sync_root, manifest_rel_path))?;
    Ok(())
}

/// 是否是清单文件（项目清单或 App group 清单）。
pub(crate) fn is_manifest_rel_path(rel: &str) -> bool {
    rel == crate::sync::lww::SYNC_MANIFEST_PATH || rel.ends_with("/manifest.sync.json")
}

/// 把完整清单在内存中拆成"分片路径 → canonical 字节"，不落盘。
///
/// 发布端按这份映射逐个对象比较远端已有分片，只上传内容变化的分片。
pub(crate) fn split_into_shards(
    manifest_rel_path: &str,
    manifest: &SyncManifest,
) -> crate::Result<BTreeMap<String, Vec<u8>>> {
    let mut grouped: BTreeMap<String, Vec<ManifestFileRecord>> = BTreeMap::new();
    for record in &manifest.files {
        grouped
            .entry(shard_key(&record.path))
            .or_default()
            .push(record.clone());
    }
    let mut out = BTreeMap::new();
    for (key, files) in grouped {
        let target = SyncManifest { files };
        out.insert(
            shard_rel_path(manifest_rel_path, &key),
            target.canonical_json()?.into_bytes(),
        );
    }
    if out.is_empty() {
        // 空清单也必须发布一个稳定的根分片：远端"没有清单"与"清单为空"
        // 是两种不同状态，发布端需要一次真实对象写入来承载本次业务提交。
        out.insert(
            shard_rel_path(manifest_rel_path, ROOT_SHARD),
            SyncManifest::default().canonical_json()?.into_bytes(),
        );
    }
    Ok(out)
}

/// 一次持久化计划的产物：只包含内容真的变化的分片写入与需要回收的旧分片。
#[derive(Debug, Default)]
pub(crate) struct ShardPlan {
    /// (相对同步根的路径, 字节内容)
    pub writes: Vec<(String, Vec<u8>)>,
    /// 需要删除的旧分片（相对路径）。
    pub deletes: Vec<String>,
    /// 内容未变、无需重写的分片数。
    pub unchanged: usize,
}

impl ShardPlan {
    pub(crate) fn written(&self) -> usize {
        self.writes.len()
    }
}

/// 计算把权威清单落成文件所需的写入/删除集合。
///
/// 分片内容与磁盘现状语义相同则跳过：一次只改动几章正文时，只有对应
/// `volumes` 分片被重写，其他分片保持逐字节不变。
pub(crate) fn plan_persist(
    sync_root: &Path,
    manifest_rel_path: &str,
    manifest: &SyncManifest,
) -> crate::Result<ShardPlan> {
    let mut grouped: BTreeMap<String, Vec<ManifestFileRecord>> = BTreeMap::new();
    for record in &manifest.files {
        grouped
            .entry(shard_key(&record.path))
            .or_default()
            .push(record.clone());
    }

    let mut plan = ShardPlan::default();
    for (key, files) in &grouped {
        let target = SyncManifest {
            files: files.clone(),
        };
        let rel = shard_rel_path(manifest_rel_path, key);
        let current = read_shard(&sync_root.join(&rel))?;
        if current.semantically_equals(&target) {
            plan.unchanged += 1;
            continue;
        }
        plan.writes.push((rel, target.canonical_json()?.into_bytes()));
    }

    for key in list_shard_keys(sync_root, manifest_rel_path)? {
        if !grouped.contains_key(&key) {
            plan.deletes.push(shard_rel_path(manifest_rel_path, &key));
        }
    }
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(path: &str, hash: &str) -> ManifestFileRecord {
        ManifestFileRecord {
            path: path.to_string(),
            content_hash: hash.to_string(),
            updated_at_ms: 1,
            deleted_at_ms: None,
            device_id: "dev".to_string(),
            op: "upsert".to_string(),
            schema_version: 1,
        }
    }

    const MANIFEST: &str = "app-meta/sync/manifest.sync.json";
    const GROUP_MANIFEST: &str = "app-meta/sync/groups/app/manifest.sync.json";

    #[test]
    fn shard_key_groups_by_top_level_namespace() {
        assert_eq!(shard_key("volumes/v1/chapters/c1/chapter.md"), "volumes");
        assert_eq!(shard_key("starmaps/a.json"), "starmaps");
        assert_eq!(shard_key("project.json"), "_root");
        assert_eq!(shard_key("settings"), "_root");
        assert_eq!(shard_key("时间线/a.json"), "___");
    }

    #[test]
    fn plan_only_rewrites_changed_shards() {
        let dir = tempfile::tempdir().unwrap();
        write_full(
            dir.path(),
            MANIFEST,
            &SyncManifest {
                files: vec![record("volumes/a.json", "h1"), record("starmaps/b.json", "h2")],
            },
        )
        .unwrap();
        let before = std::fs::read(shards_dir(dir.path(), MANIFEST).join("volumes.json")).unwrap();

        let plan = plan_persist(
            dir.path(),
            MANIFEST,
            &SyncManifest {
                files: vec![record("volumes/a.json", "h1"), record("starmaps/b.json", "h3")],
            },
        )
        .unwrap();
        assert_eq!(plan.written(), 1);
        assert_eq!(plan.unchanged, 1);
        let mut tx = crate::storage::transaction::SaveTransaction::new(dir.path());
        for (rel, bytes) in &plan.writes {
            tx.add_bytes(rel, bytes).unwrap();
        }
        tx.commit().unwrap();
        assert_eq!(
            std::fs::read(shards_dir(dir.path(), MANIFEST).join("volumes.json")).unwrap(),
            before
        );
        let merged = load_sharded(dir.path(), MANIFEST).unwrap().unwrap();
        assert_eq!(
            merged
                .files
                .iter()
                .find(|r| r.path == "starmaps/b.json")
                .unwrap()
                .content_hash,
            "h3"
        );
    }

    #[test]
    fn plan_deletes_shards_without_records() {
        let dir = tempfile::tempdir().unwrap();
        write_full(
            dir.path(),
            MANIFEST,
            &SyncManifest {
                files: vec![record("volumes/a.json", "h1"), record("project.json", "h2")],
            },
        )
        .unwrap();
        let plan = plan_persist(
            dir.path(),
            MANIFEST,
            &SyncManifest {
                files: vec![record("project.json", "h2")],
            },
        )
        .unwrap();
        assert_eq!(
            plan.deletes,
            vec!["app-meta/sync/manifest-shards/volumes.json".to_string()]
        );
    }

    #[test]
    fn legacy_manifest_is_migrated_on_first_load() {
        let dir = tempfile::tempdir().unwrap();
        let legacy = SyncManifest {
            files: vec![record("volumes/a.json", "h1"), record("project.json", "h2")],
        };
        std::fs::create_dir_all(dir.path().join("app-meta/sync")).unwrap();
        std::fs::write(dir.path().join(MANIFEST), legacy.canonical_json().unwrap()).unwrap();
        let loaded = load_manifest(dir.path(), MANIFEST).unwrap();
        assert!(loaded.semantically_equals(&legacy));
        assert!(shards_dir(dir.path(), MANIFEST)
            .join("volumes.json")
            .is_file());
        assert!(shards_dir(dir.path(), MANIFEST)
            .join("_root.json")
            .is_file());
    }

    #[test]
    fn group_manifest_uses_sibling_shards_dir() {
        let dir = tempfile::tempdir().unwrap();
        write_full(
            dir.path(),
            GROUP_MANIFEST,
            &SyncManifest {
                files: vec![record("settings/app.json", "h1")],
            },
        )
        .unwrap();
        assert!(dir
            .path()
            .join("app-meta/sync/groups/app/manifest-shards/settings.json")
            .is_file());
        assert!(is_manifest_storage_path(
            "app-meta/sync/groups/app/manifest-shards/settings.json"
        ));
    }

    #[test]
    fn corrupt_shard_is_reported_not_treated_as_empty() {
        let dir = tempfile::tempdir().unwrap();
        write_full(
            dir.path(),
            MANIFEST,
            &SyncManifest {
                files: vec![record("volumes/a.json", "h1")],
            },
        )
        .unwrap();
        std::fs::write(
            shards_dir(dir.path(), MANIFEST).join("volumes.json"),
            b"{not json",
        )
        .unwrap();
        assert!(load_sharded(dir.path(), MANIFEST).is_err());
    }
}
