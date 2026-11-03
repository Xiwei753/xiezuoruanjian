// =============================================================================
// starmap_view/preview_cache.rs — 子星图微缩预览的共享快照服务（Linux 平台端）
// =============================================================================
//
// 层级：Linux_qt 平台层显示几何缓存。
//
// 职责：
//   - 读取目标星图 DTO，生成与相机无关的预览几何；
//   - 节点尺寸只消费 QML 用 StarMapNodeMeasurer / StarMapNodeMetrics 批量
//     实测出的布局结果，不在 Rust 重新猜字体宽度；
//   - 按 (工作区, starmapId, graphRevision, layoutSignature) 缓存不可变快照；
//   - 用单一固定工作队列读图 / 算边 / 算 bounds，多个场景和重复入口共享
//     同一份结果；
//   - 工作区事件只按 event.starmapId 精确标脏；读图或算几何期间发生新事件
//     时，旧结果不会作为当前快照返回。
//
// 边界：
//   - 快照只有平台显示几何，不写 Core，不改变节点真相；
//   - 节点、连线、文字仍是 QML 真实矢量投影，不做预栅格化；
//   - 镜头缩放 / 平移不会进入这里，也不会让缓存失效。
// =============================================================================

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex, OnceLock};

use writer_core::api::types::StarMapGraphDto;
use writer_core::api::WriterCoreApi;
use writer_core::storage::git_repo_layout::GitRepoLayout;

/// 一次后台预览任务所属的工作区 / Git 布局。
///
/// AppBackend 和 AppRef 只能在 GUI 线程使用，后台任务只拿这份可克隆的
/// 只读上下文重新构造 WriterCoreApi。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PreviewContext {
    data_root: String,
    projects_root: String,
    worktree_root: String,
    git_dir: String,
}

impl PreviewContext {
    pub fn new(data_root: &str, projects_root: &str, layout: &GitRepoLayout) -> Self {
        Self {
            data_root: data_root.to_string(),
            projects_root: projects_root.to_string(),
            worktree_root: layout.worktree_root.to_string_lossy().into_owned(),
            git_dir: layout.git_dir.to_string_lossy().into_owned(),
        }
    }

    fn api(&self) -> WriterCoreApi {
        let layout = GitRepoLayout {
            worktree_root: PathBuf::from(&self.worktree_root),
            git_dir: PathBuf::from(&self.git_dir),
        };
        crate::backend::app_backend::with_layout_core_api(
            &self.data_root,
            &self.projects_root,
            &layout,
        )
    }
}

/// QML 实测出的单个节点排版尺寸（world px）。
#[derive(Debug, Clone, serde::Deserialize)]
pub struct PreviewNodeLayout {
    pub id: String,
    pub width: f64,
    pub height: f64,
}

/// 子星图预览的不可变显示快照。字段以 camelCase 序列化给 QML。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewSnapshot {
    pub starmap_id: String,
    pub graph_revision: String,
    pub nodes: Vec<PreviewNode>,
    pub embeds: Vec<PreviewEmbed>,
    pub edges: Vec<serde_json::Value>,
    pub bounds: Option<PreviewBounds>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewNode {
    pub id: String,
    pub title: String,
    pub body: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewEmbed {
    pub instance_id: String,
    pub target_starmap_id: String,
    pub label: String,
    pub x: f64,
    pub y: f64,
    pub diameter: f64,
}

#[derive(Debug, Clone, Copy, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewBounds {
    pub min_x: f64,
    pub min_y: f64,
    pub max_x: f64,
    pub max_y: f64,
    pub width: f64,
    pub height: f64,
}

const EMBED_DIAMETER: f64 = 200.0;

/// 预览任务完成后回到 GUI 的事件。
pub enum PreviewEvent {
    /// 图数据已读取 / 命中，需要 QML 用真实文本排版测量节点后再 finalize。
    GraphReady {
        request_id: u32,
        starmap_id: String,
        graph_revision: String,
        graph_json: String,
    },
    /// 完整不可变快照已生成。
    Ready {
        request_id: u32,
        starmap_id: String,
        graph_revision: String,
        snapshot_json: String,
    },
    Failed {
        request_id: u32,
        starmap_id: String,
        message: String,
    },
}

