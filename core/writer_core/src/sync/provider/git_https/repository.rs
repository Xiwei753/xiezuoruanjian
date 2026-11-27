//! Git Smart HTTP 仓库缓存与提交实现。
//!
//! 每轮同步使用一个独立 bare 仓库缓存：fetch 远端分支、按 blob OID 读取对象，
//! `commit_batch` 以固定 base tree OID + 本轮变更只重建受影响目录，再执行普通
//! （非强制）push。缓存与用户正文目录分离；后续同步只下载新增对象。
//!
//! 错误优先识别 TLS 证书校验失败，再按 libgit2 `ErrorCode` 分类、用
//! `ErrorClass` 兜底：只有明确的 DNS/连接拒绝/超时/重置/EOF 等传输不可达
//! 才可触发 GitHub API 后备；证书验证失败、认证、权限和 non-fast-forward
//! 保留准确错误，不会被 `Http`/`Ssl` 或泛型 class 中的握手关键词误报为网络。

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use git2::{
    build::TreeUpdateBuilder, AutotagOption, Commit, DiffOptions, Direction, ErrorClass, ErrorCode,
    FetchOptions, FileMode, ObjectType, Oid, PushOptions, Repository, Signature, Tree,
};

use super::config::GitHttpsRuntimeConfig;
use super::credentials;
use crate::sync::provider::error::ProviderError;
use crate::sync::provider::model::{
    BatchMutation, DeletePrecondition, RemoteEntry, RemoteVersion, WritePrecondition,
};
use crate::sync::provider::ProviderNetworkAccounting;

const REMOTE_NAME: &str = "origin";
const AUTHOR_NAME: &str = "Sujian";
const AUTHOR_EMAIL: &str = "sync@sujian.invalid";

/// 远端分支快照：固定 base commit/tree OID + 按需解析的 path→blob 缓存。
///
/// 只有真正被查询过的路径才写入 `blobs`；提交后的新快照会预填本批变更路径，
/// 未涉及路径保持惰性。不再有进程级全树缓存，也不在 fetch 时展开整棵树。
#[derive(Debug, Default)]
pub(crate) struct Snapshot {
    pub head: Option<Oid>,
    pub tree: Option<Oid>,
    blobs: Mutex<HashMap<String, Oid>>,
}

impl Clone for Snapshot {
    fn clone(&self) -> Self {
        Self {
            head: self.head,
            tree: self.tree,
            blobs: Mutex::new(self.blob_map()),
        }
    }
}

impl Snapshot {
    fn new(head: Option<Oid>, tree: Option<Oid>) -> Self {
        Self {
            head,
            tree,
            blobs: Mutex::new(HashMap::new()),
        }
    }

    /// 按路径取 blob OID：先查已解析缓存，未命中沿 tree 分量下钻并写回缓存。
    pub(crate) fn blob_oid(
        &self,
        repo: &Repository,
        path: &str,
    ) -> Result<Option<Oid>, ProviderError> {
        if let Some(oid) = self.blob_map_get(path) {
            return Ok(Some(oid));
        }
        let Some(oid) = lookup_blob(repo, self.tree, path)? else {
            return Ok(None);
        };
        self.blob_map_insert(path, oid);
        Ok(Some(oid))
    }

    /// 递归枚举整棵树；只有 `list()` 需要全量视图时调用。
    pub(crate) fn list_blobs(
        &self,
        repo: &Repository,
    ) -> Result<Vec<(String, Oid)>, ProviderError> {
        let Some(tree_oid) = self.tree else {
            return Ok(Vec::new());
        };
        let tree = repo
            .find_tree(tree_oid)
            .map_err(|err| map_git_error(err, "find_snapshot_tree"))?;
        let mut blobs = HashMap::new();
        flatten_tree(repo, &tree, "", &mut blobs)?;
        Ok(sorted_blobs(blobs))
    }

    fn blob_map(&self) -> HashMap<String, Oid> {
        self.blobs
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .clone()
    }

    fn blob_map_get(&self, path: &str) -> Option<Oid> {
        self.blobs
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .get(path)
            .copied()
    }

    fn blob_map_insert(&self, path: &str, oid: Oid) {
        self.blobs
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .insert(path.to_string(), oid);
    }

    fn overlay_put(&self, path: &str, oid: Oid) {
        self.blob_map_insert(path, oid);
    }

    fn overlay_remove(&self, path: &str) {
        self.blobs
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .remove(path);
    }
}

fn log_provider_error(operation: &'static str, error: &ProviderError) {
    log::warn!(
        "sync.git_error: operation={} reason_code={}",
        operation,
        error.stable_code()
    );
}

fn logged_provider_error(operation: &'static str, error: ProviderError) -> ProviderError {
    log_provider_error(operation, &error);
    error
}

/// 打开或初始化 bare 仓库缓存。
pub(crate) fn open_or_init(cache_dir: &Path) -> Result<Repository, ProviderError> {
    crate::storage::git_runtime::ensure_tls_initialized().map_err(|err| {
        let reason = err.to_string();
        let mapped = if reason.contains("ca_store_unavailable") {
            ProviderError::TlsCaUnavailable { reason }
        } else {
            ProviderError::GitRuntimeInit { reason }
        };
        log_provider_error("open_or_init", &mapped);
        mapped
    })?;

    match Repository::open(cache_dir) {
        Ok(repo) => Ok(repo),
        Err(open_error) if open_error.code() != ErrorCode::NotFound => Err(logged_provider_error(
            "open_or_init",
            ProviderError::LocalGitCache {
                reason: format!("open git cache failed: {}", open_error.message()),
            },
        )),
        Err(_) => {
            std::fs::create_dir_all(cache_dir).map_err(|err| {
                logged_provider_error(
                    "open_or_init",
                    ProviderError::LocalGitCache {
                        reason: format!("create git cache dir failed: {err}"),
                    },
                )
            })?;
            Repository::init_bare(cache_dir).map_err(|err| {
                logged_provider_error(
                    "open_or_init",
                    ProviderError::LocalGitCache {
                        reason: format!("init git cache failed: {}", err.message()),
                    },
                )
            })
        }
    }
}

fn remote<'repo>(
    repo: &'repo Repository,
    config: &GitHttpsRuntimeConfig,
) -> Result<git2::Remote<'repo>, ProviderError> {
    match repo.find_remote(REMOTE_NAME) {
        Ok(remote) => {
            // 缓存目录按远端 URL 哈希隔离，正常不会出现不同 URL；若配置被改动，
            // 不能静默复用旧地址的对象，也不能覆盖这个缓存里的 remote。
            if remote.url() != Some(config.remote_url.as_str()) {
                return Err(logged_provider_error(
                    "remote",
                    ProviderError::RemoteConfig {
                        reason: "cached git repository remote URL does not match the current configuration"
                            .to_string(),
                    },
                ));
            }
            Ok(remote)
        }
        Err(err) if err.code() == ErrorCode::NotFound => {
            repo.remote(REMOTE_NAME, &config.remote_url).map_err(|err| {
                logged_provider_error(
                    "remote",
                    ProviderError::RemoteConfig {
                        reason: format!("configure git remote failed: {}", err.message()),
                    },
                )
            })
        }
        Err(err) => Err(logged_provider_error(
            "remote",
            ProviderError::RemoteConfig {
                reason: format!("open git remote failed: {}", err.message()),
            },
        )),
    }
}

