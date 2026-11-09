use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use rmcp::handler::server::wrapper::Json;
use rmcp::{ErrorData, model::ErrorCode};
use serde::Serialize;
use serde_json::{Value, json};
use writer_core::api::WriterError;
use writer_core::api::service::WriterCoreApi;
use writer_core::api::types::{
    AnnotationObjectRefDto, AnnotationTargetDto, StarMapMetaDto, StarMapPathSegmentDto,
};
use writer_core::writing_stats::{MutationContext, with_mutation_context};

/// 本次 MCP 会话的真实作用域。由桌面端在启动 broker 前固定下来，工具
/// 参数不能覆盖；当前层星图身份通过 Core 的 root + 路径解析得到。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct McpScope {
    pub project_id: String,
    pub root_starmap_id: String,
    /// 桌面端上报的活动画布 ID，只用于审计对照；真实活动层始终由
    /// `root_starmap_id + path_segments_json` 解析。
    pub active_starmap_id: String,
    pub path_segments_json: String,
    pub route_key: String,
    /// 发起本轮会话的 Agent 身份（工具审计记录执行者）。
    pub executor: String,
}

/// Local workspace context for MCP tools.
///
/// A fresh Core API is constructed for every tool call. The desktop client also
/// constructs a Core API per operation, so both sides always read the current
/// on-disk object and never keep competing starmap caches. Core-level workspace
/// write locks serialize concurrent object mutations across processes.
///
/// Tool handlers never construct mutation contexts themselves: every write goes
/// through [`McpContext::write`] / [`McpContext::write_audited`], which pins the
/// actor to `Ai` and the entrypoint to `mcp`. Tool parameters therefore cannot
/// claim another actor, and audit entries are only written after the Core
/// transaction actually committed.
#[derive(Clone)]
pub struct McpContext {
    app_data_root: PathBuf,
    projects_root: PathBuf,
    scope: McpScope,
}

impl McpContext {
    pub fn new(
        app_data_root: impl AsRef<Path>,
        projects_root: impl AsRef<Path>,
        scope: McpScope,
    ) -> Self {
        Self {
            app_data_root: app_data_root.as_ref().to_path_buf(),
            projects_root: projects_root.as_ref().to_path_buf(),
            scope,
        }
    }

    pub fn api(&self) -> WriterCoreApi {
        WriterCoreApi::new(&self.app_data_root, &self.projects_root)
    }

    /// 校验桌面端传入的根目录与固定作用域一致。
    ///
    /// MCP server 绝不能因为参数缺失/拼错就悄悄创建一个新的空工作区：那会让
    /// Agent 在错误目录里“成功”写入，而真实作品与星图完全看不到这些修改。
    /// 这里只读检查，不创建任何目录。
    pub fn validate_roots(&self) -> Result<(), String> {
        if !self.app_data_root.is_dir() {
            return Err(format!(
                "应用数据根目录不存在或不是目录：{}；请从素笺桌面端重新发起 MCP 会话，不会自动创建空工作区",
                self.app_data_root.display()
            ));
        }
        let looks_like_workspace = self.app_data_root.join("app-meta").is_dir()
            || self.app_data_root.join("starmaps").is_dir()
            || self.app_data_root.join("settings.sync.json").is_file()
            || !self.scope.project_id.is_empty()
            || !self.scope.root_starmap_id.is_empty();
        if !looks_like_workspace {
            return Err(format!(
                "应用数据根目录 {} 不包含素笺工作区数据；请检查 --app-data-root 参数，不会自动初始化新工作区",
                self.app_data_root.display()
            ));
        }
        if !self.scope.project_id.is_empty() {
            validate_scope_segment("作品", &self.scope.project_id)?;
            if !self.projects_root.is_dir() {
                return Err(format!(
                    "作品根目录不存在或不是目录：{}",
                    self.projects_root.display()
                ));
            }
            let project_root = self.projects_root.join(&self.scope.project_id);
            if !project_root.is_dir() {
                return Err(format!(
                    "当前 MCP 会话绑定的作品 {} 不在作品根目录 {} 下",
                    self.scope.project_id,
                    self.projects_root.display()
                ));
            }
        }
        if !self.scope.root_starmap_id.is_empty() {
            validate_scope_segment("星图", &self.scope.root_starmap_id)?;
            let starmap_id = &self.scope.root_starmap_id;
            let graph_dir = self.app_data_root.join("starmaps").join(starmap_id);
            let meta_path = self
                .app_data_root
                .join("starmaps")
                .join(format!("{starmap_id}.meta.json"));
            if !graph_dir.is_dir() && !meta_path.is_file() {
                return Err(format!(
                    "当前 MCP 会话绑定的根星图 {} 不在应用数据根目录 {} 下",
                    starmap_id,
                    self.app_data_root.display()
                ));
            }
        }
        Ok(())
    }

