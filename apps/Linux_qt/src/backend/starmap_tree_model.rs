// =============================================================================
// starmap_tree_model.rs — 星图导航树 QAbstractItemModel
// =============================================================================
//
// 层级：Linux_qt 平台端（Rust ↔ Qt 桥接层）
// 职责：给 StarMapNavigationTree.qml 的 Qt 6.8 原生 TreeView 提供树模型：
//   - 根层来自现有 `list_root_starmaps_json()`；
//   - 子层在该星图第一次展开时调用 `get_starmap_graph_json()`，把 Embed /
//     旧 Portal 实例转成子行；
//   - 每一行都带完整路由（rootStarmapId + segments），同一个子星图通过不同
//     Embed 到达时 routeKey 不同，因此树节点身份不是裸 starmapId。
//
// 边界：
//   - 模型只读 Core 数据，不缓存第二套星图业务状态；新建 / 重命名 / 删除仍由
//     Workspace 调 starmapBackend 完成，成功后调用 refresh()/set_starmap_title()。
//   - 层级不设人为上限；每次展开只懒加载当前分支的下一层。
// =============================================================================

use super::*;
use cpp::cpp;
use qmetaobject::{QAbstractItemModel, QModelIndex};
use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::{c_void, CString};
use writer_core::api::types::{StarMapGraphDto, StarMapMetaDto};

cpp! {{
    #include <QtCore/QMetaObject>
    #include <QtCore/QObject>
    #include <QtCore/QVariant>
}}

const ROLE_TITLE: i32 = 257;
const ROLE_STARMAP_ID: i32 = 258;
const ROLE_ROOT_STARMAP_ID: i32 = 259;
const ROLE_ROUTE_SEGMENTS: i32 = 260;
const ROLE_ROUTE_KEY: i32 = 261;
const ROLE_HAS_CHILDREN: i32 = 262;
const ROLE_EXPANDED: i32 = 263;
const ROLE_ROW_KIND: i32 = 264;
const ROLE_PARENT_STARMAP_ID: i32 = 265;
const ROLE_INSTANCE_ID: i32 = 266;
const ROLE_NODE_ID: i32 = 267;
const ROLE_REVISION: i32 = 268;
const ROLE_DEPTH: i32 = 269;

const QT_DISPLAY_ROLE: i32 = 0;
const QT_EDIT_ROLE: i32 = 2;

#[derive(Clone, Debug, PartialEq, Eq)]
enum RouteSegment {
    Embed { instance_id: String },
    Portal { node_id: String },
}

impl RouteSegment {
    fn key_part(&self) -> String {
        match self {
            Self::Embed { instance_id } => format!("enterEmbed:{instance_id}"),
            Self::Portal { node_id } => format!("enterPortal:{node_id}"),
        }
    }

    fn to_json_value(&self) -> serde_json::Value {
        match self {
            Self::Embed { instance_id } => serde_json::json!({
                "type": "enterEmbed",
                "instanceId": instance_id,
                "nodeId": serde_json::Value::Null,
            }),
            Self::Portal { node_id } => serde_json::json!({
                "type": "enterPortal",
                "instanceId": serde_json::Value::Null,
                "nodeId": node_id,
            }),
        }
    }
}

fn route_key(root_starmap_id: &str, segments: &[RouteSegment]) -> String {
    let parts: Vec<String> = segments.iter().map(RouteSegment::key_part).collect();
    format!("{root_starmap_id}|{}", parts.join("/"))
}

fn segments_json(segments: &[RouteSegment]) -> String {
    let values: Vec<serde_json::Value> = segments.iter().map(RouteSegment::to_json_value).collect();
    serde_json::to_string(&values).unwrap_or_else(|_| "[]".to_string())
}

/// 子项状态：未探测、确定有子项、确定为空。不能把“未加载”当成有子项，
/// 否则叶子节点会先出现可展开箭头。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ChildrenState {
    Unknown,
    Present,
    Empty,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RowKind {
    Root,
    Embed,
    Portal,
}

impl RowKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Root => "root",
            Self::Embed => "embed",
            Self::Portal => "portal",
        }
    }
}

struct TreeRow {
    parent: Option<usize>,
    title: String,
    /// 打开这一行后实际进入的星图 ID。
    starmap_id: String,
    root_starmap_id: String,
    segments: Vec<RouteSegment>,
    /// 子行所属的容器星图（父行实际加载的星图）；根行为空。
    parent_starmap_id: String,
    kind: RowKind,
    revision: u64,
    depth: i32,
    children: Vec<usize>,
    children_state: ChildrenState,
    expanded: bool,
}

