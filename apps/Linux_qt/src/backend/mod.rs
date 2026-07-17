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

/// Shared reference to the RefCell inside QObjectBox<AppBackend>.
///
/// Unlike the previous raw-pointer approach, this uses RefCell's runtime
/// borrow checking to prevent aliasing violations. Domain backends call
/// `with_app` / `with_app_mut` which go through `RefCell::try_borrow` /
/// `RefCell::try_borrow_mut`, returning the default value on borrow conflict
/// instead of causing undefined behaviour.
///
/// # Safety invariants (must hold for the entire lifetime of BackendRuntime)
///
/// 1. **Pinned heap allocation**: The RefCell lives inside a QObjectBox
///    which heap-allocates and pins the object. The raw pointer obtained via
///    `set_from_pinned()` remains valid as long as the QObjectBox owner is alive.
///
/// 2. **Destruction order**: `app_backend` MUST be the last field in
///    BackendRuntime so that Rust drops all domain backends (which hold
///    SafeAppPtr clones) before dropping the QObjectBox<AppBackend>.
///    If app_backend were destroyed first, the dangling pointer would be
///    unsound to dereference during child backend destructors.
///
/// 3. **Single-threaded access**: Rc<Cell<*const>> is intentionally !Send and
///    !Sync. All QObject backends live on the GUI thread. The Rust type
///    system prevents this pointer from crossing thread boundaries.
///
/// 4. **Runtime borrow checking**: `with_app` uses `RefCell::try_borrow` and
///    `with_app_mut` uses `RefCell::try_borrow_mut`. If Qt signal re-entry
///    causes a nested borrow, the inner call returns the default value instead
///    of producing undefined behaviour. This is a strict improvement over the
///    previous `&mut *app` raw-pointer approach which silently violated
///    Rust's aliasing rules.
#[derive(Clone)]
pub struct SafeAppPtr {
    cell: std::rc::Rc<std::cell::Cell<*const std::cell::RefCell<AppBackend>>>,
}

impl SafeAppPtr {
    pub fn new() -> Self {
        Self {
            cell: std::rc::Rc::new(std::cell::Cell::new(std::ptr::null())),
        }
    }

    pub fn set_from_pinned(&self, pinned: qmetaobject::QObjectPinned<AppBackend>) {
        // SAFETY: QObjectPinned is #[repr(transparent)] over &RefCell<T>;
        // transmuting to *const RefCell<T> preserves the address.
        // The caller must ensure the QObjectBox outlives all SafeAppPtr clones.
        // as *mut AppBackend pattern: this transmute extracts the heap address
        // from QObjectPinned; no actual mutability is gained — the pointer
        // is stored as *const and only dereferenced through RefCell guards.
        let cell_ptr: *const std::cell::RefCell<AppBackend> =
            // SAFETY: see comment block above; transmute extracts &RefCell address from repr(transparent) QObjectPinned.
            unsafe { std::mem::transmute(pinned) };
        self.cell.set(cell_ptr);
    }

    pub fn with_app<R>(&self, default: R, f: impl FnOnce(&AppBackend) -> R) -> R {
        let cell_ptr = self.cell.get();
        if cell_ptr.is_null() {
            crate::backend::app_backend::debug_error_static(
                "safe_app_ptr",
                "BACKEND_LINK_BROKEN",
                "app RefCell pointer is null",
            );
            return default;
        }
        // SAFETY: cell_ptr from QObjectPinned(&RefCell<AppBackend>); QObjectBox pins on heap;
        // Rc<Cell<>> is !Send/!Sync; app_backend is last field so outlives domain backends;
        // RefCell::try_borrow provides runtime borrow checking; no aliasing violation.
        let cell_ref: &std::cell::RefCell<AppBackend> = unsafe { &*cell_ptr };
        match cell_ref.try_borrow() {
            Ok(guard) => f(&*guard),
            Err(_) => {
                crate::backend::app_backend::debug_error_static(
                    "safe_app_ptr",
                    "BORROW_CONFLICT",
                    "AppBackend already borrowed mutably; returning default",
                );
                default
            }
        }
    }

    pub fn with_app_mut<R>(&mut self, default: R, f: impl FnOnce(&mut AppBackend) -> R) -> R {
        let cell_ptr = self.cell.get();
        if cell_ptr.is_null() {
            crate::backend::app_backend::debug_error_static(
                "safe_app_ptr",
                "BACKEND_LINK_BROKEN",
                "app RefCell pointer is null",
            );
            return default;
        }
        // SAFETY: same as with_app; &mut self prevents concurrent with_app_mut;
        // RefCell::try_borrow_mut catches Qt signal re-entry; no aliasing violation.
        let cell_ref: &std::cell::RefCell<AppBackend> = unsafe { &*cell_ptr };
        match cell_ref.try_borrow_mut() {
            Ok(mut guard) => f(&mut *guard),
            Err(_) => {
                crate::backend::app_backend::debug_error_static(
                    "safe_app_ptr",
                    "BORROW_CONFLICT",
                    "AppBackend already borrowed; returning default",
                );
                default
            }
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
        app_ptr.set_from_pinned(app_backend.pinned());

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
