//! 远端清单存储的严格读取与原子发布 helper。
//!
//! 远端布局与本地 [`ManifestStore`] 同相对路径（相对 `remote_prefix`）：
//!
//! ```text
//! app-meta/sync/manifest/root.json               权威根：namespace → index sha256
//! app-meta/sync/manifest/index/<ns>.json         object_key → object sha256
//! app-meta/sync/manifest/objects/<sha256>.json   内容寻址的清单对象
//! ```
//!
//! ## 读取
//!
//! 根存在即权威：只读根列出的索引与对象，任何缺失、sha256 不符、JSON 损坏都返回
//! `SyncRemoteError`，**绝不**回退旧单文件或旧分片目录，也不把损坏当成空清单。
//! 根不存在才回退旧单文件 `app-meta/sync/manifest.sync.json`，再回退旧分片目录
//! `app-meta/sync/manifest-shards/*.json`（一次性迁移读取）。
//!
//! ## 发布
//!
//! 顺序固定：先写不可变对象（远端已有同名对象直接复用），再写变化的索引，
//! **最后写根**。根是提交点：根写入失败或前置条件失败时本次发布失败，远端旧根
//! 仍是权威；根成功后才回收旧根可达但新根不可达的对象/索引，以及旧单文件 /
//! 旧分片目录（回收失败只 warn，不改变已发布事实）。

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

use crate::sync::cancellation_token::SyncCancellationToken;
use crate::sync::provider::model::{DeletePrecondition, RemoteVersion, WritePrecondition};
use crate::sync::provider::SyncProvider;
use crate::sync::types::{ManifestFileRecord, SyncManifest};

use super::{
    sha256_hex, ManifestIndex, ManifestObject, ManifestRoot, ManifestStore, ManifestWritePlan,
    SYNC_MANIFEST_PATH, SYNC_MANIFEST_SHARDS_DIR,
};

const MANIFEST_DIR_NAME: &str = "manifest";
const INDEX_DIR_NAME: &str = "index";
const OBJECTS_DIR_NAME: &str = "objects";
const ROOT_FILE_NAME: &str = "root.json";
const JSON_SUFFIX: &str = ".json";

/// 远端清单存储里的文件类别；决定发布顺序（对象 → 索引 → 根）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ManifestStorageKind {
    Object,
    Index,
    Root,
}

impl ManifestStorageKind {
    fn rank(self) -> u8 {
        match self {
            ManifestStorageKind::Object => 0,
            ManifestStorageKind::Index => 1,
            ManifestStorageKind::Root => 2,
        }
    }
}

/// 本地 v2 存储中的一个待发布文件。
///
/// `bytes` 为 `Some` 表示计划刚生成、磁盘上可能还没有的字节；`None` 表示按
/// `rel` 从本地存储读取（内容寻址对象已落盘，无需重复序列化）。
#[derive(Debug, Clone)]
pub(crate) struct ManifestStorageFile {
    /// 相对 sync_root 的本地存储路径。
    pub rel: String,
    pub kind: ManifestStorageKind,
    bytes: Option<Vec<u8>>,
}

impl ManifestStorageFile {
    /// 取发布字节：优先用计划生成的字节，否则读本地存储文件。
    pub(crate) fn read_bytes(&self, sync_root: &Path) -> crate::Result<Vec<u8>> {
        if let Some(bytes) = &self.bytes {
            return Ok(bytes.clone());
        }
        let path = sync_root.join(&self.rel);
        std::fs::read(&path).map_err(|error| {
            crate::Error::Io(std::io::Error::new(
                error.kind(),
                format!(
                    "read local manifest storage file {}: {error}",
                    path.display()
                ),
            ))
        })
    }
}

fn remote_error(context: impl Into<String>) -> crate::Error {
    crate::Error::SyncRemoteError {
        category: "api_error".to_string(),
        context: context.into(),
        status: 0,
        body_preview: String::new(),
    }
}

/// `app-meta/sync`（旧单文件所在目录）。
fn sync_meta_dir_rel() -> String {
    match SYNC_MANIFEST_PATH.rsplit_once('/') {
        Some((parent, _)) => parent.to_string(),
        None => String::new(),
    }
}

/// 远端清单存储目录（相对 `remote_prefix`）。
fn remote_storage_dir_rel() -> String {
    let parent = sync_meta_dir_rel();
    if parent.is_empty() {
        MANIFEST_DIR_NAME.to_string()
    } else {
        format!("{parent}/{MANIFEST_DIR_NAME}")
    }
}

