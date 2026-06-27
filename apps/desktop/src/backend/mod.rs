// =============================================================================
// mod.rs — Desktop 客户端 QObject 后端聚合与生命周期管理器
// =============================================================================
//
// 引用了什么：
// - qmetaobject::QObjectBox/QmlEngine/pinned：Qt 绑定宏与底层引擎控制接口。
// - app_backend：引入各领域具体的后端的强类型结构体定义。
//
// 干什么的：
// - 声明并聚合所有的后台适配器子模块（workspace_backend、sync_backend 等）。
// - 实现 SafeAppPtr 指针安全共享机制，用于使子模块获取主 AppBackend 的上下文指针，避免内存泄漏与生命周期交叉问题。
// - 提供 BackendRuntime 所有者结构体，强引用并管理所有被 QObjectBox 包装的后端适配器，保证它们的生命周期贯穿 Qt 程序的整个生命周期。
// - 提供 register_context_properties 方法，负责在 main.qml 加载前一站式绑定所有的 Context Property。
//
// 被什么引用：
// - 被 apps/desktop/src/main.rs 引用，用于实例化 BackendRuntime 并注册全局 QML 环境属性。
// =============================================================================


pub mod app_backend;
pub mod desktop_layout_plan_dto;
pub mod json_utils;
pub(crate) mod message_key_mapper;

use qmetaobject::prelude::*;

pub use app_backend::{
    AppBackend, EditorBackend, ProjectBackend, SettingsBackend, StarMapBackend, SyncBackend,
    WorkspaceBackend,
};

/// Safely shares the heap-allocated AppBackend pointer with other domain backends.
/// Guaranteed to be valid as long as BackendRuntime is alive.
#[derive(Clone)]
pub struct SafeAppPtr {
    ptr: std::rc::Rc<std::cell::Cell<*mut AppBackend>>,
}

impl SafeAppPtr {
    pub fn new() -> Self {
        Self {
            ptr: std::rc::Rc::new(std::cell::Cell::new(std::ptr::null_mut())),
        }
    }
    pub fn set(&self, app: *mut AppBackend) {
        self.ptr.set(app);
    }
    pub fn get(&self) -> Option<*mut AppBackend> {
        let p = self.ptr.get();
        if p.is_null() {
            None
        } else {
            Some(p)
        }
    }
}

impl Default for SafeAppPtr {
    fn default() -> Self {
        Self::new()
    }
}

/// Owns every QObject registered into the QML root context.
///
/// Qt only receives raw QObject pointers from `set_object_property`; this
/// runtime keeps the Rust QObjectBox owners alive until after `engine.exec()`.
pub struct BackendRuntime {
    app_backend: QObjectBox<AppBackend>,
    workspace_backend: QObjectBox<WorkspaceBackend>,
    project_backend: QObjectBox<ProjectBackend>,
    editor_backend: QObjectBox<EditorBackend>,
    settings_backend: QObjectBox<SettingsBackend>,
    sync_backend: QObjectBox<SyncBackend>,
    starmap_backend: QObjectBox<StarMapBackend>,
}

impl BackendRuntime {
    pub fn new() -> Self {
        let app_backend = QObjectBox::new(AppBackend::default());
        let app_ptr = SafeAppPtr::new();
        // Safe because app_backend is pinned in the heap inside QObjectBox
        // and its lifetime is identical to BackendRuntime.
        let raw_ptr = {
            let pinned = app_backend.pinned();
            let r = pinned.borrow();
            &*r as *const AppBackend as *mut AppBackend
        };
        app_ptr.set(raw_ptr);

        Self {
            workspace_backend: QObjectBox::new(WorkspaceBackend::new(app_ptr.clone())),
            project_backend: QObjectBox::new(ProjectBackend::new(app_ptr.clone())),
            editor_backend: QObjectBox::new(EditorBackend::new(app_ptr.clone())),
            settings_backend: QObjectBox::new(SettingsBackend::new(app_ptr.clone())),
            sync_backend: QObjectBox::new(SyncBackend::new(app_ptr.clone())),
            starmap_backend: QObjectBox::new(StarMapBackend::new(app_ptr)),
            app_backend,
        }
    }

    pub fn register_context_properties(&self, engine: &mut QmlEngine) {
        engine.set_object_property("backend".into(), self.app_backend.pinned());
        engine.set_object_property("appBackend".into(), self.app_backend.pinned());
        engine.set_object_property("workspaceBackend".into(), self.workspace_backend.pinned());
        engine.set_object_property("projectBackend".into(), self.project_backend.pinned());
        engine.set_object_property("editorBackend".into(), self.editor_backend.pinned());
        engine.set_object_property("settingsBackend".into(), self.settings_backend.pinned());
        engine.set_object_property("syncBackend".into(), self.sync_backend.pinned());
        engine.set_object_property("starmapBackend".into(), self.starmap_backend.pinned());
    }
}
