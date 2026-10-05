//! Issue #832 评论 5998365709 — Linux_Qt 星图单一输入路由 + 显式根/嵌套身份守卫。
//!
//! 诊断包显示：同一次手势被根 Canvas 的递归命中与 Node/Embed 自带的
//! PointerHandler 同时参与（17 次 `embed_child_content_routed`、多次右键
//! `pointer_press`，但 `connect_begin=0` / `connect_end=0`），Qt 的
//! passive/exclusive grab 切换让 #373 的状态机失去唯一事实源；子星图的一级
//! 身份还靠扫描全部 graph 的 Embed 反推。
//!
//! 本文件锁住重构后的结构不变量，防止回退：
//!
//! 1. **输入只有一个主人**：`StarMapInputRouter.qml` 是整棵星图唯一的原始输入层
//!    （铺在根 Canvas 可视内容之上），Node/Embed/Content 不再挂任何业务手势。
//! 2. **命中只有一个入口**：`canvas.hitTargetAtScreen()` → 根 Content 的
//!    `hitTargetAtScene()`；子星图不再产生 `childContent`：interactive 子层递归，
//!    shell/preview/未加载整体按父层 `embed` 命中，右键/单击/双击都作用于入口。
//! 3. **状态提升只允许 Router**：pressPending → move / connect / contextPending /
//!    pan 只由 Router 调用；长按 Timer 在 Router 里；InteractionController 只保存
//!    状态并提供 isPressedTarget/isMovingTarget 给视觉绑定。
//! 4. **身份完整**：press / move / connect 都保存
//!    scenePathKey + kind + id + targetPath，不退化成裸 id。
//! 5. **菜单归属**：右键与空白新建都用 hit.owner 打开该层菜单，子星图内部空白
//!    就在子星图里新建。
//! 6. **根/嵌套身份显式持久化**：Core 索引新增 `root_starmap_ids`；
//!    普通新建写两个集合，嵌套 child 只写 starmap_ids；一级列表直接读索引，
//!    不再扫描 graph 反推；老索引第一次升级时用 Embed/portal 关系迁移一次。
//!
//! QML 组件依赖 qml_resources qrc 与 Rust 注册的上下文属性，无法在本仓库的
//! Rust 测试里实例化，因此按仓库既有惯例（issue801/issue814/issue817/issue822
//! 系列 WHITE_BOX 守卫）读取源码确定性断言；Router 行为另由本地
//! qmltestrunner（QtTest）验证鼠标/触屏/长按/滚轮路径。

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "common/source_guard.rs"]
mod source_guard;

use source_guard::{count_occurrences, function_window, linux_qt_root, read_src};

const ROUTER: &str = "qml/StarMapInputRouter.qml";
const CANVAS: &str = "qml/StarMapCanvas.qml";
const CONTENT: &str = "qml/StarMapSceneContent.qml";
const NODE: &str = "qml/StarMapNode.qml";
const EMBED: &str = "qml/StarMapEmbed.qml";
const INTERACTION: &str = "qml/StarMapInteractionController.qml";
const MAIN_RS: &str = "src/main.rs";
const BUILD_RS: &str = "build.rs";
const CORE_MOD: &str = "../../core/writer_core/src/starmap/mod.rs";
const CORE_MIGRATION: &str = "../../core/writer_core/src/starmap/migration.rs";
const CORE_FACADE: &str = "../../core/writer_core/src/facade/starmap_ops.rs";
const CORE_GRAPH: &str = "../../core/writer_core/src/facade/starmap_ops/graph.rs";
const CORE_DELETE: &str = "../../core/writer_core/src/storage/journal/starmap_delete.rs";

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

/// 取 `start`（含）到 `end`（不含）之间的源码片段；缺任一 marker 直接失败。
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

