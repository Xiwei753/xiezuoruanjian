//! Issue #814 评论 5946366104 — 父层背景 Handler 不得吞掉子星图内部命中（守卫更新为 #822 架构）。
//!
//! 评论 5946366104 复核发现父层背景 Handler 仍把 child content 当背景：四个背景
//! tap Handler（bgMouseLeftTap / bgTouchLeftTap.onSingleTapped /
//! bgTouchLeftTap.onLongPressed / backgroundRightTap.onSingleTapped）在 Node/Embed
//! chrome 判断后没有判断子星图内容区，点击子星图内部 child node 时父层仍
//! `clearSelection()` 吞掉子场景选中。
//!
//! Issue #822 重构后整棵星图只有一个全局视口，"父层/子层各自的 Canvas"这个
//! 划分已经不存在：所有背景 Handler 都挂在根 StarMapCanvas 上，命中统一走递归
//! 入口 `hitTargetAtScreen()` → 根层内容的 `hitTargetAtScene()`，它会把真正命中的
//! 那一层作为 `owner` 返回。于是父层不再需要"自己判断是不是落在子内容区"——
//! 命中结果里 `kind === "childContent"` 已经区分了"落在子星图区域但子层还没加载"
//! 与"落在本层空白"，而落在子层内部的真实命中会直接带着子层的 owner 返回。
//!
//! 评论 5946795049 那轮引入的 press-time 手指归属（`touchOwnerA/B`、
//! `_pinchBelongsToChild`）已被 #822 的单一全局视口整体删除，对应守卫文件
//! `issue814_comment5946795049_press_time_gesture_ownership.rs` 也已随之删除。
//! 本文件只保留 tap 侧的"父层不吞子层"语义，改写为对递归命中入口的断言。

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "common/source_guard.rs"]
mod source_guard;

use source_guard::{function_window, read_src};

const CANVAS: &str = "qml/StarMapCanvas.qml";
const CONTENT: &str = "qml/StarMapSceneContent.qml";

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
// 1. 递归命中入口把子星图内容区单独区分成 childContent，不冒充 empty
// ─────────────────────────────────────────────────────────────────────────

