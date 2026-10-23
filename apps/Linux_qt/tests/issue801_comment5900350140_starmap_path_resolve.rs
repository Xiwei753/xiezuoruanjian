//! Issue #801 评论 5900350140 — root starmap + 路径段的当前层解析/加载守卫。
//!
//! WHITE_BOX + 真实运行验证评论 5900350140 的要求：
//! 1. Linux_Qt backend 暴露路径解析入口（root starmapId + 路径段 -> finalStarmapId）；
//! 2. 当前层加载使用解析结果，`currentStarmapId` 不再由点击事件的裸 target id 直接决定；
//! 3. 路径解析失败直接报错，不再回退到点击事件传来的裸 targetStarmapId。
//!
//! #929 更新（取代 #822 的递归 Content 模型）：`StarMapSceneContent.qml` 退化为
//! 只读微缩概览；"root + 完整路径段 → 当前活动画布"的解析落在 `StarMapCanvas.qml`
//! 的 `applyRoute()` 上，由 Core 导航路由 `resolve_starmap_navigation` 返回
//! `currentStarmapId`，再交给唯一的 `StarMapActiveScene` 加载。Workspace 仍不持有
//! `currentStarmapId` / 下钻 / 返回栈，返回按 `canvas.focusIsRoot` 分流。

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "common/source_guard.rs"]
mod source_guard;

use source_guard::{function_window, read_src};
use writer_core::api::WriterCoreApi;

const CANVAS: &str = "qml/StarMapCanvas.qml";
const CONTENT: &str = "qml/StarMapSceneContent.qml";
const ACTIVE_SCENE: &str = "qml/StarMapActiveScene.qml";
const WORKSPACE: &str = "qml/StarMapWorkspace.qml";
const MAIN: &str = "qml/main.qml";
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

/// 3. #929 取代 #822：Canvas 持有 Core 导航路由身份（rootStarmapId + 完整
///    segments），通过 backend resolve_starmap_navigation 解析出当前活动画布
///    currentStarmapId；解析失败清空 currentStarmapId 并报错，
///    不回退到任何裸 targetStarmapId。
#[test]
fn canvas_resolves_active_layer_from_root_plus_segments() {
    let src = strip_line_comments(&read_src(CANVAS));
    assert!(
        src.contains("property var currentRoute: ({ rootStarmapId: \"\", segments: [] })"),
        "Canvas 必须持有 Core 导航路由身份（rootStarmapId + 完整 segments），实际源码缺少"
    );
    let func = function_window(&src, "function applyRoute(route)", 1600);
    assert!(
        func.contains("starmapBackendRef.resolve_starmap_navigation("),
        "当前活动画布身份必须由 backend resolve_starmap_navigation 解析，实际窗口:\n{func}"
    );
    assert!(
        func.contains("res.data && res.data.currentStarmapId"),
        "解析结果必须读 currentStarmapId，实际窗口:\n{func}"
    );
    assert!(
        func.contains("currentStarmapId = \"\""),
        "解析失败必须清空 currentStarmapId 让子层停止加载，实际窗口:\n{func}"
    );
    assert!(
        !func.contains("targetStarmapId"),
        "路由解析不得回退到任何裸 targetStarmapId，实际窗口:\n{func}"
    );

    // SceneContent 是只读微缩概览，不再持有层级身份 / 解析职责。
    let content = strip_line_comments(&read_src(CONTENT));
    assert!(
        !content.contains("resolvePath")
            && !content.contains("finalStarmapId")
            && !content.contains("rootStarmapId"),
        "StarMapSceneContent 不得再持有本层身份解析（#929 已收进 Canvas）"
    );
}

