use std::fs;

#[test]
fn main_qml_uses_domain_backend_for_startup_workspace_bool() {
    let qml = fs::read_to_string("qml/main.qml").expect("read main.qml");

    assert!(
        qml.contains("readonly property bool rootHasWorkspace: workspaceBackend.has_workspace === true"),
        "main.qml must derive startup workspace visibility from WorkspaceBackend with a definite bool"
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
    let load_index = main_rs.find("engine.load_file(qml_path.into())").expect("QML load call exists");

    for name in [
        "appBackend",
        "workspaceBackend",
        "projectBackend",
        "editorBackend",
        "settingsBackend",
        "syncBackend",
        "starmapBackend",
    ] {
        let registration = format!("engine.set_object_property(\"{name}\".into()");
        let index = main_rs.find(&registration).unwrap_or_else(|| panic!("missing context property registration for {name}"));
        assert!(
            index < load_index,
            "context property {name} must be registered before main.qml is loaded"
        );
    }
}

#[test]
fn workspace_backend_exposes_has_workspace_bool_property() {
    let workspace_backend = fs::read_to_string("src/backend/workspace_backend.rs").expect("read workspace backend");

    assert!(
        workspace_backend.contains("has_workspace: qt_property!(bool; READ has_workspace NOTIFY workspace_state_changed)"),
        "WorkspaceBackend must expose has_workspace as a real bool qt_property"
    );
    assert!(
        workspace_backend.contains("fn has_workspace(&self) -> bool"),
        "WorkspaceBackend has_workspace reader must return bool"
    );
}
