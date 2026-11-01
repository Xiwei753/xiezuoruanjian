//! Core-owned adapter methods for the external MCP server.
//!
//! The MCP binary only performs schema parsing and routing. Chapter paging,
//! chapter-id validation, UTF-8 boundaries and anchor persistence stay in Core
//! so that the future in-app AI entrypoint can reuse the same capability.

use std::collections::{HashSet, VecDeque};

use crate::api::service::{ApiResult, WriterCoreApi};
use crate::api::types::{
    AnnotationTextAnchorDto, CreateStarMapChildEmbedResultDto, McpChapterListItemDto,
    McpChapterReadDto, ProjectDto, StarMapMetaDto, StarMapPathSegmentDto, StarMapPointDto,
};
use crate::api::WriterError;

const DEFAULT_CHAPTER_PAGE_CHARS: usize = 8_000;
const MAX_CHAPTER_PAGE_CHARS: usize = 50_000;

fn invalid(message: impl Into<String>) -> WriterError {
    WriterError::Other(message.into())
}

fn char_to_byte(text: &str, char_index: usize) -> usize {
    text.char_indices()
        .nth(char_index)
        .map(|(byte, _)| byte)
        .unwrap_or(text.len())
}

impl WriterCoreApi {
    /// List chapters (optionally across every project) with their volume
    /// identity, so an MCP client can navigate without reading body text.
    pub fn mcp_list_chapters(
        &self,
        project_id: Option<&str>,
    ) -> ApiResult<Vec<McpChapterListItemDto>> {
        let projects: Vec<ProjectDto> = match project_id {
            Some(id) => self
                .list_projects()?
                .into_iter()
                .filter(|project| project.id == id)
                .collect(),
            None => self.list_projects()?,
        };
        if project_id.is_some() && projects.is_empty() {
            return Err(invalid("project does not exist"));
        }

        let mut result = Vec::new();
        for project in projects {
            for volume in self.list_volumes(&project.id)? {
                for chapter in self.list_chapters(&project.id, &volume.id)? {
                    result.push(McpChapterListItemDto {
                        project_id: project.id.clone(),
                        project_title: project.title.clone(),
                        volume_id: volume.id.clone(),
                        volume_title: volume.title.clone(),
                        chapter_id: chapter.id,
                        title: chapter.title,
                        order: chapter.order,
                        word_count: chapter.word_count,
                        hash: chapter.hash,
                        updated_at: chapter.updated_at,
                    });
                }
            }
        }
        Ok(result)
    }

    /// Read a chapter by character range. Character offsets are used instead of
    /// raw bytes so callers never produce a split UTF-8 sequence.
    pub fn mcp_read_chapter(
        &self,
        project_id: &str,
        chapter_id: &str,
        volume_id: Option<&str>,
        offset_chars: Option<usize>,
        max_chars: Option<usize>,
    ) -> ApiResult<McpChapterReadDto> {
        let location = self.mcp_find_chapter(project_id, chapter_id, volume_id)?;
        let chapter = self.open_chapter(project_id, &location.0, chapter_id)?;
        let total_chars = chapter.content.chars().count();
        let offset = offset_chars.unwrap_or(0);
        if offset > total_chars {
            return Err(invalid("offset_chars is past the end of the chapter"));
        }
        let limit = max_chars
            .unwrap_or(DEFAULT_CHAPTER_PAGE_CHARS)
            .clamp(1, MAX_CHAPTER_PAGE_CHARS);
        let end = offset.saturating_add(limit).min(total_chars);
        let start_byte = char_to_byte(&chapter.content, offset);
        let end_byte = char_to_byte(&chapter.content, end);
        Ok(McpChapterReadDto {
            project_id: project_id.to_string(),
            volume_id: location.0,
            chapter_id: chapter_id.to_string(),
            title: chapter.meta.title,
            hash: chapter.meta.hash,
            word_count: chapter.meta.word_count,
            total_chars,
            offset_chars: offset,
            end_chars: end,
            next_offset_chars: (end < total_chars).then_some(end),
            content: chapter.content[start_byte..end_byte].to_string(),
        })
    }

