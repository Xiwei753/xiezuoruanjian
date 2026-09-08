use super::*;

impl WriterCoreApi {
    pub fn list_projects(&self) -> ApiResult<Vec<ProjectDto>> {
        self.core_read()
            .list_projects()
            .map(|v| v.into_iter().map(Into::into).collect())
            .map_err(Into::into)
    }

    /// 批量返回项目摘要（元数据 统计）。
    pub fn list_project_summaries(&self) -> ApiResult<Vec<ProjectSummaryDto>> {
        self.core_read()
            .list_project_summaries()
            .map(|v| v.into_iter().map(Into::into).collect())
            .map_err(Into::into)
    }

    pub fn create_project(&self, title: &str) -> ApiResult<ProjectDto> {
        //   用 _with_changes 版本拿变更集，
        // 调 record_workspace_change_set_history 记录本地历史。
        let (project, change_set) = self
            .core_write()
            .create_project_with_changes(title)
            .map_err(WriterError::from)?;
        let entry = crate::search::extractor::extract_project_title_entry(&project.id, title);
        self.enqueue_search_index_update(crate::search::SearchIndexUpdate {
            action: crate::search::SearchIndexAction::Upsert,
            object_id: entry.object_id.clone(),
            scope: entry.scope,
            title: entry.title.clone(),
            body: entry.body.clone(),
            target: Some(entry.target.clone()),
        });
        let volumes_result = self.core_write().list_volumes(&project.id);
        if let Ok(volumes) = volumes_result {
            if let Some(default_vol) = volumes.first() {
                let vol_entry = crate::search::extractor::extract_volume_title_entry(
                    &project.id,
                    &default_vol.id,
                    &default_vol.title,
                );
                self.enqueue_search_index_update(crate::search::SearchIndexUpdate {
                    action: crate::search::SearchIndexAction::Upsert,
                    object_id: vol_entry.object_id.clone(),
                    scope: vol_entry.scope,
                    title: vol_entry.title.clone(),
                    body: vol_entry.body.clone(),
                    target: Some(vol_entry.target.clone()),
                });
            }
        }
        let _ = self.record_workspace_change_set_history(&change_set, "create_project");
        Ok(project.into())
    }

    /// 恢复/导入项目入口——使用 manifest 中的稳定 ID。
    ///
    /// 不自动创建"第一卷"（卷信息在 manifest 中已包含，由调用方逐卷恢复）。
    /// 不记录 workspace history（恢复场景下 manifest 是已有事实来源）。
    pub fn create_project_with_id(
        &self,
        id: &str,
        title: &str,
        order: i32,
    ) -> ApiResult<ProjectDto> {
        let project = self
            .core_write()
            .create_project_with_id(id, title, order)
            .map_err(WriterError::from)?;
        // 恢复场景：不记录 workspace history（manifest 是已有事实来源）
        // 不自动创建默认卷（卷信息在 manifest 中）
        Ok(project.into())
    }

    pub fn get_project_stats(&self, project_id: &str) -> ApiResult<ProjectStatsDto> {
        self.core_read()
            .get_project_stats(project_id)
            .map(Into::into)
            .map_err(Into::into)
    }

    /// 一次返回作品的全部卷 章节 统计。
    ///
    /// Android `ProjectViewModel` 不再逐卷调 `list_chapters`，
    /// 而是一次拿到完整快照，减少 FFI 调用次数和中间状态不一致窗口。
    pub fn get_project_workspace_snapshot(
        &self,
        project_id: &str,
    ) -> ApiResult<ProjectWorkspaceSnapshotDto> {
        let core = self.core_read();
        let project: ProjectDto = core
            .list_projects()
            .map_err(WriterError::from)?
            .into_iter()
            .find(|p| p.id == project_id)
            .map(Into::into)
            .ok_or(WriterError::ProjectNotFound)?;

        let stats: ProjectStatsDto = core
            .get_project_stats(project_id)
            .map(Into::into)
            .map_err(WriterError::from)?;

        let volumes = core.list_volumes(project_id).map_err(WriterError::from)?;

        let mut volume_snapshots = Vec::with_capacity(volumes.len());
        for vol in volumes {
            let chapters = core
                .list_chapters(project_id, &vol.id)
                .map_err(WriterError::from)?
                .into_iter()
                .map(Into::into)
                .collect();
            volume_snapshots.push(VolumeWithChaptersDto {
                volume: vol.into(),
                chapters,
            });
        }

        Ok(ProjectWorkspaceSnapshotDto {
            project,
            stats,
            volumes: volume_snapshots,
        })
    }

    pub fn rename_project(&self, project_id: &str, new_title: &str) -> ApiResult<bool> {
        //   用 _with_changes 版本拿变更集。
        let (_project, change_set) = self
            .core_write()
            .rename_project_with_changes(project_id, new_title)?;
        let entry = crate::search::extractor::extract_project_title_entry(project_id, new_title);
        self.enqueue_search_index_update(crate::search::SearchIndexUpdate {
            action: crate::search::SearchIndexAction::Upsert,
            object_id: entry.object_id.clone(),
            scope: entry.scope,
            title: entry.title.clone(),
            body: entry.body.clone(),
            target: Some(entry.target.clone()),
        });
        let _ = self.record_workspace_change_set_history(&change_set, "rename_project");
        Ok(true)
    }

