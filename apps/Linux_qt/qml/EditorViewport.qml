// =============================================================================
// EditorViewport.qml - 写作编辑区视口层（唯一原生 SujianEditorItem 的承载）
// =============================================================================
//
// 职责：承载写作区视口 —— ScrollView(editorScroll) + SujianEditorItem(唯一原生
//       编辑器实例) + 编辑器上下文菜单 + 光标闪烁 Timer + 桌面滚轮直译器。
// 边界：只做视口布局和编辑器实例持有，不持有作品/章节/保存等业务状态；
//       业务委托 editorControllerRef，设置读取 settingsBackendRef。
//       对外只暴露只读别名 editorItem 和少量转发函数。
// 约束：全 QML 树只能有一个 SujianEditorItem 实例，本组件是它的唯一承载。
//       EditorContextMenu / DesktopWheelScrollHandler 是同目录本地组件。
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import Sujian 1.0

Item {
    id: root

    required property var dt
    required property var settingsBackendRef
    required property var editorControllerRef
    property var searchBackendRef: null

    property alias editorItem: sujianEditor
    readonly property bool editorEnabled: sujianEditor.editor_enabled
    property bool linkClickEligible: false
    property real linkPressX: 0
    property real linkPressY: 0
    property var pendingLinkChoices: []
    property string linkActivationMessage: ""
    // Issue #1047：章节阅读位置恢复。恢复期间光标/正文信号不得触发自动跟随滚动，
    // 否则新章节会先被拉到光标处，再把用户原来的阅读位置覆盖掉。
    property bool restoringViewState: false
    property var pendingRestoreViewState: null
    property int restoreScrollAttempts: 0
    property bool restoreCursorApplied: false
    // Issue #1047 复审：待恢复书签绑定的章节身份。WritingWorkspace 在载入
    // 成功后设置；定时器执行前必须复核，避免上一章的恢复落到下一章。
    property string viewIdentityProjectId: ""
    property string viewIdentityChapterId: ""
    property string pendingRestoreProjectId: ""
    property string pendingRestoreChapterId: ""
    // Issue #1047 复审：节流保存计时器启动时记录的章节身份。计时器可能跨章
    // 触发，触发前必须核对；通知随附身份，工作区不再用“当前 chapterId”猜来源。
    property string notifyViewProjectId: ""
    property string notifyViewChapterId: ""
    // 程序自身写 contentY 时置位，用于区分“恢复写入”和用户主动滚动。
    property bool applyingRestoreScroll: false

    signal explicitClearRequested()
    signal proofReplayRequested()
    // 节流后的视口变化通知：光标移动或用户滚动停止后发出，工作区据此落盘。
    // 身份随信号携带，接收方不得回读“当前章节”——计时器可能属于上一章。
    signal viewStateChanged(var state, string projectId, string chapterId)

    function requestEditorFocus() {
        if (sujianEditor && sujianEditor.visible && sujianEditor.editor_enabled) {
            sujianEditor.request_text_input_focus();
        }
    }
    function resetScroll() {
        // Issue #1047 复审：回顶部同时取消上一章尚未完成的延迟恢复，
        // 避免 A 章恢复在 B 章（无书签）打开后仍执行、把 A 的光标写进 B。
        cancelViewStateRestore();
        if (editorScroll.contentItem) {
            editorScroll.contentItem.contentY = 0;
        }
    }

    // Issue #1047：抓取当前章节的阅读位置（光标 char 索引 + 纵向滚动）。
    function captureViewState() {
        return {
            cursorCharIndex: Number(sujianEditor.cursor_position || 0),
            scrollY: editorScroll.contentItem
                     ? Number(editorScroll.contentItem.contentY || 0) : 0
        };
    }

    // Issue #1047 复审：保存时优先使用“待恢复的目标书签”，而不是恢复中途的
    // 临时 contentY/暂存光标；只有用户已主动滚动（恢复已被取消）或恢复已完成，
    // 才返回当前真实视口。切章保存、离开路由、关闭窗口都走这个入口。
    function captureEffectiveViewState() {
        if (restoringViewState
                && pendingRestoreViewState
                && pendingRestoreProjectId === viewIdentityProjectId
                && pendingRestoreChapterId === viewIdentityChapterId) {
            return {
                cursorCharIndex: Number(pendingRestoreViewState.cursorCharIndex || 0),
                scrollY: Number(pendingRestoreViewState.scrollY || 0)
            };
        }
        return captureViewState();
    }

    // Issue #1047：恢复章节阅读位置。先设光标，再把 contentY 落到
    // clamp(保存值, 0, contentHeight - height)；排版尚未稳定时由定时器重试。
    // state 为空表示该章节没有记录，按“首次进入”留在顶部。
    function restoreViewState(state) {
        restoreScrollTimer.stop();
        restoreSettleTimer.stop();
        if (!state || state.scrollY === undefined) {
            restoringViewState = false;
            pendingRestoreViewState = null;
            resetScroll();
            return;
        }
        restoringViewState = true;
        pendingRestoreViewState = state;
        pendingRestoreProjectId = viewIdentityProjectId;
        pendingRestoreChapterId = viewIdentityChapterId;
        restoreScrollAttempts = 0;
        restoreCursorApplied = false;
        // 光标与 contentY 都等新章节排版出高度后再落，避免用旧布局算 caret。
        restoreScrollTimer.start();
    }

    // 搜索定位、校对跳转等显式定位优先于书签：取消未完成的恢复，避免
    // 延迟的 settle 计时器把滚动位置拉回书签。
    function cancelViewStateRestore() {
        restoreScrollTimer.stop();
        restoreSettleTimer.stop();
        restoringViewState = false;
        pendingRestoreViewState = null;
        pendingRestoreProjectId = "";
        pendingRestoreChapterId = "";
    }

    function tryApplyRestoredScroll() {
        if (!restoringViewState || !pendingRestoreViewState)
            return false;
        // 章节身份已变化：上一章的延迟恢复不得写入下一章编辑器。
        if (pendingRestoreProjectId !== viewIdentityProjectId
                || pendingRestoreChapterId !== viewIdentityChapterId) {
            cancelViewStateRestore();
            return false;
        }
        var flick = editorScroll.contentItem;
        if (!flick || editorScroll.contentHeight <= 0)
            return false;
        if (!restoreCursorApplied) {
            var cursor = Math.max(0, Number(pendingRestoreViewState.cursorCharIndex || 0));
            sujianEditor.set_cursor_position_chars(cursor);
            restoreCursorApplied = true;
        }
        var maxY = Math.max(0, editorScroll.contentHeight - editorScroll.height);
        applyingRestoreScroll = true;
        flick.contentY = Math.max(0, Math.min(
            Number(pendingRestoreViewState.scrollY || 0), maxY));
        applyingRestoreScroll = false;
        return true;
    }

    function finishRestoreViewState() {
        restoreScrollTimer.stop();
        restoreSettleTimer.stop();
        pendingRestoreViewState = null;
        pendingRestoreProjectId = "";
        pendingRestoreChapterId = "";
        restoringViewState = false;
        restoreCursorApplied = false;
        // Issue #1047 复审：恢复完成后不要立刻自动跟随光标。用户离开时的阅读
        // 视口可能远离光标，这里若 ensureCursorVisible 会把视口拉回光标，覆盖
        // 刚恢复的 scrollY。自动跟随从下一次真实光标移动/输入开始生效。
    }

    function noteViewStateChanged() {
        if (restoringViewState)
            return;
        notifyViewProjectId = viewIdentityProjectId;
        notifyViewChapterId = viewIdentityChapterId;
        viewStateNotifyTimer.restart();
    }

    // Issue #1047 复审：切章/离开前停止旧章节尚未触发的节流计时器。它保存的
    // 是旧章身份，不能在新章打开后按“当前 chapter”落盘。切章前调用方已用
    // captureEffectiveViewState() 即时保存旧章，因此这里直接丢弃计时器即可。
    function flushOrCancelViewStateNotify() {
        viewStateNotifyTimer.stop();
        notifyViewProjectId = "";
        notifyViewChapterId = "";
    }

    // Issue #1047 复审：新章节正文载入成功后设置视口身份，并清掉旧计时器。
    function beginViewIdentity(projectId, chapterId) {
        flushOrCancelViewStateNotify();
        viewIdentityProjectId = String(projectId || "");
        viewIdentityChapterId = String(chapterId || "");
    }

    Timer {
        id: restoreScrollTimer
        interval: 16
        repeat: false
        onTriggered: {
            root.restoreScrollAttempts += 1;
            if (root.tryApplyRestoredScroll()) {
                // 排版高度可能还会再长一点（换行/图片行），再稳定一拍后收口。
                restoreSettleTimer.start();
            } else if (root.restoreScrollAttempts < 40) {
                restoreScrollTimer.restart();
            } else {
                root.finishRestoreViewState();
            }
        }
    }

    Timer {
        id: restoreSettleTimer
        interval: 120
        repeat: false
        onTriggered: {
            root.tryApplyRestoredScroll();
            root.finishRestoreViewState();
        }
    }

    // Issue #1047：光标移动/滚动后节流保存，切章时另由工作区立即保存快照。
    Timer {
        id: viewStateNotifyTimer
        interval: 700
        repeat: false
        onTriggered: {
            // 计时器启动后可能已经切章；旧章身份的通知一律丢弃，避免把上一章
            // 的延迟快照写到新章书签。
            if (!root.notifyViewProjectId || !root.notifyViewChapterId)
                return
            if (root.notifyViewProjectId !== root.viewIdentityProjectId
                    || root.notifyViewChapterId !== root.viewIdentityChapterId)
                return
            var projectId = root.notifyViewProjectId
            var chapterId = root.notifyViewChapterId
            root.notifyViewProjectId = ""
            root.notifyViewChapterId = ""
            // 恢复尚未完成时发出待恢复目标书签，而不是临时视口。
            root.viewStateChanged(root.captureEffectiveViewState(),
                                  projectId, chapterId)
        }
    }

    Connections {
        target: editorScroll.contentItem
        ignoreUnknownSignals: true
        function onContentYChanged() {
            if (root.applyingRestoreScroll)
                return;
            if (root.restoringViewState) {
                // 恢复期间用户主动滚动：放弃书签恢复，之后按用户新位置保存。
                // 排版变化引起的自动 clamp 不算用户滚动，继续等待恢复完成。
                // Issue #1047 复审：这里是 `Connections` 的处理器，`contentItem`
                // 不是本地变量也不能靠 target 注入作用域；统一经 `editorScroll`
                // 读取。`editorIsScrolling` 已包含 contentItem.moving/flicking、
                // 滚动条与桌面滚轮状态。
                var userScrolling = editorScroll.editorIsScrolling
                        || (editorScroll.contentItem
                            && (editorScroll.contentItem.moving
                                || editorScroll.contentItem.flicking));
                if (!userScrolling)
                    return;
                root.cancelViewStateRestore();
            }
            root.noteViewStateChanged();
        }
    }

    Connections {
        target: editorScroll
        ignoreUnknownSignals: true
        function onContentHeightChanged() {
            if (root.restoringViewState && root.pendingRestoreViewState)
                Qt.callLater(root.tryApplyRestoredScroll);
        }
    }
    function snapNextCursorUpdate() {
        sujianEditor.snap_next_cursor_update();
    }
    // Issue #964: one-shot reveal for a proofreading jump. Center the caret once,
    // then leave the existing minimal scroll-follow logic in charge.
    function revealCurrentCaret() {
        requestEditorFocus();
        var flick = editorScroll.contentItem;
        if (flick) {
            var caretY = Number(sujianEditor.cursor_rect_y) + Number(flick.contentY);
            var targetY = caretY - editorScroll.height / 2
                    + Number(sujianEditor.cursor_rect_height) / 2;
            var maxY = Math.max(0, editorScroll.contentHeight - editorScroll.height);
            flick.contentY = Math.max(0, Math.min(maxY, targetY));
        }
        editorScroll.scheduleEnsureCursorVisible();
    }
    function undo() {
        sujianEditor.undo();
    }
    function redo() {
        sujianEditor.redo();
    }

    ScrollView {
        id: editorScroll
        // Issue #695 评论 5693346400: editorIsScrolling 把
        // desktopWheelHandler.active 算进去，这样直接修改 contentY
        // 时现有的滚动期间动画暂停逻辑仍然有效。
        readonly property bool editorIsScrolling: ScrollBar.vertical.active || desktopWheelHandler.active || (contentItem && ((contentItem.moving !== undefined && contentItem.moving) || (contentItem.flicking !== undefined && contentItem.flicking)))
        property bool editorAnimationSuppressed: false
        anchors.fill: parent
        anchors.margins: dt.sp20
        clip: true
        contentWidth: availableWidth
        contentHeight: Math.max(sujianEditor.content_height, editorCanvas.emptyContentMinimumHeight)

        function clampScroll() {
            if (contentItem) {
                var maxScroll = Math.max(0, contentHeight - height);
                // Only clamp if contentY exceeds the valid range.
                // Do NOT force contentY to 0 when contentHeight is still updating.
                if (maxScroll > 0 && contentItem.contentY > maxScroll) {
                    contentItem.contentY = maxScroll;
                }
            }
        }

        // Issue #724 评论 5751268664 缺口2: 光标自动跟随滚动。
        // 语义同 QPlainTextEdit::ensureCursorVisible()（centerOnScroll=false）：
        // 只滚刚好够让 caret 回到可视区，不每打一字就强制居中。
        // cursor_rect_y 是目标 caret 的 viewport 坐标（Rust 已减过 scroll_y），
        // 直接用它做最小滚动量。contentY 改后仍通过 scroll_y 绑定回 Rust，
        // Scene Graph 和 IME 继续使用同一滚动位置。
        // Issue #727 评论 5757225958 问题2: 删除对已删除 Rust 方法
        // set_auto_follow_anchor_with_target 的调用，也删除
        // visual_cursor_rect_y / visual_cursor_rect_height 这套只为
        // anchor 服务的旧接口。ensureCursorVisible() 只根据 viewport
        // cursor_rect_y 算 targetY，最后只做 flick.contentY = targetY。

        function ensureCursorVisible() {
            const flick = contentItem
            if (!flick)
                return

            const margin = Math.max(dt.sp12, sujianEditor.cursor_rect_height * 0.5)
            const top = sujianEditor.cursor_rect_y
            const bottom = top + sujianEditor.cursor_rect_height
            const visibleBottom = availableHeight - margin

            let nextY = flick.contentY
            if (top < margin) {
                nextY += top - margin
            } else if (bottom > visibleBottom) {
                nextY += bottom - visibleBottom
            } else {
                return
            }

            const maxY = Math.max(0, contentHeight - height)
            // Issue #727 评论 5757225958 问题2: 只算出完整目标 targetY，
            // 若与当前 contentY 差距 < 0.5 直接返回（无需滚动）。
            // 否则直接赋值 flick.contentY，不再调用已删除的
            // set_auto_follow_anchor_with_target。
            const targetY = Math.max(0, Math.min(maxY, nextY))
            if (Math.abs(targetY - flick.contentY) < 0.5)
                return
            flick.contentY = targetY
        }

        function scheduleEnsureCursorVisible() {
            Qt.callLater(ensureCursorVisible)
        }

        onContentHeightChanged: clampScroll()
        onHeightChanged: clampScroll()
        Component.onCompleted: {
            // Issue #695: Qt 6.9+ 建议桌面 Flickable 设 acceptedButtons: Qt.NoButton，
            // 鼠标按住拖动不当触屏甩动；触屏 flick 不受 mouse-button 限制。
            // 让 Qt 原生处理 wheel/scrollbar，ScrollView/Flickable 继续持有 contentY。
            if (contentItem) {
                contentItem.acceptedButtons = Qt.NoButton;
            }
        }
        onEditorIsScrollingChanged: {
            // Issue #724 评论 5752140048 问题 2: 滚动停止时恢复 false，
            // 触发 Rust set_is_scrolling(false) → resume_all()。
            // anchor 生命周期和 editorAnimationSuppressed 是两件事，
            // 不要为了删 anchor timer 顺手把滚动动画抑制状态恢复也删掉。
            editorAnimationSuppressed = editorIsScrolling
        }

        ScrollBar.horizontal.policy: ScrollBar.AlwaysOff
        ScrollBar.vertical: ScrollBar {
            policy: ScrollBar.AsNeeded
            parent: editorScroll
            anchors.top: editorScroll.top
            anchors.bottom: editorScroll.bottom
            anchors.right: editorScroll.right
        }

        Item {
            id: editorCanvas
            readonly property real emptyContentMinimumHeight: Math.max((root.settingsBackendRef ? root.settingsBackendRef.setting_font_size : 16) * 2.4 + dt.sp16 * 2, editorScroll.availableHeight)
            width: editorScroll.availableWidth
            height: editorScroll.availableHeight
            implicitHeight: Math.max(sujianEditor.content_height, emptyContentMinimumHeight)

            // NOTE: SujianEditorItem is a "viewport renderer" - it must be
            // a FIXED overlay on the viewport, NOT inside the Flickable contentItem.
            // The Flickable only holds a transparent spacer for scrollbar / contentHeight.
            // scroll_y is passed to the Rust renderer for viewport clipping.
        }
    }

    // SujianEditorItem: viewport renderer - fixed overlay on the viewport,
    // NOT inside Flickable. scroll_y passes contentY to Rust renderer
    // for viewport clipping. Flickable only holds a transparent spacer
    // for scrollbar / contentHeight.
    //
    // 关于渲染（Issue #920）：Qt 本地布局断点只控制布局策略（最大宽度和 padding），
    // 不控制 SujianEditorItem 的渲染细节，渲染由 QML layout 自动处理
    //
    // 具体分工：
    //   - 本地布局常量（contentMaxWidthVp 等）通过视口宽度传递给
    //     SujianEditorItem 的 Q_PROPERTY
    //   - 编辑器参数（font_pixel_size, line_spacing, text_indent,
    //     cursor_color, scroll_y 等）由 settingsBackend 和 EditorController
    //     直接管理，不走布局断点
    //   - 编辑区尺寸跟随视口宽度和 QML layout 自动调整
    //     不需要 SujianEditorItem 的 geometry_changed 依赖布局断点
    //   - updatePaintNode / QSG 渲染完全由 Rust 侧管理
    // Issue #677 评论 5653315696 约束:
    //   - 写作区只拿 DesignTokens 已经算好的最终颜色（editorText、
    //     textPrimary、primary、selectedText 等），不在写作区里
    //     解释主题 JSON。主题 JSON 的 snake_case key 解析只在
    //     DesignTokens.qml 里完成。
    //   - text_color 最终来自 dt.editorText → dt.textPrimary →
    //     on_surface（Core DTO snake_case 字段）。
    //   - QML 的 loading/scrolling 状态（is_loading、is_scrolling）
    //     只用于动画抑制等，不能决定 Scene Graph 根节点和正文层是否
    //     存在。visible: true 固定不受 loading/scrolling 影响；
    //     update_paint_node 通过 ensure_editor_root 在第一帧创建根
    //     节点，不再因 root 为空整帧跳过。
    SujianEditorItem {
        id: sujianEditor
        // Issue #693 评论 5689819383: SujianEditorItem 是 ScrollView
        // 外的固定 overlay，editorScroll.clip 裁不到这个 sibling。
        // 自定义 Scene Graph 默认不裁剪，会画到 item 边界外盖住顶栏。
        // 打开 clip 把自身绘制和子节点限制在 bounding rect 内。
        // 参考 https://doc.qt.io/qt-6/qquickitem.html#clip-prop
        clip: true
        // Issue #833：SujianEditorItem 和 editorScroll 是兄弟项，
        // 直接 anchors.fill: editorScroll。不再复制 editorScroll.x/y/
        // availableWidth/availableHeight，避免 ScrollView 重排时出现
        // 一帧 x/y/availableWidth 已变、overlay 还没跟上的裁剪错位。
        anchors.fill: editorScroll
        visible: true
        focus: true
        // Issue #1069：切章/开章屏障期间（isLoadingChapter）编辑器不再接收输入，
        // 避免“旧章输入被接收但 QML 忽略、随后被新章正文覆盖”。
        editor_enabled: root.editorControllerRef.chapterId !== ""
            && !root.editorControllerRef.isLoadingChapter
        font_pixel_size: root.settingsBackendRef ? root.settingsBackendRef.setting_font_size : 16
        font_family: "serif"
        line_spacing: root.settingsBackendRef ? root.settingsBackendRef.setting_line_spacing : 1.5
        text_indent: (root.settingsBackendRef && root.settingsBackendRef.setting_auto_indent_enabled)
            ? Math.max(0, (root.settingsBackendRef.setting_font_size || 16) * root.settingsBackendRef.setting_auto_indent_width)
            : 0
        padding: dt.sp16
        // Issue #710 评论 5731145076: text_color 只绑定 dt.editorText，
        // 不再有 fallback 到 dt.textPrimaryHex 的第二套逻辑（已删除）。
        // editorText = textPrimary = onSurface（DesignTokens 单一事实源），
        // 主题变化只触发正文节点颜色重建，不重新创建另一份编辑器主题状态。
        // Issue #736 评论 5777408243 问题2: 直接绑定 DesignTokens 的 hex 字符串属性，
        // 不再经 QML color → toString() 转换，避免 QString → QML color → toString() → QString 绕一圈。
        // 写作区和普通 QML 控件共用同一份最终 color token。
        text_color: dt.editorTextHex
        selection_color: dt.primaryHex
        selected_text_color: dt.selectedTextHex
        cursor_color: dt.primaryHex
        smooth_cursor_enabled: root.settingsBackendRef ? root.settingsBackendRef.setting_smooth_cursor_enabled : true
        // Issue #785 / Issue #826 评论 38：纯光标移动（点击/方向键）
        // 的 duration 始终独立，用 smooth cursor duration；
        // 协同模式的正文编辑 caret 不走这个时长，跟着 typing
        // duration 跑（与前沿同一时钟），两者不再是两条独立时间线。
        cursor_animation_duration_ms: root.settingsBackendRef ? root.settingsBackendRef.setting_smooth_cursor_duration_ms : 80
        typing_animation_enabled: root.settingsBackendRef ? root.settingsBackendRef.setting_typing_animation_enabled : true
        typing_animation_duration_ms: root.settingsBackendRef ? root.settingsBackendRef.setting_typing_animation_duration_ms : 100
        // Issue #853：正文过渡从最近成功绘制的 VisualFrame
        // 更新到最新 canonical layout；光标由 CursorController
        // 独立管理。点击移动光标不会结束正在播放的正文过渡。
        coordinated_animation_enabled: root.settingsBackendRef ? root.settingsBackendRef.setting_coordinated_text_cursor_animation_enabled : true
        scroll_y: editorScroll.contentItem ? editorScroll.contentItem.contentY : 0
        viewport_height: sujianEditor.height
        is_scrolling: editorScroll.editorAnimationSuppressed
        is_loading: root.editorControllerRef.isLoadingChapter
        is_applying_format: root.editorControllerRef.isApplyingFormat
        // Issue #721: 不再把保存 guard (settingsSaveGuardActive) 传成
        // 编辑器视觉抑制状态。主题切换不再触发 applyCurrentSettings()，
        // 正文/选区/光标颜色已通过 text_color/selection_color/
        // selected_text_color/cursor_color 绑定自动下发，Rust 侧各自
        // 走 color_only_changed() -> request_scene_rebuild()，不会清空动画。

        // Issue #693 评论 5689819383: 光标自动跟随滚动。
        // 只在真正的编辑/selection 变化时调度，不监听 scroll_y 或
        // 每次 cursor_rect_changed，避免用户手动滚离光标被立刻拽回。
        // Rust 侧先发信号再 update_cursor_visual_position()，故用
        // Qt.callLater() 等本轮 caret target 算完再读 cursor_rect_y。
        // 同一轮即使排了两次 callLater，第二次看到 caret 已可见会直接返回。
        onCursor_position_changed: {
            // Issue #1047：恢复阅读位置期间不跟随光标，避免把 contentY 拉走。
            if (!root.restoringViewState)
                editorScroll.scheduleEnsureCursorVisible()
            root.noteViewStateChanged()
        }
        onText_changed: {
            if (!root.restoringViewState)
                editorScroll.scheduleEnsureCursorVisible()
        }

        onWidthChanged: {
            Qt.callLater(sujianEditor.flush_content_height)
        }

        Component.onCompleted: {
            sujianEditor.verify_animation_signal_meta_object()
        }

        onExplicit_clear_requested: root.explicitClearRequested()

        onContext_menu_requested: function(cx, cy) {
            sujianEditor.prepare_context_menu_at(cx, cy)
            // 将局部坐标映射为全局坐标后弹出菜单
            var globalPos = sujianEditor.mapToGlobal(cx, cy)
            editorContextMenu.popup(globalPos.x, globalPos.y)
        }

        TapHandler {
            acceptedButtons: Qt.RightButton
            onTapped: function(eventPoint) {
                sujianEditor.prepare_context_menu_at(eventPoint.position.x, eventPoint.position.y)
                sujianEditor.record_pointer_activity(
                    "context_menu",
                    eventPoint.position.x,
                    eventPoint.position.y,
                    0)
                editorContextMenu.popup()
            }
        }

        // Left-button events have one owner here. Rust only receives
        // normalized press/move/release/cancel calls from this area.
        MouseArea {
            id: editorPointerArea
            anchors.fill: parent
            acceptedButtons: Qt.LeftButton
            preventStealing: true
            hoverEnabled: true
            pressAndHoldInterval: 800

            onEntered: {
                sujianEditor.record_pointer_activity(
                    "enter", mouseX, mouseY, pressed ? Qt.LeftButton : 0)
            }
            onExited: {
                sujianEditor.record_pointer_activity(
                    "leave", mouseX, mouseY, pressed ? Qt.LeftButton : 0)
            }
            onPositionChanged: function(mouse) {
                sujianEditor.record_pointer_activity(
                    "move", mouse.x, mouse.y, mouse.buttons)
                if (pressed) {
                    var dx = mouse.x - root.linkPressX
                    var dy = mouse.y - root.linkPressY
                    if (dx * dx + dy * dy >= 9)
                        root.linkClickEligible = false
                    sujianEditor.pointer_move(mouse.x, mouse.y)
                }
            }
            onPressed: function(mouse) {
                root.linkPressX = mouse.x
                root.linkPressY = mouse.y
                root.linkClickEligible = true
                sujianEditor.record_pointer_activity(
                    "press", mouse.x, mouse.y, mouse.buttons)
                sujianEditor.pointer_press(mouse.x, mouse.y)
                sujianEditor.request_text_input_focus()
            }
            onReleased: function(mouse) {
                var activateLink = root.linkClickEligible
                var hitByte = Number(sujianEditor.selection_start_byte)
                root.linkClickEligible = false
                sujianEditor.record_pointer_activity(
                    "release", mouse.x, mouse.y, mouse.buttons)
                sujianEditor.pointer_release()
                if (activateLink && root.editorControllerRef
                        && root.editorControllerRef.activateAnnotationLinkAtByte)
                    root.editorControllerRef.activateAnnotationLinkAtByte(hitByte)
            }
            onCanceled: {
                root.linkClickEligible = false
                sujianEditor.record_pointer_activity(
                    "cancel", mouseX, mouseY, 0)
                sujianEditor.pointer_cancel()
            }
            onPressAndHold: function(mouse) {
                root.linkClickEligible = false
                sujianEditor.record_pointer_activity(
                    "long_press", mouse.x, mouse.y, mouse.buttons)
                sujianEditor.activate_pointer_long_press(mouse.x, mouse.y)
            }
        }

        // Cursor is now rendered in SujianEditorItem Scene Graph (child[3])
    }

    // 编辑器上下文菜单
    EditorContextMenu {
        id: editorContextMenu
        editorItem: sujianEditor
        editorControllerRef: root.editorControllerRef
        annotationBackendRef: root.editorControllerRef.annotationBackendRef
        searchBackendRef: root.searchBackendRef
        dt: root.dt
        onProofReplayRequested: root.proofReplayRequested()
    }

    Connections {
        target: root.editorControllerRef
        function onAnnotationLinkChoicesRequested(choices) {
            root.pendingLinkChoices = choices || []
            linkChoiceDialog.open()
        }
        function onAnnotationLinkActivationFailed(message) {
            root.linkActivationMessage = String(message || qsTr("无法打开链接目标"))
            linkActivationErrorDialog.open()
        }
    }

    Dialog {
        id: linkChoiceDialog
        title: qsTr("选择链接目标")
        modal: true
        anchors.centerIn: parent
        width: Math.min(440, root.width - 32)
        standardButtons: Dialog.Cancel
        contentItem: ColumnLayout {
            Label {
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                text: qsTr("此处有多个链接目标，请选择一个可用目标。")
            }
            Repeater {
                model: root.pendingLinkChoices
                delegate: Button {
                    required property var modelData
                    Layout.fillWidth: true
                    enabled: modelData.state === "exists"
                    text: {
                        var suffix = modelData.state === "projectNotDownloaded" ? qsTr("（目标作品未下载）")
                            : modelData.state === "missing" ? qsTr("（目标已删除）")
                            : modelData.state === "unavailable" ? qsTr("（无法解析）") : ""
                        return String(modelData.label || qsTr("链接目标")) + suffix
                    }
                    onClicked: {
                        root.editorControllerRef.activateAnnotationLinkChoice(modelData)
                        linkChoiceDialog.close()
                    }
                }
            }
        }
    }

    Dialog {
        id: linkActivationErrorDialog
        title: qsTr("无法打开链接")
        modal: true
        anchors.centerIn: parent
        width: Math.min(380, root.width - 32)
        standardButtons: Dialog.Ok
        contentItem: Label {
            width: parent.width
            wrapMode: Text.Wrap
            text: root.linkActivationMessage
        }
    }

    // Issue #690 评论 5675007226 步骤 4: 光标闪烁改用低频 Timer，
    // 不再用 FrameAnimation 每帧回调。正文位置动画已由 RenderPlan 同帧计算，
    // 不再需要 QML 每帧推进。空闲闪烁只需要低频定时触发 blink，
    // 并且每次切换显式 request_frame_update()。
    Timer {
        id: cursorBlinkTimer
        interval: 265
        repeat: true
        running: sujianEditor.editor_enabled
                 && sujianEditor.focus
        onTriggered: {
            sujianEditor.tick_cursor_animation()
        }
    }

    // Animation overlay removed — text animation is now handled in
    // SujianEditorItem Scene Graph (child[1]) via ActiveVisualTransactionQueue

    // 文字动画唯一主路径：Rust Coordinator → Scene Graph (child[1])
    // 不再使用 QML overlay 路线

    // Issue #695 评论 5693346400: 桌面滚轮事件直译器
    // 用 WheelHandler 拦截 wheel 事件并直接修改 contentY，阻止
    // Qt 6.10 QQuickFlickable::wheelEvent() 自带的 wheel acceleration。
    // 放在 editorScroll/sujianEditor 之后声明（z 更高），wheel 事件
    // 先到本组件；不拦截鼠标点击/拖拽（Item 默认不处理鼠标事件）。
    DesktopWheelScrollHandler {
        id: desktopWheelHandler
        targetFlickable: editorScroll.contentItem
        // 每格滚动距离 = wheelScrollLines × 当前行高
        // 行高 = 字体大小 × 行距倍数
        lineSpacingPx: (root.settingsBackendRef ? root.settingsBackendRef.setting_font_size : 16) * (root.settingsBackendRef ? root.settingsBackendRef.setting_line_spacing : 1.5)
        anchors.fill: editorScroll
    }
}