    /// Persist a chapter text anchor. Core re-reads the saved chapter body and
    /// validates the UTF-8 byte range; the chapter text file is never modified.
    pub fn mcp_create_chapter_text_anchor(
        &self,
        project_id: &str,
        chapter_id: &str,
        start_byte: u32,
        end_byte: u32,
        expected_hash: Option<&str>,
    ) -> ApiResult<AnnotationTextAnchorDto> {
        let (volume_id, _) = self.mcp_find_chapter(project_id, chapter_id, None)?;
        let chapter = self.open_chapter(project_id, &volume_id, chapter_id)?;
        if expected_hash.is_some_and(|expected| expected != chapter.meta.hash) {
            return Err(invalid(
                "chapter body changed since it was read; re-read before creating the anchor",
            ));
        }
        self.create_text_anchor(
            project_id.to_string(),
            chapter_id.to_string(),
            start_byte,
            end_byte,
            0,
            chapter.content,
        )
    }

    /// 读取本机工作区对象变更事件，供桌面 UI 等多进程客户端增量刷新。
    pub fn workspace_changes_since(
        &self,
        offset: u64,
    ) -> ApiResult<crate::storage::workspace_events::WorkspaceChangePage> {
        crate::storage::workspace_events::read_since(&self.app_data_root, offset)
            .map_err(WriterError::from)
    }

    // ---------------------------------------------------------------- MCP scope

    /// 按根星图与路径段解析 MCP 会话真正活动的星图。
    ///
    /// Agent 提供的裸 `starmapId` 只作为请求参数，当前层身份始终由
    /// root + 路径逐段解析；解析失败直接报错，不回退到请求 ID。
    pub fn mcp_resolve_active_starmap(
        &self,
        root_starmap_id: &str,
        segments: Vec<StarMapPathSegmentDto>,
    ) -> ApiResult<String> {
        self.get_starmap(root_starmap_id)?;
        let resolved = self.resolve_starmap_path(root_starmap_id, segments)?;
        Ok(resolved.final_starmap_id)
    }

    /// 根星图及其所有 Embed 可达子图的 ID 集合。用于限制 MCP 只能读写
    /// 当前会话授权范围内的图；循环 Embed 通过 visited 集合去重。
    pub fn mcp_starmap_subtree_ids(&self, root_starmap_id: &str) -> ApiResult<Vec<String>> {
        let mut ids = Vec::new();
        let mut visited = HashSet::new();
        let mut queue = VecDeque::from([root_starmap_id.to_string()]);
        while let Some(starmap_id) = queue.pop_front() {
            if !visited.insert(starmap_id.clone()) {
                continue;
            }
            match self.get_starmap_graph(&starmap_id) {
                Ok(graph) => {
                    ids.push(starmap_id);
                    for embed in graph.embeds {
                        queue.push_back(embed.target_starmap_id);
                    }
                }
                Err(error) if starmap_id == root_starmap_id => return Err(error),
                Err(error) => {
                    // 子图缺失不扩大授权范围；保留根范围，跳过失效分支。
                    log::warn!(
                        "mcp_starmap_subtree_ids: skip unreadable child starmap {}: {}",
                        starmap_id,
                        error
                    );
                }
            }
        }
        Ok(ids)
    }