/// Issue #832：落在子星图内容区时不再产生 childContent。
/// - 子层已经是 interactive：递归进子层，子层空白由子层自己的 empty 回答；
/// - 子层还是 shell / preview / 未加载：整个子星图按父层 embed 入口命中，
///   右键/单击/双击都作用于它，不留"什么都不做"的死区。
#[test]
fn recursive_hit_test_recurses_interactive_child_and_falls_back_to_embed() {
    let src = strip_line_comments(&read_src(CONTENT));
    let hit = function_window(&src, "function hitTargetAtScene(", 3600);
    assert!(
        !hit.contains("kind: \"childContent\""),
        "不得再返回 childContent：低 LOD 子图整体按父层 embed 命中，实际窗口:\n{hit}"
    );
    assert!(
        hit.contains("if (child && child.renderDetail === \"interactive\")"),
        "只有 interactive 子层才递归进子层命中，实际窗口:\n{hit}"
    );
    assert!(
        hit.contains("var deeper = child.hitTargetAtScene(sceneX, sceneY)"),
        "interactive 子层必须递归返回真正命中的那一层，实际窗口:\n{hit}"
    );
    // embed 兜底必须排在 empty 之前：empty 只表示任何一层都没命中。
    let empty_at = hit
        .find("kind: \"empty\"")
        .expect("hitTargetAtScene 必须有 empty 兜底");
    let embed_at = hit
        .find("targetPath: embedPath(inside.instanceId)")
        .expect("hitTargetAtScene 必须有子星图 embed 兜底");
    assert!(
        embed_at < empty_at,
        "子星图 embed 兜底必须排在 empty 之前判定，实际窗口:\n{hit}"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 2. 四个背景 tap Handler 一律走递归命中入口
// ─────────────────────────────────────────────────────────────────────────

/// 鼠标左键单击：唯一 Router 走递归命中后按 kind 分发；`empty` 时清的是
/// **命中层**的选区（hit.owner），不是根层一刀切。
#[test]
fn mouse_left_tap_uses_recursive_hit() {
    let src = strip_line_comments(&read_src("qml/StarMapInputRouter.qml"));
    let window = function_window(&src, "function selectHit(", 1000);
    for kind in ["\"node\"", "\"embed\"", "\"edge\"", "\"empty\""] {
        assert!(
            window.contains(&format!("hit.kind === {kind}")),
            "selectHit 必须按命中种类分发 {kind}，实际窗口:\n{window}"
        );
    }
    assert!(
        window.contains("hit.owner.clearLayerSelection()")
            && window
                .contains("hit.owner.logInteraction(\"selection_changed\", \"none\", \"\", {})"),
        "空白清选区必须清命中层自己的选区并记该层日志，实际窗口:\n{window}"
    );
    assert!(
        !window.contains("findEmbedContentAt") && !window.contains("findNodeAt"),
        "Router 不得自己做几何命中（统一走 hitTargetAtScreen），实际窗口:\n{window}"
    );
    let canvas = strip_line_comments(&read_src(CANVAS));
    assert!(
        !canvas.contains("findEmbedContentAt") && !canvas.contains("findNodeAt"),
        "Canvas 不得再保留第二份几何命中，实际源码不符"
    );
}

/// 触屏：单击同鼠标；长按 node/embed 进 contextPending（菜单视觉 / 转 connect），
/// 长按空白才弹该层背景菜单。
#[test]
fn touch_long_press_uses_recursive_hit_and_owner_layer() {
    let src = strip_line_comments(&read_src("qml/StarMapInputRouter.qml"));
    let press = function_window(&src, "function beginPress(", 2400);
    assert!(
        press.contains("hitAt(pressScreenX, pressScreenY)")
            && press.contains("emptyLongPressArmed = (source === \"touch\")"),
        "触屏按下必须走递归命中，空白长按预备只对触屏武装，实际窗口:\n{press}"
    );

    let long_press = function_window(&src, "function handleLongPress(", 1800);
    assert!(
        long_press.contains("ic.pressPendingToContextPending(center.x, center.y)")
            && long_press.contains("canvas.showTouchPreview(hit.kind, center.x, center.y)"),
        "触屏长按 node/embed 必须进 contextPending 并显示菜单视觉，实际窗口:\n{long_press}"
    );
    assert!(
        long_press.contains("if (h && h.kind === \"empty\")")
            && long_press.contains("canvas.openBlankMenu("),
        "长按空白才弹该层背景菜单（命中对象的菜单归从 contextPending 松手），实际窗口:\n{long_press}"
    );
}

/// 右键：Router 递归命中后交给 Canvas 菜单宿主按 kind 开对应菜单，
/// 空白走该层 openBlankMenu（子星图内部空白就在子星图里新建）。
#[test]
fn right_click_uses_recursive_hit_and_menu_host() {
    let router = strip_line_comments(&read_src("qml/StarMapInputRouter.qml"));
    let click = function_window(&router, "function handleRightClick(", 700);
    assert!(
        click.contains("hitAt(point.position.x, point.position.y)")
            && click.contains("canvas.openHitContextMenu(hit, point.position.x, point.position.y)"),
        "右键必须走递归命中后交给 Canvas 菜单宿主，实际窗口:\n{click}"
    );

    let canvas = strip_line_comments(&read_src(CANVAS));
    let dispatch = function_window(&canvas, "function openHitContextMenu(", 2400);
    for kind in ["node", "embed", "edge"] {
        assert!(
            dispatch.contains(&format!("hit.kind === \"{kind}\"")),
            "openHitContextMenu 必须按命中种类开 {kind} 菜单，实际窗口:\n{dispatch}"
        );
    }
    assert!(
        dispatch.contains("openBlankMenu(sx, sy, hit, screenX, screenY)"),
        "其余情况（命中层空白）走该层空白菜单，实际窗口:\n{dispatch}"
    );
    assert!(
        !dispatch.contains("findEmbedContentAt") && !dispatch.contains("findNodeAt"),
        "菜单宿主不得再做几何命中，实际窗口:\n{dispatch}"
    );
}

/// 空白菜单的归属层就是被点中的那一层——空的子星图可以直接新建内容，
/// 不需要"进入"另一个页面，也不需要第二个 Canvas。
#[test]
fn blank_menu_owner_is_the_hit_layer() {
    let src = strip_line_comments(&read_src(CANVAS));
    let open_blank = function_window(&src, "function openBlankMenu(", 900);
    assert!(
        open_blank.contains("menuOwnerContent = hit ? hit.owner : null"),
        "空白菜单的归属层必须是命中层 owner，实际窗口:\n{open_blank}"
    );
    assert!(
        open_blank.contains("hit.scenePathKey"),
        "空白菜单日志必须带命中层的 scenePathKey，实际窗口:\n{open_blank}"
    );
}