    pub fn scope(&self) -> &McpScope {
        &self.scope
    }

    /// 供 MCP 初始化说明展示当前固定范围，避免 Agent 猜目标星图。
    pub fn scope_summary(&self) -> String {
        if self.scope.project_id.is_empty() && self.scope.root_starmap_id.is_empty() {
            return "当前会话没有绑定作品或星图范围；星图、章节与标注工具会被拒绝。".to_string();
        }
        let project = if self.scope.project_id.is_empty() {
            "<未绑定作品>"
        } else {
            self.scope.project_id.as_str()
        };
        let root = if self.scope.root_starmap_id.is_empty() {
            "<写作面板，无画布>"
        } else {
            self.scope.root_starmap_id.as_str()
        };
        // 优先使用 Core 按 root + 路径解析出的活动层；失败时退回桌面端上报值，
        // 但真正的写入校验仍以 Core 解析为准。
        let resolved_active = self.resolve_active_scene().ok().flatten();
        let active = resolved_active
            .as_deref()
            .filter(|value| !value.is_empty())
            .or_else(|| {
                (!self.scope.active_starmap_id.is_empty())
                    .then_some(self.scope.active_starmap_id.as_str())
            })
            .unwrap_or(root);
        format!(
            "当前 MCP 范围固定为：作品 {}；根星图 {}；当前活动画布 {}；路由 {}。\
             星图工具只能读写该作品与当前画布子树；需要合法子路径时必须使用 Core 返回的 ID，\
             不要猜测或复用上一张图的 starmapId。",
            project, root, active, self.scope.route_key
        )
    }

    fn project_id_opt(&self) -> Option<&str> {
        (!self.scope.project_id.is_empty()).then_some(self.scope.project_id.as_str())
    }

    fn root_starmap_id_opt(&self) -> Option<&str> {
        (!self.scope.root_starmap_id.is_empty()).then_some(self.scope.root_starmap_id.as_str())
    }

    fn path_segments(&self) -> Result<Vec<StarMapPathSegmentDto>, ErrorData> {
        if self.scope.path_segments_json.trim().is_empty() {
            return Ok(Vec::new());
        }
        serde_json::from_str(&self.scope.path_segments_json)
            .map_err(|error| invalid_params(format!("invalid MCP starmap path: {error}")))
    }

    /// 要求会话已绑定作品，并核对工具请求的作品 ID。
    pub fn require_project_scope(&self, requested: Option<&str>) -> Result<String, ErrorData> {
        let project_id = self.project_id_opt().ok_or_else(|| {
            scope_error("MCP 会话未绑定作品范围，请从写作或星图工作区重新发起会话")
        })?;
        if let Some(requested) = requested {
            if !requested.is_empty() && requested != project_id {
                return Err(scope_error(format!(
                    "请求的作品 {requested} 不属于当前 MCP 会话授权范围"
                )));
            }
        }
        let api = self.api();
        let exists = read(|| api.list_projects())?
            .iter()
            .any(|project| project.id == project_id);
        if !exists {
            return Err(scope_error(format!(
                "当前 MCP 会话绑定的作品 {project_id} 不存在"
            )));
        }
        Ok(project_id.to_string())
    }

