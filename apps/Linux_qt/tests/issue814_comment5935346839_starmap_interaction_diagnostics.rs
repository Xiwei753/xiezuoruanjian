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
//! 2. `StarMapCanvas.qml` 有统一 `logInteraction()` helper，且 9 个手势边界
//!    事件（pointer_press / pan_begin / pan_end / move_begin / move_end /
//!    connect_begin / connect_end / selection_changed / context_menu_open）都在；
//!    `pointer_press` 必须能覆盖"按在 Node/Embed 上"的场景（背景 MouseArea
//!    收不到这类 press），所以挂在根节点的 passive-grab PointHandler 上。
//! 3. 日志不进 `onPositionChanged` 这类连续移动热路径。
//! 4. `StarMapScene.qml` 记录 scene_resolved / scene_resolve_failed。
//! 5. `StarMapEmbed.qml` 只记录事件分层（embed_chrome_press /
//!    embed_child_content_routed / embed_child_scene_activated），并带
//!    parentPathKey / childScenePathKey / instanceId / targetStarmapId。
//! 6. `StarMapNode.qml` 不自己直接落盘，避免同一次点击记两份。

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "common/source_guard.rs"]
mod source_guard;

use source_guard::{function_window, read_src};

const BACKEND: &str = "src/backend/starmap_backend.rs";
const CANVAS: &str = "qml/StarMapCanvas.qml";
const SCENE: &str = "qml/StarMapScene.qml";
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
    let src = read_src(CANVAS);
    let helper = function_window(&src, "function logInteraction(", 500);
    assert!(
        helper.contains("starmapBackendRef.record_interaction(event, pathKey, starmapId"),
        "logInteraction 必须把 scenePathKey/starmapId 一并传给 record_interaction，实际窗口:\n{helper}"
    );
}

#[test]
fn canvas_logs_all_gesture_boundary_events() {
    let src = strip_line_comments(&read_src(CANVAS));
    for event in [
        "pointer_press",
        "pan_begin",
        "pan_end",
        "move_begin",
        "move_end",
        "connect_begin",
        "connect_end",
        "selection_changed",
        "context_menu_open",
    ] {
        assert!(
            src.contains(&format!("logInteraction(\"{event}\"")),
            "StarMapCanvas 必须记录 {event} 边界日志"
        );
    }
}

/// pointer_press 必须由 passive-grab PointHandler 观察：背景 MouseArea 在按到
/// Node/Embed 时收不到 press（对象 TapHandler 先取 exclusive grab），
/// 而"按在对象上没反应"正是要诊断的场景。
#[test]
fn canvas_pointer_press_covers_object_presses_via_passive_handlers() {
    let src = read_src(CANVAS);
    assert!(
        src.contains("function logPointerPress("),
        "StarMapCanvas 必须有统一的 pointer_press 计算/落盘函数"
    );
    for call in [
        "logPointerPress(\"left\", \"mouse\", point)",
        "logPointerPress(\"middle\", \"mouse\", point)",
        "logPointerPress(\"right\", \"mouse\", point)",
        "logPointerPress(\"left\", \"touch\", point)",
    ] {
        assert!(src.contains(call), "pointer_press 观察器必须覆盖 {call}");
    }
    let observers = strip_line_comments(&src);
    let count = observers.matches("logPointerPress(").count();
    assert!(
        count >= 4,
        "至少要有 4 个 press 边界观察入口（左/中/右/触屏），实际 {count}"
    );
}

#[test]
fn canvas_does_not_log_in_continuous_move_hot_path() {
    let src = read_src(CANVAS);
    let handler = slice_between(&src, "onPositionChanged: function(mouse)", "onReleased:");
    assert!(
        !handler.contains("logInteraction") && !handler.contains("logPointerPress"),
        "连续移动 onPositionChanged 不得逐帧落盘，实际窗口:\n{handler}"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 3. Scene / Embed / Node
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn scene_logs_resolve_success_and_failure() {
    let src = read_src(SCENE);
    let resolved = function_window(&src, "\"scene_resolved\"", 400);
    assert!(
        resolved.contains("pathKey")
            && resolved.contains("finalStarmapId")
            && resolved.contains("depth"),
        "scene_resolved 必须带 pathKey/rootStarmapId/finalStarmapId/depth，实际窗口:\n{resolved}"
    );
    let failed = function_window(&src, "\"scene_resolve_failed\"", 400);
    assert!(
        failed.contains("errorCode") && failed.contains("pathKey"),
        "scene_resolve_failed 必须带 pathKey/errorCode，实际窗口:\n{failed}"
    );
}

#[test]
fn embed_logs_only_its_own_event_layering() {
    let src = read_src(EMBED);
    assert!(
        src.contains("function logEmbedInteraction(")
            && src.contains("starmapBackendRef.record_interaction(event, parentPathKey, targetStarmapId, \"embed\", instanceId"),
        "StarMapEmbed 必须通过 record_interaction 记录自己的事件分层"
    );
    for event in [
        "embed_chrome_press",
        "embed_child_content_routed",
        "embed_child_scene_activated",
    ] {
        let at = src
            .find(&format!("logEmbedInteraction(\"{event}\""))
            .unwrap_or_else(|| panic!("StarMapEmbed 必须记录 {event}"));
        let window = function_window(&src[at..], &format!("logEmbedInteraction(\"{event}\""), 400);
        for field in [
            "parentPathKey",
            "childScenePathKey",
            "instanceId",
            "targetStarmapId",
        ] {
            assert!(
                window.contains(field),
                "{event} 必须带 {field}，实际窗口:\n{window}"
            );
        }
    }
    assert!(
        !src.contains("logEmbedInteraction(\"move_"),
        "Embed 不记录连续 move 日志，move 边界统一由 Canvas 记"
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
