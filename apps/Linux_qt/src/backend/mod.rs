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

/// Shared pointer to the heap-allocated AppBackend.
///
/// # Safety invariants (must hold for the entire lifetime of BackendRuntime)
///
/// 1. **Pinned heap allocation**: The AppBackend lives inside a QObjectBox
///    which heap-allocates and pins the object. The raw pointer obtained via
///    `set()` remains valid as long as the QObjectBox owner is alive.
///
/// 2. **Destruction order**: `app_backend` MUST be the last field in
///    BackendRuntime so that Rust drops all domain backends (which hold
///    SafeAppPtr clones) before dropping the QObjectBox<AppBackend>.
///    If app_backend were destroyed first, the dangling pointer would be
///    unsound to dereference during child backend destructors.
///
/// 3. **Single-threaded access**: Rc<Cell<*mut>> is intentionally !Send and
///    !Sync. All QObject backends live on the GUI thread. The Rust type
///    system prevents this pointer from crossing thread boundaries.
///
/// 4. **No concurrent &mut**: Each domain backend's `with_app_mut` takes
///    `&mut self`, which prevents two backends from simultaneously holding
///    `&mut AppBackend`. However, this is a *convention* guarantee, not
///    enforced by the type system — a future refactor should replace this
///    with a proper single-owner command dispatcher.
///
/// **Known limitation**: `&mut *app` from a raw pointer bypasses Rust's
/// alias checker. If two backends call `with_app_mut` through different
/// code paths that the borrow checker cannot see (e.g., Qt signal
/// re-entry), aliasing rules may be violated. This is a known debt that
/// should be resolved by moving to a command-dispatch architecture.
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
///
/// **Field order matters**: Rust destroys fields in declaration order.
/// `app_backend` MUST be the last field so that all domain backends
/// (which hold SafeAppPtr pointing into app_backend) are dropped first.
/// See <https://doc.rust-lang.org/reference/destructors.html>.
pub struct BackendRuntime {
    workspace_backend: QObjectBox<WorkspaceBackend>,
    project_backend: QObjectBox<ProjectBackend>,
    editor_backend: QObjectBox<EditorBackend>,
    settings_backend: QObjectBox<SettingsBackend>,
    sync_backend: QObjectBox<SyncBackend>,
    starmap_backend: QObjectBox<StarMapBackend>,
    theme_controller: QObjectBox<LinuxThemeController>,
    app_backend: QObjectBox<AppBackend>,
}

impl BackendRuntime {
    pub fn new() -> Self {
        let app_backend = QObjectBox::new(AppBackend::default());
        {
            let pinned = app_backend.pinned();
            let mut r = pinned.borrow_mut();
            r.current_setting_diagnostics_enabled = true;
            r.current_setting_diagnostics_verbose = true;
        }
        let app_ptr = SafeAppPtr::new();
        // SAFETY: See SafeAppPtr documentation for the full list of invariants.
        // Key points for this cast:
        // - QObjectBox heap-allocates and pins AppBackend; the address is stable.
        // - The pointer is stored in Rc<Cell<>> which is !Send/!Sync, preventing
        //   cross-thread propagation.
        // - app_backend is the last field in BackendRuntime, so it outlives all
        //   domain backends that dereference this pointer.
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
            starmap_backend: QObjectBox::new(StarMapBackend::new(app_ptr.clone())),
            theme_controller: QObjectBox::new(LinuxThemeController::new(app_ptr)),
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
        engine.set_object_property("themeController".into(), self.theme_controller.pinned());
    }
}
