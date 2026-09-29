//! Issue #801 评论 5900350140 — root starmap + 路径段的当前层解析/加载守卫。
//!
//! WHITE_BOX + 真实运行验证评论 5900350140 的要求：
//! 1. Linux_Qt backend 暴露路径解析入口（root starmapId + 路径段 -> finalStarmapId）；
//! 2. 当前层加载使用解析结果，`currentStarmapId` 不再由点击事件的裸 target id 直接决定；
//! 3. 路径解析失败直接报错，不再回退到点击事件传来的裸 targetStarmapId。

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "common/source_guard.rs"]
mod source_guard;

use source_guard::{function_window, read_src};
use writer_core::api::WriterCoreApi;

const CANVAS: &str = "qml/StarMapCanvas.qml";
const WORKSPACE: &str = "qml/StarMapWorkspace.qml";
const BRIDGE: &str = "src/starmap_bridge.rs";
const QT_BACKEND: &str = "src/backend/starmap_backend.rs";
const BACKEND_GRAPH: &str = "src/backend/starmap_backend/graph.rs";
const CORE_FACADE: &str = "../../core/writer_core/src/facade/starmap_ops.rs";
const CORE_API: &str = "../../core/writer_core/src/api/service/starmap_ops.rs";

/// 去掉整行 `//` 注释，只留可执行语句。
fn strip_line_comments(source: &str) -> String {
    source
        .lines()
        .map(|line| match line.find("//") {
            Some(idx) => &line[..idx],
            None => line,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// 折叠所有空白（含换行）为单空格，避免受 rustfmt 折行影响。
fn flatten_whitespace(source: &str) -> String {
    source.split_whitespace().collect::<Vec<_>>().join(" ")
}

// ---------------------------------------------------------------------------
// 真实运行：backend 解析入口按 root + 路径段逐段穿越
// ---------------------------------------------------------------------------

/// 调 Linux_Qt backend 桥接入口，返回 Ok(finalStarmapId) / Err(errorCode)。
fn resolve_path(api: &WriterCoreApi, root: &str, segments_json: &str) -> Result<String, String> {
    let raw = sujian_linux_qt::starmap_bridge::resolve_starmap_path(api, root, segments_json);
    let value: serde_json::Value = serde_json::from_str(&raw).unwrap();
    if value.get("success").and_then(|s| s.as_bool()) == Some(true) {
        Ok(value["data"]["finalStarmapId"]
            .as_str()
            .unwrap_or_default()
            .to_string())
    } else {
        Err(value
            .get("errorCode")
            .and_then(|c| c.as_str())
            .unwrap_or_default()
            .to_string())
    }
}

/// 建一张星图并返回 id。
fn create_starmap(api: &WriterCoreApi, title: &str) -> String {
    api.create_starmap(title, "", None).unwrap().starmap_id
}

/// 在 host 里嵌入 target，返回 instanceId。
fn embed(api: &WriterCoreApi, host: &str, target: &str, label: &str) -> String {
    let raw =
        sujian_linux_qt::starmap_bridge::create_starmap_embed(api, host, target, label, 0.0, 0.0);
    let value: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(
        value.get("success").and_then(|s| s.as_bool()),
        Some(true),
        "create_starmap_embed 必须成功，实际: {raw}"
    );
    value["data"]["instanceId"].as_str().unwrap().to_string()
}

/// 1. root + [] 落在 root；root + 逐段 enterEmbed 落在各层子图。
#[test]
fn backend_resolve_starmap_path_walks_root_plus_segments() {
    let dir = tempfile::tempdir().unwrap();
    let projects_root = dir.path().join("projects");
    std::fs::create_dir_all(&projects_root).unwrap();
    let api = WriterCoreApi::new(dir.path(), &projects_root);

    let root = create_starmap(&api, "根图");
    let child = create_starmap(&api, "子图");
    let grandchild = create_starmap(&api, "孙图");
    let root_child = embed(&api, &root, &child, "子图入口");
    let child_grand = embed(&api, &child, &grandchild, "孙图入口");

    // 空路径 = 根层本身
    assert_eq!(resolve_path(&api, &root, "[]").unwrap(), root);

    // 一层 EnterEmbed
    let one_segment = format!(r#"[{{"type":"enterEmbed","instanceId":"{root_child}"}}]"#);
    assert_eq!(resolve_path(&api, &root, &one_segment).unwrap(), child);

    // 两层 EnterEmbed：必须是逐段穿越后的孙图，而不是任意裸 id
    let two_segments = format!(
        r#"[{{"type":"enterEmbed","instanceId":"{root_child}"}},{{"type":"enterEmbed","instanceId":"{child_grand}"}}]"#
    );
    assert_eq!(
        resolve_path(&api, &root, &two_segments).unwrap(),
        grandchild
    );
}

/// 2. 路径失效（Embed 被删/不存在）时解析失败，不得静默回退到裸目标 id。
#[test]
fn backend_resolve_starmap_path_rejects_broken_path() {
    let dir = tempfile::tempdir().unwrap();
    let projects_root = dir.path().join("projects");
    std::fs::create_dir_all(&projects_root).unwrap();
    let api = WriterCoreApi::new(dir.path(), &projects_root);

    let root = create_starmap(&api, "根图");
    let child = create_starmap(&api, "子图");
    let root_child = embed(&api, &root, &child, "子图入口");

    // 被删除的 Embed：路径失效必须报错
    let broken = format!(r#"[{{"type":"enterEmbed","instanceId":"{root_child}"}}]"#);
    assert_eq!(resolve_path(&api, &root, &broken).unwrap(), child);
    api.delete_starmap_embed(&root, &root_child).unwrap();
    assert!(
        resolve_path(&api, &root, &broken).is_err(),
        "被删除的 Embed 段必须解析失败，不能回退到裸 target id"
    );

    // 不存在的 instanceId 同样失败
    assert!(resolve_path(
        &api,
        &root,
        r#"[{"type":"enterEmbed","instanceId":"em_missing"}]"#
    )
    .is_err());
}

// ---------------------------------------------------------------------------
// 接线守卫：QML / backend / Core 都必须走这条解析入口
// ---------------------------------------------------------------------------

/// 3. Workspace 声明 rootStarmapId 并通过 backend 解析 root + currentPathSegments。
#[test]
fn workspace_resolves_current_layer_from_root_plus_segments() {
    let src = strip_line_comments(&read_src(WORKSPACE));
    assert!(
        src.contains("property string rootStarmapId: \"\""),
        "Workspace 必须声明 rootStarmapId（层级身份的起点），实际源码缺少"
    );
    let func = function_window(&src, "function resolveStarmapPath(", 800);
    assert!(
        func.contains(
            "starmapBackendRef.resolve_starmap_path(rootStarmapId, JSON.stringify(segments))"
        ),
        "当前层解析必须调 backend resolve_starmap_path(rootStarmapId, segments)，实际窗口:\n{func}"
    );
    assert!(
        func.contains("res.data && res.data.finalStarmapId"),
        "解析结果必须读 finalStarmapId，实际窗口:\n{func}"
    );
    assert!(
        func.contains("return \"\""),
        "解析失败必须返回空 id 让调用方中止，实际窗口:\n{func}"
    );
}

/// 4. currentStarmapId 只写解析结果，不再由点击事件的裸 targetId 直接决定。
#[test]
fn current_starmap_id_only_comes_from_resolution() {
    let src = strip_line_comments(&read_src(WORKSPACE));

    let enter = function_window(&src, "function enterChildStarmap(", 900);
    assert!(
        enter.contains("resolveStarmapPath(nextSegments)"),
        "enterChildStarmap 必须先解析追加后的路径，实际窗口:\n{enter}"
    );
    assert!(
        !enter.contains("currentStarmapId = targetId"),
        "enterChildStarmap 不得再把点击事件的裸 targetId 写进 currentStarmapId，实际窗口:\n{enter}"
    );
    assert!(
        enter.contains("currentStarmapId = resolvedId"),
        "enterChildStarmap 必须写解析结果 resolvedId，实际窗口:\n{enter}"
    );
    assert!(
        enter.contains("resolvedId === \"\""),
        "enterChildStarmap 解析失败必须中止，实际窗口:\n{enter}"
    );

    let back = function_window(&src, "function returnToParentStarmap(", 900);
    assert!(
        back.contains("resolveStarmapPath(parentSegments)"),
        "returnToParentStarmap 必须解析父层路径，实际窗口:\n{back}"
    );
    assert!(
        back.contains("currentStarmapId = resolvedId"),
        "returnToParentStarmap 必须写解析结果，实际窗口:\n{back}"
    );
}

/// 5. 根层（外部切图 / 初次进入）也走同一条解析入口。
#[test]
fn root_layer_resolution_wired_on_load() {
    let src = strip_line_comments(&read_src(WORKSPACE));
    let changed = function_window(&src, "onStarmapIdChanged: {", 400);
    assert!(
        changed.contains("rootStarmapId = starmapId"),
        "onStarmapIdChanged 必须把外部 starmapId 记为 rootStarmapId，实际窗口:\n{changed}"
    );
    assert!(
        changed.contains("refreshCurrentStarmap()"),
        "onStarmapIdChanged 必须触发当前层解析，实际窗口:\n{changed}"
    );
    let completed = function_window(&src, "Component.onCompleted: {", 300);
    assert!(
        completed.contains("refreshCurrentStarmap()"),
        "Component.onCompleted 必须触发当前层解析，实际窗口:\n{completed}"
    );
}

/// 6. Canvas 仍然只加载 Workspace 解析出来的 starmapId。
#[test]
fn canvas_loads_resolved_starmap_id() {
    let workspace = strip_line_comments(&read_src(WORKSPACE));
    assert!(
        workspace.contains("starmapId: root.currentStarmapId"),
        "Canvas 必须绑定 Workspace 的 currentStarmapId（解析结果）"
    );

    let canvas = strip_line_comments(&read_src(CANVAS));
    assert!(
        canvas.contains("graphController.loadGraph()"),
        "Canvas.onStarmapIdChanged 仍须通过 graphController.loadGraph 加载当前层"
    );
}

/// 7. Linux_Qt backend 暴露 resolve_starmap_path（Qt 方法 + bridge + AppBackend）。
#[test]
fn linux_backend_exposes_path_resolution_entry() {
    let qt_backend = flatten_whitespace(&strip_line_comments(&read_src(QT_BACKEND)));
    assert!(
        qt_backend.contains(
            "resolve_starmap_path_json: qt_method!(fn(&self, root_starmap_id: QString, segments_json: QString) -> QString)"
        ),
        "StarMapBackend 必须暴露 resolve_starmap_path_json Qt 方法"
    );
    assert!(
        qt_backend.contains(
            "resolve_starmap_path: qt_method!(fn(&self, root_starmap_id: QString, segments_json: QString) -> QJsonObject)"
        ),
        "StarMapBackend 必须暴露 resolve_starmap_path Qt 方法"
    );

    let backend_graph = flatten_whitespace(&strip_line_comments(&read_src(BACKEND_GRAPH)));
    assert!(
        backend_graph.contains("starmap_bridge::resolve_starmap_path(&core, &root, &segs)"),
        "AppBackend::resolve_starmap_path_json 必须转调 starmap_bridge::resolve_starmap_path"
    );

    let bridge = strip_line_comments(&read_src(BRIDGE));
    assert!(
        bridge.contains("api.resolve_starmap_path(root_starmap_id, segments)"),
        "starmap_bridge 必须调 Core 的 WriterCoreApi::resolve_starmap_path"
    );
}

/// 8. Core 侧：facade 用 resolver 逐段解析，API 暴露同名字段 finalStarmapId。
#[test]
fn core_resolver_is_the_layer_truth() {
    let facade = strip_line_comments(&read_src(CORE_FACADE));
    let func = function_window(&facade, "pub fn resolve_starmap_path(", 1400);
    assert!(
        func.contains("resolve::resolve_target(&context, &path)"),
        "facade resolve_starmap_path 必须调 Core resolver（resolve_target），实际窗口:\n{func}"
    );
    assert!(
        func.contains("StarMapTargetDetail::Starmap"),
        "facade 解析目标必须是 Starmap（逐段可达即落到最终星图），实际窗口:\n{func}"
    );

    let api = strip_line_comments(&read_src(CORE_API));
    assert!(
        api.contains("pub fn resolve_starmap_path("),
        "WriterCoreApi 必须暴露 resolve_starmap_path"
    );
}