type PreviewCallback = Box<dyn FnOnce(PreviewEvent) + Send>;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct GraphKey {
    context: PreviewContext,
    starmap_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct SnapshotKey {
    graph: GraphKey,
    graph_revision: String,
    layout_signature: String,
}

struct PreparedGraph {
    revision: String,
    dto: StarMapGraphDto,
    epoch: u64,
}

struct PreviewWorker {
    apis: HashMap<PreviewContext, WriterCoreApi>,
    prepared: HashMap<GraphKey, PreparedGraph>,
    snapshots: HashMap<SnapshotKey, Arc<PreviewSnapshot>>,
    latest: HashMap<GraphKey, Arc<PreviewSnapshot>>,
}

impl PreviewWorker {
    fn new() -> Self {
        Self {
            apis: HashMap::new(),
            prepared: HashMap::new(),
            snapshots: HashMap::new(),
            latest: HashMap::new(),
        }
    }

    fn api_for(&mut self, context: &PreviewContext) -> &WriterCoreApi {
        self.apis
            .entry(context.clone())
            .or_insert_with(|| context.api())
    }
}

enum PreviewJob {
    Request {
        request_id: u32,
        context: PreviewContext,
        starmap_id: String,
        callback: PreviewCallback,
    },
    Finalize {
        request_id: u32,
        context: PreviewContext,
        starmap_id: String,
        graph_revision: String,
        layouts: Vec<PreviewNodeLayout>,
        callback: PreviewCallback,
    },
}

struct PreviewShared {
    epochs: Mutex<HashMap<GraphKey, u64>>,
    // 同一工作区事件会被多个 ActiveScene 看到；按事件身份去重，避免每个
    // 场景都推进一次服务级 epoch。
    seen_events: Mutex<HashMap<GraphKey, HashSet<String>>>,
}

/// 固定后台预览服务句柄。所有 QML 场景共享同一个实例。
#[derive(Clone)]
pub struct PreviewService {
    tx: mpsc::Sender<PreviewJob>,
    shared: Arc<PreviewShared>,
}

impl PreviewService {
    pub fn request_graph(
        &self,
        request_id: u32,
        context: PreviewContext,
        starmap_id: String,
        callback: PreviewCallback,
    ) {
        let _ = self.tx.send(PreviewJob::Request {
            request_id,
            context,
            starmap_id,
            callback,
        });
    }

    pub fn finalize(
        &self,
        request_id: u32,
        context: PreviewContext,
        starmap_id: String,
        graph_revision: String,
        layouts: Vec<PreviewNodeLayout>,
        callback: PreviewCallback,
    ) {
        let _ = self.tx.send(PreviewJob::Finalize {
            request_id,
            context,
            starmap_id,
            graph_revision,
            layouts,
            callback,
        });
    }

    /// 只按目标星图精确标脏。内容 / 几何依赖变化时递增 epoch；下一次读图
    /// 或 finalize 会因此丢弃旧结果并重新读取。event_key 为空表示本地写操作
    /// 强制失效；非空时同一事件被多个场景重复上报只推进一次 epoch。
    pub fn invalidate(
        &self,
        context: &PreviewContext,
        starmap_id: &str,
        _revision: u64,
        event_key: &str,
    ) -> bool {
        let key = GraphKey {
            context: context.clone(),
            starmap_id: starmap_id.to_string(),
        };
        if !event_key.is_empty() {
            if let Ok(mut seen) = self.shared.seen_events.lock() {
                let entry = seen.entry(key.clone()).or_default();
                if !entry.insert(event_key.to_string()) {
                    return false;
                }
                if entry.len() > 512 {
                    entry.clear();
                    entry.insert(event_key.to_string());
                }
            }
        }
        if let Ok(mut epochs) = self.shared.epochs.lock() {
            let next = epochs.get(&key).copied().unwrap_or(0).saturating_add(1);
            epochs.insert(key, next);
        }
        true
    }
}

static PREVIEW_SERVICE: OnceLock<PreviewService> = OnceLock::new();

pub fn service() -> &'static PreviewService {
    PREVIEW_SERVICE.get_or_init(|| {
        let (tx, rx) = mpsc::channel();
        let shared = Arc::new(PreviewShared {
            epochs: Mutex::new(HashMap::new()),
            seen_events: Mutex::new(HashMap::new()),
        });
        let thread_shared = Arc::clone(&shared);
        std::thread::Builder::new()
            .name("starmap-preview-service".to_string())
            .spawn(move || worker_loop(rx, thread_shared))
            .expect("starmap preview worker must start");
        PreviewService { tx, shared }
    })
}