/// 只连接并读取 refs，不下载对象。用于路由可达性探测与诊断。
pub(crate) fn probe(
    cache_dir: &Path,
    config: &GitHttpsRuntimeConfig,
    accounting: &Arc<ProviderNetworkAccounting>,
) -> Result<(), ProviderError> {
    let repo = open_or_init(cache_dir)?;
    let mut remote = remote(&repo, config)?;
    let callbacks = credentials::callbacks(&config.username, config.secret.as_deref());
    accounting.record_request(0);
    let connection = remote
        .connect_auth(
            Direction::Fetch,
            Some(callbacks),
            Some(credentials::proxy_options()),
        )
        .map_err(|err| map_git_error(err, "connect_auth"))?;
    connection
        .list()
        .map_err(|err| map_git_error(err, "list"))?;
    Ok(())
}

/// 轻量 HEAD 探测：只 connect + list refs，不 fetch、不 flatten tree。
///
/// 空闲轮询只需要把远端分支 HEAD 与上次消费的提交比较；这条路径不下载任何
/// 对象，也不在本地构造 blob map。分支不存在返回 `None`（允许首次 push）。
pub(crate) fn probe_head(
    cache_dir: &Path,
    config: &GitHttpsRuntimeConfig,
    accounting: &Arc<ProviderNetworkAccounting>,
) -> Result<Option<Oid>, ProviderError> {
    let repo = open_or_init(cache_dir)?;
    let mut remote = remote(&repo, config)?;
    let branch_ref = format!("refs/heads/{}", config.branch);
    let callbacks = credentials::callbacks(&config.username, config.secret.as_deref());
    accounting.record_request(0);
    let connection = remote
        .connect_auth(
            Direction::Fetch,
            Some(callbacks),
            Some(credentials::proxy_options()),
        )
        .map_err(|err| map_git_error(err, "connect_auth"))?;
    let heads = connection
        .list()
        .map_err(|err| map_git_error(err, "list"))?;
    Ok(heads
        .iter()
        .find(|head| head.name() == branch_ref)
        .map(|head| head.oid()))
}

/// 用固定 commit 的 tree diff 计算相对 `base` 发生变化的路径。
///
/// `diff_tree_to_tree` 对相同子树直接跳过，代价只跟实际差异相关。`base` 不在
/// 本地缓存（浅克隆/被 GC/首次同步）时返回 `None`，调用方回到完整扫描。
pub(crate) fn changed_paths_between(
    repo: &Repository,
    base: &str,
    head: Oid,
) -> Result<Option<Vec<String>>, ProviderError> {
    let Ok(base_oid) = Oid::from_str(base) else {
        return Ok(None);
    };
    let (Ok(base_commit), Ok(head_commit)) = (repo.find_commit(base_oid), repo.find_commit(head))
    else {
        return Ok(None);
    };
    let (Ok(base_tree), Ok(head_tree)) = (base_commit.tree(), head_commit.tree()) else {
        return Ok(None);
    };
    let mut options = DiffOptions::new();
    let diff = repo
        .diff_tree_to_tree(Some(&base_tree), Some(&head_tree), Some(&mut options))
        .map_err(|err| map_git_error(err, "diff_tree_to_tree"))?;
    let mut paths: Vec<String> = Vec::new();
    diff.foreach(
        &mut |delta, _| {
            if let Some(path) = delta.new_file().path().or_else(|| delta.old_file().path()) {
                paths.push(path.to_string_lossy().replace('\\', "/"));
            }
            true
        },
        None,
        None,
        None,
    )
    .map_err(|err| map_git_error(err, "diff_foreach"))?;
    paths.sort();
    paths.dedup();
    Ok(Some(paths))
}

/// fetch 指定分支并构建快照。分支不存在视为空仓库（允许首次 push）。
pub(crate) fn fetch_snapshot(
    repo: &Repository,
    config: &GitHttpsRuntimeConfig,
    accounting: &Arc<ProviderNetworkAccounting>,
) -> Result<Snapshot, ProviderError> {
    let mut remote = remote(repo, config)?;
    let branch_ref = format!("refs/heads/{}", config.branch);

    // 先 connect/list：既验证认证与可达性，也准确判断分支是否已存在。
    let callbacks = credentials::callbacks(&config.username, config.secret.as_deref());
    accounting.record_request(0);
    let connection = remote
        .connect_auth(
            Direction::Fetch,
            Some(callbacks),
            Some(credentials::proxy_options()),
        )
        .map_err(|err| map_git_error(err, "connect_auth"))?;
    let heads = connection
        .list()
        .map_err(|err| map_git_error(err, "list"))?;
    let branch_exists = heads.iter().any(|head| head.name() == branch_ref);
    drop(connection);

    if !branch_exists {
        return Ok(Snapshot::default());
    }

    let refspec = format!("+{branch_ref}:refs/remotes/{REMOTE_NAME}/{}", config.branch);
    let mut fetch_options = FetchOptions::new();
    let progress_accounting = Arc::clone(accounting);
    let mut last_received_bytes = 0usize;
    let mut callbacks = credentials::callbacks(&config.username, config.secret.as_deref());
    callbacks.transfer_progress(move |progress| {
        let received_bytes = progress.received_bytes();
        let delta = received_bytes.saturating_sub(last_received_bytes);
        if delta > 0 {
            progress_accounting.record_bytes_received(delta);
            last_received_bytes = received_bytes;
        }
        true
    });
    fetch_options.remote_callbacks(callbacks);
    fetch_options.proxy_options(credentials::proxy_options());
    fetch_options.download_tags(AutotagOption::None);
    accounting.record_request(0);
    remote
        .fetch(
            &[refspec],
            Some(&mut fetch_options),
            Some("sujian sync fetch"),
        )
        .map_err(|err| map_git_error(err, "fetch"))?;

    let remote_ref = format!("refs/remotes/{REMOTE_NAME}/{}", config.branch);
    let head = repo
        .refname_to_id(&remote_ref)
        .map_err(|err| map_git_error(err, "resolve_fetched_branch"))?;
    let commit = repo
        .find_commit(head)
        .map_err(|err| map_git_error(err, "find_fetched_commit"))?;

    Ok(Snapshot::new(Some(head), Some(commit.tree_id())))
}

/// 按路径分量从 tree 根下钻查询单个 blob；不会展开整棵树。
fn lookup_blob(
    repo: &Repository,
    tree_oid: Option<Oid>,
    path: &str,
) -> Result<Option<Oid>, ProviderError> {
    let Some(tree_oid) = tree_oid else {
        return Ok(None);
    };
    if !is_valid_blob_path(path) {
        return Ok(None);
    }
    let mut components: Vec<&str> = path.split('/').collect();
    let Some(file_name) = components.pop() else {
        return Ok(None);
    };
    let mut current = repo
        .find_tree(tree_oid)
        .map_err(|err| map_git_error(err, "find_snapshot_tree"))?;
    for component in components {
        let child_oid = {
            let Some(entry) = current.get_name(component) else {
                return Ok(None);
            };
            if entry.kind() != Some(ObjectType::Tree) {
                return Ok(None);
            }
            entry.id()
        };
        current = repo
            .find_tree(child_oid)
            .map_err(|err| map_git_error(err, "read_tree_entry"))?;
    }
    let Some(entry) = current.get_name(file_name) else {
        return Ok(None);
    };
    if entry.kind() == Some(ObjectType::Blob) {
        Ok(Some(entry.id()))
    } else {
        Ok(None)
    }
}

/// 解析目录前缀对应的子树；缺失或指向 blob 时返回 `None`。
fn tree_at_path<'repo>(
    repo: &'repo Repository,
    root: Option<Oid>,
    path: &str,
) -> Result<Option<Tree<'repo>>, ProviderError> {
    let Some(root_oid) = root else {
        return Ok(None);
    };
    let mut current = repo
        .find_tree(root_oid)
        .map_err(|err| map_git_error(err, "find_snapshot_tree"))?;
    if path.is_empty() {
        return Ok(Some(current));
    }
    if !is_valid_blob_path(path) {
        return Ok(None);
    }
    for component in path.split('/') {
        let child_oid = {
            let Some(entry) = current.get_name(component) else {
                return Ok(None);
            };
            if entry.kind() != Some(ObjectType::Tree) {
                return Ok(None);
            }
            entry.id()
        };
        current = repo
            .find_tree(child_oid)
            .map_err(|err| map_git_error(err, "read_tree_entry"))?;
    }
    Ok(Some(current))
}