    pub fn delete_project(&self, project_id: &str) -> ApiResult<bool> {
        //
        // - core 层不再吞 list_starmaps_bound_to_project 错误，index.json 损坏时删除返回 Err。
        // - 不再二次枚举绑定 starmap——用 outcome.unbound_starmap_ids（journal 里记录的
        //   唯一事实来源）刷搜索索引。
        // - 不再在 core 内 complete/cleanup journal——记 history 成功后调
        //   ack_project_delete_history 推进到 HistoryRecorded → Completed 并清 journal。
        //   history 失败时 journal 保留在 StarMapsUnbound，下次启动 recover 补记。

        //   读取 device_id 传给 delete_project_with_changes，
        // 写入 journal 供 ack/recover 构造 PendingDeletedTarget（LWW tie-break）。
        let device_id = crate::settings::load_device_info(&self.app_data_root)
            .map(|i| i.device_id)
            .unwrap_or_default();

        let outcome = self
            .core_write()
            .delete_project_with_changes(project_id, &device_id)?;

        // 删除成功后才清搜索索引。搜索索引清理放在删除成功之后，避免
        // project 没删掉但搜索索引已清空的不一致状态。
        for prefix in &[
            format!("project:{}", project_id),
            format!("volume:{}:", project_id),
            format!("chapter_title:{}:", project_id),
            format!("chapter_body:{}:", project_id),
            format!("chapter_note:{}:", project_id),
        ] {
            self.remove_search_index_by_prefix(prefix);
        }

        // 用 outcome.unbound_starmap_ids 刷搜索索引，
        // 不再二次枚举 list_starmaps_bound_to_project（避免与 core 层结果不一致）。
        for sm_id in &outcome.unbound_starmap_ids {
            self.refresh_starmap_search_index(sm_id);
        }

        // 用 outcome.changes 记本地 history，
        // 成功后 ack 推进 journal。history 失败时 log::warn 并保留 journal，
        // 下次启动 recover 补记——不让 history 失败把删除变成失败（项目已删）。
        let layout_guard = match self.workspace_git_layout.read() {
            Ok(g) => g,
            Err(_) => {
                log::warn!(
                    "delete_project: layout lock poisoned, skipping history + ack; \
                     journal retained for recovery"
                );
                return Ok(true);
            }
        };
        match crate::storage::workspace_git::record_workspace_change_set(
            &layout_guard,
            &outcome.changes,
            "delete_project",
        ) {
            Ok(result) => {
                if result.oid.is_some() {
                    log::debug!(
                        "delete_project: history committed ({} staged)",
                        result.staged_count
                    );
                }
                // history 成功，ack 推进 journal 到 HistoryRecorded → Completed 并清 journal。
                if let Err(e) = crate::storage::journal::project_delete::ack_project_delete_history(
                    &self.app_data_root,
                    &outcome.journal_token,
                ) {
                    log::warn!(
                        "delete_project: ack_project_delete_history failed: {} — \
                         journal retained for recovery",
                        e
                    );
                }
            }
            Err(e) => {
                log::warn!(
                    "delete_project: record_workspace_change_set failed: {} — \
                     journal retained for recovery, history will be补 on next startup",
                    e
                );
            }
        }

        Ok(true)
    }

    pub fn reorder_projects(&self, ordered_project_ids: &[String]) -> ApiResult<bool> {
        //   用 _with_changes 版本拿变更集。
        let change_set = self
            .core_write()
            .reorder_projects_with_changes(ordered_project_ids)?;
        let _ = self.record_workspace_change_set_history(&change_set, "reorder_projects");
        Ok(true)
    }

    pub fn list_volumes(&self, project_id: &str) -> ApiResult<Vec<VolumeDto>> {
        self.core_read()
            .list_volumes(project_id)
            .map(|v| v.into_iter().map(Into::into).collect())
            .map_err(Into::into)
    }

    pub fn create_volume(&self, project_id: &str, title: &str) -> ApiResult<VolumeDto> {
        //   用 _with_changes 版本拿变更集，
        // 调 record_workspace_change_set_history 记录本地历史。
        let (volume, change_set) = self
            .core_write()
            .create_volume_with_changes(project_id, title)
            .map_err(WriterError::from)?;
        let volume: VolumeDto = volume.into();
        let entry =
            crate::search::extractor::extract_volume_title_entry(project_id, &volume.id, title);
        self.enqueue_search_index_update(crate::search::SearchIndexUpdate {
            action: crate::search::SearchIndexAction::Upsert,
            object_id: entry.object_id.clone(),
            scope: entry.scope,
            title: entry.title.clone(),
            body: entry.body.clone(),
            target: Some(entry.target.clone()),
        });
        let _ = self.record_workspace_change_set_history(&change_set, "create_volume");
        Ok(volume)
    }

    /// 恢复/导入卷——使用 manifest 中的稳定 ID。
    ///
    /// 恢复场景：不记录 workspace history（manifest 是已有事实来源）。
    pub fn create_volume_with_id(
        &self,
        project_id: &str,
        id: &str,
        title: &str,
        order: i32,
    ) -> ApiResult<VolumeDto> {
        let volume = self
            .core_write()
            .create_volume_with_id(project_id, id, title, order)
            .map_err(WriterError::from)?;
        Ok(volume.into())
    }

