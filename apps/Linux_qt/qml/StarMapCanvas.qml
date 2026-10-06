// =============================================================================
// StarMapCanvas.qml — 星图画布（整棵星图唯一的 viewport / camera）
// =============================================================================
//
// 层级：Linux_qt UI 层（QML UI 组件）
// 职责：全局相机（pan/zoom）数据与纯方法、唯一命中入口、根层内容容器、
//   菜单宿主与弹窗、connect 预览 overlay
//
// Issue #822：整棵星图只有一个全局视口。
//   panX / panY / zoomLevel 只在这里存在。鼠标停在任意深度的节点、子星图、
//   孙星图上，滚轮和捏合都调同一个 zoomAt()，只改根 zoomLevel/panX/panY。
//   子星图"看起来更大/更小"是 Deep Zoom 显示档位：每层内容做 local fit，
//   ownerEffectiveScale = globalZoom × 祖先 local fit，再用投影覆盖率决定
//   子内容是完整交互 / 轻量 preview / 只留外壳。档位绝不反写全局相机，
//   也不改 Embed 的 world 几何（见 StarMapEmbed / docs/starmap_viewport.md）。
//
//   递归的是"内容"不是"视口"：根层内容由 StarMapSceneContent 渲染，
//   子星图内容在 Embed 内部懒加载下一层 StarMapSceneContent。
//   节点/连线都画在各自层的局部坐标里，相机只作用于根层 Content 这一张 Item。
//
// Issue #832：原始输入只有一个主人 StarMapInputRouter。
//   本文件不再挂背景 TapHandler / DragHandler / PinchHandler / WheelHandler /
//   MouseArea，也不做手势仲裁；只保留相机数据与纯方法
//   （screen/world 换算、panBy()、zoomAt()、focusOnSceneRect）、
//   hitTargetAtScreen() 唯一命中入口、菜单宿主和 connect 预览 overlay。
//   交互状态机 StarMapInteractionController 仍由本文件创建一次，整棵递归树共享；
//   长按 Timer 与所有状态提升都在 Router。
//
// 约束：
//   - 纯渲染和输入层，星图业务逻辑委托给各层 StarMapGraphController
//   - 使用 Canvas 进行自定义绘制
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Item {
    id: canvasArea
    clip: true

    // 根星图 ID。根层内容用它作为整棵递归树的 rootStarmapId。
    property string starmapId: ""
    // Issue #832 评论 6014361379：切图时单独清焦点栈。不塞进 resetInteraction()，
    // 因为 pinch 开始也会调 resetInteraction()，不能一捏就清视觉焦点。
    onStarmapIdChanged: {
        if (focusStack.length > 0) {
            focusStack = []
            visualFocusChanged()
        }
    }
    required property var dt

    readonly property color _primary: dt.primary
    readonly property color _onPrimary: dt.onPrimary
    readonly property color _accent: dt.accent
    readonly property color _accentSoft: dt.accentSoft
    readonly property color _accentHover: dt.accentHover
    readonly property color _border: dt.border
    readonly property color _card: dt.card
    readonly property color _danger: dt.danger
    readonly property color _dangerContainer: dt.dangerContainer
    readonly property color _surfaceContainer: dt.surfaceContainer
    readonly property color _textPrimary: dt.textPrimary
    readonly property color _textSecondary: dt.textSecondary
    readonly property color _textMuted: dt.textMuted
    readonly property color _error: dt.error
    readonly property color _onError: dt.onError
    readonly property color _scrim: dt.scrim
    readonly property int _radiusXs: dt.radiusXs
    readonly property int _radiusSm: dt.radiusSm
    readonly property int _dialogRadius: dt.dialogRadius

    property var starmapBackendRef: null
    readonly property string errorMessage: rootContent ? rootContent.errorMessage : ""

    // ── 全局相机（唯一）──
    // Issue #814 评论 5935285879: 无限画布 — pan 两个方向都不设边界。
    // 改用"根层 Content 一张带 transform 的 Item + delegate 直接局部坐标"承载世界后，
    // 世界坐标可以是任意正负值，pan 不再需要 clamp。
    property real panX: 0
    property real panY: 0
    property real zoomLevel: 1.0

    // Issue #822 评论 5972215936: 相机范围只保留数值安全边界，不设产品硬天花板。
    // 与 docs/starmap_viewport.md 的 CAMERA_SCALE_MIN/MAX、ZOOM_FACTOR 同一口径：
    // 乘法步进保证各档手感一致，Deep Zoom 的 coverage 档位由用户能到的真实比例决定
    // （0.35~2.5 的旧硬上限会让 1080 高窗口里的子星图永远停在 preview）。
    readonly property real _cameraScaleMin: 1e-4
    readonly property real _cameraScaleMax: 1e5
    readonly property real _zoomFactor: 1.2

    function applyPan(nextX, nextY) {
        panX = nextX
        panY = nextY
    }

    // 唯一平移入口：Router 交给 Canvas 的原始位移换算成 canvas-local 增量后
    // 由这里落到相机上。pan 两个方向都不设边界（无限画布）。
    function panBy(dx, dy) {
        applyPan(panX + dx, panY + dy)
    }

    // scene 坐标（整棵递归树的顶层 world 坐标）↔ 视口坐标。
    // 相机只有这一个，所以换算是全局唯一入口，不再有"每层各自的 zoomLevel"。
    function worldToScreenX(wx) { return panX + wx * zoomLevel }
    function worldToScreenY(wy) { return panY + wy * zoomLevel }
    function screenToWorldX(sx) { return (sx - panX) / zoomLevel }
    function screenToWorldY(sy) { return (sy - panY) / zoomLevel }

    // 唯一缩放入口：滚轮、捏合、+/- 按钮都走这里，只改根 zoomLevel/panX/panY。
    // 只夹数值安全范围（CAMERA_SCALE_MIN/MAX），不再有产品缩放上限。
    function zoomAt(screenX, screenY, nextZoom) {
        var target = Math.max(_cameraScaleMin, Math.min(_cameraScaleMax, nextZoom))
        var oldZoom = zoomLevel
        if (target === oldZoom)
            return
        zoomLevel = target
        applyPan(
            screenX - (screenX - panX) * (zoomLevel / oldZoom),
            screenY - (screenY - panY) * (zoomLevel / oldZoom)
        )
        // Issue #832 评论 6013799805 / #373：缩放后重算焦点链覆盖率，
        // 栈顶 embed 缩出窗口就 demote（只 pop，不 push）。
        recomputeFocusFromCoverage()
    }

    // 双击 Embed 的"进入"：把相机聚焦/放大到该 Embed 的 scene 矩形。
    // 仍然是同一张全局画布，不开新页面、不切换星图身份；放大后 Deep Zoom
    // 档位自然从 shell/preview 跨到 interactive，子内容就地展开。
    function focusOnSceneRect(sceneX, sceneY, sceneW, sceneH) {
        var w = Math.max(sceneW, 1)
        var h = Math.max(sceneH, 1)
        var next = Math.min(width / w, height / h) * 0.72
        next = Math.max(_cameraScaleMin, Math.min(_cameraScaleMax, next))
        zoomLevel = next
        applyPan(width / 2 - (sceneX + sceneW / 2) * next,
                 height / 2 - (sceneY + sceneH / 2) * next)
    }

    // ---------------------------------------------------------------------------
    // Issue #832 评论 6013799805 / #373：纯视觉焦点链（返回父星图）
    // ---------------------------------------------------------------------------
    // 整棵星图仍然只有这一个全局相机；focusStack 只记录"双击进入过哪些 embed"，
    // 给左上返回按钮和缩放覆盖率滞回用。它不切换 root starmap、不落盘、
    // 不复制 Core 业务状态，纯粹是显示参考根。
    //   每个元素 = { scenePathKey, starmapId, embedInstanceId,
    //                sceneRect:{x,y,width,height}, targetPath }
    //   focusIsRoot === focusStack.length === 0：左上返回退出星图工作区；
    //   否则左上返回 pop 一层，相机回到新栈顶的 sceneRect（root 时不动）。
    // ---------------------------------------------------------------------------
    property var focusStack: []
    readonly property bool focusIsRoot: focusStack.length === 0
    // 栈顶的 scenePathKey（root 时为 "root"），供 Workspace/日志用。
    readonly property string focusScenePathKey:
        focusStack.length > 0 ? focusStack[focusStack.length - 1].scenePathKey : "root"
    signal visualFocusChanged()

    // 双击 embed 的"进入"= 相机聚焦 + 推进焦点链。
    // hit 是 hitTargetAtScreen 返回的完整命中（owner/scenePathKey/starmapId/
    // kind/id/targetPath）。scene 矩形由 hit.owner.itemSceneRect 给出（scene/world 坐标）。
    // Issue #832 评论 6014908211：
    // - 父子校验必须在 focusOnSceneRect 之前：失败时不移动相机、不进栈。
    // - hit.scenePathKey 是 embed 所属的 Scene；只有当它等于当前焦点
    //   scenePathKey 时，进入的 child 才是当前焦点的直接子层。
    // - 直接子层：push 一项，保证 stack 相邻两项永远是直接父子。
    // - 兄弟/祖先/深层：不改变焦点和相机。深层目标（hit.scenePathKey 是
    //   focusScenePathKey 的真后代）理论上应补齐中间父链，但中间层的
    //   sceneRect 不可得，贸然 push 会破坏 recomputeFocusFromCoverage 的
    //   覆盖率滞回；保持不动更安全。
    // - scenePathKey 记录进入后的 child scene path（hit.owner.enteredChildSceneKey），
    //   不是 hit.scenePathKey（那是 embed 所属父 Scene，焦点身份会慢一层）。
    // - 保存进入前的父层相机 parentCamera，pop 时恢复原视角。
    function focusEmbed(hit) {
        if (!hit || !hit.owner)
            return
        var rect = hit.owner.itemSceneRect(hit.kind, hit.id)
        if (!rect)
            return
        // 真正的 child scene path：hit.scenePathKey 是 embed 所属父 Scene，
        // 进入后的 child scene 要再往下钻一层。
        var childKey = hit.owner.enteredChildSceneKey(hit.id)
        // 父子校验在 focusOnSceneRect 之前：只有 embed 所属 Scene 等于当前
        // 焦点 scenePathKey 时，child 才是直接子层。否则不移动相机、不进栈。
        if (hit.scenePathKey !== focusScenePathKey)
            return
        // 校验通过：保存父层相机，移动相机，推进焦点链。
        var parentCamera = { zoom: zoomLevel, panX: panX, panY: panY }
        focusOnSceneRect(rect.x, rect.y, rect.width, rect.height)
        var entry = {
            scenePathKey: childKey,
            starmapId: hit.starmapId,
            embedInstanceId: hit.id,
            sceneRect: { x: rect.x, y: rect.y, width: rect.width, height: rect.height },
            targetPath: hit.targetPath,
            parentCamera: parentCamera
        }
        var next = focusStack.slice()
        next.push(entry)
        focusStack = next
        visualFocusChanged()
    }

    // 左上"返回父星图"：pop 最后一层。
    // - 栈空：已是 root，返回 false（Workspace 据此走 backClicked 退出星图工作区）。
    // - pop 后：用被 pop 的 entry 里保存的 parentCamera 恢复相机，
    //   而不是用新栈顶的 sceneRect 重新 focusOnSceneRect。这样每一级返回
    //   真的回到父层原视角（root 时恢复 root 的 parentCamera）。
    function focusParentScene() {
        if (focusStack.length === 0)
            return false
        var next = focusStack.slice()
        var popped = next.pop()
        focusStack = next
        if (popped.parentCamera) {
            zoomLevel = popped.parentCamera.zoom
            applyPan(popped.parentCamera.panX, popped.parentCamera.panY)
        }
        visualFocusChanged()
        return true
    }

    // 缩放覆盖率滞回：zoomAt 末尾调用。只做 demote（pop），不主动 push
    // （push 只由双击 focusEmbed 触发）。覆盖率口径与 Harmony resolveFocusScenePath
    // 一致：子星图投影直径 / 视口短边（不是裁剪交集面积 / 视口面积）。
    // focusOnSceneRect 把子星图放到短边约 72%，旧面积比 0.72*0.72=0.5184 < 0.55
    // 会在双击进入后下一次 zoomAt 立刻 pop；投影直径比 0.72 > 0.55 才稳定。
    // 圆心必须落在窗口 20%~80% 区间。退出阈值 0.55，进入阈值 0.70 不在这里 push。
    // 用 while 循环：一次缩小可能让多层同时掉出窗口。
    function recomputeFocusFromCoverage() {
        if (focusStack.length === 0)
            return
        var vpW = canvasArea.width
        var vpH = canvasArea.height
        if (!(vpW > 0) || !(vpH > 0))
            return
        var vpShort = Math.min(vpW, vpH)
        var changed = false
        while (focusStack.length > 0) {
            var top = focusStack[focusStack.length - 1]
            var r = top.sceneRect
            var sx0 = worldToScreenX(r.x)
            var sy0 = worldToScreenY(r.y)
            var sx1 = worldToScreenX(r.x + r.width)
            var sy1 = worldToScreenY(r.y + r.height)
            var projectedDiameter = Math.min(Math.abs(sx1 - sx0), Math.abs(sy1 - sy0))
            var coverage = projectedDiameter / vpShort
            var cx = worldToScreenX(r.x + r.width / 2)
            var cy = worldToScreenY(r.y + r.height / 2)
            var centerInWindow = cx >= 0.2 * vpW && cx <= 0.8 * vpW
                    && cy >= 0.2 * vpH && cy <= 0.8 * vpH
            if (coverage >= 0.55 && centerInWindow)
                break
            var next = focusStack.slice()
            next.pop()
            focusStack = next
            changed = true
        }
        if (changed)
            visualFocusChanged()
    }

    // Issue #834：connectArmed（菜单"连线"发起）时 Router 的 HoverHandler 调这里
    // 刷新预览。rootContent 是 Canvas 内部 id，Router 跨组件拿不到，由这里转发。
    function refreshConnectPreview() {
        if (rootContent)
            rootContent.refreshConnectPreview()
    }

    // 当前可见区域（scene 坐标矩形），逐层传给内容做子星图懒加载判定。
    readonly property var viewportSceneRect: ({
        x: screenToWorldX(0),
        y: screenToWorldY(0),
        width: width / (zoomLevel > 0 ? zoomLevel : 1),
        height: height / (zoomLevel > 0 ? zoomLevel : 1)
    })

    // DragHandler 的 activeTranslation 是 Qt scene 坐标增量。
    // 映射到本 Canvas local 再用；根层无祖先 scale 时退化成恒等。
    function sceneDeltaToCanvas(dx, dy) {
        var o = canvasArea.mapFromItem(null, 0, 0)
        var p = canvasArea.mapFromItem(null, dx, dy)
        return { x: p.x - o.x, y: p.y - o.y }
    }

    // Issue #801 评论 5894639734: +/- 触屏按钮按需显示，鼠标模式不常驻。
    // Router 在每次鼠标/触屏事件开头调用 notePointerDevice 切换。
    property bool _touchInputActive: false
    function notePointerDevice(isTouch) {
        _touchInputActive = isTouch
    }

    // Issue #832：内联编辑汇总键（由各层 Content 经 noteInlineEditing 汇总）。
    // 非空时 Router 的拖动 Handler 让位给编辑中的 TextInput。
    property string inlineEditingKey: ""
    function setInlineEditingKey(key) {
        inlineEditingKey = key
    }

    // Issue #814 评论 5935346839: 星图交互边界日志统一入口。
    // pathKey 显式传参：现在是全局相机，没有"哪一层是 root"的隐含身份判断。
    function logInteraction(event, itemKind, itemId, fields, scenePathKey) {
        if (!starmapBackendRef) return
        var fj = fields ? JSON.stringify(fields) : ""
        starmapBackendRef.record_interaction(event, scenePathKey === undefined ? "root" : scenePathKey,
                                              starmapId, itemKind, itemId, fj)
    }

    // Issue #822: 统一命中判断入口 —— 递归命中测试，从根层内容开始往下钻。
    // 空白点击、拖动画布、pointer_press、右键菜单全部共用（现在唯一调用方是 Router）。
    function hitTargetAtScreen(sx, sy) {
        if (!rootContent) return null
        return rootContent.hitTargetAtScene(screenToWorldX(sx), screenToWorldY(sy))
    }

    // Issue #814 评论 5935346839: pointer_press 是完整手势的起点边界。
    // Router 在按下当场调用：命中的对象照常拿到完整交互，
    // hitKind/hitId 由递归命中当场重算。
    function logPointerPress(button, device, point) {
        var hit = hitTargetAtScreen(point.position.x, point.position.y)
        var kind = hit ? hit.kind : "empty"
        var hitId = hit ? hit.id : ""
        var hitPathKey = hit ? hit.scenePathKey : "root"
        logInteraction("pointer_press", kind, hitId, {
            "button": button,
            "device": device,
            "screenX": point.position.x,
            "screenY": point.position.y,
            "sceneX": screenToWorldX(point.position.x),
            "sceneY": screenToWorldY(point.position.y),
            "panX": panX,
            "panY": panY,
            "zoomLevel": zoomLevel
        }, hitPathKey)
    }

    // ---------------------------------------------------------------------------
    // Issue #822：整棵递归树唯一的交互状态机，就在这里创建一次。
    // 各层 StarMapSceneContent 拿到的都是同一个实例（经 Embed 原样下传）。
    // Issue #832：长按 Timer 与所有状态提升都在 StarMapInputRouter，
    // 本文件只创建状态机实例。
    // ---------------------------------------------------------------------------
    StarMapInteractionController {
        id: interaction
        // 长按阈值用系统的 mousePressAndHoldInterval，和平台其它长按一致。
        longPressInterval: Application.styleHints.mousePressAndHoldInterval > 0
                ? Application.styleHints.mousePressAndHoldInterval
                : 800
    }

    readonly property var sharedInteraction: interaction

    // 菜单辅助状态：菜单归属层就是被点中的那一层。
    property var menuOwnerContent: null
    property var selectedNodeForMenu: null
    property var selectedEdgeForMenu: null
    property var selectedEmbedForMenu: null
    // 右键命中点的 scene 坐标，创建类操作由归属层换算成局部坐标。
    property real contextMenuSceneX: 0
    property real contextMenuSceneY: 0

    // 新建对话框状态：先收集名字再写 Core（Issue #793 评论 5884923277）
    property string createMode: ""      // "node" / "starmap"

    // Signals
    signal nodeSelected(var node)
    signal edgeSelected(var edge)
    signal selectionCleared()

    // Issue #805 评论 5907045450 第 1 部分：删掉 drillDownRequested / drillUpRequested。
    // 递归渲染由 StarMapSceneContent 处理，Canvas 不再上抛层级切换请求。
    // Issue #822: 删掉 editNodeRequested / childEditNodeRequested ——
    // 节点标题改成节点自身的内联编辑，不再经过外部 Popup。

    function clearError() {
        if (rootContent) rootContent.clearError()
    }

    // Issue #798: 公开 reset 入口，供 Workspace 切图 / 不可见时清瞬时交互状态。
    // Issue #832: 手势现场已经全部集中在 Router + 共享状态机，这里只需复位
    // 状态机并让整棵树的连线回到 canonical。内联编辑汇总键不在这里清：
    // 由 Node 的 editing 变化和 delegate 销毁（Repeater.itemRemoved）负责。
    function resetInteraction() {
        // Issue #798 评论 5892406254: reset 前若正在 move，edgeRenders 已被
        // transient 坐标更新。reset 后 delegate 回 canonical，edge cache 也要
        // 一起恢复 canonical，否则节点回去了线还停在拖动位置。
        interaction.reset()
        if (inputRouter) inputRouter.cancelLocalState()
        hideTouchPreview()
        if (rootContent) rootContent.refreshAllEdges()
    }

    // ---------------------------------------------------------------------------
    // 相机层 + 根层内容：整棵递归树的入口。
    // 相机平移/缩放只存在于 cameraLayer 这一张 Item 上（x/y/scale），
    // rootContent 只用 anchors.fill 占满相机层，绝不自己再拿 x/y 当平移 ——
    // 一个 Item 上不能同时有 anchors 和相机两套几何来源。
    // ---------------------------------------------------------------------------
    Item {
        id: cameraLayer
        x: canvasArea.panX
        y: canvasArea.panY
        width: canvasArea.width
        height: canvasArea.height
        scale: canvasArea.zoomLevel
        transformOrigin: Item.TopLeft

        StarMapSceneContent {
            id: rootContent
            anchors.fill: parent
            dt: canvasArea.dt
            rootStarmapId: canvasArea.starmapId
            pathSegments: []
            // Issue #822: 根层显式传 "root"，不再由默认值冒名顶替。
            scenePathKey: "root"
            starmapBackendRef: canvasArea.starmapBackendRef
            selectionController: canvasArea.selectionController
            interactionController: canvasArea.sharedInteraction
            globalZoom: canvasArea.zoomLevel
            depth: 0
            rootContent: rootContent
            rootViewportRect: canvasArea.viewportSceneRect
            menuHost: canvasArea

            onNodeSelected: function(node) { canvasArea.nodeSelected(node) }
            onEdgeSelected: function(edge) { canvasArea.edgeSelected(edge) }
            onSelectionCleared: canvasArea.selectionCleared()
        }
    }

    // Issue #822: 整棵递归树共享的选中状态控制器，由 Workspace 创建并传入。
    property var selectionController: null

    // Background Grid
    Rectangle {
        anchors.fill: parent
        color: "transparent"
        opacity: 0.3

        Canvas {
            id: gridCanvas
            anchors.fill: parent
            onPaint: {
                var ctx = getContext("2d")
                ctx.clearRect(0, 0, width, height)
                ctx.fillStyle = _textMuted

                // 网格自身做 LOD：相机放开到 1e-4 后，50 * zoomLevel 会掉到亚像素，
                // 双重循环按 1/zoom² 爆炸。按 5 倍档抬 world 间距，屏幕上实际画出来的
                // 间距永远 >= 16px，循环次数只跟屏幕尺寸有关。
                if (!(zoomLevel > 0))
                    return
                var worldSpacing = 50
                var gridSpacing = worldSpacing * zoomLevel
                while (gridSpacing < 16) {
                    worldSpacing *= 5
                    gridSpacing = worldSpacing * zoomLevel
                }
                var startX = (panX % gridSpacing)
                var startY = (panY % gridSpacing)

                for (var x = startX; x < width; x += gridSpacing) {
                    for (var y = startY; y < height; y += gridSpacing) {
                        ctx.fillRect(x - 1, y - 1, 2, 2)
                    }
                }
            }
            Connections {
                target: canvasArea
                function onPanXChanged() { gridCanvas.requestPaint() }
                function onPanYChanged() { gridCanvas.requestPaint() }
                function onZoomLevelChanged() { gridCanvas.requestPaint() }
            }
        }
    }

    // ---------------------------------------------------------------------------
    // Issue #832：整棵星图唯一的原始输入层，透明铺在整棵可视内容之上。
    // 所有鼠标/触摸语义（单击/双击/右键/拖动/长按/连线/捏合/滚轮）只从这里进入，
    // 递归 Node/Embed 不再各自挂业务手势，同一个手势不会再有第二个解释者。
    // ---------------------------------------------------------------------------
    StarMapInputRouter {
        id: inputRouter
        anchors.fill: parent
        canvas: canvasArea
        z: 10
    }

    // 连线预览线：整棵星图只有这一条，在根 Canvas 的 overlay 上。
    // 坐标是 scene 坐标（整棵递归树的顶层 world 坐标），用全局相机换算。
    Canvas {
        id: connectPreview
        anchors.fill: parent
        z: 3

        Connections {
            target: canvasArea
            function onPanXChanged() { connectPreview.requestPaint() }
            function onPanYChanged() { connectPreview.requestPaint() }
            function onZoomLevelChanged() { connectPreview.requestPaint() }
        }
        Connections {
            target: interaction
            function onConnectMouseXChanged() { connectPreview.requestPaint() }
            function onConnectMouseYChanged() { connectPreview.requestPaint() }
            function onConnectFromSceneXChanged() { connectPreview.requestPaint() }
            function onConnectFromSceneYChanged() { connectPreview.requestPaint() }
            function onConnectPreviewEndXChanged() { connectPreview.requestPaint() }
            function onConnectPreviewEndYChanged() { connectPreview.requestPaint() }
            function onPointerModeChanged() { connectPreview.requestPaint() }
        }

        onPaint: {
            var ctx = getContext("2d")
            ctx.clearRect(0, 0, width, height)
            if (interaction.pointerMode !== "connect")
                return
            if (interaction.connectFromKind === "")
                return
            ctx.save()
            ctx.translate(panX, panY)
            ctx.scale(zoomLevel, zoomLevel)
            ctx.beginPath()
            // 预览起点/终点都取"与正式边同源"的可见端点（悬停合法目标时贴边界）。
            ctx.moveTo(interaction.connectFromSceneX, interaction.connectFromSceneY)
            ctx.lineTo(interaction.connectPreviewEndX, interaction.connectPreviewEndY)
            ctx.strokeStyle = _accent
            // 线宽与命中阈值共用同一份 world 线宽常量。
            ctx.lineWidth = rootContent ? rootContent._edgeLineWorldWidth : 2
            ctx.stroke()
            ctx.restore()
        }
    }

    // Issue #801 评论 5894639734: 触屏缩放 +/- 按钮（右下角浮层）。
    RowLayout {
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        anchors.rightMargin: 16
        anchors.bottomMargin: 16
        spacing: 8
        z: 50
        visible: _touchInputActive

        AppButton {
            dt: canvasArea.dt
            text: qsTr("+")
            // 缩放锚点是画布中心：按钮自己的 width/height 不是画布尺寸。
            onClicked: zoomAt(canvasArea.width / 2, canvasArea.height / 2,
                              zoomLevel * _zoomFactor)
        }

        AppButton {
            dt: canvasArea.dt
            text: qsTr("−")
            onClicked: zoomAt(canvasArea.width / 2, canvasArea.height / 2,
                              zoomLevel / _zoomFactor)
        }
    }

    Rectangle {
        id: errorBanner
        width: parent.width - 32
        height: 40
        anchors.bottom: parent.bottom
        anchors.bottomMargin: 16
        anchors.horizontalCenter: parent.horizontalCenter
        color: _error
        radius: _radiusSm
        visible: errorMessage.length > 0
        z: 100

        AppText {
            dt: canvasArea.dt
            anchors.centerIn: parent
            text: canvasArea.errorMessage
            color: _onError
            font.pointSize: dt.bodyPt
        }
        MouseArea {
            anchors.fill: parent
            onClicked: canvasArea.clearError()
        }
    }

    // Issue #801 评论 5895310100: 触屏长按菜单视觉层（不抢 pointer grab）。
    // 此组件纯视觉，无任何 TapHandler/MouseArea/Handler。
    Item {
        id: touchContextPreview
        visible: false
        z: 60

        property string previewKind: ""   // "node" / "embed"
        property real anchorX: 0
        property real anchorY: 0

        x: anchorX - width / 2
        y: anchorY + 8
        width: 150
        height: 120

        Rectangle {
            anchors.fill: parent
            color: _card
            border.color: _border
            border.width: 1
            radius: _radiusSm
        }

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: 8
            spacing: 4

            AppText {
                dt: canvasArea.dt
                Layout.fillWidth: true
                text: touchContextPreview.previewKind === "embed" ? qsTr("编辑名称") : qsTr("编辑")
                color: _textPrimary
                font.pointSize: dt.labelPt
                leftPadding: 4
            }
            AppText {
                dt: canvasArea.dt
                Layout.fillWidth: true
                text: qsTr("移动")
                color: _textPrimary
                font.pointSize: dt.labelPt
                leftPadding: 4
            }
            AppText {
                dt: canvasArea.dt
                Layout.fillWidth: true
                text: qsTr("删除")
                color: _textPrimary
                font.pointSize: dt.labelPt
                leftPadding: 4
            }
        }

        function show(kind, sx, sy) {
            previewKind = kind
            anchorX = sx
            anchorY = sy
            visible = true
        }

        function hide() {
            visible = false
        }
    }

    // ---------------------------------------------------------------------------
    // 菜单入口：归属层由递归命中决定，坐标是 scene 坐标。
    // Issue #832：Router 是唯一调用方；菜单宿主仍在本文件。
    // ---------------------------------------------------------------------------
    function openBlankMenu(sceneX, sceneY, hit, screenX, screenY) {
        menuOwnerContent = hit ? hit.owner : null
        selectedNodeForMenu = null
        selectedEdgeForMenu = null
        selectedEmbedForMenu = null
        contextMenuSceneX = sceneX
        contextMenuSceneY = sceneY
        logInteraction("context_menu_open", "empty", "", {
            "menuKind": "bg",
            "sceneX": sceneX,
            "sceneY": sceneY
        }, hit ? hit.scenePathKey : "root")
        bgContextMenu.popup(screenX, screenY)
    }

    // 右键命中分发：node / embed / edge 开对应菜单；空白用命中层的 owner 打开
    // 该层的新建菜单（在子星图空白右键就在子星图里新建）。
    function openHitContextMenu(hit, screenX, screenY) {
        var sx = screenToWorldX(screenX)
        var sy = screenToWorldY(screenY)
        if (hit.kind === "node") {
            hit.owner.selectNode(hit.id)
            menuOwnerContent = hit.owner
            selectedNodeForMenu = hit
            logInteraction("context_menu_open", "node", hit.id, {
                "menuKind": "node",
                "sceneX": sx,
                "sceneY": sy
            }, hit.scenePathKey)
            nodeContextMenu.popup(screenX, screenY)
        } else if (hit.kind === "embed") {
            hit.owner.selectEmbed(hit.id)
            menuOwnerContent = hit.owner
            selectedEmbedForMenu = hit
            logInteraction("context_menu_open", "embed", hit.id, {
                "menuKind": "embed",
                "sceneX": sx,
                "sceneY": sy
            }, hit.scenePathKey)
            embedContextMenu.popup(screenX, screenY)
        } else if (hit.kind === "edge") {
            hit.owner.selectEdge(hit.id)
            menuOwnerContent = hit.owner
            selectedEdgeForMenu = hit
            logInteraction("context_menu_open", "edge", hit.id, {
                "menuKind": "edge",
                "sceneX": sx,
                "sceneY": sy
            }, hit.scenePathKey)
            edgeContextMenu.popup(screenX, screenY)
        } else {
            openBlankMenu(sx, sy, hit, screenX, screenY)
        }
    }

    // 触屏长按视觉层：scene 坐标 → 屏幕坐标后显示。
    function showTouchPreview(kind, sceneX, sceneY) {
        touchContextPreview.show(kind, worldToScreenX(sceneX), worldToScreenY(sceneY))
    }

    function hideTouchPreview() {
        touchContextPreview.hide()
    }

    // 触屏长按松手：弹出真正可点击的菜单，归属层就是长按对象的归属层。
    function showLongPressMenu(kind, id, sceneX, sceneY, owner) {
        menuOwnerContent = owner
        if (kind === "node") {
            selectedNodeForMenu = owner.hitNode(id)
            if (!selectedNodeForMenu)
                return
            owner.selectNode(id)
            nodeContextMenu.popup(worldToScreenX(sceneX), worldToScreenY(sceneY))
        } else if (kind === "embed") {
            selectedEmbedForMenu = owner.hitEmbed(id)
            if (!selectedEmbedForMenu)
                return
            owner.selectEmbed(id)
            embedContextMenu.popup(worldToScreenX(sceneX), worldToScreenY(sceneY))
        }
    }

    // Context Menus
    Menu {
        id: bgContextMenu

        background: Rectangle {
            implicitWidth: 150
            color: _card
            border.color: _border
            border.width: 1
            radius: _radiusSm
        }

        MenuItem {
            id: bgMenuItem1
            text: qsTr("新建节点")
            contentItem: AppText {
                dt: canvasArea.dt
                text: bgMenuItem1.text
                color: bgMenuItem1.hovered ? _accent : _textPrimary
                font.pointSize: dt.labelPt
                font.bold: true
                verticalAlignment: Text.AlignVCenter
                leftPadding: 12
            }
            background: Rectangle {
                color: bgMenuItem1.hovered ? _accentSoft : "transparent"
                radius: _radiusXs
            }
            onTriggered: createDialog.open("node", contextMenuSceneX, contextMenuSceneY)
        }

        MenuItem {
            id: bgMenuItem2
            text: qsTr("新建子星图")
            contentItem: AppText {
                dt: canvasArea.dt
                text: bgMenuItem2.text
                color: bgMenuItem2.hovered ? _accent : _textPrimary
                font.pointSize: dt.labelPt
                font.bold: true
                verticalAlignment: Text.AlignVCenter
                leftPadding: 12
            }
            background: Rectangle {
                color: bgMenuItem2.hovered ? _accentSoft : "transparent"
                radius: _radiusXs
            }
            onTriggered: createDialog.open("starmap", contextMenuSceneX, contextMenuSceneY)
        }
    }

    Menu {
        id: nodeContextMenu

        background: Rectangle {
            implicitWidth: 150
            color: _card
            border.color: _border
            border.width: 1
            radius: _radiusSm
        }

        // Issue #822: 节点标题编辑就在节点框内（TextInput 内联），
        // 这里只负责把光标放进节点框，不再打开外部编辑 Popup。
        MenuItem {
            id: nodeMenuItemEdit
            text: qsTr("编辑")
            contentItem: AppText {
                dt: canvasArea.dt
                text: nodeMenuItemEdit.text
                color: nodeMenuItemEdit.hovered ? _accent : _textPrimary
                font.pointSize: dt.labelPt
                verticalAlignment: Text.AlignVCenter
                leftPadding: 12
            }
            background: Rectangle {
                color: nodeMenuItemEdit.hovered ? _accentSoft : "transparent"
                radius: _radiusXs
            }
            onTriggered: {
                if (selectedNodeForMenu && menuOwnerContent)
                    menuOwnerContent.beginInlineEdit(selectedNodeForMenu.id)
            }
        }

        MenuItem {
            id: nodeMenuItemMove
            text: qsTr("移动")
            contentItem: AppText {
                dt: canvasArea.dt
                text: nodeMenuItemMove.text
                color: nodeMenuItemMove.hovered ? _accent : _textPrimary
                font.pointSize: dt.labelPt
                verticalAlignment: Text.AlignVCenter
                leftPadding: 12
            }
            background: Rectangle {
                color: nodeMenuItemMove.hovered ? _accentSoft : "transparent"
                radius: _radiusXs
            }
            onTriggered: {
                // 菜单已确定目标，直接进 move，不经过 pressPending 仲裁；
                // 完整身份（kind/id/targetPath/scenePathKey）与手势路径同源。
                if (selectedNodeForMenu && menuOwnerContent) {
                    var item = menuOwnerContent.hitNode(selectedNodeForMenu.id)
                    if (!item)
                        return
                    interaction.beginMove("node", item.id,
                                          menuOwnerContent.nodePath(item.id),
                                          menuOwnerContent.scenePathKey,
                                          item.x, item.y)
                }
            }
        }

        MenuItem {
            id: nodeMenuItemLink
            text: qsTr("内部链接")
            contentItem: AppText {
                dt: canvasArea.dt
                text: nodeMenuItemLink.text
                color: nodeMenuItemLink.hovered ? _accent : _textPrimary
                font.pointSize: dt.labelPt
                verticalAlignment: Text.AlignVCenter
                leftPadding: 12
            }
            background: Rectangle {
                color: nodeMenuItemLink.hovered ? _accentSoft : "transparent"
                radius: _radiusXs
            }
            onTriggered: {
                // Issue #834：节点内部链接菜单（StarMapLink，内部跳转）。
                // menuOwnerContent 是命中层 SceneContent，source path 用该层完整 nodePath。
                if (selectedNodeForMenu && menuOwnerContent)
                    linkDialog.open("node", selectedNodeForMenu.id, menuOwnerContent,
                                    menuOwnerContent.nodePath(selectedNodeForMenu.id))
            }
        }

        MenuItem {
            id: nodeMenuItemConnect
            text: qsTr("连线")
            contentItem: AppText {
                dt: canvasArea.dt
                text: nodeMenuItemConnect.text
                color: nodeMenuItemConnect.hovered ? _accent : _textPrimary
                font.pointSize: dt.labelPt
                verticalAlignment: Text.AlignVCenter
                leftPadding: 12
            }
            background: Rectangle {
                color: nodeMenuItemConnect.hovered ? _accentSoft : "transparent"
                radius: _radiusXs
            }
            onTriggered: {
                // Issue #834：菜单发起连线。source 已知，进 connectArmed 等点 target。
                if (selectedNodeForMenu && menuOwnerContent) {
                    var center = menuOwnerContent.itemCenterScene("node", selectedNodeForMenu.id)
                    if (center)
                        interaction.beginConnectFromMenu("node", selectedNodeForMenu.id,
                                menuOwnerContent.nodePath(selectedNodeForMenu.id),
                                menuOwnerContent.scenePathKey, center.x, center.y)
                }
            }
        }

        MenuItem {
            id: nodeMenuItemDelete
            text: qsTr("删除")
            contentItem: AppText {
                dt: canvasArea.dt
                text: nodeMenuItemDelete.text
                color: nodeMenuItemDelete.hovered ? _danger : _textPrimary
                font.pointSize: dt.labelPt
                verticalAlignment: Text.AlignVCenter
                leftPadding: 12
            }
            background: Rectangle {
                color: nodeMenuItemDelete.hovered ? _dangerContainer : "transparent"
                radius: _radiusXs
            }
            onTriggered: {
                if (selectedNodeForMenu && menuOwnerContent)
                    menuOwnerContent.deleteNode(selectedNodeForMenu.id)
            }
        }
    }

    Menu {
        id: edgeContextMenu

        background: Rectangle {
            implicitWidth: 150
            color: _card
            border.color: _border
            border.width: 1
            radius: _radiusSm
        }

        MenuItem {
            id: edgeMenuItem1
            text: qsTr("重命名连线")
            contentItem: AppText {
                dt: canvasArea.dt
                text: edgeMenuItem1.text
                color: edgeMenuItem1.hovered ? _accent : _textPrimary
                font.pointSize: dt.labelPt
                verticalAlignment: Text.AlignVCenter
                leftPadding: 12
            }
            background: Rectangle {
                color: edgeMenuItem1.hovered ? _accentSoft : "transparent"
                radius: _radiusXs
            }
            onTriggered: {
                if (selectedEdgeForMenu) {
                    renameDialog.open("edge", selectedEdgeForMenu.id, selectedEdgeForMenu.label || "")
                }
            }
        }

        MenuItem {
            id: edgeMenuItem2
            text: qsTr("删除连线")
            contentItem: AppText {
                dt: canvasArea.dt
                text: edgeMenuItem2.text
                color: edgeMenuItem2.hovered ? _danger : _textPrimary
                font.pointSize: dt.labelPt
                verticalAlignment: Text.AlignVCenter
                leftPadding: 12
            }
            background: Rectangle {
                color: edgeMenuItem2.hovered ? _dangerContainer : "transparent"
                radius: _radiusXs
            }
            onTriggered: {
                if (selectedEdgeForMenu && menuOwnerContent)
                    menuOwnerContent.deleteEdge(selectedEdgeForMenu.id)
            }
        }
    }

    // Issue #796 评论 5886483653: Embed 右键菜单：编辑名称 / 移动 / 删除
    Menu {
        id: embedContextMenu

        background: Rectangle {
            implicitWidth: 150
            color: _card
            border.color: _border
            border.width: 1
            radius: _radiusSm
        }

        MenuItem {
            id: embedMenuItemRename
            text: qsTr("编辑名称")
            contentItem: AppText {
                dt: canvasArea.dt
                text: embedMenuItemRename.text
                color: embedMenuItemRename.hovered ? _accent : _textPrimary
                font.pointSize: dt.labelPt
                verticalAlignment: Text.AlignVCenter
                leftPadding: 12
            }
            background: Rectangle {
                color: embedMenuItemRename.hovered ? _accentSoft : "transparent"
                radius: _radiusXs
            }
            onTriggered: {
                if (selectedEmbedForMenu) {
                    renameDialog.open("embed", selectedEmbedForMenu.instanceId, selectedEmbedForMenu.label || "")
                }
            }
        }

        MenuItem {
            id: embedMenuItemMove
            text: qsTr("移动")
            contentItem: AppText {
                dt: canvasArea.dt
                text: embedMenuItemMove.text
                color: embedMenuItemMove.hovered ? _accent : _textPrimary
                font.pointSize: dt.labelPt
                verticalAlignment: Text.AlignVCenter
                leftPadding: 12
            }
            background: Rectangle {
                color: embedMenuItemMove.hovered ? _accentSoft : "transparent"
                radius: _radiusXs
            }
            onTriggered: {
                if (selectedEmbedForMenu && menuOwnerContent) {
                    var item = menuOwnerContent.hitEmbed(selectedEmbedForMenu.instanceId)
                    if (!item)
                        return
                    interaction.beginMove("embed", item.instanceId,
                                          menuOwnerContent.embedPath(item.instanceId),
                                          menuOwnerContent.scenePathKey,
                                          item.x, item.y)
                }
            }
        }

        MenuItem {
            id: embedMenuItemLink
            text: qsTr("内部链接")
            contentItem: AppText {
                dt: canvasArea.dt
                text: embedMenuItemLink.text
                color: embedMenuItemLink.hovered ? _accent : _textPrimary
                font.pointSize: dt.labelPt
                verticalAlignment: Text.AlignVCenter
                leftPadding: 12
            }
            background: Rectangle {
                color: embedMenuItemLink.hovered ? _accentSoft : "transparent"
                radius: _radiusXs
            }
            onTriggered: {
                // Issue #834：子星图入口内部链接菜单（StarMapLink，内部跳转）。
                // menuOwnerContent 是命中层 SceneContent，source path 用该层完整 embedPath。
                if (selectedEmbedForMenu && menuOwnerContent)
                    linkDialog.open("embed", selectedEmbedForMenu.instanceId, menuOwnerContent,
                                    menuOwnerContent.embedPath(selectedEmbedForMenu.instanceId))
            }
        }

        MenuItem {
            id: embedMenuItemConnect
            text: qsTr("连线")
            contentItem: AppText {
                dt: canvasArea.dt
                text: embedMenuItemConnect.text
                color: embedMenuItemConnect.hovered ? _accent : _textPrimary
                font.pointSize: dt.labelPt
                verticalAlignment: Text.AlignVCenter
                leftPadding: 12
            }
            background: Rectangle {
                color: embedMenuItemConnect.hovered ? _accentSoft : "transparent"
                radius: _radiusXs
            }
            onTriggered: {
                // Issue #834：菜单发起连线。source 已知，进 connectArmed 等点 target。
                if (selectedEmbedForMenu && menuOwnerContent) {
                    var center = menuOwnerContent.itemCenterScene("embed", selectedEmbedForMenu.instanceId)
                    if (center)
                        interaction.beginConnectFromMenu("embed", selectedEmbedForMenu.instanceId,
                                menuOwnerContent.embedPath(selectedEmbedForMenu.instanceId),
                                menuOwnerContent.scenePathKey, center.x, center.y)
                }
            }
        }

        MenuItem {
            id: embedMenuItemDelete
            text: qsTr("删除")
            contentItem: AppText {
                dt: canvasArea.dt
                text: embedMenuItemDelete.text
                color: embedMenuItemDelete.hovered ? _danger : _textPrimary
                font.pointSize: dt.labelPt
                verticalAlignment: Text.AlignVCenter
                leftPadding: 12
            }
            background: Rectangle {
                color: embedMenuItemDelete.hovered ? _dangerContainer : "transparent"
                radius: _radiusXs
            }
            onTriggered: {
                if (selectedEmbedForMenu && menuOwnerContent)
                    menuOwnerContent.deleteEmbed(selectedEmbedForMenu.instanceId)
            }
        }
    }

    // Issue #796 评论 5886483653: 弹窗改 Qt Quick Controls Popup。
    Popup {
        id: renameDialog
        modal: true
        focus: true
        closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
        width: 300
        height: 160
        anchors.centerIn: Overlay.overlay
        Overlay.modal: Rectangle { color: Qt.rgba(0, 0, 0, 0.32) }
        background: Rectangle {
            color: _card
            border.color: _border
            border.width: 1.5
            radius: _dialogRadius
        }

        property string targetType: "" // "edge" / "embed"
        property string targetId: ""
        property string initialText: ""

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: 20
            spacing: 16

            AppText {
                dt: canvasArea.dt
                text: renameDialog.targetType === "embed" ? qsTr("修改子星图名称")
                     : qsTr("修改连线标签")
                font.pointSize: dt.fontLgPt
                font.bold: true
                color: _textPrimary
            }

            TextField {
                id: renameInput
                Layout.fillWidth: true
                height: 36
                color: _textPrimary
                font.pointSize: dt.bodyPt
                focus: renameDialog.visible
                text: renameDialog.initialText

                background: Rectangle {
                    color: _surfaceContainer
                    border.color: renameInput.activeFocus ? _accent : _border
                    border.width: 1.5
                    radius: _radiusXs
                }

                Keys.onReturnPressed: renameDialog.confirm()
                Keys.onEscapePressed: renameDialog.close()
            }

            RowLayout {
                Layout.alignment: Qt.AlignRight
                spacing: 12

                Button {
                    id: cancelBtn
                    text: qsTr("取消")
                    onClicked: renameDialog.close()
                    contentItem: AppText {
                        dt: canvasArea.dt
                        text: cancelBtn.text
                        color: _textSecondary
                        font.pointSize: dt.labelPt
                    }
                    background: Rectangle {
                        color: cancelBtn.hovered ? _surfaceContainer : "transparent"
                        border.color: _border
                        radius: _radiusXs
                    }
                }

                Button {
                    id: confirmBtn
                    text: qsTr("确定")
                    onClicked: renameDialog.confirm()
                    contentItem: AppText {
                        dt: canvasArea.dt
                        text: confirmBtn.text
                        color: _onPrimary
                        font.bold: true
                        font.pointSize: dt.labelPt
                    }
                    background: Rectangle {
                        color: confirmBtn.hovered ? _accentHover : _accent
                        radius: _radiusXs
                    }
                }
            }
        }

        function open(type, id, text) {
            targetType = type
            targetId = id
            initialText = text
            renameInput.text = text
            visible = true
            renameInput.forceActiveFocus()
        }

        function close() {
            visible = false
        }

        function confirm() {
            // Issue #822: 连线标签和子星图名称仍然用这个弹窗；
            // 节点标题已改为节点内联编辑，不再走这里。
            if (!menuOwnerContent) {
                close()
                return
            }
            if (targetType === "edge") {
                menuOwnerContent.updateEdgeLabel(targetId, renameInput.text)
            } else if (targetType === "embed") {
                menuOwnerContent.updateEmbedLabel(targetId, renameInput.text)
            }
            close()
        }
    }

    // Issue #793 评论 5884923277: 新建节点/子星图 Dialog
    Popup {
        id: createDialog
        modal: true
        focus: true
        closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
        width: 300
        height: 160
        anchors.centerIn: Overlay.overlay
        Overlay.modal: Rectangle { color: Qt.rgba(0, 0, 0, 0.32) }
        background: Rectangle {
            color: _card
            border.color: _border
            border.width: 1.5
            radius: _dialogRadius
        }

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: 20
            spacing: 16

            AppText {
                dt: canvasArea.dt
                text: createMode === "starmap" ? qsTr("新建子星图") : qsTr("新建节点")
                font.pointSize: dt.fontLgPt
                font.bold: true
                color: _textPrimary
            }

            TextField {
                id: createInput
                Layout.fillWidth: true
                height: 36
                color: _textPrimary
                font.pointSize: dt.bodyPt
                placeholderText: qsTr("名称")
                focus: createDialog.visible
                text: ""

                background: Rectangle {
                    color: _surfaceContainer
                    border.color: createInput.activeFocus ? _accent : _border
                    border.width: 1.5
                    radius: _radiusXs
                }

                Keys.onReturnPressed: createDialog.confirm()
                Keys.onEscapePressed: createDialog.close()
            }

            RowLayout {
                Layout.alignment: Qt.AlignRight
                spacing: 12

                Button {
                    id: createCancelBtn
                    text: qsTr("取消")
                    onClicked: createDialog.close()
                    contentItem: AppText {
                        dt: canvasArea.dt
                        text: createCancelBtn.text
                        color: _textSecondary
                        font.pointSize: dt.labelPt
                    }
                    background: Rectangle {
                        color: createCancelBtn.hovered ? _surfaceContainer : "transparent"
                        border.color: _border
                        radius: _radiusXs
                    }
                }

                Button {
                    id: createConfirmBtn
                    text: qsTr("确定")
                    onClicked: createDialog.confirm()
                    contentItem: AppText {
                        dt: canvasArea.dt
                        text: createConfirmBtn.text
                        color: _onPrimary
                        font.bold: true
                        font.pointSize: dt.labelPt
                    }
                    background: Rectangle {
                        color: createConfirmBtn.hovered ? _accentHover : _accent
                        radius: _radiusXs
                    }
                }
            }
        }

        function open(mode, sceneX, sceneY) {
            createMode = mode
            contextMenuSceneX = sceneX
            contextMenuSceneY = sceneY
            createInput.text = ""
            visible = true
            createInput.forceActiveFocus()
        }

        function close() {
            visible = false
        }

        function confirm() {
            var name = createInput.text.trim()
            if (name.length === 0 || !menuOwnerContent) {
                close()
                return
            }
            // Issue #822: 归属层自己把 scene 坐标换算成本层局部坐标，
            // 空的子星图可以直接新建内容，不用"进入"另一个页面。
            if (createMode === "node")
                menuOwnerContent.createNodeWithName(name, contextMenuSceneX, contextMenuSceneY)
            else if (createMode === "starmap")
                menuOwnerContent.createSubStarmapWithName(name, contextMenuSceneX, contextMenuSceneY)
            close()
        }
    }

    // ---------------------------------------------------------------------------
    // Issue #834：内部链接弹窗（StarMapLink，内部跳转）。
    // 列出该 source 已有的内部链接，可删除；"选择目标"进入 linkArmed，
    // 下一次点 node/embed 是 target（不填 URI，不是外部超链接）。
    // source path 由菜单传入（命中层 SceneContent 的 nodePath/embedPath）。
    // ---------------------------------------------------------------------------
    Popup {
        id: linkDialog
        modal: true
        focus: true
        closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
        width: 360
        height: 320
        anchors.centerIn: Overlay.overlay
        Overlay.modal: Rectangle { color: Qt.rgba(0, 0, 0, 0.32) }
        background: Rectangle {
            color: _card
            border.color: _border
            border.width: 1.5
            radius: _dialogRadius
        }

        property string sourceKind: ""    // "node" / "embed"
        property string sourceId: ""
        property var sourceOwner: null    // 命中层 SceneContent
        property var sourcePath: null     // StarMapTargetPathDto
        property var linkItems: []        // 已有内部链接列表

        function refreshItems() {
            if (sourceOwner && sourcePath)
                linkItems = sourceOwner.listLinksForSource(sourcePath)
            else
                linkItems = []
        }

        // target 路径摘要：node 显示 nodeId，starmap(embed) 显示"子星图"。
        function targetSummary(link) {
            if (!link || !link.target)
                return ""
            var t = link.target.target
            if (!t)
                return ""
            return t.type === "node" ? (t.nodeId || "") : qsTr("子星图")
        }

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: 20
            spacing: 12

            AppText {
                dt: canvasArea.dt
                text: qsTr("内部链接")
                font.pointSize: dt.fontLgPt
                font.bold: true
                color: _textPrimary
            }

            AppText {
                dt: canvasArea.dt
                text: linkDialog.linkItems.length > 0
                      ? qsTr("已链接到：")
                      : qsTr("尚无内部链接，选择目标开始建立。")
                color: _textSecondary
                font.pointSize: dt.labelPt
                Layout.fillWidth: true
                wrapMode: Text.WordWrap
            }

            // 已有链接列表（每条：摘要 + 删除按钮）
            ListView {
                id: linkList
                Layout.fillWidth: true
                Layout.fillHeight: true
                clip: true
                model: linkDialog.linkItems
                spacing: 6
                delegate: RowLayout {
                    width: linkList.width
                    spacing: 8

                    AppText {
                        dt: canvasArea.dt
                        Layout.fillWidth: true
                        text: linkDialog.targetSummary(modelData)
                                + (modelData.label ? "（" + modelData.label + "）" : "")
                        color: _textPrimary
                        font.pointSize: dt.bodyPt
                        elide: Text.ElideRight
                    }

                    Button {
                        id: linkDelBtn
                        text: qsTr("删除")
                        onClicked: {
                            if (linkDialog.sourceOwner && modelData.linkId) {
                                linkDialog.sourceOwner.deleteLink(modelData.linkId)
                                linkDialog.refreshItems()
                            }
                        }
                        contentItem: AppText {
                            dt: canvasArea.dt
                            text: linkDelBtn.text
                            color: _danger
                            font.pointSize: dt.labelPt
                        }
                        background: Rectangle {
                            color: linkDelBtn.hovered ? _dangerContainer : "transparent"
                            border.color: _border
                            radius: _radiusXs
                        }
                    }
                }
            }

            RowLayout {
                Layout.alignment: Qt.AlignRight
                spacing: 12

                Button {
                    id: linkCancelBtn
                    text: qsTr("取消")
                    onClicked: linkDialog.close()
                    contentItem: AppText {
                        dt: canvasArea.dt
                        text: linkCancelBtn.text
                        color: _textSecondary
                        font.pointSize: dt.labelPt
                    }
                    background: Rectangle {
                        color: linkCancelBtn.hovered ? _surfaceContainer : "transparent"
                        border.color: _border
                        radius: _radiusXs
                    }
                }

                Button {
                    id: linkPickTargetBtn
                    text: qsTr("选择目标")
                    onClicked: {
                        // 关闭弹窗，进入 linkArmed：下一次点 node/embed 是 target。
                        if (linkDialog.sourceOwner && linkDialog.sourcePath) {
                            interaction.beginLinkArmed(linkDialog.sourceKind,
                                    linkDialog.sourceId, linkDialog.sourcePath,
                                    linkDialog.sourceOwner.scenePathKey)
                        }
                        linkDialog.close()
                    }
                    contentItem: AppText {
                        dt: canvasArea.dt
                        text: linkPickTargetBtn.text
                        color: _onPrimary
                        font.bold: true
                        font.pointSize: dt.labelPt
                    }
                    background: Rectangle {
                        color: linkPickTargetBtn.hovered ? _accentHover : _accent
                        radius: _radiusXs
                    }
                }
            }
        }

        function open(kind, id, owner, sourcePath) {
            sourceKind = kind
            sourceId = id
            sourceOwner = owner
            linkDialog.sourcePath = sourcePath
            refreshItems()
            visible = true
        }

        function close() {
            visible = false
        }
    }
}