    /// Core 解析当前活动画布。无根星图（写作面板）返回 `None`。
    pub fn resolve_active_scene(&self) -> Result<Option<String>, ErrorData> {
        let Some(root) = self.root_starmap_id_opt() else {
            return Ok(None);
        };
        let segments = self.path_segments()?;
        let api = self.api();
        let active = read(|| {
            api.mcp_validate_starmap_target(
                self.project_id_opt(),
                Some(root),
                segments.clone(),
                // 先用根图验证范围，再解析真实活动层，避免信任上报的裸 ID。
                root,
            )
        })?;
        let active = read(|| {
            api.mcp_resolve_active_starmap(self.project_id_opt(), &active, segments)
        })?;
        if !self.scope.active_starmap_id.is_empty() && self.scope.active_starmap_id != active {
            eprintln!(
                "sujian_mcp: 上报的活动星图 {} 与 Core 解析结果 {} 不一致，以 Core 为准",
                self.scope.active_starmap_id, active
            );
        }
        Ok(Some(active))
    }

    /// 校验并返回工具请求的目标星图 ID。
    pub fn require_starmap_scope(&self, requested: &str) -> Result<String, ErrorData> {
        if self.project_id_opt().is_none() && self.root_starmap_id_opt().is_none() {
            return Err(scope_error("MCP 会话未绑定作品或星图范围，不能读写星图"));
        }
        let segments = self.path_segments()?;
        let api = self.api();
        read(|| {
            api.mcp_validate_starmap_target(
                self.project_id_opt(),
                self.root_starmap_id_opt(),
                segments,
                requested,
            )
        })
    }

    /// 会话授权范围内的星图元数据。
    pub fn authorized_starmaps(&self) -> Result<Vec<StarMapMetaDto>, ErrorData> {
        let api = self.api();
        read(|| api.mcp_list_authorized_starmaps(self.project_id_opt(), self.root_starmap_id_opt()))
    }

    /// 授权范围内的星图 ID，用于过滤搜索索引结果。
    pub fn authorized_starmap_ids(&self) -> Result<Vec<String>, ErrorData> {
        Ok(self
            .authorized_starmaps()?
            .into_iter()
            .map(|meta| meta.starmap_id)
            .collect())
    }

    /// 只在会话授权作品/星图范围内搜索。
    pub fn search(
        &self,
        query: &str,
        scope: &str,
        limit: u32,
        cursor: Option<&str>,
    ) -> Result<Value, ErrorData> {
        let project_id = self.project_id_opt().map(str::to_string);
        let starmap_ids = self.authorized_starmap_ids()?;
        let api = self.api();
        let raw = read(|| {
            api.mcp_search_json_scoped(
                query,
                scope,
                limit,
                cursor,
                project_id.as_deref(),
                &starmap_ids,
            )
        })?;
        serde_json::from_str(&raw)
            .map_err(|error| invalid_params(format!("invalid search response: {error}")))
    }

    /// 校验来源对象和链接目标都属于授权作品。
    pub fn require_object_ref(&self, target: &AnnotationObjectRefDto) -> Result<(), ErrorData> {
        self.require_project_scope(Some(&target.project_id))?;
        Ok(())
    }

    pub fn require_internal_target(&self, target: &AnnotationTargetDto) -> Result<(), ErrorData> {
        self.require_object_ref(&target.object_ref)
    }

