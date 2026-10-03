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
//!    `true/false`，命中层的 `connect_end.success` 使用这个返回值，不再
//!    找到目标就写 true。
//!
//! #822 更新：根 Canvas 不再持有节点/Embed/连线的业务入口，这三处的落点
//! 都搬到了递归层容器 `StarMapSceneContent.qml`（Canvas 只剩全局相机与
//! 全局输入）。Canvas 侧的 `findEmbedContentAt` 转发也换成根递归命中入口
//! `hitTargetAtScreen` → `hitTargetAtScene`。

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "common/source_guard.rs"]
mod source_guard;

use source_guard::{function_window, read_src};

const CONTROLLER: &str = "qml/StarMapGraphController.qml";
const CANVAS: &str = "qml/StarMapCanvas.qml";
const CONTENT: &str = "qml/StarMapSceneContent.qml";

// ─────────────────────────────────────────────────────────────────────────
// 1. 子星图内部点击不再冒充 empty
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn controller_has_find_embed_content_at() {
    let src = read_src(CONTROLLER);
    let window = function_window(&src, "function findEmbedContentAt(", 1300);
    assert!(
        window.contains("_insideEmbedCircle(em, wx, wy)")
            && window.contains("_chromeHeight")
            && window.contains("_insideEmbedBorderRing(em, wx, wy)"),
        "findEmbedContentAt 必须先做圆内判定，再排除 chrome（标题带 + 圆周环），\
         实际窗口:\n{window}"
    );
    // 必须返回 embed 对象（带 instanceId），不是只返回布尔
    assert!(
        window.contains("return em"),
        "findEmbedContentAt 命中 content 区域必须返回 embed 对象，实际窗口:\n{window}"
    );
}

#[test]
fn content_recursive_hit_uses_find_embed_content_at() {
    let src = read_src(CONTENT);
    let window = function_window(&src, "function hitTargetAtScene(", 3000);
    assert!(
        window.contains("graphController.findEmbedContentAt("),
        "Content 的递归命中入口必须调用本层 graphController.findEmbedContentAt，实际窗口:\n{window}"
    );
}

#[test]
fn canvas_pointer_press_distinguishes_embed_chrome_and_child_content() {
    // Issue #817 评论 5949494799: 命中种类统一由递归命中入口产出
    // （node / embed / childContent / edge / empty），logPointerPress 只透传
    // hit.kind。Issue #822 后该入口是 Content 的 hitTargetAtScene，
    // Canvas 的 hitTargetAtScreen 只做 screen→scene 转换后转发。
    let canvas = read_src(CANVAS);
    let screen_window = function_window(&canvas, "function hitTargetAtScreen(", 400);
    assert!(
        screen_window.contains("rootContent.hitTargetAtScene(screenToWorldX(sx), screenToWorldY(sy))"),
        "hitTargetAtScreen 必须转发到根 Content 的递归命中入口，实际窗口:\n{screen_window}"
    );

    let content = read_src(CONTENT);
    let hit_window = function_window(&content, "function hitTargetAtScene(", 3000);
    for kind in ["\"node\"", "\"embed\"", "\"childContent\"", "\"edge\"", "\"empty\""] {
        assert!(
            hit_window.contains(&format!("kind: {kind}")),
            "hitTargetAtScene 必须能返回 kind: {kind}，实际窗口:\n{hit_window}"
        );
    }
    assert!(
        hit_window.contains("owner: content"),
        "hitTargetAtScene 必须返回真正的命中层 owner，菜单/连线/选中都靠它定位，实际窗口:\n{hit_window}"
    );

    let window = function_window(&canvas, "function logPointerPress(", 900);
    assert!(
        window.contains("hitTargetAtScreen(point.position.x, point.position.y)"),
        "logPointerPress 必须走统一递归命中入口，实际窗口:\n{window}"
    );
    assert!(
        window.contains("logInteraction(\"pointer_press\", kind,"),
        "logPointerPress 必须把命中的 kind 原样写进 pointer_press 日志，实际窗口:\n{window}"
    );
    // 不应再保留旧的单一 "embed" hitKind（应已拆成 embed / childContent）
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
fn content_move_end_uses_commit_return_values() {
    let src = read_src(CONTENT);
    let window = function_window(&src, "function finishMove(", 1400);
    assert!(
        window.contains("committed = graphController.commitNodeMove(")
            && window.contains("committed = graphController.commitEmbedMove("),
        "命中层的 finishMove 必须接 commitNodeMove/commitEmbedMove 返回值到 committed，实际窗口:\n{window}"
    );
    assert!(
        window.contains("\"commitSuccess\": committed"),
        "move_end 日志必须写 commitNodeMove/commitEmbedMove 的真实返回值，实际窗口:\n{window}"
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
fn content_create_edge_with_paths_forwards_return_value() {
    let src = read_src(CONTENT);
    let window = function_window(&src, "function createEdgeWithPaths(", 400);
    assert!(
        window.contains("return graphController.createEdgeWithPaths"),
        "Content 的 createEdgeWithPaths 必须返回 graphController 的返回值，实际窗口:\n{window}"
    );
}

#[test]
fn content_connect_end_uses_create_edge_return_value() {
    let src = read_src(CONTENT);
    let window = function_window(&src, "function finishConnect(", 2000);
    assert!(
        window.contains("success = createEdgeWithPaths(fromPath, toPath)"),
        "connect_end.success 必须使用 createEdgeWithPaths 返回值，实际窗口:\n{window}"
    );
    assert!(
        window.contains("\"success\": success"),
        "connect_end 日志必须写 createEdgeWithPaths 的真实返回值，实际窗口:\n{window}"
    );
    // 建边由源的归属层执行，from/to 都保持完整路径 DTO
    assert!(
        window.contains("hit.targetPath") && window.contains("var fromPath = ic.connectFromPath"),
        "connect_end 必须用完整 StarMapTargetPathDto（from 与 hit.targetPath），不退化成 nodeId-only，实际窗口:\n{window}"
    );
}