fn remote_root_rel() -> String {
    format!("{}/{}", remote_storage_dir_rel(), ROOT_FILE_NAME)
}

fn remote_index_rel(namespace: &str) -> String {
    format!(
        "{}/{}/{}{}",
        remote_storage_dir_rel(),
        INDEX_DIR_NAME,
        namespace,
        JSON_SUFFIX
    )
}

fn remote_object_rel(sha: &str) -> String {
    format!(
        "{}/{}/{}{}",
        remote_storage_dir_rel(),
        OBJECTS_DIR_NAME,
        sha,
        JSON_SUFFIX
    )
}

fn remote_path(prefix: &str, rel: &str) -> String {
    if prefix.is_empty() {
        rel.to_string()
    } else {
        format!("{prefix}/{rel}")
    }
}

fn legacy_shards_prefix() -> String {
    format!("{SYNC_MANIFEST_SHARDS_DIR}/")
}

/// 本地存储路径 → 远端相对路径（相对 `remote_prefix`）。
///
/// 项目清单的存储目录与远端一致；App group 的本地清单位于
/// `app-meta/sync/groups/<key>/manifest/`，但远端一直固定在
/// `<prefix>/app-meta/sync/manifest/`（v1 单文件同样固定在该目录），因此只替换
/// 存储目录前缀，文件名/两级索引结构保持不变。
pub(crate) fn remote_rel_for(store: &ManifestStore, local_rel: &str) -> crate::Result<String> {
    let storage_prefix = format!("{}/", store.storage_dir_rel());
    let suffix = local_rel.strip_prefix(&storage_prefix).ok_or_else(|| {
        crate::Error::Other(format!(
            "manifest storage file {local_rel} is outside {}",
            store.storage_dir_rel()
        ))
    })?;
    Ok(format!("{}/{}", remote_storage_dir_rel(), suffix))
}

// ── 读取 ──

/// 按远端 tree 选择清单来源后严格读取。
///
/// 根存在 → 只读 v2；否则旧单文件；否则旧分片目录；都没有 → 空清单。
pub(crate) fn fetch_remote_manifest(
    provider: &dyn SyncProvider,
    remote_prefix: &str,
    remote_tree_files: &HashMap<String, String>,
) -> crate::Result<SyncManifest> {
    let root_rel = remote_root_rel();
    if remote_tree_files.contains_key(&root_rel) {
        let root_bytes = read_required(provider, remote_prefix, &root_rel, "root")?;
        return read_v2_manifest(provider, remote_prefix, &root_bytes);
    }
    if remote_tree_files.contains_key(SYNC_MANIFEST_PATH) {
        return Ok(read_legacy_single(provider, remote_prefix)?.unwrap_or_default());
    }
    let shard_prefix = legacy_shards_prefix();
    let mut shard_paths: Vec<String> = remote_tree_files
        .keys()
        .filter(|path| path.starts_with(&shard_prefix))
        .cloned()
        .collect();
    if shard_paths.is_empty() {
        return Ok(SyncManifest::default());
    }
    shard_paths.sort();
    read_legacy_shards(provider, remote_prefix, &shard_paths)
}

/// 不需要远端 tree 的版本：直接探测根 → 旧单文件 → 旧分片目录。
///
/// 三种来源都不存在 → `Ok(None)`（调用方决定是否允许"没有清单"）；
/// 任一来源存在但损坏/校验失败 → `Err`，不静默当成没有清单。
pub(crate) fn fetch_remote_manifest_at(
    provider: &dyn SyncProvider,
    remote_prefix: &str,
) -> crate::Result<Option<SyncManifest>> {
    let root_rel = remote_root_rel();
    if let Some(object) = provider.read(&remote_path(remote_prefix, &root_rel))? {
        return read_v2_manifest(provider, remote_prefix, &object.content).map(Some);
    }
    let legacy_rel = SYNC_MANIFEST_PATH;
    if let Some(object) = provider.read(&remote_path(remote_prefix, legacy_rel))? {
        let origin = remote_path(remote_prefix, legacy_rel);
        return parse_legacy_single(&object.content, &origin).map(Some);
    }
    let shard_prefix = legacy_shards_prefix();
    let mut shard_paths: Vec<String> = provider
        .list(remote_prefix)?
        .into_iter()
        .map(|entry| entry.path)
        .filter(|path| path.starts_with(&shard_prefix))
        .collect();
    if shard_paths.is_empty() {
        return Ok(None);
    }
    shard_paths.sort();
    read_legacy_shards(provider, remote_prefix, &shard_paths).map(Some)
}