    pub fn rename_volume(
        &self,
        project_id: &str,
        volume_id: &str,
        new_title: &str,
    ) -> ApiResult<bool> {
        //   用 _with_changes 版本拿变更集。
        let change_set = self
            .core_write()
            .rename_volume_with_changes(project_id, volume_id, new_title)?;
        let entry =
            crate::search::extractor::extract_volume_title_entry(project_id, volume_id, new_title);
        self.enqueue_search_index_update(crate::search::SearchIndexUpdate {
            action: crate::search::SearchIndexAction::Upsert,
            object_id: entry.object_id.clone(),
            scope: entry.scope,
            title: entry.title.clone(),
            body: entry.body.clone(),
            target: Some(entry.target.clone()),
        });
        let _ = self.record_workspace_change_set_history(&change_set, "rename_volume");
        Ok(true)
    }

    pub fn delete_volume(&self, project_id: &str, volume_id: &str) -> ApiResult<bool> {
        //   用 _with_changes 版本拿变更集。
        // change_set 由底层 delete_volume_with_changes 返回，包含
        // DeleteTree(projects/{pid}/volumes/{vid})，不再手拼路径。
        let change_set = self
            .core_write()
            .delete_volume_with_changes(project_id, volume_id)?;
        for prefix in &[
            format!("volume:{}:{}", project_id, volume_id),
            format!("chapter_title:{}:{}:", project_id, volume_id),
            format!("chapter_body:{}:{}:", project_id, volume_id),
            format!("chapter_note:{}:{}:", project_id, volume_id),
        ] {
            self.remove_search_index_by_prefix(prefix);
        }
        let _ = self.record_workspace_change_set_history(&change_set, "delete_volume");
        Ok(true)
    }

    pub fn reorder_volumes(
        &self,
        project_id: &str,
        ordered_volume_ids: &[String],
    ) -> ApiResult<bool> {
        //   用 _with_changes 版本拿变更集。
        let change_set = self
            .core_write()
            .reorder_volumes_with_changes(project_id, ordered_volume_ids)?;
        let _ = self.record_workspace_change_set_history(&change_set, "reorder_volumes");
        Ok(true)
    }

    /// 恢复作品树——一次跨 FFI 传入完整作品树。
    ///
    /// Core 负责：
    /// 1. 校验 project/volume/chapter ID 非空、格式合法（UUID）；
    /// 2. 校验目标 ID 不冲突（project_id 不已存在、同 project 内 volume_id 唯一、
    ///    同 volume 内 chapter_id 唯一）；
    /// 3. 逐层创建项目/卷/章节、保存正文、按 input 顺序 reorder；
    /// 4. 任一步失败时回滚已创建的项目（delete_project），返回 Err，不留半成品；
    /// 5. 全部成功后把这次恢复作为一次 workspace Git 变更记录下来；
    /// 6. 返回创建的 ProjectDto。
    ///
    /// staging 原子发布——
    /// 先在 `.restore-staging/<txId>/<projectId>/` 下完整生成，
    /// 所有 ID/正文校验完成后再 rename 到 `projects/<projectId>/`。
    pub fn restore_project_tree(&self, input: &RestoreProjectInputDto) -> ApiResult<ProjectDto> {
        use std::fs;
        use uuid::Uuid;

        // 精确幂等恢复。
        // 只校验输入格式（ID 格式、唯一性），不检查 project 是否已存在。
        Self::validate_restore_input_shape(input)?;

        // 如果 project 已存在，检查是否与恢复输入完全一致（上一次恢复已成功）。
        // 完全一致 → 幂等返回已有 ProjectDto；不同 → 冲突，拒绝覆盖。
        // 幂等匹配成功时也必须补记 Git history，
        // 防止 rename 成功后进程死亡导致 Git history 永久缺失。
        // history 失败返回 Err，下次重试会再次进入此分支直到 Git 也进入完成态。
        if self.project_exists(input)? {
            if self.restored_project_matches_input(input)? {
                self.ensure_restore_history(input)?;
                let project_dto: ProjectDto = self
                    .core_read()
                    .list_projects()
                    .map_err(WriterError::from)?
                    .into_iter()
                    .find(|p| p.id == input.project_id)
                    .map(Into::into)
                    .ok_or(WriterError::ProjectNotFound)?;
                return Ok(project_dto);
            }
            return Err(WriterError::Other(format!(
                "restore_project_tree: existing project differs from restore input: {}",
                input.project_id
            )));
        }

        // 2. 生成事务 ID，创建 staging 目录
        // 路径：app_data_root/.restore-staging/<txId>/
        // 项目会创建在：app_data_root/.restore-staging/<txId>/<projectId>/
        let transaction_id = Uuid::new_v4().to_string();
        let staging_root = self
            .app_data_root
            .join(".restore-staging")
            .join(&transaction_id);

        // 3. 创建临时 WriterCoreApi，使用 staging 目录作为 projects_root
        // 这样 create_project_with_id 会在 staging_root/<projectId>/ 下创建项目
        let staging_api = WriterCoreApi::new(&self.app_data_root, &staging_root);

        // 4. 在 staging 目录下创建项目树
        if let Err(e) = self.create_restore_tree_with_staging_api(&staging_api, input) {
            // 失败时直接删除 staging 目录，不走正常 delete_project()
            let _ = fs::remove_dir_all(&staging_root);
            return Err(self.rollback_restore_with_staging(e, &staging_root));
        }

        // 5. 所有校验完成，原子 rename staging/<projectId> → 最终位置
        let staging_project_dir = staging_root.join(&input.project_id);
        let target_project_dir = self.projects_root.join(&input.project_id);
        if let Some(parent) = target_project_dir.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::rename(&staging_project_dir, &target_project_dir).map_err(|e| {
            // rename 失败时清理 staging
            let _ = fs::remove_dir_all(&staging_root);
            WriterError::Other(format!(
                "restore_project_tree: failed to rename staging to final: {}",
                e
            ))
        })?;

        // 6. 清理空的 staging 目录（如果存在）
        let _ = fs::remove_dir(&staging_root);

        //   history 失败返回 Err，不再用 let _ = 忽略。
        // canonical 目录已经存在，下一次恢复会命中"内容完全一致"分支，
        // 再次调用 ensure_restore_history()，直到 Git 也真正进入完成态。
        self.ensure_restore_history(input)?;

        // 8. 返回创建的 ProjectDto
        let project_dto: ProjectDto = self
            .core_read()
            .list_projects()
            .map_err(WriterError::from)?
            .into_iter()
            .find(|p| p.id == input.project_id)
            .map(Into::into)
            .ok_or(WriterError::ProjectNotFound)?;

        Ok(project_dto)
    }

