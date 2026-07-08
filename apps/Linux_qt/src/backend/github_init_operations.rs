// =============================================================================
// github_init_operations.rs — GitHub 克隆/导入工作区（从 workspace_backend.rs 拆分）
// =============================================================================

use super::*;
use crate::sync_bridge::{
    mask_sync_error, save_sync_configs, sync_error_category, sync_error_category_from_code,
};

impl AppBackend {
    pub(crate) fn execute_github_init(
        &mut self,
        path: QString,
        remote_url: QString,
        branch: QString,
        token: QString,
    ) {
        let path_str = path.to_string();
        let path_obj = std::path::Path::new(&path_str);
        if !path_obj.exists() || !path_obj.is_dir() {
            let state = writer_core::api::SyncOperationStateDto {
                operation_id: String::new(),
                operation_kind: "github_init".to_string(),
                status_code: "error".to_string(),
                phase_key: None,
                summary_key: Some("sync.block.invalid_directory".to_string()),
                summary_args: std::collections::HashMap::new(),
                counts: writer_core::api::SyncOperationCountsDto::default(),
                raw_error: None,
            };
            self.set_error("sync.block.invalid_directory");
            self.current_sync_operation_state = serde_json::to_string(&state).unwrap_or_default();
            self.sync_action_completed();
            return;
        }

        let remote_url_str = remote_url.to_string();
        if remote_url_str.is_empty() {
            let state = writer_core::api::SyncOperationStateDto {
                operation_id: String::new(),
                operation_kind: "github_init".to_string(),
                status_code: "error".to_string(),
                phase_key: None,
                summary_key: Some("sync.block.remote_url_missing".to_string()),
                summary_args: std::collections::HashMap::new(),
                counts: writer_core::api::SyncOperationCountsDto::default(),
                raw_error: None,
            };
            self.set_error("sync.block.remote_url_missing");
            self.current_sync_operation_state = serde_json::to_string(&state).unwrap_or_default();
            self.sync_action_completed();
            return;
        }

        let branch_str = if branch.to_string().is_empty() {
            "main".to_string()
        } else {
            branch.to_string()
        };
        let token_str = token.to_string();

        let op_id = uuid::Uuid::new_v4().to_string();
        self.current_sync_operation_id = op_id.clone();
        self.current_sync_operation_kind = "sync".to_string();

        self.current_sync_status = "syncing".to_string();
        self.sync_status_changed();
        let state = writer_core::api::SyncOperationStateDto {
            operation_id: op_id.clone(),
            operation_kind: "github_init".to_string(),
            status_code: "syncing".to_string(),
            phase_key: Some("sync.phase.github_init".to_string()),
            summary_key: None,
            summary_args: std::collections::HashMap::new(),
            counts: writer_core::api::SyncOperationCountsDto::default(),
            raw_error: None,
        };
        self.current_sync_operation_state = serde_json::to_string(&state).unwrap_or_default();

        let qptr = QPointer::from(&*self);
        let callback = qmetaobject::queued_callback(move |outcome: SyncTaskOutcome| {
            qptr.as_pinned().map(|this| {
                let mut this = this.borrow_mut();
                this.handle_sync_outcome(outcome);
            });
        });

        let op_id_capture = op_id.clone();
        thread::spawn(move || {
            let result = Self::do_github_init(
                &op_id_capture,
                &path_str,
                &remote_url_str,
                &branch_str,
                &token_str,
            );
            callback(result);
        });
    }

