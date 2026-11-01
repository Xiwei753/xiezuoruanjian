use std::time::{SystemTime, UNIX_EPOCH};

use rmcp::{
    handler::server::wrapper::{Json, Parameters},
    model::ErrorData,
    schemars::JsonSchema,
    tool, tool_router,
};
use serde::Deserialize;
use serde_json::{Value, json};
use writer_core::api::types::{
    AnnotationObjectRefDto, AnnotationTargetDto, StarMapAnchorDto, StarMapEdgeDto,
    StarMapEdgeKindDto, StarMapEdgePatchDto, StarMapEmbedDto, StarMapEmbedPatchDto, StarMapLinkDto,
    StarMapLinkPatchDto, StarMapNodeContentDto, StarMapNodeDto, StarMapNodeKindDto,
    StarMapNodePatchDto, StarMapNodeStyleDto, StarMapPointDto, StarMapProvenanceDto,
    StarMapReviewStatusDto, StarMapSourceKindDto, StarMapTargetPathDto,
};

use crate::context::{invalid_params, to_json};
use crate::server::SujianMcpServer;

fn parse_input<T: serde::de::DeserializeOwned>(value: Value, field: &str) -> Result<T, ErrorData> {
    serde_json::from_value(value)
        .map_err(|error| invalid_params(format!("invalid {field}: {error}")))
}

fn parse_enum<T: serde::de::DeserializeOwned>(value: &str, field: &str) -> Result<T, ErrorData> {
    serde_json::from_value(Value::String(value.to_string()))
        .map_err(|error| invalid_params(format!("invalid {field}: {error}")))
}

/// External webpage/URI bindings no longer exist in the data model. Core DTOs
/// ignore unknown fields, so reject the removed shapes at the MCP boundary
/// instead of silently dropping them.
const REMOVED_EXTERNAL_FIELDS: &[&str] = &[
    "externalRef",
    "externalUrl",
    "external_url",
    "targetUri",
    "target_uri",
    "uri",
];

