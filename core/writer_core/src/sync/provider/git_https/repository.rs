//! Git Smart HTTP 仓库缓存与提交实现。
//!
//! 每轮同步使用一个独立 bare 仓库缓存：fetch 远端分支、按 blob OID 读取对象，
//! `commit_batch` 在本地构造完整 tree + commit 后执行普通（非强制）push。
//! 缓存与用户正文目录分离；后续同步只下载新增对象。
//!
//! 所有网络错误按 libgit2 error class 分类：只有 `Net` 被视为“传输不可达”，
//! 认证、权限、TLS 校验和 non-fast-forward 不会触发 GitHub API 后备。

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use git2::{
    AutotagOption, Commit, Direction, ErrorClass, ErrorCode, FetchOptions, Index, IndexTime,
    ObjectType, Oid, PushOptions, Repository, Signature, Tree,
};

use super::config::GitHttpsRuntimeConfig;
use super::credentials;
use crate::sync::provider::error::ProviderError;
use crate::sync::provider::model::{
    BatchMutation, DeletePrecondition, RemoteVersion, WritePrecondition,
};

const REMOTE_NAME: &str = "origin";
const AUTHOR_NAME: &str = "Sujian";
const AUTHOR_EMAIL: &str = "sync@sujian.invalid";

/// 远端分支快照：commit + 全仓 path→blob OID。
#[derive(Debug, Clone, Default)]
pub(crate) struct Snapshot {
    pub head: Option<Oid>,
    pub blobs: HashMap<String, Oid>,
}

/// 打开或初始化 bare 仓库缓存。
pub(crate) fn open_or_init(cache_dir: &Path) -> Result<Repository, ProviderError> {
    crate::storage::git_runtime::ensure_initialized().map_err(|err| ProviderError::Other {
        reason: format!("git runtime init failed: {err}"),
    })?;
    credentials::configure_timeouts();

    if let Ok(repo) = Repository::open(cache_dir) {
        return Ok(repo);
    }
    std::fs::create_dir_all(cache_dir).map_err(|err| ProviderError::Other {
        reason: format!("create git cache dir failed: {err}"),
    })?;
    Repository::init_bare(cache_dir).map_err(|err| map_git_error(err, "init git cache"))
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
                return Err(ProviderError::Other {
                    reason:
                        "cached git repository remote URL does not match the current configuration"
                            .to_string(),
                });
            }
            Ok(remote)
        }
        Err(err) if err.code() == ErrorCode::NotFound => repo
            .remote(REMOTE_NAME, &config.remote_url)
            .map_err(|err| map_git_error(err, "configure remote")),
        Err(err) => Err(map_git_error(err, "open git remote")),
    }
}

/// 只连接并读取 refs，不下载对象。用于路由可达性探测与诊断。
pub(crate) fn probe(cache_dir: &Path, config: &GitHttpsRuntimeConfig) -> Result<(), ProviderError> {
    let repo = open_or_init(cache_dir)?;
    let mut remote = remote(&repo, config)?;
    let callbacks = credentials::callbacks(&config.username, config.secret.as_deref());
    let connection = remote
        .connect_auth(
            Direction::Fetch,
            Some(callbacks),
            Some(credentials::proxy_options()),
        )
        .map_err(|err| map_git_error(err, "connect to git remote"))?;
    connection
        .list()
        .map_err(|err| map_git_error(err, "list remote refs"))?;
    Ok(())
}

/// fetch 指定分支并构建快照。分支不存在视为空仓库（允许首次 push）。
pub(crate) fn fetch_snapshot(
    repo: &Repository,
    config: &GitHttpsRuntimeConfig,
) -> Result<Snapshot, ProviderError> {
    let mut remote = remote(repo, config)?;
    let branch_ref = format!("refs/heads/{}", config.branch);

    // 先 connect/list：既验证认证与可达性，也准确判断分支是否已存在。
    let callbacks = credentials::callbacks(&config.username, config.secret.as_deref());
    let connection = remote
        .connect_auth(
            Direction::Fetch,
            Some(callbacks),
            Some(credentials::proxy_options()),
        )
        .map_err(|err| map_git_error(err, "connect to git remote"))?;
    let heads = connection
        .list()
        .map_err(|err| map_git_error(err, "list remote refs"))?;
    let branch_exists = heads.iter().any(|head| head.name() == branch_ref);
    drop(connection);

    if !branch_exists {
        return Ok(Snapshot::default());
    }

    let refspec = format!("+{branch_ref}:refs/remotes/{REMOTE_NAME}/{}", config.branch);
    let mut fetch_options = FetchOptions::new();
    fetch_options.remote_callbacks(credentials::callbacks(
        &config.username,
        config.secret.as_deref(),
    ));
    fetch_options.proxy_options(credentials::proxy_options());
    fetch_options.download_tags(AutotagOption::None);
    remote
        .fetch(
            &[refspec],
            Some(&mut fetch_options),
            Some("sujian sync fetch"),
        )
        .map_err(|err| map_git_error(err, "fetch git branch"))?;

    let remote_ref = format!("refs/remotes/{REMOTE_NAME}/{}", config.branch);
    let head = repo
        .refname_to_id(&remote_ref)
        .map_err(|err| map_git_error(err, "resolve fetched branch"))?;
    let commit = repo
        .find_commit(head)
        .map_err(|err| map_git_error(err, "find fetched commit"))?;
    let tree = commit
        .tree()
        .map_err(|err| map_git_error(err, "read fetched tree"))?;

    let mut blobs = HashMap::new();
    flatten_tree(repo, &tree, "", &mut blobs)?;

    Ok(Snapshot {
        head: Some(head),
        blobs,
    })
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
                    .map_err(|err| map_git_error(err, "read tree entry"))?;
                flatten_tree(repo, &child, &path, out)?;
            }
            _ => {}
        }
    }
    Ok(())
}

