pub mod app_backend;
pub mod json_utils;

use qmetaobject::prelude::*;

pub use app_backend::{
    AppBackend, EditorBackend, ProjectBackend, SettingsBackend, StarMapBackend, SyncBackend,
    WorkspaceBackend,
};

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
        let app_ptr = {
            let app_pinned = app_backend.pinned();
            let app_ref = app_pinned.borrow();
            QPointer::from(&*app_ref)
        };

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