fn reject_external_bindings(value: &Value, field: &str) -> Result<(), ErrorData> {
    match value {
        Value::Object(map) => {
            if let Some(kind) = map.get("type").and_then(Value::as_str) {
                if kind == "external" || kind == "externalRef" {
                    return Err(invalid_params(format!(
                        "invalid {field}: `{kind}` is no longer supported; reference an internal chapter, entity or object instead"
                    )));
                }
            }
            for key in REMOVED_EXTERNAL_FIELDS {
                if map.contains_key(*key) {
                    return Err(invalid_params(format!(
                        "invalid {field}: `{key}` is no longer supported; Sujian only stores internal object references"
                    )));
                }
            }
            for nested in map.values() {
                reject_external_bindings(nested, field)?;
            }
            Ok(())
        }
        Value::Array(items) => {
            for item in items {
                reject_external_bindings(item, field)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn parse_internal_target(value: Value, field: &str) -> Result<AnnotationTargetDto, ErrorData> {
    reject_external_bindings(&value, field)?;
    parse_input(value, field)
}

fn generated_by_mcp() -> StarMapProvenanceDto {
    StarMapProvenanceDto {
        source: StarMapSourceKindDto::Ai,
        source_id: None,
        generated_by: Some("sujian_mcp".to_string()),
        prompt_id: None,
        review_status: StarMapReviewStatusDto::Unknown,
        created_from_anchor: None,
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

macro_rules! empty_params {
    ($name:ident) => {
        #[derive(Debug, Deserialize, JsonSchema)]
        struct $name {}
    };
}

empty_params!(EmptyParams);

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct ListChaptersParams {
    /// Optional project id; when omitted, the authorized project is used.
    project_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct ReadChapterParams {
    project_id: String,
    chapter_id: String,
    volume_id: Option<String>,
    /// Character offset into the chapter body, not a byte offset.
    offset_chars: Option<usize>,
    /// Maximum number of characters to return (1..=50000, default 8000).
    max_chars: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct SearchParams {
    query: String,
    /// chapterBody/chapterTitle/chapterNote/projectTitle/volumeTitle/starmapTitle/
    /// starmapNode/starmapEdgeLabel/starmapLink/starmapEmbed/
    /// setting/annotationTag/annotationEntry/annotationLink
    scope: Option<String>,
    limit: Option<u32>,
    cursor: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct StarmapIdParams {
    starmap_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct ProjectIdParams {
    project_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct ChapterIdParams {
    project_id: String,
    chapter_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct ContributionStatsParams {
    /// Inclusive local date, YYYY-MM-DD.
    start_date: String,
    /// Inclusive local date, YYYY-MM-DD.
    end_date: String,
    project_id: Option<String>,
    /// human/ai/unknown/system.
    actor: Option<String>,
    /// starmap/starmap_node/tag/tag_binding/text_anchor/object_link/...
    object_type: Option<String>,
    device_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct CreateStarmapParams {
    title: String,
    description: Option<String>,
    template_id: Option<String>,
    /// 当前画布内创建子图时的宿主图；省略时使用会话解析出的活动画布。
    parent_starmap_id: Option<String>,
    /// 仅在无画布根图的写作面板创建作品内一级星图；已绑定画布时传 true
    /// 会被拒绝，必须改用当前画布内的子图。
    #[serde(default)]
    create_as_root: bool,
    #[serde(default)]
    x: f64,
    #[serde(default)]
    y: f64,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct RenameStarmapParams {
    starmap_id: String,
    new_title: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct CreateNodeParams {
    starmap_id: String,
    /// Optional caller-supplied id; a UUID is generated when omitted.
    id: Option<String>,
    title: String,
    /// PascalCase node kind: Character/Event/Location/Item/Concept/Theme/Note/
    /// Organization/Timeline/Plot/Foreshadowing/Chapter/Custom. Default Note.
    kind: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    payload: Option<String>,
    /// Inline summary; used only when `content` is omitted.
    summary: Option<String>,
    /// Inline body; used only when `content` is omitted.
    body: Option<String>,
    /// Full StarMapNodeContentDto object. Takes precedence over summary/body.
    /// `type` must be empty/inline/chapterRef/entityRef; external URI content is rejected.
    content: Option<Value>,
    #[serde(default)]
    anchors: Vec<Value>,
    portal: Option<Value>,
    #[serde(default)]
    x: f64,
    #[serde(default)]
    y: f64,
    style: Option<Value>,
    provenance: Option<Value>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct UpdateNodeParams {
    starmap_id: String,
    node_id: String,
    /// Revision from the last read. Required to prevent overwriting newer changes.
    expected_revision: u64,
    /// StarMapNodePatchDto JSON. Use null for an inner field to clear it.
    patch: Value,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct NodeIdParams {
    starmap_id: String,
    node_id: String,
    /// Revision from the last read. Required to prevent overwriting newer changes.
    expected_revision: u64,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct CreateEdgeParams {
    starmap_id: String,
    id: Option<String>,
    from: Value,
    to: Value,
    /// PascalCase edge kind, e.g. RelatedTo. Default RelatedTo.
    kind: Option<String>,
    label: Option<String>,
    payload: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct UpdateEdgeParams {
    starmap_id: String,
    edge_id: String,
    /// Revision from the last read. Required to prevent overwriting newer changes.
    expected_revision: u64,
    /// StarMapEdgePatchDto JSON.
    patch: Value,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct EdgeIdParams {
    starmap_id: String,
    edge_id: String,
    /// Revision from the last read. Required to prevent overwriting newer changes.
    expected_revision: u64,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct CreateEmbedParams {
    starmap_id: String,
    target_starmap_id: String,
    /// Parent path in the host graph. Required.
    host_path: Value,
    label: Option<String>,
    #[serde(default)]
    x: f64,
    #[serde(default)]
    y: f64,
    provenance: Option<Value>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct CreateChildEmbedParams {
    /// 省略时使用会话解析出的当前活动画布。
    host_starmap_id: Option<String>,
    title: String,
    #[serde(default)]
    x: f64,
    #[serde(default)]
    y: f64,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct UpdateEmbedParams {
    starmap_id: String,
    instance_id: String,
    /// Revision from the last read. Required to prevent overwriting newer changes.
    expected_revision: u64,
    /// StarMapEmbedPatchDto JSON.
    patch: Value,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct EmbedIdParams {
    starmap_id: String,
    instance_id: String,
    /// Revision from the last read. Required to prevent overwriting newer changes.
    expected_revision: u64,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct CreateLinkParams {
    starmap_id: String,
    link_id: Option<String>,
    source: Value,
    target: Value,
    label: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct UpdateLinkParams {
    starmap_id: String,
    link_id: String,
    /// Revision from the last read. Required to prevent overwriting newer changes.
    expected_revision: u64,
    /// StarMapLinkPatchDto JSON.
    patch: Value,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct LinkIdParams {
    starmap_id: String,
    link_id: String,
    /// Revision from the last read. Required to prevent overwriting newer changes.
    expected_revision: u64,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct BindStarmapParams {
    starmap_id: String,
    project_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct CreateTagParams {
    project_id: String,
    name: String,
    color: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct RenameTagParams {
    project_id: String,
    tag_id: String,
    name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct TagIdParams {
    project_id: String,
    tag_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct MergeTagsParams {
    project_id: String,
    source_tag_id: String,
    target_tag_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct BindingsParams {
    project_id: String,
    target: Value,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct SetBindingsParams {
    project_id: String,
    target: Value,
    tag_ids: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct CreateObjectLinkParams {
    source: Value,
    /// AnnotationTargetDto: `{"objectRef": {"kind", "projectId", "objectId"}}`.
    /// External URLs are not supported.
    target: Value,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct UpdateObjectLinkParams {
    project_id: String,
    link_id: String,
    source: Value,
    /// AnnotationTargetDto: `{"objectRef": {"kind", "projectId", "objectId"}}`.
    /// External URLs are not supported.
    target: Value,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct ObjectLinkIdParams {
    project_id: String,
    link_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct LinkSourceParams {
    source: Value,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct BacklinksParams {
    target: Value,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct CreateTextAnchorParams {
    project_id: String,
    chapter_id: String,
    start_byte: u32,
    end_byte: u32,
    /// `read_chapter` 返回的 hash；提供时正文已变化会拒绝创建。
    expected_hash: Option<String>,
}

#[tool_router(vis = "pub(crate)")]
impl SujianMcpServer {
    // ---------------------------------------------------------------- reading

    #[tool(
        name = "list_projects",
        description = "List the Sujian project authorized for this session. Chapter body content is read-only."
    )]
    fn list_projects(
        &self,
        Parameters(_): Parameters<EmptyParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let api = self.ctx().api();
        let project_id = self.ctx().scope().project_id.clone();
        let projects = crate::context::read(|| api.list_projects())?;
        let projects: Vec<_> = if project_id.is_empty() {
            Vec::new()
        } else {
            projects
                .into_iter()
                .filter(|project| project.id == project_id)
                .collect()
        };
        to_json(&projects)
    }

    #[tool(
        name = "list_volumes",
        description = "List volumes of the authorized project."
    )]
    fn list_volumes(
        &self,
        Parameters(params): Parameters<ProjectIdParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let project_id = self.ctx().require_project_scope(Some(&params.project_id))?;
        let api = self.ctx().api();
        let volumes = crate::context::read(|| api.list_volumes(&project_id))?;
        to_json(&volumes)
    }

    #[tool(
        name = "list_chapters",
        description = "List chapter metadata for the authorized project. Does not return body text."
    )]
    fn list_chapters(
        &self,
        Parameters(params): Parameters<ListChaptersParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let project_id = self
            .ctx()
            .require_project_scope(params.project_id.as_deref())?;
        let api = self.ctx().api();
        let chapters = crate::context::read(|| api.mcp_list_chapters(Some(&project_id)))?;
        to_json(&chapters)
    }

    #[tool(
        name = "read_chapter",
        description = "Read a chapter body by character range. Body is read-only; use offsetChars/nextOffsetChars for long chapters."
    )]
    fn read_chapter(
        &self,
        Parameters(params): Parameters<ReadChapterParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let project_id = self.ctx().require_project_scope(Some(&params.project_id))?;
        let api = self.ctx().api();
        let chapter = crate::context::read(|| {
            api.mcp_read_chapter(
                &project_id,
                &params.chapter_id,
                params.volume_id.as_deref(),
                params.offset_chars,
                params.max_chars,
            )
        })?;
        to_json(&chapter)
    }

    #[tool(
        name = "search",
        description = "Search within the authorized project/starmap scope only."
    )]
    fn search(
        &self,
        Parameters(params): Parameters<SearchParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let scope = params.scope.as_deref().unwrap_or("All");
        let limit = params.limit.unwrap_or(50);
        let value = self
            .ctx()
            .search(&params.query, scope, limit, params.cursor.as_deref())?;
        Ok(Json(value))
    }

    #[tool(
        name = "list_starmaps",
        description = "List starmaps in the authorized project and current root starmap subtree."
    )]
    fn list_starmaps(
        &self,
        Parameters(params): Parameters<ListChaptersParams>,
    ) -> Result<Json<Value>, ErrorData> {
        if let Some(project_id) = params.project_id.as_deref() {
            self.ctx().require_project_scope(Some(project_id))?;
        }
        let maps = self.ctx().authorized_starmaps()?;
        to_json(&maps)
    }

    #[tool(
        name = "get_starmap_graph",
        description = "Read a full starmap graph within the authorized scope: nodes, edges, embeds and links."
    )]
    fn get_starmap_graph(
        &self,
        Parameters(params): Parameters<StarmapIdParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let starmap_id = self.ctx().require_starmap_scope(&params.starmap_id)?;
        let api = self.ctx().api();
        let graph = crate::context::read(|| api.get_starmap_graph(&starmap_id))?;
        to_json(&graph)
    }

    #[tool(
        name = "list_tags",
        description = "List annotation tags of the authorized project."
    )]
    fn list_tags(
        &self,
        Parameters(params): Parameters<ProjectIdParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let project_id = self.ctx().require_project_scope(Some(&params.project_id))?;
        let api = self.ctx().api();
        let tags = crate::context::read(|| api.list_tags(project_id))?;
        to_json(&tags)
    }

    #[tool(
        name = "list_bindings_for_target",
        description = "List tag bindings for an AnnotationObjectRefDto target inside the authorized project."
    )]
    fn list_bindings_for_target(
        &self,
        Parameters(params): Parameters<BindingsParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let target: AnnotationObjectRefDto = parse_input(params.target, "target")?;
        self.ctx().require_object_ref(&target)?;
        let project_id = self.ctx().require_project_scope(Some(&params.project_id))?;
        let api = self.ctx().api();
        let bindings = crate::context::read(|| api.list_bindings_for_target(project_id, target))?;
        to_json(&bindings)
    }

    #[tool(
        name = "list_links_for_source",
        description = "List object links whose source is an AnnotationObjectRefDto inside the authorized project."
    )]
    fn list_links_for_source(
        &self,
        Parameters(params): Parameters<LinkSourceParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let source: AnnotationObjectRefDto = parse_input(params.source, "source")?;
        self.ctx().require_object_ref(&source)?;
        let api = self.ctx().api();
        let links = crate::context::read(|| api.list_links_for_source(source))?;
        to_json(&links)
    }

    #[tool(
        name = "find_backlinks",
        description = "Find object links inside the authorized project that point at an internal object target (AnnotationTargetDto.objectRef). External URLs are not supported."
    )]
    fn find_backlinks(
        &self,
        Parameters(params): Parameters<BacklinksParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let target = parse_internal_target(params.target, "target")?;
        self.ctx().require_internal_target(&target)?;
        let api = self.ctx().api();
        let links = crate::context::read(|| api.find_backlinks(target))?;
        to_json(&links)
    }

    #[tool(
        name = "list_text_anchors",
        description = "List annotation text anchors for a chapter in the authorized project. Read-only metadata."
    )]
    fn list_text_anchors(
        &self,
        Parameters(params): Parameters<ChapterIdParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let project_id = self.ctx().require_project_scope(Some(&params.project_id))?;
        let api = self.ctx().api();
        let anchors = crate::context::read(|| {
            api.list_text_anchors_for_chapter(project_id, params.chapter_id)
        })?;
        to_json(&anchors)
    }

    #[tool(
        name = "get_contribution_stats",
        description = "Read contribution statistics for the authorized project."
    )]
    fn get_contribution_stats(
        &self,
        Parameters(params): Parameters<ContributionStatsParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let project_id = self
            .ctx()
            .require_project_scope(params.project_id.as_deref())?;
        let api = self.ctx().api();
        let stats = crate::context::read(|| {
            api.get_contribution_stats(
                &params.start_date,
                &params.end_date,
                Some(&project_id),
                params.actor.as_deref(),
                params.object_type.as_deref(),
                params.device_id.as_deref(),
            )
        })?;
        to_json(&stats)
    }

    // --------------------------------------------------------- starmap writes

    #[tool(
        name = "create_starmap",
        description = "Create a starmap. In a canvas, creates and embeds a child starmap in the active graph; otherwise creates a project starmap in one Core transaction."
    )]
    fn create_starmap(
        &self,
        Parameters(params): Parameters<CreateStarmapParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let description = params.description.unwrap_or_default();
        self.ctx().create_starmap(
            &params.title,
            &description,
            params.template_id.as_deref(),
            params.parent_starmap_id.as_deref(),
            params.create_as_root,
            params.x,
            params.y,
        )
    }

    #[tool(
        name = "rename_starmap",
        description = "Rename a starmap inside the authorized scope."
    )]
    fn rename_starmap(
        &self,
        Parameters(params): Parameters<RenameStarmapParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let starmap_id = self.ctx().require_starmap_scope(&params.starmap_id)?;
        self.ctx()
            .write_audited("rename_starmap", &params.starmap_id, &starmap_id, |api| {
                api.rename_starmap(&starmap_id, &params.new_title)
            })
    }

    #[tool(
        name = "delete_starmap",
        description = "Delete an empty or explicitly confirmed starmap inside the authorized scope."
    )]
    fn delete_starmap(
        &self,
        Parameters(params): Parameters<StarmapIdParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let starmap_id = self.ctx().require_starmap_scope(&params.starmap_id)?;
        self.ctx()
            .write_audited("delete_starmap", &params.starmap_id, &starmap_id, |api| {
                api.delete_starmap(&starmap_id)
                    .map(|deleted| json!({ "deleted": deleted, "starmapId": starmap_id }))
            })
    }

    #[tool(
        name = "create_starmap_node",
        description = "Create a starmap node. Inline summary/body or a full content object can be supplied."
    )]
    fn create_starmap_node(
        &self,
        Parameters(params): Parameters<CreateNodeParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let node_id = params
            .id
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let kind = match params.kind.as_deref() {
            Some(value) => parse_enum(value, "kind")?,
            None => StarMapNodeKindDto::Note,
        };
        let content: StarMapNodeContentDto = match params.content {
            Some(value) => {
                reject_external_bindings(&value, "content")?;
                parse_input(value, "content")?
            }
            None => {
                if params.summary.is_some() || params.body.is_some() {
                    StarMapNodeContentDto {
                        kind: "inline".to_string(),
                        summary: params.summary,
                        body: params.body,
                        ..StarMapNodeContentDto::default()
                    }
                } else {
                    StarMapNodeContentDto::default()
                }
            }
        };
        let anchors = params
            .anchors
            .into_iter()
            .map(|value| {
                reject_external_bindings(&value, "anchors")?;
                parse_input::<StarMapAnchorDto>(value, "anchors")
            })
            .collect::<Result<Vec<_>, _>>()?;
        let portal = params
            .portal
            .map(|value| parse_input(value, "portal"))
            .transpose()?;
        let style = params
            .style
            .map(|value| parse_input::<StarMapNodeStyleDto>(value, "style"))
            .transpose()?
            .unwrap_or_default();
        let provenance = params
            .provenance
            .map(|value| parse_input(value, "provenance"))
            .transpose()?
            .unwrap_or_else(generated_by_mcp);
        let node = StarMapNodeDto {
            id: node_id,
            title: params.title,
            kind,
            payload: params.payload,
            tags: params.tags,
            content,
            anchors,
            portal,
            position: StarMapPointDto {
                x: params.x,
                y: params.y,
            },
            style,
            provenance,
            created_at: now_ms(),
            updated_at: now_ms(),
        };
        let starmap_id = self.ctx().require_starmap_scope(&params.starmap_id)?;
        self.ctx().write_audited(
            "create_starmap_node",
            &params.starmap_id,
            &starmap_id,
            |api| api.add_starmap_node(&starmap_id, node, params.x, params.y),
        )
    }

    #[tool(
        name = "update_starmap_node",
        description = "Update a starmap node with a StarMapNodePatchDto JSON patch inside the authorized scope."
    )]
    fn update_starmap_node(
        &self,
        Parameters(params): Parameters<UpdateNodeParams>,
    ) -> Result<Json<Value>, ErrorData> {
        reject_external_bindings(&params.patch, "patch")?;
        let patch: StarMapNodePatchDto = parse_input(params.patch, "patch")?;
        let starmap_id = self.ctx().require_starmap_scope(&params.starmap_id)?;
        self.ctx().write_audited(
            "update_starmap_node",
            &params.starmap_id,
            &starmap_id,
            |api| {
                api.update_starmap_node_expected(
                    &starmap_id,
                    &params.node_id,
                    patch,
                    Some(params.expected_revision),
                )
            },
        )
    }

    #[tool(
        name = "delete_starmap_node",
        description = "Delete a starmap node inside the authorized scope."
    )]
    fn delete_starmap_node(
        &self,
        Parameters(params): Parameters<NodeIdParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let starmap_id = self.ctx().require_starmap_scope(&params.starmap_id)?;
        self.ctx().write_audited(
            "delete_starmap_node",
            &params.starmap_id,
            &starmap_id,
            |api| {
                api.delete_starmap_node_expected(
                    &starmap_id,
                    &params.node_id,
                    Some(params.expected_revision),
                )
                .map(|deleted| {
                    json!({
                        "deleted": deleted,
                        "starmapId": starmap_id,
                        "nodeId": params.node_id,
                    })
                })
            },
        )
    }

    #[tool(
        name = "create_starmap_edge",
        description = "Create an edge between two StarMapTargetPathDto endpoints."
    )]
    fn create_starmap_edge(
        &self,
        Parameters(params): Parameters<CreateEdgeParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let from: StarMapTargetPathDto = parse_input(params.from, "from")?;
        let to: StarMapTargetPathDto = parse_input(params.to, "to")?;
        let kind: StarMapEdgeKindDto = match params.kind.as_deref() {
            Some(value) => parse_enum(value, "kind")?,
            None => StarMapEdgeKindDto::RelatedTo,
        };
        let edge = StarMapEdgeDto {
            id: params
                .id
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
            from,
            to,
            kind,
            label: params.label,
            payload: params.payload,
            created_at: now_ms(),
            updated_at: now_ms(),
        };
        let starmap_id = self.ctx().require_starmap_scope(&params.starmap_id)?;
        self.ctx().write_audited(
            "create_starmap_edge",
            &params.starmap_id,
            &starmap_id,
            |api| api.add_starmap_edge(&starmap_id, edge),
        )
    }

    #[tool(
        name = "update_starmap_edge",
        description = "Update a starmap edge with a StarMapEdgePatchDto JSON patch inside the authorized scope."
    )]
    fn update_starmap_edge(
        &self,
        Parameters(params): Parameters<UpdateEdgeParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let patch: StarMapEdgePatchDto = parse_input(params.patch, "patch")?;
        let starmap_id = self.ctx().require_starmap_scope(&params.starmap_id)?;
        self.ctx().write_audited(
            "update_starmap_edge",
            &params.starmap_id,
            &starmap_id,
            |api| {
                api.update_starmap_edge_expected(
                    &starmap_id,
                    &params.edge_id,
                    patch,
                    Some(params.expected_revision),
                )
            },
        )
    }

    #[tool(
        name = "delete_starmap_edge",
        description = "Delete a starmap edge inside the authorized scope."
    )]
    fn delete_starmap_edge(
        &self,
        Parameters(params): Parameters<EdgeIdParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let starmap_id = self.ctx().require_starmap_scope(&params.starmap_id)?;
        self.ctx().write_audited(
            "delete_starmap_edge",
            &params.starmap_id,
            &starmap_id,
            |api| {
                api.delete_starmap_edge_expected(
                    &starmap_id,
                    &params.edge_id,
                    Some(params.expected_revision),
                )
                .map(|deleted| {
                    json!({
                        "deleted": deleted,
                        "starmapId": starmap_id,
                        "edgeId": params.edge_id,
                    })
                })
            },
        )
    }

    #[tool(
        name = "create_starmap_embed",
        description = "Embed an existing starmap inside a host starmap graph."
    )]
    fn create_starmap_embed(
        &self,
        Parameters(params): Parameters<CreateEmbedParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let host_path: StarMapTargetPathDto = parse_input(params.host_path, "hostPath")?;
        let target_starmap_id = self
            .ctx()
            .require_starmap_scope(&params.target_starmap_id)?;
        let provenance = params
            .provenance
            .map(|value| parse_input(value, "provenance"))
            .transpose()?
            .unwrap_or_else(generated_by_mcp);
        let embed = StarMapEmbedDto {
            instance_id: uuid::Uuid::new_v4().to_string(),
            target_starmap_id,
            label: params.label,
            position: StarMapPointDto {
                x: params.x,
                y: params.y,
            },
            host_path,
            provenance,
            created_at: now_ms(),
            updated_at: now_ms(),
        };
        let starmap_id = self.ctx().require_starmap_scope(&params.starmap_id)?;
        self.ctx().write_audited(
            "create_starmap_embed",
            &params.starmap_id,
            &starmap_id,
            |api| api.add_starmap_embed(&starmap_id, embed),
        )
    }

    #[tool(
        name = "create_starmap_child_embed",
        description = "Atomically create a child starmap and embed it in the active canvas (or an explicit host graph)."
    )]
    fn create_starmap_child_embed(
        &self,
        Parameters(params): Parameters<CreateChildEmbedParams>,
    ) -> Result<Json<Value>, ErrorData> {
        self.ctx().create_child_embed(
            params.host_starmap_id.as_deref(),
            &params.title,
            params.x,
            params.y,
        )
    }

    #[tool(
        name = "update_starmap_embed",
        description = "Update an embed with a StarMapEmbedPatchDto JSON patch inside the authorized scope."
    )]
    fn update_starmap_embed(
        &self,
        Parameters(params): Parameters<UpdateEmbedParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let patch: StarMapEmbedPatchDto = parse_input(params.patch, "patch")?;
        let starmap_id = self.ctx().require_starmap_scope(&params.starmap_id)?;
        self.ctx().write_audited(
            "update_starmap_embed",
            &params.starmap_id,
            &starmap_id,
            |api| {
                api.update_starmap_embed_expected(
                    &starmap_id,
                    &params.instance_id,
                    patch,
                    Some(params.expected_revision),
                )
            },
        )
    }

    #[tool(
        name = "delete_starmap_embed",
        description = "Remove an embed from a starmap inside the authorized scope."
    )]
    fn delete_starmap_embed(
        &self,
        Parameters(params): Parameters<EmbedIdParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let starmap_id = self.ctx().require_starmap_scope(&params.starmap_id)?;
        self.ctx().write_audited(
            "delete_starmap_embed",
            &params.starmap_id,
            &starmap_id,
            |api| {
                api.delete_starmap_embed_expected(
                    &starmap_id,
                    &params.instance_id,
                    Some(params.expected_revision),
                )
                .map(|deleted| {
                    json!({
                        "deleted": deleted,
                        "starmapId": starmap_id,
                        "instanceId": params.instance_id,
                    })
                })
            },
        )
    }

    #[tool(
        name = "create_starmap_link",
        description = "Create a link between two target paths inside a starmap."
    )]
    fn create_starmap_link(
        &self,
        Parameters(params): Parameters<CreateLinkParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let source: StarMapTargetPathDto = parse_input(params.source, "source")?;
        let target: StarMapTargetPathDto = parse_input(params.target, "target")?;
        let link = StarMapLinkDto {
            link_id: params
                .link_id
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
            source,
            target,
            label: params.label,
            created_at: now_ms(),
            updated_at: now_ms(),
        };
        let starmap_id = self.ctx().require_starmap_scope(&params.starmap_id)?;
        self.ctx().write_audited(
            "create_starmap_link",
            &params.starmap_id,
            &starmap_id,
            |api| api.add_starmap_link(&starmap_id, link),
        )
    }

    #[tool(
        name = "update_starmap_link",
        description = "Update a starmap link with a StarMapLinkPatchDto JSON patch inside the authorized scope."
    )]
    fn update_starmap_link(
        &self,
        Parameters(params): Parameters<UpdateLinkParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let patch: StarMapLinkPatchDto = parse_input(params.patch, "patch")?;
        let starmap_id = self.ctx().require_starmap_scope(&params.starmap_id)?;
        self.ctx().write_audited(
            "update_starmap_link",
            &params.starmap_id,
            &starmap_id,
            |api| {
                api.update_starmap_link_expected(
                    &starmap_id,
                    &params.link_id,
                    patch,
                    Some(params.expected_revision),
                )
            },
        )
    }

    #[tool(
        name = "delete_starmap_link",
        description = "Delete a starmap link inside the authorized scope."
    )]
    fn delete_starmap_link(
        &self,
        Parameters(params): Parameters<LinkIdParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let starmap_id = self.ctx().require_starmap_scope(&params.starmap_id)?;
        self.ctx().write_audited(
            "delete_starmap_link",
            &params.starmap_id,
            &starmap_id,
            |api| {
                api.delete_starmap_link_expected(
                    &starmap_id,
                    &params.link_id,
                    Some(params.expected_revision),
                )
                .map(|deleted| {
                    json!({
                        "deleted": deleted,
                        "starmapId": starmap_id,
                        "linkId": params.link_id,
                    })
                })
            },
        )
    }

    #[tool(
        name = "bind_starmap_to_project",
        description = "Bind a starmap to the authorized project after validating source and target identity."
    )]
    fn bind_starmap_to_project(
        &self,
        Parameters(params): Parameters<BindStarmapParams>,
    ) -> Result<Json<Value>, ErrorData> {
        self.ctx()
            .bind_starmap(&params.project_id, &params.starmap_id)
    }

    #[tool(
        name = "unbind_starmap_from_project",
        description = "Remove a starmap from the authorized project without deleting it."
    )]
    fn unbind_starmap_from_project(
        &self,
        Parameters(params): Parameters<StarmapIdParams>,
    ) -> Result<Json<Value>, ErrorData> {
        self.ctx().unbind_starmap(&params.starmap_id)
    }

    // ------------------------------------------------------------- tag writes

    #[tool(
        name = "create_tag",
        description = "Create an annotation tag in the authorized project."
    )]
    fn create_tag(
        &self,
        Parameters(params): Parameters<CreateTagParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let project_id = self.ctx().require_project_scope(Some(&params.project_id))?;
        self.ctx().write_audited("create_tag", "", "", |api| {
            api.create_tag(project_id, params.name, params.color)
        })
    }

    #[tool(
        name = "rename_tag",
        description = "Rename an annotation tag in the authorized project."
    )]
    fn rename_tag(
        &self,
        Parameters(params): Parameters<RenameTagParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let project_id = self.ctx().require_project_scope(Some(&params.project_id))?;
        self.ctx().write_audited("rename_tag", "", "", |api| {
            api.rename_tag(project_id, params.tag_id, params.name)
        })
    }

    #[tool(
        name = "delete_tag",
        description = "Delete an annotation tag and its bindings in the authorized project."
    )]
    fn delete_tag(
        &self,
        Parameters(params): Parameters<TagIdParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let project_id = self.ctx().require_project_scope(Some(&params.project_id))?;
        self.ctx().write_audited("delete_tag", "", "", |api| {
            api.delete_tag(project_id.clone(), params.tag_id)
                .map(|deleted| json!({ "deleted": deleted, "projectId": project_id }))
        })
    }

    #[tool(
        name = "merge_tags",
        description = "Merge the source tag into the target tag in the authorized project."
    )]
    fn merge_tags(
        &self,
        Parameters(params): Parameters<MergeTagsParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let project_id = self.ctx().require_project_scope(Some(&params.project_id))?;
        self.ctx().write_audited("merge_tags", "", "", |api| {
            api.merge_tags(project_id, params.source_tag_id, params.target_tag_id)
                .map(|merged| json!({ "merged": merged }))
        })
    }

    #[tool(
        name = "set_tag_bindings",
        description = "Replace all tag bindings on an AnnotationObjectRefDto target inside the authorized project."
    )]
    fn set_tag_bindings(
        &self,
        Parameters(params): Parameters<SetBindingsParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let target: AnnotationObjectRefDto = parse_input(params.target, "target")?;
        self.ctx().require_object_ref(&target)?;
        let project_id = self.ctx().require_project_scope(Some(&params.project_id))?;
        self.ctx().write_audited("set_tag_bindings", "", "", |api| {
            api.set_tag_bindings(project_id, target, params.tag_ids)
        })
    }

    // ---------------------------------------------------- object-link writes

    #[tool(
        name = "create_object_link",
        description = "Create a cross-object link from an AnnotationObjectRefDto to an internal object target (AnnotationTargetDto.objectRef) in the authorized project. External URLs are not supported."
    )]
    fn create_object_link(
        &self,
        Parameters(params): Parameters<CreateObjectLinkParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let source: AnnotationObjectRefDto = parse_input(params.source, "source")?;
        let target = parse_internal_target(params.target, "target")?;
        self.ctx().require_object_ref(&source)?;
        self.ctx().require_internal_target(&target)?;
        self.ctx()
            .write_audited("create_object_link", "", "", |api| {
                api.create_link(source, target)
            })
    }

    #[tool(
        name = "update_object_link",
        description = "Update an object link's source and internal object target (AnnotationTargetDto.objectRef) in the authorized project. External URLs are not supported."
    )]
    fn update_object_link(
        &self,
        Parameters(params): Parameters<UpdateObjectLinkParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let source: AnnotationObjectRefDto = parse_input(params.source, "source")?;
        let target = parse_internal_target(params.target, "target")?;
        self.ctx().require_object_ref(&source)?;
        self.ctx().require_internal_target(&target)?;
        let project_id = self.ctx().require_project_scope(Some(&params.project_id))?;
        self.ctx()
            .write_audited("update_object_link", "", "", |api| {
                api.update_link(project_id, params.link_id, source, target)
            })
    }

    #[tool(
        name = "delete_object_link",
        description = "Delete an object link in the authorized project."
    )]
    fn delete_object_link(
        &self,
        Parameters(params): Parameters<ObjectLinkIdParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let project_id = self.ctx().require_project_scope(Some(&params.project_id))?;
        self.ctx()
            .write_audited("delete_object_link", "", "", |api| {
                api.delete_link(project_id.clone(), params.link_id.clone())
                    .map(|deleted| {
                        json!({
                            "deleted": deleted,
                            "projectId": project_id,
                            "linkId": params.link_id,
                        })
                    })
            })
    }

    #[tool(
        name = "create_chapter_text_anchor",
        description = "Create annotation metadata anchoring a chapter text byte range in the authorized project. The chapter body is never modified."
    )]
    fn create_chapter_text_anchor(
        &self,
        Parameters(params): Parameters<CreateTextAnchorParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let project_id = self.ctx().require_project_scope(Some(&params.project_id))?;
        self.ctx()
            .write_audited("create_chapter_text_anchor", "", "", |api| {
                api.mcp_create_chapter_text_anchor(
                    &project_id,
                    &params.chapter_id,
                    params.start_byte,
                    params.end_byte,
                    params.expected_hash.as_deref(),
                )
            })
    }
}
