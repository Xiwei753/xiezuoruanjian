use crate::sync::conflict::build_conflict_summary;
use crate::sync::conflict::collect_index_conflicts;
use crate::sync::service::SyncService;
use crate::sync::types::SyncConflictSummary;
use std::path::Path;

pub enum GitAuth {
    HttpsToken { username: String, token: String },
    SshDeployKey,
}

pub trait GitBackend {
    fn init_repo(&self, local_repo_path: &Path) -> crate::Result<()>;
    fn ensure_remote(&self, local_repo_path: &Path, remote_url: &str) -> crate::Result<()>;
    fn has_repo(&self, local_repo_path: &Path) -> bool;
    fn is_worktree_empty_or_git_only(&self, local_repo_path: &Path) -> crate::Result<bool>;
    fn clone_repo(
        &self,
        remote_url: &str,
        local_repo_path: &Path,
        auth: Option<&GitAuth>,
    ) -> crate::Result<()>;
    fn open_repo(&self, local_repo_path: &Path) -> crate::Result<()>;
    fn pull(
        &self,
        local_repo_path: &Path,
        branch: &str,
        auth: Option<&GitAuth>,
    ) -> crate::Result<()>;
    fn stage_paths(&self, local_repo_path: &Path, paths: &[&str]) -> crate::Result<()>;
    fn commit(&self, local_repo_path: &Path, message: &str) -> crate::Result<Option<String>>;
    fn push(
        &self,
        local_repo_path: &Path,
        branch: &str,
        auth: Option<&GitAuth>,
    ) -> crate::Result<()>;
    fn current_head(&self, local_repo_path: &Path) -> crate::Result<Option<String>>;
    fn status(&self, local_repo_path: &Path) -> crate::Result<Vec<String>>; // Returns changed files
}

pub struct Git2Backend;

impl Git2Backend {
    fn build_callbacks<'a>(
        auth: Option<&'a GitAuth>,
        username_override: Option<&'a str>,
    ) -> git2::RemoteCallbacks<'a> {
        let mut callbacks = git2::RemoteCallbacks::new();
        if let Some(auth) = auth {
            callbacks.credentials(move |_url, username_from_url, _allowed_types| match auth {
                GitAuth::HttpsToken { username, token } => {
                    let user = username_override.or(username_from_url).unwrap_or(username);
                    git2::Cred::userpass_plaintext(user, token)
                }
                GitAuth::SshDeployKey => {
                    Err(git2::Error::from_str("SshDeployKey is NotImplemented"))
                }
            });
        }
        callbacks
    }
}

impl GitBackend for Git2Backend {
    fn init_repo(&self, local_repo_path: &Path) -> crate::Result<()> {
        git2::Repository::init(local_repo_path)
            .map_err(|e| crate::Error::Io(std::io::Error::other(e.to_string())))?;
        Ok(())
    }

    fn ensure_remote(&self, local_repo_path: &Path, remote_url: &str) -> crate::Result<()> {
        let repo = git2::Repository::open(local_repo_path)
            .map_err(|e| crate::Error::Io(std::io::Error::other(e.to_string())))?;
        if repo.find_remote("origin").is_err() {
            repo.remote("origin", remote_url)
                .map_err(|e| crate::Error::Io(std::io::Error::other(e.to_string())))?;
        } else {
            repo.remote_set_url("origin", remote_url)
                .map_err(|e| crate::Error::Io(std::io::Error::other(e.to_string())))?;
        }
        Ok(())
    }

    fn has_repo(&self, local_repo_path: &Path) -> bool {
        git2::Repository::open(local_repo_path).is_ok()
    }

