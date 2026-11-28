//! 版本化根 + 内容寻址对象 + 两级索引的权威清单存储。
//!
//! 布局（相对 `manifest_rel_path` 的父目录）：
//!
//! ```text
//! manifest/root.json                        权威根：generation + namespace → index sha256
//! manifest/index/<ns>/<sha256>.json         namespace → object_key → object sha256
//! manifest/objects/<sha256>.json            object_key 下的全部文件记录
//! ```
//!
//! 不变量：
//!
//! - `root.json` 存在即权威；旧单文件 `manifest.sync.json` 与旧
//!   `manifest-shards/` 目录只作为迁移输入，迁移后保留但不参与读取。
//! - 索引与对象都是内容寻址的不可变文件：文件名等于内容（canonical JSON）的
//!   sha256；同名文件直接复用，绝不重写。根按 sha 定位索引，因此根切换前
//!   旧根引用的索引不会被覆盖。
//! - `manifest/index/<ns>.json` 是 87b15f2 的旧固定路径布局，只在
//!   `<ns>/<sha256>.json` 缺失时作为一次性迁移读取；下一次写入计划会把它写到
//!   内容寻址路径。只有新根已用同一 sha 提供内容寻址副本时才回收旧固定路径；
//!   索引换 sha 或 namespace 被移除时，它仍是上一代根读者的唯一副本，保留一代。
//! - `ManifestWritePlan::writes` 的对象/索引/根顺序固定，根（若写）永远最后
//!   一项；调用方用 [`crate::storage::transaction::SaveTransaction`] 一次提交，
//!   根写失败时旧根仍指向旧对象，不会出现"根指向不存在对象"。
//! - 根版本只在清单语义（namespace → index sha256 映射）真实变化或首次建立
//!   权威时才推进：`apply_delta`/`plan_write_all`/`plan_sync_manifest` 先算
//!   最终映射并与当前根比较；映射不变时不写根、不回收任何文件，只保留
//!   "补写缺失的内容寻址文件"（例如旧固定路径索引迁移）这类幂等修复写入。
//! - `generation` 由 `(schemaVersion, namespaces, localPreviousRoot)` 内容派生，
//!   调用方传入的 generation 不参与身份；同一内容得到逐字节一致的根。
//! - 本地根的 `previousRoot` 恒为 `None`：本地 staging/merge 不是已发布代，
//!   `localPreviousRoot` 记录写盘前本地根 canonical sha256。
//! - GC 保留一代：根切换后只回收「既不在新根可达集合、也不在上一代根可达
//!   集合」的文件（上一代 = 本次切换读到的旧根）。因此旧根读者在切换后仍能
//!   解析自己的索引/对象。
//! - 任一索引/对象缺失、sha256 不符、JSON 损坏、路径不属于该 object_key
//!   都返回 `Err`；禁止把损坏或缺失静默当成空清单。

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::sync::manifest_delta::ManifestDelta;
use crate::sync::path::ValidatedSyncPath;
use crate::sync::types::{ManifestFileRecord, SyncManifest};

use super::index::{namespace_for_path, ManifestIndex};
use super::objects::{is_sha256_hex, object_key, sha256_hex, ManifestObject};
use super::root::ManifestRoot;

/// v2 存储目录名（与旧清单文件 / 旧分片目录同级）。
pub(crate) const MANIFEST_DIR_NAME: &str = "manifest";
/// 旧分片目录名（仅迁移读取与扫描排除）。
pub(crate) const LEGACY_SHARDS_DIR_NAME: &str = "manifest-shards";

const ROOT_FILE_NAME: &str = "root.json";
const INDEX_DIR_NAME: &str = "index";
const OBJECTS_DIR_NAME: &str = "objects";
const JSON_SUFFIX: &str = ".json";

/// 一次清单持久化计划。
#[derive(Debug, Clone, Default)]
pub(crate) struct ManifestWritePlan {
    /// (相对 sync_root 的路径, canonical 字节)；写根时最后一项固定是 root。
    ///
    /// 清单语义未变化时根不进入 writes；没有需要补写的内容寻址文件时为空。
    pub writes: Vec<(String, Vec<u8>)>,
    /// 本计划后不再可达、需要回收的旧对象/索引（相对路径）。
    ///
    /// 只包含「新根与上一代本地根都不可达」的文件，因此根切换后立即执行
    /// 也不会破坏旧根读者的依赖；执行顺序固定排在根写入之后。清单语义
    /// 未变化（不写根、不推进代次）时恒为空。
    pub deletes: Vec<String>,
    /// root.json 相对 sync_root 的路径。
    // 发布端要按这个路径单独上传/替换根，当前主链路只消费 writes/deletes。
    #[allow(dead_code)]
    pub root_rel: String,
    /// 本计划生成后的全部可达文件（objects + indexes + root）。
    // 发布/GC 接线的输入；本轮先由 plan 计算并保留。
    #[allow(dead_code)]
    pub reachable: BTreeSet<String>,
}

/// 是否等于项目/App group 的旧单文件清单。
pub(crate) fn is_manifest_rel_path(rel: &str) -> bool {
    let rel = normalize_rel(rel);
    rel == super::SYNC_MANIFEST_PATH || rel.ends_with("/manifest.sync.json")
}

/// 是否是清单存储路径（新 `manifest/` 目录、旧 `manifest-shards/`、旧单文件）。
///
/// 扫描/白名单用这个函数把清单存储自身排除在业务对象之外，避免清单收录
/// 自己的对象文件形成递归。
pub(crate) fn is_manifest_storage_path(rel: &str) -> bool {
    let rel = normalize_rel(rel);
    if is_manifest_rel_path(&rel) {
        return true;
    }
    let is_dir_path = |name: &str| {
        rel == name
            || rel.starts_with(&format!("{name}/"))
            || rel.contains(&format!("/{name}/"))
            || rel.ends_with(&format!("/{name}"))
    };
    is_dir_path(MANIFEST_DIR_NAME) || is_dir_path(LEGACY_SHARDS_DIR_NAME)
}

