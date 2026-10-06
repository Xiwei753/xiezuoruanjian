// =============================================================================
// StarMapInputRouter.qml — 星图唯一原始输入路由
// =============================================================================
//
// 层级：Linux_qt UI 层（QML UI 组件）
// 职责：整棵星图唯一的原始输入层，透明铺在根 Canvas 的整棵可视内容之上。
//   所有鼠标/触摸语义都从这里进入，递归 Node/Embed 不再各自挂业务手势，
//   同一个手势不会再有第二个解释者：
//
//   鼠标：
//     - 左键单击 node/embed/edge：选中；空白：清该层选区
//     - 左键双击 node：内联编辑；双击 embed：相机聚焦推进该子星图（不开新页面）
//     - 左键按下 node/embed：pressPending。拖动阈值先到 → move；
//       长按时间先到 → connect
//     - 左键拖空白：pan
//     - 右键 node/embed/edge：对应菜单；右键空白：该层的新建菜单
//     - connect 松手：再调一次根递归命中，目标只接受 node/embed，
//       复用 StarMapPathPlanner.planCrossLayerRelation() 找 LCA 宿主并落边
//
//   触屏：
//     - 未长按直接滑动：无论起点是不是 node/embed 都优先 pan
//     - 长按 node/embed：contextPending + 菜单视觉；继续移动超阈值 → 关闭视觉
//       切 connect；直接松手 → 正式菜单
//     - 长按空白：该层背景菜单
//     - Pinch 一激活就 beginPinch()（清掉单指状态），结束统一 endPinch()
//     - Wheel 只改根相机 zoom，永远不改某个子图自己的尺寸状态
//
//   长按 Timer 就在本文件；StarMapInteractionController 只保存状态，
//   状态提升（pressPending -> move/connect/contextPending/pan）只允许本文件调用。
//
// 约束：
//   - 不读写 Core，不做几何命中（命中统一走 canvas.hitTargetAtScreen）
//   - 内联编辑进行中整体禁用 4 个单指业务 Handler（disabled 的 Handler 不再
//     参与事件投递），TextInput 独占点击/拖动/文本选区
// =============================================================================

import QtQuick

