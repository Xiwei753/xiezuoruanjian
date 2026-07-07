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
    assert!(
        !readme.contains("App.xaml")
            && !readme.contains("MainWindow.xaml")
            && !readme.contains("Win2D")
            && !readme.contains("WriterCoreBridge` stub"),
        "apps/windows README must not claim unsubmitted Windows app shell/editor/bridge files are complete"
    );
    let obsolete_path_tokens = [
        ["apps/windows", "-desktop"].concat(),
        ["apps/linux", "-desktop"].concat(),
        ["docs/windows", "_native_route.md"].concat(),
        ["docs/desktop", "_split_contract.md"].concat(),
    ];
    for obsolete in obsolete_path_tokens {
        assert!(
            !readme.contains(&obsolete),
            "issue #433 local route wording must not keep obsolete path token: {obsolete}"
        );
    }
}

#[test]
fn linux_insert_animation_id_route_is_wired_end_to_end() {
    let root = repo_root();
    let writing_workspace = fs::read_to_string(root.join("apps/Linux_qt/qml/WritingWorkspace.qml"))
        .expect("WritingWorkspace.qml should be readable");
    assert!(
        writing_workspace.contains(
            "onInsertAnimationFinished: function(transactionId, rangeId, byteStart, byteEnd)"
        ) && writing_workspace.contains(
            "sujianEditor.on_insert_animation_finished_by_id(transactionId, rangeId, byteStart, byteEnd)"
        ),
        "WritingWorkspace.qml must forward insertAnimationFinished transactionId/rangeId/byteStart/byteEnd to SujianEditorItem"
    );
    assert!(
        writing_workspace.contains(
            "onInsertAnimationSkipped: function(transactionId, rangeId, byteStart, byteEnd)"
        ) && writing_workspace.contains(
            "sujianEditor.on_insert_animation_skipped_by_id(transactionId, rangeId, byteStart, byteEnd)"
        ),
        "WritingWorkspace.qml must forward insertAnimationSkipped transactionId/rangeId/byteStart/byteEnd to SujianEditorItem"
    );

    let sujian_item = fs::read_to_string(root.join("apps/Linux_qt/src/sujian_editor_item/mod.rs"))
        .expect("SujianEditorItem source should be readable");
    assert!(
        sujian_item.contains(
            "on_insert_animation_finished_by_id: qt_method!(fn(&mut self, transaction_id: i32, range_id: i32, byte_start: i32, byte_end: i32))"
        ) && sujian_item.contains(
            "on_insert_animation_skipped_by_id: qt_method!(fn(&mut self, transaction_id: i32, range_id: i32, byte_start: i32, byte_end: i32))"
        ),
        "SujianEditorItem qt_method signatures must keep four i32 parameters for transaction/range aware cleanup"
    );
    assert!(
        sujian_item.contains("start_insert_with_ids(")
            && sujian_item.contains("Some(vt.id)")
            && sujian_item.contains("hidden_range_id"),
        "record_transaction Insert path must start TextAnimationState with transactionId/rangeId"
    );
    assert!(
        !sujian_item.contains("text_anim_state.start_insert(\n"),
        "record_transaction Insert path must not regress to start_insert byte-range-only cleanup"
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
