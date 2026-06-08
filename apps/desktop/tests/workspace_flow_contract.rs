use std::fs;
use tempfile::tempdir;

#[test]
fn workspace_backend_exposes_path_based_methods() {
    let workspace_backend =
        fs::read_to_string("src/backend/workspace_backend.rs").expect("read workspace backend");

    assert!(
        workspace_backend.contains(
            "create_workspace_with_path: qt_method!(fn(&mut self, path: QString) -> QJsonObject)"
        ),
        "WorkspaceBackend must expose create_workspace_with_path"
    );
    assert!(
        workspace_backend.contains(
            "open_workspace_with_path: qt_method!(fn(&mut self, path: QString) -> QJsonObject)"
        ),
        "WorkspaceBackend must expose open_workspace_with_path"
    );
}

#[test]
fn empty_workspace_qml_uses_folder_dialog() {
    let empty_workspace_qml =
        fs::read_to_string("qml/EmptyWorkspace.qml").expect("read EmptyWorkspace.qml");

    assert!(
        empty_workspace_qml.contains("FolderDialog {"),
        "EmptyWorkspace.qml must use FolderDialog for directory selection"
    );
    assert!(
        empty_workspace_qml.contains("signal createWorkspaceWithPath(string path)"),
        "EmptyWorkspace.qml must emit createWorkspaceWithPath signal"
    );
    assert!(
        empty_workspace_qml.contains("signal openWorkspaceWithPath(string path)"),
        "EmptyWorkspace.qml must emit openWorkspaceWithPath signal"
    );
}

#[test]
fn main_qml_connects_to_new_signals() {
    let main_qml = fs::read_to_string("qml/main.qml").expect("read main.qml");

    assert!(
        main_qml.contains("onCreateWorkspaceWithPath:"),
        "main.qml must connect to onCreateWorkspaceWithPath"
    );
    assert!(
        main_qml.contains("onOpenWorkspaceWithPath:"),
        "main.qml must connect to onOpenWorkspaceWithPath"
    );
    assert!(
        main_qml.contains("onInitFromGithub:"),
        "main.qml must handle onInitFromGithub"
    );
    assert!(
        main_qml.contains("openSyncDialog()"),
        "main.qml must open SyncDialog on init from github"
    );
}