// ─────────────────────────────────────────────────────────────────────────
// 1. 输入只有一个主人：Router 是唯一原始输入层
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn router_file_exists_and_is_registered_in_qrc_and_build() {
    assert!(
        linux_qt_root().join(ROUTER).exists(),
        "Issue #832 要求新建 {ROUTER}：整棵星图唯一的原始输入层"
    );

    let main = strip_line_comments(&read_src(MAIN_RS));
    assert!(
        main.contains("qml/StarMapInputRouter.qml"),
        "src/main.rs 必须把 qml/StarMapInputRouter.qml 加进 qrc"
    );
    let build = strip_line_comments(&read_src(BUILD_RS));
    assert!(
        build.contains("qml/StarMapInputRouter.qml"),
        "build.rs 的 rerun-if-changed 必须包含 qml/StarMapInputRouter.qml"
    );
}

#[test]
fn canvas_has_no_raw_input_handlers_left() {
    let canvas = strip_line_comments(&read_src(CANVAS));
    for forbidden in [
        "TapHandler",
        "DragHandler",
        "PinchHandler",
        "WheelHandler",
        "PointHandler",
    ] {
        assert!(
            !canvas.contains(forbidden),
            "StarMapCanvas 不得再挂原始输入 {forbidden}：全部收进唯一 Router"
        );
    }
    // 画布手势全部让位；只保留 errorBanner 上的关闭 affordance（不是画布输入层）。
    assert_eq!(
        count_occurrences(&canvas, "MouseArea"),
        1,
        "Canvas 只允许 errorBanner 上的关闭 MouseArea"
    );
    let banner = slice_between(&canvas, "id: errorBanner", "id: touchContextPreview");
    assert!(
        banner.contains("MouseArea") && banner.contains("canvasArea.clearError()"),
        "唯一的 MouseArea 必须属于 errorBanner，实际片段:\n{banner}"
    );

    let router = strip_line_comments(&read_src(ROUTER));
    for required in [
        "TapHandler",
        "DragHandler",
        "PinchHandler",
        "WheelHandler",
        "PointHandler",
    ] {
        assert!(
            router.contains(required),
            "原始输入必须集中在 Router：缺 {required}"
        );
    }
}