impl TreeRow {
    fn route_key(&self) -> String {
        route_key(&self.root_starmap_id, &self.segments)
    }

    fn instance_id(&self) -> String {
        match self.segments.last() {
            Some(RouteSegment::Embed { instance_id }) => instance_id.clone(),
            _ => String::new(),
        }
    }

    fn node_id(&self) -> String {
        match self.segments.last() {
            Some(RouteSegment::Portal { node_id }) => node_id.clone(),
            _ => String::new(),
        }
    }
}

#[derive(Default)]
struct TreeState {
    rows: Vec<TreeRow>,
    root_ids: Vec<usize>,
    index_by_route: HashMap<String, usize>,
    titles: HashMap<String, String>,
}

struct ChildSpec {
    kind: RowKind,
    title: String,
    starmap_id: String,
    segment: RouteSegment,
    revision: u64,
}

#[allow(non_snake_case)]
#[derive(QObject, Default)]
pub struct StarmapTreeModel {
    base: qt_base_class!(trait QAbstractItemModel),

    set_backend_ref: qt_method!(fn(&mut self, backend: QVariant)),
    refresh: qt_method!(fn(&self)),
    ensure_children_loaded: qt_method!(fn(&self, route_key: QString) -> bool),
    index_for_route:
        qt_method!(fn(&self, root_starmap_id: QString, segments_json: QString) -> QModelIndex),
    /// 用完整 routeKey 找回行并懒加载路径；刷新后恢复展开状态使用。
    index_for_key: qt_method!(fn(&self, route_key: QString) -> QModelIndex),
    route_key_of: qt_method!(fn(&self, index: QModelIndex) -> QString),
    set_expanded: qt_method!(fn(&self, route_key: QString, expanded: bool)),
    set_starmap_title: qt_method!(fn(&self, starmap_id: QString, title: QString)),
    remove_starmap: qt_method!(fn(&self, starmap_id: QString)),

    /// StarMapBackend 的 QObject 指针。QML 在组件完成时注入；为空时模型只是空树。
    backend_ptr: usize,
    state: RefCell<TreeState>,
}

impl StarmapTreeModel {
    fn set_backend_ref(&mut self, backend: QVariant) {
        let ptr = cpp!(unsafe [backend as "QVariant"] -> *mut c_void as "QObject*" {
            return backend.value<QObject*>();
        });
        self.backend_ptr = ptr as usize;
    }

    fn call_backend_json(&self, method: &str, arg: Option<&str>) -> Option<String> {
        if self.backend_ptr == 0 {
            return None;
        }
        let obj = self.backend_ptr as *mut c_void;
        let method_name = CString::new(method).ok()?;
        let method_ptr = method_name.as_ptr();
        let result: QString = match arg {
            Some(value) => {
                let arg_value = QString::from(value);
                cpp!(unsafe [
                    obj as "QObject*",
                    method_ptr as "const char*",
                    arg_value as "QString"
                ] -> QString as "QString" {
                    QString out;
                    bool ok = QMetaObject::invokeMethod(
                        obj, method_ptr, Qt::DirectConnection,
                        Q_RETURN_ARG(QString, out), Q_ARG(QString, arg_value));
                    return ok ? out : QString();
                })
            }
            None => cpp!(unsafe [
                obj as "QObject*",
                method_ptr as "const char*"
            ] -> QString as "QString" {
                QString out;
                bool ok = QMetaObject::invokeMethod(
                    obj, method_ptr, Qt::DirectConnection,
                    Q_RETURN_ARG(QString, out));
                return ok ? out : QString();
            }),
        };
        Some(result.to_string())
    }

    fn envelope_data(raw: &str) -> Option<serde_json::Value> {
        let value: serde_json::Value = serde_json::from_str(raw).ok()?;
        if value.get("success").and_then(serde_json::Value::as_bool) != Some(true) {
            return None;
        }
        value.get("data").cloned()
    }

    fn title_for_starmap(&self, starmap_id: &str) -> Option<String> {
        if let Some(title) = self.state.borrow().titles.get(starmap_id) {
            return Some(title.clone());
        }
        let raw = self.call_backend_json("get_starmap_json", Some(starmap_id))?;
        let data = Self::envelope_data(&raw)?;
        let meta: StarMapMetaDto = serde_json::from_value(data).ok()?;
        let title = meta.title;
        self.state
            .borrow_mut()
            .titles
            .insert(starmap_id.to_string(), title.clone());
        Some(title)
    }