    fn is_worktree_empty_or_git_only(&self, local_repo_path: &Path) -> crate::Result<bool> {
        let entries = std::fs::read_dir(local_repo_path)
            .map_err(|e| crate::Error::Io(std::io::Error::other(e.to_string())))?;
        for entry in entries {
            let entry =
                entry.map_err(|e| crate::Error::Io(std::io::Error::other(e.to_string())))?;
            let name = entry.file_name();
            if name != ".git" {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn clone_repo(
        &self,
        remote_url: &str,
        local_repo_path: &Path,
        auth: Option<&GitAuth>,
    ) -> crate::Result<()> {
        let mut fetch_options = git2::FetchOptions::new();
        let callbacks = Self::build_callbacks(auth, None);
        fetch_options.remote_callbacks(callbacks);

        let mut builder = git2::build::RepoBuilder::new();
        builder.fetch_options(fetch_options);

        builder
            .clone(remote_url, local_repo_path)
            .map_err(|e: git2::Error| crate::Error::Io(std::io::Error::other(e.to_string())))?;
        Ok(())
    }

    fn open_repo(&self, local_repo_path: &Path) -> crate::Result<()> {
        git2::Repository::open(local_repo_path)
            .map_err(|e: git2::Error| crate::Error::Io(std::io::Error::other(e.to_string())))?;
        Ok(())
    }

    fn pull(
        &self,
        local_repo_path: &Path,
        branch: &str,
        auth: Option<&GitAuth>,
    ) -> crate::Result<()> {
        let repo = git2::Repository::open(local_repo_path)
            .map_err(|e: git2::Error| crate::Error::Io(std::io::Error::other(e.to_string())))?;

        // Record transaction anchors
        let is_unborn = repo.head().is_err();
        let original_head_ref_name = repo.head().ok().and_then(|r| r.name().map(String::from));
        let original_head_oid = repo.head().ok().and_then(|r| r.target());
        let original_index_bytes = std::fs::read(repo.path().join("index")).ok();

        let rollback = |repo: &git2::Repository| {
            let _ = repo.cleanup_state();
            if !is_unborn {
                if let Some(oid) = original_head_oid {
                    if let Ok(obj) = repo.find_object(oid, None) {
                        let mut cb = git2::build::CheckoutBuilder::default();
                        cb.force();
                        let _ = repo.reset(&obj, git2::ResetType::Hard, Some(&mut cb));
                    }
                }
            }
            if let Some(ref bytes) = original_index_bytes {
                let _ = std::fs::write(repo.path().join("index"), bytes);
                if let Ok(mut index) = repo.index() {
                    let _ = index.read(true);
                }
            }
            if let Some(ref ref_name) = original_head_ref_name {
                let _ = repo.set_head(ref_name);
            }
        };

        let mut remote = repo
            .find_remote("origin")
            .map_err(|e: git2::Error| crate::Error::Io(std::io::Error::other(e.to_string())))?;

        let mut fetch_options = git2::FetchOptions::new();
        fetch_options.remote_callbacks(Self::build_callbacks(auth, None));

        if let Err(e) = remote.fetch(&[branch], Some(&mut fetch_options), None) {
            rollback(&repo);
            return Err(crate::Error::Io(std::io::Error::other(e.to_string())));
        }

        let fetch_head = repo
            .find_reference("FETCH_HEAD")
            .map_err(|e: git2::Error| {
                rollback(&repo);
                crate::Error::Io(std::io::Error::other(e.to_string()))
            })?;
        let fetch_commit =
            repo.reference_to_annotated_commit(&fetch_head)
                .map_err(|e: git2::Error| {
                    rollback(&repo);
                    crate::Error::Io(std::io::Error::other(e.to_string()))
                })?;

        // Handle unborn local repository
        if repo.head().is_err() {
            let commit_obj = match repo.find_commit(fetch_commit.id()) {
                Ok(c) => c,
                Err(e) => {
                    rollback(&repo);
                    return Err(crate::Error::Io(std::io::Error::other(e.to_string())));
                }
            };
            if let Err(e) = repo.checkout_tree(
                commit_obj.as_object(),
                Some(git2::build::CheckoutBuilder::default().force()),
            ) {
                rollback(&repo);
                return Err(crate::Error::Io(std::io::Error::other(e.to_string())));
            }
            if let Err(e) = repo.branch(branch, &commit_obj, true) {
                rollback(&repo);
                return Err(crate::Error::Io(std::io::Error::other(e.to_string())));
            }
            if let Err(e) = repo.set_head(&format!("refs/heads/{}", branch)) {
                rollback(&repo);
                return Err(crate::Error::Io(std::io::Error::other(e.to_string())));
            }
            return Ok(());
        }

        let analysis = repo
            .merge_analysis(&[&fetch_commit])
            .map_err(|e: git2::Error| {
                rollback(&repo);
                crate::Error::Io(std::io::Error::other(e.to_string()))
            })?;

        // Pre-pull safety check: check for index conflicts and blocking untracked files
        let refname = format!("refs/heads/{}", branch);
        if let Ok(statuses) =
            repo.statuses(Some(git2::StatusOptions::new().include_untracked(true)))
        {
            let mut blocking_files = Vec::new();
            for entry in statuses.iter() {
                if let Some(path) = entry.path() {
                    if SyncService::is_blacklisted_path(path) {
                        continue;
                    }
                    let status = entry.status();
                    if status.is_index_new()
                        || status.is_index_deleted()
                        || status.is_index_modified()
                    {
                        // Index has conflicts or unmerged entries
                        if status.is_conflicted() {
                            rollback(&repo);
                            let conflicts = collect_index_conflicts(&repo);
                            let summary = SyncConflictSummary {
                                status: "conflict".to_string(),
                                local_dirty: true,
                                remote_changed: true,
                                conflicted_files: conflicts,
                                blocked_reason: "本地 Git 暂存区存在未解决的冲突，请先解决冲突。"
                                    .to_string(),
                                safe_next_steps: vec![
                                    "备份当前工作区。".to_string(),
                                    "运行诊断确认网络/认证没问题。".to_string(),
                                    "手动处理冲突后重新同步。".to_string(),
                                ],
                            };
                            let payload = serde_json::to_string(&summary).unwrap_or_default();
                            return Err(crate::Error::Io(std::io::Error::other(format!(
                                "checkout_conflict_payload:{}",
                                payload
                            ))));
                        }
                    }
                    // Check for untracked files that would be overwritten
                    if status.is_wt_new() && SyncService::is_whitelisted_path(path) {
                        blocking_files.push(path.to_string());
                    }
                }
            }
            if !blocking_files.is_empty() {
                rollback(&repo);
                let summary = SyncConflictSummary {
                    status: "conflict".to_string(),
                    local_dirty: true,
                    remote_changed: true,
                    conflicted_files: blocking_files.clone(),
                    blocked_reason: format!(
                        "本地工作区有 {} 个未跟踪文件会阻止远端 checkout。",
                        blocking_files.len()
                    ),
                    safe_next_steps: vec![
                        "备份当前工作区。".to_string(),
                        "运行诊断确认网络/认证没问题。".to_string(),
                        "手动处理冲突后重新同步。".to_string(),
                    ],
                };
                let payload = serde_json::to_string(&summary).unwrap_or_default();
                return Err(crate::Error::Io(std::io::Error::other(format!(
                    "checkout_conflict_payload:{}",
                    payload
                ))));
            }
        }

        if analysis.0.is_up_to_date() {
            // Do nothing
        } else if analysis.0.is_fast_forward() {
            // Fast-forward: checkout target tree FIRST, then update ref/head.
            // This ensures that if checkout fails, HEAD/ref remain unchanged.

            // Step 1: Dry-run checkout to detect conflicts before making any changes
            let conflicted_paths = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
            let cp_clone = conflicted_paths.clone();

            let mut dry_run_builder = git2::build::CheckoutBuilder::default();
            dry_run_builder.notify_on(git2::CheckoutNotificationType::CONFLICT);
            dry_run_builder.notify(move |_, path, _, _, _| {
                if let Some(p) = path {
                    if let Some(s) = p.to_str() {
                        cp_clone.borrow_mut().push(s.to_string());
                    }
                }
                true
            });
            dry_run_builder.dry_run();
            let fetch_tree = match repo.find_commit(fetch_commit.id()).and_then(|c| c.tree()) {
                Ok(t) => t,
                Err(e) => {
                    rollback(&repo);
                    return Err(crate::Error::Io(std::io::Error::other(e.to_string())));
                }
            };
            if let Err(e) = repo.checkout_tree(fetch_tree.as_object(), Some(&mut dry_run_builder)) {
                rollback(&repo);
                let err_msg = e.to_string();
                if err_msg.contains("conflict") || err_msg.contains("Conflict") {
                    let paths = conflicted_paths.borrow().clone();
                    let summary = crate::sync::SyncConflictSummary {
                        status: "conflict".to_string(),
                        local_dirty: true,
                        remote_changed: true,
                        conflicted_files: paths,
                        blocked_reason: "本地未提交的改动与远端更新冲突，Git 无法安全检出。"
                            .to_string(),
                        safe_next_steps: vec![
                            "备份当前工作区。".to_string(),
                            "运行诊断确认网络/认证没问题。".to_string(),
                            "手动处理冲突后重新同步。".to_string(),
                        ],
                    };
                    let payload = serde_json::to_string(&summary).unwrap_or_default();
                    return Err(crate::Error::Io(std::io::Error::other(format!(
                        "checkout_conflict_payload:{}",
                        payload
                    ))));
                }
                return Err(crate::Error::Io(std::io::Error::other(format!(
                    "checkout dry-run failed: {}",
                    err_msg
                ))));
            }

            // Step 2: Actual checkout (safe) - still no ref/head change yet
            let fetch_tree2 = match repo.find_commit(fetch_commit.id()).and_then(|c| c.tree()) {
                Ok(t) => t,
                Err(e) => {
                    rollback(&repo);
                    return Err(crate::Error::Io(std::io::Error::other(e.to_string())));
                }
            };
            if let Err(e) = repo.checkout_tree(
                fetch_tree2.as_object(),
                Some(git2::build::CheckoutBuilder::default().safe()),
            ) {
                rollback(&repo);
                return Err(crate::Error::Io(std::io::Error::other(e.to_string())));
            }

            // Step 3: Only after successful checkout, update ref and head
            let mut reference = match repo.find_reference(&refname) {
                Ok(r) => r,
                Err(e) => {
                    rollback(&repo);
                    return Err(crate::Error::Io(std::io::Error::other(e.to_string())));
                }
            };
            if let Err(e) = reference.set_target(fetch_commit.id(), "Fast-Forward") {
                rollback(&repo);
                return Err(crate::Error::Io(std::io::Error::other(e.to_string())));
            }
            if let Err(e) = repo.set_head(&refname) {
                rollback(&repo);
                return Err(crate::Error::Io(std::io::Error::other(e.to_string())));
            }
        } else if analysis.0.is_normal() {
            let mut merge_opts = git2::MergeOptions::new();
            if let Err(e) = repo.merge(&[&fetch_commit], Some(&mut merge_opts), None) {
                rollback(&repo);
                let err_msg = e.to_string();
                if e.code() == git2::ErrorCode::Conflict
                    || e.class() == git2::ErrorClass::Checkout
                    || err_msg.contains("conflict")
                    || err_msg.contains("Conflict")
                {
                    let summary = build_conflict_summary(
                        &repo,
                        Some(fetch_commit.id()),
                        "本地未提交的改动或冲突阻止了合并操作。",
                    );
                    let payload = serde_json::to_string(&summary).unwrap_or_default();
                    return Err(crate::Error::Io(std::io::Error::other(format!(
                        "checkout_conflict_payload:{}",
                        payload
                    ))));
                }
                return Err(crate::Error::Io(std::io::Error::other(err_msg)));
            }

            let mut index = match repo.index() {
                Ok(i) => i,
                Err(e) => {
                    rollback(&repo);
                    return Err(crate::Error::Io(std::io::Error::other(e.to_string())));
                }
            };

            // Settings semantic merge conflict resolution
            let mut settings_conflict_details = None;
            let mut resolved_settings = false;
            if index.has_conflicts() {
                if let Ok(mut conflicts) = index.conflicts() {
                    let mut settings_conflict = None;
                    for c in conflicts.by_ref().flatten() {
                        let path_opt = c
                            .our
                            .as_ref()
                            .map(|o| String::from_utf8_lossy(&o.path).to_string())
                            .or_else(|| {
                                c.their
                                    .as_ref()
                                    .map(|t| String::from_utf8_lossy(&t.path).to_string())
                            })
                            .or_else(|| {
                                c.ancestor
                                    .as_ref()
                                    .map(|a| String::from_utf8_lossy(&a.path).to_string())
                            });
                        if let Some(p) = path_opt {
                            if p == "app-meta/settings/settings.sync.json" {
                                settings_conflict = Some(c);
                                break;
                            }
                        }
                    }

                    if let Some(c) = settings_conflict {
                        let base_json = c
                            .ancestor
                            .as_ref()
                            .and_then(|entry| repo.find_blob(entry.id).ok())
                            .and_then(|blob| {
                                let s = std::str::from_utf8(blob.content()).ok()?;
                                serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(
                                    s,
                                )
                                .ok()
                            })
                            .unwrap_or_default();

                        let local_json = c
                            .our
                            .as_ref()
                            .and_then(|entry| repo.find_blob(entry.id).ok())
                            .and_then(|blob| {
                                let s = std::str::from_utf8(blob.content()).ok()?;
                                serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(
                                    s,
                                )
                                .ok()
                            })
                            .unwrap_or_default();

                        let remote_json = c
                            .their
                            .as_ref()
                            .and_then(|entry| repo.find_blob(entry.id).ok())
                            .and_then(|blob| {
                                let s = std::str::from_utf8(blob.content()).ok()?;
                                serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(
                                    s,
                                )
                                .ok()
                            })
                            .unwrap_or_default();

                        match SyncService::semantic_merge_json(
                            &base_json,
                            &local_json,
                            &remote_json,
                        ) {
                            Ok(merged_map) => {
                                let merged_value = serde_json::Value::Object(merged_map);
                                let merged_str =
                                    serde_json::to_string_pretty(&merged_value).unwrap_or_default();

                                let settings_path =
                                    local_repo_path.join("app-meta/settings/settings.sync.json");
                                if let Some(parent) = settings_path.parent() {
                                    std::fs::create_dir_all(parent).ok();
                                }
                                let _ = std::fs::write(&settings_path, &merged_str);

                                if let Ok(mut mut_index) = repo.index() {
                                    if mut_index
                                        .add_path(Path::new("app-meta/settings/settings.sync.json"))
                                        .is_ok()
                                    {
                                        let _ = mut_index.write();
                                        resolved_settings = true;
                                    }
                                }
                            }
                            Err(key_conflicts) => {
                                settings_conflict_details = Some(key_conflicts);
                            }
                        }
                    }
                }
            }

            if resolved_settings {
                if let Ok(reloaded) = repo.index() {
                    index = reloaded;
                }
            }

            if let Some(details) = settings_conflict_details {
                rollback(&repo);
                let payload = serde_json::to_string(&details).unwrap_or_default();
                return Err(crate::Error::Io(std::io::Error::other(format!(
                    "settings_conflict_payload:{}",
                    payload
                ))));
            }

            if index.has_conflicts() {
                rollback(&repo);
                // Return an error for conflicts with a special prefix that can be parsed
                return Err(crate::Error::Io(std::io::Error::other(
                    "SyncConflict_Detected".to_string(),
                )));
            } else {
                let oid = match index.write_tree() {
                    Ok(o) => o,
                    Err(e) => {
                        rollback(&repo);
                        return Err(crate::Error::Io(std::io::Error::other(e.to_string())));
                    }
                };
                let signature = match git2::Signature::now("Sync User", "sync@writer.app") {
                    Ok(s) => s,
                    Err(e) => {
                        rollback(&repo);
                        return Err(crate::Error::Io(std::io::Error::other(e.to_string())));
                    }
                };
                let tree = match repo.find_tree(oid) {
                    Ok(t) => t,
                    Err(e) => {
                        rollback(&repo);
                        return Err(crate::Error::Io(std::io::Error::other(e.to_string())));
                    }
                };
                let head_ref = match repo.head() {
                    Ok(r) => r,
                    Err(e) => {
                        rollback(&repo);
                        return Err(crate::Error::Io(std::io::Error::other(e.to_string())));
                    }
                };
                let head_commit = match head_ref.peel_to_commit() {
                    Ok(c) => c,
                    Err(e) => {
                        rollback(&repo);
                        return Err(crate::Error::Io(std::io::Error::other(e.to_string())));
                    }
                };
                let fetch_commit_obj = match repo.find_commit(fetch_commit.id()) {
                    Ok(c) => c,
                    Err(e) => {
                        rollback(&repo);
                        return Err(crate::Error::Io(std::io::Error::other(e.to_string())));
                    }
                };
                if let Err(e) = repo.commit(
                    Some("HEAD"),
                    &signature,
                    &signature,
                    "Merge remote-tracking branch",
                    &tree,
                    &[&head_commit, &fetch_commit_obj],
                ) {
                    rollback(&repo);
                    return Err(crate::Error::Io(std::io::Error::other(e.to_string())));
                }
                if let Err(e) = repo.cleanup_state() {
                    rollback(&repo);
                    return Err(crate::Error::Io(std::io::Error::other(e.to_string())));
                }
            }
        } else {
            rollback(&repo);
            return Err(crate::Error::Io(std::io::Error::other(
                "Unable to pull: remote branch is unrelated or unable to merge",
            )));
        }

        Ok(())
    }

    fn stage_paths(&self, local_repo_path: &Path, paths: &[&str]) -> crate::Result<()> {
        let repo = git2::Repository::open(local_repo_path)
            .map_err(|e: git2::Error| crate::Error::Io(std::io::Error::other(e.to_string())))?;
        let mut index = repo
            .index()
            .map_err(|e: git2::Error| crate::Error::Io(std::io::Error::other(e.to_string())))?;
        for p in paths {
            if SyncService::is_blacklisted_path(p) || !SyncService::is_whitelisted_path(p) {
                continue;
            }
            index
                .add_path(Path::new(p))
                .map_err(|e: git2::Error| crate::Error::Io(std::io::Error::other(e.to_string())))?;
        }
        index
            .write()
            .map_err(|e: git2::Error| crate::Error::Io(std::io::Error::other(e.to_string())))?;
        Ok(())
    }

    fn commit(&self, local_repo_path: &Path, message: &str) -> crate::Result<Option<String>> {
        let repo = git2::Repository::open(local_repo_path)
            .map_err(|e: git2::Error| crate::Error::Io(std::io::Error::other(e.to_string())))?;
        let mut index = repo
            .index()
            .map_err(|e: git2::Error| crate::Error::Io(std::io::Error::other(e.to_string())))?;
        let oid = index
            .write_tree()
            .map_err(|e: git2::Error| crate::Error::Io(std::io::Error::other(e.to_string())))?;
        let signature = git2::Signature::now("Sync User", "sync@writer.app")
            .map_err(|e: git2::Error| crate::Error::Io(std::io::Error::other(e.to_string())))?;

        let tree = repo
            .find_tree(oid)
            .map_err(|e: git2::Error| crate::Error::Io(std::io::Error::other(e.to_string())))?;

        let head_commit = match repo.head() {
            Ok(head) => {
                let target = head.target().ok_or_else(|| {
                    crate::Error::Io(std::io::Error::other("HEAD target not found"))
                })?;
                Some(
                    repo.find_commit(target)
                        .map_err(|e| crate::Error::Io(std::io::Error::other(e.to_string())))?,
                )
            }
            Err(_) => None,
        };

        let parents = if let Some(ref c) = head_commit {
            vec![c]
        } else {
            vec![]
        };

        let mut parent_refs = vec![];
        for p in &parents {
            parent_refs.push(*p);
        }

        let commit_id = repo
            .commit(
                Some("HEAD"),
                &signature,
                &signature,
                message,
                &tree,
                &parent_refs,
            )
            .map_err(|e: git2::Error| crate::Error::Io(std::io::Error::other(e.to_string())))?;

        Ok(Some(commit_id.to_string()))
    }

    fn push(
        &self,
        local_repo_path: &Path,
        branch: &str,
        auth: Option<&GitAuth>,
    ) -> crate::Result<()> {
        let repo = git2::Repository::open(local_repo_path)
            .map_err(|e: git2::Error| crate::Error::Io(std::io::Error::other(e.to_string())))?;

        // 1. Check index conflicts
        if let Ok(index) = repo.index() {
            if index.has_conflicts() {
                return Err(crate::Error::Io(std::io::Error::other(
                    "fatal_error: Cannot push: index has unresolved conflicts.".to_string(),
                )));
            }
        }

        let branch_ref_name = format!("refs/heads/{}", branch);
        let branch_exists = repo.find_reference(&branch_ref_name).is_ok();

        // 2. Check HEAD commit
        let head_ref = repo.head();
        let head_commit = head_ref.as_ref().ok().and_then(|r| r.peel_to_commit().ok());

        match (branch_exists, head_commit) {
            (true, Some(_)) => {
                // Normal case: branch ref exists, HEAD points to a commit.
            }
            (false, Some(commit)) => {
                // Branch ref doesn't exist but HEAD has a commit. Reconstruct it.
                repo.branch(branch, &commit, false).map_err(|e| {
                    crate::Error::Io(std::io::Error::other(format!(
                        "fatal_error: Failed to reconstruct branch ref: {}",
                        e
                    )))
                })?;
                let _ = repo.set_head(&branch_ref_name);
            }
            (_, None) => {
                // HEAD unborn / no commits.
                return Err(crate::Error::Io(std::io::Error::other(
                    "recoverable_error: HEAD is unborn and has no commit.".to_string(),
                )));
            }
        }

        let mut remote = repo
            .find_remote("origin")
            .map_err(|e: git2::Error| crate::Error::Io(std::io::Error::other(e.to_string())))?;

        let mut push_options = git2::PushOptions::new();
        push_options.remote_callbacks(Self::build_callbacks(auth, None));

        let refspec = format!("refs/heads/{}:refs/heads/{}", branch, branch);
        remote
            .push(&[&refspec], Some(&mut push_options))
            .map_err(|e: git2::Error| crate::Error::Io(std::io::Error::other(e.to_string())))?;
        Ok(())
    }

    fn current_head(&self, local_repo_path: &Path) -> crate::Result<Option<String>> {
        let repo = git2::Repository::open(local_repo_path)
            .map_err(|e: git2::Error| crate::Error::Io(std::io::Error::other(e.to_string())))?;
        if let Ok(head) = repo.head() {
            if let Some(target) = head.target() {
                return Ok(Some(target.to_string()));
            }
        }
        Ok(None)
    }

    fn status(&self, local_repo_path: &Path) -> crate::Result<Vec<String>> {
        let repo = git2::Repository::open(local_repo_path)
            .map_err(|e: git2::Error| crate::Error::Io(std::io::Error::other(e.to_string())))?;
        let mut opts = git2::StatusOptions::new();
        opts.include_untracked(true);
        let statuses = repo
            .statuses(Some(&mut opts))
            .map_err(|e: git2::Error| crate::Error::Io(std::io::Error::other(e.to_string())))?;
        let mut res = Vec::new();
        for entry in statuses.iter() {
            if let Some(path) = entry.path() {
                if !SyncService::is_blacklisted_path(path) && SyncService::is_whitelisted_path(path)
                {
                    res.push(path.to_string());
                }
            }
        }
        Ok(res)
    }
}

pub(crate) fn fetch_and_reset_local_repo(
    workspace_path: &Path,
    config: &SyncConfig,
    token: &str,
    new_commit_sha: &str,
) -> crate::Result<()> {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if let Ok(repo) = git2::Repository::open(workspace_path) {
            let mut remote = repo
                .find_remote("origin")
                .or_else(|_| repo.remote("origin", &config.remote_url))
                .map_err(|e| crate::Error::Other(e.to_string()))?;

            let mut fetch_opts = git2::FetchOptions::new();
            if !token.is_empty() {
                let mut callbacks = git2::RemoteCallbacks::new();
                let token_clone = token.to_string();
                callbacks.credentials(move |_url, username_from_url, _allowed_types| {
                    let user = username_from_url.unwrap_or("x-access-token");
                    git2::Cred::userpass_plaintext(user, &token_clone)
                });
                fetch_opts.remote_callbacks(callbacks);
            }

            let refspec = format!(
                "refs/heads/{}:refs/remotes/origin/{}",
                config.branch, config.branch
            );
            remote
                .fetch(&[refspec], Some(&mut fetch_opts), None)
                .map_err(|e| crate::Error::Other(e.to_string()))?;

            let commit_oid = git2::Oid::from_str(new_commit_sha)
                .map_err(|e| crate::Error::Other(e.to_string()))?;
            let commit_obj = repo
                .find_commit(commit_oid)
                .map_err(|e| crate::Error::Other(e.to_string()))?;

            repo.reset(commit_obj.as_object(), git2::ResetType::Mixed, None)
                .map_err(|e| crate::Error::Other(e.to_string()))?;

            let branch_ref_name = format!("refs/heads/{}", config.branch);
            repo.reference(&branch_ref_name, commit_oid, true, "LWW sync update")
                .map_err(|e| crate::Error::Other(e.to_string()))?;

            let _ = repo.set_head(&format!("refs/heads/{}", config.branch));
        }
        Ok(())
    }));

    match result {
        Ok(inner_res) => inner_res,
        Err(panic_err) => {
            let panic_msg = if let Some(s) = panic_err.downcast_ref::<&str>() {
                s.to_string()
            } else if let Some(s) = panic_err.downcast_ref::<String>() {
                s.clone()
            } else {
                "未知 Panic".to_string()
            };
            Err(crate::Error::Other(format!(
                "fetch_and_reset_local_repo panic: {}",
                panic_msg
            )))
        }
    }
}
