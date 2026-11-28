//! 远端清单存储的严格读取与原子发布 helper。
//!
//! 远端布局与本地 [`ManifestStore`] 同相对路径（相对 `remote_prefix`）：
//!
//! ```text
//! app-meta/sync/manifest/root.json                    权威根：namespace → index sha256
//! app-meta/sync/manifest/index/<ns>/<sha256>.json     object_key → object sha256
//! app-meta/sync/manifest/objects/<sha256>.json        内容寻址的清单对象
//! ```
//!
//! ## 读取
//!
//! 根存在即权威：只读根列出的索引与对象，任何缺失、sha256 不符、JSON 损坏都返回
//! `SyncRemoteError`，**绝不**回退旧单文件或旧分片目录，也不把损坏当成空清单。
//! 索引先读内容寻址路径 `<ns>/<sha256>.json`；缺失时回退 87b15f2 的固定路径
//! `index/<ns>.json`（一次性迁移读取），但内容 sha256 必须等于根记录值。
//! 根不存在才回退旧单文件 `app-meta/sync/manifest.sync.json`，再回退旧分片目录
//! `app-meta/sync/manifest-shards/*.json`（一次性迁移读取）。
//!
//! ## 发布
//!
//! 顺序固定：先写不可变对象（远端已有同名对象直接复用），再写变化的索引，
//! **最后写根**。根是提交点：根写入失败或前置条件失败时本次发布失败，远端旧根
//! 仍是权威；根成功且新根可读后才回收「新根与上一代根都不可达」的对象/索引，
//! 以及旧单文件 / 旧分片目录（回收失败只 warn，不改变已发布事实）。
//!
//! 上一代根 = 本次发布前读到的固定远端 base 根；它的 canonical sha256 由
//! **远端发布决议**写进新根的 `previousRoot`（本地 staging/merge 不得自称
//! 已发布代）。因此根切换后旧根读者仍能解析自己的索引/对象（保留一代）。
//! 旧固定路径索引只有在同 sha 的内容寻址副本已由新根提供时才立即回收；索引
//! 换 sha 或 namespace 被移除时，它仍是上一代根读者的唯一副本，保留一代。
//!
//! 发布根不携带本地血缘：`localPreviousRoot` 在发布视图中恒为 `None`；
//! `previousRoot` 只作为"上一代已发布根"的身份记录，GC 保留边界仍按发布时
//! 实际读到的旧根可达集合结算。远端根与本地最终 namespace 映射相同（语义
//! 未变化）时不重写根，避免无意义版本推进。

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

use crate::sync::cancellation_token::SyncCancellationToken;
use crate::sync::manifest_delta::ManifestDelta;
use crate::sync::provider::model::{DeletePrecondition, RemoteVersion, WritePrecondition};
use crate::sync::provider::SyncProvider;
use crate::sync::types::{ManifestFileRecord, SyncManifest};