#[test]
fn recursive_delegates_carry_no_business_gestures() {
    for rel in [NODE, EMBED, CONTENT] {
        let src = strip_line_comments(&read_src(rel));
        for forbidden in [
            "TapHandler",
            "DragHandler",
            "PinchHandler",
            "PointHandler",
            "MouseArea",
        ] {
            assert!(
                !src.contains(forbidden),
                "{rel} 不得再挂业务手势 {forbidden}：递归 delegate 不解释输入"
            );
        }
    }

    // Node/Embed 也不再上抛任何手势信号。
    for rel in [NODE, EMBED] {
        let src = strip_line_comments(&read_src(rel));
        for forbidden in [
            "itemPressed",
            "moveDelta",
            "leftReleased",
            "touchLongPressed",
            "mouseInteracted",
        ] {
            assert!(
                !src.contains(forbidden),
                "{rel} 不得再上抛 {forbidden}：手势信号已整体删除"
            );
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────
// 2. 命中只有一个入口，子星图不再有 childContent 死区
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn hit_testing_has_one_entry_with_embed_fallback() {
    let canvas = strip_line_comments(&read_src(CANVAS));
    let entry = function_window(&canvas, "function hitTargetAtScreen(", 400);
    assert!(
        entry.contains("rootContent.hitTargetAtScene(screenToWorldX(sx), screenToWorldY(sy))"),
        "Canvas 必须只有一个命中入口，从根内容开始递归，实际窗口:\n{entry}"
    );

    let content = strip_line_comments(&read_src(CONTENT));
    let hit = function_window(&content, "function hitTargetAtScene(", 3600);
    assert!(
        hub(&hit),
        "子星图内容区必须 interactive 递归 / 低 LOD 整体 embed，实际窗口:\n{hit}"
    );

    // 唯一输入主人也只有一个命中转发入口。
    let router = strip_line_comments(&read_src(ROUTER));
    assert_eq!(
        count_occurrences(&router, "function hitAt("),
        1,
        "Router 必须只有一个命中转发入口 hitAt"
    );
    assert_eq!(
        count_occurrences(&router, "hitTargetAtScreen("),
        1,
        "Router 只通过 hitAt() 调 canvas.hitTargetAtScreen"
    );
}

/// 子星图命中的两条分支：interactive 递归 + 低 LOD 父层 embed 兜底。
fn hub(hit: &str) -> bool {
    hit.contains("if (child && child.renderDetail === \"interactive\")")
        && hit.contains("var deeper = child.hitTargetAtScene(sceneX, sceneY)")
        && hit.contains("targetPath: embedPath(inside.instanceId)")
        && !hit.contains("kind: \"childContent\"")
}

// ─────────────────────────────────────────────────────────────────────────
// 3. 状态提升只允许 Router；长按 Timer 在 Router
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn state_promotion_only_happens_in_the_router() {
    let router = strip_line_comments(&read_src(ROUTER));
    for call in [
        "ic.pressPendingToMove(",
        "ic.pressPendingToConnect(",
        "ic.pressPendingToContextPending(",
        "ic.pressPendingToPan(",
        "ic.contextPendingToConnect(",
        "ic.beginPan()",
    ] {
        assert!(router.contains(call), "状态提升必须在 Router：缺 {call}");
    }
    for rel in [CONTENT, NODE, EMBED] {
        let src = strip_line_comments(&read_src(rel));
        for forbidden in [
            "pressPendingToMove(",
            "pressPendingToConnect(",
            "pressPendingToContextPending(",
            "pressPendingToPan(",
            "contextPendingToConnect(",
        ] {
            assert!(
                !src.contains(forbidden),
                "{rel} 不得自行提升手势状态：{forbidden} 只允许 Router 调用"
            );
        }
    }

    // 长按 Timer 与超时提升都在 Router；InteractionController 不再发 pressTimeout。
    let timer = function_window(&router, "Timer {", 400);
    assert!(
        timer.contains("onTriggered: router.handleLongPress()"),
        "长按 Timer 必须由 Router 触发状态提升，实际窗口:\n{timer}"
    );
    let controller = strip_line_comments(&read_src(INTERACTION));
    assert!(
        !controller.contains("pressTimeout"),
        "InteractionController 不得再保留 pressTimeout：超时归唯一 Router"
    );

    // 视觉绑定只能查询共享状态机。
    let node = strip_line_comments(&read_src(NODE));
    let embed = strip_line_comments(&read_src(EMBED));
    for (name, src) in [("StarMapNode", &node), ("StarMapEmbed", &embed)] {
        assert!(
            src.contains("interactionController.isPressedTarget(scenePathKey,"),
            "{name} 的按下视觉必须绑定共享 InteractionController.isPressedTarget"
        );
        assert!(
            !src.contains(".pressed"),
            "{name} 不得再依赖自己的 Handler.pressed 暂停动画"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────
// 4. press / move / connect 身份完整（scenePathKey + kind + id + targetPath）
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn press_move_connect_keep_full_identity() {
    let controller = strip_line_comments(&read_src(INTERACTION));
    for field in [
        "property string pressScenePathKey",
        "property string pressKind",
        "property string pressId",
        "property var pressTargetPath",
        "property string connectFromScenePathKey",
        "property string connectFromKind",
        "property string connectFromId",
        "property var connectFromPath",
        "property string moveScenePathKey",
        "property string moveKind",
        "property string moveId",
        "property var moveTargetPath",
    ] {
        assert!(
            controller.contains(field),
            "press/move/connect 身份必须完整：缺 {field}"
        );
    }
    assert!(
        controller.contains("function isPressedTarget(")
            && controller.contains("function isMovingTarget("),
        "共享状态机必须提供 isPressedTarget / isMovingTarget 供视觉绑定"
    );

    // Router 的按下登记必须带上完整 targetPath。
    let router = strip_line_comments(&read_src(ROUTER));
    let begin = function_window(&router, "function beginPress(", 1200);
    assert!(
        begin.contains("ic.beginPress(hit.kind, hit.id, hit.targetPath, hit.scenePathKey,"),
        "按下登记必须带完整 targetPath + scenePathKey，实际窗口:\n{begin}"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 5. 菜单归属：右键/空白新建都作用于命中层
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn context_menus_use_the_hit_layer_owner() {
    let canvas = strip_line_comments(&read_src(CANVAS));
    let dispatch = function_window(&canvas, "function openHitContextMenu(", 2400);
    for kind in ["node", "embed", "edge"] {
        assert!(
            dispatch.contains(&format!("hit.kind === \"{kind}\"")),
            "openHitContextMenu 必须按命中种类开 {kind} 菜单，实际窗口:\n{dispatch}"
        );
    }
    assert!(
        dispatch.contains("hit.owner.selectNode(hit.id)")
            && dispatch.contains("hit.owner.selectEmbed(hit.id)")
            && dispatch.contains("hit.owner.selectEdge(hit.id)"),
        "菜单选中必须作用于 hit.owner（命中层），实际窗口:\n{dispatch}"
    );
    assert!(
        dispatch.contains("openBlankMenu(sx, sy, hit, screenX, screenY)"),
        "空白走该层 openBlankMenu，实际窗口:\n{dispatch}"
    );
    let blank = function_window(&canvas, "function openBlankMenu(", 700);
    assert!(
        blank.contains("menuOwnerContent = hit ? hit.owner : null"),
        "空白菜单归属层必须是命中层 owner，实际窗口:\n{blank}"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 6. 内联编辑：Router 在编辑期间整体让位
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn inline_editing_gets_the_router_out_of_the_way() {
    let router = strip_line_comments(&read_src(ROUTER));
    assert!(
        router.contains("readonly property bool editingBlocks:")
            && router.contains("canvas.inlineEditingKey !== \"\""),
        "Router 必须根据内联编辑汇总键决定是否让位"
    );
    // disabled 的 Handler 不参与事件投递，因此编辑期间必须把 4 个单指
    // Handler（鼠标/触屏 tap、drag、release 观察）整体禁用，
    // TextInput 才能收到点击/拖动放置光标和选词。
    assert_eq!(
        count_occurrences(&router, "enabled: !router.editingBlocks")
            + count_occurrences(&router, "!router.pinchActive && !router.editingBlocks"),
        4,
        "编辑期间 Router 的 4 个单指 Handler 必须整体让位给 TextInput"
    );

    let canvas = strip_line_comments(&read_src(CANVAS));
    assert!(
        canvas.contains("property string inlineEditingKey: \"\"")
            && canvas.contains("function setInlineEditingKey(key)"),
        "Canvas 必须汇总内联编辑键"
    );
    let content = strip_line_comments(&read_src(CONTENT));
    assert!(
        content.contains("function noteInlineEditing(nodeId, editing)")
            && content.contains("menuHost.setInlineEditingKey(key)"),
        "Content 必须把本层编辑状态汇总到菜单宿主，实际源码缺少"
    );
    assert!(
        content.contains("onItemRemoved: function(index, item)")
            && content.contains("content.noteInlineEditing(item.nodeId, false)"),
        "编辑中的 delegate 被销毁时必须清掉内联编辑汇总键，否则 Router 会永久让位"
    );
    let node = strip_line_comments(&read_src(NODE));
    assert!(
        node.contains("TextInput {") && node.contains("readOnly: !root.editing"),
        "节点内联编辑的 TextInput 必须保留"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 7. Core：根/嵌套身份显式持久化
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn index_record_persists_explicit_root_starmap_ids() {
    let src = strip_line_comments(&read_src(CORE_MOD));
    assert!(
        src.contains("pub root_starmap_ids: Vec<String>"),
        "StarMapIndexRecord 必须持久化 root_starmap_ids"
    );
    let roots = function_window(&src, "pub fn list_root_starmaps(", 500);
    assert!(
        roots.contains("idx.root_starmap_ids") && roots.contains("load_starmap_meta("),
        "list_root_starmaps 必须直接读索引里的显式 root id，实际窗口:\n{roots}"
    );
    assert!(
        !roots.contains("filter_root_starmaps") && !roots.contains("StarMapStore"),
        "正常运行列一级列表不得再扫描 graph 反推身份，实际窗口:\n{roots}"
    );

    let create = function_window(&src, "pub fn create_starmap_with_id(", 400);
    assert!(
        create.contains("true,"),
        "普通新建星图必须走 root 登记路径，实际窗口:\n{create}"
    );
    let nested = function_window(&src, "pub fn create_nested_starmap_with_id(", 400);
    assert!(
        nested.contains("false,"),
        "嵌套子星图必须走 nested 创建路径（不写 root），实际窗口:\n{nested}"
    );
    let entry = function_window(&src, "fn create_starmap_entry(", 1200);
    assert!(
        entry.contains("if is_root && !idx.root_starmap_ids.iter().any(")
            && entry.contains("idx.root_starmap_ids.push(meta.starmap_id.clone())"),
        "只有 is_root 的创建路径才写 root_starmap_ids，实际窗口:\n{entry}"
    );
}

#[test]
fn child_embed_creation_uses_nested_entry_and_delete_clears_both_sets() {
    let graph = strip_line_comments(&read_src(CORE_GRAPH));
    let create = function_window(&graph, "pub fn create_starmap_child_embed(", 9000);
    assert!(
        create.contains("crate::starmap::create_nested_starmap_with_id("),
        "create_starmap_child_embed 必须走 nested entry 创建路径，\
         不能先登记成 root 再靠 Embed 关系过滤，实际窗口:\n{create}"
    );

    let delete = strip_line_comments(&read_src(CORE_DELETE));
    let apply = function_window(&delete, "fn apply_starmap_delete_internal(", 3000);
    assert!(
        apply.contains("idx.starmap_ids.retain(|id| id != starmap_id)")
            && apply.contains("idx.root_starmap_ids.retain(|id| id != starmap_id)"),
        "删除星图必须两个集合都移除，实际窗口:\n{apply}"
    );
}

#[test]
fn index_migration_derives_roots_once_from_graph_relations() {
    let src = strip_line_comments(&read_src(CORE_MIGRATION));
    assert!(
        src.contains("pub(crate) const NEW_INDEX_SCHEMA_VERSION: u32 = 3"),
        "index schema 必须升级到 3（引入显式 root 集合）"
    );
    let migrate = function_window(&src, "pub fn migrate_index(", 3000);
    assert!(
        migrate.contains("derive_root_starmap_ids(app_data_root, &record.starmap_ids)"),
        "老索引第一次升级必须用 Embed/legacy portal 关系迁移出 root 集合，实际窗口:\n{migrate}"
    );
    let derive = function_window(&src, "fn derive_root_starmap_ids(", 1800);
    assert!(
        derive.contains("super::filter_root_starmaps("),
        "迁移必须复用同一份 root 推导规则，实际窗口:\n{derive}"
    );
    assert!(
        derive.contains("store.load_full()"),
        "迁移必须基于完整 graph 关系，实际窗口:\n{derive}"
    );
}

#[test]
fn facade_root_list_reads_index_without_loading_stores() {
    let facade = strip_line_comments(&read_src(CORE_FACADE));
    let list = function_window(&facade, "pub fn list_root_starmaps(", 600);
    assert!(
        list.contains("crate::starmap::list_root_starmaps(&self.app_data_root)"),
        "facade 一级列表必须直接读索引里的显式 root，实际窗口:\n{list}"
    );
    assert!(
        !list.contains("filter_root_starmaps") && !list.contains("ensure_fully_loaded"),
        "一级列表不得为了根身份加载全部 Store，实际窗口:\n{list}"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 8. Linux UI：一级页仍然只调 list_root_starmaps_json，并在回到 Tab 时刷新
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn starmap_list_page_still_uses_root_list_api() {
    let controller = strip_line_comments(&read_src("qml/StarMapController.qml"));
    assert!(
        controller.contains("api.list_root_starmaps_json()"),
        "StarMapController 必须继续只调 list_root_starmaps_json()"
    );

    let page = strip_line_comments(&read_src("qml/StarMapPage.qml"));
    assert!(
        page.contains("function refreshStarmaps()")
            && page.contains("starMapController.listStarmaps()")
            && page.contains("onVisibleChanged: if (visible) refreshStarmaps()"),
        "一级页每次回到星图 Tab 必须重新取 root 列表，实际源码缺少"
    );
}