fn sorted_blobs(blobs: HashMap<String, Oid>) -> Vec<(String, Oid)> {
    let mut entries: Vec<(String, Oid)> = blobs.into_iter().collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    entries
}

fn flatten_tree(
    repo: &Repository,
    tree: &Tree<'_>,
    prefix: &str,
    out: &mut HashMap<String, Oid>,
) -> Result<(), ProviderError> {
    for entry in tree.iter() {
        let name = String::from_utf8_lossy(entry.name_bytes()).into_owned();
        let path = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        match entry.kind() {
            Some(ObjectType::Blob) => {
                out.insert(path, entry.id());
            }
            Some(ObjectType::Tree) => {
                let child = repo
                    .find_tree(entry.id())
                    .map_err(|err| map_git_error(err, "read_tree_entry"))?;
                flatten_tree(repo, &child, &path, out)?;
            }
            _ => {}
        }
    }
    Ok(())
}

/// 可安全交给 libgit2 tree 更新器的 blob 路径。
fn is_valid_blob_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.ends_with('/')
        && !path.as_bytes().contains(&0)
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

fn validate_blob_path(path: &str) -> Result<(), ProviderError> {
    if !is_valid_blob_path(path) {
        return Err(ProviderError::Other {
            reason: format!("invalid repository path: {path}"),
        });
    }
    if path.as_bytes().len() >= 0x0fff {
        return Err(ProviderError::Other {
            reason: format!("repository path is too long: {path}"),
        });
    }
    Ok(())
}

/// 从本地已创建的 commit 重建快照，不访问网络、不展开 tree。
///
/// 只用于把本 provider 自己成功 push 的提交推进为下一轮基线；远端是否变化
/// 由普通（非强制）push 的 fast-forward 约束决定。
pub(crate) fn snapshot_from_commit(
    repo: &Repository,
    commit_oid: Oid,
) -> Result<Snapshot, ProviderError> {
    let commit = repo
        .find_commit(commit_oid)
        .map_err(|err| map_git_error(err, "find_committed_commit"))?;
    Ok(Snapshot::new(Some(commit_oid), Some(commit.tree_id())))
}

/// 在当前快照基础上提交一个 commit 并 push。返回新 commit OID 与本提交的本地快照。
pub(crate) fn commit_and_push(
    repo: &Repository,
    config: &GitHttpsRuntimeConfig,
    snapshot: &Snapshot,
    mutations: &[BatchMutation],
    message: &str,
    accounting: &Arc<ProviderNetworkAccounting>,
) -> Result<(Oid, Snapshot), ProviderError> {
    let built = build_tree(repo, snapshot, mutations)?;

    // No-change rounds must not publish a commit.  Git already has the exact
    // tree; compare the tree OID before creating a commit or pushing.
    if let (Some(head), Some(base_tree)) = (snapshot.head, built.base_tree) {
        if base_tree == built.tree {
            return Ok((head, snapshot.clone()));
        }
    }

    let signature = Signature::now(AUTHOR_NAME, AUTHOR_EMAIL)
        .map_err(|err| map_git_error(err, "create_signature"))?;

    let parent = snapshot
        .head
        .map(|oid| repo.find_commit(oid))
        .transpose()
        .map_err(|err| map_git_error(err, "load_parent_commit"))?;
    let parents: Vec<&Commit<'_>> = parent.iter().collect();
    let tree = repo
        .find_tree(built.tree)
        .map_err(|err| map_git_error(err, "find_written_tree"))?;
    let commit_oid = repo
        .commit(
            None,
            &signature,
            &signature,
            message,
            &tree,
            parents.as_slice(),
        )
        .map_err(|err| map_git_error(err, "create_commit"))?;

    let local_ref = format!("refs/heads/{}", config.branch);
    if !git2::Reference::is_valid_name(&local_ref) {
        return Err(ProviderError::Other {
            reason: format!("invalid branch name: {}", config.branch),
        });
    }
    repo.reference(&local_ref, commit_oid, true, "sujian sync commit")
        .map_err(|err| map_git_error(err, "update_local_branch"))?;

    push_branch(repo, config, &local_ref, &commit_oid, accounting)?;
    // 新基线按差量构造：快照固定新 commit/tree OID，只预填本批变更路径，
    // 其余路径继续按需查询（Issue #1065 复审 #5 / Issue #1062）。
    let committed_snapshot = Snapshot::new(Some(commit_oid), Some(built.tree));
    for mutation in mutations {
        match mutation {
            BatchMutation::Put { path, .. } => {
                if let Some(oid) = built.written_blobs.get(path) {
                    committed_snapshot.overlay_put(path, *oid);
                }
            }
            BatchMutation::ReuseVersion { path, version } => {
                if let Ok(oid) = Oid::from_str(version.as_str()) {
                    committed_snapshot.overlay_put(path, oid);
                }
            }
            BatchMutation::Delete { path } | BatchMutation::DeleteIfMatch { path, .. } => {
                committed_snapshot.overlay_remove(path);
            }
            BatchMutation::CheckObject { .. } | BatchMutation::CheckPrefix { .. } => {}
        }
    }
    Ok((commit_oid, committed_snapshot))
}

struct TreeBuild {
    tree: Oid,
    base_tree: Option<Oid>,
    written_blobs: HashMap<String, Oid>,
}

enum TreeChange {
    Upsert(Oid),
    Remove,
}

/// 用固定 base tree OID + 本批变更直接重建 tree。
///
/// 只重建受影响目录，未变子树 OID 原样复用；不再先 `Index::read_tree` 装载
/// 整棵树，也不再展开整棵树来生成快照。
fn build_tree(
    repo: &Repository,
    snapshot: &Snapshot,
    mutations: &[BatchMutation],
) -> Result<TreeBuild, ProviderError> {
    let base_tree_oid = match snapshot.tree {
        Some(tree) => Some(tree),
        None => snapshot
            .head
            .map(|head| {
                repo.find_commit(head)
                    .map(|commit| commit.tree_id())
                    .map_err(|err| map_git_error(err, "find_base_commit"))
            })
            .transpose()?,
    };

    // 同一路径在一个 batch 里多次出现时以后出现的动作为准；libgit2 的
    // tree 更新器不允许重复条目。
    let mut changes: HashMap<String, TreeChange> = HashMap::new();
    let mut written_blobs: HashMap<String, Oid> = HashMap::new();
    for mutation in mutations {
        match mutation {
            BatchMutation::Put { path, content } => {
                validate_blob_path(path)?;
                let oid = repo
                    .blob(content)
                    .map_err(|err| map_git_error(err, "write_blob"))?;
                changes.insert(path.clone(), TreeChange::Upsert(oid));
                written_blobs.insert(path.clone(), oid);
            }
            BatchMutation::ReuseVersion { path, version } => {
                validate_blob_path(path)?;
                let oid = Oid::from_str(version.as_str()).map_err(|_| {
                    ProviderError::PreconditionFailed {
                        path: path.clone(),
                        reason: format!("invalid blob version: {version}"),
                    }
                })?;
                if repo.find_blob(oid).is_err() {
                    return Err(ProviderError::PreconditionFailed {
                        path: path.clone(),
                        reason: "reused blob is not available in the local git cache".to_string(),
                    });
                }
                changes.insert(path.clone(), TreeChange::Upsert(oid));
            }
            BatchMutation::Delete { path } => {
                // 删除不存在的路径保持幂等，与 HTTP DELETE 语义一致；
                // 构建 tree 时会跳过 base 中不存在的路径。
                changes.insert(path.clone(), TreeChange::Remove);
            }
            BatchMutation::DeleteIfMatch { path, version } => {
                check_delete_precondition(
                    repo,
                    snapshot,
                    path,
                    &DeletePrecondition::IfMatch(version.clone()),
                )?;
                changes.insert(path.clone(), TreeChange::Remove);
            }
            BatchMutation::CheckObject {
                path,
                expected_version,
            } => check_object_precondition(repo, snapshot, path, expected_version.as_ref())?,
            BatchMutation::CheckPrefix {
                prefix,
                expected_entries,
            } => check_prefix_precondition(repo, snapshot, prefix, expected_entries)?,
        }
    }

    let base_tree = resolve_base_tree(repo, base_tree_oid)?;
    let mut updates = TreeUpdateBuilder::new();
    for (path, change) in &changes {
        match change {
            TreeChange::Upsert(oid) => {
                updates.upsert(path.as_str(), *oid, FileMode::Blob);
            }
            TreeChange::Remove => {
                // `git_treebuilder_remove` 对不存在的条目报错；幂等删除要求
                // 先确认 base 中确实存在该路径。
                if snapshot.blob_oid(repo, path)?.is_some() {
                    updates.remove(path.as_str());
                }
            }
        }
    }
    let tree = updates
        .create_updated(repo, &base_tree)
        .map_err(|err| map_git_error(err, "write_tree"))?;
    Ok(TreeBuild {
        tree,
        base_tree: base_tree_oid,
        written_blobs,
    })
}