    pub(crate) fn do_github_init(
        operation_id: &str,
        path: &str,
        remote_url: &str,
        branch: &str,
        token: &str,
    ) -> SyncTaskOutcome {
        use writer_core::sync::{
            sanitize_remote_url, BackendType, SyncConfig, SyncSecrets,
        };

        let parsed = sanitize_remote_url(remote_url);
        let sanitized_url = parsed.sanitized_url;

        let path_obj = std::path::Path::new(path);

        let has_content = || -> bool {
            if let Ok(mut entries) = std::fs::read_dir(path_obj) {
                entries.next().is_some()
            } else {
                false
            }
        };

        let has_workspace = || -> bool {
            WriterCoreApi::new(path)
                .validate_workspace()
                .unwrap_or(false)
        };

        let is_git_repo = || -> bool { path_obj.join(".git").exists() };

        let effective_token = if token.is_empty() {
            parsed.extracted_token.clone()
        } else {
            Some(token.to_string())
        };

        let config = SyncConfig {
            enabled: true,
            backend_type: BackendType::GithubApi,
            remote_url: sanitized_url.clone(),
            transport: writer_core::sync::SyncTransport::HttpsToken,
            branch: branch.to_string(),
            auto_sync: false,
            sync_interval_seconds: 300,
            username: parsed.extracted_username.clone().unwrap_or_default(),
            android_has_internet_permission: true,
            android_has_access_network_state_permission: true,
        };

        let secrets = SyncSecrets {
            token: effective_token,
            ssh_private_key: None,
        };

        let cfg_ref = &config;
        let sec_ref = &secrets;

        if !has_content() {
            let backend = writer_core::sync::create_sync_backend(&config.backend_type);
            match backend.sync(path_obj, &config, &secrets, true) {
                Ok(result) => {
                    if result.status == writer_core::sync::SyncStatus::Success {
                        let api = WriterCoreApi::new(path);
                        if !api.validate_workspace().unwrap_or(false) {
                            if let Err(e) = api.create_workspace_if_needed() {
                                return SyncTaskOutcome {
                                    operation_id: operation_id.to_string(),
                                    operation_kind: "github_init".to_string(),
                                    sync_status: "error".to_string(),
                                    action_result: serde_json::to_string(&writer_core::api::SyncOperationStateDto {
                                        operation_id: operation_id.to_string(),
                                        operation_kind: "github_init".to_string(),
                                        status_code: "error".to_string(),
                                        phase_key: None,
                                        summary_key: Some("sync.result.clone_success_init_failed".to_string()),
                                        summary_args: [("error".to_string(), mask_sync_error(&e.to_string()))].into_iter().collect(),
                                        counts: writer_core::api::SyncOperationCountsDto::default(),
                                        raw_error: Some(mask_sync_error(&e.to_string())),
                                    }).unwrap_or_default(),
                                };
                            }
                            let push_backend = writer_core::sync::create_sync_backend(
                                &config.backend_type,
                            );
                            let push_result = push_backend.sync(path_obj, &config, &secrets, true);
                            let save_first = match &push_result {
                                Ok(r)
                                    if r.status
                                        != writer_core::sync::SyncStatus::Success =>
                                {
                                    true
                                }
                                Err(_) => true,
                                _ => false,
                            };
                            if save_first {
                                let save_outcome = match save_sync_configs(path, cfg_ref, sec_ref) {
                                    Ok(()) => None,
                                    Err(e) => Some(e),
                                };
                                match push_result {
                                    Ok(push_res) => {
                                        let category = push_res.error_category.clone();
                                        let err = push_res.error.unwrap_or_default();
                                        if let Some(se) = save_outcome {
                                            return SyncTaskOutcome {
                                                operation_id: operation_id.to_string(), operation_kind: "sync".to_string(), sync_status: "error".to_string(),
                                                action_result: serde_json::to_string(&writer_core::api::SyncOperationStateDto { operation_id: operation_id.to_string(), operation_kind: "github_init".to_string(), status_code: "error".to_string(), phase_key: None, summary_key: Some("sync.result.push_failed_save_config_failed".to_string()), summary_args: [("push_error".to_string(), mask_sync_error(&err)), ("save_error".to_string(), se)].into_iter().collect(), counts: writer_core::api::SyncOperationCountsDto::default(), raw_error: Some(mask_sync_error(&err)) }).unwrap_or_default(),
                                            };
                                        }
                                        return SyncTaskOutcome {
                                            operation_id: operation_id.to_string(), operation_kind: "sync".to_string(), sync_status: sync_error_category_from_code(category.as_deref(), &err),
                                            action_result: serde_json::to_string(&writer_core::api::SyncOperationStateDto { operation_id: operation_id.to_string(), operation_kind: "github_init".to_string(), status_code: sync_error_category_from_code(category.as_deref(), &err), phase_key: None, summary_key: Some("sync.result.push_failed".to_string()), summary_args: [("error".to_string(), mask_sync_error(&err))].into_iter().collect(), counts: writer_core::api::SyncOperationCountsDto::default(), raw_error: Some(mask_sync_error(&err)) }).unwrap_or_default(),
                                        };
                                    }
                                    Err(e) => {
                                        if let Some(se) = save_outcome {
                                            return SyncTaskOutcome {
                                                operation_id: operation_id.to_string(), operation_kind: "sync".to_string(), sync_status: "error".to_string(),
                                                action_result: serde_json::to_string(&writer_core::api::SyncOperationStateDto { operation_id: operation_id.to_string(), operation_kind: "github_init".to_string(), status_code: "error".to_string(), phase_key: None, summary_key: Some("sync.result.push_failed_save_config_failed".to_string()), summary_args: [("push_error".to_string(), mask_sync_error(&e.to_string())), ("save_error".to_string(), se)].into_iter().collect(), counts: writer_core::api::SyncOperationCountsDto::default(), raw_error: Some(mask_sync_error(&e.to_string())) }).unwrap_or_default(),
                                            };
                                        }
                                        return SyncTaskOutcome {
                                            operation_id: operation_id.to_string(), operation_kind: "sync".to_string(), sync_status: sync_error_category(&e.to_string()),
                                            action_result: serde_json::to_string(&writer_core::api::SyncOperationStateDto { operation_id: operation_id.to_string(), operation_kind: "github_init".to_string(), status_code: sync_error_category(&e.to_string()), phase_key: None, summary_key: Some("sync.result.push_failed".to_string()), summary_args: [("error".to_string(), mask_sync_error(&e.to_string()))].into_iter().collect(), counts: writer_core::api::SyncOperationCountsDto::default(), raw_error: Some(mask_sync_error(&e.to_string())) }).unwrap_or_default(),
                                        };
                                    }
                                }
                            }
                        }
                        match save_sync_configs(path, cfg_ref, sec_ref) {
                            Ok(()) => SyncTaskOutcome {
                                operation_id: operation_id.to_string(), operation_kind: "sync".to_string(), sync_status: "success".to_string(),
                                action_result: serde_json::to_string(&writer_core::api::SyncOperationStateDto { operation_id: operation_id.to_string(), operation_kind: "github_init".to_string(), status_code: "success".to_string(), phase_key: None, summary_key: Some("sync.result.clone_init_success".to_string()), summary_args: std::collections::HashMap::new(), counts: writer_core::api::SyncOperationCountsDto::default(), raw_error: None }).unwrap_or_default(),
                            },
                            Err(e) => SyncTaskOutcome {
                                operation_id: operation_id.to_string(), operation_kind: "sync".to_string(), sync_status: "error".to_string(),
                                action_result: serde_json::to_string(&writer_core::api::SyncOperationStateDto { operation_id: operation_id.to_string(), operation_kind: "github_init".to_string(), status_code: "error".to_string(), phase_key: None, summary_key: Some("sync.result.save_config_failed".to_string()), summary_args: [("error".to_string(), e)].into_iter().collect(), counts: writer_core::api::SyncOperationCountsDto::default(), raw_error: None }).unwrap_or_default(),
                            },
                        }
                    } else {
                        let err = result.error.unwrap_or_default();
                        SyncTaskOutcome {
                            operation_id: operation_id.to_string(),
                            operation_kind: "sync".to_string(),
                            sync_status: sync_error_category_from_code(
                                result.error_category.as_deref(),
                                &err,
                            ),
                            action_result: serde_json::to_string(&writer_core::api::SyncOperationStateDto { operation_id: operation_id.to_string(), operation_kind: "github_init".to_string(), status_code: sync_error_category_from_code(result.error_category.as_deref(), &err), phase_key: None, summary_key: Some("sync.result.clone_failed".to_string()), summary_args: std::collections::HashMap::new(), counts: writer_core::api::SyncOperationCountsDto::default(), raw_error: Some(mask_sync_error(&err)) }).unwrap_or_default(),
                        }
                    }
                }
                Err(e) => SyncTaskOutcome {
                    operation_id: operation_id.to_string(),
                    operation_kind: "sync".to_string(),
                    sync_status: sync_error_category(&e.to_string()),
                    action_result: serde_json::to_string(&writer_core::api::SyncOperationStateDto { operation_id: operation_id.to_string(), operation_kind: "github_init".to_string(), status_code: sync_error_category(&e.to_string()), phase_key: None, summary_key: Some("sync.result.clone_failed".to_string()), summary_args: std::collections::HashMap::new(), counts: writer_core::api::SyncOperationCountsDto::default(), raw_error: Some(mask_sync_error(&e.to_string())) }).unwrap_or_default(),
                },
            }
        } else if has_workspace() {
            let backend = writer_core::sync::create_sync_backend(&config.backend_type);
            match backend.sync(path_obj, &config, &secrets, true) {
                Ok(result) => {
                    if result.status == writer_core::sync::SyncStatus::Success {
                        match save_sync_configs(path, cfg_ref, sec_ref) {
                            Ok(()) => SyncTaskOutcome {
                                operation_id: operation_id.to_string(),
                                operation_kind: "sync".to_string(),
                                sync_status: "success".to_string(),
                                action_result: serde_json::to_string(&writer_core::api::SyncOperationStateDto { operation_id: operation_id.to_string(), operation_kind: "github_init".to_string(), status_code: "success".to_string(), phase_key: None, summary_key: Some("sync.result.remote_configured_sync_success".to_string()), summary_args: std::collections::HashMap::new(), counts: writer_core::api::SyncOperationCountsDto::default(), raw_error: None }).unwrap_or_default(),
                            },
                            Err(e) => SyncTaskOutcome {
                                operation_id: operation_id.to_string(),
                                operation_kind: "github_init".to_string(),
                                sync_status: "error".to_string(),
                                action_result: serde_json::to_string(&writer_core::api::SyncOperationStateDto {
                                    operation_id: operation_id.to_string(),
                                    operation_kind: "github_init".to_string(),
                                    status_code: "error".to_string(),
                                    phase_key: None,
                                    summary_key: Some("sync.result.save_config_failed".to_string()),
                                    summary_args: [("error".to_string(), e)].into_iter().collect(),
                                    counts: writer_core::api::SyncOperationCountsDto::default(),
                                    raw_error: None,
                                }).unwrap_or_default(),
                            },
                        }
                    } else if result.status == writer_core::sync::SyncStatus::Conflict {
                        let mut files = result
                            .conflicts
                            .iter()
                            .map(|c| c.local_path.clone())
                            .collect::<Vec<_>>();
                        if let Some(summary) = &result.conflict_summary {
                            for f in &summary.conflicted_files {
                                files.push(f.clone());
                            }
                        }
                        files.sort();
                        files.dedup();

                        let file_str = if files.is_empty() {
                            "sync.result.no_conflict_files".to_string()
                        } else {
                            let display_files = if files.len() > 100 {
                                let mut subset = files[0..100].to_vec();
                                subset.push(format!("sync.result.more_files_count: {}", files.len()));
                                subset
                            } else {
                                files.clone()
                            };
                            display_files.join("\n  - ")
                        };

                        let masked_err = result
                            .error
                            .as_deref()
                            .map(mask_sync_error)
                            .unwrap_or_else(|| "None".to_string());
                        debug_log_static(
                            "sync",
                            "conflict_detected",
                            &format!(
                                "conflicted file count={}, masked error={}",
                                files.len(),
                                masked_err
                            ),
                        );

                        let m = format!(
                            "sync.result.conflict_summary: {}",
                            file_str
                        );
                        SyncTaskOutcome {
                            operation_id: operation_id.to_string(),
                            operation_kind: "github_init".to_string(),
                            sync_status: "conflict".to_string(),
                            action_result: serde_json::to_string(&writer_core::api::SyncOperationStateDto {
                                operation_id: operation_id.to_string(),
                                operation_kind: "github_init".to_string(),
                                status_code: "conflict".to_string(),
                                phase_key: None,
                                summary_key: Some("sync.result.conflict_summary".to_string()),
                                summary_args: [("conflict_files".to_string(), file_str)].into_iter().collect(),
                                counts: writer_core::api::SyncOperationCountsDto { conflicts: files.len() as u32, ..Default::default() },
                                raw_error: result.error.as_ref().map(|e| mask_sync_error(e)),
                            }).unwrap_or_default(),
                        }
                    } else {
                        let err = result.error.unwrap_or_default();
                        let cat =
                            sync_error_category_from_code(result.error_category.as_deref(), &err);
                        let summary_key = if cat == "conflict" {
                            debug_log_static(
                                "sync",
                                "conflict_detected",
                                &format!(
                                    "conflicted file count=unknown, masked error={}",
                                    mask_sync_error(&err)
                                ),
                            );
                            "sync.result.conflict_summary".to_string()
                        } else {
                            "sync.result.generic_error".to_string()
                        };
                        SyncTaskOutcome {
                            operation_id: operation_id.to_string(),
                            operation_kind: "github_init".to_string(),
                            sync_status: cat.clone(),
                            action_result: serde_json::to_string(&writer_core::api::SyncOperationStateDto {
                                operation_id: operation_id.to_string(),
                                operation_kind: "github_init".to_string(),
                                status_code: cat,
                                phase_key: None,
                                summary_key: Some(summary_key),
                                summary_args: std::collections::HashMap::new(),
                                counts: writer_core::api::SyncOperationCountsDto::default(),
                                raw_error: Some(mask_sync_error(&err)),
                            }).unwrap_or_default(),
                        }
                    }
                }
                Err(e) => {
                    let err_str = e.to_string();
                    let cat = sync_error_category(&err_str);
                    let summary_key = if cat == "conflict" {
                        debug_log_static(
                            "sync",
                            "conflict_detected",
                            &format!(
                                "conflicted file count=unknown, masked error={}",
                                mask_sync_error(&err_str)
                            ),
                        );
                        "sync.result.conflict_summary".to_string()
                    } else {
                        "sync.result.generic_error".to_string()
                    };
                    SyncTaskOutcome {
                        operation_id: operation_id.to_string(),
                        operation_kind: "github_init".to_string(),
                        sync_status: cat.clone(),
                        action_result: serde_json::to_string(&writer_core::api::SyncOperationStateDto {
                            operation_id: operation_id.to_string(),
                            operation_kind: "github_init".to_string(),
                            status_code: cat,
                            phase_key: None,
                            summary_key: Some(summary_key),
                            summary_args: std::collections::HashMap::new(),
                            counts: writer_core::api::SyncOperationCountsDto::default(),
                            raw_error: Some(mask_sync_error(&err_str)),
                        }).unwrap_or_default(),
                    }
                }
            }
        } else if is_git_repo() {
            SyncTaskOutcome {
                operation_id: operation_id.to_string(), operation_kind: "github_init".to_string(), sync_status: "error".to_string(),
                action_result: serde_json::to_string(&writer_core::api::SyncOperationStateDto {
                    operation_id: operation_id.to_string(),
                    operation_kind: "github_init".to_string(),
                    status_code: "error".to_string(),
                    phase_key: None,
                    summary_key: Some("sync.result.git_repo_not_workspace".to_string()),
                    summary_args: std::collections::HashMap::new(),
                    counts: writer_core::api::SyncOperationCountsDto::default(),
                    raw_error: None,
                }).unwrap_or_default(),
            }
        } else {
            SyncTaskOutcome {
                operation_id: operation_id.to_string(),
                operation_kind: "github_init".to_string(),
                sync_status: "error".to_string(),
                action_result: serde_json::to_string(&writer_core::api::SyncOperationStateDto {
                    operation_id: operation_id.to_string(),
                    operation_kind: "github_init".to_string(),
                    status_code: "error".to_string(),
                    phase_key: None,
                    summary_key: Some("sync.result.directory_not_empty_not_workspace".to_string()),
                    summary_args: std::collections::HashMap::new(),
                    counts: writer_core::api::SyncOperationCountsDto::default(),
                    raw_error: None,
                }).unwrap_or_default(),
            }
        }
    }
}
