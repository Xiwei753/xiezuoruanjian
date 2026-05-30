pub mod app_backend;
pub mod json_utils;

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
        if p.is_null() { None } else { Some(p) }
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
        let mut app_backend = QObjectBox::new(AppBackend::default());
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
