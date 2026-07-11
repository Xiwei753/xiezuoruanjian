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
    let windows_workflow = root.join(".github/workflows/windows_build.yml");
    if windows_workflow.exists() {
        let wf = fs::read_to_string(&windows_workflow)
            .expect("windows_build.yml should be readable");
        assert!(
            !wf.contains("push:") || wf.contains("workflow_dispatch:"),
            "windows_build.yml must be workflow_dispatch only, not auto-triggered on push"
        );
        assert!(
            !wf.contains("Qt") && !wf.contains("qmake") && !wf.contains("linuxdeploy"),
            "windows_build.yml must be native WinUI 3 route, not legacy Qt cross-build"
        );
    }
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
        !readme.contains("WriterCoreBridge` stub"),
        "apps/windows README must not claim WriterCoreBridge is a stub"
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
    let sujian_mod = fs::read_to_string(root.join("apps/Linux_qt/src/sujian_editor_item/mod.rs"))
        .expect("SujianEditorItem mod.rs should be readable");
    let sujian_transaction = fs::read_to_string(root.join("apps/Linux_qt/src/sujian_editor_item/transaction.rs"))
        .unwrap_or_default();
    let sujian_coordinator = fs::read_to_string(root.join("apps/Linux_qt/src/sujian_editor_item/animation_coordinator.rs"))
        .unwrap_or_default();
    let sujian_qquickitem = fs::read_to_string(root.join("apps/Linux_qt/src/sujian_editor_item/qquickitem_impl.rs"))
        .unwrap_or_default();
    let sujian_sg_renderer = fs::read_to_string(root.join("apps/Linux_qt/src/sujian_editor_item/scene_graph_renderer.rs"))
        .unwrap_or_default();
    let combined = format!("{sujian_mod}\n{sujian_transaction}\n{sujian_coordinator}\n{sujian_qquickitem}\n{sujian_sg_renderer}");

    // Scene Graph animation layer (child[1]) reads ActiveVisualTransactionQueue
    // every frame in update_paint_node, computes progress, and drives
    // update_animation_layer / clear_animation_layer directly.
    // Completion is atomic: vt_queue.complete(tid, gen) + finish_overlay_plan
    // remove hidden ranges and trigger static text re-render.
    // No QML overlay callback is needed for the normal completion path.
    assert!(
        combined.contains("update_animation_layer"),
        "update_paint_node must call update_animation_layer to render animated glyphs in Scene Graph child[1]"
    );
    assert!(
        combined.contains("clear_animation_layer"),
        "update_paint_node must call clear_animation_layer when no active transactions remain"
    );
    assert!(
        combined.contains("vt_queue.complete"),
        "update_paint_node must complete transactions via vt_queue.complete when progress >= 1.0"
    );
    assert!(
        combined.contains("finish_by_key"),
        "update_paint_node must call finish_by_key to atomically remove hidden ranges and complete transaction when progress >= 1.0"
    );
    assert!(
        combined.contains("mark_rendering"),
        "update_paint_node must mark Prepared transactions as Rendering on first frame"
    );

    // Legacy QML callback methods are still declared for backward compat
    assert!(
        combined.contains("on_insert_animation_finished_by_id: qt_method!(fn(&mut self, transaction_id: QString, range_id: QString, byte_start: i32, byte_end: i32))")
            && combined.contains("on_insert_animation_skipped_by_id: qt_method!(fn(&mut self, transaction_id: QString, range_id: QString, byte_start: i32, byte_end: i32))"),
        "SujianEditorItem qt_method signatures must carry transaction/range ids as strings to avoid u64 truncation"
    );
    assert!(
        combined.contains("start_insert(")
            && combined.contains("alloc_key()")
            && combined.contains("core_range_id"),
        "record_transaction Insert path must start AnimationRangeRegistry with VisualTransactionKey via coordinator"
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