/// 取 baseline tree；首次提交时以 Git 空 tree 作为基线。
fn resolve_base_tree<'repo>(
    repo: &'repo Repository,
    base_tree: Option<Oid>,
) -> Result<Tree<'repo>, ProviderError> {
    match base_tree {
        Some(oid) => repo
            .find_tree(oid)
            .map_err(|err| map_git_error(err, "find_base_tree")),
        None => {
            let builder = repo
                .treebuilder(None)
                .map_err(|err| map_git_error(err, "create_empty_tree"))?;
            let empty_oid = builder
                .write()
                .map_err(|err| map_git_error(err, "write_empty_tree"))?;
            repo.find_tree(empty_oid)
                .map_err(|err| map_git_error(err, "find_empty_tree"))
        }
    }
}

fn push_branch(
    repo: &Repository,
    config: &GitHttpsRuntimeConfig,
    local_ref: &str,
    commit_oid: &Oid,
    accounting: &Arc<ProviderNetworkAccounting>,
) -> Result<(), ProviderError> {
    let mut remote = remote(repo, config)?;
    let rejection: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let rejection_sink = Arc::clone(&rejection);

    let mut callbacks = credentials::callbacks(&config.username, config.secret.as_deref());
    callbacks.push_update_reference(move |refname, message| {
        if let Some(message) = message {
            if let Ok(mut slot) = rejection_sink.lock() {
                *slot = Some(format!("{refname}: {message}"));
            }
        }
        Ok(())
    });
    let progress_accounting = Arc::clone(accounting);
    let mut last_sent_bytes = 0usize;
    callbacks.push_transfer_progress(move |_current, _total, sent_bytes| {
        let delta = sent_bytes.saturating_sub(last_sent_bytes);
        if delta > 0 {
            progress_accounting.record_bytes_sent(delta);
            last_sent_bytes = sent_bytes;
        }
    });

    let mut push_options = PushOptions::new();
    push_options.remote_callbacks(callbacks);
    push_options.proxy_options(credentials::proxy_options());
    let refspec = format!("{local_ref}:refs/heads/{}", config.branch);
    accounting.record_request(0);
    let push_result = remote.push(&[refspec], Some(&mut push_options));

    if let Some(reason) = rejection.lock().ok().and_then(|mut slot| slot.take()) {
        return Err(classify_push_rejection(&reason));
    }
    if let Err(err) = push_result {
        let mapped = map_git_error(err, "push");
        // 只把能确定的 non-fast-forward 转为可重规划冲突；协议、钩子、本地
        // 或其他服务器限制保留原始错误，不能统一伪装成并发冲突。
        if let ProviderError::Other { reason } = &mapped {
            if is_non_fast_forward_reason(reason) {
                return Err(ProviderError::PreconditionFailed {
                    path: commit_oid.to_string(),
                    reason: reason.clone(),
                });
            }
        }
        return Err(mapped);
    }
    Ok(())
}

fn classify_push_rejection(reason: &str) -> ProviderError {
    let lower = reason.to_ascii_lowercase();
    if lower.contains("protected")
        || lower.contains("not allowed")
        || lower.contains("permission")
        || lower.contains("denied")
    {
        return ProviderError::PermissionDenied {
            reason: reason.to_string(),
        };
    }
    if is_non_fast_forward_reason(reason) {
        return ProviderError::PreconditionFailed {
            path: "refs/heads".to_string(),
            reason: reason.to_string(),
        };
    }
    ProviderError::Other {
        reason: reason.to_string(),
    }
}

/// 远端/ref 回调中可确定是并发基线变化的描述。
fn is_non_fast_forward_reason(reason: &str) -> bool {
    let lower = reason.to_ascii_lowercase();
    lower.contains("non-fast-forward")
        || lower.contains("non fast forward")
        || lower.contains("not a fast forward")
        || lower.contains("fetch first")
        || lower.contains("stale info")
        || lower.contains("failed to update ref")
        || lower.contains("cannot lock ref")
}

/// 校验写前置条件；调用方已持有最新快照。
pub(crate) fn check_write_precondition(
    repo: &Repository,
    snapshot: &Snapshot,
    path: &str,
    precondition: &WritePrecondition,
) -> Result<(), ProviderError> {
    match precondition {
        WritePrecondition::IfMatch(expected) => match snapshot.blob_oid(repo, path)? {
            Some(current) if current.to_string() == expected.as_str() => Ok(()),
            Some(current) => Err(ProviderError::PreconditionFailed {
                path: path.to_string(),
                reason: format!("version mismatch: expected={expected}, current={current}"),
            }),
            None => Err(ProviderError::NotFound {
                path: path.to_string(),
            }),
        },
        WritePrecondition::CreateNew => {
            if snapshot.blob_oid(repo, path)?.is_some() {
                Err(ProviderError::PreconditionFailed {
                    path: path.to_string(),
                    reason: "object already exists".to_string(),
                })
            } else {
                Ok(())
            }
        }
        WritePrecondition::Unconditional => Ok(()),
    }
}

pub(crate) fn check_delete_precondition(
    repo: &Repository,
    snapshot: &Snapshot,
    path: &str,
    precondition: &DeletePrecondition,
) -> Result<(), ProviderError> {
    match precondition {
        DeletePrecondition::IfMatch(expected) => match snapshot.blob_oid(repo, path)? {
            Some(current) if current.to_string() == expected.as_str() => Ok(()),
            Some(current) => Err(ProviderError::PreconditionFailed {
                path: path.to_string(),
                reason: format!("version mismatch: expected={expected}, current={current}"),
            }),
            None => Err(ProviderError::NotFound {
                path: path.to_string(),
            }),
        },
        DeletePrecondition::Unconditional => Ok(()),
    }
}

pub(crate) fn check_object_precondition(
    repo: &Repository,
    snapshot: &Snapshot,
    path: &str,
    expected_version: Option<&RemoteVersion>,
) -> Result<(), ProviderError> {
    let current = blob_version(repo, snapshot, path)?;
    if current.as_ref() == expected_version {
        Ok(())
    } else {
        Err(ProviderError::PreconditionFailed {
            path: path.to_string(),
            reason: format!(
                "object version mismatch: expected={expected_version:?}, current={current:?}"
            ),
        })
    }
}