fn worker_loop(receiver: mpsc::Receiver<PreviewJob>, shared: Arc<PreviewShared>) {
    let mut worker = PreviewWorker::new();
    while let Ok(job) = receiver.recv() {
        match job {
            PreviewJob::Request {
                request_id,
                context,
                starmap_id,
                callback,
            } => handle_request(
                &mut worker,
                &shared,
                request_id,
                context,
                starmap_id,
                callback,
            ),
            PreviewJob::Finalize {
                request_id,
                context,
                starmap_id,
                graph_revision,
                layouts,
                callback,
            } => handle_finalize(
                &mut worker,
                &shared,
                request_id,
                context,
                starmap_id,
                graph_revision,
                layouts,
                callback,
            ),
        }
    }
}

fn current_epoch(shared: &PreviewShared, key: &GraphKey) -> u64 {
    shared
        .epochs
        .lock()
        .ok()
        .and_then(|epochs| epochs.get(key).copied())
        .unwrap_or(0)
}

fn handle_request(
    worker: &mut PreviewWorker,
    shared: &PreviewShared,
    request_id: u32,
    context: PreviewContext,
    starmap_id: String,
    callback: PreviewCallback,
) {
    let key = GraphKey {
        context,
        starmap_id: starmap_id.clone(),
    };
    let epoch = current_epoch(shared, &key);

    if let Some(prepared) = worker.prepared.get(&key) {
        if prepared.epoch == epoch {
            if let Some(snapshot) = latest_for_revision(worker, &key, &prepared.revision) {
                if let Ok(json) = to_json(&snapshot) {
                    callback(PreviewEvent::Ready {
                        request_id,
                        starmap_id,
                        graph_revision: prepared.revision.clone(),
                        snapshot_json: json,
                    });
                    return;
                }
            }
            match serde_json::to_string(&prepared.dto) {
                Ok(graph_json) => {
                    callback(PreviewEvent::GraphReady {
                        request_id,
                        starmap_id,
                        graph_revision: prepared.revision.clone(),
                        graph_json,
                    });
                }
                Err(error) => callback(PreviewEvent::Failed {
                    request_id,
                    starmap_id,
                    message: format!("序列化星图失败: {error}"),
                }),
            }
            return;
        }
    }

    let (dto, revision, read_epoch) = match read_graph(worker, shared, &key) {
        Ok(value) => value,
        Err(message) => {
            callback(PreviewEvent::Failed {
                request_id,
                starmap_id,
                message,
            });
            return;
        }
    };

    if let Some(snapshot) = latest_for_revision(worker, &key, &revision) {
        if let Ok(json) = to_json(&snapshot) {
            worker.prepared.insert(
                key,
                PreparedGraph {
                    revision: revision.clone(),
                    dto,
                    epoch: read_epoch,
                },
            );
            callback(PreviewEvent::Ready {
                request_id,
                starmap_id,
                graph_revision: revision,
                snapshot_json: json,
            });
            return;
        }
    }

    worker.prepared.insert(
        key,
        PreparedGraph {
            revision: revision.clone(),
            dto: dto.clone(),
            epoch: read_epoch,
        },
    );
    match serde_json::to_string(&dto) {
        Ok(graph_json) => callback(PreviewEvent::GraphReady {
            request_id,
            starmap_id,
            graph_revision: revision,
            graph_json,
        }),
        Err(error) => callback(PreviewEvent::Failed {
            request_id,
            starmap_id,
            message: format!("序列化星图失败: {error}"),
        }),
    }
}

