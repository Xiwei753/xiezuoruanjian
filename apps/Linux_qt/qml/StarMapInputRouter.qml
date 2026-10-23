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

Item {
    id: router

    // 根 Canvas：唯一命中入口 / 相机 / 菜单宿主 / 预览 overlay 的宿主。
    required property var canvas

    readonly property var ic: canvas ? canvas.sharedInteraction : null
    readonly property bool pinchActive:
        pinchHandler.active || (ic && ic.pointerMode === "pinch")

    // Issue #987：本次具体指针手势的编辑仲裁结果。左右键完全隔离，按下时决定，
    // 持续到下一次同键按下；松手/取消时只复位本手势的现场，不清仲裁结果，
    // 因为 TapHandler.singleTapped 可能晚于 release tracker 到达。
    //   blocked            — 本次手势归编辑器，或框外保存失败：不得执行任何业务。
    //   pointerInEditor    — 按下点在编辑框内：拖动类 Handler 立即让位，避免取独占抓取。
    property bool leftGestureBlocked: false
    property bool rightGestureBlocked: false
    property bool leftPointerInEditor: false
    property bool rightPointerInEditor: false

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

    // 右键手势现场（Issue #930）：右键单击开菜单；按下后拖过阈值直接拉线。
    // rightPressHit 是按下时的完整递归命中，拖动激活时据此进入 connect；
    // rightWasDrag 标记本次右键已发生拖动，TapHandler.singleTapped 据此跳过菜单。
    // rightPressScreenX/Y 是本次右键按下的屏幕坐标（不与左键 pressScreenX/Y 混用）：
    // connect 起点种子的真实指针位置。
    property bool rightPressActive: false
    property bool rightWasDrag: false
    property var rightPressHit: null
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
        return !!canvas && canvas.inlineEditingKey !== ""
    }

    // Issue #987：每次业务 press 开始前的唯一编辑仲裁。
    //   "claimed" — 指针在实际编辑框内：本次手势交给编辑器。
    //   "blocked" — 指针在框外但保存失败：本次手势整体作废，编辑器保留草稿。
    //   "free"    — 无编辑会话，或框外保存成功：继续原有 press 流程。
    // 必须在 cancelViewportMotion / beginPress 等业务状态之前调用。
    function arbitrateEditorPress(point, button) {
        if (!canvas)
            return "blocked"
        if (pointInsideNodeEditor(point)) {
            canvas.logInteraction("node_edit_pointer_claimed", "node", "", {
                "button": button,
                "screenX": point.position.x,
                "screenY": point.position.y
            })
            return "claimed"
        }
        if (!hasActiveNodeEdit())
            return "free"
        return canvas.commitNodeEditForOutsideGesture(point.position.x,
                                                       point.position.y) === true
                ? "free" : "blocked"
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
        leftGestureBlocked = false
        leftPointerInEditor = false
        // 编辑仲裁必须先于任何业务 press 状态；框外保存成功后继续本次手势。
        var verdict = arbitrateEditorPress(point, "left")
        if (verdict !== "free") {
            leftGestureBlocked = true
            leftPointerInEditor = verdict === "claimed"
            cancelLocalState()
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

    // ── 右键按下（Issue #930）：复用左键状态机，但不启用长按计时 ──
    // 右键点击继续走 handleRightClick 开菜单；真拖动越过阈值由
    // rightConnectDrag 调 beginRightConnectDrag() 提升成 connect。
    function beginRightPress(point) {
        // 新一次按下先清掉上一次的拖动标记：TapHandler.singleTapped 只按本次
        // 手势判断是否开菜单。armed / 菜单 move 等 guard 提前返回时也要清，
        // 否则旧标记会压掉 #834 的"右键取消 armed 再开菜单"路径。
        rightWasDrag = false
        rightGestureBlocked = false
        rightPointerInEditor = false
        rightPressHit = null
        if (!canvas || !ic)
            return
        // Issue #987：右键同样先做编辑区域仲裁，再决定是否进入 connect 候选。
        var verdict = arbitrateEditorPress(point, "right")
        if (verdict !== "free") {
            rightGestureBlocked = true
            rightPointerInEditor = verdict === "claimed"
            cancelLocalState()
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
        canvas.logPointerPress("right", "mouse", point)

        if (hasGestureTarget(hit)) {
            gestureOwner = hit.owner
            if (ic.beginPress(hit.kind, hit.id, hit.targetPath, hit.scenePathKey,
                              canvas.screenToWorldX(point.position.x),
                              canvas.screenToWorldY(point.position.y))) {
                ic.pointerSource = "mouse"
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
        // Issue #987：本次右键已归编辑器或保存失败，不再打开画布菜单。
        if (rightGestureBlocked || pointInsideNodeEditor(point))
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
        if (leftGestureBlocked)
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
        if (leftGestureBlocked)
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
        if (!hasGestureTarget(rightPressHit) || !gestureOwner)
            return
        var h = rightPressHit
        var center = gestureOwner.itemCenterScene(h.kind, h.id)
        if (!center)
            return
        if (ic.pressPendingToConnect(h.kind, h.id, h.targetPath,
                                     center.x, center.y)) {
            gestureOwner.logInteraction("connect_begin", h.kind, h.id, {
                "kind": h.kind, "fromId": h.id,
                "fromX": center.x, "fromY": center.y,
                "button": "right"
            })
            // 节点中心只作源锚点；终点立刻落回本次右键按下的真实指针位置，
            // 之后每帧由 updateConnectEndpointFromScreen() 跟随指针。
            updateConnectEndpointFromScreen(rightPressScreenX, rightPressScreenY)
        }
    }

    // ── connect 端点几何：源锚点与指针终点分开，唯一一份 ──
    // connectFromSceneX/Y 是源对象上的锚点（refreshConnectPreview 再按指针方向贴到
    // 可见边界）；终点永远取当前指针的绝对位置：screen → 当前活动画布 world。
    // 不累计位移，所以任意 zoom 下终点都等于真实指针，松手 finishConnect 的命中
    // 不会带源节点中心/按下点的偏移；左键长按、触屏转 connect、右键拉线共用它。
    function updateConnectEndpointFromScreen(sx, sy) {
        if (!canvas || !ic || !gestureOwner || ic.pointerMode !== "connect")
            return
        ic.updateConnect(canvas.screenToWorldX(sx), canvas.screenToWorldY(sy))
        gestureOwner.refreshConnectPreview()
    }

    // ── 松手统一出口：click / move / connect / pan 都在这里闭环 ──
    function handleRelease() {
        if (!canvas || !ic) {
            cancelLocalState()
            return
        }
        // Issue #987：本次手势由编辑器认领或框外保存失败，只清 Router 现场。
        if (leftGestureBlocked) {
            cancelLocalState()
            return
        }
        // Issue #834 复核：armed 态下 release 不走普通 mode 收尾。
        // armed 的 target 选择由 handleSingleTap 负责（TapHandler 和
        // PointHandler 是独立 passive 观察者，同一条 pointer 流都会收到，
        // 不能赌 signal 顺序）。release 只清 Router 自己的临时 press 状态，
        // armed 状态留给 handleSingleTap() / Escape / 右键取消。
        if (ic.linkArmed || ic.connectArmed) {
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

    // ── 右键松手统一出口（Issue #930）──
    // 真拖动：finishConnect() 按既有出口递归命中解析目标并落边，空白目标
    // 由 finishConnect 自己取消；未拖动：普通点击，pressPending 直接取消，
    // 菜单交给同一手势的 TapHandler.singleTapped。
    // rightWasDrag 不在这里清除：singleTapped 可能在本回调之后才到，靠它
    // 跳过菜单；下一次右键按下在 beginRightPress 里重置。
    function endRightPress() {
        if (rightGestureBlocked) {
            rightPressActive = false
            rightPressHit = null
            cancelLocalState()
            return
        }
        if (!rightPressActive)
            return
        if (rightWasDrag && ic.pointerMode === "connect" && gestureOwner)
            gestureOwner.finishConnect()
        else if (ic.pointerMode === "pressPending")
            ic.cancelPressPending()
        rightPressActive = false
        rightPressHit = null
        if (rightWasDrag)
            cancelLocalState()
    }

    // ── 长按到点：唯一 Router 调状态提升 ──
    function handleLongPress() {
        if (!canvas || !ic)
            return
        if (leftGestureBlocked)
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
            if (pressed)
                router.beginPress(mouseLeftTap.point, "mouse")
            else
                router.leftPointerInEditor = false
        }
        onSingleTapped: function(eventPoint) {
            if (router.leftGestureBlocked)
                return
            router.handleSingleTap(eventPoint, "mouse")
        }
        onDoubleTapped: function(eventPoint) {
            if (router.leftGestureBlocked)
                return
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
        onSingleTapped: function(eventPoint) {
            if (router.leftGestureBlocked)
                return
            router.handleSingleTap(eventPoint, "touch")
        }
        onDoubleTapped: function(eventPoint) {
            if (router.leftGestureBlocked)
                return
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
        onSingleTapped: function(eventPoint) {
            if (router.rightGestureBlocked || router.rightWasDrag)
                return
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
        // Issue #987：按下点在编辑框内时本次手势归编辑器，立即让出抓取；
        // 其它区域（包括编辑框外）仍由状态机处理。
        enabled: !router.leftPointerInEditor
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
        enabled: !router.rightPointerInEditor
        onActiveChanged: {
            if (active) {
                if (router.rightGestureBlocked)
                    return
                router.beginRightConnectDrag()
                // 激活当帧端点立即对齐指针（按下点种子只兜住随后的零位移场景）。
                router.updateConnectEndpointFromScreen(rightConnectDrag.centroid.position.x,
                                                       rightConnectDrag.centroid.position.y)
            }
        }
        onActiveTranslationChanged: {
            if (!active || router.rightGestureBlocked)
                return
            router.updateConnectEndpointFromScreen(rightConnectDrag.centroid.position.x,
                                                   rightConnectDrag.centroid.position.y)
        }
    }

    // ── 左键 press→release 观察（passive grab）：松手统一出口 ──
    PointHandler {
        id: leftReleaseTracker
        acceptedButtons: Qt.LeftButton
        onActiveChanged: {
            if (!active)
                router.handleRelease()
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
            if (!active)
                router.endRightPress()
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
                canvas.resetInteraction()
                canvas.hideTouchPreview()
                router.cancelLocalState()
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
        blocking: true
        onWheel: function(event) {
            if (!canvas)
                return
            // Issue #987：编辑框内的滚轮归编辑器正文；框外仍缩放画布。
            // 不在编辑期间全局禁用滚轮。
            if (router.pointInEditor(point.position.x, point.position.y))
                return
            canvas.notePointerDevice(false)
            var delta = event.angleDelta.y !== 0
                ? event.angleDelta.y / 120
                : event.pixelDelta.y / 120.0
            if (delta === 0)
                return
            var oldZoom = canvas.zoomTargetLevel
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
