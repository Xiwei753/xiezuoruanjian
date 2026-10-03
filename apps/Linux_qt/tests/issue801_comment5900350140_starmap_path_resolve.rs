//! Issue #801 评论 5900350140 — root starmap + 路径段的当前层解析/加载守卫。
//!
//! WHITE_BOX + 真实运行验证评论 5900350140 的要求：
//! 1. Linux_Qt backend 暴露路径解析入口（root starmapId + 路径段 -> finalStarmapId）；
//! 2. 当前层加载使用解析结果，`currentStarmapId` 不再由点击事件的裸 target id 直接决定；
//! 3. 路径解析失败直接报错，不再回退到点击事件传来的裸 targetStarmapId。
//!
//! #822 更新：`StarMapScene.qml` 已删除，递归层改成 `StarMapSceneContent.qml`，
//! 根层由 `StarMapCanvas.qml` 直接创建。因此"每层自己解析 root + 自己的路径段、
//! 只用解析结果加载本层"这条契约现在落在 Content 的 `resolvePath()` 上，
//! Workspace 不再持有 `currentStarmapId` / 下钻 / 返回栈。

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "common/source_guard.rs"]
mod source_guard;

use source_guard::{function_window, read_src};
use writer_core::api::WriterCoreApi;

const CANVAS: &str = "qml/StarMapCanvas.qml";
const CONTENT: &str = "qml/StarMapSceneContent.qml";
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

/// 3. Issue #822：每层 Content 持有自己的 rootStarmapId + pathSegments，
/// 通过 backend 解析出本层 finalStarmapId；解析失败清空 finalStarmapId 并报错，
/// 不回退到任何裸 targetStarmapId。
#[test]
fn content_resolves_its_own_layer_from_root_plus_segments() {
    let src = strip_line_comments(&read_src(CONTENT));
    assert!(
        src.contains("required property string rootStarmapId")
            && src.contains("required property var pathSegments"),
        "Content 必须声明 rootStarmapId 与 pathSegments（层级身份的起点），实际源码缺少"
    );
    let func = function_window(&src, "function resolvePath(", 2200);
    assert!(
        func.contains(
            "starmapBackendRef.resolve_starmap_path(rootStarmapId, JSON.stringify(pathSegments))"
        ),
        "本层解析必须调 backend resolve_starmap_path(rootStarmapId, pathSegments)，实际窗口:\n{func}"
    );
    assert!(
        func.contains("res.data && res.data.finalStarmapId"),
        "解析结果必须读 finalStarmapId，实际窗口:\n{func}"
    );
    assert!(
        func.contains("finalStarmapId = \"\""),
        "解析失败必须清空 finalStarmapId 让本层停止加载，实际窗口:\n{func}"
    );
    assert!(
        !func.contains("targetStarmapId"),
        "本层解析不得回退到点击事件传来的裸 targetStarmapId，实际窗口:\n{func}"
    );
}

/// 4. 本层 GraphController 的 starmapId 只绑解析结果 `finalStarmapId`，
/// 不绑点击事件或递归容器带来的任何裸 targetStarmapId。
#[test]
fn layer_starmap_id_only_comes_from_resolution() {
    let src = strip_line_comments(&read_src(CONTENT));
    let controller = function_window(&src, "id: graphController", 600);
    assert!(
        controller.contains("starmapId: content.finalStarmapId"),
        "本层 GraphController 必须只绑解析结果 finalStarmapId，实际窗口:\n{controller}"
    );
    assert!(
        !controller.contains("targetStarmapId"),
        "本层 GraphController 不得绑裸 targetStarmapId，实际窗口:\n{controller}"
    );
    let changed = function_window(&src, "onFinalStarmapIdChanged:", 200);
    assert!(
        changed.contains("graphController.loadGraph()"),
        "解析出 finalStarmapId 后才加载本层图，实际窗口:\n{changed}"
    );
}

/// 5. Issue #822：解析入口对根层和子层是同一条 —— rootStarmapId / pathSegments /
/// backend 三者任一变化都重新解析，完成时也解析一次。
/// 子层不再"进入/返回"页面，所以没有 enterChildStarmap / returnToParentStarmap。
#[test]
fn resolution_wired_on_load_and_on_every_identity_change() {
    let src = strip_line_comments(&read_src(CONTENT));
    for trigger in [
        "onRootStarmapIdChanged: resolvePath()",
        "onPathSegmentsChanged: resolvePath()",
        "Component.onCompleted: resolvePath()",
    ] {
        assert!(
            src.contains(trigger),
            "Content 必须在 `{trigger}` 触发本层解析"
        );
    }
    assert!(
        src.contains("onStarmapBackendRefChanged:"),
        "后端注入时机不保证，backend 到达后必须能补解析一次"
    );

    let workspace = strip_line_comments(&read_src(WORKSPACE));
    assert!(
        !workspace.contains("enterChildStarmap")
            && !workspace.contains("returnToParentStarmap")
            && !workspace.contains("currentStarmapId"),
        "Workspace 不得再有下钻/返回导航栈，子星图是就地展开的递归内容"
    );
}

/// 6. Issue #822：根 Canvas 把外部 starmapId 作为 rootStarmapId 交给根 Content，
/// 加载本层图的入口是 Content 的 GraphController。
#[test]
fn root_canvas_passes_external_starmap_id_as_root_content_input() {
    let canvas = strip_line_comments(&read_src(CANVAS));
    let root_content = function_window(&canvas, "id: rootContent", 900);
    assert!(
        root_content.contains("rootStarmapId: canvasArea.starmapId"),
        "根 Content 必须把外部 starmapId 当作自己的 rootStarmapId，实际窗口:\n{root_content}"
    );
    assert!(
        root_content.contains("scenePathKey: \"root\""),
        "根层 scenePathKey 必须由根 Canvas 显式写死 root，实际窗口:\n{root_content}"
    );

    let content = strip_line_comments(&read_src(CONTENT));
    assert!(
        content.contains("starmapId: content.finalStarmapId")
            && content.contains("graphController.loadGraph()"),
        "Content 必须只用解析结果驱动本层 GraphController 加载"
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