    /// 校验 MCP 请求的目标星图是否属于会话授权范围。
    ///
    /// 授权规则：
    /// - 目标绑定到会话已授权作品时允许；
    /// - 目标未绑定作品（嵌套子图）时必须位于当前根星图及其 Embed 子树；
    /// - 目标绑定到其他作品时一律拒绝。
    pub fn mcp_validate_starmap_target(
        &self,
        project_id: Option<&str>,
        root_starmap_id: Option<&str>,
        segments: Vec<StarMapPathSegmentDto>,
        target_starmap_id: &str,
    ) -> ApiResult<String> {
        if target_starmap_id.trim().is_empty() {
            return Err(invalid("starmapId is required"));
        }
        if let Some(project_id) = project_id {
            if !self
                .list_projects()?
                .iter()
                .any(|project| project.id == project_id)
            {
                return Err(invalid("project does not exist"));
            }
        }
        let target = self.get_starmap(target_starmap_id)?;
        if let Some(project_id) = project_id {
            if target
                .project_id
                .as_deref()
                .is_some_and(|bound| bound != project_id)
            {
                return Err(invalid(
                    "target starmap belongs to a project outside this MCP scope",
                ));
            }
        } else if target.project_id.is_some() {
            return Err(invalid(
                "target starmap belongs to a project outside this MCP scope",
            ));
        }

        match root_starmap_id {
            Some(root_starmap_id) => {
                let root = self.get_starmap(root_starmap_id)?;
                if let Some(project_id) = project_id {
                    if root
                        .project_id
                        .as_deref()
                        .is_some_and(|bound| bound != project_id)
                    {
                        return Err(invalid(
                            "root starmap belongs to a project outside this MCP scope",
                        ));
                    }
                } else if root.project_id.is_some() {
                    return Err(invalid(
                        "root starmap belongs to a project outside this MCP scope",
                    ));
                }
                // 路径必须能解析；这里不信任 Agent 提供的 activeStarmapId。
                let _active = self.mcp_resolve_active_starmap(root_starmap_id, segments)?;
                // 画布会话的写范围就是当前根图及其 Embed 子树。同一作品下
                // 另一张无关星图也必须显式切换画布后才能操作，避免 Agent
                // 继续拿上一张图的裸 starmapId 写入错误目标。
                let ids = self.mcp_starmap_subtree_ids(root_starmap_id)?;
                if !ids.iter().any(|id| id == target_starmap_id) {
                    return Err(invalid(
                        "target starmap is outside the current root starmap subtree",
                    ));
                }
            }
            None => {
                if project_id.is_none() {
                    return Err(invalid(
                        "MCP session has no authorized project or starmap scope",
                    ));
                }
            }
        }
        Ok(target_starmap_id.to_string())
    }

    /// 会话授权范围内的星图元数据。项目范围内的图与当前根子树取并集，
    /// 供 `list_starmaps` 和写审计确定真实目标。
    pub fn mcp_list_authorized_starmaps(
        &self,
        project_id: Option<&str>,
        root_starmap_id: Option<&str>,
    ) -> ApiResult<Vec<StarMapMetaDto>> {
        let mut result: Vec<StarMapMetaDto> = Vec::new();
        let mut seen = HashSet::new();
        if let Some(root_starmap_id) = root_starmap_id {
            // 画布会话只暴露当前根图子树，不把同作品下其他无关星图
            // 混进可选列表，避免模型凭列表猜测写入目标。
            let root = self.get_starmap(root_starmap_id)?;
            if let Some(project_id) = project_id {
                if root
                    .project_id
                    .as_deref()
                    .is_some_and(|bound| bound != project_id)
                {
                    return Err(invalid(
                        "root starmap belongs to a project outside this MCP scope",
                    ));
                }
            } else if root.project_id.is_some() {
                return Err(invalid(
                    "root starmap belongs to a project outside this MCP scope",
                ));
            }
            for starmap_id in self.mcp_starmap_subtree_ids(root_starmap_id)? {
                let meta = self.get_starmap(&starmap_id)?;
                if seen.insert(starmap_id.clone()) {
                    result.push(meta);
                }
            }
        } else if let Some(project_id) = project_id {
            if !self
                .list_projects()?
                .iter()
                .any(|project| project.id == project_id)
            {
                return Err(invalid("project does not exist"));
            }
            for meta in self.list_starmaps_for_project(project_id)? {
                if seen.insert(meta.starmap_id.clone()) {
                    result.push(meta);
                }
            }
        } else {
            return Err(invalid(
                "MCP session has no authorized project or starmap scope",
            ));
        }
        Ok(result)
    }

