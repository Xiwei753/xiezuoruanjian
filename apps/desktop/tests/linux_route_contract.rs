use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("apps/desktop should live two levels below repo root")
        .to_path_buf()
}

#[test]
fn windows_qt_workflow_is_not_a_default_push_path() {
    let workflow = fs::read_to_string(repo_root().join(".github/workflows/windows_build.yml"))
        .expect("windows workflow should be readable");

    assert!(
        workflow.contains("workflow_dispatch:"),
        "legacy Windows Qt workflow may remain manually dispatchable"
    );
    assert!(
        !workflow.contains("\n  push:"),
        "apps/desktop Linux work must not trigger Windows Qt build/test/package on push"
    );
    assert!(
        workflow.contains("legacy manual"),
        "workflow name/comment should mark Windows Qt route as legacy/manual"
    );
}

#[test]
fn linux_start_scripts_are_explicitly_linux_scoped() {
    for script_name in ["start.sh", "start-debug.sh"] {
        let script = fs::read_to_string(repo_root().join(script_name))
            .unwrap_or_else(|err| panic!("{script_name} should be readable: {err}"));
        assert!(
            script.contains("uname -s") && script.contains("Linux) ;;"),
            "{script_name} must guard the Linux Qt/QML runtime profile"
        );
        assert!(
            script.contains("linux-debug") && script.contains("linux-appimage"),
            "{script_name} must keep Linux debug/AppImage runtime profiles visible"
        );
        assert!(
            !script.contains("windows-debug") && !script.contains("windows-packaged"),
            "{script_name} must not advertise Windows Qt runtime profiles"
        );
    }
}