    /// 创建星图：写作面板（无画布根图）在授权作品下创建一级图；星图面板
    /// 始终在当前活动画布内创建子图并嵌入，避免 Agent 用可选布尔值把图
    /// 建到画布之外。缺省或空父 ID 统一解析当前活动图。
    pub fn create_starmap(
        &self,
        title: &str,
        description: &str,
        template_id: Option<&str>,
        parent_starmap_id: Option<&str>,
        create_as_root: bool,
        x: f64,
        y: f64,
    ) -> Result<Json<Value>, ErrorData> {
        let has_canvas_root = self.root_starmap_id_opt().is_some();
        if create_as_root && has_canvas_root {
            return Err(scope_error(
                "当前星图会话已绑定画布，不能创建画布外的一级星图；请在当前画布内创建子图",
            ));
        }
        let explicit_parent = parent_starmap_id
            .map(str::trim)
            .filter(|parent| !parent.is_empty())
            .map(str::to_string);
        let parent = if create_as_root {
            // 只有无画布根图的写作面板允许创建作品内一级图。
            None
        } else {
            match explicit_parent {
                Some(parent) => Some(parent),
                None => self.resolve_active_scene()?,
            }
        };
        if let Some(parent) = parent {
            let parent = self.require_starmap_scope(&parent)?;
            let project_id = self.project_id_opt().map(str::to_string);
            let root = self.root_starmap_id_opt().map(str::to_string);
            let segments = self.path_segments()?;
            let position = writer_core::api::types::StarMapPointDto { x, y };
            let title = title.to_string();
            return self.write_audited("create_starmap", "", "", |api| {
                api.mcp_create_starmap_child_embed_scoped(
                    project_id.as_deref(),
                    root.as_deref(),
                    segments,
                    &parent,
                    &title,
                    position,
                )
                .map(|result| {
                    json!({
                        "starmapId": result.starmap.starmap_id,
                        "projectId": result.starmap.project_id,
                        "parentStarmapId": parent,
                        "embedInstanceId": result.embed.instance_id,
                        "starmap": result.starmap,
                        "embed": result.embed,
                        "pending": result.pending,
                    })
                })
            });
        }
        let project_id = self.require_project_scope(None)?;
        let title = title.to_string();
        let description = description.to_string();
        let template_id = template_id.map(str::to_string);
        self.write_audited("create_starmap", "", "", |api| {
            api.mcp_create_starmap_in_project(
                &project_id,
                &title,
                &description,
                template_id.as_deref(),
            )
        })
    }

    /// 在当前画布（或显式宿主图）内创建子星图并原子嵌入。
    pub fn create_child_embed(
        &self,
        host_starmap_id: Option<&str>,
        title: &str,
        x: f64,
        y: f64,
    ) -> Result<Json<Value>, ErrorData> {
        let host = match host_starmap_id {
            Some(host) if !host.is_empty() => host.to_string(),
            _ => self
                .resolve_active_scene()?
                .ok_or_else(|| scope_error("当前会话没有可用的活动画布，请指定 hostStarmapId"))?,
        };
        let host = self.require_starmap_scope(&host)?;
        let project_id = self.project_id_opt().map(str::to_string);
        let root = self.root_starmap_id_opt().map(str::to_string);
        let segments = self.path_segments()?;
        let title = title.to_string();
        let position = writer_core::api::types::StarMapPointDto { x, y };
        self.write_audited("create_starmap_child_embed", &host, "", |api| {
            api.mcp_create_starmap_child_embed_scoped(
                project_id.as_deref(),
                root.as_deref(),
                segments,
                &host,
                &title,
                position,
            )
            .map(|result| {
                json!({
                    "starmapId": result.starmap.starmap_id,
                    "projectId": result.starmap.project_id,
                    "parentStarmapId": host.clone(),
                    "embedInstanceId": result.embed.instance_id,
                    "starmap": result.starmap,
                    "embed": result.embed,
                    "pending": result.pending,
                })
            })
        })
    }