pub(crate) fn check_prefix_precondition(
    repo: &Repository,
    snapshot: &Snapshot,
    prefix: &str,
    expected_entries: &[RemoteEntry],
) -> Result<(), ProviderError> {
    // 与既有 provider 的 `"{prefix}/"` 过滤语义保持一致：空前缀不匹配任何
    // 路径，不能把根目录当成"空前缀下的全部对象"。
    let mut current_entries: Vec<RemoteEntry> = if prefix.is_empty() {
        Vec::new()
    } else {
        list_blobs_under(repo, snapshot.tree, prefix)?
            .into_iter()
            .map(|(path, oid)| RemoteEntry {
                path,
                version: RemoteVersion::new(oid.to_string()),
            })
            .collect()
    };
    current_entries.sort_by(|a, b| a.path.cmp(&b.path));

    let mut expected_entries = expected_entries.to_vec();
    expected_entries.sort_by(|a, b| a.path.cmp(&b.path));
    if current_entries == expected_entries {
        Ok(())
    } else {
        Err(ProviderError::PreconditionFailed {
            path: prefix.to_string(),
            reason: "directory object set or version changed".to_string(),
        })
    }
}

/// 枚举目录前缀下的全部 blob（相对前缀的路径），不展开前缀之外的子树。
fn list_blobs_under(
    repo: &Repository,
    tree_oid: Option<Oid>,
    prefix: &str,
) -> Result<Vec<(String, Oid)>, ProviderError> {
    let Some(tree) = tree_at_path(repo, tree_oid, prefix)? else {
        return Ok(Vec::new());
    };
    let mut blobs = HashMap::new();
    flatten_tree(repo, &tree, "", &mut blobs)?;
    Ok(sorted_blobs(blobs))
}

pub(crate) fn blob_version(
    repo: &Repository,
    snapshot: &Snapshot,
    path: &str,
) -> Result<Option<RemoteVersion>, ProviderError> {
    Ok(snapshot
        .blob_oid(repo, path)?
        .map(|oid| RemoteVersion(oid.to_string())))
}

pub(crate) fn read_blob(
    repo: &Repository,
    snapshot: &Snapshot,
    path: &str,
) -> Result<Option<Vec<u8>>, ProviderError> {
    let Some(oid) = snapshot.blob_oid(repo, path)? else {
        return Ok(None);
    };
    let blob = repo
        .find_blob(oid)
        .map_err(|err| map_git_error(err, "read_blob"))?;
    Ok(Some(blob.content().to_vec()))
}

/// 把 libgit2 错误映射到 ProviderError，并在第一现场记录不含敏感信息的结构化诊断。
///
/// `operation` 必须是稳定的 snake_case 名称（如 `open_or_init`、`connect_auth`、
/// `list`）。网络不可达只有在本层被明确识别后才返回 `Network`，供 router 决定
/// 是否进入 GitHub API 后备；证书、认证、HTTP 状态和本地 Git 错误保持独立。
pub(crate) fn map_git_error(err: git2::Error, operation: &'static str) -> ProviderError {
    let raw_message = err.message().to_string();
    let code = err.code();
    let class = err.class();
    let mapped = classify_git_error(operation, &raw_message, code, class);
    // 只记录错误码/类别/稳定原因码、错误文本提示和消息类型，
    // 不落 URL、Token、文件路径、证书内容或原始报文。
    log::warn!(
        "sync.git_error: operation={} git_error_code={:?} git_error_class={:?} reason_code={} tls_message_hint={} message_kind={}",
        operation,
        code,
        class,
        mapped.stable_code(),
        tls_message_hint(&raw_message),
        message_kind(&raw_message)
    );
    mapped
}

fn classify_git_error(
    operation: &str,
    raw_message: &str,
    code: ErrorCode,
    class: ErrorClass,
) -> ProviderError {
    let message = format!("{operation}: {raw_message}");

    // 证书校验失败可能被 libgit2 报成 `Certificate` code，也可能落在
    // `Http`/`Net`/`Ssl` 或泛型 class/code 中。必须在任何 class 和网络
    // 关键词判断之前识别，否则 `certificate verify failed during TLS
    // handshake` 这类文本会被 `handshake` 等泛关键词误判为网络不可达，
    // 从而错误触发 GitHub API 后备。
    if code == ErrorCode::Certificate {
        return classify_tls_verification_error(&message, raw_message).unwrap_or_else(|| {
            ProviderError::TlsCertificateInvalid {
                reason: format!("TLS certificate verification failed: {message}"),
            }
        });
    }
    if let Some(error) = classify_tls_verification_error(&message, raw_message) {
        return error;
    }

    // 明确的 HTTP 认证/权限/状态错误也先于连接不可达判断。`Http` class
    // 必然检查；其他 class 只有文本包含明确 HTTP 语义时才检查，避免把
    // 本地 Git 错误里的普通词或 OID 数字误当成 HTTP 状态。
    if class == ErrorClass::Http || has_explicit_http_status(raw_message) {
        let mapped = classify_http_error(&message, code);
        if !matches!(mapped, ProviderError::Other { .. }) {
            return mapped;
        }
    }

    match code {
        ErrorCode::Timeout | ErrorCode::Eof => {
            return ProviderError::Network { reason: message };
        }
        ErrorCode::Auth => {
            return ProviderError::AuthFailed { reason: message };
        }
        ErrorCode::NotFastForward => {
            return ProviderError::PreconditionFailed {
                path: operation.to_string(),
                reason: raw_message.to_string(),
            };
        }
        _ => {}
    }

    // Issue #1011：网络操作在 TLS/套接字读写时被信号打断（EINTR）时，
    // libgit2/OpenSSL 可能没有对应错误码，只把错误归到 `Net`/`Ssl`/`Http`
    // class；真机日志里同一文本也可能落到 `Os`。此时在一个已经出错的会话上
    // 原样重放没有意义，也不能落进 `Other` 静默收尾；识别为 `Network`，让
    // 整轮同步重新建立 Provider 并重新读取远端。只接受明确的
    // `Interrupted system call`/`EINTR` 文本，且限定在网络操作，避免把证书、
    // 认证或本地 Git 读写中断误报为网络故障。`Os` 必须同时具备 SSL/TLS 与
    // socket/system-call failure 语义，普通本地文件 IO 仍走 `Other`。
    let interrupted_transport_class =
        matches!(class, ErrorClass::Net | ErrorClass::Ssl | ErrorClass::Http)
            || (class == ErrorClass::Os && is_ssl_syscall_failure(raw_message));
    if is_interruptible_network_operation(operation)
        && is_transport_interrupted(raw_message)
        && interrupted_transport_class
    {
        return ProviderError::Network {
            reason: format!("transport interrupted: {message}"),
        };
    }

    match class {
        // `ErrorClass::Net` 是 libgit2 对连接层的明确分类；证书和 HTTP
        // 失败已在上面优先排除，此处才允许直接进入 GitHub API 后备。
        ErrorClass::Net => ProviderError::Network { reason: message },
        // 未命中证书文本的 SSL/握手错误性质不明，不能凭 `handshake`、
        // `ssl connect error` 等泛关键词触发 REST 后备。只有明确出现
        // 连接拒绝/超时/重置/EOF 等传输失败时才归为 Network。
        //
        // 注意：libgit2 的 OpenSSL 封装会把 SSL_ERROR_NONE、ZERO_RETURN 及
        // 其它未单独分支的返回值统一写成 `SSL error: unknown error`；
        // `git2::Error` 不暴露 `SSL_get_error` 的具体分支，文本也无法反推出
        // 真实失败阶段，因此保持 Other，不能误报证书错误或网络不可达。
        ErrorClass::Ssl => {
            if is_network_transport_error(raw_message) {
                ProviderError::Network {
                    reason: format!("TLS transport error: {message}"),
                }
            } else {
                ProviderError::Other { reason: message }
            }
        }
        ErrorClass::Http => {
            // 状态码/认证错误已在上面返回；这里只处理无法识别状态码的
            // HTTP 错误，并且只有明确传输失败文本才转为 Network。
            if is_network_transport_error(raw_message) {
                ProviderError::Network { reason: message }
            } else {
                ProviderError::Other { reason: message }
            }
        }
        ErrorClass::Reference if code == ErrorCode::NotFound => ProviderError::NotFound {
            path: operation.to_string(),
        },
        _ => {
            if is_network_transport_error(raw_message) {
                ProviderError::Network { reason: message }
            } else {
                ProviderError::Other { reason: message }
            }
        }
    }
}