fn read_required(
    provider: &dyn SyncProvider,
    remote_prefix: &str,
    rel: &str,
    kind: &str,
) -> crate::Result<Vec<u8>> {
    let path = remote_path(remote_prefix, rel);
    match provider.read(&path)? {
        Some(object) => Ok(object.content),
        None => Err(remote_error(format!(
            "remote manifest {kind} missing: {path}"
        ))),
    }
}

fn parse_root(bytes: &[u8], origin: &str) -> crate::Result<ManifestRoot> {
    let root: ManifestRoot = serde_json::from_slice(bytes)
        .map_err(|error| remote_error(format!("invalid remote manifest root {origin}: {error}")))?;
    root.validate(origin)
        .map_err(|error| remote_error(format!("invalid remote manifest root {origin}: {error}")))?;
    Ok(root)
}

fn parse_index(bytes: &[u8], origin: &str, namespace: &str) -> crate::Result<ManifestIndex> {
    let index: ManifestIndex = serde_json::from_slice(bytes).map_err(|error| {
        remote_error(format!("invalid remote manifest index {origin}: {error}"))
    })?;
    index.validate(origin, namespace).map_err(|error| {
        remote_error(format!("invalid remote manifest index {origin}: {error}"))
    })?;
    Ok(index)
}

fn parse_object(bytes: &[u8], origin: &str, key: &str) -> crate::Result<ManifestObject> {
    let object: ManifestObject = serde_json::from_slice(bytes).map_err(|error| {
        remote_error(format!("invalid remote manifest object {origin}: {error}"))
    })?;
    object.validate(origin, key).map_err(|error| {
        remote_error(format!("invalid remote manifest object {origin}: {error}"))
    })?;
    Ok(object)
}

/// 严格读取 v2 清单：根 → 每个索引 → objects 映射里的每个对象。
///
/// 任一文件缺失、sha256 不符、JSON 损坏、路径不属于该 object_key 都返回
/// `SyncRemoteError`；同一 path 出现在两个对象里同样报错，绝不返回部分清单。
fn read_v2_manifest(
    provider: &dyn SyncProvider,
    remote_prefix: &str,
    root_bytes: &[u8],
) -> crate::Result<SyncManifest> {
    let root_rel = remote_root_rel();
    let root = parse_root(root_bytes, &remote_path(remote_prefix, &root_rel))?;
    let mut by_path: BTreeMap<String, ManifestFileRecord> = BTreeMap::new();
    for (namespace, index_sha) in &root.namespaces {
        let index_rel = remote_index_rel(namespace);
        let index_path = remote_path(remote_prefix, &index_rel);
        let index_bytes = read_required(provider, remote_prefix, &index_rel, "index")?;
        let actual_sha = sha256_hex(&index_bytes);
        if actual_sha != *index_sha {
            return Err(remote_error(format!(
                "remote manifest index {index_path} sha256 mismatch: root expects {index_sha}, \
                 actual {actual_sha}"
            )));
        }
        let index = parse_index(&index_bytes, &index_path, namespace)?;
        for (key, object_sha) in &index.objects {
            let object_rel = remote_object_rel(object_sha);
            let object_path = remote_path(remote_prefix, &object_rel);
            let object_bytes = read_required(provider, remote_prefix, &object_rel, "object")?;
            let actual_sha = sha256_hex(&object_bytes);
            if actual_sha != *object_sha {
                return Err(remote_error(format!(
                    "remote manifest object {object_path} sha256 mismatch: index expects \
                     {object_sha}, actual {actual_sha}"
                )));
            }
            let object = parse_object(&object_bytes, &object_path, key)?;
            for record in object.files {
                if by_path.contains_key(&record.path) {
                    return Err(remote_error(format!(
                        "remote manifest contains duplicate path {}",
                        record.path
                    )));
                }
                by_path.insert(record.path.clone(), record);
            }
        }
    }
    Ok(SyncManifest {
        files: by_path.into_values().collect(),
    })
}

fn read_legacy_single(
    provider: &dyn SyncProvider,
    remote_prefix: &str,
) -> crate::Result<Option<SyncManifest>> {
    let path = remote_path(remote_prefix, SYNC_MANIFEST_PATH);
    let Some(object) = provider.read(&path)? else {
        return Ok(None);
    };
    parse_legacy_single(&object.content, &path).map(Some)
}

fn parse_legacy_single(bytes: &[u8], origin: &str) -> crate::Result<SyncManifest> {
    serde_json::from_slice::<SyncManifest>(bytes)
        .map_err(|error| remote_error(format!("invalid remote manifest {origin}: {error}")))
}