    /// 绑定星图：来源图与目标作品都必须是当前会话已授权的范围。
    pub fn bind_starmap(
        &self,
        project_id: &str,
        starmap_id: &str,
    ) -> Result<Json<Value>, ErrorData> {
        let project_id = self.require_project_scope(Some(project_id))?;
        let starmap_id = self.require_starmap_scope(starmap_id)?;
        let root = self.root_starmap_id_opt().map(str::to_string);
        let segments = self.path_segments()?;
        self.write_audited("bind_starmap_to_project", &starmap_id, &starmap_id, |api| {
            api.mcp_bind_starmap_scoped(&project_id, root.as_deref(), segments, &starmap_id)
                .map(|bound| {
                    json!({
                        "bound": bound,
                        "starmapId": starmap_id,
                        "projectId": project_id,
                    })
                })
        })
    }

    /// 解绑星图：只允许操作已授权作品内当前绑定的图。
    pub fn unbind_starmap(&self, starmap_id: &str) -> Result<Json<Value>, ErrorData> {
        let project_id = self.require_project_scope(None)?;
        let starmap_id = self.require_starmap_scope(starmap_id)?;
        let root = self.root_starmap_id_opt().map(str::to_string);
        let segments = self.path_segments()?;
        self.write_audited(
            "unbind_starmap_from_project",
            &starmap_id,
            &starmap_id,
            |api| {
                api.mcp_unbind_starmap_scoped(&project_id, root.as_deref(), segments, &starmap_id)
                    .map(|unbound| {
                        json!({
                            "unbound": unbound,
                            "starmapId": starmap_id,
                            "projectId": project_id,
                        })
                    })
            },
        )
    }

    /// Runs a Core mutation as an MCP-originated AI edit.
    pub fn write<T>(
        &self,
        operation: impl FnOnce(&WriterCoreApi) -> Result<T, WriterError>,
    ) -> Result<T, ErrorData> {
        let api = self.api();
        with_mutation_context(MutationContext::mcp(), || operation(&api)).map_err(api_error)
    }

    /// 写入并记录审计。审计只在 Core 写事务成功后标记为成功；失败响应
    /// 不会留下“已提交”的假记录。
    pub fn write_audited<T: Serialize>(
        &self,
        tool: &str,
        requested_starmap_id: &str,
        resolved_starmap_id: &str,
        operation: impl FnOnce(&WriterCoreApi) -> Result<T, WriterError>,
    ) -> Result<Json<Value>, ErrorData> {
        let started = Instant::now();
        match self.write(operation) {
            Ok(value) => match serde_json::to_value(&value) {
                Ok(value) => {
                    self.append_audit(
                        tool,
                        requested_starmap_id,
                        resolved_starmap_id,
                        &value,
                        None,
                        started,
                    );
                    Ok(Json(value))
                }
                Err(error) => {
                    let message = format!("serialize response: {error}");
                    self.append_audit(
                        tool,
                        requested_starmap_id,
                        resolved_starmap_id,
                        &Value::Null,
                        Some(message.clone()),
                        started,
                    );
                    Err(ErrorData::internal_error(message, None))
                }
            },
            Err(error) => {
                let message = error.to_string();
                self.append_audit(
                    tool,
                    requested_starmap_id,
                    resolved_starmap_id,
                    &Value::Null,
                    Some(message),
                    started,
                );
                Err(error)
            }
        }
    }