/// 按 OpenSSL/libgit2 的错误文本区分 CA 信任源缺失与证书链/主机名校验失败。
///
/// 返回 `None` 表示文本没有明确证书校验语义，留给后续 class/code 判断。
fn classify_tls_verification_error(message: &str, raw_message: &str) -> Option<ProviderError> {
    if is_ca_trust_error(raw_message) {
        return Some(ProviderError::TlsCaUnavailable {
            reason: format!("TLS CA trust unavailable: {message}"),
        });
    }
    if is_tls_certificate_error(raw_message) {
        return Some(ProviderError::TlsCertificateInvalid {
            reason: format!("TLS certificate verification failed: {message}"),
        });
    }
    None
}

/// 明确的 HTTP 状态/错误语义。用于让非 `Http` class 的 HTTP 错误也能先于
/// 连接不可达判断，同时避免仅凭普通词或 OID 中的数字误判。
fn has_explicit_http_status(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    if lower.contains("unauthorized")
        || lower.contains("forbidden")
        || lower.contains("proxy authentication")
        || lower.contains("proxy authorization")
        || lower.contains("internal server error")
        || lower.contains("bad gateway")
        || lower.contains("service unavailable")
        || lower.contains("gateway timeout")
    {
        return true;
    }
    let has_status_code = ["401", "403", "404", "407", "500", "502", "503", "504"]
        .iter()
        .any(|status| lower.contains(status));
    has_status_code && (lower.contains("http") || lower.contains("status"))
}

/// 明确属于连接层不可达的错误文本。只用于分类，不写入诊断日志。
///
/// 不包含 `handshake`、`ssl connect error` 或单独 `proxy` 等性质不明的
/// 关键词；这类错误保持 `Other`，由诊断继续观察，不能驱动 REST 后备。
fn is_network_transport_error(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    const NEEDLES: &[&str] = &[
        "could not resolve host",
        "could not resolve",
        "failed to resolve",
        "temporary failure in name resolution",
        "name or service not known",
        "nodename nor servname",
        "no such host",
        "connection refused",
        "connection reset",
        "connection timed out",
        "connection was reset",
        "failed to connect",
        "couldn't connect",
        "could not connect",
        "unable to connect",
        "network is unreachable",
        "no route to host",
        "host is unreachable",
        "operation timed out",
        "timed out",
        "timeout",
        "proxy connection failed",
        "failed to connect to proxy",
        "could not connect to proxy",
        "unable to connect to proxy",
        "proxy connect error",
        "tunnel connection failed",
        "early eof",
        "unexpected eof",
        "end of file",
        "broken pipe",
        "connection closed",
        "recv failure",
        "send failure",
    ];
    NEEDLES.iter().any(|needle| lower.contains(needle))
}

/// 会经过 TLS/长连接传输的 Git 操作。只有这些操作中的 EINTR 才表示
/// 传输被系统调用中断；本地 Git 读写被信号打断不属于可切换网络的故障。
fn is_interruptible_network_operation(operation: &str) -> bool {
    matches!(operation, "connect_auth" | "list" | "fetch" | "push")
}

/// 明确的系统调用被信号中断语义。只匹配 EINTR 文本，不把泛 SSL/握手
/// 错误都当成传输中断。
fn is_transport_interrupted(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("interrupted system call")
        || lower.contains("syscall failure: interrupted")
        || lower.contains("eintr")
}

/// `ErrorClass::Os` 下的 EINTR 只有同时具备 SSL/TLS 与 socket/system-call
/// failure 语义时才属于可切换的传输中断；普通本地文件 IO 的 `Os` 错误仍按
/// `Other` 处理。
fn is_ssl_syscall_failure(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    let has_tls = lower.contains("ssl") || lower.contains("tls");
    let has_syscall = lower.contains("syscall failure")
        || lower.contains("system call")
        || lower.contains("socket");
    has_tls && has_syscall
}

/// 脱敏诊断用的消息类型；不记录原始文本。
fn message_kind(message: &str) -> &'static str {
    let lower = message.to_ascii_lowercase();
    if is_ca_trust_error(&lower) {
        return "tls_ca";
    }
    if is_tls_certificate_error(message) || lower.contains("certificate") {
        return "tls_certificate";
    }
    if is_transport_interrupted(message) {
        return "transport_interrupted";
    }
    if lower.contains("handshake") || lower.contains("ssl connect error") {
        return "tls_handshake";
    }
    if lower.contains("could not resolve")
        || lower.contains("failed to resolve")
        || lower.contains("name resolution")
        || lower.contains("name or service not known")
        || lower.contains("no such host")
    {
        return "dns";
    }
    if lower.contains("connection refused") {
        return "connect_refused";
    }
    if lower.contains("connection reset")
        || lower.contains("broken pipe")
        || lower.contains("connection closed")
        || lower.contains("recv failure")
        || lower.contains("send failure")
    {
        return "connect_reset";
    }
    if lower.contains("timed out") || lower.contains("timeout") {
        return "timeout";
    }
    if lower.contains("proxy authentication") || lower.contains("proxy authorization") {
        return "auth";
    }
    if lower.contains("proxy") || lower.contains("tunnel") {
        return "proxy";
    }
    if lower.contains("eof") || lower.contains("end of file") {
        return "eof";
    }
    if lower.contains("401")
        || lower.contains("407")
        || lower.contains("unauthorized")
        || lower.contains("authentication")
        || lower.contains("credentials")
    {
        return "auth";
    }
    if lower.contains("403") || lower.contains("forbidden") {
        return "http_forbidden";
    }
    if lower.contains("404") || lower.contains("not found") {
        return "http_not_found";
    }
    if lower.contains("500")
        || lower.contains("502")
        || lower.contains("503")
        || lower.contains("504")
        || lower.contains("server error")
        || lower.contains("bad gateway")
        || lower.contains("service unavailable")
        || lower.contains("gateway timeout")
    {
        return "http_server";
    }
    "unknown"
}

