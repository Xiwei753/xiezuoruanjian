//! Issue #814 评论 5945557717 — 星图诊断日志"说真话"守卫。
//!
//! 评论 5935346839 接通了星图交互边界日志，但 5945557717 复核发现三处会让
//! 诊断包在真正出错时给出错误结论的地方，本测试锁住这三处修复不再回退：
//!
//! 1. **子星图内部点击不再冒充 empty**：`findEmbedContentAt` 存在，且
//!    `logPointerPress` 把 hitKind 明确区分成 `node / embedChrome /
//!    childContent / edge / empty`。合法的子场景内部点击记成 `childContent`
//!    并带 instanceId，不再落成 `empty`（empty 应只表示坐标/命中错误）。
//! 2. **move_end.commitSuccess 是真实后端结果**：Canvas 不再无条件写
//!    `"commitSuccess": true`，而是接 `commitNodeMove/commitEmbedMove` 的
//!    返回值。
//! 3. **connect_end.success 是真实后端结果**：`createEdgeWithPaths` 返回
//!    `true/false`，Canvas 的 `connect_end.success` 使用这个返回值，不再
//!    找到目标就写 true。

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "common/source_guard.rs"]
mod source_guard;

use source_guard::{function_window, read_src};

const CONTROLLER: &str = "qml/StarMapGraphController.qml";
const CANVAS: &str = "qml/StarMapCanvas.qml";

// ─────────────────────────────────────────────────────────────────────────
// 1. 子星图内部点击不再冒充 empty
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn controller_has_find_embed_content_at() {
    let src = read_src(CONTROLLER);
    let window = function_window(&src, "function findEmbedContentAt(", 1300);
    assert!(
        window.contains("_rectContains(em.x, em.y, em.width, em.height, wx, wy)")
            && window.contains("_chromeHeight")
            && window.contains("_borderSlop"),
        "findEmbedContentAt 必须判断整个 Embed 矩形内、排除 chrome 区域，实际窗口:\n{window}"
    );
    // 必须返回 embed 对象（带 instanceId），不是只返回布尔
    assert!(
        window.contains("return em"),
        "findEmbedContentAt 命中 content 区域必须返回 embed 对象，实际窗口:\n{window}"
    );
}

#[test]
fn canvas_forwards_find_embed_content_at() {
    let src = read_src(CANVAS);
    let window = function_window(&src, "function findEmbedContentAt(", 200);
    assert!(
        window.contains("graphController.findEmbedContentAt"),
        "Canvas 必须转发 findEmbedContentAt 到 graphController，实际窗口:\n{window}"
    );
}