    /// 校验恢复输入的格式：ID 非空、UUID 格式、唯一性。
    ///
    /// 不再检查 project_id 是否已存在，
    /// 改由 [restore_project_tree] 做幂等匹配。
    fn validate_restore_input_shape(input: &RestoreProjectInputDto) -> ApiResult<()> {
        use std::collections::HashSet;

        Self::validate_id_non_empty(&input.project_id, "project_id")?;
        Self::validate_uuid_format(&input.project_id, "project_id")?;

        // 校验 volume_id 唯一、非空、UUID 格式；chapter_id 同理
        let mut seen_volume_ids: HashSet<&str> = HashSet::new();
        for vol in &input.volumes {
            Self::validate_id_non_empty(&vol.volume_id, "volume_id")?;
            Self::validate_uuid_format(&vol.volume_id, "volume_id")?;
            if !seen_volume_ids.insert(vol.volume_id.as_str()) {
                return Err(WriterError::Other(format!(
                    "restore_project_tree: duplicate volume_id in input: {}",
                    vol.volume_id
                )));
            }

            Self::validate_volume_chapters(vol)?;
        }
        Ok(())
    }

    /// 校验单个卷内 chapter_id 唯一、非空、UUID 格式。
    fn validate_volume_chapters(vol: &RestoreVolumeInputDto) -> ApiResult<()> {
        use std::collections::HashSet;

        let mut seen_chapter_ids: HashSet<&str> = HashSet::new();
        for ch in &vol.chapters {
            Self::validate_id_non_empty(&ch.chapter_id, "chapter_id")?;
            Self::validate_uuid_format(&ch.chapter_id, "chapter_id")?;
            if !seen_chapter_ids.insert(ch.chapter_id.as_str()) {
                return Err(WriterError::Other(format!(
                    "restore_project_tree: duplicate chapter_id in volume {}: {}",
                    vol.volume_id, ch.chapter_id
                )));
            }
        }
        Ok(())
    }

    /// 校验 ID 非空。
    fn validate_id_non_empty(id: &str, name: &str) -> ApiResult<()> {
        if id.trim().is_empty() {
            return Err(WriterError::Other(format!(
                "restore_project_tree: {} must not be empty",
                name
            )));
        }
        Ok(())
    }

    /// 校验 UUID 格式。
    fn validate_uuid_format(id: &str, name: &str) -> ApiResult<()> {
        if uuid::Uuid::parse_str(id).is_err() {
            return Err(WriterError::Other(format!(
                "restore_project_tree: invalid {} format: {}",
                name, id
            )));
        }
        Ok(())
    }

    /// 检查 project 是否已存在。
    fn project_exists(&self, input: &RestoreProjectInputDto) -> ApiResult<bool> {
        let existing = self
            .core_read()
            .list_projects()
            .map_err(WriterError::from)?;
        Ok(existing.iter().any(|p| p.id == input.project_id))
    }

    /// 比较已有 project 与恢复输入是否完全一致。
    ///
    /// 比较维度：
    /// - project title
    /// - volume 数量、每个 volume 的 id/title/order（按 order 排序后逐个比较）
    /// - 每个 volume 下的 chapter 数量、每个 chapter 的 id/title/order/content
    ///
    /// 任何一个维度不同就返回 false，不会尝试合并或覆盖。
    fn restored_project_matches_input(&self, input: &RestoreProjectInputDto) -> ApiResult<bool> {
        // 比较 project title
        let existing_project = self
            .core_read()
            .list_projects()
            .map_err(WriterError::from)?
            .into_iter()
            .find(|p| p.id == input.project_id);
        match existing_project {
            Some(ep) if ep.title == input.title => {}
            _ => return Ok(false),
        }

        // 读取已有 project 的 volumes，按 order 排序后与 input 比较
        let mut existing_vols = self.list_volumes(&input.project_id)?;
        existing_vols.sort_by_key(|v| v.order);
        let mut input_vols: Vec<_> = input.volumes.iter().collect();
        input_vols.sort_by_key(|v| v.order);

        if existing_vols.len() != input_vols.len() {
            return Ok(false);
        }

        for (ev, iv) in existing_vols.iter().zip(input_vols.iter()) {
            if ev.id != iv.volume_id || ev.title != iv.title || ev.order != iv.order {
                return Ok(false);
            }
            if !self.volume_matches_input(&input.project_id, ev, iv)? {
                return Ok(false);
            }
        }

        Ok(true)
    }

