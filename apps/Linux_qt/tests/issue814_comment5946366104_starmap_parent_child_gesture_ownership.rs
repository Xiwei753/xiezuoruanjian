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

/// `hitTargetAtScene` 必须把"落在子星图内容区、但子层还没加载出来"单独落成
/// `childContent`，不得冒充 `empty`——否则父层会把子星图内部的空白当成自己的
/// 空白，清掉子层刚建立的状态。
#[test]
fn recursive_hit_test_separates_child_content_from_empty() {
    let src = strip_line_comments(&read_src(CONTENT));
    let hit = function_window(&src, "function hitTargetAtScene(", 3200);
    assert!(
        hit.contains("kind: \"childContent\""),
        "hitTargetAtScene 必须把未加载的子星图内容区落成 childContent，实际窗口:\n{hit}"
    );
    // empty 只能是所有层都没命中之后的结果。
    let empty_at = hit
        .find("kind: \"empty\"")
        .expect("hitTargetAtScene 必须有 empty 兜底");
    let child_at = hit
        .find("kind: \"childContent\"")
        .expect("hitTargetAtScene 必须有 childContent 分支");
    assert!(
        child_at < empty_at,
        "childContent 必须排在 empty 之前判定：empty 只表示任何一层都没命中"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 2. 四个背景 tap Handler 一律走递归命中入口
// ─────────────────────────────────────────────────────────────────────────

/// 鼠标左键单击：走递归命中，`empty` 时清的是**命中层**的选区，
/// node / embed / childContent 都不在这里吞，交给 delegate。
#[test]
fn bg_mouse_left_tap_uses_recursive_hit() {
    let src = strip_line_comments(&read_src(CANVAS));
    let window = function_window(&src, "id: bgMouseLeftTap", 1500);
    assert!(
        window.contains("var hit = hitTargetAtScreen("),
        "bgMouseLeftTap 必须走递归命中入口 hitTargetAtScreen，实际窗口:\n{window}"
    );
    assert!(
        window.contains("hit.kind === \"edge\"") && window.contains("hit.kind === \"empty\""),
        "bgMouseLeftTap 只处理 edge 与 empty，其余交给 delegate，实际窗口:\n{window}"
    );
    assert!(
        window.contains("hit.owner.clearLayerSelection()"),
        "空白清选区必须清命中层自己的选区（hit.owner），不是根层一刀切，实际窗口:\n{window}"
    );
    assert!(
        !window.contains("findEmbedContentAt") && !window.contains("findNodeAt"),
        "背景 Handler 不得再自己做几何命中，实际窗口:\n{window}"
    );
}

/// 触屏左键单击同鼠标；长按只在空白处弹背景菜单，且菜单归属层是命中层。
#[test]
fn bg_touch_left_tap_uses_recursive_hit() {
    let src = strip_line_comments(&read_src(CANVAS));
    let window = function_window(&src, "id: bgTouchLeftTap", 3000);
    assert!(
        window.contains("var hit = hitTargetAtScreen("),
        "bgTouchLeftTap 必须走递归命中入口 hitTargetAtScreen，实际窗口:\n{window}"
    );
    assert!(
        window.contains("hit.owner.clearLayerSelection()"),
        "触屏空白清选区也必须清命中层自己的选区，实际窗口:\n{window}"
    );
    // 长按在命中对象上时不弹背景菜单。
    assert!(
        window.contains("if (!hit || hit.kind !== \"empty\")"),
        "触屏长按必须先递归判命中，命中对象的长按归 delegate，实际窗口:\n{window}"
    );
    assert!(
        window.contains("openBlankMenu("),
        "触屏长按落在空白处才弹背景菜单，实际窗口:\n{window}"
    );
}

/// 右键：递归命中后按 kind 开对应菜单，空白走 openBlankMenu。
#[test]
fn background_right_tap_uses_recursive_hit() {
    let src = strip_line_comments(&read_src(CANVAS));
    let window = function_window(&src, "id: backgroundRightTap", 2200);
    assert!(
        window.contains("var hit = hitTargetAtScreen("),
        "backgroundRightTap 必须走递归命中入口 hitTargetAtScreen，实际窗口:\n{window}"
    );
    for kind in ["node", "embed", "edge"] {
        assert!(
            window.contains(&format!("hit.kind === \"{kind}\"")),
            "backgroundRightTap 必须按命中种类开 {kind} 菜单，实际窗口:\n{window}"
        );
    }
    assert!(
        window.contains("openBlankMenu(sx, sy, hit, px, py)"),
        "其余情况（子星图内部空白）走空白菜单，实际窗口:\n{window}"
    );
    assert!(
        !window.contains("findEmbedContentAt") && !window.contains("findNodeAt"),
        "右键 Handler 不得再自己做几何命中，实际窗口:\n{window}"
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