use super::*;

impl WriterCoreApi {
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
    pub(crate) fn build_restore_workspace_change_set(
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
