use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("apps/Linux_qt should live two levels below repo root")
        .to_path_buf()
}

#[test]
fn windows_route_is_native_winui_app_sdk() {
    let root = repo_root();
    assert!(
        !root.join(".github/workflows/windows_build.yml").exists(),
        "legacy Windows Qt build/package workflow must not remain after Linux_qt split"
    );
    assert!(
        !root.join("packaging/windows").exists(),
        "legacy Windows Qt installer files must not remain after Linux_qt split"
    );
    let readme = fs::read_to_string(root.join("apps/windows/README.md"))
        .expect("apps/windows README should describe the native Windows route");
    for required in [
        "WinUI 3",
        "Windows App SDK",
        "SujianEditor",
        "DirectWrite",
        "Direct2D",
        "Windows IME",
        "writer_core",
    ] {
        assert!(
            readme.contains(required),
            "apps/windows README must mention required native route token: {required}"
        );
    }
    assert!(
        readme.contains("先做自研") || readme.contains("先验证自研"),
        "issue #433 execution order must put SujianEditor MVP before full pages"
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
