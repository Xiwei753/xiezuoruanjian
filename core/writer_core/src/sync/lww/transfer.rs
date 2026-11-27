//! LWW transfer — remote tree、download、upload、delete 辅助。
//!
//! 从 mod.rs 抽出的传输相关函数：远端 tree/manifest 拉取、并行下载、串行上传/删除、
//! trash 移动、冲突副本保存。
//!
//! 所有远端操作通过 [`crate::sync::provider::SyncProvider`] trait 执行，
//! 不直接依赖 GitHub API 或 `SyncTransport`。ProviderError 通过 `From` 自动转为 `crate::Error`。

use std::collections::HashMap;
use std::path::Path;

use rayon::prelude::*;

use crate::sync::path::ValidatedSyncPath;
use crate::sync::provider::model::{BatchMutation, RemoteVersion};
use crate::sync::provider::SyncProvider;
use crate::sync::types::SyncManifest;

use super::merge::LwwMergeOutcome;

/// 并行下载最大线程数上限。实际线程数由 `SyncCapabilities::max_parallel_downloads`
/// 和 `MAX_PARALLEL_DOWNLOADS` 的较小值决定。
pub(super) const MAX_PARALLEL_DOWNLOADS: usize = 4;

/// 创建并行下载线程池。线程数取 `task_count`、`max_threads` 和
/// `MAX_PARALLEL_DOWNLOADS` 的较小值，至少 1 线程。
/// 使用 rayon 的 work-stealing 调度器。
pub(super) fn sync_download_pool(
    task_count: usize,
    max_threads: usize,
) -> crate::Result<rayon::ThreadPool> {
    let effective = task_count.clamp(1, max_threads.min(MAX_PARALLEL_DOWNLOADS));
    rayon::ThreadPoolBuilder::new()
        .num_threads(effective)
        .build()
        .map_err(|e| {
            crate::Error::Io(std::io::Error::other(format!(
                "sync_parallel_pool_error: {}",
                e
            )))
        })
}

/// 将远端冲突内容保存为本地备份文件。
///
/// 文件名格式：`{原文件名}.remote-conflict-{时间戳}`，保存在原文件同目录下。
/// 此备份供用户手动对比本地与远端内容，不参与自动合并逻辑。
///
/// `path` 必须通过 [`ValidatedSyncPath`] 验证（调用方已在上层校验）。
///
/// 返回相对 `sync_root` 的完整 snapshot 路径（如
/// `volumes/v1/chapters/c1/chapter.md.remote-conflict-20260925-012345`），
/// 供 [`SyncConflict::remote_snapshot_path`] 记录，预览 API 据此读取远端内容。
pub(crate) fn save_conflict_copy(
    sync_root: &Path,
    path: &str,
    remote_content: &[u8],
) -> crate::Result<String> {
    let validated = ValidatedSyncPath::new(path)?;
    let full_path = validated.join_under(sync_root);
    let filename = full_path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    let timestamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    let conflict_filename = format!("{}.remote-conflict-{}", filename, timestamp);

    let conflict_path = full_path
        .parent()
        .unwrap_or(&full_path)
        .join(&conflict_filename);

    crate::storage::transaction::atomic_write_bytes(&conflict_path, remote_content).map_err(
        |e| {
            crate::Error::Io(std::io::Error::other(format!(
                "write conflict copy {}: {}",
                path, e
            )))
        },
    )?;

    // 返回相对 sync_root 的路径，供 remote_snapshot_path 记录。
    let relative = conflict_path
        .strip_prefix(sync_root)
        .map_err(|e| {
            crate::Error::Io(std::io::Error::other(format!(
                "save_conflict_copy: conflict_path not under sync_root: {}",
                e
            )))
        })?
        .to_string_lossy()
        .to_string();
    Ok(relative)
}

/// 通过 Provider 拉取远端 tree，返回以本地相对路径为 key、远端版本为 value 的 map。
///
/// `provider.list(remote_prefix)` 返回已剥前缀的 `RemoteEntry` 列表，
/// 此函数转为 `HashMap<path, version_string>` 供 LWW 比较使用。
pub(super) fn fetch_remote_tree(
    provider: &dyn SyncProvider,
    remote_prefix: &str,
) -> crate::Result<HashMap<String, String>> {
    let entries = provider.list(remote_prefix)?;
    let mut map = HashMap::with_capacity(entries.len());
    for entry in entries {
        map.insert(entry.path, entry.version.0);
    }
    Ok(map)
}