    /// 比较单个卷的 chapters 是否与恢复输入完全一致。
    fn volume_matches_input(
        &self,
        project_id: &str,
        existing_vol: &VolumeDto,
        input_vol: &RestoreVolumeInputDto,
    ) -> ApiResult<bool> {
        let mut existing_chs = self.list_chapters(project_id, &existing_vol.id)?;
        existing_chs.sort_by_key(|c| c.order);
        let mut input_chs: Vec<_> = input_vol.chapters.iter().collect();
        input_chs.sort_by_key(|c| c.order);

        if existing_chs.len() != input_chs.len() {
            return Ok(false);
        }

        for (ec, ic) in existing_chs.iter().zip(input_chs.iter()) {
            if ec.id != ic.chapter_id || ec.title != ic.title || ec.order != ic.order {
                return Ok(false);
            }
            let existing_content = self.open_chapter(project_id, &existing_vol.id, &ec.id)?;
            if existing_content.content != ic.content {
                return Ok(false);
            }
        }

        Ok(true)
    }

    ///   staging helper 只负责在 staging 目录下生成完整项目树，
    /// 不再零散累加 change set。恢复 change set 由 build_restore_workspace_change_set() 统一构造。
    fn create_restore_tree_with_staging_api(
        &self,
        staging_api: &WriterCoreApi,
        input: &RestoreProjectInputDto,
    ) -> crate::error::Result<()> {
        // 创建项目（使用 staging API，会在 staging 目录下创建）
        staging_api.core_write().create_project_with_id(
            &input.project_id,
            &input.title,
            input.order,
        )?;

        // 逐卷创建
        for vol in &input.volumes {
            self.create_restore_volume_with_staging_api(staging_api, input, vol)?;
        }

        // 按 input 顺序 reorder volumes（确保 order 连续 0,1,2,...）
        if !input.volumes.is_empty() {
            let ordered_volume_ids: Vec<String> =
                input.volumes.iter().map(|v| v.volume_id.clone()).collect();
            staging_api
                .core_write()
                .reorder_volumes_with_changes(&input.project_id, &ordered_volume_ids)?;
        }

        // 按 input 顺序 reorder 每个卷的 chapters
        for vol in &input.volumes {
            self.reorder_restore_chapters_with_staging_api(staging_api, input, vol)?;
        }

        Ok(())
    }

    /// 使用 staging API 创建单个恢复卷及其章节。
    fn create_restore_volume_with_staging_api(
        &self,
        staging_api: &WriterCoreApi,
        input: &RestoreProjectInputDto,
        vol: &RestoreVolumeInputDto,
    ) -> crate::error::Result<()> {
        staging_api.core_write().create_volume_with_id(
            &input.project_id,
            &vol.volume_id,
            &vol.title,
            vol.order,
        )?;

        // 逐章创建 + 保存正文
        for ch in &vol.chapters {
            self.create_restore_chapter_with_staging_api(staging_api, input, vol, ch)?;
        }

        Ok(())
    }

    /// 使用 staging API 创建单个恢复章节并保存正文。
    ///
    /// 不再零散累加 change set。
    /// chapter.md 始终由 save_chapter_verified_with_changes_with_options 写入 staging，
    /// 空正文时 create_chapter_with_id 也创建空 chapter.md，文件磁盘上一定存在。
    /// 恢复 change set 由 build_restore_workspace_change_set() 统一构造，
    /// 无论正文是否为空都包含 chapter.md。
    fn create_restore_chapter_with_staging_api(
        &self,
        staging_api: &WriterCoreApi,
        input: &RestoreProjectInputDto,
        vol: &RestoreVolumeInputDto,
        ch: &RestoreChapterInputDto,
    ) -> crate::error::Result<()> {
        staging_api.core_write().create_chapter_with_id(
            &input.project_id,
            &vol.volume_id,
            &ch.chapter_id,
            &ch.title,
            ch.order,
        )?;

        // 保存正文：content 非空时才写入（create_chapter_with_id 已创建空 chapter.md）
        if !ch.content.is_empty() {
            staging_api
                .core_write()
                .save_chapter_verified_with_changes_with_options(
                    &input.project_id,
                    &vol.volume_id,
                    &ch.chapter_id,
                    &ch.content,
                    true,
                )?;
        }

        Ok(())
    }

    /// 使用 staging API reorder 单个卷的章节。
    fn reorder_restore_chapters_with_staging_api(
        &self,
        staging_api: &WriterCoreApi,
        input: &RestoreProjectInputDto,
        vol: &RestoreVolumeInputDto,
    ) -> crate::error::Result<()> {
        if vol.chapters.is_empty() {
            return Ok(());
        }
        let ordered_chapter_ids: Vec<String> =
            vol.chapters.iter().map(|c| c.chapter_id.clone()).collect();
        staging_api.core_write().reorder_chapters_with_changes(
            &input.project_id,
            &vol.volume_id,
            &ordered_chapter_ids,
        )?;
        Ok(())
    }

