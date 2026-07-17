// =============================================================================
// mod.rs — Linux_qt 客户端 QObject 后端聚合与生命周期管理器
// =============================================================================

pub mod app_backend;
pub mod linux_qt_layout_plan_dto;
pub mod linux_theme_controller;
pub mod diagnostics;
pub mod json_utils;
pub(crate) mod message_key_mapper;

use qmetaobject::prelude::*;

pub use app_backend::{
    AppBackend, EditorBackend, ProjectBackend, SettingsBackend, StarMapBackend, SyncBackend,
    WorkspaceBackend,
};
pub use linux_theme_controller::LinuxThemeController;

/// Shared reference to AppBackend via Rc<RefCell<>>.
///
/// Domain backends call `with_app` / `with_app_mut` which go through
/// `RefCell::try_borrow` / `RefCell::try_borrow_mut`, returning the default
/// value on borrow conflict instead of causing undefined behaviour.
///
/// # Ownership and lifetime
///
/// `Rc<RefCell<AppBackend>>` is the single ownership root. All domain backends
/// hold a clone of the same `Rc`, so `AppBackend` lives until the last reference
/// is dropped. `Rc` is `!Send + !Sync`, so all access is confined to the GUI
/// thread — the Rust type system enforces this automatically.
///
/// # Destruction order
///
/// `app` MUST be the last field in `BackendRuntime` so that Rust drops all
/// domain backends (which hold `AppRef` clones) before dropping the final
/// `Rc<RefCell<AppBackend>>`. See <https://doc.rust-lang.org/reference/destructors.html>.
#[derive(Clone)]
pub struct AppRef {
    inner: std::rc::Rc<std::cell::RefCell<AppBackend>>,
}

impl AppRef {
    pub fn new(app: std::rc::Rc<std::cell::RefCell<AppBackend>>) -> Self {
        Self { inner: app }
    }

    pub fn rc(&self) -> &std::rc::Rc<std::cell::RefCell<AppBackend>> {
        &self.inner
    }

    pub fn with_app<R>(&self, default: R, f: impl FnOnce(&AppBackend) -> R) -> R {
        match self.inner.try_borrow() {
            Ok(guard) => f(&*guard),
            Err(_) => {
                crate::backend::app_backend::debug_error_static(
                    "app_ref",
                    "BORROW_CONFLICT",
                    "AppBackend already borrowed mutably; returning default",
                );
                default
            }
        }
    }

    pub fn with_app_mut<R>(&self, default: R, f: impl FnOnce(&mut AppBackend) -> R) -> R {
        match self.inner.try_borrow_mut() {
            Ok(mut guard) => f(&mut *guard),
            Err(_) => {
                crate::backend::app_backend::debug_error_static(
                    "app_ref",
                    "BORROW_CONFLICT",
                    "AppBackend already borrowed; returning default",
                );
                default
            }
        }
    }
}

impl Default for AppRef {
    fn default() -> Self {
        Self {
            inner: std::rc::Rc::new(std::cell::RefCell::new(AppBackend::default())),
        }
    }
}

/// Owns every QObject registered into the QML root context.
///
/// Qt only receives raw QObject pointers from `set_object_property`; this
/// runtime keeps the Rust QObjectBox owners alive until after `engine.exec()`.
///
/// **Field order matters**: Rust destroys fields in declaration order.
/// `app` MUST be the last field so that all domain backends
/// (which hold AppRef clones pointing into app) are dropped first.
/// See <https://doc.rust-lang.org/reference/destructors.html>.
pub struct BackendRuntime {
    workspace_backend: QObjectBox<WorkspaceBackend>,
    project_backend: QObjectBox<ProjectBackend>,
    editor_backend: QObjectBox<EditorBackend>,
    settings_backend: QObjectBox<SettingsBackend>,
    sync_backend: QObjectBox<SyncBackend>,
    starmap_backend: QObjectBox<StarMapBackend>,
    theme_controller: QObjectBox<LinuxThemeController>,
    app: std::rc::Rc<std::cell::RefCell<AppBackend>>,
}

impl BackendRuntime {
    pub fn new() -> Self {
        let app = std::rc::Rc::new(std::cell::RefCell::new(AppBackend::default()));
        {
            let mut r = app.borrow_mut();
            r.current_setting_diagnostics_enabled = true;
            r.current_setting_diagnostics_verbose = true;
        }
        let app_ref = AppRef::new(app.clone());

        Self {
            workspace_backend: QObjectBox::new(WorkspaceBackend::new(app_ref.clone())),
            project_backend: QObjectBox::new(ProjectBackend::new(app_ref.clone())),
            editor_backend: QObjectBox::new(EditorBackend::new(app_ref.clone())),
            settings_backend: QObjectBox::new(SettingsBackend::new(app_ref.clone())),
            sync_backend: QObjectBox::new(SyncBackend::new(app_ref.clone())),
            starmap_backend: QObjectBox::new(StarMapBackend::new(app_ref.clone())),
            theme_controller: QObjectBox::new(LinuxThemeController::new(app_ref)),
            app,
        }
    }

    pub fn register_context_properties(&self, engine: &mut QmlEngine) {
        // SAFETY: Rc<RefCell<AppBackend>> heap-allocates and pins the RefCell;
        // Rc is !Send + !Sync, confining access to the GUI thread.
        // QObjectPinned::new borrows the RefCell only for set_object_property.
        let pinned: qmetaobject::QObjectPinned<AppBackend> =
            unsafe { qmetaobject::QObjectPinned::new(&self.app) };
        engine.set_object_property("backend".into(), pinned);
        engine.set_object_property("appBackend".into(), pinned);
        engine.set_object_property("workspaceBackend".into(), self.workspace_backend.pinned());
        engine.set_object_property("projectBackend".into(), self.project_backend.pinned());
        engine.set_object_property("editorBackend".into(), self.editor_backend.pinned());
        engine.set_object_property("settingsBackend".into(), self.settings_backend.pinned());
        engine.set_object_property("syncBackend".into(), self.sync_backend.pinned());
        engine.set_object_property("starmapBackend".into(), self.starmap_backend.pinned());
        engine.set_object_property("themeController".into(), self.theme_controller.pinned());
    }
}
