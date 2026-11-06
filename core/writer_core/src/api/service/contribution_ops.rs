//! 对象贡献事件的业务层记录器。
//!
//! 所有星图/注释写操作在 Core 持久化成功后调用这里。调用点不自行拼统计
//! 数字，也不允许平台端上报 actor；actor/entrypoint 来自当前业务入口的
//! [`MutationContext`]。事件先写入 pending journal，再追加到正式分区；
//! 记录失败会作为业务 API 错误返回，不再静默丢失来源。业务写入失败时不会
//! 留下任何事件。

use std::cell::RefCell;

use crate::api::service::WriterCoreApi;
use crate::starmap::semantic::StarMapNodeContent;
use crate::starmap::types::StarMapGraph;
use crate::starmap::StarMapMeta;
use crate::writing_stats::contribution::{
    current_mutation_context, ContributionEvent, ContributionObjectSnapshot,
    ContributionObjectType, ContributionOperation, CurrentTextSnapshot, MutationContext,
};
use crate::writing_stats::starmap_checkpoint::has_starmap_checkpoints;
use crate::writing_stats::starmap_proof::{
    StarMapProofBaseline, StarMapProofDelta, StarMapProofTrajectorySample,
};

thread_local! {
    static ACTIVE_BATCH: RefCell<Option<Vec<ContributionEvent>>> = const { RefCell::new(None) };
}

/// 一次业务事务内的来源事件批次。成功提交时整批写入 pending journal；
/// 业务返回错误时 [`Drop`] 丢弃，绝不留下假事件。
pub(crate) struct ContributionBatch {
    owns_batch: bool,
}

impl Drop for ContributionBatch {
    fn drop(&mut self) {
        if self.owns_batch {
            ACTIVE_BATCH.with(|slot| {
                slot.borrow_mut().take();
            });
        }
    }
}

impl WriterCoreApi {
    pub(crate) fn begin_contribution_batch(&self) -> ContributionBatch {
        let owns_batch = ACTIVE_BATCH.with(|slot| {
            let mut slot = slot.borrow_mut();
            if slot.is_some() {
                false
            } else {
                *slot = Some(Vec::new());
                true
            }
        });
        ContributionBatch { owns_batch }
    }

    pub(crate) fn commit_contribution_batch(
        &self,
        batch: &mut ContributionBatch,
    ) -> super::ApiResult<()> {
        if !batch.owns_batch {
            return Ok(());
        }
        batch.owns_batch = false;
        let events = ACTIVE_BATCH
            .with(|slot| slot.borrow_mut().take())
            .unwrap_or_default();
        self.record_contribution_events(events)
    }

    pub(crate) fn record_contribution_event(
        &self,
        project_id: &str,
        object_type: ContributionObjectType,
        object_id: &str,
        operation: ContributionOperation,
        container_id: Option<String>,
        before: Option<ContributionObjectSnapshot>,
        after: Option<ContributionObjectSnapshot>,
    ) -> super::ApiResult<()> {
        let context = current_mutation_context();
        self.record_contribution_event_with_context(
            &context,
            project_id,
            object_type,
            object_id,
            operation,
            container_id,
            before,
            after,
        )
    }

    /// 同一业务事务里的多个对象变更必须共享事务 ID。
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn record_contribution_event_with_context(
        &self,
        context: &MutationContext,
        project_id: &str,
        object_type: ContributionObjectType,
        object_id: &str,
        operation: ContributionOperation,
        container_id: Option<String>,
        before: Option<ContributionObjectSnapshot>,
        after: Option<ContributionObjectSnapshot>,
    ) -> super::ApiResult<()> {
        self.record_contribution_event_with_context_and_delta(
            context,
            project_id,
            object_type,
            object_id,
            operation,
            container_id,
            before,
            after,
            None,
        )
    }