fn handle_finalize(
    worker: &mut PreviewWorker,
    shared: &PreviewShared,
    request_id: u32,
    context: PreviewContext,
    starmap_id: String,
    graph_revision: String,
    layouts: Vec<PreviewNodeLayout>,
    callback: PreviewCallback,
) {
    let key = GraphKey {
        context,
        starmap_id: starmap_id.clone(),
    };

    // 读取 / 校验目标 revision。工作区事件可能在 QML 测量期间到达；如果
    // 图内容已经前进，先返回新的 GraphReady，让 QML 用新图重新测量。
    loop {
        let epoch = current_epoch(shared, &key);
        let prepared_matches = worker
            .prepared
            .get(&key)
            .map(|prepared| prepared.epoch == epoch && prepared.revision == graph_revision)
            .unwrap_or(false);

        let dto = if prepared_matches {
            worker
                .prepared
                .get(&key)
                .map(|prepared| prepared.dto.clone())
                .expect("prepared graph checked above")
        } else {
            let (dto, revision, read_epoch) = match read_graph(worker, shared, &key) {
                Ok(value) => value,
                Err(message) => {
                    callback(PreviewEvent::Failed {
                        request_id,
                        starmap_id,
                        message,
                    });
                    return;
                }
            };
            if revision != graph_revision {
                if let Some(snapshot) = latest_for_revision(worker, &key, &revision) {
                    if let Ok(json) = to_json(&snapshot) {
                        worker.prepared.insert(
                            key,
                            PreparedGraph {
                                revision: revision.clone(),
                                dto,
                                epoch: read_epoch,
                            },
                        );
                        callback(PreviewEvent::Ready {
                            request_id,
                            starmap_id,
                            graph_revision: revision,
                            snapshot_json: json,
                        });
                        return;
                    }
                }
                worker.prepared.insert(
                    key,
                    PreparedGraph {
                        revision: revision.clone(),
                        dto: dto.clone(),
                        epoch: read_epoch,
                    },
                );
                match serde_json::to_string(&dto) {
                    Ok(graph_json) => callback(PreviewEvent::GraphReady {
                        request_id,
                        starmap_id,
                        graph_revision: revision,
                        graph_json,
                    }),
                    Err(error) => callback(PreviewEvent::Failed {
                        request_id,
                        starmap_id,
                        message: format!("序列化星图失败: {error}"),
                    }),
                }
                return;
            }
            worker.prepared.insert(
                key.clone(),
                PreparedGraph {
                    revision: revision.clone(),
                    dto: dto.clone(),
                    epoch: read_epoch,
                },
            );
            dto
        };

        let signature = layout_signature(&layouts);
        let snapshot_key = SnapshotKey {
            graph: key.clone(),
            graph_revision: graph_revision.clone(),
            layout_signature: signature.clone(),
        };

        if let Some(snapshot) = worker.snapshots.get(&snapshot_key).cloned() {
            if let Ok(json) = to_json(&snapshot) {
                callback(PreviewEvent::Ready {
                    request_id,
                    starmap_id,
                    graph_revision,
                    snapshot_json: json,
                });
                return;
            }
        }

        let snapshot = match build_snapshot(&dto, &layouts, graph_revision.clone()) {
            Ok(snapshot) => Arc::new(snapshot),
            Err(message) => {
                callback(PreviewEvent::Failed {
                    request_id,
                    starmap_id,
                    message,
                });
                return;
            }
        };

        // 构建期间再次收到同一目标事件时，不算当前快照；重新读图并比较
        // revision。内容相同则继续复用本次真实布局。
        if current_epoch(shared, &key) != epoch {
            continue;
        }

        let json = match to_json(&snapshot) {
            Ok(json) => json,
            Err(message) => {
                callback(PreviewEvent::Failed {
                    request_id,
                    starmap_id,
                    message,
                });
                return;
            }
        };
        worker.snapshots.insert(snapshot_key, Arc::clone(&snapshot));
        worker.latest.insert(key.clone(), snapshot);
        callback(PreviewEvent::Ready {
            request_id,
            starmap_id,
            graph_revision,
            snapshot_json: json,
        });
        return;
    }
}