    fn index_for_node(&self, node_id: usize) -> QModelIndex {
        let row_index = {
            let state = self.state.borrow();
            let Some(row) = state.rows.get(node_id) else {
                return QModelIndex::default();
            };
            match row.parent {
                Some(parent_id) => state
                    .rows
                    .get(parent_id)
                    .and_then(|parent| parent.children.iter().position(|id| *id == node_id))
                    .map_or(-1, |row| row as i32),
                None => state
                    .root_ids
                    .iter()
                    .position(|id| *id == node_id)
                    .map_or(-1, |row| row as i32),
            }
        };
        if row_index < 0 {
            return QModelIndex::default();
        }
        // QModelIndex 不保存父指针；父子关系由 model->parent() 反查。
        self.create_index(row_index, 0, node_id)
    }

    fn node_at_route(&self, key: &str) -> Option<usize> {
        self.state.borrow().index_by_route.get(key).copied()
    }

    /// 准确的三态判断。未探测时读取一次真实 graph，只缓存“有/无子项”，
    /// 不插入子行；子行仍在用户展开时懒加载。
    fn row_has_children(&self, node_id: usize) -> bool {
        let (state, starmap_id) = {
            let state = self.state.borrow();
            let Some(row) = state.rows.get(node_id) else {
                return false;
            };
            (row.children_state, row.starmap_id.clone())
        };
        match state {
            ChildrenState::Present => true,
            ChildrenState::Empty => false,
            ChildrenState::Unknown => {
                let Some(specs) = self.load_child_specs(&starmap_id) else {
                    // 读取失败时保留 Unknown 并临时显示箭头，允许用户重试；
                    // 不能把一次后端失败缓存成“确定为空”。
                    return true;
                };
                let present = !specs.is_empty();
                if let Ok(mut state) = self.state.try_borrow_mut() {
                    if let Some(row) = state.rows.get_mut(node_id) {
                        row.children_state = if present {
                            ChildrenState::Present
                        } else {
                            ChildrenState::Empty
                        };
                    }
                }
                present
            }
        }
    }

    fn row_for_index(&self, index: QModelIndex) -> Option<usize> {
        if !index.is_valid() {
            return None;
        }
        let node_id = index.id();
        self.state.borrow().rows.get(node_id).map(|_| node_id)
    }

    fn refresh_internal(&self) {
        let roots: Vec<StarMapMetaDto> = self
            .call_backend_json("list_root_starmaps_json", None)
            .as_deref()
            .and_then(Self::envelope_data)
            .and_then(|data| serde_json::from_value(data).ok())
            .unwrap_or_default();

        self.begin_reset_model();
        {
            let mut state = self.state.borrow_mut();
            state.rows.clear();
            state.root_ids.clear();
            state.index_by_route.clear();
            state.titles.clear();
            for meta in roots {
                let node_id = state.rows.len();
                let root_id = meta.starmap_id;
                state.titles.insert(root_id.clone(), meta.title.clone());
                state.rows.push(TreeRow {
                    parent: None,
                    title: meta.title,
                    starmap_id: root_id.clone(),
                    root_starmap_id: root_id.clone(),
                    segments: Vec::new(),
                    parent_starmap_id: String::new(),
                    kind: RowKind::Root,
                    revision: meta.updated_at,
                    depth: 0,
                    children: Vec::new(),
                    children_state: ChildrenState::Unknown,
                    expanded: false,
                });
                state.root_ids.push(node_id);
                state
                    .index_by_route
                    .insert(route_key(&root_id, &[]), node_id);
            }
        }
        self.end_reset_model();
    }