use super::{
    is_sha256_hex, namespace_for_path, object_key, sha256_hex, ManifestIndex, ManifestObject,
    ManifestRoot, ManifestStore, ManifestWritePlan, SYNC_MANIFEST_PATH, SYNC_MANIFEST_SHARDS_DIR,
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

fn remote_index_rel(namespace: &str, sha: &str) -> String {
    format!(
        "{}/{}/{}/{}{}",
        remote_storage_dir_rel(),
        INDEX_DIR_NAME,
        namespace,
        sha,
        JSON_SUFFIX
    )
}

/// 87b15f2 旧布局的固定路径远端索引（只读迁移来源）。
fn remote_legacy_index_rel(namespace: &str) -> String {
    format!(
        "{}/{}/{}{}",
        remote_storage_dir_rel(),
        INDEX_DIR_NAME,
        namespace,
        JSON_SUFFIX
    )
}

/// 旧固定路径索引的相对路径 → namespace；内容寻址路径返回 `None`。
fn legacy_remote_index_namespace(rel: &str) -> Option<&str> {
    let prefix = format!("{}/{}/", remote_storage_dir_rel(), INDEX_DIR_NAME);
    let rest = rel.strip_prefix(&prefix)?;
    if rest.contains('/') {
        return None;
    }
    rest.strip_suffix(JSON_SUFFIX)
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

/// 读取根列出的索引：内容寻址路径优先，缺失回退 87b15f2 固定路径。
///
/// 返回 `(bytes, 实际读取路径)`。两种布局都必须让内容 sha256 等于根记录值；
/// 都缺失 → `Err`（消息带 namespace、sha、两条路径），不静默当成空索引。
fn read_remote_index(
    provider: &dyn SyncProvider,
    remote_prefix: &str,
    namespace: &str,
    expected_sha: &str,
) -> crate::Result<(Vec<u8>, String)> {
    if !is_sha256_hex(expected_sha) {
        return Err(remote_error(format!(
            "remote manifest root references invalid index sha256 for namespace {namespace}: \
             {expected_sha}"
        )));
    }
    let hashed_path = remote_path(remote_prefix, &remote_index_rel(namespace, expected_sha));
    if let Some(object) = provider.read(&hashed_path)? {
        let bytes = verify_remote_index_sha(namespace, expected_sha, &hashed_path, object.content)?;
        return Ok((bytes, hashed_path));
    }
    let legacy_path = remote_path(remote_prefix, &remote_legacy_index_rel(namespace));
    match provider.read(&legacy_path)? {
        Some(object) => {
            let bytes =
                verify_remote_index_sha(namespace, expected_sha, &legacy_path, object.content)?;
            Ok((bytes, legacy_path))
        }
        None => Err(remote_error(format!(
            "remote manifest index for namespace {namespace} with sha256 {expected_sha} is \
             missing: {hashed_path} and {legacy_path}"
        ))),
    }
}

fn verify_remote_index_sha(
    namespace: &str,
    expected_sha: &str,
    path: &str,
    content: Vec<u8>,
) -> crate::Result<Vec<u8>> {
    let actual_sha = sha256_hex(&content);
    if actual_sha != expected_sha {
        return Err(remote_error(format!(
            "remote manifest index {path} sha256 mismatch for namespace {namespace}: root expects \
             {expected_sha}, actual {actual_sha}"
        )));
    }
    Ok(content)
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
        let (index_bytes, index_path) =
            read_remote_index(provider, remote_prefix, namespace, index_sha)?;
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
/// `manifest` 为 `Some` 时用 [`ManifestStore::plan_write_all`] 从这份快照推导
/// 完整可达集合与字节；为 `None` 时严格按现有本地根读取，根不存在返回 `Err`
/// （不能发布空权威）。
///
/// 发布根按发布视图重新构造：generation 由 namespace 映射内容派生（同一份
/// 清单得到逐字节一致的根），`localPreviousRoot` 被剥离。本入口的调用方
/// （LWW 全量发布 batch）没有携带固定远端 base 根身份，因此 `previousRoot`
/// 留空而不是冒充本地根；REST 发布入口
/// [`publish_remote_manifest`] 通过 [`local_manifest_storage_with_base`] 传入
/// 本轮读取的远端 base 根。
pub(crate) fn local_manifest_storage(
    store: &ManifestStore,
    manifest: Option<&SyncManifest>,
) -> crate::Result<Vec<ManifestStorageFile>> {
    local_manifest_storage_with_base(store, manifest, None)
}

fn local_manifest_storage_with_base(
    store: &ManifestStore,
    manifest: Option<&SyncManifest>,
    previous_published_root: Option<&str>,
) -> crate::Result<Vec<ManifestStorageFile>> {
    let mut files = match manifest {
        Some(manifest) => {
            let plan = store.plan_write_all(manifest, "")?;
            storage_files_from_plan(store, &plan, previous_published_root)?
        }
        None => {
            // 严格按现有本地根读取；旧固定路径索引若尚未迁移，`load_publish_files`
            // 会把它的字节带回内容寻址路径，发布时一并完成迁移。
            let Some(view) = store.load_publish_files(previous_published_root)? else {
                return Err(crate::Error::Other(format!(
                    "manifest root {} missing — refusing to publish an empty authority",
                    store.sync_root().join(store.root_rel()).display()
                )));
            };
            view.into_iter()
                .map(|(rel, bytes)| ManifestStorageFile {
                    kind: storage_kind(store, &rel),
                    rel,
                    bytes,
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

/// 稀疏轮的增量发布视图：只返回本 delta 涉及的 objects/indexes 与根。
///
/// merge 已把 `ManifestDelta` 持久化进本地 [`ManifestStore`]（`apply_delta`
/// + 一次事务，本地根已指向新索引/对象）。因此发布端不能再调
/// `plan_write_all` 展开完整可达集合，也不重新 `apply_delta`（那会重算全量
/// 可达集合、且只报告根变化）：这里只按 delta 涉及的 namespace/object_key
/// 从当前根取出变化文件的 identity 与字节。
///
/// - root 不存在 → `Err`（调用方回退完整入口，不发布空权威）；
/// - 只读 delta 涉及的索引与对象，未变化 object/index 不加载；
/// - 顺序 objects → indexes → root，根永远最后；
/// - 根是**发布视图**：`previousRoot` 只由远端发布决议填写，LWW batch 调用方
///   没有携带固定远端 base 根身份，因此这里不冒充任何已发布上一代（留空），
///   且不携带本地血缘（`localPreviousRoot` = None）；generation 由最终
///   namespace 映射内容派生，同一份清单内容稳定。
pub(crate) fn local_manifest_storage_for_delta(
    store: &ManifestStore,
    delta: &ManifestDelta,
) -> crate::Result<Vec<ManifestStorageFile>> {
    let Some(root) = store.load_root()? else {
        return Err(crate::Error::Other(format!(
            "manifest root {} is missing — refusing to publish a delta without authority",
            store.sync_root().join(store.root_rel()).display()
        )));
    };

    // namespace → 受影响 object_key 集合。upsert 与 delete 都可能让对象/
    // 索引换内容；不在 delta 里的 namespace/object 一律不碰。
    let mut affected: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut upserted: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for path in delta.upserts.keys() {
        let namespace = namespace_for_path(path);
        let key = object_key(path);
        affected
            .entry(namespace.clone())
            .or_default()
            .insert(key.clone());
        upserted.entry(namespace).or_default().insert(key);
    }
    for path in &delta.deletes {
        affected
            .entry(namespace_for_path(path))
            .or_default()
            .insert(object_key(path));
    }

    let mut files: BTreeMap<String, ManifestStorageFile> = BTreeMap::new();
    for (namespace, keys) in &affected {
        let Some(index_sha) = root.namespaces.get(namespace) else {
            if upserted.get(namespace).is_some_and(|keys| !keys.is_empty()) {
                // upsert 的 namespace 不在当前根里：本地 store 不是 delta 的
                // 超集（未应用或已损坏），发布根会丢数据 → 回退完整入口。
                return Err(crate::Error::Other(format!(
                    "manifest delta upserts namespace {namespace} that is missing from the local \
                     root — local store does not reflect the delta"
                )));
            }
            // namespace 被整段删除（对象为空）：新根不再引用它，没有索引要发布。
            continue;
        };
        let index_rel = store.index_rel(namespace, index_sha);
        let (index, index_bytes, _) = store.load_publish_index(namespace, index_sha)?;
        files.insert(
            index_rel.clone(),
            ManifestStorageFile {
                rel: index_rel,
                kind: ManifestStorageKind::Index,
                bytes: Some(index_bytes),
            },
        );
        for key in keys {
            let Some(object_sha) = index.objects.get(key) else {
                if upserted
                    .get(namespace)
                    .is_some_and(|keys| keys.contains(key))
                {
                    return Err(crate::Error::Other(format!(
                        "manifest delta upserts object key {key} (namespace {namespace}) that is \
                         missing from the local index — local store does not reflect the delta"
                    )));
                }
                // 对象被删空：索引不再引用，发布端不做任何回收。
                continue;
            };
            let rel = store.object_rel(object_sha);
            let path = store.sync_root().join(&rel);
            let bytes = std::fs::read(&path).map_err(|error| {
                crate::Error::Io(std::io::Error::new(
                    error.kind(),
                    format!(
                        "manifest object {} referenced by namespace {namespace} is not readable: \
                         {error}",
                        path.display()
                    ),
                ))
            })?;
            // 对象必须已经反映 delta：upsert 记录在、delete 路径不在。否则本地
            // store 不是 delta 的超集（merge 未持久化或存储损坏），发布根会
            // 指向旧内容 → 回退完整入口。
            let object: ManifestObject = serde_json::from_slice(&bytes).map_err(|error| {
                crate::Error::Other(format!(
                    "manifest object {} is corrupt: {error}",
                    path.display()
                ))
            })?;
            for record in delta.upserts.values() {
                if object_key(&record.path) != *key {
                    continue;
                }
                if !object.files.iter().any(|existing| existing == record) {
                    return Err(crate::Error::Other(format!(
                        "manifest object {} does not reflect delta upsert {} — local store does \
                         not reflect the delta",
                        path.display(),
                        record.path
                    )));
                }
            }
            if object.files.iter().any(|existing| {
                delta.deletes.contains(&existing.path) && object_key(&existing.path) == *key
            }) {
                return Err(crate::Error::Other(format!(
                    "manifest object {} still contains a delta delete — local store does not \
                     reflect the delta",
                    path.display()
                )));
            }
            files.insert(
                rel.clone(),
                ManifestStorageFile {
                    rel,
                    kind: ManifestStorageKind::Object,
                    bytes: Some(bytes),
                },
            );
        }
    }

    // 根是提交点，最后一项。发布视图不冒充"已发布上一代"：previousRoot 只由
    // 远端发布决议（publish_remote_manifest）按本轮读取的远端 base 根填写，
    // 这里留空；localPreviousRoot 不得进入发布视图。generation 由最终
    // namespace 映射内容派生（同内容稳定，不因随机串每轮制造伪提交）。
    let new_root = ManifestRoot::published(root.namespaces.clone(), None);
    let root_rel = store.root_rel();
    files.insert(
        root_rel.clone(),
        ManifestStorageFile {
            rel: root_rel,
            kind: ManifestStorageKind::Root,
            bytes: Some(new_root.canonical_bytes()?),
        },
    );

    let mut files: Vec<ManifestStorageFile> = files.into_values().collect();
    files.sort_by(|a, b| {
        a.kind
            .rank()
            .cmp(&b.kind.rank())
            .then_with(|| a.rel.cmp(&b.rel))
    });
    Ok(files)
}

fn storage_files_from_plan(
    store: &ManifestStore,
    plan: &ManifestWritePlan,
    previous_published_root: Option<&str>,
) -> crate::Result<Vec<ManifestStorageFile>> {
    let writes: BTreeMap<&str, &[u8]> = plan
        .writes
        .iter()
        .map(|(rel, bytes)| (rel.as_str(), bytes.as_slice()))
        .collect();
    // 最终 namespace 映射：本计划写根时取新根字节，否则取当前本地根。
    let namespaces = match writes.get(plan.root_rel.as_str()) {
        Some(bytes) => {
            let root: ManifestRoot = serde_json::from_slice(bytes).map_err(|error| {
                crate::Error::Other(format!("manifest plan root is unreadable: {error}"))
            })?;
            root.namespaces
        }
        None => store
            .load_root()?
            .map(|root| root.namespaces)
            .unwrap_or_default(),
    };
    // 发布视图重新构造根：远端 base 身份只由发布决议填写，本地血缘剥离。
    let publish_root_bytes =
        ManifestRoot::published(namespaces, previous_published_root).canonical_bytes()?;
    let mut files: Vec<ManifestStorageFile> = plan
        .reachable
        .iter()
        .map(|rel| ManifestStorageFile {
            kind: storage_kind(store, rel),
            rel: rel.clone(),
            bytes: if rel == &plan.root_rel {
                Some(publish_root_bytes.clone())
            } else {
                writes.get(rel.as_str()).map(|bytes| (*bytes).to_vec())
            },
        })
        .collect();
    files.sort_by(|a, b| {
        a.kind
            .rank()
            .cmp(&b.kind.rank())
            .then_with(|| a.rel.cmp(&b.rel))
    });
    Ok(files)
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

/// 远端当前根的可达集合（发布端回收旧对象/索引用），并保存本轮读取的固定
/// base 根身份。
struct RemoteManifestState {
    /// 本轮读取的远端根原始字节；语义未变化时用它确认远端根未被并发改写。
    root_bytes: Vec<u8>,
    /// 远端根 canonical bytes 的 sha256（发布根 `previousRoot` 的唯一来源）。
    root_sha256: String,
    /// 远端旧根解析结果（namespaces 等）。
    root: ManifestRoot,
    /// 相对 `remote_prefix` 的 root/index/object 路径（索引用内容寻址路径）。
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
    let root_bytes = object.content;
    let root = parse_root(&root_bytes, &root_path)?;
    // 身份按 canonical bytes 结算：外部写入的非规范序列化不影响"上一代"身份。
    let root_sha256 = root.canonical_sha256()?;
    let mut reachable = BTreeSet::new();
    reachable.insert(root_rel);
    for (namespace, index_sha) in &root.namespaces {
        let (index_bytes, index_path) =
            read_remote_index(provider, remote_prefix, namespace, index_sha)?;
        let index = parse_index(&index_bytes, &index_path, namespace)?;
        for object_sha in index.objects.values() {
            reachable.insert(remote_object_rel(object_sha));
        }
        // 可达集合用内容寻址路径表示：旧固定路径只是迁移来源，回收判断按 sha
        // 身份而不是物理冗余副本。
        reachable.insert(remote_index_rel(namespace, index_sha));
    }
    Ok(Some(RemoteManifestState {
        root_bytes,
        root_sha256,
        root,
        reachable,
    }))
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

/// 根切换后确认新根可读：root 字节一致，且根列出的每个内容寻址索引都存在、
/// sha256 匹配。任一失败 → `Err`，调用方不再执行任何回收。
fn verify_published_remote_root(
    provider: &dyn SyncProvider,
    remote_prefix: &str,
    expected_root_bytes: &[u8],
) -> crate::Result<()> {
    let root_path = remote_path(remote_prefix, &remote_root_rel());
    let Some(object) = provider.read(&root_path)? else {
        return Err(remote_error(format!(
            "published remote manifest root is not readable: {root_path}"
        )));
    };
    if object.content != expected_root_bytes {
        return Err(remote_error(format!(
            "published remote manifest root {root_path} does not match the bytes just written"
        )));
    }
    let root = parse_root(&object.content, &root_path)?;
    for (namespace, index_sha) in &root.namespaces {
        let index_path = remote_path(remote_prefix, &remote_index_rel(namespace, index_sha));
        let Some(index_object) = provider.read(&index_path)? else {
            return Err(remote_error(format!(
                "published remote manifest index {index_path} for namespace {namespace} \
                 (sha256 {index_sha}) is not readable"
            )));
        };
        verify_remote_index_sha(namespace, index_sha, &index_path, index_object.content)?;
    }
    Ok(())
}

/// 逐文件发布 v2 清单（REST / 非 batch 路径）：对象 → 索引 → 根，根最后写。
///
/// 根写入失败直接返回 `Err`（远端旧根仍是权威）；根成功并确认新根可读后，才
/// 回收「新根与上一代根都不可达」的对象/索引，以及旧单文件 / 旧分片目录，
/// 回收失败只 warn。
///
/// 版本身份：
/// - 发布根 `previousRoot` = 本轮读取的固定远端 base 根 canonical sha256，
///   只在这里（远端发布决议）填写；本地根不自称已发布代；
/// - 远端根与本地最终 namespace 映射一致（清单语义未变化）时不重写根，
///   只补齐缺失的 objects/indexes；
/// - 回收边界仍按发布时实际读到的旧根可达集合结算，`previousRoot` 只作身份
///   记录，不参与删除判定。
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
    // 先读本轮固定远端 base 根：发布根 previousRoot 只能来自它。
    let old_state = read_remote_manifest_state(provider, remote_prefix)?;
    let previous_published_root = old_state.as_ref().map(|state| state.root_sha256.as_str());
    let files = local_manifest_storage_with_base(&store, None, previous_published_root)?;
    let mut new_rels: BTreeSet<String> = BTreeSet::new();
    for file in &files {
        new_rels.insert(remote_rel_for(&store, &file.rel)?);
    }

    let root_file = files
        .iter()
        .find(|file| file.kind == ManifestStorageKind::Root)
        .ok_or_else(|| {
            crate::Error::Other(format!(
                "manifest publish plan for {remote_prefix} has no root file"
            ))
        })?;
    let root_bytes = root_file.read_bytes(sync_root)?;
    let publish_root: ManifestRoot = serde_json::from_slice(&root_bytes).map_err(|error| {
        crate::Error::Other(format!(
            "manifest publish root for {remote_prefix} is unreadable: {error}"
        ))
    })?;
    // 远端根已经与本地最终 namespace 映射一致：清单语义未变化，不推进根版本。
    let root_unchanged = old_state
        .as_ref()
        .is_some_and(|state| state.root.namespaces == publish_root.namespaces);

    let caps = provider.capabilities();
    for file in &files {
        if file.kind == ManifestStorageKind::Root && root_unchanged {
            continue;
        }
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

    // 提交点确认：写根时确认新根字节与索引可读；语义未变化时确认远端根仍是
    // 本轮读到的固定 base（没有并发改写）。
    let verified_root_bytes: &[u8] = match (&old_state, root_unchanged) {
        (Some(state), true) => &state.root_bytes,
        _ => &root_bytes,
    };
    verify_published_remote_root(provider, remote_prefix, verified_root_bytes)?;

    // 上一代已发布根的可达集合：根切换后保留一代，旧根读者仍能解析自己的
    // 索引/对象。保留边界按本轮实际读到的远端旧根可达集合结算；旧根上的
    // `previousRoot` 字段只作身份记录，不参与这里的删除判定。
    let (previous_reachable, previous_namespaces) = match old_state {
        Some(state) => (state.reachable, state.root.namespaces),
        None => (BTreeSet::new(), BTreeMap::new()),
    };
    let index_prefix = format!("{}/{}/", remote_storage_dir_rel(), INDEX_DIR_NAME);
    let object_prefix = format!("{}/{}/", remote_storage_dir_rel(), OBJECTS_DIR_NAME);
    let mut stale: Vec<String> = Vec::new();
    for rel in remote_tree_files.keys() {
        if !rel.starts_with(&index_prefix) && !rel.starts_with(&object_prefix) {
            continue;
        }
        if let Some(namespace) = legacy_remote_index_namespace(rel) {
            // 旧固定路径索引：上一代根仍按该 sha 引用、且新根没有提供同一 sha
            // 的内容寻址副本时，它是旧根读者的唯一副本，保留一代；否则迁移
            // 已由新根完成，可以回收。
            let Some(object) = provider.read(&remote_path(remote_prefix, rel))? else {
                continue;
            };
            let sha = sha256_hex(&object.content);
            let retained = previous_namespaces.get(namespace) == Some(&sha)
                && !new_rels.contains(&remote_index_rel(namespace, &sha));
            if !retained {
                stale.push(rel.clone());
            }
            continue;
        }
        if !new_rels.contains(rel) && !previous_reachable.contains(rel) {
            stale.push(rel.clone());
        }
    }
    stale.sort();
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