Item {
    id: router

    // 根 Canvas：唯一命中入口 / 相机 / 菜单宿主 / 预览 overlay 的宿主。
    required property var canvas

    readonly property var ic: canvas ? canvas.sharedInteraction : null
    readonly property bool pinchActive:
        pinchHandler.active || (ic && ic.pointerMode === "pinch")
    // 有节点正在内联编辑：拖动 Handler 让位，TextInput 独占文本交互。
    readonly property bool editingBlocks: canvas ? canvas.inlineEditingKey !== "" : false

    // ── 本文件持有的手势现场 ──
    // 按下时的完整递归命中（owner/scenePathKey/starmapId/kind/id/targetPath）。
    property var pressHit: null
    // 当前手势归属层（按下目标的 owner；右键菜单"移动"也复用）。
    property var gestureOwner: null
    // 空白/连线按下（还没决定 pan），松手按点击处理。
    property bool emptyPressActive: false
    // 触屏空白长按预备（到点弹该层背景菜单）。
    property bool emptyLongPressArmed: false
    property bool panActive: false
    property real panBeginX: 0
    property real panBeginY: 0
    property real pressScreenX: 0
    property real pressScreenY: 0

    function hitAt(sx, sy) {
        return canvas ? canvas.hitTargetAtScreen(sx, sy) : null
    }

    function hasGestureTarget(hit) {
        return !!hit && (hit.kind === "node" || hit.kind === "embed")
    }

    function panDevice() {
        return (ic && ic.pointerSource === "touch") ? "touch" : "mouse"
    }

    function cancelLocalState() {
        pressHit = null
        gestureOwner = null
        emptyPressActive = false
        emptyLongPressArmed = false
        panActive = false
    }

    // ── 按下：只登记命中与归属，不决定 move/connect/pan ──
    function beginPress(point, source) {
        if (!canvas || !ic)
            return
        if (ic.pointerMode === "pinch")
            return
        canvas.notePointerDevice(source === "touch")
        pressScreenX = point.position.x
        pressScreenY = point.position.y
        var hit = hitAt(pressScreenX, pressScreenY)
        pressHit = hit
        emptyPressActive = false
        emptyLongPressArmed = false
        canvas.logPointerPress("left", source, point)

        if (ic.pointerMode !== "idle") {
            // 右键菜单"移动"已经确定了目标：只有按在同一个目标上才是这次 move 的
            // 继续，其它对象上的按下不改变任何状态。
            if (ic.pointerMode === "move" && hasGestureTarget(hit)
                    && ic.isMovingTarget(hit.scenePathKey, hit.kind, hit.id)) {
                gestureOwner = hit.owner
            }
            return
        }

        if (hasGestureTarget(hit)) {
            gestureOwner = hit.owner
            ic.pointerSource = source
            ic.beginPress(hit.kind, hit.id, hit.targetPath, hit.scenePathKey,
                          canvas.screenToWorldX(pressScreenX),
                          canvas.screenToWorldY(pressScreenY))
            return
        }

        // 空白 / 连线：等拖动阈值决定 pan；松手走点击语义。
        gestureOwner = null
        emptyPressActive = true
        emptyLongPressArmed = (source === "touch")
    }

    // ── 点击类 ──
    function selectHit(hit) {
        if (!hit || !hit.owner)
            return
        if (hit.kind === "node") {
            hit.owner.selectNode(hit.id)
            hit.owner.logInteraction("selection_changed", "node", hit.id, {})
        } else if (hit.kind === "embed") {
            hit.owner.selectEmbed(hit.id)
            hit.owner.logInteraction("selection_changed", "embed", hit.id, {})
        } else if (hit.kind === "edge") {
            hit.owner.selectEdge(hit.id)
            hit.owner.logInteraction("selection_changed", "edge", hit.id, {})
        } else if (hit.kind === "empty") {
            hit.owner.clearLayerSelection()
            hit.owner.logInteraction("selection_changed", "none", "", {})
        }
    }

    function handleSingleTap(point, source) {
        if (!canvas || !ic)
            return
        if (ic.pointerMode === "pinch")
            return
        canvas.notePointerDevice(source === "touch")
        // Issue #834：菜单发起的 armed 态优先于普通选中。
        // linkArmed：下一次 tap 命中 node/embed 是 Link 的 target；命中自己拒绝
        //   （取消 armed，不建自指），命中空白取消。
        // connectArmed：同构，命中 node/embed 把 connect 鼠标更新到本次 tap 的
        //   scene 位置再走 finishConnect（菜单发起时 connectMouse 停在 source 中心）。
        if (ic.linkArmed) {
            var hitL = hitAt(point.position.x, point.position.y)
            if (hasGestureTarget(hitL)) {
                if (hitL.scenePathKey === ic.linkFromScenePathKey && hitL.id === ic.linkFromId)
                    ic.cancelArmed()
                else
                    hitL.owner.finishLink(hitL)
            } else {
                ic.cancelArmed()
            }
            return
        }
        if (ic.connectArmed) {
            var hitC = hitAt(point.position.x, point.position.y)
            if (hasGestureTarget(hitC)) {
                if (hitC.scenePathKey === ic.connectFromScenePathKey && hitC.id === ic.connectFromId) {
                    ic.cancelArmed()
                } else {
                    ic.connectMouseX = canvas.screenToWorldX(point.position.x)
                    ic.connectMouseY = canvas.screenToWorldY(point.position.y)
                    hitC.owner.finishConnect()
                }
            } else {
                ic.cancelArmed()
            }
            return
        }
        selectHit(hitAt(point.position.x, point.position.y))
    }

    function handleDoubleTap(point, source) {
        if (!canvas || !ic)
            return
        if (ic.pointerMode === "pinch")
            return
        var hit = hitAt(point.position.x, point.position.y)
        if (!hit)
            return
        if (hit.kind === "node") {
            hit.owner.beginInlineEdit(hit.id)
        } else if (hit.kind === "embed") {
            focusEmbed(hit)
        }
    }

    // 双击 embed 的"进入"= 相机聚焦推进该子星图：仍然是同一张全局画布，
    // 不开新页面，也不切换当前星图身份。
    // Issue #832 评论 6013799805 / #373：交给 Canvas.focusEmbed 同时聚焦相机
    // 并推进焦点链，左上返回按钮据此 demote。
    function focusEmbed(hit) {
        if (!hit || !hit.owner)
            return
        canvas.focusEmbed(hit)
        var rect = hit.owner.itemSceneRect(hit.kind, hit.id)
        if (rect) {
            hit.owner.logInteraction("embed_focus", "embed", hit.id, {
                "sceneX": rect.x, "sceneY": rect.y,
                "width": rect.width, "height": rect.height
            })
        }
    }

    function handleRightClick(point) {
        if (!canvas || !ic)
            return
        if (ic.pointerMode === "pinch")
            return
        // Issue #834：armed 态下右键先取消 armed 再开菜单（不连环/连链接）。
        if (ic.linkArmed || ic.connectArmed)
            ic.cancelArmed()
        canvas.notePointerDevice(false)
        canvas.logPointerPress("right", "mouse", point)
        var hit = hitAt(point.position.x, point.position.y)
        if (!hit)
            return
        canvas.openHitContextMenu(hit, point.position.x, point.position.y)
    }

    // ── 拖动仲裁 ──
    // DragHandler 激活 = 已越过 Qt 拖动阈值。这里决定这次手势是 move / pan /
    // connect 的哪一条（长按先到的已经在 Timer 里提升为 connect/contextPending）。
    function handleDragActivated() {
        if (!canvas || !ic)
            return
        if (pinchActive)
            return
        // 任何真实拖动都取消"空白长按弹菜单"的候补。
        emptyLongPressArmed = false
        var mode = ic.pointerMode
        if (mode === "connect")
            return
        if (mode === "contextPending") {
            // 长按后继续移动超过阈值：关闭菜单视觉，切 connect。
            canvas.hideTouchPreview()
            if (ic.contextPendingToConnect()) {
                if (gestureOwner) {
                    gestureOwner.logInteraction("connect_begin", ic.connectFromKind, ic.connectFromId, {
                        "kind": ic.connectFromKind, "fromId": ic.connectFromId,
                        "fromX": ic.connectFromSceneX, "fromY": ic.connectFromSceneY
                    })
                    gestureOwner.refreshConnectPreview()
                }
            }
            return
        }
        if (mode === "pressPending") {
            if (ic.pointerSource === "touch") {
                // 未长按直接滑动：无论起点是不是 node/embed 都优先 pan。
                if (ic.pressPendingToPan()) {
                    panActive = true
                    beginPanLog()
                }
            } else if (gestureOwner && hasGestureTarget(pressHit)) {
                var item = gestureOwner.itemOf(pressHit.kind, pressHit.id)
                if (item && ic.pressPendingToMove(pressHit.scenePathKey, pressHit.kind,
                                                  pressHit.id, pressHit.targetPath,
                                                  item.x, item.y)) {
                    gestureOwner.logInteraction("move_begin", pressHit.kind, pressHit.id, {
                        "kind": pressHit.kind, "fromX": item.x, "fromY": item.y
                    })
                    gestureOwner.refreshEdges()
                }
            }
            return
        }
        if (mode === "idle" && emptyPressActive) {
            if (ic.beginPan()) {
                panActive = true
                beginPanLog()
            }
        }
    }

    function beginPanLog() {
        panBeginX = canvas.panX
        panBeginY = canvas.panY
        canvas.logInteraction("pan_begin", "empty", "", {
            "startPanX": canvas.panX,
            "startPanY": canvas.panY,
            "device": panDevice()
        })
    }

    function endPanWithLog() {
        if (!canvas || !ic)
            return
        ic.endPan()
        canvas.logInteraction("pan_end", "empty", "", {
            "startPanX": panBeginX,
            "startPanY": panBeginY,
            "endPanX": canvas.panX,
            "endPanY": canvas.panY,
            "device": panDevice()
        })
    }

    // 位移全部是原始 Qt scene 像素：阈值判断在状态机里，几何换算在各归属层。
    function handleDragDelta(dx, dy) {
        if (!canvas || !ic)
            return
        if (pinchActive)
            return
        var mode = ic.pointerMode
        if (mode === "move") {
            if (gestureOwner)
                gestureOwner.applyMoveDelta(dx, dy)
            return
        }
        if (mode === "connect") {
            if (gestureOwner)
                gestureOwner.applyConnectDelta(dx, dy)
            return
        }
        if (mode === "contextPending") {
            // 端点先按 root-world 增量累计；超阈值后切 connect（拖动手感阈值
            // 吃原始像素，两个口径分开）。
            if (gestureOwner)
                gestureOwner.applyConnectDelta(dx, dy)
            ic.noteDragDelta(dx, dy)
            if (ic.pressDragDistance > ic.moveThreshold) {
                canvas.hideTouchPreview()
                if (ic.contextPendingToConnect() && gestureOwner) {
                    gestureOwner.logInteraction("connect_begin", ic.connectFromKind, ic.connectFromId, {
                        "kind": ic.connectFromKind, "fromId": ic.connectFromId,
                        "fromX": ic.connectFromSceneX, "fromY": ic.connectFromSceneY
                    })
                    gestureOwner.refreshConnectPreview()
                }
            }
            return
        }
        if (mode === "pan" && panActive) {
            var cd = canvas.sceneDeltaToCanvas(dx, dy)
            canvas.panBy(cd.x, cd.y)
        }
    }

    // ── 松手统一出口：click / move / connect / pan 都在这里闭环 ──
    function handleRelease() {
        if (!canvas || !ic) {
            cancelLocalState()
            return
        }
        var mode = ic.pointerMode
        if (mode === "connect") {
            if (gestureOwner) gestureOwner.finishConnect()
            else ic.endConnect()
        } else if (mode === "move") {
            if (gestureOwner) gestureOwner.finishMove()
            else ic.endMove()
        } else if (mode === "contextPending") {
            if (gestureOwner) gestureOwner.finishContextPending()
            else {
                ic.endContextPending()
                canvas.hideTouchPreview()
            }
        } else if (mode === "pressPending") {
            ic.cancelPressPending()
        } else if (mode === "pan") {
            endPanWithLog()
        }
        // pinch 不属于单指业务：缩放结束由 PinchHandler 的 endPinch() 统一复位。
        cancelLocalState()
    }

    // ── 长按到点：唯一 Router 调状态提升 ──
    function handleLongPress() {
        if (!canvas || !ic)
            return
        if (pinchActive)
            return
        if (ic.pointerMode === "pressPending") {
            ic.pressTimerActive = false
            var hit = pressHit
            if (!hasGestureTarget(hit) || !gestureOwner)
                return
            var center = gestureOwner.itemCenterScene(hit.kind, hit.id)
            if (!center)
                return
            if (ic.pointerSource === "touch") {
                // 触屏长按：进入 contextPending（不移动弹菜单，移动转 connect）。
                if (ic.pressPendingToContextPending(center.x, center.y))
                    canvas.showTouchPreview(hit.kind, center.x, center.y)
            } else {
                // 鼠标长按：直接进入 connect。
                if (ic.pressPendingToConnect(hit.kind, hit.id, hit.targetPath, center.x, center.y)) {
                    gestureOwner.logInteraction("connect_begin", hit.kind, hit.id, {
                        "kind": hit.kind, "fromId": hit.id,
                        "fromX": center.x, "fromY": center.y
                    })
                    gestureOwner.refreshConnectPreview()
                }
            }
            return
        }
        if (emptyLongPressArmed && emptyPressActive) {
            emptyLongPressArmed = false
            var h = pressHit
            if (h && h.kind === "empty") {
                canvas.openBlankMenu(canvas.screenToWorldX(pressScreenX),
                                     canvas.screenToWorldY(pressScreenY),
                                     h, pressScreenX, pressScreenY)
            }
        }
    }

    // ── 长按计时：Timer 只能挂在 Item 下，就放在唯一 Router 里 ──
    Timer {
        id: longPressTimer
        interval: router.ic ? router.ic.longPressInterval : 800
        repeat: false
        running: (router.ic && router.ic.pressTimerActive) || router.emptyLongPressArmed
        onTriggered: router.handleLongPress()
    }

    // ── 左键按下/单击/双击（桌面指针）──
    // 默认 gesturePolicy=DragThreshold：press 只取 passive grab。
    // 内联编辑期间整个让位（disabled 的 Handler 不参与投递），
    // TextInput 才能收到点击/拖动去放置光标和选词。
    TapHandler {
        id: mouseLeftTap
        acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
        acceptedButtons: Qt.LeftButton
        enabled: !router.pinchActive && !router.editingBlocks
        onPressedChanged: {
            if (pressed)
                router.beginPress(mouseLeftTap.point, "mouse")
        }
        onSingleTapped: router.handleSingleTap(mouseLeftTap.point, "mouse")
        onDoubleTapped: router.handleDoubleTap(mouseLeftTap.point, "mouse")
    }

    // ── 左键按下/单击/双击（触屏）──
    // Pinch 激活期间直接禁用，让 passive grab 的 tap 识别当场取消；
    // 长按不走 TapHandler.longPressed，统一由上面的 Timer 仲裁。
    TapHandler {
        id: touchTap
        acceptedDevices: PointerDevice.TouchScreen
        acceptedButtons: Qt.LeftButton
        enabled: !router.pinchActive && !router.editingBlocks
        onPressedChanged: {
            if (pressed)
                router.beginPress(touchTap.point, "touch")
        }
        onSingleTapped: router.handleSingleTap(touchTap.point, "touch")
        onDoubleTapped: router.handleDoubleTap(touchTap.point, "touch")
    }

    // ── 右键：菜单归属由递归命中决定 ──
    TapHandler {
        id: mouseRightTap
        acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
        acceptedButtons: Qt.RightButton
        onSingleTapped: router.handleRightClick(mouseRightTap.point)
    }

    // ── 拖动：鼠标拖动进入 move/pan，触屏未长按滑动优先 pan ──
    // target: null —— 拖动不直接改任何 Item 几何，全部交给状态机 + 归属层。
    DragHandler {
        id: dragHandler
        target: null
        acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad | PointerDevice.TouchScreen
        acceptedButtons: Qt.LeftButton
        // 内联编辑时让位：把文本选区/光标拖拽留给 TextInput。
        enabled: !router.editingBlocks
        property real lastTx: 0
        property real lastTy: 0
        onActiveChanged: {
            if (active) {
                lastTx = 0
                lastTy = 0
                router.handleDragActivated()
            }
        }
        onActiveTranslationChanged: {
            var dx = activeTranslation.x - lastTx
            var dy = activeTranslation.y - lastTy
            lastTx = activeTranslation.x
            lastTy = activeTranslation.y
            router.handleDragDelta(dx, dy)
        }
    }

    // ── 左键 press→release 观察（passive grab）：松手统一出口 ──
    PointHandler {
        id: leftReleaseTracker
        acceptedButtons: Qt.LeftButton
        enabled: !router.editingBlocks
        onActiveChanged: {
            if (!active)
                router.handleRelease()
        }
    }

    // ── 捏合缩放：一激活就整体接管单指状态，作用在全局相机上 ──
    PinchHandler {
        id: pinchHandler
        target: null
        acceptedDevices: PointerDevice.TouchScreen
        property real _pinchStartZoom: 1.0
        onActiveChanged: {
            if (active) {
                canvas.resetInteraction()
                canvas.hideTouchPreview()
                router.cancelLocalState()
                router.ic.beginPinch()
                _pinchStartZoom = canvas.zoomLevel
                canvas.notePointerDevice(true)
            } else {
                router.ic.endPinch()
            }
        }
        onActiveScaleChanged: {
            canvas.zoomAt(centroid.position.x, centroid.position.y,
                          _pinchStartZoom * activeScale)
        }
    }

    // ── 滚轮：只改根相机 zoom，永远不改某个子图自己的尺寸状态 ──
    WheelHandler {
        id: wheelHandler
        target: null
        acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
        blocking: true
        onWheel: function(event) {
            if (!canvas)
                return
            canvas.notePointerDevice(false)
            var delta = event.angleDelta.y !== 0
                ? event.angleDelta.y / 120
                : event.pixelDelta.y / 120.0
            if (delta === 0)
                return
            var oldZoom = canvas.zoomLevel
            var newZoom = oldZoom * Math.pow(canvas._zoomFactor, delta)
            if (newZoom === oldZoom)
                return
            canvas.zoomAt(point.position.x, point.position.y, newZoom)
            canvas.logInteraction("zoom_wheel", "scene", canvas.starmapId, {
                "oldZoom": oldZoom,
                "newZoom": canvas.zoomLevel,
                "screenX": point.position.x,
                "screenY": point.position.y
            })
        }
    }

    // Issue #834：connectArmed（菜单"连线"发起）时没有左键按下，DragHandler 不激活，
    // 用 HoverHandler 观察鼠标移动更新预览终点。point.position 是 vector2d，绑定到
    // property 上，变化时刷新预览。linkArmed 不画预览（Link 不是语义边）。
    HoverHandler {
        id: connectArmedHover
        acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
        enabled: router.ic && router.ic.connectArmed
        property vector2d _hoverPos: point.position
        on_HoverPosChanged: {
            if (!router.ic || !router.ic.connectArmed || !router.canvas)
                return
            router.ic.connectMouseX = router.canvas.screenToWorldX(_hoverPos.x)
            router.ic.connectMouseY = router.canvas.screenToWorldY(_hoverPos.y)
            router.canvas.refreshConnectPreview()
        }
    }

    // Issue #834：armed 态下 Escape 取消（空白取消已在 handleSingleTap 处理）。
    // 用 Shortcut 全局拦截，不抢 TextInput 焦点；armed 时无 Popup 打开，不冲突。
    Shortcut {
        sequence: "Escape"
        enabled: router.ic && (router.ic.linkArmed || router.ic.connectArmed)
        onActivated: router.ic.cancelArmed()
    }
}
