// =============================================================================
// github_init_operations.rs — GitHub 克隆/导入工作区（从 workspace_backend.rs 拆分）
// =============================================================================

use super::*;
use crate::sync_bridge::{mask_sync_error, save_sync_configs, sync_error_category_from_code};

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
        // 从作品目录路径推断 project_id（目录名即作品 ID）。
        let project_id_str = std::path::Path::new(&path_str)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        thread::spawn(move || {
            let result = Self::do_github_init(
                &op_id_capture,
                &path_str,
                &project_id_str,
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
        project_id: &str,
        remote_url: &str,
        branch: &str,
        token: &str,
    ) -> SyncTaskOutcome {
        use writer_core::sync::{
            provider::github::config::GitHubProviderConfig, provider::ProviderConfig,
            sanitize_remote_url, SyncConfig, SyncProtocol, SyncSecrets,
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

        let has_directory = || -> bool { path_obj.is_dir() };

        let is_git_repo = || -> bool { path_obj.join(".git").exists() };

        let effective_token = if token.is_empty() {
            parsed.extracted_token.clone()
        } else {
            Some(token.to_string())
        };

        let net = crate::backend::app_backend::current_network_state();
        // Issue #645：SyncConfig 改为 provider-neutral 结构，GitHub 字段通过
        // provider_config: Option<ProviderConfig::GitHub> 携带。
        let config = SyncConfig {
            enabled: true,
            active_provider: "github_api".to_string(),
            provider_config: Some(ProviderConfig::GitHub(GitHubProviderConfig {
                remote_url: sanitized_url.clone(),
                branch: branch.to_string(),
                username: parsed.extracted_username.clone().unwrap_or_default(),
                transport: SyncProtocol::HttpsToken,
            })),
            auto_sync: false,
            sync_interval_seconds: 300,
            has_network_permission: net.is_connected,
            has_network_state_permission: true,
        };

        // Issue #645：SyncSecrets 改为 provider_secrets → ProviderSecrets::GitHub { token }。
        // effective_token 为 None 时构造空 secrets（等价旧 SyncSecrets { token: None, .. }）。
        let secrets = match effective_token {
            Some(t) => SyncSecrets::from_github_token(t),
            None => SyncSecrets::default(),
        };

        let cfg_ref = &config;
        let sec_ref = &secrets;

        // 推断 projects_root（path 的父目录）用于创建 Core API。
        let projects_root = path_obj
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| ".".to_string());
        let api = crate::backend::app_backend::create_core_api(path, &projects_root);
        let config_dto: writer_core::api::types::SyncConfigDto = config.clone().into();

        if !has_content() {
            // 空目录：先确保 projects 子目录存在，再执行全量同步（clone + push）。
            // Core 已删除 workspace 概念。clone 成功后只需确保 projects 子目录存在。
            let projects_dir = path_obj.join("projects");
            if let Err(e) = std::fs::create_dir_all(&projects_dir) {
                return SyncTaskOutcome {
                    operation_id: operation_id.to_string(),
                    sync_status: "error".to_string(),
                    action_result: serde_json::to_string(
                        &writer_core::api::SyncOperationStateDto {
                            operation_id: operation_id.to_string(),
                            operation_kind: "github_init".to_string(),
                            status_code: "error".to_string(),
                            phase_key: None,
                            summary_key: Some("sync.result.clone_success_init_failed".to_string()),
                            summary_args: [("error".to_string(), mask_sync_error(&e.to_string()))]
                                .into_iter()
                                .collect(),
                            counts: writer_core::api::SyncOperationCountsDto::default(),
                            raw_error: Some(mask_sync_error(&e.to_string())),
                        },
                    )
                    .unwrap_or_default(),
                };
            }
            Self::run_github_init_sync(
                operation_id,
                &api,
                &config_dto,
                cfg_ref,
                sec_ref,
                path,
                project_id,
                "sync.result.clone_init_success",
            )
        } else if has_directory() {
            Self::run_github_init_sync(
                operation_id,
                &api,
                &config_dto,
                cfg_ref,
                sec_ref,
                path,
                project_id,
                "sync.result.remote_configured_sync_success",
            )
        } else if is_git_repo() {
            SyncTaskOutcome {
                operation_id: operation_id.to_string(),
                sync_status: "error".to_string(),
                action_result: serde_json::to_string(&writer_core::api::SyncOperationStateDto {
                    operation_id: operation_id.to_string(),
                    operation_kind: "github_init".to_string(),
                    status_code: "error".to_string(),
                    phase_key: None,
                    summary_key: Some("sync.result.git_repo_not_workspace".to_string()),
                    summary_args: std::collections::HashMap::new(),
                    counts: writer_core::api::SyncOperationCountsDto::default(),
                    raw_error: None,
                })
                .unwrap_or_default(),
            }
        } else {
            SyncTaskOutcome {
                operation_id: operation_id.to_string(),
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
                })
                .unwrap_or_default(),
            }
        }
    }

    /// 执行 github_init 的全量同步并构造 SyncTaskOutcome。
    ///
    /// 旧实现用 `create_sync_backend(...).sync(...)` 做单 target clone/sync（返回 SyncResult）；
    /// Issue #645 后 core 删除了 `create_sync_backend`/`BackendType`，改用 facade 的
    /// `perform_full_sync` 统一入口（返回 FullSyncResultDto）。本方法把 FullSyncResultDto
    /// 映射为等价的 SyncTaskOutcome，保留成功/冲突/失败的语义分支。
    fn run_github_init_sync(
        operation_id: &str,
        api: &writer_core::api::WriterCoreApi,
        config_dto: &writer_core::api::types::SyncConfigDto,
        config: &writer_core::sync::SyncConfig,
        secrets: &writer_core::sync::SyncSecrets,
        path: &str,
        project_id: &str,
        success_summary_key: &str,
    ) -> SyncTaskOutcome {
        match api.perform_full_sync(config_dto.clone(), true) {
            Ok(result) => {
                let status = result.overall_status.as_str();
                if matches!(status, "success" | "latest_wins_applied" | "no_changes") {
                    // 同步成功后保存配置。
                    match save_sync_configs(path, project_id, config, secrets) {
                        Ok(()) => SyncTaskOutcome {
                            operation_id: operation_id.to_string(),
                            sync_status: "success".to_string(),
                            action_result: serde_json::to_string(
                                &writer_core::api::SyncOperationStateDto {
                                    operation_id: operation_id.to_string(),
                                    operation_kind: "github_init".to_string(),
                                    status_code: "success".to_string(),
                                    phase_key: None,
                                    summary_key: Some(success_summary_key.to_string()),
                                    summary_args: std::collections::HashMap::new(),
                                    counts: writer_core::api::SyncOperationCountsDto::default(),
                                    raw_error: None,
                                },
                            )
                            .unwrap_or_default(),
                        },
                        Err(e) => SyncTaskOutcome {
                            operation_id: operation_id.to_string(),
                            sync_status: "error".to_string(),
                            action_result: serde_json::to_string(
                                &writer_core::api::SyncOperationStateDto {
                                    operation_id: operation_id.to_string(),
                                    operation_kind: "github_init".to_string(),
                                    status_code: "error".to_string(),
                                    phase_key: None,
                                    summary_key: Some("sync.result.save_config_failed".to_string()),
                                    summary_args: [("error".to_string(), e)].into_iter().collect(),
                                    counts: writer_core::api::SyncOperationCountsDto::default(),
                                    raw_error: None,
                                },
                            )
                            .unwrap_or_default(),
                        },
                    }
                } else if matches!(status, "conflict" | "partial_conflict") {
                    // Issue #645：冲突文件从 FullSyncResultDto.targets[].result.conflicts 提取。
                    let mut files: Vec<String> = result
                        .targets
                        .iter()
                        .flat_map(|t| t.result.conflicts.iter().map(|c| c.local_path.clone()))
                        .collect();
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

                    SyncTaskOutcome {
                        operation_id: operation_id.to_string(),
                        sync_status: "conflict".to_string(),
                        action_result: serde_json::to_string(
                            &writer_core::api::SyncOperationStateDto {
                                operation_id: operation_id.to_string(),
                                operation_kind: "github_init".to_string(),
                                status_code: "conflict".to_string(),
                                phase_key: None,
                                summary_key: Some("sync.result.conflict_summary".to_string()),
                                summary_args: [("conflict_files".to_string(), file_str)]
                                    .into_iter()
                                    .collect(),
                                counts: writer_core::api::SyncOperationCountsDto {
                                    conflicts: files.len() as u32,
                                    ..Default::default()
                                },
                                raw_error: result.error.as_ref().map(|e| mask_sync_error(e)),
                            },
                        )
                        .unwrap_or_default(),
                    }
                } else {
                    let err = result.error.unwrap_or_default();
                    let cat = sync_error_category_from_code(result.error_category.as_deref(), &err);
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
                        sync_status: cat.clone(),
                        action_result: serde_json::to_string(
                            &writer_core::api::SyncOperationStateDto {
                                operation_id: operation_id.to_string(),
                                operation_kind: "github_init".to_string(),
                                status_code: cat,
                                phase_key: None,
                                summary_key: Some(summary_key),
                                summary_args: std::collections::HashMap::new(),
                                counts: writer_core::api::SyncOperationCountsDto::default(),
                                raw_error: Some(mask_sync_error(&err)),
                            },
                        )
                        .unwrap_or_default(),
                    }
                }
            }
            Err(e) => {
                let err_str = e.to_string();
                let cat = sync_error_category_from_code(None, &err_str);
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
                    "sync.result.clone_failed".to_string()
                };
                SyncTaskOutcome {
                    operation_id: operation_id.to_string(),
                    sync_status: cat.clone(),
                    action_result: serde_json::to_string(
                        &writer_core::api::SyncOperationStateDto {
                            operation_id: operation_id.to_string(),
                            operation_kind: "github_init".to_string(),
                            status_code: cat,
                            phase_key: None,
                            summary_key: Some(summary_key),
                            summary_args: std::collections::HashMap::new(),
                            counts: writer_core::api::SyncOperationCountsDto::default(),
                            raw_error: Some(mask_sync_error(&err_str)),
                        },
                    )
                    .unwrap_or_default(),
                }
            }
        }
    }
}