/// 通过 Provider 拉取远端 manifest。
///
/// 权威来源是远端 v2 清单存储（`app-meta/sync/manifest/root.json` + 两级索引 +
/// 内容寻址对象）。根存在时只读根列出的索引/对象，缺文件或校验失败直接报错；
/// 根不存在才回退旧单文件，再回退旧分片目录（一次性迁移读取）。
/// 三种来源都没有时返回空清单。
pub(super) fn fetch_remote_manifest(
    provider: &dyn SyncProvider,
    remote_prefix: &str,
    remote_tree_files: &HashMap<String, String>,
) -> crate::Result<SyncManifest> {
    super::manifest::fetch_remote_manifest_v2(provider, remote_prefix, remote_tree_files)
}

/// 并行下载 pending_take_remote 列表中的文件。
///
/// 返回 `Vec<(path, Option<content>)>`，`None` 表示远端文件缺失。
///
/// 所有远端路径通过 [`ValidatedSyncPath`] 验证后才 join 到本地。
#[allow(clippy::excessive_nesting, clippy::type_complexity)]
pub(super) fn download_pending_take_remote(
    sync_root: &Path,
    provider: &dyn SyncProvider,
    remote_prefix: &str,
    pending_paths: &[String],
) -> crate::Result<Vec<(String, Option<Vec<u8>>)>> {
    let caps = provider.capabilities();
    let download_pool = sync_download_pool(pending_paths.len(), caps.max_parallel_downloads)?;
    download_pool.install(|| {
        pending_paths
            .par_iter()
            .map(|path| {
                let validated = ValidatedSyncPath::new(path)?;
                let remote_path = format!("{}/{}", remote_prefix, path);
                let remote = provider.read(&remote_path)?;
                let Some(obj) = remote else {
                    return Ok((path.clone(), None));
                };
                let content = obj.content;

                let full_path = validated.join_under(sync_root);
                crate::storage::transaction::atomic_write_bytes(&full_path, &content).map_err(
                    |e| {
                        crate::Error::Io(std::io::Error::other(format!(
                            "write pending_take_remote {}: {}",
                            path, e
                        )))
                    },
                )?;
                Ok((path.clone(), Some(content)))
            })
            .collect()
    })
}

/// 将远端已删除的文件移至 trash 目录而非直接删除。
///
/// trash 文件名格式：`{timestamp}_{uuid}_{original_filename}`。
/// 返回 `Result`——rename 失败必须显式报错；远端 tombstone 要删除本地文件时，
/// 如果移动失败，这一轮 merge 就失败；真实文件还在时不能先把 manifest 当成"已经删除"。
pub(crate) fn move_to_trash(sync_root: &Path, paths: &[String]) -> crate::Result<()> {
    for path in paths {
        let validated = ValidatedSyncPath::new(path)?;
        let full_path = validated.join_under(sync_root);
        if full_path.exists() {
            let filename = full_path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let trash_dir = sync_root.join("app-meta/sync/trash");
            std::fs::create_dir_all(&trash_dir)?;
            let trash_path = trash_dir.join(format!(
                "{}_{}_{}",
                chrono::Utc::now().timestamp_millis(),
                uuid::Uuid::new_v4(),
                filename
            ));
            crate::storage::transaction::durable_rename(&full_path, &trash_path).map_err(|e| {
                crate::Error::Io(std::io::Error::other(format!(
                    "move_to_trash: failed to move {} to trash: {}",
                    path, e
                )))
            })?;
        }
    }
    Ok(())
}

/// 并行下载远端较新文件到本地。
///
/// 使用 rayon 并行线程池，每个文件先写入临时文件（带随机后缀），再 rename 替换目标文件，
/// 保证下载中断不会留下半写入文件。
///
/// 所有远端路径通过 [`ValidatedSyncPath`] 验证后才 join 到本地。
#[allow(clippy::excessive_nesting)]
pub(super) fn download_remote_files(
    sync_root: &Path,
    provider: &dyn SyncProvider,
    remote_prefix: &str,
    to_download: &[String],
) -> crate::Result<()> {
    if to_download.is_empty() {
        return Ok(());
    }
    let caps = provider.capabilities();
    let download_pool = sync_download_pool(to_download.len(), caps.max_parallel_downloads)?;
    let download_result: crate::Result<()> = download_pool.install(|| {
        to_download.par_iter().try_for_each(|path| {
            let validated = ValidatedSyncPath::new(path)?;
            let remote_path = format!("{}/{}", remote_prefix, path);
            let Some(obj) = provider.read(&remote_path)? else {
                return Err(crate::Error::SyncRemoteError {
                    category: "api_error".to_string(),
                    context: format!("remote file missing while downloading {}", path),
                    status: 0,
                    body_preview: String::new(),
                });
            };
            let content = obj.content;
            let full_path = validated.join_under(sync_root);
            crate::storage::transaction::atomic_write_bytes(&full_path, &content)
                .map_err(|e| crate::Error::Io(std::io::Error::other(format!("{}: {}", path, e))))?;
            Ok(())
        })
    });
    download_result
}