    /// 记录一条对象事件，并可附带星图回放增量。
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn record_contribution_event_with_context_and_delta(
        &self,
        context: &MutationContext,
        project_id: &str,
        object_type: ContributionObjectType,
        object_id: &str,
        operation: ContributionOperation,
        container_id: Option<String>,
        before: Option<ContributionObjectSnapshot>,
        after: Option<ContributionObjectSnapshot>,
        replay_delta: Option<StarMapProofDelta>,
    ) -> super::ApiResult<()> {
        let mut event = ContributionEvent::new(
            context,
            project_id,
            object_type,
            object_id,
            operation,
            container_id,
            before,
            after,
        );
        let batched = ACTIVE_BATCH.with(|slot| slot.borrow().is_some());
        if batched {
            let order = ACTIVE_BATCH.with(|slot| {
                slot.borrow()
                    .as_ref()
                    .map(|events| {
                        events
                            .iter()
                            .filter(|event| event.replay_delta.is_some())
                            .count() as u32
                    })
                    .unwrap_or(0)
            });
            if let Some(delta) = replay_delta {
                event.attach_replay(delta, Some(order));
            }
            ACTIVE_BATCH.with(|slot| {
                if let Some(events) = slot.borrow_mut().as_mut() {
                    events.push(event);
                }
            });
            return Ok(());
        }
        if let Some(delta) = replay_delta {
            event.attach_replay(delta, Some(0));
        }
        self.record_contribution_events(vec![event])
    }

    pub(crate) fn record_contribution_events(
        &self,
        events: Vec<ContributionEvent>,
    ) -> super::ApiResult<()> {
        if events.is_empty() {
            return Ok(());
        }
        if crate::writing_stats::contribution_transaction::queue_active_events(events.clone()) {
            return Ok(());
        }
        self.core_write()
            .get_stats_api()
            .record_contribution_events(events)
            .map(|_| ())
            .map_err(Into::into)
    }

    pub(crate) fn begin_starmap_contribution_transaction(
        &self,
        starmap_id: &str,
    ) -> super::ApiResult<crate::writing_stats::contribution_transaction::ContributionTransaction>
    {
        let _ = starmap_id;
        crate::writing_stats::contribution_transaction::ContributionTransaction::begin_deferred(
            &self.app_data_root,
        )
        .map_err(Into::into)
    }

    pub(crate) fn begin_starmaps_contribution_transaction(
        &self,
    ) -> super::ApiResult<crate::writing_stats::contribution_transaction::ContributionTransaction>
    {
        crate::writing_stats::contribution_transaction::ContributionTransaction::begin_deferred(
            &self.app_data_root,
        )
        .map_err(Into::into)
    }

    /// 子星图创建来源事务。
    ///
    /// 在写任何宿主文件前调用：子图 meta/index、宿主 Embed/graph meta、全局
    /// index、同步状态和子嵌入 journal 都进入同一份恢复清单。宿主 Embed 文件
    /// 路径由预生成的 `embed_instance_id` 确定；journal 路径在创建前登记为
    /// “不存在”，回滚会删除它，避免 journal 恢复再补写已撤销的宿主 Embed。
    pub(crate) fn begin_starmap_child_embed_contribution_transaction(
        &self,
        host_starmap_id: &str,
        child_starmap_id: &str,
        embed_instance_id: &str,
        journal_tx_id: &str,
    ) -> super::ApiResult<crate::writing_stats::contribution_transaction::ContributionTransaction>
    {
        let starmaps = self.app_data_root.join("starmaps");
        let host_embed_path =
            self.app_data_root
                .join(crate::starmap::package_storage::embed_rel_path(
                    host_starmap_id,
                    embed_instance_id,
                ));
        let host_graph_meta_path =
            self.app_data_root
                .join(crate::starmap::package_storage::graph_meta_rel_path(
                    host_starmap_id,
                ));
        let child_journal_path = crate::storage::journal::starmap_child_embed::journal_file_path(
            &self.app_data_root,
            journal_tx_id,
        );
        crate::writing_stats::contribution_transaction::ContributionTransaction::begin(
            &self.app_data_root,
            vec![
                // 先撤销子嵌入 journal：回滚中途崩溃时，后续恢复不会把已撤销的
                // 宿主 Embed 当成“journal 已记录、待补 history”再补回来。
                child_journal_path,
                starmaps.join(child_starmap_id),
                starmaps.join(format!("{child_starmap_id}.meta.json")),
                starmaps.join("index.json"),
                self.app_data_root.join("app-meta/sync/state.local.json"),
                host_embed_path,
                host_graph_meta_path,
            ],
        )
        .map_err(Into::into)
    }