    fn append_audit(
        &self,
        tool: &str,
        requested_starmap_id: &str,
        resolved_starmap_id: &str,
        value: &Value,
        error: Option<String>,
        started: Instant,
    ) {
        let (object_ids, revision) = collect_audit_objects(value);
        let derived_resolved = object_ids
            .iter()
            .find_map(|id| id.strip_prefix("starmapId="))
            .unwrap_or("");
        let resolved_starmap_id = if resolved_starmap_id.is_empty() {
            derived_resolved
        } else {
            resolved_starmap_id
        };
        let entry = json!({
            "occurredAtMs": now_ms(),
            "tool": tool,
            "executor": self.scope.executor,
            "boundProjectId": self.scope.project_id,
            "boundRootStarmapId": self.scope.root_starmap_id,
            "requestedStarmapId": requested_starmap_id,
            "resolvedStarmapId": resolved_starmap_id,
            "objectIds": object_ids,
            "committedRevision": revision,
            "routeKey": self.scope.route_key,
            "success": error.is_none(),
            "error": error.map(|message| truncate(&message, 500)),
            "durationMs": started.elapsed().as_millis(),
        });
        if let Err(write_error) = self.append_audit_line(&entry) {
            eprintln!("sujian_mcp: 写入 MCP 审计失败：{write_error}");
        }
        eprintln!(
            "sujian_mcp: tool={} success={} project={} requested={} resolved={} revision={} durationMs={}",
            tool,
            entry["success"],
            entry["boundProjectId"].as_str().unwrap_or(""),
            requested_starmap_id,
            resolved_starmap_id,
            revision,
            entry["durationMs"],
        );
    }

    fn append_audit_line(&self, entry: &Value) -> std::io::Result<()> {
        use std::io::Write;
        let directory = self.app_data_root.join("app-meta");
        std::fs::create_dir_all(&directory)?;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(directory.join("mcp-audit.local.jsonl"))?;
        writeln!(file, "{entry}")
    }
}

fn collect_audit_objects(value: &Value) -> (Vec<String>, u64) {
    fn walk(value: &Value, ids: &mut Vec<String>, revision: &mut u64) {
        match value {
            Value::Object(map) => {
                for (key, nested) in map {
                    match key.as_str() {
                        "starmapId" | "parentStarmapId" | "nodeId" | "edgeId" | "instanceId"
                        | "embedInstanceId" | "linkId" | "objectId" | "id" => {
                            if let Some(id) = nested.as_str().filter(|id| !id.is_empty()) {
                                let item = format!("{key}={id}");
                                if !ids.iter().any(|existing| existing == &item) {
                                    ids.push(item);
                                }
                            }
                        }
                        "updatedAt" | "revision" => {
                            if let Some(number) = nested.as_u64() {
                                *revision = (*revision).max(number);
                            }
                        }
                        _ => {}
                    }
                    walk(nested, ids, revision);
                }
            }
            Value::Array(items) => {
                for item in items {
                    walk(item, ids, revision);
                }
            }
            _ => {}
        }
    }

    let mut ids = Vec::new();
    let mut revision = 0;
    walk(value, &mut ids, &mut revision);
    (ids, revision)
}

fn truncate(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

/// Runs a read-only Core query and maps its error to an MCP error response.
pub fn read<T>(operation: impl FnOnce() -> Result<T, WriterError>) -> Result<T, ErrorData> {
    operation().map_err(api_error)
}

pub fn api_error(error: WriterError) -> ErrorData {
    ErrorData::new(ErrorCode::INTERNAL_ERROR, error.to_string(), None)
}

fn scope_error(message: impl Into<String>) -> ErrorData {
    ErrorData::new(ErrorCode::INVALID_REQUEST, message.into(), None)
}

fn validate_scope_segment(label: &str, value: &str) -> Result<(), String> {
    if value == "."
        || value == ".."
        || value.contains('/')
        || value.contains('\\')
        || value.contains('\0')
    {
        return Err(format!("MCP 会话绑定的{label} ID {value} 不是合法目录段"));
    }
    Ok(())
}

pub fn invalid_params(message: impl Into<String>) -> ErrorData {
    ErrorData::invalid_params(message.into(), None)
}

pub fn to_json<T: serde::Serialize>(
    value: &T,
) -> Result<rmcp::handler::server::wrapper::Json<serde_json::Value>, ErrorData> {
    let value = serde_json::to_value(value)
        .map_err(|error| ErrorData::internal_error(format!("serialize response: {error}"), None))?;
    Ok(rmcp::handler::server::wrapper::Json(value))
}