    fn load_child_specs(&self, starmap_id: &str) -> Option<Vec<ChildSpec>> {
        self.call_backend_json("get_starmap_graph_json", Some(starmap_id))
            .as_deref()
            .and_then(Self::envelope_data)
            .and_then(|data| data.get("graph").cloned())
            .and_then(|graph| serde_json::from_value::<StarMapGraphDto>(graph).ok())
            .map(|graph| {
                let mut children = Vec::new();
                for embed in graph.embeds {
                    let target_starmap_id = embed.target_starmap_id;
                    let instance_id = embed.instance_id;
                    let title = embed
                        .label
                        .filter(|label| !label.trim().is_empty())
                        .or_else(|| self.title_for_starmap(&target_starmap_id))
                        .unwrap_or_else(|| target_starmap_id.clone());
                    children.push(ChildSpec {
                        kind: RowKind::Embed,
                        title,
                        starmap_id: target_starmap_id,
                        segment: RouteSegment::Embed { instance_id },
                        revision: embed.updated_at,
                    });
                }
                for node in graph.nodes {
                    let Some(portal) = node.portal else {
                        continue;
                    };
                    let target_starmap_id = portal.destination_starmap_id;
                    let title = if node.title.trim().is_empty() {
                        self.title_for_starmap(&target_starmap_id)
                            .unwrap_or_else(|| target_starmap_id.clone())
                    } else {
                        node.title
                    };
                    children.push(ChildSpec {
                        kind: RowKind::Portal,
                        title,
                        starmap_id: target_starmap_id,
                        segment: RouteSegment::Portal { node_id: node.id },
                        revision: node.updated_at,
                    });
                }
                children
            })
    }

    fn ensure_children_loaded_internal(&self, key: &str) -> bool {
        let Some(node_id) = self.node_at_route(key) else {
            return false;
        };
        let (
            depth,
            children_state,
            children_loaded,
            starmap_id,
            root_starmap_id,
            segments,
            parent_starmap_id,
        ) = {
            let state = self.state.borrow();
            let Some(row) = state.rows.get(node_id) else {
                return false;
            };
            (
                row.depth,
                row.children_state,
                !row.children.is_empty(),
                row.starmap_id.clone(),
                row.root_starmap_id.clone(),
                row.segments.clone(),
                row.starmap_id.clone(),
            )
        };
        // `row_has_children` 探测真实 graph 后只缓存 Present/Empty，不会插入
        // 子行；因此 Present 但 children 为空时仍然必须在这里真正加载一次。
        if children_state == ChildrenState::Empty
            || (children_state == ChildrenState::Present && children_loaded)
        {
            return true;
        }

        let Some(specs) = self.load_child_specs(&starmap_id) else {
            return false;
        };

        if specs.is_empty() {
            let mut state = self.state.borrow_mut();
            if let Some(row) = state.rows.get_mut(node_id) {
                row.children_state = ChildrenState::Empty;
            }
            drop(state);
            let index = self.index_for_node(node_id);
            self.data_changed(index, index);
            return true;
        }

        let parent_index = self.index_for_node(node_id);
        let child_count = specs.len() as i32;
        self.begin_insert_rows(parent_index, 0, child_count - 1);
        {
            let mut state = self.state.borrow_mut();
            let mut child_ids = Vec::with_capacity(specs.len());
            for spec in specs {
                let child_id = state.rows.len();
                let mut child_segments = segments.clone();
                child_segments.push(spec.segment);
                let child_key = route_key(&root_starmap_id, &child_segments);
                state.rows.push(TreeRow {
                    parent: Some(node_id),
                    title: spec.title,
                    starmap_id: spec.starmap_id,
                    root_starmap_id: root_starmap_id.clone(),
                    segments: child_segments,
                    parent_starmap_id: parent_starmap_id.clone(),
                    kind: spec.kind,
                    revision: spec.revision,
                    depth: depth + 1,
                    children: Vec::new(),
                    children_state: ChildrenState::Unknown,
                    expanded: false,
                });
                state.index_by_route.insert(child_key, child_id);
                child_ids.push(child_id);
            }
            if let Some(row) = state.rows.get_mut(node_id) {
                row.children = child_ids;
                row.children_state = ChildrenState::Present;
                row.expanded = true;
            }
        }
        self.end_insert_rows();
        let index = self.index_for_node(node_id);
        self.data_changed(index, index);
        true
    }
}

impl QAbstractItemModel for StarmapTreeModel {
    fn index(&self, row: i32, column: i32, parent: QModelIndex) -> QModelIndex {
        if row < 0 || column < 0 {
            return QModelIndex::default();
        }
        let node_id = if parent.is_valid() {
            let parent_id = parent.id();
            let state = self.state.borrow();
            let Some(parent_row) = state.rows.get(parent_id) else {
                return QModelIndex::default();
            };
            parent_row.children.get(row as usize).copied()
        } else {
            self.state.borrow().root_ids.get(row as usize).copied()
        };
        match node_id {
            Some(id) => self.create_index(row, column, id),
            None => QModelIndex::default(),
        }
    }

