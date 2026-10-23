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
//     - 右键在 node/embed 上按下并拖过阈值：直接进入 connect 拉线
//       （connect_begin 在日志里带 button=right）；松手命中另一个 node/embed
//       即建边，空白/自身取消；右键拖动不移动节点、不平移画布（Issue #930）
//     - connect 的"源锚点"与"指针终点"分开：锚点在源对象上（预览再贴边界），
//       终点每帧取当前指针的绝对位置（screen → 活动画布 world），不累计位移；
//       左键长按、触屏转 connect、右键拉线共用同一份 connect 几何
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
//   - 内联编辑只占用实际编辑框内的输入：按下点在编辑框内时，本次手势整体交给
//     编辑器；按下点在框外且存在编辑会话时，先同步提交草稿，成功后在同一个
//     手势内继续原操作，失败则本次手势 blocked 并保留编辑器。左右键各自记录
//     本次手势的仲裁结果，不在全局禁用 Handler。
// =============================================================================

import QtQuick
import QtQuick.Window

Item {
    id: router

    // 根 Canvas：唯一命中入口 / 相机 / 菜单宿主 / 预览 overlay 的宿主。
    required property var canvas

    readonly property var ic: canvas ? canvas.sharedInteraction : null
    readonly property bool pinchActive:
        pinchHandler.active || (ic && ic.pointerMode === "pinch")

    // Issue #987 / #989：一次物理按下 = 一条独立手势记录。左右键完全隔离，
    // 按下时重新分配序号与现场，绝不继承上一次的 blocked / pressPending。
    // 松手/取消时只复位本键现场；仲裁结果保留到下一次同键按下，因为
    // TapHandler.singleTapped / doubleTapped 可能晚于 release tracker 到达。
    //   blocked            — 本次手势归编辑器，或框外保存失败：不得执行任何业务。
    //   pointerInEditor    — 按下点在编辑框内：拖动类 Handler 不抢独占抓取。
    property int gestureSequence: 0
    property int leftGestureId: 0
    property int rightGestureId: 0
    property bool leftGestureFinished: true
    property bool rightGestureFinished: true
    // 本次左键手势是否真的接管了共享状态机；并发的右键手势不能被左键收尾抹掉。
    property bool leftGestureOwnsController: false
    // 当前真正持有共享状态机的按键。空按左键尚未 beginPress、随后右键命中时，
    // 单靠 leftGestureOwnsController 无法判断状态机已经转手，必须用归属标记。
    property string controllerGesture: ""
    property bool leftGestureBlocked: false
    property bool rightGestureBlocked: false
    property bool leftPointerInEditor: false
    property bool rightPointerInEditor: false
    // TapHandler 的点击候选：按下建立，取消/已处理时清除。pressed=false 只表示
    // TapHandler 因拖动阈值取消，不是物理松手，不能拿它结束业务手势。
    property bool leftTapCandidateValid: false
    property bool rightTapCandidateValid: false

    // ── 左键手势现场 ──
    // 按下时的完整递归命中（owner/scenePathKey/starmapId/kind/id/targetPath）。
    property var pressHit: null
    // 当前左键手势归属层（按下目标的 owner）。
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

    // ── 右键手势现场（Issue #930）──
    // 右键单击开菜单；按下后拖过阈值直接拉线。rightPressHit 是按下时的完整
    // 递归命中；rightWasDrag 标记本次右键已发生拖动，单击据此跳过菜单。
    // rightPressScreenX/Y 是本次右键按下的屏幕坐标（不与左键混用）。
    property bool rightPressActive: false
    property bool rightWasDrag: false
    property var rightPressHit: null
    property var rightGestureOwner: null
    property real rightPressScreenX: 0
    property real rightPressScreenY: 0

    function hitAt(sx, sy) {
        return canvas ? canvas.hitTargetAtScreen(sx, sy) : null
    }

    function hasGestureTarget(hit) {
        return !!hit && (hit.kind === "node" || hit.kind === "embed")
    }

    function pointInEditor(sx, sy) {
        return !!canvas && canvas.isInsideNodeEditor(sx, sy)
    }

    function pointInsideNodeEditor(point) {
        if (!point)
            return false
        return pointInEditor(point.position.x, point.position.y)
    }

    function hasActiveNodeEdit() {
        if (!canvas)
            return false
        if (typeof canvas.hasActiveNodeEdit === "function")
            return canvas.hasActiveNodeEdit() === true
        return canvas.inlineEditingKey !== ""
    }

    // Issue #987：每次业务 press 开始前的唯一编辑仲裁。
    //   "claimed" — 指针在实际编辑框内：本次手势交给编辑器。
    //   "blocked" — 指针在框外但保存失败：本次手势整体作废，编辑器保留草稿。
    //   "free"    — 无编辑会话，或框外保存成功：继续原有 press 流程。
    // 必须在 cancelViewportMotion / beginPress 等业务状态之前调用。
    function arbitrateEditorPress(point, button, gestureId) {
        if (!canvas)
            return "blocked"
        if (pointInsideNodeEditor(point)) {
            canvas.logInteraction("node_edit_pointer_claimed", "node", "", {
                "button": button,
                "gesture": gestureId,
                "screenX": point.position.x,
                "screenY": point.position.y
            })
            return "claimed"
        }
        if (!hasActiveNodeEdit())
            return "free"
        return canvas.commitNodeEditForOutsideGesture(point.position.x,
                                                       point.position.y,
                                                       button, gestureId) === true
                ? "free" : "blocked"
    }

    function panDevice() {
        return (ic && ic.pointerSource === "touch") ? "touch" : "mouse"
    }

    // 分配一条全新的手势序号；只清本键的现场，另一键完全不参与。
    function beginGesture(button) {
        // 兜底：上一次同键物理松手若还没走 Qt.callLater 兜底，先按正常松手
        // 收尾，绝不让新按下继承旧 controller 状态。
        if (button === "right") {
            if (!rightGestureFinished)
                finishRightGesture(false, "replaced")
        } else if (!leftGestureFinished) {
            finishLeftGesture(false, "replaced")
        }
        gestureSequence += 1
        if (button === "right") {
            rightGestureId = gestureSequence
            rightGestureFinished = false
            rightGestureBlocked = false
            rightPointerInEditor = false
            rightPressActive = false
            rightWasDrag = false
            rightPressHit = null
            rightGestureOwner = null
            rightTapCandidateValid = true
        } else {
            leftGestureId = gestureSequence
            leftGestureFinished = false
            leftGestureOwnsController = false
            leftGestureBlocked = false
            leftPointerInEditor = false
            leftTapCandidateValid = true
            pressHit = null
            gestureOwner = null
            emptyPressActive = false
            emptyLongPressArmed = false
            panActive = false
        }
        return gestureSequence
    }

    function resetLeftLocalState() {
        leftGestureOwnsController = false
        if (controllerGesture === "left")
            controllerGesture = ""
        pressHit = null
        gestureOwner = null
        emptyPressActive = false
        emptyLongPressArmed = false
        panActive = false
    }

    function resetRightLocalState() {
        if (controllerGesture === "right")
            controllerGesture = ""
        rightPressActive = false
        rightPressHit = null
        rightGestureOwner = null
    }

    // 兼容既有调用点：只清现场，不结束任何正在进行的手势。
    function cancelLocalState() {
        resetLeftLocalState()
        resetRightLocalState()
    }

    // ── 按下：只登记命中与归属，不决定 move/connect/pan ──
    function beginPress(point, source) {
        if (!canvas || !ic)
            return
        if (ic.pointerMode === "pinch")
            return
        beginGesture("left")
        // 一次物理按下只在这里记录一次 pointer_press；后续 release/cancel 用同一序号。
        canvas.logPointerPress("left", source, point, leftGestureId)
        // 编辑仲裁必须先于任何业务 press 状态；框外保存成功后继续本次手势。
        var verdict = arbitrateEditorPress(point, "left", leftGestureId)
        if (verdict !== "free") {
            leftGestureBlocked = true
            leftPointerInEditor = verdict === "claimed"
            resetLeftLocalState()
            return
        }
        canvas.notePointerDevice(source === "touch")
        // Issue #834 复核：armed 态期间下一次左键只负责选 target，
        // 不参与普通 pressPending / move / 长按连线。
        // linkArmed 时 pointerMode 是 idle，不拦会重新进 pressPending，
        // 长按甚至被提升成 Edge connect，两个模式会串。
        // connectArmed 时 pointerMode 是 connect，release 会提前 endConnect。
        if (ic.linkArmed || ic.connectArmed)
            return
        // Issue #949：新一次按下先让过渡演出落到最终状态，命中与业务都按
        // 最新 Core route + 看得见的那一帧算（旧动画不再把画面拉回去）。
        canvas.cancelViewportMotion()
        pressScreenX = point.position.x
        pressScreenY = point.position.y
        var hit = hitAt(pressScreenX, pressScreenY)
        pressHit = hit
        emptyPressActive = false
        emptyLongPressArmed = false

        if (ic.pointerMode !== "idle") {
            // 右键菜单"移动"已经确定了目标：只有按在同一个目标上才是这次 move 的
            // 继续，其它对象上的按下不改变任何状态。并发的右键 connect 等状态
            // 不能被本次左键收尾误结束。
            if (ic.pointerMode === "move" && hasGestureTarget(hit)
                    && ic.isMovingTarget(hit.scenePathKey, hit.kind, hit.id)) {
                gestureOwner = hit.owner
                leftGestureOwnsController = true
                controllerGesture = "left"
            }
            return
        }

        if (hasGestureTarget(hit)) {
            gestureOwner = hit.owner
            ic.pointerSource = source
            leftGestureOwnsController =
                    ic.beginPress(hit.kind, hit.id, hit.targetPath, hit.scenePathKey,
                                  canvas.screenToWorldX(pressScreenX),
                                  canvas.screenToWorldY(pressScreenY))
            if (leftGestureOwnsController)
                controllerGesture = "left"
            return
        }

        // 空白 / 连线：等拖动阈值决定 pan；松手走点击语义。
        gestureOwner = null
        emptyPressActive = true
        emptyLongPressArmed = (source === "touch")
        leftGestureOwnsController = true
        controllerGesture = "left"
    }

    // ── 右键按下（Issue #930）：复用左键状态机，但不启用长按计时 ──
    // 右键点击继续走 handleRightClick 开菜单；真拖动越过阈值由
    // rightConnectDrag 调 beginRightConnectDrag() 提升成 connect。
    function beginRightPress(point) {
        if (!canvas || !ic)
            return
        // 新一次按下分配新序号并清掉上一次的拖动标记：单击只按本次手势
        // 判断是否开菜单，绝不继承旧的 blocked / rightWasDrag。
        beginGesture("right")
        canvas.logPointerPress("right", "mouse", point, rightGestureId)
        // Issue #987：右键同样先做编辑区域仲裁，再决定是否进入 connect 候选。
        var verdict = arbitrateEditorPress(point, "right", rightGestureId)
        if (verdict !== "free") {
            rightGestureBlocked = true
            rightPointerInEditor = verdict === "claimed"
            resetRightLocalState()
            return
        }
        // armed 态下右键由 TapHandler.singleTapped 取消 armed 并开菜单。
        if (ic.linkArmed || ic.connectArmed)
            return
        if (ic.pointerMode !== "idle")
            return
        canvas.cancelViewportMotion()

        rightPressScreenX = point.position.x
        rightPressScreenY = point.position.y
        var hit = hitAt(rightPressScreenX, rightPressScreenY)
        rightPressHit = hit
        rightPressActive = true

        if (hasGestureTarget(hit)) {
            rightGestureOwner = hit.owner
            if (ic.beginPress(hit.kind, hit.id, hit.targetPath, hit.scenePathKey,
                              canvas.screenToWorldX(point.position.x),
                              canvas.screenToWorldY(point.position.y))) {
                ic.pointerSource = "mouse"
                controllerGesture = "right"
                // 右键已接管状态机：左键随后松手不得再补一次点击选中。
                leftTapCandidateValid = false
                // 右拖不需要等"长按"时限：拖动阈值一到就走 connect，
                // 不拖则松手仍是普通点击（菜单）。
                ic.pressTimerActive = false
            }
        }
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
        // Issue #987：本次手势已归编辑器或保存失败，点击不得落到节点/画布上。
        if (leftGestureBlocked)
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
        // Issue #987：编辑器认领的手势不做双击编辑 / 进入 Embed。
        if (leftGestureBlocked)
            return
        var hit = hitAt(point.position.x, point.position.y)
        if (!hit)
            return
        if (hit.kind === "node") {
            // 同一手势序号贯穿 node_edit_begin / ready / commit。
            hit.owner.beginInlineEdit(hit.id, leftGestureId, "left")
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
        // Issue #987：本次右键已归编辑器或保存失败，不再打开画布菜单。
        if (rightGestureBlocked || pointInsideNodeEditor(point))
            return
        // Issue #834：armed 态下右键先取消 armed 再开菜单（不连环/连链接）。
        if (ic.linkArmed || ic.connectArmed)
            ic.cancelArmed()
        canvas.notePointerDevice(false)
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
        if (leftGestureFinished || leftGestureBlocked)
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
                    updateConnectEndpointFromScreen(dragHandler.centroid.position.x,
                                                    dragHandler.centroid.position.y)
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
        if (leftGestureFinished || leftGestureBlocked)
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
            // 端点 = 当前指针绝对位置（不累计位移）：任意 zoom 下都不漂移。
            updateConnectEndpointFromScreen(dragHandler.centroid.position.x,
                                            dragHandler.centroid.position.y)
            return
        }
        if (mode === "contextPending") {
            // 阈值继续吃原始 Qt scene 像素；端点不预存增量，转 connect 时
            // 直接取当前指针绝对位置（触屏与鼠标同一份 connect 几何）。
            ic.noteDragDelta(dx, dy)
            if (ic.pressDragDistance > ic.moveThreshold) {
                canvas.hideTouchPreview()
                if (ic.contextPendingToConnect() && gestureOwner) {
                    gestureOwner.logInteraction("connect_begin", ic.connectFromKind, ic.connectFromId, {
                        "kind": ic.connectFromKind, "fromId": ic.connectFromId,
                        "fromX": ic.connectFromSceneX, "fromY": ic.connectFromSceneY
                    })
                    updateConnectEndpointFromScreen(dragHandler.centroid.position.x,
                                                    dragHandler.centroid.position.y)
                }
            }
            return
        }
        if (mode === "pan" && panActive) {
            var cd = canvas.sceneDeltaToCanvas(dx, dy)
            canvas.panBy(cd.x, cd.y)
        }
    }

    // ── 右键越阈值（Issue #930）：直接进 connect，不走 move / pan ──
    // 起点必须是有手势目标的 node/embed（edge/空白什么都不做）；
    // 端点身份与左键长按连线完全同源：pressPendingToConnect + 归属层预览刷新。
    function beginRightConnectDrag() {
        if (!rightPressActive || rightGestureBlocked)
            return
        rightWasDrag = true
        if (!hasGestureTarget(rightPressHit) || !rightGestureOwner)
            return
        var h = rightPressHit
        var owner = rightGestureOwner
        var center = owner.itemCenterScene(h.kind, h.id)
        if (!center)
            return
        if (ic.pressPendingToConnect(h.kind, h.id, h.targetPath,
                                     center.x, center.y)) {
            owner.logInteraction("connect_begin", h.kind, h.id, {
                "kind": h.kind, "fromId": h.id,
                "fromX": center.x, "fromY": center.y,
                "button": "right",
                "gesture": rightGestureId
            })
            // 节点中心只作源锚点；终点立刻落回本次右键按下的真实指针位置，
            // 之后每帧由 updateConnectEndpointFromScreen() 跟随指针。
            updateConnectEndpointFromScreen(rightPressScreenX, rightPressScreenY, owner)
        }
    }

    // ── connect 端点几何：源锚点与指针终点分开，唯一一份 ──
    // connectFromSceneX/Y 是源对象上的锚点（refreshConnectPreview 再按指针方向贴到
    // 可见边界）；终点永远取当前指针的绝对位置：screen → 当前活动画布 world。
    // 不累计位移，所以任意 zoom 下终点都等于真实指针，松手 finishConnect 的命中
    // 不会带源节点中心/按下点的偏移；左键长按、触屏转 connect、右键拉线共用它。
    function updateConnectEndpointFromScreen(sx, sy, owner) {
        var target = owner !== undefined ? owner : gestureOwner
        if (!canvas || !ic || !target || ic.pointerMode !== "connect")
            return
        ic.updateConnect(canvas.screenToWorldX(sx), canvas.screenToWorldY(sy))
        target.refreshConnectPreview()
    }

    // ── 手势边界日志：press / grab / release / cancel 共用同一序号 ──
    function logGestureBoundary(event, button, gestureId, mode, reason, blocked,
                                editorOwned) {
        if (!canvas)
            return
        canvas.logInteraction(event, "gesture", "", {
            "button": button,
            "gesture": gestureId,
            "mode": mode,
            "reason": reason,
            "blocked": blocked,
            "editorOwned": editorOwned
        })
    }

    // ── 左键唯一收尾出口：正常松手与异常取消都必须走这里，且只执行一次 ──
    // 正常 release：connect/move/context 走业务收尾，pressPending/pan 正常结束。
    // 异常 cancel：只调对应的 end*/cancel*，绝不落边、不开菜单、不提交 move。
    function finishLeftGesture(cancelled, reason) {
        if (leftGestureFinished)
            return
        leftGestureFinished = true
        if (cancelled)
            leftTapCandidateValid = false
        if (!canvas || !ic) {
            resetLeftLocalState()
            return
        }
        var mode = ic.pointerMode
        var blocked = leftGestureBlocked
        var armed = ic.linkArmed || ic.connectArmed
        var owner = gestureOwner
        logGestureBoundary(cancelled ? "pointer_cancel" : "pointer_release",
                           "left", leftGestureId, mode, reason, blocked,
                           leftPointerInEditor)
        // 编辑器认领 / 框外保存失败 / armed 选择 / pinch / 并发的右键手势：
        // 本次左键没有接管共享状态机，收尾时只清自己的现场。
        if (blocked || armed || mode === "pinch"
                || !leftGestureOwnsController || controllerGesture !== "left") {
            resetLeftLocalState()
            return
        }
        if (cancelled) {
            if (mode === "connect") {
                ic.endConnect()
                canvas.hideTouchPreview()
            } else if (mode === "move") {
                ic.endMove()
                if (owner)
                    owner.refreshEdges()
            } else if (mode === "contextPending") {
                ic.endContextPending()
                canvas.hideTouchPreview()
            } else if (mode === "pressPending") {
                ic.cancelPressPending()
            } else if (mode === "pan") {
                ic.endPan()
            }
        } else if (mode === "connect") {
            if (owner) owner.finishConnect()
            else ic.endConnect()
        } else if (mode === "move") {
            if (owner) owner.finishMove()
            else ic.endMove()
        } else if (mode === "contextPending") {
            if (owner) owner.finishContextPending()
            else {
                ic.endContextPending()
                canvas.hideTouchPreview()
            }
        } else if (mode === "pressPending") {
            ic.cancelPressPending()
        } else if (mode === "pan") {
            endPanWithLog()
        }
        resetLeftLocalState()
    }

    function handleRelease() {
        finishLeftGesture(false, "release")
    }

    function handleLeftCancel(reason) {
        finishLeftGesture(true, reason)
    }

    // ── 右键唯一收尾出口（Issue #930 / #989）──
    // 真拖动：finishConnect() 按既有出口递归命中解析目标并落边；未拖动：
    // pressPending 直接取消，菜单交给同一手势的 TapHandler.singleTapped。
    // rightWasDrag 保留到 next press；tap 候选在异常取消时作废。
    function finishRightGesture(cancelled, reason) {
        if (rightGestureFinished)
            return
        rightGestureFinished = true
        if (cancelled)
            rightTapCandidateValid = false
        if (!canvas || !ic) {
            resetRightLocalState()
            return
        }
        var mode = ic.pointerMode
        var blocked = rightGestureBlocked
        var armed = ic.linkArmed || ic.connectArmed
        var owner = rightGestureOwner
        logGestureBoundary(cancelled ? "pointer_cancel" : "pointer_release",
                           "right", rightGestureId, mode, reason, blocked,
                           rightPointerInEditor)
        if (!rightPressActive || blocked || armed || mode === "pinch"
                || controllerGesture !== "right") {
            resetRightLocalState()
            return
        }
        if (cancelled) {
            if (mode === "connect" && rightWasDrag) {
                ic.endConnect()
                canvas.hideTouchPreview()
            } else if (mode === "pressPending") {
                ic.cancelPressPending()
            }
        } else if (rightWasDrag && mode === "connect") {
            if (owner) owner.finishConnect()
            else ic.endConnect()
        } else if (mode === "pressPending") {
            ic.cancelPressPending()
        }
        resetRightLocalState()
    }

    function endRightPress() {
        finishRightGesture(false, "release")
    }

    function endRightPressCanceled(reason) {
        finishRightGesture(true, reason)
    }

    // 窗口失焦 / 切画布 / resetInteraction 的唯一取消入口。
    function cancelAllGestures(reason) {
        leftTapCandidateValid = false
        rightTapCandidateValid = false
        finishLeftGesture(true, reason)
        finishRightGesture(true, reason)
        resetLeftLocalState()
        resetRightLocalState()
    }

    function grabTransitionName(transition) {
        switch (transition) {
        case PointerDevice.GrabPassive: return "grab_passive"
        case PointerDevice.UngrabPassive: return "ungrab_passive"
        case PointerDevice.CancelGrabPassive: return "cancel_grab_passive"
        case PointerDevice.OverrideGrabPassive: return "override_grab_passive"
        case PointerDevice.GrabExclusive: return "grab_exclusive"
        case PointerDevice.UngrabExclusive: return "ungrab_exclusive"
        case PointerDevice.CancelGrabExclusive: return "cancel_grab_exclusive"
        default: return "unknown"
        }
    }

    // PointHandler 同时给 onActiveChanged / onCanceled / grabChanged 三条路：
    // Cancel* -> 异常取消；Ungrab* -> 正常结束。共同的 finished 标记保证只执行一次。
    function handlePointerGrab(button, transition) {
        if (!canvas)
            return
        var isLeft = button === "left"
        canvas.logInteraction("pointer_grab_transition", "gesture", "", {
            "button": button,
            "gesture": isLeft ? leftGestureId : rightGestureId,
            "transition": grabTransitionName(transition),
            "mode": ic ? ic.pointerMode : ""
        })
        if (transition === PointerDevice.CancelGrabPassive
                || transition === PointerDevice.CancelGrabExclusive) {
            if (isLeft)
                handleLeftCancel("grab_cancel")
            else
                endRightPressCanceled("grab_cancel")
        } else if (transition === PointerDevice.UngrabPassive
                   || transition === PointerDevice.UngrabExclusive) {
            if (isLeft)
                handleRelease()
            else
                endRightPress()
        }
    }

    // ── 长按到点：唯一 Router 调状态提升 ──
    function handleLongPress() {
        if (!canvas || !ic)
            return
        if (leftGestureFinished || leftGestureBlocked)
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
                // 鼠标长按：直接进入 connect。源锚点是节点中心，终点立刻落回
                // 本次按下的真实指针位置，之后每帧跟随指针。
                if (ic.pressPendingToConnect(hit.kind, hit.id, hit.targetPath, center.x, center.y)) {
                    gestureOwner.logInteraction("connect_begin", hit.kind, hit.id, {
                        "kind": hit.kind, "fromId": hit.id,
                        "fromX": center.x, "fromY": center.y
                    })
                    updateConnectEndpointFromScreen(pressScreenX, pressScreenY)
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
    // Issue #987：不在编辑会话期间整体禁用；由 beginPress 的编辑仲裁决定本次
    // 手势归属。编辑框内按下时只让 DragHandler 让位，TapHandler 仍保持 passive，
    // 文本控件因此能收到点击、选字与拖动。
    TapHandler {
        id: mouseLeftTap
        acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
        acceptedButtons: Qt.LeftButton
        enabled: !router.pinchActive
        onPressedChanged: {
            // pressed=false 只是 TapHandler 自己退出（阈值/取消），不是物理松手；
            // 真正的 release 只由 passive PointHandler 收尾。
            if (pressed)
                router.beginPress(mouseLeftTap.point, "mouse")
            else
                router.leftPointerInEditor = false
        }
        onCanceled: router.leftTapCandidateValid = false
        onSingleTapped: function(eventPoint) {
            if (!router.leftTapCandidateValid || router.leftGestureBlocked)
                return
            // Qt 的双击会先发 singleTapped 再发 doubleTapped；这里不能清候选，
            // 否则 doubleTapped 会被自己的 guard 拦掉。
            router.handleSingleTap(eventPoint, "mouse")
        }
        onDoubleTapped: function(eventPoint) {
            if (!router.leftTapCandidateValid || router.leftGestureBlocked)
                return
            router.leftTapCandidateValid = false
            router.handleDoubleTap(eventPoint, "mouse")
        }
    }

    // ── 左键按下/单击/双击（触屏）──
    // Pinch 激活期间直接禁用，让 passive grab 的 tap 识别当场取消；
    // 长按不走 TapHandler.longPressed，统一由上面的 Timer 仲裁。
    TapHandler {
        id: touchTap
        acceptedDevices: PointerDevice.TouchScreen
        acceptedButtons: Qt.LeftButton
        enabled: !router.pinchActive
        onPressedChanged: {
            if (pressed)
                router.beginPress(touchTap.point, "touch")
            else
                router.leftPointerInEditor = false
        }
        onCanceled: router.leftTapCandidateValid = false
        onSingleTapped: function(eventPoint) {
            if (!router.leftTapCandidateValid || router.leftGestureBlocked)
                return
            // 同上：单击不能提前使双击候选失效。
            router.handleSingleTap(eventPoint, "touch")
        }
        onDoubleTapped: function(eventPoint) {
            if (!router.leftTapCandidateValid || router.leftGestureBlocked)
                return
            router.leftTapCandidateValid = false
            router.handleDoubleTap(eventPoint, "touch")
        }
    }

    // ── 右键（Issue #930）：单击开菜单；按下后拖过阈值 = 直接拉线 ──
    // 菜单归属仍由递归命中决定；右键拖动完成后由 rightWasDrag 抑制菜单
    // （TapHandler 是 DragThreshold 策略，真拖动本身也会取消 tap）。
    TapHandler {
        id: mouseRightTap
        acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
        acceptedButtons: Qt.RightButton
        onPressedChanged: {
            if (pressed)
                router.beginRightPress(mouseRightTap.point)
            else
                router.rightPointerInEditor = false
        }
        onCanceled: router.rightTapCandidateValid = false
        onSingleTapped: function(eventPoint) {
            if (!router.rightTapCandidateValid
                    || router.rightGestureBlocked || router.rightWasDrag)
                return
            router.rightTapCandidateValid = false
            router.handleRightClick(eventPoint)
        }
    }

    // ── 拖动：鼠标拖动进入 move/pan，触屏未长按滑动优先 pan ──
    // target: null —— 拖动不直接改任何 Item 几何，全部交给状态机 + 归属层。
    DragHandler {
        id: dragHandler
        target: null
        acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad | PointerDevice.TouchScreen
        acceptedButtons: Qt.LeftButton
        // Issue #989：Handler 始终保持稳定启用，不能在手势中途改 enabled 破坏
        // grab 生命周期。按下点的归属已由编辑仲裁写入本次 blocked；并且不主动
        // 从 Item（TextInput/TextEdit）手里抢独占抓取。
        grabPermissions: PointerHandler.ApprovesTakeOverByItems
                          | PointerHandler.ApprovesTakeOverByHandlersOfDifferentType
        property real lastTx: 0
        property real lastTy: 0
        onActiveChanged: {
            if (active) {
                if (router.leftGestureBlocked)
                    return
                lastTx = 0
                lastTy = 0
                router.handleDragActivated()
                // 长按已经进 connect 时，激活当帧就把端点对齐指针：单次大位移
                // 后立刻松手也不留下"只到按下点"的偏移（非 connect 时无操作）。
                router.updateConnectEndpointFromScreen(dragHandler.centroid.position.x,
                                                       dragHandler.centroid.position.y)
            }
        }
        onActiveTranslationChanged: {
            if (!active || router.leftGestureBlocked)
                return
            var dx = activeTranslation.x - lastTx
            var dy = activeTranslation.y - lastTy
            lastTx = activeTranslation.x
            lastTy = activeTranslation.y
            router.handleDragDelta(dx, dy)
        }
    }

    // ── 右键拖动（Issue #930）：单独的 Handler，不混进左键 dragHandler ──
    // 越阈值只进入 connect 预览（beginRightConnectDrag），绝不进入
    // pressPendingToMove / beginPan，所以右键拖动既不移动节点也不平移画布。
    // 每帧用 centroid 的绝对指针位置刷新端点（与左键 connect 同一份几何）。
    DragHandler {
        id: rightConnectDrag
        target: null
        acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
        acceptedButtons: Qt.RightButton
        grabPermissions: PointerHandler.ApprovesTakeOverByItems
                          | PointerHandler.ApprovesTakeOverByHandlersOfDifferentType
        onActiveChanged: {
            if (active) {
                if (router.rightGestureBlocked)
                    return
                router.beginRightConnectDrag()
                // 激活当帧端点立即对齐指针（按下点种子只兜住随后的零位移场景）。
                router.updateConnectEndpointFromScreen(rightConnectDrag.centroid.position.x,
                                                       rightConnectDrag.centroid.position.y,
                                                       router.rightGestureOwner)
            }
        }
        onActiveTranslationChanged: {
            if (!active || router.rightGestureBlocked)
                return
            router.updateConnectEndpointFromScreen(rightConnectDrag.centroid.position.x,
                                                   rightConnectDrag.centroid.position.y,
                                                   router.rightGestureOwner)
        }
    }

    // ── 左键 press→release 观察（passive grab）：松手统一出口 ──
    PointHandler {
        id: leftReleaseTracker
        acceptedButtons: Qt.LeftButton
        // active=false 只做兜底，延后一拍；同步的 canceled/grabChanged 先到时不重复。
        onActiveChanged: {
            if (!active) {
                var gid = router.leftGestureId
                Qt.callLater(function() {
                    if (router.leftGestureId === gid)
                        router.handleRelease()
                })
            }
        }
        onCanceled: router.handleLeftCancel("point_canceled")
        onGrabChanged: function(transition, point) {
            router.handlePointerGrab("left", transition)
        }
    }

    // ── 右键 press→release 观察（passive grab）：拖动收尾 / 点击判定 ──
    // 拖动时 finishConnect 落边；未拖动时只清 pressPending，菜单由
    // 同一手势的 TapHandler.singleTapped 处理。
    PointHandler {
        id: rightReleaseTracker
        acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
        acceptedButtons: Qt.RightButton
        onActiveChanged: {
            if (!active) {
                var gid = router.rightGestureId
                Qt.callLater(function() {
                    if (router.rightGestureId === gid)
                        router.endRightPress()
                })
            }
        }
        onCanceled: router.endRightPressCanceled("point_canceled")
        onGrabChanged: function(transition, point) {
            router.handlePointerGrab("right", transition)
        }
    }

    // ── 捏合缩放：一激活就整体接管单指状态，作用在全局相机上 ──
    PinchHandler {
        id: pinchHandler
        target: null
        acceptedDevices: PointerDevice.TouchScreen
        property real _pinchStartZoom: 1.0
        // Issue #987：捏合中心在实际编辑框内时归编辑器，不用双指缩放画布。
        property bool _editorOwnsPinch: false
        onActiveChanged: {
            if (active) {
                _editorOwnsPinch = router.pointInEditor(pinchHandler.centroid.position.x,
                                                        pinchHandler.centroid.position.y)
                router.leftPointerInEditor = false
                router.rightPointerInEditor = false
                // 双指接管也走统一取消入口；随后 reset 只是状态机复位。
                router.cancelAllGestures("pinch")
                canvas.resetInteraction()
                canvas.hideTouchPreview()
                if (_editorOwnsPinch)
                    return
                router.ic.beginPinch()
                _pinchStartZoom = canvas.zoomLevel
                canvas.notePointerDevice(true)
            } else {
                if (!_editorOwnsPinch)
                    router.ic.endPinch()
                _editorOwnsPinch = false
            }
        }
        onActiveScaleChanged: {
            if (_editorOwnsPinch)
                return
            canvas.zoomAt(centroid.position.x, centroid.position.y,
                          _pinchStartZoom * activeScale)
        }
    }

    // ── 滚轮：只改当前活动画布的相机 zoom，永远不改某个子图自己的尺寸状态 ──
    // Issue #949：滚轮一档只更新"累计目标"（下一档从上一档的目标继续乘），
    // 显示镜头由 Canvas 的单一补间连续追上去；连续滚动不会重起一堆互相
    // 打断的补间。
    WheelHandler {
        id: wheelHandler
        target: null
        acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
        // 不抢在编辑器/正文 Flickable 之前吞掉滚轮；是否缩放由 accepted 明确决定。
        blocking: false
        onWheel: function(event) {
            if (!canvas) {
                event.accepted = false
                return
            }
            // Issue #987 / #989：编辑框内的滚轮归编辑器正文；这里不 accept，
            // 让 bodyFlick 的 WheelHandler 继续滚动。框外仍缩放画布。
            if (router.pointInEditor(point.position.x, point.position.y)) {
                event.accepted = false
                return
            }
            canvas.notePointerDevice(false)
            var delta = event.angleDelta.y !== 0
                ? event.angleDelta.y / 120
                : event.pixelDelta.y / 120.0
            if (delta === 0) {
                event.accepted = false
                return
            }
            var oldZoom = canvas.zoomTargetLevel
            var newZoom = oldZoom * Math.pow(canvas._zoomFactor, delta)
            if (newZoom === oldZoom) {
                event.accepted = false
                return
            }
            canvas.zoomAt(point.position.x, point.position.y, newZoom)
            event.accepted = true
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

    // 窗口失活（切到别的应用）时，物理 release 可能永远不会送达：
    // 用同一个取消入口清掉左右手势与控制器状态，下一次按下重新分配现场。
    Connections {
        target: Window.window
        function onActiveChanged() {
            if (!Window.window || !Window.window.active)
                router.cancelAllGestures("window_inactive")
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