/// 4. 活动画布的 GraphController 的 starmapId 只绑解析结果 currentStarmapId，
///    不绑点击事件或递归容器带来的任何裸 targetStarmapId。
#[test]
fn layer_starmap_id_only_comes_from_resolution() {
    let canvas = strip_line_comments(&read_src(CANVAS));
    // Canvas 只把解析结果交给唯一活动画布；再没有第二张画布在别处加载图。
    let loader = function_window(&canvas, "id: activeSceneLoader", 2000);
    assert!(
        loader.contains("starmapId: canvasArea.currentStarmapId"),
        "活动画布必须只绑解析结果 currentStarmapId，实际窗口:\n{loader}"
    );
    assert!(
        !loader.contains("targetStarmapId"),
        "活动画布不得绑任何裸 targetStarmapId，实际窗口:\n{loader}"
    );

    let active = strip_line_comments(&read_src(ACTIVE_SCENE));
    let controller = function_window(&active, "id: graphController", 600);
    assert!(
        controller.contains("starmapId: scene.starmapId"),
        "活动画布 GraphController 必须只绑解析后的 scene.starmapId，实际窗口:\n{controller}"
    );
    assert!(
        !controller.contains("targetStarmapId"),
        "本层 GraphController 不得绑裸 targetStarmapId，实际窗口:\n{controller}"
    );
    let reload = function_window(&active, "function reloadActiveGraph()", 400);
    assert!(
        reload.contains("graphController.loadGraph()"),
        "解析出当前画布后才加载本画布图，实际窗口:\n{reload}"
    );
    let changed = function_window(&active, "onStarmapIdChanged:", 200);
    assert!(
        changed.contains("reloadActiveGraph()"),
        "活动画布身份变化必须重新加载图，实际窗口:\n{changed}"
    );
}

/// 5. #929：解析入口对根画布和子画布是同一条 —— 外部 starmapId 变化、后端注入、
///    组件完成都重新走 applyRoute/resetToRoot。子画布不再"进入/返回"页面，
///    返回值按 Canvas 的 Core 导航路由状态分流，没有 enterChildStarmap /
///    returnToParentStarmap。
#[test]
fn resolution_wired_on_load_and_on_every_identity_change() {
    let canvas = strip_line_comments(&read_src(CANVAS));
    let completed = function_window(&canvas, "Component.onCompleted:", 200);
    assert!(
        completed.contains("resetToRoot()"),
        "组件完成时必须初始化路由与镜头，实际窗口:\n{completed}"
    );
    let identity_changed = function_window(&canvas, "onStarmapIdChanged:", 200);
    assert!(
        identity_changed.contains("resetToRoot()"),
        "外部 starmapId 变化必须重新解析根路由，实际窗口:\n{identity_changed}"
    );
    let backend_changed = function_window(&canvas, "onStarmapBackendRefChanged:", 300);
    assert!(
        backend_changed.contains("applyRoute(currentRoute)"),
        "后端注入时机不保证，到达后必须能补解析一次，实际窗口:\n{backend_changed}"
    );
    let reset = function_window(&canvas, "function resetToRoot()", 600);
    assert!(
        reset.contains("applyRoute(route)"),
        "根路由必须通过 applyRoute 解析当前活动画布，实际窗口:\n{reset}"
    );

    let workspace = strip_line_comments(&read_src(WORKSPACE));
    assert!(
        !workspace.contains("enterChildStarmap")
            && !workspace.contains("returnToParentStarmap")
            && !workspace.contains("currentStarmapId"),
        "Workspace 不得再有下钻/返回导航栈，活动画布身份归 Canvas"
    );
    assert!(
        workspace.contains("canvas.focusParentScene()") && workspace.contains("canvas.focusIsRoot"),
        "Workspace 返回按钮必须按 Canvas 的 Core 导航路由状态分流，实际源码缺少"
    );
}