    /// 从 RestoreProjectInputDto 构造唯一一份确定性的
    /// restore change set，确保空正文章节的 chapter.md 也包含在内。
    ///
    /// 不再由 staging helper 零散累加 change set；恢复只有一份 change-set 真值。
    fn build_restore_workspace_change_set(
        input: &RestoreProjectInputDto,
    ) -> crate::storage::workspace_git::WorkspaceChangeSet {
        let mut cs = crate::storage::workspace_git::WorkspaceChangeSet::new().add_upsert(
            std::path::PathBuf::from("projects")
                .join(&input.project_id)
                .join("project.json"),
        );

        for vol in &input.volumes {
            cs = cs.add_upsert(
                std::path::PathBuf::from("projects")
                    .join(&input.project_id)
                    .join("volumes")
                    .join(&vol.volume_id)
                    .join("volume.json"),
            );

            for ch in &vol.chapters {
                let chapter_dir = std::path::PathBuf::from("projects")
                    .join(&input.project_id)
                    .join("volumes")
                    .join(&vol.volume_id)
                    .join("chapters")
                    .join(&ch.chapter_id);

                cs = cs
                    .add_upsert(chapter_dir.join("chapter.meta.json"))
                    .add_upsert(chapter_dir.join("chapter.md"));
            }
        }

        cs
    }

    /// 把 Git history 记录收成幂等 helper。
    ///
    /// 两条成功路径（幂等匹配 + 新恢复）都必须调用它。history 失败返回 Err，
    /// 因为 canonical 目录已存在，下一次恢复会命中"内容完全一致"分支，
    /// 再次调用 `ensure_restore_history()`，直到 Git 也真正进入完成态。
    fn ensure_restore_history(&self, input: &RestoreProjectInputDto) -> ApiResult<()> {
        let cs = Self::build_restore_workspace_change_set(input);
        self.record_workspace_change_set_history(&cs, "restore_project_tree")
            .map_err(WriterError::from)?;
        Ok(())
    }

    /// 回滚恢复操作（staging 模式）：直接删除 staging 目录，不走正常 delete_project()。
    /// 返回原始错误，让调用方返回给 FFI 调用方。
    fn rollback_restore_with_staging(
        &self,
        err: crate::error::Error,
        staging_root: &std::path::Path,
    ) -> WriterError {
        use std::fs;
        log::warn!(
            "restore_project_tree: rolling back staging due to an error; details propagated to caller",
        );
        // 直接删除 staging 目录，不走正常 delete_project()（避免走 history/tombstone）
        if let Err(del_err) = fs::remove_dir_all(staging_root) {
            log::warn!(
                "restore_project_tree: rollback delete staging failed: {}; staging directory may remain",
                del_err,
            );
        }
        WriterError::from(err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::types::{
        ChapterMetaDto, RestoreChapterInputDto, RestoreProjectInputDto, RestoreVolumeInputDto,
    };
    use tempfile::tempdir;
    use uuid::Uuid;

    /// 构造一个合法的 UUID 字符串。
    fn new_uuid() -> String {
        Uuid::new_v4().to_string()
    }

    /// 构造一个完整的 RestoreProjectInputDto，包含 2 卷，每卷 2 章节，正文非空。
    fn make_full_input() -> RestoreProjectInputDto {
        let project_id = new_uuid();
        let vol1_id = new_uuid();
        let vol2_id = new_uuid();
        let ch1_id = new_uuid();
        let ch2_id = new_uuid();
        let ch3_id = new_uuid();
        let ch4_id = new_uuid();

        RestoreProjectInputDto {
            project_id,
            title: "恢复测试作品".to_string(),
            order: 0,
            volumes: vec![
                RestoreVolumeInputDto {
                    volume_id: vol1_id,
                    title: "第一卷".to_string(),
                    order: 0,
                    chapters: vec![
                        RestoreChapterInputDto {
                            chapter_id: ch1_id,
                            title: "第一章".to_string(),
                            order: 0,
                            content: "第一章正文内容。".to_string(),
                        },
                        RestoreChapterInputDto {
                            chapter_id: ch2_id,
                            title: "第二章".to_string(),
                            order: 1,
                            content: "第二章正文内容。".to_string(),
                        },
                    ],
                },
                RestoreVolumeInputDto {
                    volume_id: vol2_id,
                    title: "第二卷".to_string(),
                    order: 1,
                    chapters: vec![
                        RestoreChapterInputDto {
                            chapter_id: ch3_id,
                            title: "第三章".to_string(),
                            order: 0,
                            content: "第三章正文内容。".to_string(),
                        },
                        RestoreChapterInputDto {
                            chapter_id: ch4_id,
                            title: "第四章".to_string(),
                            order: 1,
                            content: "".to_string(),
                        },
                    ],
                },
            ],
        }
    }

    /// 创建测试用 WriterCoreApi 实例（含初始化 workspace Git 仓库）。
    fn make_api() -> (tempfile::TempDir, WriterCoreApi) {
        let temp_dir = tempdir().unwrap();
        std::fs::create_dir_all(temp_dir.path().join("projects")).unwrap();
        let api = WriterCoreApi::new(temp_dir.path(), temp_dir.path().join("projects"));
        // 初始化 workspace Git 仓库，让 record_workspace_change_set_history 可用
        let layout =
            crate::storage::git_repo_layout::GitRepoLayout::new(temp_dir.path().to_path_buf());
        crate::storage::workspace_git::ensure_workspace_repo(&layout).unwrap();
        api.set_workspace_git_layout(layout);
        (temp_dir, api)
    }

    #[test]
    fn restore_project_tree_success_creates_full_tree() {
        let (_dir, api) = make_api();
        let input = make_full_input();

        let expected_project_id = input.project_id.clone();
        let expected_vol_ids: Vec<String> =
            input.volumes.iter().map(|v| v.volume_id.clone()).collect();
        let expected_chapters: Vec<(String, String, String)> = input
            .volumes
            .iter()
            .flat_map(|v| {
                v.chapters
                    .iter()
                    .map(|c| (v.volume_id.clone(), c.chapter_id.clone(), c.content.clone()))
            })
            .collect();

        let result = api.restore_project_tree(&input).unwrap();
        assert_eq!(result.id, expected_project_id);
        assert_eq!(result.title, "恢复测试作品");

        // 验证卷和章节都按指定 ID 创建
        let volumes = api.list_volumes(&expected_project_id).unwrap();
        assert_eq!(volumes.len(), 2);
        let actual_vol_ids: Vec<String> = volumes.iter().map(|v| v.id.clone()).collect();
        for expected_id in &expected_vol_ids {
            assert!(
                actual_vol_ids.contains(expected_id),
                "volume {} should exist",
                expected_id
            );
        }
        // 验证卷 order 连续（0, 1）
        let mut sorted_vols = volumes.clone();
        sorted_vols.sort_by_key(|v| v.order);
        assert_eq!(sorted_vols[0].order, 0);
        assert_eq!(sorted_vols[1].order, 1);

        // 验证章节和正文
        for (vol_id, ch_id, expected_content) in &expected_chapters {
            let chapters = api.list_chapters(&expected_project_id, vol_id).unwrap();
            let chapter: &ChapterMetaDto = chapters
                .iter()
                .find(|c| &c.id == ch_id)
                .unwrap_or_else(|| panic!("chapter {} should exist", ch_id));

            let opened = api
                .open_chapter(&expected_project_id, vol_id, ch_id)
                .unwrap();
            assert_eq!(opened.meta.id, chapter.id);
            assert_eq!(opened.content, *expected_content);
        }
    }

    #[test]
    fn restore_project_tree_project_id_conflict_returns_err_and_no_partial() {
        let (_dir, api) = make_api();

        // 先创建一个项目（标题和结构都与 input 不同）
        let existing = api.create_project("已有作品").unwrap();

        // 尝试用相同 project_id 但不同内容恢复 → 应报冲突
        let mut input = make_full_input();
        input.project_id = existing.id.clone();

        let err = api.restore_project_tree(&input).unwrap_err();
        assert!(
            matches!(err, WriterError::Other(ref msg) if msg.contains("differs from restore input")),
            "expected 'differs from restore input' error, got: {:?}",
            err,
        );

        // 验证原有项目仍然完好（没有被破坏）
        let projects = api.list_projects().unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].id, existing.id);

        // 验证原有项目的卷仍然存在（create_project 会创建默认卷）
        let volumes = api.list_volumes(&existing.id).unwrap();
        assert!(!volumes.is_empty());
    }