/// 拼接相对远端根的完整路径（`remote_prefix` 为空时不加前导 `/`）。
fn remote_path(remote_prefix: &str, rel: &str) -> String {
    if remote_prefix.is_empty() {
        rel.to_string()
    } else {
        format!("{remote_prefix}/{rel}")
    }
}

/// 同一批次内同路径 `Put` 去重：后写覆盖前写，且不产生重复 path 的 mutation。
fn upsert_put(mutations: &mut Vec<BatchMutation>, path: String, content: Vec<u8>) {
    let mutation = BatchMutation::Put { path, content };
    match mutations
        .iter_mut()
        .find(|existing| existing.path() == mutation.path())
    {
        Some(existing) => *existing = mutation,
        None => mutations.push(mutation),
    }
}

/// 本地业务变更的 mutation 规划：
/// - `remote_upload_paths` → `Put`（读 `sync_root` 下的字节；同路径后写覆盖）；
/// - `remote_delete_paths` → 远端有版本时 `DeleteIfMatch`，否则 `Delete`。
fn plan_business_mutations(
    sync_root: &Path,
    remote_prefix: &str,
    outcome: &LwwMergeOutcome,
) -> crate::Result<Vec<BatchMutation>> {
    let mut mutations: Vec<BatchMutation> = Vec::new();
    for rel in &outcome.remote_upload_paths {
        let full_path = sync_root.join(rel);
        let content = std::fs::read(&full_path).map_err(|error| {
            crate::Error::Io(std::io::Error::other(format!(
                "lww atomic publish: read {}: {}",
                full_path.display(),
                error
            )))
        })?;
        upsert_put(&mut mutations, remote_path(remote_prefix, rel), content);
    }
    for rel in &outcome.remote_delete_paths {
        let path = remote_path(remote_prefix, rel);
        // 同一批次里不能对同一 path 同时 Put 和 Delete：本地内容已进入本批时，
        // 以 Put 为准（merge 不应同时产出两者，这里只做防御）。
        if mutations
            .iter()
            .any(|mutation| mutation.path() == path.as_str())
        {
            continue;
        }
        mutations.push(match outcome.remote_tree_files.get(rel.as_str()) {
            Some(version) => BatchMutation::DeleteIfMatch {
                path,
                version: RemoteVersion(version.clone()),
            },
            None => BatchMutation::Delete { path },
        });
    }
    Ok(mutations)
}

/// 远端当前字节是否与本地待发布字节一致（索引等非内容寻址文件用）。
fn remote_content_equals(
    provider: &dyn SyncProvider,
    path: &str,
    bytes: &[u8],
) -> crate::Result<bool> {
    Ok(match provider.read(path)? {
        Some(object) => object.content == bytes,
        None => false,
    })
}

/// 旧清单（v1 单文件 / 旧分片目录）回收：远端存在时按版本 `DeleteIfMatch`。
///
/// 内容寻址 objects/indexes 与上一代根有 `previousRoot` 保留语义，由清单存储的
/// 发布/GC 契约决定；LWW 原子发布不在这里重算代次，也不删除任何新根可达文件。
fn plan_legacy_manifest_cleanup(
    remote_prefix: &str,
    remote_tree_files: &HashMap<String, String>,
) -> Vec<BatchMutation> {
    let shard_prefix = format!("{}/", super::SYNC_MANIFEST_SHARDS_DIR);
    let mut legacy: Vec<&String> = remote_tree_files
        .keys()
        .filter(|rel| rel.as_str() == super::SYNC_MANIFEST_PATH || rel.starts_with(&shard_prefix))
        .collect();
    legacy.sort();
    let mut mutations = Vec::with_capacity(legacy.len());
    for rel in legacy {
        let Some(version) = remote_tree_files.get(rel) else {
            continue;
        };
        mutations.push(BatchMutation::DeleteIfMatch {
            path: remote_path(remote_prefix, rel),
            version: RemoteVersion(version.clone()),
        });
    }
    mutations
}