/// 6. #929 取代 #822：根 Canvas 把外部 starmapId 作为整条导航路由的 root，
///    解析出的 currentStarmapId 驱动唯一活动画布加载；活动场景的 rootStarmapId
///    直接来自 currentRoute.rootStarmapId，scenePathKey 是 root id + 完整 segments
///    的 routeKey。
#[test]
fn root_canvas_passes_external_starmap_id_as_route_root() {
    let canvas = strip_line_comments(&read_src(CANVAS));
    let reset = function_window(&canvas, "function resetToRoot()", 600);
    assert!(
        reset.contains("var route = { rootStarmapId: starmapId, segments: [] }"),
        "外部 starmapId 必须成为导航路由的 root，实际窗口:\n{reset}"
    );
    assert!(
        reset.contains("applyRoute(route)") && reset.contains("cameraStack = ({})"),
        "切回根图必须清层相机并重新解析根路由，实际窗口:\n{reset}"
    );
    let loader = function_window(&canvas, "id: activeSceneLoader", 2000);
    assert!(
        loader.contains("rootStarmapId: canvasArea.currentRoute.rootStarmapId")
            && loader.contains("pathSegments: canvasArea.currentRoute.segments"),
        "活动画布必须拿到 currentRoute 的 rootStarmapId + 完整 segments，实际窗口:\n{loader}"
    );
    assert!(
        loader.contains("scenePathKey: canvasArea.routeKey"),
        "活动画布 scenePathKey 必须是 routeKey，实际窗口:\n{loader}"
    );
    let key = function_window(&canvas, "function routeKeyOf(route)", 800);
    assert!(
        key.contains("(route.rootStarmapId || \"\") + \"|\" + parts.join(\"/\")"),
        "routeKey 必须是 root id + 完整 segments 的拼接，实际窗口:\n{key}"
    );

    let active = strip_line_comments(&read_src(ACTIVE_SCENE));
    assert!(
        active.contains("function reloadActiveGraph()")
            && active.contains("graphController.loadGraph()"),
        "活动画布必须用解析结果驱动本画布 GraphController 加载"
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

/// Issue #984 第三轮复核：所有离开活动星图画布的路径都必须先提交节点编辑，
/// 保存失败时停在本层，不能继续换路由或销毁工作区。
#[test]
fn leaving_starmap_commits_pending_node_edit_first() {
    let canvas = strip_line_comments(&read_src(CANVAS));
    let helper = function_window(
        &canvas,
        "function commitPendingNodeEditBeforeNavigation()",
        700,
    );
    assert!(
        helper.contains("scene.flushPendingNodeEdit()"),
        "Canvas 离开前保存必须委托活动 Scene 的唯一编辑会话，实际窗口:\n{helper}"
    );
    assert!(
        helper.contains("routeError = qsTr") && helper.contains("return false"),
        "保存失败必须留在原画布并显示错误，实际窗口:\n{helper}"
    );

    let reset = function_window(&canvas, "function resetToRoot()", 900);
    assert!(
        reset.contains("if (!commitPendingNodeEditBeforeNavigation())")
            && reset.contains("return false")
            && reset.contains("applyRoute(route)"),
        "resetToRoot 必须先确认保存成功再解析根路由，实际窗口:\n{reset}"
    );

    let reveal = function_window(&canvas, "function revealSearchTarget(target)", 2200);
    assert!(
        reveal.contains("if (!resetToRoot())"),
        "全局搜索定位子图返回根图时必须先走 resetToRoot 的保存守卫，实际窗口:\n{reveal}"
    );

    let backend = function_window(&canvas, "onStarmapBackendRefChanged:", 500);
    assert!(
        backend.contains("commitPendingNodeEditBeforeNavigation()")
            && backend.contains("applyRoute(currentRoute)"),
        "后端晚注入重解析前也必须提交当前编辑，实际窗口:\n{backend}"
    );

    let workspace = strip_line_comments(&read_src(WORKSPACE));
    let back = function_window(&workspace, "function navigateBack()", 700);
    assert!(
        back.contains("canvas.flushPendingNodeEdit()") && back.contains("root.backClicked()"),
        "根图返回工作台必须先经 Canvas 统一保存入口，实际窗口:\n{back}"
    );
    assert!(
        back.find("canvas.flushPendingNodeEdit()").unwrap()
            < back.find("root.backClicked()").unwrap(),
        "保存成功前不得发出 backClicked，实际窗口:\n{back}"
    );

    let main = strip_line_comments(&read_src(MAIN));
    let search = function_window(&main, "function navigateToSearchResult(result)", 2600);
    assert!(
        search.contains("appController.inStarmap")
            && search.contains("starmapWorkspace.flushPendingNodeEdit()")
            && search.contains("globalSearchOverlay.showMessage("),
        "从星图搜索跳章节/根图前必须走 Workspace 保存入口并提示失败，实际窗口:\n{search}"
    );
    assert!(
        search
            .find("starmapWorkspace.flushPendingNodeEdit()")
            .unwrap()
            < search.find("appController.openWriting(").unwrap(),
        "保存未确认前不得切换工作区，实际窗口:\n{search}"
    );
}