fn index_entry(path: &str, oid: Oid, size: usize) -> Result<git2::IndexEntry, ProviderError> {
    if path.is_empty()
        || path.starts_with('/')
        || path.ends_with('/')
        || path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(ProviderError::Other {
            reason: format!("invalid repository path: {path}"),
        });
    }
    let path_bytes = path.as_bytes().to_vec();
    if path_bytes.len() >= 0x0fff {
        return Err(ProviderError::Other {
            reason: format!("repository path is too long: {path}"),
        });
    }
    Ok(git2::IndexEntry {
        ctime: IndexTime::new(0, 0),
        mtime: IndexTime::new(0, 0),
        dev: 0,
        ino: 0,
        mode: 0o100_644,
        uid: 0,
        gid: 0,
        file_size: u32::try_from(size).unwrap_or(u32::MAX),
        id: oid,
        flags: u16::try_from(path_bytes.len()).unwrap_or(0),
        flags_extended: 0,
        path: path_bytes,
    })
}

fn build_index(
    repo: &Repository,
    snapshot: &Snapshot,
    mutations: &[BatchMutation],
) -> Result<(Index, HashMap<String, Oid>), ProviderError> {
    let mut index = Index::new().map_err(|err| map_git_error(err, "create in-memory index"))?;
    if let Some(head) = snapshot.head {
        let commit = repo
            .find_commit(head)
            .map_err(|err| map_git_error(err, "find base commit"))?;
        let tree = commit
            .tree()
            .map_err(|err| map_git_error(err, "read base tree"))?;
        index
            .read_tree(&tree)
            .map_err(|err| map_git_error(err, "load base tree into index"))?;
    }

    let mut written_blobs = HashMap::new();
    for mutation in mutations {
        match mutation {
            BatchMutation::Put { path, content } => {
                let oid = repo
                    .blob(content)
                    .map_err(|err| map_git_error(err, "write blob"))?;
                index
                    .add(&index_entry(path, oid, content.len())?)
                    .map_err(|err| map_git_error(err, "add blob to index"))?;
                written_blobs.insert(path.clone(), oid);
            }
            BatchMutation::ReuseVersion { path, version } => {
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
                let size = repo
                    .find_blob(oid)
                    .map(|blob| blob.content().len())
                    .unwrap_or(0);
                index
                    .add(&index_entry(path, oid, size)?)
                    .map_err(|err| map_git_error(err, "reuse blob in index"))?;
            }
            BatchMutation::Delete { path } => {
                // 删除不存在的路径保持幂等，与 HTTP DELETE 语义一致。
                let _ = index.remove_path(Path::new(path));
            }
        }
    }
    Ok((index, written_blobs))
}

/// 在当前快照基础上提交一个 commit 并 push。返回新 commit OID。
pub(crate) fn commit_and_push(
    repo: &Repository,
    config: &GitHttpsRuntimeConfig,
    snapshot: &Snapshot,
    mutations: &[BatchMutation],
    message: &str,
) -> Result<Oid, ProviderError> {
    let (mut index, _) = build_index(repo, snapshot, mutations)?;
    let tree_oid = index
        .write_tree_to(repo)
        .map_err(|err| map_git_error(err, "write tree"))?;
    let tree = repo
        .find_tree(tree_oid)
        .map_err(|err| map_git_error(err, "find written tree"))?;
    let signature = Signature::now(AUTHOR_NAME, AUTHOR_EMAIL)
        .map_err(|err| map_git_error(err, "create commit signature"))?;

    let parent = snapshot
        .head
        .map(|oid| repo.find_commit(oid))
        .transpose()
        .map_err(|err| map_git_error(err, "load parent commit"))?;
    let parents: Vec<&Commit<'_>> = parent.iter().collect();
    let commit_oid = repo
        .commit(
            None,
            &signature,
            &signature,
            message,
            &tree,
            parents.as_slice(),
        )
        .map_err(|err| map_git_error(err, "create commit"))?;

    let local_ref = format!("refs/heads/{}", config.branch);
    if !git2::Reference::is_valid_name(&local_ref) {
        return Err(ProviderError::Other {
            reason: format!("invalid branch name: {}", config.branch),
        });
    }
    repo.reference(&local_ref, commit_oid, true, "sujian sync commit")
        .map_err(|err| map_git_error(err, "update local branch"))?;

    push_branch(repo, config, &local_ref, &commit_oid)?;
    Ok(commit_oid)
}