fn normalize_rel(rel: &str) -> String {
    rel.replace('\\', "/")
}

fn manifest_parent(manifest_rel_path: &str) -> String {
    match manifest_rel_path.rsplit_once('/') {
        Some((parent, _)) => parent.to_string(),
        None => String::new(),
    }
}

/// 项目/App group 的清单存储。
#[derive(Debug, Clone)]
pub(crate) struct ManifestStore {
    sync_root: PathBuf,
    manifest_rel_path: String,
    storage_dir_rel: String,
}

impl ManifestStore {
    pub(crate) fn new(sync_root: &Path, manifest_rel_path: &str) -> Self {
        let manifest_rel_path = normalize_rel(manifest_rel_path);
        let parent = manifest_parent(&manifest_rel_path);
        let storage_dir_rel = if parent.is_empty() {
            MANIFEST_DIR_NAME.to_string()
        } else {
            format!("{parent}/{MANIFEST_DIR_NAME}")
        };
        Self {
            sync_root: sync_root.to_path_buf(),
            manifest_rel_path,
            storage_dir_rel,
        }
    }

    // 以下访问器/发布入口留给 generation/transfer/sync_api 接线使用。
    #[allow(dead_code)]
    pub(crate) fn sync_root(&self) -> &Path {
        &self.sync_root
    }

    #[allow(dead_code)]
    pub(crate) fn manifest_rel_path(&self) -> &str {
        &self.manifest_rel_path
    }

    /// v2 存储目录（相对 sync_root），如 `app-meta/sync/manifest`。
    #[allow(dead_code)]
    pub(crate) fn storage_dir_rel(&self) -> &str {
        &self.storage_dir_rel
    }

    pub(crate) fn root_rel(&self) -> String {
        format!("{}/{}", self.storage_dir_rel, ROOT_FILE_NAME)
    }

    /// 内容寻址索引路径：`<storage>/index/<namespace>/<sha256>.json`。
    ///
    /// `sha` 必须等于索引 canonical bytes 的 sha256，也是 root 中记录的值。
    pub(crate) fn index_rel(&self, namespace: &str, sha: &str) -> String {
        format!(
            "{}/{INDEX_DIR_NAME}/{namespace}/{sha}{JSON_SUFFIX}",
            self.storage_dir_rel
        )
    }

    /// 87b15f2 旧布局的固定路径索引：`<storage>/index/<namespace>.json`。
    ///
    /// 只用于一次性迁移读取与根切换后的回收，不再写入。
    pub(crate) fn legacy_index_rel(&self, namespace: &str) -> String {
        format!(
            "{}/{INDEX_DIR_NAME}/{namespace}{JSON_SUFFIX}",
            self.storage_dir_rel
        )
    }

    pub(crate) fn object_rel(&self, sha: &str) -> String {
        format!(
            "{}/{OBJECTS_DIR_NAME}/{sha}{JSON_SUFFIX}",
            self.storage_dir_rel
        )
    }

    fn legacy_shards_rel(&self) -> String {
        let parent = manifest_parent(&self.manifest_rel_path);
        if parent.is_empty() {
            LEGACY_SHARDS_DIR_NAME.to_string()
        } else {
            format!("{parent}/{LEGACY_SHARDS_DIR_NAME}")
        }
    }

    /// 是否存在旧格式来源（旧单文件或旧分片目录）。
    ///
    /// App group 聚合清单的只读回退由调用方判断，不在这里。
    pub(crate) fn has_legacy_source(&self) -> bool {
        self.sync_root.join(&self.manifest_rel_path).is_file()
            || self.sync_root.join(self.legacy_shards_rel()).is_dir()
    }

    /// 读取权威根。
    ///
    /// root.json 不存在 → `Ok(None)`；存在但解析失败/版本不符 → `Err`
    /// （错误消息带路径）。
    pub(crate) fn load_root(&self) -> crate::Result<Option<ManifestRoot>> {
        let rel = self.root_rel();
        let Some(bytes) = self.read_optional_bytes(&rel)? else {
            return Ok(None);
        };
        let path = self.sync_root.join(&rel);
        let origin = path.display().to_string();
        let root: ManifestRoot = serde_json::from_slice(&bytes).map_err(|error| {
            crate::Error::Other(format!("manifest root {origin} is corrupt: {error}"))
        })?;
        root.validate(&origin)?;
        Ok(Some(root))
    }

    /// 严格读取完整清单。
    ///
    /// root 不存在 → `Ok(None)`；root 存在但没有 namespaces → `Ok(Some(空清单))`。
    /// 任一索引/对象缺失、sha256 不符、JSON 损坏、路径不属于该 object_key → `Err`，
    /// 绝不返回部分清单。
    pub(crate) fn load_all(&self) -> crate::Result<Option<SyncManifest>> {
        let Some(root) = self.load_root()? else {
            return Ok(None);
        };
        let mut by_path: BTreeMap<String, ManifestFileRecord> = BTreeMap::new();
        for (namespace, index_sha) in &root.namespaces {
            let index = self.read_verified_index(namespace, index_sha)?;
            for (key, object_sha) in &index.objects {
                let object = self.read_verified_object(key, object_sha)?;
                for record in object.files {
                    if by_path.contains_key(&record.path) {
                        return Err(crate::Error::Other(format!(
                            "manifest storage contains duplicate path {}",
                            record.path
                        )));
                    }
                    by_path.insert(record.path.clone(), record);
                }
            }
        }
        Ok(Some(SyncManifest {
            files: by_path.into_values().collect(),
        }))
    }