#[test]
fn canvas_pointer_press_distinguishes_embed_chrome_and_child_content() {
    let src = read_src(CANVAS);
    // Issue #817 评论 5949494799: 命中种类统一由 hitPointerAtScreen 产出
    // （node / embedChrome / childContent / edge / empty），logPointerPress 只透传
    // hit.kind。守卫跟随新契约把“种类判定”和“日志透传”分开检查，覆盖不减弱：
    // embedChrome/childContent 的区分和 findEmbedContentAt 调用仍然必须存在。
    let hit_window = function_window(&src, "function hitPointerAtScreen(", 900);
    assert!(
        hit_window.contains("\"embedChrome\""),
        "hitPointerAtScreen 必须把 Embed chrome 命中区分成 embedChrome，实际窗口:\n{hit_window}"
    );
    assert!(
        hit_window.contains("\"childContent\""),
        "hitPointerAtScreen 必须把子场景内部命中区分成 childContent，实际窗口:\n{hit_window}"
    );
    assert!(
        hit_window.contains("findEmbedContentAt(wx, wy)"),
        "hitPointerAtScreen 必须调用 findEmbedContentAt 判 childContent，实际窗口:\n{hit_window}"
    );

    let window = function_window(&src, "function logPointerPress(", 900);
    assert!(
        window.contains("hitPointerAtScreen(point.position.x, point.position.y)"),
        "logPointerPress 必须走 hitPointerAtScreen 统一命中入口，实际窗口:\n{window}"
    );
    assert!(
        window.contains("logInteraction(\"pointer_press\", hit.kind,"),
        "logPointerPress 必须把 hit.kind 原样写进 pointer_press 日志，实际窗口:\n{window}"
    );
    // 不应再保留旧的单一 "embed" hitKind（应已拆成 embedChrome/childContent）
    assert!(
        !window.contains("hitKind = \"embed\""),
        "logPointerPress 不应再使用旧的单一 \"embed\" hitKind，实际窗口:\n{window}"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 2. move_end.commitSuccess 是真实后端结果
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn canvas_move_end_does_not_hardcode_commit_success_true() {
    let src = read_src(CANVAS);
    // 所有 move_end 日志都不得再无条件写 "commitSuccess": true
    assert!(
        !src.contains("\"commitSuccess\": true"),
        "move_end.commitSuccess 不得硬编码 true，必须接 commitNodeMove/commitEmbedMove 返回值"
    );
}

#[test]
fn canvas_move_end_uses_commit_return_values() {
    let src = read_src(CANVAS);
    // 触屏路径：commitNodeMove/commitEmbedMove 返回值赋给变量
    assert!(
        src.contains("_touchCommitOk = graphController.commitNodeMove(")
            && src.contains("_touchCommitOk = graphController.commitEmbedMove("),
        "触屏 move_end 必须接 commit 返回值到 _touchCommitOk"
    );
    // 鼠标 node 路径
    assert!(
        src.contains("_nodeCommitOk = graphController.commitNodeMove("),
        "鼠标 node move_end 必须接 commitNodeMove 返回值到 _nodeCommitOk"
    );
    // 鼠标 embed 路径
    assert!(
        src.contains("_embedCommitOk = graphController.commitEmbedMove("),
        "鼠标 embed move_end 必须接 commitEmbedMove 返回值到 _embedCommitOk"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 3. connect_end.success 是真实后端结果
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn controller_create_edge_with_paths_returns_bool() {
    let src = read_src(CONTROLLER);
    let window = function_window(&src, "function createEdgeWithPaths(", 700);
    assert!(
        window.contains("return false") && window.contains("return true"),
        "createEdgeWithPaths 必须成功返回 true、失败返回 false，实际窗口:\n{window}"
    );
    // ensureBackend 失败也要返回 false，不能裸 return
    assert!(
        window.contains("if (!ensureBackend()) return false;"),
        "createEdgeWithPaths 的 ensureBackend 失败分支必须 return false，实际窗口:\n{window}"
    );
}

#[test]
fn canvas_create_edge_with_paths_forwards_return_value() {
    let src = read_src(CANVAS);
    let window = function_window(&src, "function createEdgeWithPaths(", 200);
    assert!(
        window.contains("return graphController.createEdgeWithPaths"),
        "Canvas 的 createEdgeWithPaths 必须返回 graphController 的返回值，实际窗口:\n{window}"
    );
}

#[test]
fn canvas_connect_end_uses_create_edge_return_value() {
    let src = read_src(CANVAS);
    // node 端和 embed 端都必须把 createEdgeWithPaths 返回值赋给 success 变量
    assert!(
        src.contains("_connectSuccess = createEdgeWithPaths("),
        "node 端 connect_end.success 必须使用 createEdgeWithPaths 返回值"
    );
    assert!(
        src.contains("_eSuccess = createEdgeWithPaths("),
        "embed 端 connect_end.success 必须使用 createEdgeWithPaths 返回值"
    );
    // 不应再保留"调用后无条件写 true"的旧模式
    assert!(
        !src.contains("createEdgeWithPaths(interaction.connectFromPath, _toPath)\n                            _connectSuccess = true"),
        "node 端不得再调用 createEdgeWithPaths 后无条件写 _connectSuccess = true"
    );
    assert!(
        !src.contains("createEdgeWithPaths(interaction.connectFromPath, _eToPath)\n                                _eSuccess = true"),
        "embed 端不得再调用 createEdgeWithPaths 后无条件写 _eSuccess = true"
    );
}
