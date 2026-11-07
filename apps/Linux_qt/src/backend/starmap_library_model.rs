// =============================================================================
// starmap_library_model.rs — 星图素材库目录 QAbstractItemModel
// =============================================================================
//
// 层级：Linux_qt 平台端（Rust ↔ Qt 桥接层）
// 职责：为「全局素材库」「当前作品素材库」提供同一套扁平可缩进目录模型。
//   Core 返回素材 entry 与 folder 的原始归属；本模型只按 parentFolderId
//   组装可见行、记录展开状态，不复制星图业务状态。
//
// 边界：
//   - 模型只读 Core；新建文件夹、移动归属、复制、插入画布与删除本体仍由
//     QML 调 starmapBackend，成功后 refresh()。
//   - 行身份是 entryId / folderId；assetId 只用于创建实例引用和定位全部引用。
// =============================================================================

use super::*;
use cpp::cpp;
use qmetaobject::{QAbstractItemModel, QModelIndex};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::ffi::{c_void, CString};

cpp! {{
    #include <QtCore/QMetaObject>
    #include <QtCore/QObject>
    #include <QtCore/QVariant>
}}

const ROLE_TITLE: i32 = 300;
const ROLE_SUBTITLE: i32 = 301;
const ROLE_ROW_KIND: i32 = 302;
const ROLE_ASSET_ID: i32 = 303;
const ROLE_ENTRY_ID: i32 = 304;
const ROLE_FOLDER_ID: i32 = 305;
const ROLE_SCOPE_JSON: i32 = 306;
const ROLE_TARGET_JSON: i32 = 307;
const ROLE_DEPTH: i32 = 308;
const ROLE_EXPANDED: i32 = 309;
const ROLE_HAS_CHILDREN: i32 = 310;

const QT_DISPLAY_ROLE: i32 = 0;

#[derive(Clone, Default)]
struct LibraryRow {
    title: String,
    subtitle: String,
    row_kind: String,
    asset_id: String,
    entry_id: String,
    folder_id: String,
    scope_json: String,
    target_json: String,
    depth: i32,
    expanded: bool,
    has_children: bool,
}

#[derive(Default)]
struct LibraryState {
    rows: Vec<LibraryRow>,
    scope_json: String,
    expanded_folders: HashSet<String>,
}

#[allow(non_snake_case)]
#[derive(QObject, Default)]
pub struct StarmapLibraryModel {
    base: qt_base_class!(trait QAbstractItemModel),

    set_backend_ref: qt_method!(fn(&mut self, backend: QVariant)),
    set_scope_json: qt_method!(fn(&mut self, scope_json: QString)),
    refresh: qt_method!(fn(&self)),
    toggle_folder: qt_method!(fn(&self, folder_id: QString)),

    backend_ptr: usize,
    state: RefCell<LibraryState>,
}

impl StarmapLibraryModel {
    fn set_backend_ref(&mut self, backend: QVariant) {
        let ptr = cpp!(unsafe [backend as "QVariant"] -> *mut c_void as "QObject*" {
            return backend.value<QObject*>();
        });
        self.backend_ptr = ptr as usize;
    }

    fn set_scope_json(&mut self, scope_json: QString) {
        self.state.borrow_mut().scope_json = scope_json.to_string();
    }

    fn call_backend_json(&self, method: &str, arg: &str) -> Option<String> {
        if self.backend_ptr == 0 {
            return None;
        }
        let obj = self.backend_ptr as *mut c_void;
        let method_name = CString::new(method).ok()?;
        let method_ptr = method_name.as_ptr();
        let arg_value = QString::from(arg);
        let result: QString = cpp!(unsafe [
            obj as "QObject*",
            method_ptr as "const char*",
            arg_value as "QString"
        ] -> QString as "QString" {
            QString out;
            bool ok = QMetaObject::invokeMethod(
                obj, method_ptr, Qt::DirectConnection,
                Q_RETURN_ARG(QString, out), Q_ARG(QString, arg_value));
            return ok ? out : QString();
        });
        Some(result.to_string())
    }