fn read_graph(
    worker: &mut PreviewWorker,
    shared: &PreviewShared,
    key: &GraphKey,
) -> Result<(StarMapGraphDto, String, u64), String> {
    loop {
        let epoch = current_epoch(shared, key);
        let dto = worker
            .api_for(&key.context)
            .get_starmap_graph(&key.starmap_id)
            .map_err(|error| error.to_string())?;
        let revision = revision_of(&dto)?;
        if current_epoch(shared, key) == epoch {
            return Ok((dto, revision, epoch));
        }
    }
}

fn latest_for_revision(
    worker: &PreviewWorker,
    key: &GraphKey,
    revision: &str,
) -> Option<Arc<PreviewSnapshot>> {
    worker
        .latest
        .get(key)
        .filter(|snapshot| snapshot.graph_revision == revision)
        .cloned()
}

fn layout_signature(layouts: &[PreviewNodeLayout]) -> String {
    let mut ordered: Vec<&PreviewNodeLayout> = layouts.iter().collect();
    ordered.sort_by(|left, right| left.id.cmp(&right.id));
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for layout in ordered {
        layout.id.hash(&mut hasher);
        layout.width.to_bits().hash(&mut hasher);
        layout.height.to_bits().hash(&mut hasher);
    }
    format!("{:016x}", hasher.finish())
}

fn revision_of(dto: &StarMapGraphDto) -> Result<String, String> {
    let graph_json =
        serde_json::to_string(dto).map_err(|error| format!("序列化星图失败: {error}"))?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    graph_json.hash(&mut hasher);
    Ok(format!("{:016x}", hasher.finish()))
}