    /// 读取权威清单；root 不存在时迁移旧格式。
    ///
    /// 迁移来源优先级：旧 `manifest-shards/` 目录（上一代权威）→ 旧单文件
    /// （更早一代，可能过期）→ 空清单。
    /// 有旧数据时经 [`Self::plan_write_all`] 生成写入计划，再用
    /// `SaveTransaction` 一次提交（对象→索引→根，根最后）；成功后才把根
    /// 当权威。旧文件/旧目录保留不删，由后续发布端回收。
    pub(crate) fn load_or_legacy(&self) -> crate::Result<SyncManifest> {
        if self.load_root()?.is_some() {
            return self.load_all()?.ok_or_else(|| {
                crate::Error::Other(format!(
                    "manifest root {} disappeared while loading",
                    self.sync_root.join(self.root_rel()).display()
                ))
            });
        }
        let Some(manifest) = self.read_legacy_manifest()? else {
            // 没有任何清单来源：这是合法空清单，不写根（写根是业务提交的职责）。
            return Ok(SyncManifest::default());
        };
        // 首次迁移建立权威根：generation 由最终 namespace 映射内容派生，
        // 不再用随机 UUID 充当版本身份。
        let plan = self.plan_write_all(&manifest, "")?;
        self.commit_plan(&plan)?;
        Ok(manifest)
    }