    fn envelope_data(raw: &str) -> Option<serde_json::Value> {
        let value: serde_json::Value = serde_json::from_str(raw).ok()?;
        if value.get("success").and_then(serde_json::Value::as_bool) != Some(true) {
            return None;
        }
        value.get("data").cloned()
    }

    fn refresh_internal(&self) {
        let scope_json = self.state.borrow().scope_json.clone();
        let data = self
            .call_backend_json("list_starmap_library_json", &scope_json)
            .as_deref()
            .and_then(Self::envelope_data)
            .unwrap_or(serde_json::Value::Null);
        let folders = data
            .get("folders")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        let entries = data
            .get("entries")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();

        let expanded = self.state.borrow().expanded_folders.clone();
        let mut rows = Vec::new();
        Self::push_folder_children(
            &scope_json,
            None,
            0,
            &folders,
            &entries,
            &expanded,
            &mut rows,
        );

        self.begin_reset_model();
        self.state.borrow_mut().rows = rows;
        self.end_reset_model();
    }

    fn push_folder_children(
        scope_json: &str,
        parent: Option<&str>,
        depth: i32,
        folders: &[serde_json::Value],
        entries: &[serde_json::Value],
        expanded: &HashSet<String>,
        rows: &mut Vec<LibraryRow>,
    ) {
        for folder in folders {
            let Some(folder_id) = folder
                .get("folderId")
                .and_then(serde_json::Value::as_str)
                .filter(|value| !value.is_empty())
            else {
                continue;
            };
            let folder_parent = folder
                .get("parentFolderId")
                .and_then(serde_json::Value::as_str)
                .filter(|value| !value.is_empty());
            if folder_parent != parent {
                continue;
            }
            let is_expanded = expanded.contains(folder_id);
            let has_children = folders.iter().any(|candidate| {
                candidate
                    .get("parentFolderId")
                    .and_then(serde_json::Value::as_str)
                    == Some(folder_id)
            }) || entries.iter().any(|candidate| {
                candidate
                    .get("entry")
                    .and_then(|entry| entry.get("parentFolderId"))
                    .and_then(serde_json::Value::as_str)
                    == Some(folder_id)
            });
            rows.push(LibraryRow {
                title: folder
                    .get("name")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("文件夹")
                    .to_string(),
                subtitle: String::new(),
                row_kind: "folder".to_string(),
                folder_id: folder_id.to_string(),
                scope_json: scope_json.to_string(),
                depth,
                expanded: is_expanded,
                has_children,
                ..Default::default()
            });
            if is_expanded {
                Self::push_folder_children(
                    scope_json,
                    Some(folder_id),
                    depth + 1,
                    folders,
                    entries,
                    expanded,
                    rows,
                );
            }
        }

        for wrapped in entries {
            let Some(entry) = wrapped.get("entry") else {
                continue;
            };
            let entry_parent = entry
                .get("parentFolderId")
                .and_then(serde_json::Value::as_str)
                .filter(|value| !value.is_empty());
            if entry_parent != parent {
                continue;
            }
            let Some(target) = entry.get("target") else {
                continue;
            };
            let kind = target
                .get("kind")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("node");
            let Some(asset_id) = target
                .get("assetId")
                .and_then(serde_json::Value::as_str)
                .filter(|value| !value.is_empty())
            else {
                continue;
            };
            rows.push(LibraryRow {
                title: wrapped
                    .get("title")
                    .and_then(serde_json::Value::as_str)
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or(asset_id)
                    .to_string(),
                subtitle: if kind == "starmap" {
                    "星图".to_string()
                } else {
                    "节点".to_string()
                },
                row_kind: kind.to_string(),
                asset_id: asset_id.to_string(),
                entry_id: entry
                    .get("entryId")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                scope_json: scope_json.to_string(),
                target_json: target.to_string(),
                depth,
                ..Default::default()
            });
        }
    }

