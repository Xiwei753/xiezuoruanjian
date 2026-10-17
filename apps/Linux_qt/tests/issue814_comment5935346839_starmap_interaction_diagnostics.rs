//! Issue #814 评论 5935346839 — 星图交互边界诊断日志接线守卫。
//!
//! WHITE_BOX 验证策略：QML 无法在本仓库的 Rust 测试里实例化（组件依赖
//! `qml_resources` qrc 与 Rust 注册的上下文属性），因此按仓库既有惯例
//! （见 `issue801_comment5895709352_starmap_embed_single_semantics.rs` /
//! `issue762_comment5826175490_qml_wiring.rs`）读取 QML 源码，确定性断言这次
//! 修复的接线不变量，防止回退：
//!
//! 1. `StarMapBackend.record_interaction` 直接走
//!    `writer_diagnostics::record_event`，origin=User、事件名 `starmap.` 前缀、
//!    target=`linux_qt.starmap`，不受 `WRITER_DEBUG_QML` 控制；
//!    `fields_json` 解析失败只记 `fields_parse_error=true`，不中断事件落盘。
//! 2. `StarMapCanvas.qml` 与 `StarMapActiveScene.qml` 各有统一 `logInteraction()`
//!    helper，且 9 个手势边界事件（pointer_press / pan_begin / pan_end /
//!    move_begin / move_end / connect_begin / connect_end / selection_changed /
//!    context_menu_open）都在；`pointer_press` 必须能覆盖"按在 Node/Embed 上"的
//!    场景（背景 MouseArea 收不到这类 press），所以挂在根节点的 passive-grab
//!    PointHandler 上。
//! 3. 日志不进 `onPositionChanged` 这类连续移动热路径。
//! 4. 画布穿越的真相由 `StarMapCanvas` 记录：scene_enter / scene_exit /
//!    scene_enter_failed / scene_exit_failed（带 routeKey、trigger、覆盖率）。
//! 5. `StarMapEmbed.qml` 是父图里的纯渲染对象：不观察输入、不落交互日志。
//! 6. `StarMapNode.qml` 不自己直接落盘，避免同一次点击记两份。
//!
//! #822 更新：`StarMapScene.qml` 已删除，根层由 `StarMapCanvas.qml` 直接创建。
//! #929 更新：独立画布协议删除递归 Content 交互层。pointer_press /
//! pan_begin / pan_end / selection_changed / context_menu_open 属于全局相机与
//! 全局输入，仍在 Canvas / Router；move_end / connect_end 属于活动画布手势收尾，
//! 改由唯一 `StarMapActiveScene` 记录；`StarMapSceneContent` 是只读微缩概览，
//! 不记录任何交互边界日志。

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "common/source_guard.rs"]
mod source_guard;

use source_guard::{function_window, read_src};

const BACKEND: &str = "src/backend/starmap_backend.rs";
const CANVAS: &str = "qml/StarMapCanvas.qml";
const ACTIVE_SCENE: &str = "qml/StarMapActiveScene.qml";
const CONTENT: &str = "qml/StarMapSceneContent.qml";
const EMBED: &str = "qml/StarMapEmbed.qml";
const NODE: &str = "qml/StarMapNode.qml";

/// 取从 `start`（含）到 `end`（不含）之间的源码片段；缺任一 marker 直接失败。
fn slice_between(src: &str, start: &str, end: &str) -> String {
    let s = src
        .find(start)
        .unwrap_or_else(|| panic!("missing marker `{start}`"));
    let e = src[s..]
        .find(end)
        .map(|i| s + i)
        .unwrap_or_else(|| panic!("missing marker `{end}`"));
    src[s..e].to_string()
}

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

// ─────────────────────────────────────────────────────────────────────────
// 1. 后端 record_interaction
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn backend_exposes_record_interaction_as_qml_method() {
    let src = read_src(BACKEND);
    assert!(
        src.contains("record_interaction: qt_method!("),
        "StarMapBackend 必须把 record_interaction 暴露成 QML 可调用的 qt_method"
    );
    let window = function_window(&src, "fn record_interaction(", 2600);
    assert!(
        window.contains("event: QString")
            && window.contains("scene_path_key: QString")
            && window.contains("starmap_id: QString")
            && window.contains("item_kind: QString")
            && window.contains("item_id: QString")
            && window.contains("fields_json: QString"),
        "record_interaction 必须接收 event/scene_path_key/starmap_id/item_kind/item_id/fields_json，实际窗口:\n{window}"
    );
}

#[test]
fn backend_record_interaction_goes_to_writer_diagnostics() {
    let src = read_src(BACKEND);
    let window = function_window(&src, "fn record_interaction(", 2600);
    assert!(
        window.contains("writer_diagnostics::record_event"),
        "record_interaction 必须直接走 writer_diagnostics::record_event，实际窗口:\n{window}"
    );
    assert!(
        window.contains("DiagnosticOrigin::User"),
        "星图交互是用户操作，origin 必须是 User，实际窗口:\n{window}"
    );
    assert!(
        window.contains("format!(\"starmap.{event}\")"),
        "事件名必须统一加 starmap. 前缀，实际窗口:\n{window}"
    );
    assert!(
        window.contains("\"linux_qt.starmap\""),
        "target 必须是 linux_qt.starmap，实际窗口:\n{window}"
    );
    assert!(
        !window.contains("WRITER_DEBUG_QML") && !window.contains("std::env::var"),
        "record_interaction 不得受 WRITER_DEBUG_QML / 环境变量控制，实际窗口:\n{window}"
    );
}

