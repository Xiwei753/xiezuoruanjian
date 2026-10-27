//! Git Smart HTTP 仓库缓存与提交实现。
//!
//! 每轮同步使用一个独立 bare 仓库缓存：fetch 远端分支、按 blob OID 读取对象，
//! `commit_batch` 在本地构造完整 tree + commit 后执行普通（非强制）push。
//! 缓存与用户正文目录分离；后续同步只下载新增对象。
//!
//! 错误优先识别 TLS 证书校验失败，再按 libgit2 `ErrorCode` 分类、用
//! `ErrorClass` 兜底：只有明确的 DNS/连接拒绝/超时/重置/EOF 等传输不可达
//! 才可触发 GitHub API 后备；证书验证失败、认证、权限和 non-fast-forward
//! 保留准确错误，不会被 `Http`/`Ssl` 或泛型 class 中的握手关键词误报为网络。

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
    BatchMutation, DeletePrecondition, RemoteEntry, RemoteVersion, WritePrecondition,
};
use crate::sync::provider::ProviderNetworkAccounting;

const REMOTE_NAME: &str = "origin";
const AUTHOR_NAME: &str = "Sujian";
const AUTHOR_EMAIL: &str = "sync@sujian.invalid";

/// 远端分支快照：commit + 全仓 path→blob OID。
#[derive(Debug, Clone, Default)]
pub(crate) struct Snapshot {
    pub head: Option<Oid>,
    pub blobs: HashMap<String, Oid>,
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
        Err(open_error) if open_error.code() != ErrorCode::NotFound => {
            Err(logged_provider_error(
                "open_or_init",
                ProviderError::LocalGitCache {
                    reason: format!("open git cache failed: {}", open_error.message()),
                },
            ))
        }
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
        Err(err) if err.code() == ErrorCode::NotFound => repo
            .remote(REMOTE_NAME, &config.remote_url)
            .map_err(|err| {
                logged_provider_error(
                    "remote",
                    ProviderError::RemoteConfig {
                        reason: format!("configure git remote failed: {}", err.message()),
                    },
                )
            }),
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
    let tree = commit
        .tree()
        .map_err(|err| map_git_error(err, "read_fetched_tree"))?;

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
                    .map_err(|err| map_git_error(err, "read_tree_entry"))?;
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
    let mut index = Index::new().map_err(|err| map_git_error(err, "create_index"))?;
    if let Some(head) = snapshot.head {
        let commit = repo
            .find_commit(head)
            .map_err(|err| map_git_error(err, "find_base_commit"))?;
        let tree = commit
            .tree()
            .map_err(|err| map_git_error(err, "read_base_tree"))?;
        index
            .read_tree(&tree)
            .map_err(|err| map_git_error(err, "load_base_tree"))?;
    }

    let mut written_blobs = HashMap::new();
    for mutation in mutations {
        match mutation {
            BatchMutation::Put { path, content } => {
                let oid = repo
                    .blob(content)
                    .map_err(|err| map_git_error(err, "write_blob"))?;
                index
                    .add(&index_entry(path, oid, content.len())?)
                    .map_err(|err| map_git_error(err, "add_blob"))?;
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
                    .map_err(|err| map_git_error(err, "reuse_blob"))?;
            }
            BatchMutation::Delete { path } => {
                // 删除不存在的路径保持幂等，与 HTTP DELETE 语义一致。
                let _ = index.remove_path(Path::new(path));
            }
            BatchMutation::DeleteIfMatch { path, version } => {
                check_delete_precondition(
                    snapshot,
                    path,
                    &DeletePrecondition::IfMatch(version.clone()),
                )?;
                let _ = index.remove_path(Path::new(path));
            }
            BatchMutation::CheckObject {
                path,
                expected_version,
            } => check_object_precondition(snapshot, path, expected_version.as_ref())?,
            BatchMutation::CheckPrefix {
                prefix,
                expected_entries,
            } => check_prefix_precondition(snapshot, prefix, expected_entries)?,
        }
    }
    Ok((index, written_blobs))
}

/// 从本地已创建的 commit 重建快照，不访问网络。
///
/// 只用于把本 provider 自己成功 push 的提交推进为下一轮基线；远端是否变化
/// 仍由每次提交前的 fetch + HEAD 比较决定。
pub(crate) fn snapshot_from_commit(
    repo: &Repository,
    commit_oid: Oid,
) -> Result<Snapshot, ProviderError> {
    let commit = repo
        .find_commit(commit_oid)
        .map_err(|err| map_git_error(err, "find_committed_commit"))?;
    let tree = commit
        .tree()
        .map_err(|err| map_git_error(err, "read_committed_tree"))?;
    let mut blobs = HashMap::new();
    flatten_tree(repo, &tree, "", &mut blobs)?;
    Ok(Snapshot {
        head: Some(commit_oid),
        blobs,
    })
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
    let (mut index, _) = build_index(repo, snapshot, mutations)?;
    let tree_oid = index
        .write_tree_to(repo)
        .map_err(|err| map_git_error(err, "write_tree"))?;
    let tree = repo
        .find_tree(tree_oid)
        .map_err(|err| map_git_error(err, "find_written_tree"))?;
    let signature = Signature::now(AUTHOR_NAME, AUTHOR_EMAIL)
        .map_err(|err| map_git_error(err, "create_signature"))?;

    let parent = snapshot
        .head
        .map(|oid| repo.find_commit(oid))
        .transpose()
        .map_err(|err| map_git_error(err, "load_parent_commit"))?;
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
    let committed_snapshot = snapshot_from_commit(repo, commit_oid)?;
    Ok((commit_oid, committed_snapshot))
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

pub(crate) fn check_object_precondition(
    snapshot: &Snapshot,
    path: &str,
    expected_version: Option<&RemoteVersion>,
) -> Result<(), ProviderError> {
    let current = blob_version(snapshot, path);
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
    snapshot: &Snapshot,
    prefix: &str,
    expected_entries: &[RemoteEntry],
) -> Result<(), ProviderError> {
    let prefix_with_slash = format!("{prefix}/");
    let mut current_entries: Vec<RemoteEntry> = snapshot
        .blobs
        .iter()
        .filter_map(|(path, oid)| {
            path.strip_prefix(&prefix_with_slash)
                .map(|relative_path| RemoteEntry {
                    path: relative_path.to_string(),
                    version: RemoteVersion::new(oid.to_string()),
                })
        })
        .collect();
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
    // 只记录错误码/类别/稳定原因码和消息类型，不落 URL、Token、文件路径或原始报文。
    log::warn!(
        "sync.git_error: operation={} git_error_code={:?} git_error_class={:?} reason_code={} message_kind={}",
        operation,
        code,
        class,
        mapped.stable_code(),
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
    // class。此时在一个已经出错的会话上原样重放没有意义，也不能落进
    // `Other` 静默收尾；识别为 `Network`，让整轮同步重新建立 Provider 并
    // 重新读取远端。只接受明确的 `Interrupted system call`/`EINTR` 文本，
    // 且限定在网络操作，避免把证书、认证或本地 Git 读写中断误报为网络故障。
    if is_interruptible_network_operation(operation)
        && is_transport_interrupted(raw_message)
        && matches!(class, ErrorClass::Net | ErrorClass::Ssl | ErrorClass::Http)
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
