use std::fs;

#[test]
fn main_qml_uses_domain_backend_for_startup_workspace_bool() {
    let qml = fs::read_to_string("qml/main.qml").expect("read main.qml");

    assert!(
        qml.contains("readonly property bool rootHasWorkspace: workspaceBackend !== null && workspaceBackend.has_workspace === true"),
        "main.qml must derive startup workspace visibility from WorkspaceBackend with a definite bool and a diagnostic-safe null guard"
    );

    for old_binding in [
        "active: appState.hasWorkspace",
        "visible: appState.hasWorkspace",
        "visible: !appState.hasWorkspace",
    ] {
        assert!(
            !qml.contains(old_binding),
            "main.qml startup bool binding still uses mutable appState directly: {old_binding}"
        );
    }

    for old_domain_access in [
        "appBackend.setting_",
        "appBackend.sync_",
        "appBackend.ai_",
        "backend.setting_",
        "backend.sync_",
    ] {
        assert!(
            !qml.contains(old_domain_access),
            "main.qml must use split domain backends instead of old AppBackend access: {old_domain_access}"
        );
    }
}

#[test]
fn split_backend_context_properties_are_registered_before_qml_load() {
    let main_rs = fs::read_to_string("src/main.rs").expect("read src/main.rs");
    let backend_mod = fs::read_to_string("src/backend/mod.rs").expect("read src/backend/mod.rs");
    let load_index = main_rs
        .find("engine.load_file(qml_path.into())")
        .expect("QML load call exists");
    let runtime_index = main_rs
        .find("let backend_runtime = BackendRuntime::new()")
        .expect("BackendRuntime is created in main");
    let register_index = main_rs
        .find("backend_runtime.register_context_properties(&mut engine)")
        .expect("BackendRuntime registers context properties");

    assert!(
        runtime_index < register_index && register_index < load_index,
        "BackendRuntime must be created and registered before main.qml is loaded"
    );

    assert!(
        backend_mod.contains("pub struct BackendRuntime"),
        "backend module must define a long-lived BackendRuntime owner"
    );
    let register_fn_index = backend_mod
        .find("pub fn register_context_properties(&self, engine: &mut QmlEngine)")
        .expect("BackendRuntime has register_context_properties");

    for name in [
        "backend",
        "appBackend",
        "workspaceBackend",
        "projectBackend",
        "editorBackend",
        "settingsBackend",
        "syncBackend",
        "starmapBackend",
    ] {
        let registration = format!("engine.set_object_property(\"{name}\".into()");
        let index = backend_mod
            .find(&registration)
            .unwrap_or_else(|| panic!("missing context property registration for {name}"));
        assert!(
            register_fn_index < index,
            "context property {name} must be registered by BackendRuntime"
        );
    }
}

#[test]
fn backend_runtime_owns_all_qml_qobjects_until_after_event_loop() {
    let main_rs = fs::read_to_string("src/main.rs").expect("read src/main.rs");
    let backend_mod = fs::read_to_string("src/backend/mod.rs").expect("read src/backend/mod.rs");

    for field in [
        "app_backend: QObjectBox<AppBackend>",
        "workspace_backend: QObjectBox<WorkspaceBackend>",
        "project_backend: QObjectBox<ProjectBackend>",
        "editor_backend: QObjectBox<EditorBackend>",
        "settings_backend: QObjectBox<SettingsBackend>",
        "sync_backend: QObjectBox<SyncBackend>",
        "starmap_backend: QObjectBox<StarMapBackend>",
    ] {
        assert!(
            backend_mod.contains(field),
            "BackendRuntime must own registered QObjectBox field: {field}"
        );
    }

    let exec_index = main_rs
        .find("engine.exec()")
        .expect("event loop call exists");
    let runtime_index = main_rs
        .find("let backend_runtime = BackendRuntime::new()")
        .expect("BackendRuntime is created");
    assert!(
        runtime_index < exec_index,
        "BackendRuntime local must be created before engine.exec so it drops only after the event loop returns"
    );

    for forbidden in [
        "let workspace_backend = QObjectBox::new",
        "let project_backend = QObjectBox::new",
        "let editor_backend = QObjectBox::new",
        "let settings_backend = QObjectBox::new",
        "let sync_backend = QObjectBox::new",
        "let starmap_backend = QObjectBox::new",
    ] {
        assert!(
            !main_rs.contains(forbidden),
            "main.rs must not register function-local backend QObjectBox owners: {forbidden}"
        );
    }
}

#[test]
fn main_qml_has_backend_runtime_startup_diagnostics() {
    let qml = fs::read_to_string("qml/main.qml").expect("read main.qml");

    assert!(
        qml.contains("function verifyBackendRuntime()"),
        "main.qml must diagnose null context properties at startup"
    );

    for name in ["workspaceBackend", "settingsBackend", "syncBackend"] {
        let check = format!("if ({name} === null) reportNullBackend(\"{name}\")");
        assert!(qml.contains(&check), "main.qml must report null {name}");
    }
}

#[test]
fn embedded_qrc_includes_editor_qml_components() {
    let main_rs = fs::read_to_string("src/main.rs").expect("read src/main.rs");

    for component in ["EditorWheelScroller.qml"] {
        let entry = format!("\"qml/{component}\" as \"{component}\"");
        assert!(
            main_rs.contains(&entry),
            "embedded qrc must include {component} so AppImage can load qrc:/main.qml"
        );
    }
}

#[test]
fn workspace_backend_exposes_has_workspace_bool_property() {
    let workspace_backend =
        fs::read_to_string("src/backend/workspace_backend.rs").expect("read workspace backend");

    assert!(
        workspace_backend.contains(
            "has_workspace: qt_property!(bool; READ has_workspace NOTIFY workspace_state_changed)"
        ),
        "WorkspaceBackend must expose has_workspace as a real bool qt_property"
    );
    assert!(
        workspace_backend.contains("fn has_workspace(&self) -> bool"),
        "WorkspaceBackend has_workspace reader must return bool"
    );
}