/// 合并旧 `manifest-shards/*.json`（每片 `{"files":[...]}`）。
fn read_legacy_shards(
    provider: &dyn SyncProvider,
    remote_prefix: &str,
    shard_paths: &[String],
) -> crate::Result<SyncManifest> {
    let mut by_path: BTreeMap<String, ManifestFileRecord> = BTreeMap::new();
    for rel in shard_paths {
        let path = remote_path(remote_prefix, rel);
        let bytes = read_required(provider, remote_prefix, rel, "shard")?;
        let shard = parse_legacy_single(&bytes, &path)?;
        for record in shard.files {
            by_path.insert(record.path.clone(), record);
        }
    }
    Ok(SyncManifest {
        files: by_path.into_values().collect(),
    })
}

// ── 本地存储视图 ──

/// 本地 v2 存储的待发布视图（objects → indexes → root 顺序）。
///
/// `manifest` 为 `Some` 时用 `plan_write_all` 从这份快照推导完整可达集合与
/// 字节（generation 由内容 sha256 派生，保证同一份清单得到逐字节一致的根，
/// 不因随机 generation 每轮制造伪提交）；为 `None` 时严格按现有本地根读取，
/// 根不存在返回 `Err`（不能发布空权威）。
pub(crate) fn local_manifest_storage(
    store: &ManifestStore,
    manifest: Option<&SyncManifest>,
) -> crate::Result<Vec<ManifestStorageFile>> {
    let mut files = match manifest {
        Some(manifest) => {
            let plan = store.plan_write_all(manifest, &stable_generation(manifest)?)?;
            storage_files_from_plan(store, &plan)
        }
        None => {
            let Some((root_bytes, reachable)) = store.load_reachable()? else {
                return Err(crate::Error::Other(format!(
                    "manifest root {} missing — refusing to publish an empty authority",
                    store.sync_root().join(store.root_rel()).display()
                )));
            };
            let root_rel = store.root_rel();
            reachable
                .into_iter()
                .map(|rel| {
                    let bytes = if rel == root_rel {
                        Some(root_bytes.clone())
                    } else {
                        None
                    };
                    ManifestStorageFile {
                        kind: storage_kind(store, &rel),
                        rel,
                        bytes,
                    }
                })
                .collect()
        }
    };
    files.sort_by(|a, b| {
        a.kind
            .rank()
            .cmp(&b.kind.rank())
            .then_with(|| a.rel.cmp(&b.rel))
    });
    Ok(files)
}

fn stable_generation(manifest: &SyncManifest) -> crate::Result<String> {
    Ok(format!(
        "manifest-{}",
        sha256_hex(manifest.canonical_json()?.as_bytes())
    ))
}

fn storage_files_from_plan(
    store: &ManifestStore,
    plan: &ManifestWritePlan,
) -> Vec<ManifestStorageFile> {
    let writes: BTreeMap<&str, &[u8]> = plan
        .writes
        .iter()
        .map(|(rel, bytes)| (rel.as_str(), bytes.as_slice()))
        .collect();
    plan.reachable
        .iter()
        .map(|rel| ManifestStorageFile {
            kind: storage_kind(store, rel),
            rel: rel.clone(),
            bytes: writes.get(rel.as_str()).map(|bytes| (*bytes).to_vec()),
        })
        .collect()
}

fn storage_kind(store: &ManifestStore, rel: &str) -> ManifestStorageKind {
    if rel == store.root_rel() {
        return ManifestStorageKind::Root;
    }
    let index_prefix = format!("{}/{}/", store.storage_dir_rel(), INDEX_DIR_NAME);
    if rel.starts_with(&index_prefix) {
        ManifestStorageKind::Index
    } else {
        ManifestStorageKind::Object
    }
}

// ── 发布 ──

/// 远端当前根的可达集合（发布端回收旧对象/索引用）。
struct RemoteManifestState {
    /// 相对 `remote_prefix` 的 root/index/object 路径。
    reachable: BTreeSet<String>,
}