/// v2 清单的 mutation 规划（objects → indexes → 旧清单回收 → root 最后）。
///
/// - 内容寻址对象：远端已有同名对象即同字节，直接跳过；
/// - 索引：远端字节一致时跳过；
/// - 根：始终按本地新字节 `Put`，且是整批最后一个 mutation（远端提交点）。
///
/// 路径全部由 `local_manifest_storage` + `remote_rel_for` 得出，不硬编码
/// 索引/对象的固定文件名。
fn plan_manifest_mutations(
    sync_root: &Path,
    provider: &dyn SyncProvider,
    remote_prefix: &str,
    manifest_rel: &str,
    outcome: &LwwMergeOutcome,
) -> crate::Result<Vec<BatchMutation>> {
    let store = super::ManifestStore::new(sync_root, manifest_rel);
    // 稀疏轮的 merge 已把对象级增量写进本地 store：只发布 delta 涉及的
    // objects/indexes 与根，不展开完整清单。root 缺失或 delta 视图不可用时
    // 回退完整入口，保持首次同步/迁移/损坏修复的行为不变。
    let delta_files = match outcome.manifest_delta.as_ref() {
        Some(delta) => match super::local_manifest_storage_for_delta(&store, delta) {
            Ok(files) => Some(files),
            Err(error) => {
                log::warn!(
                    "[sync] lww delta manifest view unavailable for {}: {error} — falling back \
                     to the full manifest",
                    sync_root.display()
                );
                None
            }
        },
        None => None,
    };
    let files = match delta_files {
        Some(files) => files,
        None => super::local_manifest_storage(&store, Some(&outcome.merged_manifest))?,
    };
    let mut mutations: Vec<BatchMutation> = Vec::with_capacity(files.len() + 1);
    let mut root_mutation: Option<BatchMutation> = None;
    for file in &files {
        let remote_rel = super::remote_rel_for(&store, &file.rel)?;
        let path = remote_path(remote_prefix, &remote_rel);
        match file.kind {
            super::ManifestStorageKind::Object => {
                if outcome.remote_tree_files.contains_key(&remote_rel) {
                    continue;
                }
                let content = file.read_bytes(sync_root)?;
                mutations.push(BatchMutation::Put { path, content });
            }
            super::ManifestStorageKind::Index => {
                let content = file.read_bytes(sync_root)?;
                if remote_content_equals(provider, &path, &content)? {
                    continue;
                }
                mutations.push(BatchMutation::Put { path, content });
            }
            super::ManifestStorageKind::Root => {
                // 根是提交点：不因远端字节相同而跳过，放到整批最后。
                root_mutation = Some(BatchMutation::Put {
                    path,
                    content: file.read_bytes(sync_root)?,
                });
            }
        }
    }
    mutations.extend(plan_legacy_manifest_cleanup(
        remote_prefix,
        &outcome.remote_tree_files,
    ));
    if let Some(root) = root_mutation {
        mutations.push(root);
    }
    Ok(mutations)
}

/// 把一次 LWW merge 结果规划成**一组**原子提交的 mutations。
///
/// 顺序固定：
/// 1. 正文/业务对象 `Put`；
/// 2. 业务删除 `DeleteIfMatch` / `Delete`；
/// 3. v2 清单 objects → indexes 的 `Put`；
/// 4. 旧 v1 清单（旧单文件、旧分片）回收；
/// 5. **清单根 `Put` 最后**（根是远端提交点）。
///
/// 业务内容与清单根因此在同一次 `commit_batch` 中原子生效，不再出现
/// "正文已变、根未变"的中间版本。
///
/// 稀疏轮（`outcome.manifest_delta` 为 `Some`）的清单只包含 delta 涉及的
/// objects/indexes 与根，不构造完整清单；本地 root 缺失或 delta 视图不可用时
/// 回退完整清单入口。
pub(crate) fn plan_lww_publish(
    sync_root: &Path,
    provider: &dyn SyncProvider,
    remote_prefix: &str,
    manifest_rel: &str,
    outcome: &LwwMergeOutcome,
) -> crate::Result<Vec<BatchMutation>> {
    let mut mutations = plan_business_mutations(sync_root, remote_prefix, outcome)?;
    // 清单语义未变化且业务侧也没有变更时不提交：写根只会制造空提交。
    let publish_manifest = !outcome.manifest_unchanged || !mutations.is_empty();
    if publish_manifest {
        mutations.extend(plan_manifest_mutations(
            sync_root,
            provider,
            remote_prefix,
            manifest_rel,
            outcome,
        )?);
    }
    Ok(mutations)
}