fn push_branch(
    repo: &Repository,
    config: &GitHttpsRuntimeConfig,
    local_ref: &str,
    commit_oid: &Oid,
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

    let mut push_options = PushOptions::new();
    push_options.remote_callbacks(callbacks);
    push_options.proxy_options(credentials::proxy_options());
    let refspec = format!("{local_ref}:refs/heads/{}", config.branch);
    let push_result = remote.push(&[refspec], Some(&mut push_options));

    if let Some(reason) = rejection.lock().ok().and_then(|mut slot| slot.take()) {
        return Err(classify_push_rejection(&reason));
    }
    if let Err(err) = push_result {
        let mapped = map_git_error(err, "push git branch");
        if matches!(mapped, ProviderError::Other { .. }) {
            return Err(ProviderError::PreconditionFailed {
                path: commit_oid.to_string(),
                reason: "remote rejected push (non-fast-forward or protected branch)".to_string(),
            });
        }
        return Err(mapped);
    }
    Ok(())
}

fn classify_push_rejection(reason: &str) -> ProviderError {
    let lower = reason.to_ascii_lowercase();
    if lower.contains("protected") || lower.contains("not allowed") || lower.contains("permission")
    {
        return ProviderError::PermissionDenied {
            reason: reason.to_string(),
        };
    }
    ProviderError::PreconditionFailed {
        path: "refs/heads".to_string(),
        reason: reason.to_string(),
    }
}

/// 校验写前置条件；调用方已持有最新快照。
pub(crate) fn check_write_precondition(
    snapshot: &Snapshot,
    path: &str,
    precondition: &WritePrecondition,
) -> Result<(), ProviderError> {
    match precondition {
        WritePrecondition::IfMatch(expected) => match snapshot.blobs.get(path) {
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
            if snapshot.blobs.contains_key(path) {
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
    snapshot: &Snapshot,
    path: &str,
    precondition: &DeletePrecondition,
) -> Result<(), ProviderError> {
    match precondition {
        DeletePrecondition::IfMatch(expected) => match snapshot.blobs.get(path) {
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

pub(crate) fn blob_version(snapshot: &Snapshot, path: &str) -> Option<RemoteVersion> {
    snapshot
        .blobs
        .get(path)
        .map(|oid| RemoteVersion(oid.to_string()))
}

pub(crate) fn read_blob(
    repo: &Repository,
    snapshot: &Snapshot,
    path: &str,
) -> Result<Option<Vec<u8>>, ProviderError> {
    let Some(oid) = snapshot.blobs.get(path) else {
        return Ok(None);
    };
    let blob = repo
        .find_blob(*oid)
        .map_err(|err| map_git_error(err, "read blob"))?;
    Ok(Some(blob.content().to_vec()))
}

/// 把 libgit2 错误映射到 ProviderError。
///
/// 只有 `ErrorClass::Net` 映射为 `Network`（自动 API 后备的唯一触发条件）；
/// TLS、认证、权限、HTTP 状态错误保持独立语义。
pub(crate) fn map_git_error(err: git2::Error, context: &str) -> ProviderError {
    let message = format!("{context}: {}", err.message());
    match err.class() {
        ErrorClass::Net => ProviderError::Network { reason: message },
        ErrorClass::Ssl => ProviderError::Other {
            reason: format!("TLS certificate verification failed: {message}"),
        },
        ErrorClass::Http => classify_http_error(&message, err.code()),
        ErrorClass::Reference if err.code() == ErrorCode::NotFound => ProviderError::NotFound {
            path: context.to_string(),
        },
        ErrorClass::Reference if err.code() == ErrorCode::NotFastForward => {
            ProviderError::PreconditionFailed {
                path: context.to_string(),
                reason: err.message().to_string(),
            }
        }
        _ => ProviderError::Other { reason: message },
    }
}

fn classify_http_error(message: &str, code: ErrorCode) -> ProviderError {
    let lower = message.to_ascii_lowercase();
    if lower.contains("401")
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
    {
        return ProviderError::TemporaryUnavailable {
            reason: message.to_string(),
        };
    }
    ProviderError::Other {
        reason: message.to_string(),
    }
}