/// 从 OpenSSL/libgit2 的既有错误文本提取稳定的诊断提示。
///
/// 注意：这只是对 `git2::Error::message()` 的文本归类，不是 OpenSSL 的真实
/// 验证结果；libgit2 的 `SSL error: unknown error` 无法从文本逆推出
/// `SSL_get_error` 的具体分支。这里只返回固定枚举字符串，原始错误文本、
/// 证书主题/签发者、URL 和路径都不进入日志。`openssl_error_unknown`
/// 仅表示文本里出现 SSL/TLS 字样，不能据此推断网络不可达或证书失败。
fn tls_message_hint(message: &str) -> &'static str {
    let lower = message.to_ascii_lowercase();
    if lower.contains("unable to get local issuer") {
        "x509_unable_to_get_issuer_cert_locally"
    } else if lower.contains("unable to get issuer certificate") {
        "x509_unable_to_get_issuer_cert"
    } else if lower.contains("unable to verify the first certificate") {
        "x509_unable_to_verify_first_cert"
    } else if lower.contains("unknown ca") {
        "x509_unknown_ca"
    } else if lower.contains("no ca certificate") {
        "x509_no_ca_certificate"
    } else if lower.contains("self signed") || lower.contains("self-signed") {
        "x509_self_signed_certificate"
    } else if lower.contains("hostname mismatch")
        || lower.contains("hostname does not match")
        || lower.contains("does not match certificate")
        || lower.contains("certificate subject name")
    {
        "x509_hostname_mismatch"
    } else if lower.contains("certificate has expired") {
        "x509_certificate_expired"
    } else if lower.contains("certificate is not yet valid") {
        "x509_certificate_not_yet_valid"
    } else if lower.contains("certificate revoked") {
        "x509_certificate_revoked"
    } else if lower.contains("unsupported certificate purpose") {
        "x509_unsupported_certificate_purpose"
    } else if lower.contains("certificate chain")
        || lower.contains("peer certificate cannot be authenticated")
        || lower.contains("invalid certificate")
    {
        "x509_certificate_chain_invalid"
    } else if lower.contains("certificate verify")
        || lower.contains("certificate verification")
        || lower.contains("cert verify")
        || lower.contains("ssl verify")
    {
        "x509_certificate_verify_failed"
    } else if lower.contains("ssl") || lower.contains("tls") || lower.contains("handshake") {
        "openssl_error_unknown"
    } else {
        "none"
    }
}

/// OpenSSL 的“找不到签发者/系统根”文本表示 CA 信任源不可用，而不是
/// 服务端证书本身过期或主机名不匹配。两者都要保留真实原因，但不能混报。
fn is_ca_trust_error(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("unable to get local issuer")
        || lower.contains("unable to get issuer certificate")
        || lower.contains("unable to verify the first certificate")
        || lower.contains("unknown ca")
        || lower.contains("no ca certificate")
        || lower.contains("ca certificate")
}

/// 明确的证书链/主机名/有效期校验失败文本。
fn is_tls_certificate_error(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("certificate verify")
        || lower.contains("certificate verification")
        || lower.contains("cert verify")
        || lower.contains("ssl verify")
        || lower.contains("self signed")
        || lower.contains("self-signed")
        || lower.contains("hostname mismatch")
        || lower.contains("hostname does not match")
        || lower.contains("does not match certificate")
        || lower.contains("certificate subject name")
        || lower.contains("certificate has expired")
        || lower.contains("certificate is not yet valid")
        || lower.contains("certificate chain")
        || lower.contains("peer certificate cannot be authenticated")
        || lower.contains("certificate revoked")
        || lower.contains("unsupported certificate purpose")
        || lower.contains("invalid certificate")
}