    #[test]
    fn restore_project_tree_idempotent_when_project_matches() {
        let (_dir, api) = make_api();
        let input = make_full_input();

        // 第一次恢复：成功创建
        let first = api.restore_project_tree(&input).unwrap();
        assert_eq!(first.id, input.project_id);

        // 第二次恢复：内容完全一致 → 幂等返回，不报错
        let second = api.restore_project_tree(&input).unwrap();
        assert_eq!(second.id, input.project_id);
        assert_eq!(second.title, input.title);

        // 验证项目仍然只有一个（没有重复创建）
        let projects = api.list_projects().unwrap();
        assert_eq!(projects.len(), 1);
    }

    #[test]
    fn restore_project_tree_non_idempotent_when_content_differs() {
        let (_dir, api) = make_api();
        let input = make_full_input();

        // 第一次恢复：成功创建
        api.restore_project_tree(&input).unwrap();

        // 第二次恢复：同一 project_id 但 title 不同 → 冲突
        let mut input2 = input.clone();
        input2.title = "不同标题".to_string();

        let err = api.restore_project_tree(&input2).unwrap_err();
        assert!(
            matches!(err, WriterError::Other(ref msg) if msg.contains("differs from restore input")),
            "expected 'differs from restore input' error for title mismatch, got: {:?}",
            err,
        );
    }

    #[test]
    fn restore_project_tree_duplicate_volume_id_returns_err_and_rolls_back() {
        let (_dir, api) = make_api();

        let dup_id = new_uuid();
        let mut input = make_full_input();
        // 让两个卷用同一个 volume_id
        input.volumes[0].volume_id = dup_id.clone();
        input.volumes[1].volume_id = dup_id.clone();

        let err = api.restore_project_tree(&input).unwrap_err();
        assert!(matches!(err, WriterError::Other(ref msg) if msg.contains("duplicate volume_id")));

        // 验证没有留下半成品：项目不应存在
        let projects = api.list_projects().unwrap();
        assert!(
            projects.is_empty(),
            "no partial project should remain after rollback"
        );
    }