    fn row_for_index(&self, index: QModelIndex) -> Option<usize> {
        if !index.is_valid() {
            return None;
        }
        let row = index.id();
        self.state.borrow().rows.get(row).map(|_| row)
    }

    fn refresh(&self) {
        self.refresh_internal();
    }

    fn toggle_folder(&self, folder_id: QString) {
        let folder_id = folder_id.to_string();
        {
            let mut state = self.state.borrow_mut();
            if !state.expanded_folders.remove(&folder_id) {
                state.expanded_folders.insert(folder_id);
            }
        }
        self.refresh_internal();
    }
}

impl QAbstractItemModel for StarmapLibraryModel {
    fn index(&self, row: i32, column: i32, _parent: QModelIndex) -> QModelIndex {
        if row < 0 || column < 0 {
            return QModelIndex::default();
        }
        if row as usize >= self.state.borrow().rows.len() {
            return QModelIndex::default();
        }
        self.create_index(row, column, row as usize)
    }

    fn parent(&self, _index: QModelIndex) -> QModelIndex {
        QModelIndex::default()
    }

    fn row_count(&self, _parent: QModelIndex) -> i32 {
        self.state.borrow().rows.len() as i32
    }

    fn column_count(&self, _parent: QModelIndex) -> i32 {
        1
    }

    fn data(&self, index: QModelIndex, role: i32) -> QVariant {
        let Some(row_index) = self.row_for_index(index) else {
            return QVariant::default();
        };
        let state = self.state.borrow();
        let Some(row) = state.rows.get(row_index) else {
            return QVariant::default();
        };
        match role {
            QT_DISPLAY_ROLE => QVariant::from(QString::from(row.title.as_str())),
            ROLE_TITLE => QVariant::from(QString::from(row.title.as_str())),
            ROLE_SUBTITLE => QVariant::from(QString::from(row.subtitle.as_str())),
            ROLE_ROW_KIND => QVariant::from(QString::from(row.row_kind.as_str())),
            ROLE_ASSET_ID => QVariant::from(QString::from(row.asset_id.as_str())),
            ROLE_ENTRY_ID => QVariant::from(QString::from(row.entry_id.as_str())),
            ROLE_FOLDER_ID => QVariant::from(QString::from(row.folder_id.as_str())),
            ROLE_SCOPE_JSON => QVariant::from(QString::from(row.scope_json.as_str())),
            ROLE_TARGET_JSON => QVariant::from(QString::from(row.target_json.as_str())),
            ROLE_DEPTH => QVariant::from(row.depth),
            ROLE_EXPANDED => QVariant::from(row.expanded),
            ROLE_HAS_CHILDREN => QVariant::from(row.has_children),
            _ => QVariant::default(),
        }
    }

    fn role_names(&self) -> HashMap<i32, QByteArray> {
        let mut names = HashMap::new();
        names.insert(ROLE_TITLE, QByteArray::from("title"));
        names.insert(ROLE_SUBTITLE, QByteArray::from("subtitle"));
        names.insert(ROLE_ROW_KIND, QByteArray::from("rowKind"));
        names.insert(ROLE_ASSET_ID, QByteArray::from("assetId"));
        names.insert(ROLE_ENTRY_ID, QByteArray::from("entryId"));
        names.insert(ROLE_FOLDER_ID, QByteArray::from("folderId"));
        names.insert(ROLE_SCOPE_JSON, QByteArray::from("scopeJson"));
        names.insert(ROLE_TARGET_JSON, QByteArray::from("targetJson"));
        names.insert(ROLE_DEPTH, QByteArray::from("depth"));
        names.insert(ROLE_EXPANDED, QByteArray::from("expanded"));
        names.insert(ROLE_HAS_CHILDREN, QByteArray::from("hasChildren"));
        names
    }
}