fn classify_http_error(message: &str, code: ErrorCode) -> ProviderError {
    let lower = message.to_ascii_lowercase();
    if lower.contains("401")
        || lower.contains("407")
        || lower.contains("unauthorized")
        || lower.contains("proxy authentication")
        || lower.contains("proxy authorization")
        || lower.contains("authentication")
        || lower.contains("credentials")
        || code == ErrorCode::Auth
    {
        return ProviderError::AuthFailed {
            reason: message.to_string(),
        };
    }
    if lower.contains("403") || lower.contains("forbidden") {
        return ProviderError::PermissionDenied {
            reason: message.to_string(),
        };
    }
    if lower.contains("404") || lower.contains("not found") {
        return ProviderError::NotFound {
            path: message.to_string(),
        };
    }
    if lower.contains("500")
        || lower.contains("502")
        || lower.contains("503")
        || lower.contains("504")
        || lower.contains("internal server error")
        || lower.contains("bad gateway")
        || lower.contains("service unavailable")
        || lower.contains("gateway timeout")
    {
        return ProviderError::TemporaryUnavailable {
            reason: message.to_string(),
        };
    }
    ProviderError::Other {
        reason: message.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    struct TreeFixture {
        cache: Repository,
        config: GitHttpsRuntimeConfig,
        accounting: Arc<ProviderNetworkAccounting>,
        origin_path: PathBuf,
        snapshot: Snapshot,
        // Keep the temporary directory alive for the whole test; fields drop
        // top-to-bottom, so the repositories above close before the directory.
        _dir: tempfile::TempDir,
    }

    /// 本地 bare origin + 带多层目录的 base commit + fetch 出的惰性快照。
    fn tree_fixture() -> TreeFixture {
        let dir = tempfile::tempdir().unwrap();
        let origin_path = dir.path().join("origin.git");
        Repository::init_bare(&origin_path).unwrap();

        let seed_dir = dir.path().join("seed");
        let seed = Repository::init(&seed_dir).unwrap();
        let base_paths = [
            ("a/b/unchanged.txt", b"keep".as_slice()),
            ("a/old.txt", b"old".as_slice()),
            ("z/untouched/deep.txt", b"deep".as_slice()),
        ];
        for (path, content) in base_paths {
            let full = seed_dir.join(path);
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(full, content).unwrap();
        }
        let mut index = seed.index().unwrap();
        for (path, _) in base_paths {
            index.add_path(Path::new(path)).unwrap();
        }
        index.write().unwrap();
        let tree_oid = index.write_tree().unwrap();
        let tree = seed.find_tree(tree_oid).unwrap();
        let signature = Signature::now(AUTHOR_NAME, AUTHOR_EMAIL).unwrap();
        seed.commit(
            Some("refs/heads/main"),
            &signature,
            &signature,
            "base",
            &tree,
            &[],
        )
        .unwrap();

        let mut remote = seed
            .remote("origin", origin_path.to_str().unwrap())
            .unwrap();
        remote
            .push(&["refs/heads/main:refs/heads/main"], None)
            .unwrap();
        drop(remote);
        drop(tree);
        drop(index);
        drop(seed);

        let cache_dir = dir.path().join("cache");
        let cache = open_or_init(&cache_dir).unwrap();
        let config = GitHttpsRuntimeConfig {
            remote_url: origin_path.to_string_lossy().into_owned(),
            branch: "main".to_string(),
            username: String::new(),
            secret: None,
        };
        let accounting = Arc::new(ProviderNetworkAccounting::new("git_https_test"));
        let snapshot = fetch_snapshot(&cache, &config, &accounting).unwrap();

        TreeFixture {
            cache,
            config,
            accounting,
            origin_path,
            snapshot,
            _dir: dir,
        }
    }

    fn entry_oid_at(repo: &Repository, tree_oid: Oid, path: &str) -> Oid {
        let mut tree = repo.find_tree(tree_oid).unwrap();
        let mut parts = path.split('/').peekable();
        while let Some(part) = parts.next() {
            let (entry_oid, has_more) = {
                let entry = tree.get_name(part).unwrap();
                (entry.id(), parts.peek().is_some())
            };
            if has_more {
                tree = repo.find_tree(entry_oid).unwrap();
            } else {
                return entry_oid;
            }
        }
        tree.id()
    }

    #[test]
    fn batch_rebuilds_affected_subtrees_and_reuses_untouched_ones() {
        let fixture = tree_fixture();
        let TreeFixture {
            cache,
            config,
            accounting,
            origin_path,
            snapshot,
            _dir,
        } = fixture;

        // fetch 只记录 head/tree，不展开整棵树。
        assert!(snapshot.tree.is_some());
        assert!(snapshot.blob_map().is_empty());

        let base_head = snapshot.head.unwrap();
        let base_tree = snapshot.tree.unwrap();
        let base_unchanged = snapshot
            .blob_oid(&cache, "a/b/unchanged.txt")
            .unwrap()
            .unwrap();
        let base_deep = snapshot
            .blob_oid(&cache, "z/untouched/deep.txt")
            .unwrap()
            .unwrap();

        let new_content = b"new".as_slice();
        let new_oid = Oid::hash_object(ObjectType::Blob, new_content).unwrap();
        let mutations = vec![
            BatchMutation::Put {
                path: "a/new/file.txt".to_string(),
                content: new_content.to_vec(),
            },
            BatchMutation::Delete {
                path: "a/old.txt".to_string(),
            },
            BatchMutation::ReuseVersion {
                path: "a/b/unchanged.txt".to_string(),
                version: RemoteVersion::new(base_unchanged.to_string()),
            },
        ];

        let (commit_oid, committed) = commit_and_push(
            &cache,
            &config,
            &snapshot,
            &mutations,
            "test batch",
            &accounting,
        )
        .unwrap();

        assert_ne!(commit_oid, base_head);
        assert_eq!(committed.head, Some(commit_oid));
        let new_tree = committed.tree.unwrap();
        assert_ne!(new_tree, base_tree);

        // 未受影响子树 OID 与 base 完全相同；本轮重建过的 a/b 因内容未变
        // 也复用同一 OID。
        assert_eq!(
            entry_oid_at(&cache, new_tree, "z"),
            entry_oid_at(&cache, base_tree, "z")
        );
        assert_eq!(
            entry_oid_at(&cache, new_tree, "a/b"),
            entry_oid_at(&cache, base_tree, "a/b")
        );
        assert_eq!(entry_oid_at(&cache, new_tree, "a/new/file.txt"), new_oid);

        let listed: Vec<String> = committed
            .list_blobs(&cache)
            .unwrap()
            .into_iter()
            .map(|(path, _)| path)
            .collect();
        assert_eq!(
            listed,
            vec![
                "a/b/unchanged.txt".to_string(),
                "a/new/file.txt".to_string(),
                "z/untouched/deep.txt".to_string(),
            ]
        );

        assert_eq!(
            committed.blob_oid(&cache, "a/new/file.txt").unwrap(),
            Some(new_oid)
        );
        assert_eq!(
            committed.blob_oid(&cache, "a/b/unchanged.txt").unwrap(),
            Some(base_unchanged)
        );
        assert_eq!(
            committed.blob_oid(&cache, "z/untouched/deep.txt").unwrap(),
            Some(base_deep)
        );
        assert_eq!(committed.blob_oid(&cache, "a/old.txt").unwrap(), None);
        assert_eq!(
            read_blob(&cache, &committed, "a/new/file.txt").unwrap(),
            Some(new_content.to_vec())
        );

        let origin = Repository::open(&origin_path).unwrap();
        assert_eq!(origin.refname_to_id("refs/heads/main").unwrap(), commit_oid);
        drop(origin);

        // 没有实际变更的 batch 不创建 commit、不 push。
        let (noop_commit, noop_snapshot) =
            commit_and_push(&cache, &config, &committed, &[], "noop", &accounting).unwrap();
        assert_eq!(noop_commit, commit_oid);
        assert_eq!(noop_snapshot.head, Some(commit_oid));
        assert_eq!(noop_snapshot.tree, Some(new_tree));

        // 删除不存在路径保持幂等，也不会产生新 commit。
        let (missing_commit, _) = commit_and_push(
            &cache,
            &config,
            &committed,
            &[BatchMutation::Delete {
                path: "a/missing.txt".to_string(),
            }],
            "noop delete",
            &accounting,
        )
        .unwrap();
        assert_eq!(missing_commit, commit_oid);

        let origin = Repository::open(&origin_path).unwrap();
        assert_eq!(origin.refname_to_id("refs/heads/main").unwrap(), commit_oid);
    }

    #[test]
    fn lazy_preconditions_resolve_paths_from_base_tree() {
        let fixture = tree_fixture();
        let TreeFixture {
            cache,
            snapshot,
            _dir,
            ..
        } = fixture;

        let unchanged = snapshot
            .blob_oid(&cache, "a/b/unchanged.txt")
            .unwrap()
            .unwrap();
        let old = snapshot.blob_oid(&cache, "a/old.txt").unwrap().unwrap();

        assert!(check_write_precondition(
            &cache,
            &snapshot,
            "a/b/unchanged.txt",
            &WritePrecondition::IfMatch(RemoteVersion::new(unchanged.to_string())),
        )
        .is_ok());
        assert!(check_write_precondition(
            &cache,
            &snapshot,
            "a/old.txt",
            &WritePrecondition::CreateNew,
        )
        .is_err());
        assert!(matches!(
            check_delete_precondition(
                &cache,
                &snapshot,
                "a/old.txt",
                &DeletePrecondition::IfMatch(RemoteVersion::new(old.to_string())),
            ),
            Ok(())
        ));
        assert!(check_object_precondition(
            &cache,
            &snapshot,
            "z/untouched/deep.txt",
            Some(&RemoteVersion::new(
                snapshot
                    .blob_oid(&cache, "z/untouched/deep.txt")
                    .unwrap()
                    .unwrap()
                    .to_string(),
            )),
        )
        .is_ok());

        let expected = vec![RemoteEntry {
            path: "deep.txt".to_string(),
            version: RemoteVersion::new(
                snapshot
                    .blob_oid(&cache, "z/untouched/deep.txt")
                    .unwrap()
                    .unwrap()
                    .to_string(),
            ),
        }];
        assert!(check_prefix_precondition(&cache, &snapshot, "z/untouched", &expected).is_ok());
        assert!(check_prefix_precondition(&cache, &snapshot, "z/untouched", &[]).is_err());
    }

    #[test]
    fn interrupted_ssl_transport_maps_to_network() {
        let error = classify_git_error(
            "fetch",
            "SSL error: syscall failure: Interrupted system call",
            ErrorCode::GenericError,
            ErrorClass::Ssl,
        );

        assert!(matches!(error, ProviderError::Network { .. }));
        assert_eq!(
            message_kind("SSL error: syscall failure: Interrupted system call"),
            "transport_interrupted"
        );
    }

    #[test]
    fn interrupted_ssl_transport_with_os_class_maps_to_network() {
        let raw_message = "SSL error: syscall failure: Interrupted system call";
        let error = classify_git_error(
            "fetch",
            raw_message,
            ErrorCode::GenericError,
            ErrorClass::Os,
        );

        assert!(matches!(error, ProviderError::Network { .. }));
        assert_eq!(message_kind(raw_message), "transport_interrupted");
    }

    #[test]
    fn interrupted_local_operation_is_not_network() {
        let error = classify_git_error(
            "read_blob",
            "Interrupted system call",
            ErrorCode::GenericError,
            ErrorClass::Ssl,
        );

        assert!(matches!(error, ProviderError::Other { .. }));
    }

    #[test]
    fn interrupted_local_os_error_is_not_network() {
        let error = classify_git_error(
            "read_blob",
            "Interrupted system call",
            ErrorCode::GenericError,
            ErrorClass::Os,
        );

        assert!(matches!(error, ProviderError::Other { .. }));
    }

    #[test]
    fn os_interruption_without_ssl_syscall_semantics_is_not_network() {
        let error = classify_git_error(
            "fetch",
            "Interrupted system call",
            ErrorCode::GenericError,
            ErrorClass::Os,
        );

        assert!(matches!(error, ProviderError::Other { .. }));
    }

    #[test]
    fn certificate_error_wins_over_interrupted_transport() {
        let error = classify_git_error(
            "fetch",
            "certificate verify failed: Interrupted system call",
            ErrorCode::GenericError,
            ErrorClass::Ssl,
        );

        assert!(matches!(error, ProviderError::TlsCertificateInvalid { .. }));
    }
}