    /// 在已授权作品内创建一级星图并绑定到作品，整个过程处于同一个 Core
    /// 写锁和来源事务中，返回真实落盘后的 meta（含 projectId）。
    pub fn mcp_create_starmap_in_project(
        &self,
        project_id: &str,
        title: &str,
        description: &str,
        template_id: Option<&str>,
    ) -> ApiResult<StarMapMetaDto> {
        if title.trim().is_empty() {
            return Err(invalid("starmap title is required"));
        }
        if !self
            .list_projects()?
            .iter()
            .any(|project| project.id == project_id)
        {
            return Err(invalid("project does not exist"));
        }
        let _workspace_write_lock =
            crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)?;
        let mut contribution_batch = self.begin_contribution_batch();
        let starmap_id = format!("sm_{}", uuid::Uuid::new_v4());
        let mut contribution_txn =
            self.begin_starmap_creation_contribution_transaction(&starmap_id)?;
        let history_token = crate::storage::journal::workspace_change::new_token();
        self.register_starmap_history_journal(&mut contribution_txn, &history_token)?;
        let (created, change_set) = self
            .core_write()
            .create_starmap_with_id_with_changes(&starmap_id, title, description, template_id)
            .map_err(WriterError::from)?;
        // 新建图的 meta.projectId 为空；在同一事务内绑定，避免 AI 创建的图
        // 成为不属于任何作品、用户也看不到的孤立图。
        self.core_write()
            .bind_starmap_to_project(&created.starmap_id, project_id)
            .map_err(WriterError::from)?;
        let meta = self
            .core_write()
            .get_starmap(&created.starmap_id)
            .map_err(WriterError::from)?;
        let history_journal = self.save_starmap_history_journal(
            &history_token,
            &change_set,
            crate::storage::journal::workspace_change::WorkspaceChangeOpType::CreateStarMap,
        )?;
        let entry = crate::search::extractor::extract_starmap_title_entry(
            &meta.starmap_id,
            Some(project_id),
            &meta.title,
        );
        self.enqueue_search_index_update(crate::search::SearchIndexUpdate {
            action: crate::search::SearchIndexAction::Upsert,
            object_id: entry.object_id.clone(),
            scope: entry.scope,
            title: entry.title.clone(),
            body: entry.body.clone(),
            target: Some(entry.target.clone()),
        });
        let context = crate::writing_stats::current_mutation_context();
        self.record_contribution_event_with_context(
            &context,
            project_id,
            crate::writing_stats::ContributionObjectType::StarMap,
            &meta.starmap_id,
            crate::writing_stats::ContributionOperation::Create,
            None,
            None,
            Some(Self::starmap_meta_snapshot(&meta)),
        )?;
        // 模板可能已经生成节点/边/嵌入/链接，逐个记录来源，保持与
        // `create_starmap` 相同的统计口径。
        if let Some(graph) = self.get_starmap_graph_snapshot(&meta.starmap_id) {
            for node in &graph.nodes {
                self.record_starmap_object_with_context(
                    &context,
                    Some(project_id),
                    &meta.starmap_id,
                    crate::writing_stats::ContributionObjectType::StarMapNode,
                    &node.id,
                    crate::writing_stats::ContributionOperation::Create,
                    None,
                    Some(&graph),
                )?;
            }
            for edge in &graph.edges {
                self.record_starmap_object_with_context(
                    &context,
                    Some(project_id),
                    &meta.starmap_id,
                    crate::writing_stats::ContributionObjectType::StarMapEdge,
                    &edge.id,
                    crate::writing_stats::ContributionOperation::Create,
                    None,
                    Some(&graph),
                )?;
            }
            for embed in &graph.embeds {
                self.record_starmap_object_with_context(
                    &context,
                    Some(project_id),
                    &meta.starmap_id,
                    crate::writing_stats::ContributionObjectType::StarMapEmbed,
                    &embed.instance_id,
                    crate::writing_stats::ContributionOperation::Create,
                    None,
                    Some(&graph),
                )?;
            }
            for link in &graph.links {
                self.record_starmap_object_with_context(
                    &context,
                    Some(project_id),
                    &meta.starmap_id,
                    crate::writing_stats::ContributionObjectType::StarMapLink,
                    &link.link_id,
                    crate::writing_stats::ContributionOperation::Create,
                    None,
                    Some(&graph),
                )?;
            }
        }
        self.commit_contribution_batch(&mut contribution_batch)?;
        self.finish_starmap_contribution_transaction(&meta.starmap_id, &mut contribution_txn)?;
        if let Err(error) = history_journal.mark_local_applied(&self.app_data_root) {
            log::warn!(
                "mcp_create_starmap_in_project: mark_local_applied failed for {}: {} — journal retained",
                history_journal.token,
                error
            );
        }
        self.finish_starmap_history_journal(&history_journal, "mcp_create_starmap_in_project");
        if let Err(error) = crate::storage::workspace_events::append_structure_event(
            &self.app_data_root,
            &meta.starmap_id,
            "starmap",
            &meta.starmap_id,
        ) {
            log::warn!("failed to append starmap structure event: {error}");
        }
        Ok(meta.into())
    }

    /// 校验宿主图属于会话范围后，在同一事务里创建子图并嵌入宿主图。
    pub fn mcp_create_starmap_child_embed_scoped(
        &self,
        project_id: Option<&str>,
        root_starmap_id: Option<&str>,
        segments: Vec<StarMapPathSegmentDto>,
        host_starmap_id: &str,
        title: &str,
        position: StarMapPointDto,
    ) -> ApiResult<CreateStarMapChildEmbedResultDto> {
        let _workspace_write_lock =
            crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)?;
        let host = self.mcp_validate_starmap_target(
            project_id,
            root_starmap_id,
            segments,
            host_starmap_id,
        )?;
        let result = self.create_starmap_child_embed(&host, title, position)?;
        Ok(result)
    }

    /// 把已授权范围内的星图绑定到授权作品；来源与目标都必须通过范围校验。
    pub fn mcp_bind_starmap_scoped(
        &self,
        project_id: &str,
        root_starmap_id: Option<&str>,
        segments: Vec<StarMapPathSegmentDto>,
        starmap_id: &str,
    ) -> ApiResult<bool> {
        let _workspace_write_lock =
            crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)?;
        let target = self.mcp_validate_starmap_target(
            Some(project_id),
            root_starmap_id,
            segments,
            starmap_id,
        )?;
        let result = self.bind_starmap_to_project(&target, project_id)?;
        Ok(result)
    }

    /// 把授权作品内的星图解绑；不允许解绑其他作品或范围外的图。
    pub fn mcp_unbind_starmap_scoped(
        &self,
        project_id: &str,
        root_starmap_id: Option<&str>,
        segments: Vec<StarMapPathSegmentDto>,
        starmap_id: &str,
    ) -> ApiResult<bool> {
        let _workspace_write_lock =
            crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)?;
        let target = self.mcp_validate_starmap_target(
            Some(project_id),
            root_starmap_id,
            segments,
            starmap_id,
        )?;
        let result = self.unbind_starmap_from_project(&target)?;
        Ok(result)
    }

    /// 按会话作品/星图范围过滤全局搜索结果，避免 MCP 读到其他作品。
    pub fn mcp_search_json_scoped(
        &self,
        query: &str,
        scope: &str,
        limit: u32,
        cursor: Option<&str>,
        project_id: Option<&str>,
        authorized_starmap_ids: &[String],
    ) -> ApiResult<String> {
        let raw = self.global_search_json(query, scope, limit, cursor)?;
        let results: Vec<crate::search::SearchResult> = serde_json::from_str(&raw)
            .map_err(|error| invalid(format!("invalid search response: {error}")))?;
        let allowed: HashSet<&str> = authorized_starmap_ids.iter().map(String::as_str).collect();
        let filtered: Vec<crate::search::SearchResult> = results
            .into_iter()
            .filter(|result| {
                let target = &result.target;
                if let Some(project_id) = project_id {
                    match target.project_id.as_deref() {
                        Some(bound) => bound == project_id,
                        None => target
                            .starmap_id
                            .as_deref()
                            .is_some_and(|id| allowed.contains(id)),
                    }
                } else {
                    target.project_id.is_none()
                        && target
                            .starmap_id
                            .as_deref()
                            .is_some_and(|id| allowed.contains(id))
                }
            })
            .collect();
        Self::json_string(&filtered)
    }

    fn mcp_find_chapter(
        &self,
        project_id: &str,
        chapter_id: &str,
        volume_id: Option<&str>,
    ) -> ApiResult<(String, String)> {
        if !self
            .list_projects()?
            .iter()
            .any(|project| project.id == project_id)
        {
            return Err(invalid("project does not exist"));
        }
        let volumes = self.list_volumes(project_id)?;
        for volume in volumes {
            if volume_id.is_some_and(|requested| requested != volume.id) {
                continue;
            }
            let chapter = self
                .list_chapters(project_id, &volume.id)?
                .into_iter()
                .find(|chapter| chapter.id == chapter_id);
            if let Some(chapter) = chapter {
                return Ok((volume.id, chapter.title));
            }
        }
        Err(invalid("chapter does not exist in this project"))
    }
}