    fn parent(&self, index: QModelIndex) -> QModelIndex {
        let Some(node_id) = self.row_for_index(index) else {
            return QModelIndex::default();
        };
        let parent_id = {
            let state = self.state.borrow();
            state.rows.get(node_id).and_then(|row| row.parent)
        };
        match parent_id {
            Some(parent_id) => self.index_for_node(parent_id),
            None => QModelIndex::default(),
        }
    }

    fn row_count(&self, parent: QModelIndex) -> i32 {
        if !parent.is_valid() {
            return self.state.borrow().root_ids.len() as i32;
        }
        let parent_id = parent.id();
        self.state
            .borrow()
            .rows
            .get(parent_id)
            .map_or(0, |row| row.children.len() as i32)
    }

    fn column_count(&self, _parent: QModelIndex) -> i32 {
        1
    }

    fn data(&self, index: QModelIndex, role: i32) -> QVariant {
        let Some(node_id) = self.row_for_index(index) else {
            return QVariant::default();
        };
        if role == ROLE_HAS_CHILDREN {
            return QVariant::from(self.row_has_children(node_id));
        }
        let state = self.state.borrow();
        let Some(row) = state.rows.get(node_id) else {
            return QVariant::default();
        };
        match role {
            QT_DISPLAY_ROLE | QT_EDIT_ROLE => QVariant::from(QString::from(row.title.as_str())),
            ROLE_TITLE => QVariant::from(QString::from(row.title.as_str())),
            ROLE_STARMAP_ID => QVariant::from(QString::from(row.starmap_id.as_str())),
            ROLE_ROOT_STARMAP_ID => QVariant::from(QString::from(row.root_starmap_id.as_str())),
            ROLE_ROUTE_SEGMENTS => QVariant::from(QString::from(segments_json(&row.segments))),
            ROLE_ROUTE_KEY => QVariant::from(QString::from(row.route_key())),
            ROLE_EXPANDED => QVariant::from(row.expanded),
            ROLE_ROW_KIND => QVariant::from(QString::from(row.kind.as_str())),
            ROLE_PARENT_STARMAP_ID => QVariant::from(QString::from(row.parent_starmap_id.as_str())),
            ROLE_INSTANCE_ID => QVariant::from(QString::from(row.instance_id())),
            ROLE_NODE_ID => QVariant::from(QString::from(row.node_id())),
            ROLE_REVISION => QVariant::from(row.revision),
            ROLE_DEPTH => QVariant::from(row.depth),
            _ => QVariant::default(),
        }
    }

    fn role_names(&self) -> HashMap<i32, QByteArray> {
        let mut names = HashMap::new();
        names.insert(ROLE_TITLE, QByteArray::from("title"));
        names.insert(ROLE_STARMAP_ID, QByteArray::from("starmapId"));
        names.insert(ROLE_ROOT_STARMAP_ID, QByteArray::from("rootStarmapId"));
        names.insert(ROLE_ROUTE_SEGMENTS, QByteArray::from("routeSegments"));
        names.insert(ROLE_ROUTE_KEY, QByteArray::from("routeKey"));
        names.insert(ROLE_HAS_CHILDREN, QByteArray::from("hasChildren"));
        names.insert(ROLE_EXPANDED, QByteArray::from("expanded"));
        names.insert(ROLE_ROW_KIND, QByteArray::from("rowKind"));
        names.insert(ROLE_PARENT_STARMAP_ID, QByteArray::from("parentStarmapId"));
        names.insert(ROLE_INSTANCE_ID, QByteArray::from("instanceId"));
        names.insert(ROLE_NODE_ID, QByteArray::from("nodeId"));
        names.insert(ROLE_REVISION, QByteArray::from("revision"));
        names.insert(ROLE_DEPTH, QByteArray::from("depth"));
        names
    }
}

impl StarmapTreeModel {
    fn refresh(&self) {
        self.refresh_internal();
    }

    fn ensure_children_loaded(&self, route_key: QString) -> bool {
        self.ensure_children_loaded_internal(&route_key.to_string())
    }