/// 读取远端当前根并解析其可达集合；根不存在 → `Ok(None)`。
fn read_remote_manifest_state(
    provider: &dyn SyncProvider,
    remote_prefix: &str,
) -> crate::Result<Option<RemoteManifestState>> {
    let root_rel = remote_root_rel();
    let root_path = remote_path(remote_prefix, &root_rel);
    let Some(object) = provider.read(&root_path)? else {
        return Ok(None);
    };
    let root = parse_root(&object.content, &root_path)?;
    let mut reachable = BTreeSet::new();
    reachable.insert(root_rel);
    for (namespace, index_sha) in &root.namespaces {
        let index_rel = remote_index_rel(namespace);
        let index_path = remote_path(remote_prefix, &index_rel);
        let index_bytes = read_required(provider, remote_prefix, &index_rel, "index")?;
        let actual_sha = sha256_hex(&index_bytes);
        if actual_sha != *index_sha {
            return Err(remote_error(format!(
                "remote manifest index {index_path} sha256 mismatch: root expects {index_sha}, \
                 actual {actual_sha}"
            )));
        }
        let index = parse_index(&index_bytes, &index_path, namespace)?;
        for object_sha in index.objects.values() {
            reachable.insert(remote_object_rel(object_sha));
        }
        reachable.insert(index_rel);
    }
    Ok(Some(RemoteManifestState { reachable }))
}

fn write_precondition(conditional_write: bool, version: Option<&String>) -> WritePrecondition {
    if !conditional_write {
        return WritePrecondition::Unconditional;
    }
    match version {
        Some(version) => WritePrecondition::IfMatch(RemoteVersion(version.clone())),
        None => WritePrecondition::CreateNew,
    }
}

/// 逐文件发布 v2 清单（REST / 非 batch 路径）：对象 → 索引 → 根，根最后写。
///
/// 根写入失败直接返回 `Err`（远端旧根仍是权威）；根成功后才回收旧可达但新根
/// 不可达的对象/索引，以及旧单文件 / 旧分片目录，回收失败只 warn。
#[allow(clippy::too_many_lines)]
pub(crate) fn publish_remote_manifest(
    provider: &dyn SyncProvider,
    remote_prefix: &str,
    sync_root: &Path,
    manifest_rel: &str,
    remote_tree_files: &HashMap<String, String>,
    cancellation_token: Option<&SyncCancellationToken>,
) -> crate::Result<()> {
    if let Some(token) = cancellation_token {
        if token.is_cancelled() {
            log::info!(
                "[sync] publish_remote_manifest: cancellation requested — skipping manifest \
                 publish for {remote_prefix}"
            );
            return Ok(());
        }
    }
    let store = ManifestStore::new(sync_root, manifest_rel);
    let files = local_manifest_storage(&store, None)?;
    let old_state = read_remote_manifest_state(provider, remote_prefix)?;
    let mut new_rels: BTreeSet<String> = BTreeSet::new();
    for file in &files {
        new_rels.insert(remote_rel_for(&store, &file.rel)?);
    }

    let caps = provider.capabilities();
    for file in &files {
        let remote_rel = remote_rel_for(&store, &file.rel)?;
        let path = remote_path(remote_prefix, &remote_rel);
        let version = remote_tree_files.get(&remote_rel);
        let bytes = match file.kind {
            ManifestStorageKind::Object => {
                // 内容寻址：远端已有同名对象直接复用，不重写。
                if version.is_some() {
                    continue;
                }
                file.read_bytes(sync_root)?
            }
            ManifestStorageKind::Index | ManifestStorageKind::Root => {
                let bytes = file.read_bytes(sync_root)?;
                if let Some(object) = provider.read(&path)? {
                    if object.content == bytes {
                        continue;
                    }
                }
                bytes
            }
        };
        provider.write(
            &path,
            &bytes,
            write_precondition(caps.conditional_write, version),
        )?;
    }

    // 根（files 最后一项）已经写入成功，此后才允许回收旧权威的残留。
    let mut stale: Vec<String> = Vec::new();
    if let Some(state) = old_state {
        stale.extend(
            state
                .reachable
                .difference(&new_rels)
                .filter(|rel| remote_tree_files.contains_key(*rel))
                .cloned(),
        );
    }
    let shard_prefix = legacy_shards_prefix();
    let mut legacy: Vec<String> = remote_tree_files
        .keys()
        .filter(|rel| rel.as_str() == SYNC_MANIFEST_PATH || rel.starts_with(&shard_prefix))
        .cloned()
        .collect();
    legacy.sort();
    for rel in stale.into_iter().chain(legacy) {
        let Some(version) = remote_tree_files.get(&rel) else {
            continue;
        };
        let precondition = if caps.conditional_write {
            DeletePrecondition::IfMatch(RemoteVersion(version.clone()))
        } else {
            DeletePrecondition::Unconditional
        };
        if let Err(error) = provider.delete(&remote_path(remote_prefix, &rel), precondition) {
            log::warn!(
                "[sync] publish_remote_manifest: cleanup of stale manifest object {rel} failed \
                 after root switch: {error}"
            );
        }
    }
    Ok(())
}