/// 用 QML 实测布局 + 图 DTO 构建快照。普通节点缺少真实布局时直接失败，
/// 不退回第二套固定高度 / 字符宽度估算。
pub fn build_snapshot(
    dto: &StarMapGraphDto,
    layouts: &[PreviewNodeLayout],
    graph_revision: String,
) -> Result<PreviewSnapshot, String> {
    let starmap_id = dto.starmap_id.clone();
    let core_graph = writer_core::starmap::types::StarMapGraph::try_from(dto.clone())
        .map_err(|error| error.to_string())?;
    let layout_by_id: HashMap<&str, (f64, f64)> = layouts
        .iter()
        .map(|layout| (layout.id.as_str(), (layout.width, layout.height)))
        .collect();

    let mut nodes = Vec::new();
    let mut embeds = Vec::new();
    let mut node_scenes = Vec::new();
    let mut embed_scenes = Vec::new();
    let mut bounds: Option<PreviewBounds> = None;

    for node in &dto.nodes {
        let x = node.position.x;
        let y = node.position.y;
        if let Some(portal) = &node.portal {
            if !portal.destination_starmap_id.is_empty() {
                let label = if node.title.is_empty() {
                    "未命名".to_string()
                } else {
                    node.title.clone()
                };
                embeds.push(PreviewEmbed {
                    instance_id: format!("legacy-portal:{}", node.id),
                    target_starmap_id: portal.destination_starmap_id.clone(),
                    label,
                    x,
                    y,
                    diameter: EMBED_DIAMETER,
                });
                include_rect(&mut bounds, x, y, EMBED_DIAMETER, EMBED_DIAMETER);
                node_scenes.push(serde_json::json!({
                    "id": node.id,
                    "x": x,
                    "y": y,
                    "width": EMBED_DIAMETER,
                    "height": EMBED_DIAMETER,
                }));
                embed_scenes.push(serde_json::json!({
                    "instanceId": format!("legacy-portal:{}", node.id),
                    "x": x,
                    "y": y,
                    "width": EMBED_DIAMETER,
                    "height": EMBED_DIAMETER,
                }));
                continue;
            }
        }

        let Some(&(width, height)) = layout_by_id.get(node.id.as_str()) else {
            return Err(format!("节点 {} 缺少 Qt 文本排版尺寸", node.id));
        };
        if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
            return Err(format!("节点 {} 的 Qt 文本排版尺寸无效", node.id));
        }
        nodes.push(PreviewNode {
            id: node.id.clone(),
            title: if node.title.is_empty() {
                "未命名".to_string()
            } else {
                node.title.clone()
            },
            body: display_body(node),
            x,
            y,
            width,
            height,
        });
        include_rect(&mut bounds, x, y, width, height);
        node_scenes.push(serde_json::json!({
            "id": node.id,
            "x": x,
            "y": y,
            "width": width,
            "height": height,
        }));
    }

    for embed in &dto.embeds {
        let x = embed.position.x;
        let y = embed.position.y;
        embeds.push(PreviewEmbed {
            instance_id: embed.instance_id.clone(),
            target_starmap_id: embed.target_starmap_id.clone(),
            label: embed
                .label
                .clone()
                .filter(|label| !label.is_empty())
                .unwrap_or_else(|| "未命名".to_string()),
            x,
            y,
            diameter: EMBED_DIAMETER,
        });
        include_rect(&mut bounds, x, y, EMBED_DIAMETER, EMBED_DIAMETER);
        embed_scenes.push(serde_json::json!({
            "instanceId": embed.instance_id,
            "x": x,
            "y": y,
            "width": EMBED_DIAMETER,
            "height": EMBED_DIAMETER,
        }));
    }

    let nodes_json = serde_json::to_string(&node_scenes)
        .map_err(|error| format!("序列化节点几何失败: {error}"))?;
    let embeds_json = serde_json::to_string(&embed_scenes)
        .map_err(|error| format!("序列化入口几何失败: {error}"))?;
    let render_json = crate::starmap_view::bridge::compute_edge_renders_json(
        &core_graph,
        &nodes_json,
        &embeds_json,
    );
    let edges = serde_json::from_str::<serde_json::Value>(&render_json)
        .ok()
        .and_then(|value| {
            if value.get("success").and_then(|v| v.as_bool()) == Some(true) {
                value.get("data").and_then(|data| data.as_array()).cloned()
            } else {
                None
            }
        })
        .unwrap_or_default();

    Ok(PreviewSnapshot {
        starmap_id,
        graph_revision,
        nodes,
        embeds,
        edges,
        bounds,
    })
}

pub fn to_json(snapshot: &PreviewSnapshot) -> Result<String, String> {
    serde_json::to_string(snapshot).map_err(|error| format!("序列化预览快照失败: {error}"))
}

fn include_rect(bounds: &mut Option<PreviewBounds>, x: f64, y: f64, width: f64, height: f64) {
    let next_min_x = x;
    let next_min_y = y;
    let next_max_x = x + width;
    let next_max_y = y + height;
    *bounds = Some(match *bounds {
        Some(current) => PreviewBounds {
            min_x: current.min_x.min(next_min_x),
            min_y: current.min_y.min(next_min_y),
            max_x: current.max_x.max(next_max_x),
            max_y: current.max_y.max(next_max_y),
            width: 0.0,
            height: 0.0,
        },
        None => PreviewBounds {
            min_x: next_min_x,
            min_y: next_min_y,
            max_x: next_max_x,
            max_y: next_max_y,
            width: 0.0,
            height: 0.0,
        },
    });
    if let Some(current) = bounds.as_mut() {
        current.width = current.max_x - current.min_x;
        current.height = current.max_y - current.min_y;
    }
}

fn display_body(node: &writer_core::api::types::StarMapNodeDto) -> String {
    // 与 StarMapNodeMetrics.bodyOf() 一致：只有 empty / inline 才渲染正文。
    if node.content.kind != "empty" && node.content.kind != "inline" {
        return String::new();
    }
    node.content.body.clone().unwrap_or_default()
}