    pub(crate) fn begin_starmap_creation_contribution_transaction(
        &self,
        starmap_id: &str,
    ) -> super::ApiResult<crate::writing_stats::contribution_transaction::ContributionTransaction>
    {
        let starmaps = self.app_data_root.join("starmaps");
        crate::writing_stats::contribution_transaction::ContributionTransaction::begin(
            &self.app_data_root,
            vec![
                starmaps.join(starmap_id),
                starmaps.join(format!("{starmap_id}.meta.json")),
                starmaps.join("index.json"),
                self.app_data_root.join("app-meta/sync/state.local.json"),
            ],
        )
        .map_err(Into::into)
    }

    /// 删除星图会在事务提交前物理移除 graph 目录，因此必须在删除前把
    /// 将被删除的路径登记到预备事务里，不能依赖 flush 时的 dirty 文件列表。
    pub(crate) fn begin_starmap_delete_contribution_transaction(
        &self,
        starmap_id: &str,
    ) -> super::ApiResult<crate::writing_stats::contribution_transaction::ContributionTransaction>
    {
        let starmaps = self.app_data_root.join("starmaps");
        crate::writing_stats::contribution_transaction::ContributionTransaction::begin(
            &self.app_data_root,
            vec![
                starmaps.join(starmap_id),
                starmaps.join(format!("{starmap_id}.meta.json")),
                starmaps.join("index.json"),
                self.app_data_root.join("app-meta/sync/state.local.json"),
            ],
        )
        .map_err(Into::into)
    }

    pub(crate) fn begin_annotation_contribution_transaction(
        &self,
        project_id: &str,
    ) -> super::ApiResult<crate::writing_stats::contribution_transaction::ContributionTransaction>
    {
        crate::writing_stats::contribution_transaction::ContributionTransaction::begin(
            &self.app_data_root,
            vec![self.projects_root.join(project_id).join("annotations")],
        )
        .map_err(Into::into)
    }

    /// 从星图当前快照生成对象来源快照；不存在返回 `None`（删除后）。
    pub(crate) fn starmap_object_snapshot(
        graph: &StarMapGraph,
        object_type: ContributionObjectType,
        object_id: &str,
    ) -> Option<ContributionObjectSnapshot> {
        match object_type {
            ContributionObjectType::StarMapNode => graph
                .nodes
                .iter()
                .find(|node| node.id == object_id)
                .map(|node| {
                    ContributionObjectSnapshot::text(
                        node.updated_at.to_string(),
                        node.title.clone(),
                        Self::node_contributing_text(&node.title, &node.content),
                    )
                }),
            ContributionObjectType::StarMapEdge => graph
                .edges
                .iter()
                .find(|edge| edge.id == object_id)
                .map(|edge| {
                    ContributionObjectSnapshot::object(
                        edge.updated_at.to_string(),
                        edge.label.clone().unwrap_or_else(|| edge.id.clone()),
                    )
                }),
            ContributionObjectType::StarMapEmbed => graph
                .embeds
                .iter()
                .find(|embed| embed.instance_id == object_id)
                .map(|embed| {
                    ContributionObjectSnapshot::object(
                        embed.updated_at.to_string(),
                        embed
                            .label
                            .clone()
                            .unwrap_or_else(|| embed.instance_id.clone()),
                    )
                }),
            ContributionObjectType::StarMapLink => graph
                .links
                .iter()
                .find(|link| link.link_id == object_id)
                .map(|link| {
                    ContributionObjectSnapshot::object(
                        link.updated_at.to_string(),
                        link.label.clone().unwrap_or_else(|| link.link_id.clone()),
                    )
                }),
            _ => None,
        }
    }