#[test]
fn backend_fields_json_parse_failure_only_marks_flag() {
    let src = read_src(BACKEND);
    let window = function_window(&src, "fn record_interaction(", 2600);
    assert!(
        window.contains("\"fields_parse_error\"") && window.contains("true.into()"),
        "fields_json 解析失败必须记录 fields_parse_error=true，实际窗口:\n{window}"
    );
    assert!(
        window.contains("fields.extend(obj)")
            && window.contains("writer_diagnostics::record_event"),
        "fields_json 解析失败不能让交互或事件落盘中断，实际窗口:\n{window}"
    );
    let err_branch = slice_between(&window, "Err(_) => {", "writer_diagnostics::record_event");
    assert!(
        !err_branch.contains("return"),
        "fields_json 解析失败分支不得提前 return 丢掉事件，实际分支:\n{err_branch}"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 2. Canvas 边界事件
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn canvas_has_unified_log_interaction_helper() {
    // #929：scenePathKey 缺省用当前 routeKey（根 ID + 完整 segments），
    // starmapId 用当前活动画布，不再写死 "root"。
    let src = read_src(CANVAS);
    let helper = function_window(&src, "function logInteraction(", 500);
    assert!(
        helper.contains("scenePathKey === undefined ? routeKey : scenePathKey,")
            && helper.contains("currentStarmapId !== \"\" ? currentStarmapId : starmapId,")
            && helper.contains("itemKind, itemId, fj)"),
        "logInteraction 必须把当前 routeKey/starmapId 一并传给 record_interaction，实际窗口:\n{helper}"
    );
}

#[test]
fn active_scene_has_unified_log_interaction_helper() {
    // #929 取代 #822：递归 Content 不再记录交互；活动画布 StarMapActiveScene
    // 用本 route 的 scenePathKey/starmapId 落盘命中层的手势收尾。
    let src = read_src(ACTIVE_SCENE);
    let helper = function_window(&src, "function logInteraction(", 500);
    assert!(
        helper.contains("starmapBackendRef.record_interaction(event, scenePathKey, starmapId, itemKind, itemId, fj)"),
        "StarMapActiveScene 的 logInteraction 必须用本 route 的 scenePathKey/starmapId 落盘，实际窗口:\n{helper}"
    );
    // 只读微缩概览不是活动画布，不得记录任何交互日志。
    let content = read_src(CONTENT);
    assert!(
        !content.contains("function logInteraction(") && !content.contains("record_interaction"),
        "只读微缩概览不得记录交互日志"
    );
}

/// Issue #832：边界事件按"谁解释输入"落点——
/// - 全局相机/press 观察记在 Canvas（Canvas.logPointerPress / context_menu_open）；
/// - 单指手势边界（pan / selection / move_begin / connect_begin / zoom）记在唯一 Router；
/// - 活动画布上的收尾（move_end / connect_end）记在 StarMapActiveScene。
/// #929：画布穿越边界（scene_enter / scene_exit）也记在 Canvas，且以
/// routeKey 说真话。
#[test]
fn canvas_router_and_active_scene_log_all_gesture_boundary_events() {
    let canvas = strip_line_comments(&read_src(CANVAS));
    let router = strip_line_comments(&read_src("qml/StarMapInputRouter.qml"));
    let scene = strip_line_comments(&read_src(ACTIVE_SCENE));
    let content = strip_line_comments(&read_src(CONTENT));

    for event in [
        "pointer_press",
        "context_menu_open",
        "scene_enter",
        "scene_exit",
    ] {
        assert!(
            canvas.contains(&format!("logInteraction(\"{event}\"")),
            "StarMapCanvas 必须记录 {event} 边界日志"
        );
    }
    for event in [
        "pan_begin",
        "pan_end",
        "selection_changed",
        "move_begin",
        "connect_begin",
        "zoom_wheel",
    ] {
        assert!(
            router.contains(&format!("logInteraction(\"{event}\"")),
            "StarMapInputRouter 必须记录 {event} 边界日志"
        );
    }
    for event in ["move_end", "connect_end"] {
        assert!(
            scene.contains(&format!("logInteraction(\"{event}\"")),
            "StarMapActiveScene 必须记录活动画布上的 {event} 边界日志"
        );
    }
    for event in ["move_end", "connect_end"] {
        assert!(
            !content.contains(&format!("logInteraction(\"{event}\"")),
            "只读微缩概览不得记录 {event}：概览不可交互"
        );
    }
}

/// Issue #832：pointer_press 由唯一 Router 在按下当场调用 Canvas 的统一
/// 计算/落盘函数：左键（鼠标/触屏共用，device 由 source 给出）与右键两路。
/// 中键历史 pan 入口已删除。
#[test]
fn router_pointer_press_covers_left_and_right() {
    let canvas = read_src(CANVAS);
    assert!(
        canvas.contains("function logPointerPress("),
        "StarMapCanvas 必须有统一的 pointer_press 计算/落盘函数"
    );
    let router = strip_line_comments(&read_src("qml/StarMapInputRouter.qml"));
    assert!(
        router.contains("canvas.logPointerPress(\"left\", source, point)"),
        "左键（鼠标/触屏）按下必须记 pointer_press"
    );
    assert!(
        router.contains("canvas.logPointerPress(\"right\", \"mouse\", point)"),
        "右键按下必须记 pointer_press"
    );
    assert!(
        !router.contains("logPointerPress(\"middle\"") && !canvas.contains("Qt.MiddleButton"),
        "中键历史 pan 入口已删除，不得再保留 middle press 观察器"
    );
    let count = router.matches("logPointerPress(").count();
    assert!(count >= 2, "左右两路 press 边界都要有，实际 {count}");
}

#[test]
fn router_does_not_log_in_continuous_move_hot_path() {
    // 越过阈值后的持续 pan / move / connect 分支是逐帧热路径，不得落盘；
    // pan_begin / move_begin / connect_begin 只发生一次，允许在边界处记录。
    let src = read_src("qml/StarMapInputRouter.qml");
    let hot_path = function_window(&src, "if (mode === \"pan\" && panActive) {", 300);
    assert!(
        !hot_path.contains("logInteraction") && !hot_path.contains("logPointerPress"),
        "持续 pan 分支不得逐帧落盘，实际窗口:\n{hot_path}"
    );
    let move_path = function_window(&src, "if (mode === \"move\") {", 300);
    assert!(
        !move_path.contains("logInteraction"),
        "持续 move 分支不得逐帧落盘，实际窗口:\n{move_path}"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 3. ActiveScene / Content / Embed / Node
// ─────────────────────────────────────────────────────────────────────────

/// #929 取代 #822：不再有递归 scene_resolved / scene_resolve_failed。
/// 画布穿越的真相由 Canvas 的 scene_enter / scene_exit 记录（routeKey、
/// trigger、父子 zoom 与覆盖率），失败路径带 errorCode。
#[test]
fn canvas_logs_canvas_enter_and_exit_with_route_truth() {
    let src = read_src(CANVAS);
    let enter = function_window(&src, "logInteraction(\"scene_enter\"", 500);
    for field in [
        "trigger",
        "childRouteKey",
        "childStarmapId",
        "parentZoom",
        "childZoom",
    ] {
        assert!(
            enter.contains(field),
            "scene_enter 必须带 {field}，实际窗口:\n{enter}"
        );
    }
    let exit = function_window(&src, "logInteraction(\"scene_exit\"", 500);
    for field in ["parentRouteKey", "continuous", "childZoom", "parentZoom"] {
        assert!(
            exit.contains(field),
            "scene_exit 必须带 {field}，实际窗口:\n{exit}"
        );
    }
    let failed = function_window(&src, "logInteraction(\"scene_enter_failed\"", 400);
    assert!(
        failed.contains("errorCode") && failed.contains("trigger"),
        "scene_enter_failed 必须带 trigger/errorCode，实际窗口:\n{failed}"
    );
    let failed_exit = function_window(&src, "logInteraction(\"scene_exit_failed\"", 400);
    assert!(
        failed_exit.contains("errorCode"),
        "scene_exit_failed 必须带 errorCode，实际窗口:\n{failed_exit}"
    );

    let content = read_src(CONTENT);
    assert!(
        !content.contains("scene_resolved") && !content.contains("scene_resolve_failed"),
        "只读微缩概览不得再假装解析场景层级"
    );
}

/// #929 取代 #832：Embed 是父图里的纯渲染对象，不再观察输入、也不再落任何
/// 诊断日志；"进入子画布"的真相由 Canvas 走 Core 导航并记 scene_enter。
#[test]
fn embed_no_longer_logs_interactions() {
    let stripped = strip_line_comments(&read_src(EMBED));
    for removed in [
        "record_interaction",
        "logEmbedInteraction",
        "embed_chrome_press",
        "embed_child_content_routed",
        "embed_child_content_activated",
    ] {
        assert!(
            !stripped.contains(removed),
            "StarMapEmbed 不得再保留交互日志 {removed}：进入画布由 Canvas.scene_enter 记录"
        );
    }

    let canvas = read_src(CANVAS);
    assert!(
        canvas.contains("enter_starmap_embed(")
            && canvas.contains("logInteraction(\"scene_enter\""),
        "进入子画布必须由 Canvas 走 Core 导航并记 scene_enter"
    );
}

#[test]
fn node_does_not_write_diagnostics_directly() {
    let src = read_src(NODE);
    assert!(
        !src.contains("record_interaction") && !src.contains("writer_diagnostics"),
        "StarMapNode 不得自己直接落盘诊断日志，避免与 Canvas 重复记录"
    );
}