    fn index_for_route(&self, root_starmap_id: QString, segments_json: QString) -> QModelIndex {
        let root_id = root_starmap_id.to_string();
        let raw_segments: Vec<serde_json::Value> =
            serde_json::from_str(&segments_json.to_string()).unwrap_or_default();
        let mut segments: Vec<RouteSegment> = Vec::new();
        for raw in raw_segments {
            let kind = raw
                .get("type")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            match kind {
                "enterEmbed" => {
                    let Some(instance_id) = raw
                        .get("instanceId")
                        .and_then(serde_json::Value::as_str)
                        .filter(|value| !value.is_empty())
                    else {
                        return QModelIndex::default();
                    };
                    segments.push(RouteSegment::Embed {
                        instance_id: instance_id.to_string(),
                    });
                }
                "enterPortal" => {
                    let Some(node_id) = raw
                        .get("nodeId")
                        .and_then(serde_json::Value::as_str)
                        .filter(|value| !value.is_empty())
                    else {
                        return QModelIndex::default();
                    };
                    segments.push(RouteSegment::Portal {
                        node_id: node_id.to_string(),
                    });
                }
                _ => return QModelIndex::default(),
            }
        }

        self.index_for_segments(&root_id, &segments)
    }

    fn index_for_segments(&self, root_id: &str, segments: &[RouteSegment]) -> QModelIndex {
        let Some(mut node_id) = self.node_at_route(&route_key(root_id, &[])) else {
            return QModelIndex::default();
        };
        let mut walked: Vec<RouteSegment> = Vec::new();
        for segment in segments {
            let parent_key = route_key(root_id, &walked);
            if !self.ensure_children_loaded_internal(&parent_key) {
                return QModelIndex::default();
            }
            let next = {
                let state = self.state.borrow();
                let Some(parent) = state.rows.get(node_id) else {
                    return QModelIndex::default();
                };
                parent
                    .children
                    .iter()
                    .find(|child_id| {
                        state
                            .rows
                            .get(**child_id)
                            .and_then(|child| child.segments.last())
                            .is_some_and(|last| last == segment)
                    })
                    .copied()
            };
            let Some(next_id) = next else {
                return QModelIndex::default();
            };
            walked.push(segment.clone());
            node_id = next_id;
        }
        self.index_for_node(node_id)
    }

    fn index_for_key(&self, key: QString) -> QModelIndex {
        let key = key.to_string();
        let Some((root_id, rest)) = key.split_once('|') else {
            return QModelIndex::default();
        };
        if rest.is_empty() {
            return self.index_for_segments(root_id, &[]);
        }
        let mut segments = Vec::new();
        for part in rest.split('/') {
            if let Some(instance_id) = part.strip_prefix("enterEmbed:") {
                if instance_id.is_empty() {
                    return QModelIndex::default();
                }
                segments.push(RouteSegment::Embed {
                    instance_id: instance_id.to_string(),
                });
            } else if let Some(node_id) = part.strip_prefix("enterPortal:") {
                if node_id.is_empty() {
                    return QModelIndex::default();
                }
                segments.push(RouteSegment::Portal {
                    node_id: node_id.to_string(),
                });
            } else {
                return QModelIndex::default();
            }
        }
        self.index_for_segments(root_id, &segments)
    }

    fn route_key_of(&self, index: QModelIndex) -> QString {
        let Some(node_id) = self.row_for_index(index) else {
            return QString::default();
        };
        let key = self
            .state
            .borrow()
            .rows
            .get(node_id)
            .map_or_else(String::new, TreeRow::route_key);
        QString::from(key)
    }

    fn set_expanded(&self, route_key: QString, expanded: bool) {
        let Some(node_id) = self.node_at_route(&route_key.to_string()) else {
            return;
        };
        {
            let mut state = self.state.borrow_mut();
            if let Some(row) = state.rows.get_mut(node_id) {
                row.expanded = expanded;
            }
        }
        let index = self.index_for_node(node_id);
        self.data_changed(index, index);
    }

    fn set_starmap_title(&self, starmap_id: QString, title: QString) {
        let starmap_id = starmap_id.to_string();
        let title = title.to_string();
        let changed: Vec<usize> = {
            let mut state = self.state.borrow_mut();
            state.titles.insert(starmap_id.clone(), title.clone());
            state
                .rows
                .iter_mut()
                .enumerate()
                .filter_map(|(node_id, row)| {
                    if row.starmap_id == starmap_id {
                        row.title = title.clone();
                        Some(node_id)
                    } else {
                        None
                    }
                })
                .collect()
        };
        for node_id in changed {
            let index = self.index_for_node(node_id);
            self.data_changed(index, index);
        }
    }

    fn remove_starmap(&self, _starmap_id: QString) {
        // 删除后的层级关系由 Core 重新计算；整棵树重读，避免本地拼一份
        // 可能与 Core 不一致的“孤儿行”状态。
        self.refresh_internal();
    }
}