    pub(crate) fn starmap_meta_snapshot(meta: &StarMapMeta) -> ContributionObjectSnapshot {
        ContributionObjectSnapshot::object(meta.updated_at.to_string(), meta.title.clone())
    }

    /// 记录一次星图对象写入。`before`/`after` 是同一对象的图快照，
    /// 节点文本差异由 [`ContributionEvent::new`] 计算；同时保存完整对象增量
    /// 供星图手写证明回放。
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn record_starmap_object_with_context(
        &self,
        context: &MutationContext,
        project_id: Option<&str>,
        starmap_id: &str,
        object_type: ContributionObjectType,
        object_id: &str,
        operation: ContributionOperation,
        before_graph: Option<&StarMapGraph>,
        after_graph: Option<&StarMapGraph>,
    ) -> super::ApiResult<()> {
        self.record_starmap_object_with_context_and_trajectory(
            context,
            project_id,
            starmap_id,
            object_type,
            object_id,
            operation,
            before_graph,
            after_graph,
            &[],
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn record_starmap_object_with_context_and_trajectory(
        &self,
        context: &MutationContext,
        project_id: Option<&str>,
        starmap_id: &str,
        object_type: ContributionObjectType,
        object_id: &str,
        operation: ContributionOperation,
        before_graph: Option<&StarMapGraph>,
        after_graph: Option<&StarMapGraph>,
        trajectory: &[StarMapProofTrajectorySample],
    ) -> super::ApiResult<()> {
        let before = before_graph
            .and_then(|graph| Self::starmap_object_snapshot(graph, object_type, object_id));
        let after = after_graph
            .and_then(|graph| Self::starmap_object_snapshot(graph, object_type, object_id));
        let revision = after
            .as_ref()
            .and_then(|snapshot| snapshot.version.parse::<u64>().ok())
            .unwrap_or(0);
        let mut delta = StarMapProofDelta::for_object(
            starmap_id,
            object_type,
            object_id,
            operation,
            before_graph,
            after_graph,
        )
        .with_trajectory(trajectory.to_vec());
        if let Some(baseline) = self.starmap_replay_baseline(
            starmap_id,
            before_graph,
            after_graph,
            object_type,
            object_id,
        ) {
            delta = delta.with_baseline(baseline);
        }
        let result = self.record_contribution_event_with_context_and_delta(
            context,
            project_id.unwrap_or_default(),
            object_type,
            object_id,
            operation,
            Some(starmap_id.to_string()),
            before,
            after,
            Some(delta),
        );
        if result.is_ok() {
            if let Err(error) = crate::storage::workspace_events::append_event(
                &self.app_data_root,
                "starmap",
                starmap_id,
                object_type.as_str(),
                object_id,
                revision,
            ) {
                log::warn!("failed to append workspace change event: {error}");
            }
        }
        result
    }

    /// 记录星图元数据操作（创建/重命名/删除/绑定项目等）。
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn record_starmap_meta_with_context(
        &self,
        context: &MutationContext,
        project_id: Option<&str>,
        starmap_id: &str,
        operation: ContributionOperation,
        before: Option<&StarMapMeta>,
        after: Option<&StarMapMeta>,
        roots_after: Option<Vec<String>>,
    ) -> super::ApiResult<()> {
        let before_snapshot = before.map(Self::starmap_meta_snapshot);
        let after_snapshot = after.map(Self::starmap_meta_snapshot);
        let mut delta = StarMapProofDelta::for_meta(starmap_id, operation, before, after);
        if let Some(roots) = roots_after {
            delta = delta.with_roots_after(roots);
        }
        if operation == ContributionOperation::Create {
            // 新星图从空白开始：位置 0 的空基线同时充当“已开始记录”标记，
            // 之后的普通写操作不会再误判为旧星图而补历史基线。
            let mut graph = StarMapGraph::default();
            graph.starmap_id = starmap_id.to_string();
            delta = delta.with_baseline(StarMapProofBaseline {
                meta: None,
                graph,
                root_starmap_ids: Vec::new(),
                history_complete: true,
            });
        } else if let Some(baseline) = self.starmap_meta_replay_baseline(starmap_id, before) {
            delta = delta.with_baseline(baseline);
        }
        self.record_contribution_event_with_context_and_delta(
            context,
            project_id.unwrap_or_default(),
            ContributionObjectType::StarMap,
            starmap_id,
            operation,
            None,
            before_snapshot,
            after_snapshot,
            Some(delta),
        )
    }

    /// 旧星图第一次进入回放账本时，从真实图快照生成基线。
    fn starmap_replay_baseline(
        &self,
        starmap_id: &str,
        before_graph: Option<&StarMapGraph>,
        after_graph: Option<&StarMapGraph>,
        object_type: ContributionObjectType,
        object_id: &str,
    ) -> Option<StarMapProofBaseline> {
        if starmap_created_in_active_batch(starmap_id) {
            return None;
        }
        let device_id = self
            .core_write()
            .get_stats_api()
            .contribution_store()
            .resolve_origin_device_id()
            .ok()?;
        if has_starmap_checkpoints(&self.app_data_root, &device_id, starmap_id) {
            return None;
        }
        let graph = match before_graph {
            Some(graph) => graph.clone(),
            None => {
                // 第一条操作就是新增对象：用写入后的真实图减去该对象，得到
                // 操作前的图状态，而不是把新对象提前放进基线。
                let mut graph = after_graph?.clone();
                match object_type {
                    ContributionObjectType::StarMapNode => {
                        graph.nodes.retain(|node| node.id != object_id);
                    }
                    ContributionObjectType::StarMapEdge => {
                        graph.edges.retain(|edge| edge.id != object_id);
                    }
                    ContributionObjectType::StarMapEmbed => {
                        graph.embeds.retain(|embed| embed.instance_id != object_id);
                    }
                    ContributionObjectType::StarMapLink => {
                        graph.links.retain(|link| link.link_id != object_id);
                    }
                    _ => return None,
                }
                graph
            }
        };
        let meta = self.core_write().get_starmap(starmap_id).ok();
        let root_starmap_ids = crate::starmap::load_index(&self.app_data_root)
            .map(|index| index.root_starmap_ids)
            .unwrap_or_default();
        Some(StarMapProofBaseline {
            meta,
            graph,
            root_starmap_ids,
            history_complete: false,
        })
    }

    fn starmap_meta_replay_baseline(
        &self,
        starmap_id: &str,
        before: Option<&StarMapMeta>,
    ) -> Option<StarMapProofBaseline> {
        if starmap_created_in_active_batch(starmap_id) {
            return None;
        }
        let device_id = self
            .core_write()
            .get_stats_api()
            .contribution_store()
            .resolve_origin_device_id()
            .ok()?;
        if has_starmap_checkpoints(&self.app_data_root, &device_id, starmap_id) {
            return None;
        }
        let graph = self.get_starmap_graph_snapshot(starmap_id)?;
        let root_starmap_ids = crate::starmap::load_index(&self.app_data_root)
            .map(|index| index.root_starmap_ids)
            .unwrap_or_default();
        Some(StarMapProofBaseline {
            meta: before.cloned(),
            graph,
            root_starmap_ids,
            history_complete: false,
        })
    }

    pub(crate) fn record_starmap_object(
        &self,
        project_id: Option<&str>,
        starmap_id: &str,
        object_type: ContributionObjectType,
        object_id: &str,
        operation: ContributionOperation,
        before_graph: Option<&StarMapGraph>,
        after_graph: Option<&StarMapGraph>,
    ) -> super::ApiResult<()> {
        let context = current_mutation_context();
        self.record_starmap_object_with_context(
            &context,
            project_id,
            starmap_id,
            object_type,
            object_id,
            operation,
            before_graph,
            after_graph,
        )
    }

    pub(crate) fn get_starmap_graph_snapshot(&self, starmap_id: &str) -> Option<StarMapGraph> {
        self.core_write().get_starmap_graph(starmap_id).ok()
    }

    /// 收集当前仍存在的星图节点文字快照。
    ///
    /// 快照包含节点标题和正文，带 project/container/version；统计投影用它
    /// 与事件历史对账，删除或外部改盘的内容不会被错误计入当前保留来源。
    pub(crate) fn collect_current_starmap_texts(
        &self,
        project_id: Option<&str>,
    ) -> super::ApiResult<Vec<CurrentTextSnapshot>> {
        let starmaps = self.list_starmaps()?;
        let mut snapshots = Vec::new();
        for starmap in starmaps {
            let starmap_project = starmap.project_id.clone().unwrap_or_default();
            if project_id.is_some_and(|value| value != starmap_project) {
                continue;
            }
            let Ok(graph) = self.get_starmap_graph(&starmap.starmap_id) else {
                continue;
            };
            for node in graph.nodes {
                let text = dto_node_contributing_text(&node.title, &node.content);
                if text.is_empty() {
                    continue;
                }
                snapshots.push(CurrentTextSnapshot {
                    project_id: starmap_project.clone(),
                    object_type: ContributionObjectType::StarMapNode,
                    object_id: node.id,
                    container_id: Some(starmap.starmap_id.clone()),
                    version: node.updated_at.to_string(),
                    text,
                });
            }
        }
        Ok(snapshots)
    }

    pub(crate) fn node_contributing_text(title: &str, content: &StarMapNodeContent) -> String {
        let mut text = String::new();
        text.push_str(title);
        if let StarMapNodeContent::Inline { summary, body } = content {
            if let Some(summary) = summary {
                text.push('\n');
                text.push_str(summary);
            }
            if let Some(body) = body {
                text.push('\n');
                text.push_str(body);
            }
        }
        text
    }

    /// 注释对象（标签、绑定、锚点、链接）事件。
    pub(crate) fn record_annotation_object(
        &self,
        project_id: &str,
        object_type: ContributionObjectType,
        object_id: &str,
        operation: ContributionOperation,
        before: Option<ContributionObjectSnapshot>,
        after: Option<ContributionObjectSnapshot>,
    ) -> super::ApiResult<()> {
        let context = current_mutation_context();
        self.record_annotation_object_with_context(
            &context,
            project_id,
            object_type,
            object_id,
            operation,
            before,
            after,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn record_annotation_object_with_context(
        &self,
        context: &MutationContext,
        project_id: &str,
        object_type: ContributionObjectType,
        object_id: &str,
        operation: ContributionOperation,
        before: Option<ContributionObjectSnapshot>,
        after: Option<ContributionObjectSnapshot>,
    ) -> super::ApiResult<()> {
        self.record_contribution_event_with_context(
            context,
            project_id,
            object_type,
            object_id,
            operation,
            None,
            before,
            after,
        )
    }

    pub(crate) fn annotation_snapshot(
        version: impl Into<String>,
        summary: impl Into<String>,
    ) -> ContributionObjectSnapshot {
        ContributionObjectSnapshot::object(version, summary)
    }
}

/// 当前批次的星图创建事件会从空白开始回放，不能再生成“旧图基线”。
fn starmap_created_in_active_batch(starmap_id: &str) -> bool {
    ACTIVE_BATCH.with(|slot| {
        slot.borrow().as_ref().is_some_and(|events| {
            events.iter().any(|event| {
                event.object_type == ContributionObjectType::StarMap
                    && event.object_id == starmap_id
                    && event.operation == ContributionOperation::Create
            })
        })
    })
}

fn dto_node_contributing_text(
    title: &str,
    content: &crate::api::types::StarMapNodeContentDto,
) -> String {
    let mut text = title.to_string();
    if content.kind != "inline" {
        return text;
    }
    if let Some(summary) = &content.summary {
        text.push('\n');
        text.push_str(summary);
    }
    if let Some(body) = &content.body {
        text.push('\n');
        text.push_str(body);
    }
    text
}