    /// 应用对象级增量，只为受影响的 object/index/root 生成写入。
    ///
    /// 未涉及的 namespace/index/object 不重写；删除后为空的 objects 文件
    /// 不写入并在 `deletes` 里回收。
    ///
    /// `generation` 仅为兼容旧调用方保留，**不参与根身份**：需要写根时
    /// generation 由最终 namespace 映射 + 本地前一个持久根内容派生。
    #[allow(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        clippy::excessive_nesting,
        clippy::type_complexity
    )]
    pub(crate) fn apply_delta(
        &self,
        delta: &ManifestDelta,
        _generation: &str,
    ) -> crate::Result<ManifestWritePlan> {
        let current_root = self.load_root()?;
        let mut namespaces: BTreeMap<String, String> = match &current_root {
            Some(root) => root.namespaces.clone(),
            None => BTreeMap::new(),
        };

        let mut upserts: BTreeMap<String, BTreeMap<String, Vec<ManifestFileRecord>>> =
            BTreeMap::new();
        for record in delta.upserts.values() {
            let namespace = namespace_for_path(&record.path);
            let key = object_key(&record.path);
            upserts
                .entry(namespace)
                .or_default()
                .entry(key)
                .or_default()
                .push(record.clone());
        }
        let mut removed: BTreeMap<String, BTreeMap<String, Vec<String>>> = BTreeMap::new();
        for path in &delta.deletes {
            let namespace = namespace_for_path(path);
            let key = object_key(path);
            removed
                .entry(namespace)
                .or_default()
                .entry(key)
                .or_default()
                .push(path.clone());
        }

        let affected: BTreeSet<String> = upserts.keys().chain(removed.keys()).cloned().collect();

        let mut writes: Vec<(String, Vec<u8>)> = Vec::new();
        let mut final_indexes: BTreeMap<String, ManifestIndex> = BTreeMap::new();

        for namespace in &affected {
            let old_index_sha = namespaces.get(namespace).cloned();
            let mut index = match &old_index_sha {
                Some(index_sha) => self.read_verified_index(namespace, index_sha)?,
                None => ManifestIndex::new(namespace),
            };

            let mut keys: BTreeSet<String> = BTreeSet::new();
            if let Some(map) = upserts.get(namespace) {
                keys.extend(map.keys().cloned());
            }
            if let Some(map) = removed.get(namespace) {
                keys.extend(map.keys().cloned());
            }

            for key in keys {
                let current_object_sha = index.objects.get(&key).cloned();
                let mut records: BTreeMap<String, ManifestFileRecord> = BTreeMap::new();
                if let Some(object_sha) = &current_object_sha {
                    let object = self.read_verified_object(&key, object_sha)?;
                    for record in object.files {
                        records.insert(record.path.clone(), record);
                    }
                }
                if let Some(list) = upserts.get(namespace).and_then(|map| map.get(&key)) {
                    for record in list {
                        let actual = object_key(&record.path);
                        if actual != key {
                            return Err(crate::Error::Other(format!(
                                "manifest delta upsert path {} belongs to object key {} not {}",
                                record.path, actual, key
                            )));
                        }
                        records.insert(record.path.clone(), record.clone());
                    }
                }
                if let Some(paths) = removed.get(namespace).and_then(|map| map.get(&key)) {
                    for path in paths {
                        records.remove(path);
                    }
                }

                if records.is_empty() {
                    index.objects.remove(&key);
                    continue;
                }
                let object = ManifestObject::new(&key, records.into_values().collect());
                let bytes = object.canonical_bytes()?;
                let object_sha = sha256_hex(&bytes);
                if current_object_sha.as_deref() != Some(object_sha.as_str()) {
                    let rel = self.object_rel(&object_sha);
                    if !self.object_file_identical(&rel, &bytes)? {
                        writes.push((rel, bytes));
                    }
                    index.objects.insert(key, object_sha);
                }
            }

            if index.objects.is_empty() {
                namespaces.remove(namespace);
                continue;
            }
            let bytes = index.canonical_bytes()?;
            let index_sha = sha256_hex(&bytes);
            // 内容寻址：同 sha 已存在（含从旧固定路径迁移过来的情况）则复用，
            // 只新建/补齐 `<ns>/<sha>.json`，绝不覆盖同名文件。
            let rel = self.index_rel(namespace, &index_sha);
            if !self.content_addressed_file_identical(&rel, &bytes, "index")? {
                writes.push((rel, bytes));
            }
            namespaces.insert(namespace.clone(), index_sha);
            final_indexes.insert(namespace.clone(), index);
        }

        // 旧固定路径索引迁移：未受本计划影响的 namespace 若内容寻址文件尚不存在
        // （87b15f2 布局），必须补写同一份字节到 `<ns>/<sha>.json`，根中 sha 不变。
        // 否则本计划的 GC 会删掉旧固定路径这份唯一副本。
        let planned: BTreeSet<String> = writes.iter().map(|(rel, _)| rel.clone()).collect();
        let mut migration_writes: Vec<(String, Vec<u8>)> = Vec::new();
        for (namespace, index_sha) in &namespaces {
            let rel = self.index_rel(namespace, index_sha);
            if planned.contains(&rel) || self.sync_root.join(&rel).is_file() {
                continue;
            }
            let (_, bytes, _) = self.read_verified_index_with_bytes(namespace, index_sha)?;
            migration_writes.push((rel, bytes));
        }
        writes.extend(migration_writes);

        let root_rel = self.root_rel();
        // 根版本只在清单语义真实变化或首次建立权威时推进。最终 namespace 映射
        // 与当前根一致（空 delta / 相同记录重放 / 旧固定路径迁移）时：不写根、
        // 不改 generation、不回收任何文件；writes 只保留上面确实缺失的
        // 内容寻址索引/对象补写。
        let root_changes = Self::root_version_changes(current_root.as_ref(), &namespaces);
        if root_changes {
            writes.push((
                root_rel.clone(),
                Self::local_root_bytes(current_root.as_ref(), &namespaces)?,
            ));
        }

        let pending_writes: BTreeSet<String> = writes.iter().map(|(rel, _)| rel.clone()).collect();
        let new_reachable =
            self.reachable_with_overrides(&namespaces, &final_indexes, &pending_writes, false)?;
        // GC 保留一代：上一代本地根（本次切换读到的旧根）仍可达的文件不能删。
        // 语义未变化时没有代次推进，也就没有可结算的回收边界。
        let deletes = if root_changes {
            self.plan_deletes(&new_reachable, current_root.as_ref())?
        } else {
            Vec::new()
        };

        Ok(ManifestWritePlan {
            writes,
            deletes,
            root_rel,
            reachable: new_reachable,
        })
    }

    /// 完整重建：首次写入 / 旧格式迁移用。
    ///
    /// `generation` 仅为兼容旧调用方保留，不参与根身份（见 [`Self::apply_delta`]）。
    #[allow(clippy::type_complexity)]
    pub(crate) fn plan_write_all(
        &self,
        manifest: &SyncManifest,
        _generation: &str,
    ) -> crate::Result<ManifestWritePlan> {
        let mut grouped: BTreeMap<String, BTreeMap<String, BTreeMap<String, ManifestFileRecord>>> =
            BTreeMap::new();
        for record in &manifest.files {
            let namespace = namespace_for_path(&record.path);
            let key = object_key(&record.path);
            grouped
                .entry(namespace)
                .or_default()
                .entry(key)
                .or_default()
                .insert(record.path.clone(), record.clone());
        }

        let mut writes: Vec<(String, Vec<u8>)> = Vec::new();
        let mut namespaces: BTreeMap<String, String> = BTreeMap::new();
        let mut indexes: BTreeMap<String, ManifestIndex> = BTreeMap::new();

        for (namespace, keys) in grouped {
            let mut index = ManifestIndex::new(&namespace);
            for (key, records) in keys {
                let object = ManifestObject::new(&key, records.into_values().collect());
                let bytes = object.canonical_bytes()?;
                let object_sha = sha256_hex(&bytes);
                let rel = self.object_rel(&object_sha);
                if !self.object_file_identical(&rel, &bytes)? {
                    writes.push((rel, bytes));
                }
                index.objects.insert(key, object_sha);
            }
            if index.objects.is_empty() {
                continue;
            }
            let bytes = index.canonical_bytes()?;
            let index_sha = sha256_hex(&bytes);
            let rel = self.index_rel(&namespace, &index_sha);
            if !self.content_addressed_file_identical(&rel, &bytes, "index")? {
                writes.push((rel, bytes));
            }
            namespaces.insert(namespace.clone(), index_sha);
            indexes.insert(namespace, index);
        }

        let root_rel = self.root_rel();
        let current_root = self.load_root()?;
        // 与 apply_delta 同一判定：映射相同（含空清单首次/重复重建）时只有
        // 缺失的内容寻址文件需要补写，根与代次不变。
        let root_changes = Self::root_version_changes(current_root.as_ref(), &namespaces);
        if root_changes {
            writes.push((
                root_rel.clone(),
                Self::local_root_bytes(current_root.as_ref(), &namespaces)?,
            ));
        }

        let pending_writes: BTreeSet<String> = writes.iter().map(|(rel, _)| rel.clone()).collect();
        let new_reachable =
            self.reachable_with_overrides(&namespaces, &indexes, &pending_writes, false)?;
        // GC 保留一代：上一代本地根（本次切换读到的旧根）仍可达的文件不能删。
        let deletes = if root_changes {
            self.plan_deletes(&new_reachable, current_root.as_ref())?
        } else {
            Vec::new()
        };

        Ok(ManifestWritePlan {
            writes,
            deletes,
            root_rel,
            reachable: new_reachable,
        })
    }

    /// 以传入的完整清单为目标求"存储差"，只为变化的 object/index/root 生成计划。
    ///
    /// 用于 LWW merge 后的持久化：不能把 known_files 基线当存储增量，也不能
    /// 退化成整份单文件重写。清单语义未变化时返回空 writes/deletes，调用方仍
    /// 可提交真正变化的 state/conflicts。
    ///
    /// `generation` 仅为兼容旧调用方保留，不参与根身份。
    pub(crate) fn plan_sync_manifest(
        &self,
        manifest: &SyncManifest,
        generation: &str,
    ) -> crate::Result<ManifestWritePlan> {
        let Some(current) = self.load_all()? else {
            return self.plan_write_all(manifest, generation);
        };
        let current_records: BTreeMap<String, ManifestFileRecord> = current
            .files
            .into_iter()
            .map(|record| (record.path.clone(), record))
            .collect();
        let mut target: BTreeMap<String, &ManifestFileRecord> = BTreeMap::new();
        for record in &manifest.files {
            target.insert(record.path.clone(), record);
        }

        let mut delta = ManifestDelta::default();
        for (path, record) in &target {
            if current_records.get(path) == Some(*record) {
                continue;
            }
            delta.upserts.insert(path.clone(), (*record).clone());
        }
        for path in current_records.keys() {
            if !target.contains_key(path) {
                delta.deletes.insert(path.clone());
            }
        }
        self.apply_delta(&delta, generation)
    }

    /// 本次计划是否推进根版本：首次建立权威根，或 namespace → index sha256
    /// 映射真实变化（增/删 namespace、索引换 sha）才推进。
    fn root_version_changes(
        current_root: Option<&ManifestRoot>,
        namespaces: &BTreeMap<String, String>,
    ) -> bool {
        match current_root {
            Some(root) => root.namespaces != *namespaces,
            None => true,
        }
    }

    /// 本地存储根的 canonical 字节。
    ///
    /// - generation 由 `(schemaVersion, namespaces, localPreviousRoot)` 派生，
    ///   同一内容稳定，不使用调用方传入的随机串；
    /// - `previousRoot` 恒为 `None`：本地 staging/merge 不是已发布代；
    /// - `localPreviousRoot` = 写盘前当前本地根 canonical sha256。
    fn local_root_bytes(
        current_root: Option<&ManifestRoot>,
        namespaces: &BTreeMap<String, String>,
    ) -> crate::Result<Vec<u8>> {
        let local_previous_root = current_root
            .map(ManifestRoot::canonical_sha256)
            .transpose()?;
        ManifestRoot::local(namespaces.clone(), local_previous_root).canonical_bytes()
    }

    /// 读取当前根与其全部可达文件。
    ///
    /// root 不存在 → `Ok(None)`；根/索引/对象缺失或损坏 → `Err`。
    /// 返回本地根原始字节（本地视图，含 `localPreviousRoot`）与可达路径集合
    /// （相对 sync_root）。发布字节由 [`Self::load_publish_files`] 按发布视图
    /// 重新构造。
    // 发布/GC 接线的只读入口；当前主链路尚未消费。
    #[allow(dead_code)]
    pub(crate) fn load_reachable(&self) -> crate::Result<Option<(Vec<u8>, BTreeSet<String>)>> {
        let Some(root) = self.load_root()? else {
            return Ok(None);
        };
        let root_rel = self.root_rel();
        let root_bytes = self.read_required_bytes(&root_rel, "root")?;
        let reachable = self.reachable_with_overrides(
            &root.namespaces,
            &BTreeMap::new(),
            &BTreeSet::new(),
            true,
        )?;
        Ok(Some((root_bytes, reachable)))
    }

    /// 读取一个 namespace 的索引与发布字节（旧固定路径布局时带回迁移字节）。
    ///
    /// 稀疏发布视图只读 delta 涉及的 namespace，用它取 object_key → object
    /// sha256，不展开完整可达集合。
    pub(crate) fn load_publish_index(
        &self,
        namespace: &str,
        index_sha: &str,
    ) -> crate::Result<(ManifestIndex, Vec<u8>, bool)> {
        self.read_verified_index_with_bytes(namespace, index_sha)
    }

    /// 当前根的发布视图：canonical 相对路径 + 可选内联字节。
    ///
    /// - 根按**发布视图**重新构造：`previousRoot` = 调用方传入的本轮固定远端
    ///   base 根 canonical sha256，`localPreviousRoot` 恒为 `None`（发布视图不
    ///   携带本地血缘），generation 由 namespace 映射内容派生；
    /// - 索引总是以内容寻址路径 `<ns>/<sha256>.json` 输出；若该文件尚未落盘而
    ///   旧固定路径索引可用（87b15f2 布局），则带回字节，让发布端把它写到新
    ///   路径（迁移），根中的 sha 保持不变；
    /// - 对象只给出路径（`None`），要求文件存在，否则 `Err`。
    ///
    /// root 不存在 → `Ok(None)`。
    pub(crate) fn load_publish_files(
        &self,
        previous_published_root: Option<&str>,
    ) -> crate::Result<Option<Vec<(String, Option<Vec<u8>>)>>> {
        let Some(root) = self.load_root()? else {
            return Ok(None);
        };
        let root_rel = self.root_rel();
        let publish_root =
            ManifestRoot::published(root.namespaces.clone(), previous_published_root);
        let root_bytes = publish_root.canonical_bytes()?;
        let mut files: Vec<(String, Option<Vec<u8>>)> = vec![(root_rel, Some(root_bytes))];
        let mut seen_objects: BTreeSet<String> = BTreeSet::new();
        for (namespace, index_sha) in &root.namespaces {
            let (index, index_bytes, used_legacy) =
                self.read_verified_index_with_bytes(namespace, index_sha)?;
            files.push((
                self.index_rel(namespace, index_sha),
                if used_legacy { Some(index_bytes) } else { None },
            ));
            for object_sha in index.objects.values() {
                let rel = self.object_rel(object_sha);
                if !seen_objects.insert(rel.clone()) {
                    continue;
                }
                if !self.sync_root.join(&rel).is_file() {
                    return Err(crate::Error::Other(format!(
                        "manifest object {} referenced by namespace {} is missing",
                        self.sync_root.join(&rel).display(),
                        namespace
                    )));
                }
                files.push((rel, None));
            }
        }
        Ok(Some(files))
    }

    fn read_optional_bytes(&self, rel: &str) -> crate::Result<Option<Vec<u8>>> {
        let path = self.sync_root.join(rel);
        match std::fs::read(&path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(crate::Error::Io(std::io::Error::new(
                error.kind(),
                format!("read manifest storage file {}: {error}", path.display()),
            ))),
        }
    }

    fn read_required_bytes(&self, rel: &str, kind: &str) -> crate::Result<Vec<u8>> {
        match self.read_optional_bytes(rel)? {
            Some(bytes) => Ok(bytes),
            None => Err(crate::Error::Other(format!(
                "manifest {kind} {} is missing",
                self.sync_root.join(rel).display()
            ))),
        }
    }

    fn read_verified_index(
        &self,
        namespace: &str,
        expected_sha: &str,
    ) -> crate::Result<ManifestIndex> {
        self.read_verified_index_with_bytes(namespace, expected_sha)
            .map(|(index, _, _)| index)
    }

    /// 读取索引并校验 sha256 / 结构；同时返回原始字节与是否来自旧固定路径。
    fn read_verified_index_with_bytes(
        &self,
        namespace: &str,
        expected_sha: &str,
    ) -> crate::Result<(ManifestIndex, Vec<u8>, bool)> {
        let (bytes, path, used_legacy) = self.read_index_bytes(namespace, expected_sha)?;
        let origin = path.display().to_string();
        let index: ManifestIndex = serde_json::from_slice(&bytes).map_err(|error| {
            crate::Error::Other(format!(
                "manifest index {origin} (namespace {namespace}, sha256 {expected_sha}) is corrupt: {error}"
            ))
        })?;
        index.validate(&origin, namespace)?;
        Ok((index, bytes, used_legacy))
    }

    /// 内容寻址路径优先；缺失时回退 87b15f2 的固定路径（一次性迁移读取）。
    ///
    /// 两种布局都必须让内容 sha256 等于 root 记录的 sha，否则 `Err`；都找不到
    /// 同样 `Err`（消息带 namespace、sha、两条路径），绝不静默当成空索引。
    fn read_index_bytes(
        &self,
        namespace: &str,
        expected_sha: &str,
    ) -> crate::Result<(Vec<u8>, PathBuf, bool)> {
        if !is_sha256_hex(expected_sha) {
            return Err(crate::Error::Other(format!(
                "manifest root references invalid index sha256 for namespace {namespace}: {expected_sha}"
            )));
        }
        let hashed_path = self.sync_root.join(self.index_rel(namespace, expected_sha));
        match std::fs::read(&hashed_path) {
            Ok(bytes) => {
                self.verify_index_sha(namespace, expected_sha, &bytes, &hashed_path)?;
                Ok((bytes, hashed_path, false))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let legacy_path = self.sync_root.join(self.legacy_index_rel(namespace));
                match std::fs::read(&legacy_path) {
                    Ok(bytes) => {
                        self.verify_index_sha(namespace, expected_sha, &bytes, &legacy_path)?;
                        Ok((bytes, legacy_path, true))
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        Err(crate::Error::Other(format!(
                            "manifest index for namespace {namespace} with sha256 {expected_sha} is \
                             missing: {} and {}",
                            hashed_path.display(),
                            legacy_path.display()
                        )))
                    }
                    Err(error) => Err(crate::Error::Io(std::io::Error::new(
                        error.kind(),
                        format!("read manifest index {}: {error}", legacy_path.display()),
                    ))),
                }
            }
            Err(error) => Err(crate::Error::Io(std::io::Error::new(
                error.kind(),
                format!("read manifest index {}: {error}", hashed_path.display()),
            ))),
        }
    }

    fn verify_index_sha(
        &self,
        namespace: &str,
        expected_sha: &str,
        bytes: &[u8],
        path: &Path,
    ) -> crate::Result<()> {
        let actual_sha = sha256_hex(bytes);
        if actual_sha != expected_sha {
            return Err(crate::Error::Other(format!(
                "manifest index {} sha256 mismatch for namespace {namespace}: root expects \
                 {expected_sha}, actual {actual_sha}",
                path.display()
            )));
        }
        Ok(())
    }

    fn read_verified_object(&self, key: &str, expected_sha: &str) -> crate::Result<ManifestObject> {
        if !is_sha256_hex(expected_sha) {
            return Err(crate::Error::Other(format!(
                "manifest object key {key} references invalid sha256 {expected_sha}"
            )));
        }
        let rel = self.object_rel(expected_sha);
        let path = self.sync_root.join(&rel);
        let bytes = self.read_required_bytes(&rel, "object")?;
        let actual_sha = sha256_hex(&bytes);
        if actual_sha != expected_sha {
            return Err(crate::Error::Other(format!(
                "manifest object {} sha256 mismatch: index expects {expected_sha}, actual {actual_sha}",
                path.display()
            )));
        }
        let origin = path.display().to_string();
        let object: ManifestObject = serde_json::from_slice(&bytes).map_err(|error| {
            crate::Error::Other(format!("manifest object {origin} is corrupt: {error}"))
        })?;
        object.validate(&origin, key)?;
        Ok(object)
    }

    /// 内容寻址文件已经存在且字节相同 → 复用；存在但字节不同 → 存储损坏。
    fn object_file_identical(&self, rel: &str, bytes: &[u8]) -> crate::Result<bool> {
        self.content_addressed_file_identical(rel, bytes, "object")
    }

    /// 内容寻址文件（对象/索引）已存在且字节相同 → 无需重写；存在但字节不同
    /// → 存储损坏（文件名就是内容 sha256，同名文件必须是同一内容）。
    fn content_addressed_file_identical(
        &self,
        rel: &str,
        bytes: &[u8],
        kind: &str,
    ) -> crate::Result<bool> {
        let path = self.sync_root.join(rel);
        match std::fs::read(&path) {
            Ok(existing) if existing == bytes => Ok(true),
            Ok(_) => Err(crate::Error::Other(format!(
                "manifest {kind} {} exists with content that does not match its sha256 name",
                path.display()
            ))),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(crate::Error::Io(std::io::Error::new(
                error.kind(),
                format!("read manifest {kind} {}: {error}", path.display()),
            ))),
        }
    }

    /// 本计划的 GC 回收集合。
    ///
    /// 保留边界是「新根可达 ∪ 上一代根可达」；旧固定路径索引另按迁移状态判断
    /// （见 [`Self::stale_storage_files`]）。
    fn plan_deletes(
        &self,
        new_reachable: &BTreeSet<String>,
        current_root: Option<&ManifestRoot>,
    ) -> crate::Result<Vec<String>> {
        let previous_reachable = match current_root {
            Some(root) => self.reachable_with_overrides(
                &root.namespaces,
                &BTreeMap::new(),
                &BTreeSet::new(),
                false,
            )?,
            None => BTreeSet::new(),
        };
        self.stale_storage_files(
            new_reachable,
            &previous_reachable,
            current_root.map(|root| &root.namespaces),
        )
    }

    /// 可安全回收的本地清单文件：storage 下实际存在的对象/索引，且既不在新根
    /// 可达集合、也不在上一代已发布根可达集合。
    ///
    /// 旧固定路径索引（`index/<ns>.json`）是 87b15f2 布局的迁移来源，它只有在
    /// 新根已经用同一 sha 在 `index/<ns>/<sha>.json` 提供同一份内容时才可回收；
    /// 否则（索引换 sha 或 namespace 被移除）它是上一代根读者的唯一副本，保留
    /// 一代，等更下一代发布时再回收。
    fn stale_storage_files(
        &self,
        new_reachable: &BTreeSet<String>,
        previous_reachable: &BTreeSet<String>,
        previous_namespaces: Option<&BTreeMap<String, String>>,
    ) -> crate::Result<Vec<String>> {
        let mut existing: BTreeSet<String> = BTreeSet::new();
        self.collect_storage_files(
            &format!("{}/{OBJECTS_DIR_NAME}", self.storage_dir_rel),
            &mut existing,
        )?;
        self.collect_storage_files(
            &format!("{}/{INDEX_DIR_NAME}", self.storage_dir_rel),
            &mut existing,
        )?;
        let mut deletes = Vec::new();
        for rel in existing {
            if let Some(namespace) = self.legacy_index_namespace(&rel) {
                // 旧固定路径：只有上一代根仍按该 sha 引用、且新根没有提供同一
                // sha 的内容寻址副本时才保留。
                let Some(bytes) = self.read_optional_bytes(&rel)? else {
                    continue;
                };
                let sha = sha256_hex(&bytes);
                let retained = previous_namespaces
                    .and_then(|namespaces| namespaces.get(namespace))
                    .is_some_and(|previous_sha| {
                        *previous_sha == sha
                            && !new_reachable.contains(&self.index_rel(namespace, &sha))
                    });
                if !retained {
                    deletes.push(rel);
                }
                continue;
            }
            if !new_reachable.contains(&rel) && !previous_reachable.contains(&rel) {
                deletes.push(rel);
            }
        }
        Ok(deletes)
    }

    /// 旧固定路径索引的相对路径 → namespace；内容寻址路径返回 `None`。
    fn legacy_index_namespace<'a>(&self, rel: &'a str) -> Option<&'a str> {
        let prefix = format!("{}/{}/", self.storage_dir_rel, INDEX_DIR_NAME);
        let rest = rel.strip_prefix(&prefix)?;
        if rest.contains('/') {
            return None;
        }
        rest.strip_suffix(JSON_SUFFIX)
    }

    /// 递归收集 `rel_dir` 下的 `.json` 文件（含 87b15f2 的旧固定路径索引）。
    fn collect_storage_files(
        &self,
        rel_dir: &str,
        out: &mut BTreeSet<String>,
    ) -> crate::Result<()> {
        let dir = self.sync_root.join(rel_dir);
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => {
                return Err(crate::Error::Io(std::io::Error::new(
                    error.kind(),
                    format!("read manifest storage dir {}: {error}", dir.display()),
                )))
            }
        };
        for entry in entries {
            let entry = entry.map_err(|error| {
                crate::Error::Io(std::io::Error::new(
                    error.kind(),
                    format!("read manifest storage dir {}: {error}", dir.display()),
                ))
            })?;
            let Some(name) = entry.file_name().to_str().map(str::to_string) else {
                continue;
            };
            let rel = format!("{rel_dir}/{name}");
            let file_type = entry.file_type().map_err(|error| {
                crate::Error::Io(std::io::Error::new(
                    error.kind(),
                    format!("read manifest storage entry {}: {error}", rel),
                ))
            })?;
            if file_type.is_dir() {
                self.collect_storage_files(&rel, out)?;
            } else if file_type.is_file() && name.ends_with(JSON_SUFFIX) {
                out.insert(rel);
            }
        }
        Ok(())
    }

    /// 计算根 + 全部索引 + 全部对象的可达集合。
    ///
    /// `overrides` 是本计划刚生成、尚未落盘的索引内容；其余 namespace 严格
    /// 从磁盘读取并校验 sha256。索引引用的对象文件必须存在（`pending_writes`
    /// 中的对象是本计划即将写入的，允许尚未落盘），否则视为损坏。
    /// `verify_object_bytes` 为真时还会重新读取对象内容校验 sha256。
    fn reachable_with_overrides(
        &self,
        namespaces: &BTreeMap<String, String>,
        overrides: &BTreeMap<String, ManifestIndex>,
        pending_writes: &BTreeSet<String>,
        verify_object_bytes: bool,
    ) -> crate::Result<BTreeSet<String>> {
        let mut reachable: BTreeSet<String> = BTreeSet::new();
        reachable.insert(self.root_rel());
        for (namespace, index_sha) in namespaces {
            let index = match overrides.get(namespace) {
                Some(index) => index.clone(),
                None => self.read_verified_index(namespace, index_sha)?,
            };
            // 可达集合用内容寻址路径表示：旧固定路径只是迁移来源，同一 sha 的
            // 身份是 `<ns>/<sha>.json`，发布端据此判断迁移是否完成。
            reachable.insert(self.index_rel(namespace, index_sha));
            for (key, object_sha) in &index.objects {
                let rel = self.object_rel(object_sha);
                if !pending_writes.contains(&rel) {
                    if verify_object_bytes {
                        self.read_verified_object(key, object_sha)?;
                    } else if !self.sync_root.join(&rel).is_file() {
                        return Err(crate::Error::Other(format!(
                            "manifest object {} referenced by namespace {} is missing",
                            self.sync_root.join(&rel).display(),
                            namespace
                        )));
                    }
                }
                reachable.insert(rel);
            }
        }
        Ok(reachable)
    }

    fn commit_plan(&self, plan: &ManifestWritePlan) -> crate::Result<()> {
        if plan.writes.is_empty() && plan.deletes.is_empty() {
            return Ok(());
        }
        let mut tx = crate::storage::transaction::SaveTransaction::new(&self.sync_root);
        // 顺序固定：对象 → 索引 → 根（writes 最后一项）。根写失败时旧根仍
        // 指向旧对象；deletes 必须排在根之后，只回收已不被新根引用的文件。
        for (rel, bytes) in &plan.writes {
            tx.add_bytes(rel, bytes)?;
        }
        for rel in &plan.deletes {
            tx.add_delete(rel)?;
        }
        tx.commit()
    }

    fn read_legacy_manifest(&self) -> crate::Result<Option<SyncManifest>> {
        // 迁移优先级：旧分片目录（v1.5 权威，提交时已删除旧单文件）优先于
        // 旧单文件（v1，可能停留在更早的一代）。分片目录存在且实际含分片
        // 文件时，它就是上一代的完整权威；空目录不算。
        if self.legacy_shards_have_files()? {
            if let Some(manifest) = self.read_legacy_shards()? {
                return Ok(Some(manifest));
            }
        }
        let single_rel = self.manifest_rel_path.clone();
        if let Some(bytes) = self.read_optional_bytes(&single_rel)? {
            let path = self.sync_root.join(&single_rel);
            let manifest: SyncManifest = serde_json::from_slice(&bytes).map_err(|error| {
                crate::Error::Other(format!(
                    "legacy manifest {} is corrupt: {error}",
                    path.display()
                ))
            })?;
            return Ok(Some(manifest));
        }
        self.read_legacy_shards()
    }

    /// 旧分片目录是否存在且含至少一个 `.json` 文件。
    fn legacy_shards_have_files(&self) -> crate::Result<bool> {
        let dir = self.sync_root.join(self.legacy_shards_rel());
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(crate::Error::Io(error)),
        };
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("json") {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// 合并旧 `manifest-shards/*.json`（每个是 `{"files":[...]}`）。
    ///
    /// 任一 shard 损坏 → `Err`；目录不存在 → `Ok(None)`；目录为空 → 空清单。
    fn read_legacy_shards(&self) -> crate::Result<Option<SyncManifest>> {
        let rel = self.legacy_shards_rel();
        let dir = self.sync_root.join(&rel);
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(crate::Error::Io(std::io::Error::new(
                    error.kind(),
                    format!("read legacy manifest shard dir {}: {error}", dir.display()),
                )))
            }
        };
        let mut files: Vec<PathBuf> = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|error| {
                crate::Error::Io(std::io::Error::new(
                    error.kind(),
                    format!("read legacy manifest shard dir {}: {error}", dir.display()),
                ))
            })?;
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if name.ends_with(JSON_SUFFIX) {
                files.push(path);
            }
        }
        files.sort();
        let mut by_path: BTreeMap<String, ManifestFileRecord> = BTreeMap::new();
        for path in files {
            let bytes = std::fs::read(&path).map_err(|error| {
                crate::Error::Io(std::io::Error::new(
                    error.kind(),
                    format!("read legacy manifest shard {}: {error}", path.display()),
                ))
            })?;
            let shard: SyncManifest = serde_json::from_slice(&bytes).map_err(|error| {
                crate::Error::Other(format!(
                    "legacy manifest shard {} is corrupt: {error}",
                    path.display()
                ))
            })?;
            for record in shard.files {
                if ValidatedSyncPath::new(&record.path).is_err() {
                    return Err(crate::Error::Other(format!(
                        "legacy manifest shard {} contains invalid path {}",
                        path.display(),
                        record.path
                    )));
                }
                by_path.insert(record.path.clone(), record);
            }
        }
        Ok(Some(SyncManifest {
            files: by_path.into_values().collect(),
        }))
    }
}