    #[test]
    fn restore_project_tree_duplicate_chapter_id_returns_err_and_rolls_back() {
        let (_dir, api) = make_api();

        let dup_id = new_uuid();
        let mut input = make_full_input();
        // 让同一卷下两个章节用同一个 chapter_id
        input.volumes[0].chapters[0].chapter_id = dup_id.clone();
        input.volumes[0].chapters[1].chapter_id = dup_id.clone();

        let err = api.restore_project_tree(&input).unwrap_err();
        assert!(matches!(err, WriterError::Other(ref msg) if msg.contains("duplicate chapter_id")));

        // 验证没有留下半成品
        let projects = api.list_projects().unwrap();
        assert!(
            projects.is_empty(),
            "no partial project should remain after rollback"
        );
    }

    #[test]
    fn restore_project_tree_empty_volumes_creates_project_only() {
        let (_dir, api) = make_api();

        let project_id = new_uuid();
        let input = RestoreProjectInputDto {
            project_id: project_id.clone(),
            title: "空作品".to_string(),
            order: 0,
            volumes: vec![],
        };

        let result = api.restore_project_tree(&input).unwrap();
        assert_eq!(result.id, project_id);
        assert_eq!(result.title, "空作品");

        // 验证项目存在但无卷
        let volumes = api.list_volumes(&project_id).unwrap();
        assert!(volumes.is_empty(), "no volumes should exist");
    }

    #[test]
    fn restore_project_tree_volume_with_empty_chapters() {
        let (_dir, api) = make_api();

        let project_id = new_uuid();
        let vol_id = new_uuid();
        let input = RestoreProjectInputDto {
            project_id: project_id.clone(),
            title: "空卷作品".to_string(),
            order: 0,
            volumes: vec![RestoreVolumeInputDto {
                volume_id: vol_id.clone(),
                title: "空卷".to_string(),
                order: 0,
                chapters: vec![],
            }],
        };

        let result = api.restore_project_tree(&input).unwrap();
        assert_eq!(result.id, project_id);

        // 验证卷存在但无章节
        let volumes = api.list_volumes(&project_id).unwrap();
        assert_eq!(volumes.len(), 1);
        assert_eq!(volumes[0].id, vol_id);

        let chapters = api.list_chapters(&project_id, &vol_id).unwrap();
        assert!(chapters.is_empty(), "no chapters should exist");
    }

    #[test]
    fn restore_project_tree_empty_project_id_returns_err() {
        let (_dir, api) = make_api();

        let mut input = make_full_input();
        input.project_id = "".to_string();

        let err = api.restore_project_tree(&input).unwrap_err();
        assert!(
            matches!(err, WriterError::Other(ref msg) if msg.contains("project_id must not be empty"))
        );
    }

    #[test]
    fn restore_project_tree_invalid_project_id_format_returns_err() {
        let (_dir, api) = make_api();

        let mut input = make_full_input();
        input.project_id = "not-a-uuid".to_string();

        let err = api.restore_project_tree(&input).unwrap_err();
        assert!(
            matches!(err, WriterError::Other(ref msg) if msg.contains("invalid project_id format"))
        );
    }

    #[test]
    fn restore_project_tree_invalid_volume_id_format_returns_err() {
        let (_dir, api) = make_api();

        let mut input = make_full_input();
        input.volumes[0].volume_id = "bad-volume-id".to_string();

        let err = api.restore_project_tree(&input).unwrap_err();
        assert!(
            matches!(err, WriterError::Other(ref msg) if msg.contains("invalid volume_id format"))
        );
    }

    #[test]
    fn restore_project_tree_invalid_chapter_id_format_returns_err() {
        let (_dir, api) = make_api();

        let mut input = make_full_input();
        input.volumes[0].chapters[0].chapter_id = "bad-chapter-id".to_string();

        let err = api.restore_project_tree(&input).unwrap_err();
        assert!(
            matches!(err, WriterError::Other(ref msg) if msg.contains("invalid chapter_id format"))
        );
    }

    /// 验证 build_restore_workspace_change_set 包含
    /// 空正文章节的 chapter.md。
    #[test]
    fn build_restore_change_set_includes_chapter_md_for_empty_content() {
        let input = make_full_input();
        // make_full_input 的第四章 content 为空
        assert!(
            input.volumes[1].chapters[1].content.is_empty(),
            "test precondition: chapter 4 content should be empty"
        );

        let cs = WriterCoreApi::build_restore_workspace_change_set(&input);

        // 展开所有 Upsert 路径
        let paths: Vec<std::path::PathBuf> = cs
            .changes
            .iter()
            .filter_map(|c| match c {
                crate::storage::workspace_git::WorkspaceHistoryChange::Upsert(p) => Some(p.clone()),
                _ => None,
            })
            .collect();

        // 每个章节应该有 chapter.meta.json 和 chapter.md
        for vol in &input.volumes {
            for ch in &vol.chapters {
                let meta_path = std::path::PathBuf::from("projects")
                    .join(&input.project_id)
                    .join("volumes")
                    .join(&vol.volume_id)
                    .join("chapters")
                    .join(&ch.chapter_id)
                    .join("chapter.meta.json");
                let content_path = std::path::PathBuf::from("projects")
                    .join(&input.project_id)
                    .join("volumes")
                    .join(&vol.volume_id)
                    .join("chapters")
                    .join(&ch.chapter_id)
                    .join("chapter.md");
                assert!(
                    paths.contains(&meta_path),
                    "change set should contain {}",
                    meta_path.display()
                );
                assert!(
                    paths.contains(&content_path),
                    "change set should contain {} (even for empty content)",
                    content_path.display()
                );
            }
        }
    }
}
