// =============================================================================
// StarMapSceneContent.qml — 星图单层内容容器（递归内容，不是递归视口）
// =============================================================================
//
// 层级：Linux_qt UI 层（QML UI 组件）
// 职责：加载"一层"星图数据 + 渲染该层节点/连线/子星图，并提供递归命中测试
//
// Issue #822：整棵星图只有一个全局 viewport/camera。
//   panX / panY / zoomLevel 只存在于根 StarMapCanvas；WheelHandler / PinchHandler
//   也只存在于根 Canvas。本文件是纯内容容器，不含任何相机属性或相机手势。
//   子星图"看起来更大/更小"只是视觉 LOD（visualScale），由父层根据
//   globalZoom + depth + 屏幕投影尺寸算出来；本文件不能反写全局相机，
//   也不能产生自己可独立修改的 zoomLevel。
//
// 坐标约定：
//   - 本层局部坐标 = 该层星图的 world 坐标，节点/连线都画在这里。
//   - scene 坐标 = 根 Content 的局部坐标（整棵递归树的顶层 world 坐标）。
//     sceneToLocal / localToScene / sceneDeltaToLocal 由 sceneOrigin + sceneScale
//     显式换算，sceneScale 由父 Embed 累积传下来，不依赖 mapToItem，
//     避免和 wobble 视觉偏移耦合。
//   - Qt scene 坐标（QQuickWindow 坐标）只出现在 delegate 上抛的信号里，
//     进来立刻用 mapFromItem 换算掉，不往状态机里存。
//
// 递归命中测试：
//   hitTargetAtScene(sceneX, sceneY) 先查本层节点 / Embed chrome；落在 Embed
//   内容区时递归进子 Content。返回真正命中的那一层
//   （owner / scenePathKey / starmapId / kind / id / targetPath），
//   连线松手、右键空白新建、选中、pointer_press 日志全部走这一个入口。
//
// 约束：
//   - 不出现 panX / panY / zoomLevel / WheelHandler / PinchHandler / 背景 pan handler
//   - 相机与全局手势状态由根 Canvas 唯一持有，通过 interactionController 共享
//   - 不得静态引用自身或 StarMapEmbed 类型（会形成编译期环），
//     Embed 侧必须用运行时 Qt.resolvedUrl + Loader.setSource 递归创建本组件
// =============================================================================

import QtQuick

Item {
    id: content

    required property var dt
    required property string rootStarmapId
    required property var pathSegments
    // Issue #822：required 且没有默认 "root"。
    // 根层由 Canvas 显式传 "root"，子层在创建时一次性传完整 "root/embed_xxx/..."，
    // 绝不允许子层临时冒充根层。
    required property string scenePathKey
    required property var starmapBackendRef
    required property var selectionController
    required property var interactionController
    required property real globalZoom
    required property int depth

    // 根层由根 Canvas 指向自己，子层由父 Embed 原样传下来。
    property var rootContent: null
    // 菜单宿主（根 Canvas）。子层直接回调它，不需要把信号逐层冒泡。
    property var menuHost: null

    // scene 坐标 → 本层局部坐标 的原点与比例。根层是 (0,0) / 1。
    property real sceneOriginX: 0
    property real sceneOriginY: 0
    property real sceneScale: 1.0

    // 当前可见区域（scene 坐标矩形），根层由 Canvas 按相机算出，逐层原样传下去。
    property var viewportRect: ({ x: 0, y: 0, width: 0, height: 0 })

    readonly property color _accent: dt.accent
    readonly property color _border: dt.border
    readonly property color _error: dt.error
    readonly property color _surfaceContainer: dt.surfaceContainer
    readonly property color _textPrimary: dt.textPrimary
    readonly property color _textMuted: dt.textMuted

    // ── 路径解析 ──
    // 解析出的本层星图 ID 只属于这块 Content，不能写回上层。
    property string finalStarmapId: ""
    property string resolveError: ""

    function sceneToLocal(sceneX, sceneY) {
        var s = sceneScale > 0 ? sceneScale : 1
        return { x: (sceneX - sceneOriginX) / s, y: (sceneY - sceneOriginY) / s }
    }
    function localToScene(lx, ly) {
        var s = sceneScale > 0 ? sceneScale : 1
        return { x: sceneOriginX + lx * s, y: sceneOriginY + ly * s }
    }
    // scene 坐标增量 → 本层局部坐标增量。
    function sceneDeltaToLocal(dx, dy) {
        var s = sceneScale > 0 ? sceneScale : 1
        return { x: dx / s, y: dy / s }
    }
    // 本层局部坐标增量 → scene 坐标增量（sceneDeltaToLocal 的逆）。
    function localDeltaToScene(dx, dy) {
        var s = sceneScale > 0 ? sceneScale : 1
        return { x: dx * s, y: dy * s }
    }

    // Qt scene（窗口）坐标 → 本层局部坐标。
    // 用 mapFromItem 而不是手算比例：祖先链上的累积缩放一次算完。
    function qtSceneToLocal(qx, qy) {
        return content.mapFromItem(null, qx, qy)
    }

    // Qt scene 坐标增量 → 本层局部坐标增量。
    // mapFromItem 只能映射点，不能映射增量；用 (0,0) 的自身位置做差，
    // 顺带把 wobble 那种纯平移的视觉偏移抵消掉。
    function qtSceneDeltaToLocal(dx, dy) {
        var origin = content.mapFromItem(null, 0, 0)
        var point = content.mapFromItem(null, dx, dy)
        return { x: point.x - origin.x, y: point.y - origin.y }
    }

    function resolvePath() {
        if (rootStarmapId === "") {
            resolveError = ""
            finalStarmapId = ""
            return
        }
        if (!starmapBackendRef) {
            // 后端未注入时不报错：创建顺序不保证，等后端到了再解析。
            resolveError = ""
            finalStarmapId = ""
            return
        }
        var res = starmapBackendRef.resolve_starmap_path(rootStarmapId, JSON.stringify(pathSegments))
        if (!res || res.success !== true) {
            resolveError = qsTr("解析星图层级路径失败")
                    + (res && res.errorCode ? " (" + res.errorCode + ")" : "")
            finalStarmapId = ""
            starmapBackendRef.record_interaction(
                "scene_resolve_failed", scenePathKey, rootStarmapId, "scene", "",
                JSON.stringify({
                    "pathKey": scenePathKey,
                    "errorCode": (res && res.errorCode) ? res.errorCode : "",
                    "rootStarmapId": rootStarmapId,
                    "depth": pathSegments.length
                }))
            return
        }
        var finalId = res.data && res.data.finalStarmapId ? res.data.finalStarmapId : ""
        if (finalId === "") {
            resolveError = qsTr("解析星图层级路径失败")
            finalStarmapId = ""
            starmapBackendRef.record_interaction(
                "scene_resolve_failed", scenePathKey, rootStarmapId, "scene", "",
                JSON.stringify({
                    "pathKey": scenePathKey,
                    "errorCode": "empty_final_id",
                    "rootStarmapId": rootStarmapId,
                    "depth": pathSegments.length
                }))
            return
        }
        resolveError = ""
        finalStarmapId = finalId
        starmapBackendRef.record_interaction(
            "scene_resolved", scenePathKey, rootStarmapId, "scene", finalId,
            JSON.stringify({
                "pathKey": scenePathKey,
                "rootStarmapId": rootStarmapId,
                "finalStarmapId": finalId,
                "depth": pathSegments.length
            }))
    }

    onRootStarmapIdChanged: resolvePath()
    onPathSegmentsChanged: resolvePath()
    onStarmapBackendRefChanged: {
        if (rootStarmapId !== "" && finalStarmapId === "")
            resolvePath()
    }
    onFinalStarmapIdChanged: {
        if (finalStarmapId !== "")
            graphController.loadGraph()
    }
    Component.onCompleted: resolvePath()

    signal selectionCleared()
    signal nodeSelected(var node)
    signal edgeSelected(var edge)

    readonly property string errorMessage: graphController.errorMessage

    function clearError() { graphController.clearError() }

    // ── 本层图控制器 ──
    // 每层 Content 拥有自己的 GraphController，节点与本层连线都按本层局部坐标渲染。
    StarMapGraphController {
        id: graphController
        starmapId: content.finalStarmapId
        starmapBackendRef: content.starmapBackendRef
        // Issue #814 评论 5935285879: 选择身份带本层 pathKey。
        pathKey: content.scenePathKey
        selectionController: content.selectionController
        onGraphChanged: content.refreshEdges()
        onSelectionCleared: content.selectionCleared()
        onNodeSelected: function(node) { content.nodeSelected(node) }
        onEdgeSelected: function(edge) { content.edgeSelected(edge) }
    }

    // 按 id 取本层模型条目，供菜单（移动/编辑/删除）使用。
    function hitNode(nodeId) { return graphController.getNode(nodeId) }
    function hitEmbed(instanceId) { return graphController.getEmbed(instanceId) }
    function hitEdge(edgeId) {
        for (var i = 0; i < graphController.edgesModel.length; i++) {
            if (graphController.edgesModel[i].id === edgeId)
                return graphController.edgesModel[i]
        }
        return null
    }

    // ── 路径构造（StarMapTargetPathDto JS 对象）──
    // JSON 字段名遵循 DTO serde rename：starmapId（camelCase）、segments、target.type/nodeId。
    // starmapId 用 rootStarmapId，segments 用本层 pathSegments，
    // 这样第 N 层连线端点不会退化成第一层。
    function nodePath(nodeId) {
        return {
            starmapId: rootStarmapId,
            segments: pathSegments.slice(0),
            target: { type: "node", nodeId: nodeId }
        }
    }
    // Embed 端点的完整路径由本层 pathSegments.concat 拼出来。
    function embedPath(instanceId) {
        return {
            starmapId: rootStarmapId,
            segments: pathSegments.concat([graphController.embedPathSegment(instanceId)]),
            target: { type: "starmap" }
        }
    }

    // ---------------------------------------------------------------------------
    // 递归命中测试：命中哪一层就返回哪一层的身份。
    // 顺序：本层节点 → 本层 Embed chrome → 子星图内容区（递归）→ 本层连线 → 本层空白。
    // ---------------------------------------------------------------------------
    function hitTargetAtScene(sceneX, sceneY) {
        if (finalStarmapId === "")
            return null
        var p = sceneToLocal(sceneX, sceneY)

        var node = graphController.findNodeAt(p.x, p.y)
        if (node) {
            return {
                owner: content,
                scenePathKey: scenePathKey,
                starmapId: finalStarmapId,
                kind: "node",
                id: node.id,
                targetPath: nodePath(node.id),
                localX: p.x,
                localY: p.y
            }
        }

        var chrome = graphController.findEmbedChromeAt(p.x, p.y)
        if (chrome) {
            return {
                owner: content,
                scenePathKey: scenePathKey,
                starmapId: finalStarmapId,
                kind: "embed",
                id: chrome.instanceId,
                targetPath: embedPath(chrome.instanceId),
                localX: p.x,
                localY: p.y
            }
        }

        var inside = graphController.findEmbedContentAt(p.x, p.y)
        if (inside) {
            var child = childContentOf(inside.instanceId)
            if (child) {
                var deeper = child.hitTargetAtScene(sceneX, sceneY)
                if (deeper)
                    return deeper
            }
            // 子星图内容还没加载出来：命中点归本层的 childContent，
            // 不能冒充 empty（empty 专指"任何一层都没命中"）。
            return {
                owner: content,
                scenePathKey: scenePathKey,
                starmapId: finalStarmapId,
                kind: "childContent",
                id: inside.instanceId,
                targetPath: null,
                localX: p.x,
                localY: p.y
            }
        }

        var edge = graphController.hitTestEdge(p.x, p.y)
        if (edge) {
            return {
                owner: content,
                scenePathKey: scenePathKey,
                starmapId: finalStarmapId,
                kind: "edge",
                id: edge.id,
                targetPath: null,
                localX: p.x,
                localY: p.y
            }
        }

        return {
            owner: content,
            scenePathKey: scenePathKey,
            starmapId: finalStarmapId,
            kind: "empty",
            id: "",
            targetPath: null,
            localX: p.x,
            localY: p.y
        }
    }

    function embedItemOf(instanceId) {
        for (var i = 0; i < embedRepeater.count; i++) {
            var it = embedRepeater.itemAt(i)
            if (it && it.instanceId === instanceId)
                return it
        }
        return null
    }
    function nodeItemOf(nodeId) {
        for (var i = 0; i < nodeRepeater.count; i++) {
            var it = nodeRepeater.itemAt(i)
            if (it && it.nodeId === nodeId)
                return it
        }
        return null
    }
    function childContentOf(instanceId) {
        var it = embedItemOf(instanceId)
        return it ? it.childContent() : null
    }

    // ---------------------------------------------------------------------------
    // 手势仲裁：全局状态机由根 Canvas 唯一创建并逐层共享。
    // 本层只负责"按下去的对象是不是我的"以及把位移换算成自己的局部坐标；
    // 状态只能被共享状态机提升一次，delegate 自己不决定 move/connect。
    // ---------------------------------------------------------------------------
    function ownsPress(kind, id) {
        if (!interactionController)
            return false
        var ic = interactionController
        if (ic.pointerMode === "connect" || ic.pointerMode === "contextPending")
            return ic.connectFromScenePathKey === scenePathKey
                && ic.connectFromKind === kind
                && ic.connectFromId === id
        if (ic.pointerMode === "move")
            return ic.moveScenePathKey === scenePathKey
                && (kind === "node" ? ic.pressedNodeId === id : ic.pressedEmbedId === id)
        return ic.pressScenePathKey === scenePathKey
            && ic.pressKind === kind
            && ic.pressId === id
    }

    // 节点/Embed chrome 上的鼠标按下：只登记归属，不立刻移动也不立刻连线。
    function onItemPressed(kind, id, targetPath, qtSceneX, qtSceneY) {
        if (!interactionController)
            return
        var lp = qtSceneToLocal(qtSceneX, qtSceneY)
        var sp = localToScene(lp.x, lp.y)
        interactionController.beginPress(kind, id, targetPath, scenePathKey, sp.x, sp.y)
    }

    // delegate DragHandler 上抛的原始 Qt scene 位移。
    // 只有按下归属在本层的对象时才消费；仲裁逻辑和触屏共用同一条路径，
    // 保证"鼠标长按拉线"和"触屏长按拉线"不会各写一套状态提升。
    function onItemDragDelta(kind, id, dxQtScene, dyQtScene) {
        if (!interactionController)
            return
        if (!ownsPress(kind, id))
            return
        onSceneDragDelta(dxQtScene, dyQtScene)
    }

    function promoteToMove(kind, id) {
        if (!interactionController)
            return
        var item = kind === "node" ? graphController.getNode(id) : graphController.getEmbed(id)
        if (!item)
            return
        if (!interactionController.pressPendingToMove(kind, id, scenePathKey, item.x, item.y))
            return
        logInteraction("move_begin", kind, id, {
            "kind": kind, "fromX": item.x, "fromY": item.y
        })
        refreshEdges()
    }

    // 长按计时到：由共享 InteractionController 的 Timer 触发，只有归属层提升为 connect。
    function onPressTimeout() {
        if (!interactionController)
            return
        var ic = interactionController
        if (ic.pointerMode !== "pressPending")
            return
        var kind = ic.pressKind
        var id = ic.pressId
        if (!ownsPress(kind, id))
            return
        var item = kind === "node" ? graphController.getNode(id) : graphController.getEmbed(id)
        if (!item)
            return
        var center = localToScene(item.x + item.width / 2, item.y + item.height / 2)
        if (!ic.pressPendingToConnect(kind, id, ic.pressTargetPath, center.x, center.y))
            return
        logInteraction("connect_begin", kind, id, {
            "kind": kind, "fromId": id, "fromX": center.x, "fromY": center.y
        })
    }

    // 触屏长按：不移动弹菜单，移动超阈值转 connect（#373 触屏语义）。
    function onItemTouchLongPressed(kind, id, targetPath) {
        if (!interactionController)
            return
        var ic = interactionController
        var item = kind === "node" ? graphController.getNode(id) : graphController.getEmbed(id)
        if (!item)
            return
        var center = localToScene(item.x + item.width / 2, item.y + item.height / 2)
        if (!ic.beginContextPending(kind, id, targetPath, scenePathKey, center.x, center.y))
            return
        if (menuHost)
            menuHost.showTouchPreview(kind, center.x, center.y)
    }

    // 本层是不是当前手势的归属层（pressPending / connect / contextPending / move）。
    function isGestureOwner() {
        if (!interactionController)
            return false
        var ic = interactionController
        return ic.pressScenePathKey === scenePathKey
                || ic.connectFromScenePathKey === scenePathKey
                || ic.moveScenePathKey === scenePathKey
    }

    // 触屏拖动位移：由根 Canvas 的触屏 DragHandler 驱动。
    // 手指起点可能在任意深层的节点上，归属层不一定是根层，
    // 所以从根开始往下找到真正的归属层，返回 true 表示已消费。
    function onSceneDragDelta(dxQtScene, dyQtScene) {
        if (!interactionController)
            return false
        var ic = interactionController
        if (ic.pointerMode !== "contextPending" && ic.pointerMode !== "connect"
                && ic.pointerMode !== "pressPending" && ic.pointerMode !== "move")
            return false
        if (!isGestureOwner()) {
            // 归属在更深的子层，转发下去。
            for (var i = 0; i < embedRepeater.count; i++) {
                var item = embedRepeater.itemAt(i)
                var child = item ? item.childContent() : null
                if (child && child.onSceneDragDelta(dxQtScene, dyQtScene))
                    return true
            }
            return false
        }
        var localDelta = qtSceneDeltaToLocal(dxQtScene, dyQtScene)
        var sceneDelta = localDeltaToScene(localDelta.x, localDelta.y)

        // Issue #822: 按下仲裁 —— 先超拖动阈值转 move，先到长按时间转 connect。
        if (ic.pointerMode === "pressPending") {
            ic.noteDragDelta(sceneDelta.x, sceneDelta.y)
            if (ic.pressDragDistance >= ic.dragThreshold)
                promoteToMove(ic.pressKind, ic.pressId)
            return true
        }
        if (ic.pointerMode === "move") {
            ic.updateMove(ic.moveX + localDelta.x, ic.moveY + localDelta.y)
            return true
        }
        if (ic.pointerMode === "contextPending") {
            ic.connectMouseX += sceneDelta.x
            ic.connectMouseY += sceneDelta.y
            var tdx = ic.connectMouseX - ic.connectFromSceneX
            var tdy = ic.connectMouseY - ic.connectFromSceneY
            if (Math.sqrt(tdx * tdx + tdy * tdy) > ic.moveThreshold)
                ic.contextPendingToConnect()
            return true
        }
        ic.updateConnect(ic.connectMouseX + sceneDelta.x, ic.connectMouseY + sceneDelta.y)
        return true
    }

    // 松手统一出口：click / move / connect 都从这里闭环。
    function releaseOwnerGesture() {
        if (!interactionController)
            return
        var mode = interactionController.pointerMode
        if (mode === "connect") {
            finishConnect()
        } else if (mode === "move") {
            finishMove()
        } else if (mode === "contextPending") {
            finishContextPending()
        } else if (mode === "pressPending") {
            interactionController.cancelPressPending()
        }
    }

    function finishConnect() {
        var ic = interactionController
        var fromKind = ic.connectFromKind
        var fromId = ic.connectFromId
        var fromPath = ic.connectFromPath
        var hit = rootContent ? rootContent.hitTargetAtScene(ic.connectMouseX, ic.connectMouseY) : null
        var toPath = null
        var success = false
        var cancelled = true
        if (hit && (hit.kind === "node" || hit.kind === "embed")) {
            var sameTarget = hit.scenePathKey === ic.connectFromScenePathKey && hit.id === fromId
            if (!sameTarget) {
                toPath = hit.targetPath
                // 建边由"源的归属层"执行，from/to 都是完整 StarMapTargetPathDto。
                success = createEdgeWithPaths(fromPath, toPath)
                cancelled = !success
            }
        }
        logInteraction("connect_end", fromKind, fromId, {
            "fromPath": JSON.stringify(fromPath),
            "toPath": toPath ? JSON.stringify(toPath) : "",
            "success": success,
            "cancel": cancelled
        })
        ic.endConnect()
        if (menuHost)
            menuHost.hideTouchPreview()
    }

    // Issue #822：建边入口保留完整路径，不退化成 nodeId-only。
    function createEdgeWithPaths(fromPath, toPath) {
        if (!fromPath || !toPath)
            return false
        return graphController.createEdgeWithPaths(fromPath, toPath)
    }

    function finishMove() {
        var ic = interactionController
        var nodeId = ic.pressedNodeId
        var embedId = ic.pressedEmbedId
        var nx = ic.moveX
        var ny = ic.moveY
        var committed = false
        if (nodeId !== "") {
            committed = graphController.commitNodeMove(nodeId, nx, ny)
            logInteraction("move_end", "node", nodeId, {
                "toX": nx, "toY": ny, "commitSuccess": committed, "device": "mouse"
            })
        } else if (embedId !== "") {
            committed = graphController.commitEmbedMove(embedId, nx, ny)
            logInteraction("move_end", "embed", embedId, {
                "toX": nx, "toY": ny, "commitSuccess": committed, "device": "mouse"
            })
        }
        ic.endMove()
        refreshEdges()
    }

    // 触屏长按后没移动就松手：关闭视觉层，弹出真正可点击的菜单。
    function finishContextPending() {
        var ic = interactionController
        var kind = ic.connectFromKind
        var id = ic.connectFromId
        ic.endContextPending()
        var item = kind === "node" ? graphController.getNode(id) : graphController.getEmbed(id)
        if (!item)
            return
        var center = localToScene(item.x + item.width / 2, item.y + item.height / 2)
        if (menuHost)
            menuHost.showLongPressMenu(kind, id, center.x, center.y, content)
    }

    // ---------------------------------------------------------------------------
    // 节点内联编辑：双击/右键"编辑"直接进入节点自身的 TextInput，
    // 提交后回写本层 GraphController，不再经过外部 Inspector Popup。
    // ---------------------------------------------------------------------------
    function beginInlineEdit(nodeId) {
        var it = nodeItemOf(nodeId)
        if (it)
            it.beginEdit()
    }
    function commitNodeTitle(nodeId, title) {
        graphController.updateNode(nodeId, { title: title })
    }

    // ---------------------------------------------------------------------------
    // 右键空白新建：菜单归属层就是被点中的那一层，坐标在这里换算成局部坐标。
    // ---------------------------------------------------------------------------
    function findFreeSpawnPoint(wx, wy) {
        var step = 24
        var candidates = [{ x: wx, y: wy }]
        for (var ring = 1; ring <= 8; ring++) {
            for (var dx = -ring; dx <= ring; dx++) {
                candidates.push({ x: wx + dx * step, y: wy - ring * step })
                candidates.push({ x: wx + dx * step, y: wy + ring * step })
            }
            for (var dy = -ring + 1; dy <= ring - 1; dy++) {
                candidates.push({ x: wx - ring * step, y: wy + dy * step })
                candidates.push({ x: wx + ring * step, y: wy + dy * step })
            }
        }
        for (var i = 0; i < candidates.length; i++) {
            if (!overlapsExistingNode(candidates[i].x, candidates[i].y, 150, 60, 12))
                return candidates[i]
        }
        return { x: wx, y: wy }
    }

    function overlapsExistingNode(x, y, w, h, padding) {
        var nodes = graphController.nodesModel
        for (var i = 0; i < nodes.length; i++) {
            var n = nodes[i]
            var nx = n.x - padding
            var ny = n.y - padding
            var nw = n.width + padding * 2
            var nh = n.height + padding * 2
            if (x < nx + nw && x + w > nx && y < ny + nh && y + h > ny)
                return true
        }
        return false
    }

    // 菜单归属层用：命名 → 换算到本层局部坐标 → 找不压旧节点的落点 → 写 Core。
    function createNodeWithName(name, sceneX, sceneY) {
        var p = sceneToLocal(sceneX, sceneY)
        var spawn = findFreeSpawnPoint(p.x, p.y)
        graphController.createNode(name, spawn.x, spawn.y)
    }
    function createSubStarmapWithName(name, sceneX, sceneY) {
        var p = sceneToLocal(sceneX, sceneY)
        var spawn = findFreeSpawnPoint(p.x, p.y)
        graphController.createSubStarmapAt(name, spawn.x, spawn.y)
    }

    function updateNodeTitle(nodeId, title) { graphController.updateNode(nodeId, { title: title }) }
    function deleteNode(nodeId) { graphController.deleteNode(nodeId) }
    function updateEdgeLabel(edgeId, label) { graphController.updateEdge(edgeId, { label: label }) }
    function deleteEdge(edgeId) { graphController.deleteEdge(edgeId) }
    function updateEmbedLabel(instanceId, label) { graphController.updateEmbed(instanceId, { label: label }) }
    function deleteEmbed(instanceId) { graphController.deleteEmbed(instanceId) }
    function selectNode(nodeId) { graphController.selectNode(nodeId) }
    function selectEdge(edgeId) { graphController.selectEdge(edgeId) }
    function selectEmbed(instanceId) { graphController.selectEmbed(instanceId) }
    function clearLayerSelection() { graphController.clearSelection() }

    function isMovingNode(id) {
        if (!interactionController)
            return false
        return interactionController.pointerMode === "move"
                && interactionController.moveScenePathKey === scenePathKey
                && interactionController.pressedNodeId === id
    }
    function isMovingEmbed(instanceId) {
        if (!interactionController)
            return false
        return interactionController.pointerMode === "move"
                && interactionController.moveScenePathKey === scenePathKey
                && interactionController.pressedEmbedId === instanceId
    }

    // ---------------------------------------------------------------------------
    // 连线重绘：本层 edgeRenders 只画本层连线；瞬时 move 期间用共享状态机做 override。
    // ---------------------------------------------------------------------------
    function currentMoveOverride() {
        if (!interactionController)
            return null
        var ic = interactionController
        if (ic.pointerMode !== "move" || ic.moveScenePathKey !== scenePathKey)
            return null
        if (ic.pressedNodeId !== "") {
            return { kind: "node", id: ic.pressedNodeId, x: ic.moveX, y: ic.moveY }
        }
        if (ic.pressedEmbedId !== "") {
            return { kind: "embed", id: ic.pressedEmbedId, x: ic.moveX, y: ic.moveY }
        }
        return null
    }

    function refreshEdges() {
        graphController.computeEdgeRenders(currentMoveOverride())
        edgeCanvas.requestPaint()
    }

    // 整棵递归树回到 canonical 连线位置（切图/失焦/reset 时用）。
    function refreshAllEdges() {
        graphController.computeEdgeRenders(null)
        edgeCanvas.requestPaint()
        for (var i = 0; i < embedRepeater.count; i++) {
            var child = embedRepeater.itemAt(i)
            var inner = child ? child.childContent() : null
            if (inner)
                inner.refreshAllEdges()
        }
    }

    Connections {
        target: content.interactionController
        function onMoveXChanged() { if (content.currentMoveOverride()) content.refreshEdges() }
        function onMoveYChanged() { if (content.currentMoveOverride()) content.refreshEdges() }
        function onPointerModeChanged() { content.refreshAllEdges() }
        // Issue #822: 长按计时到只由共享状态机发信号，
        // 状态提升由"归属层"完成，避免每层都去改全局状态。
        function onPressTimeout() { content.onPressTimeout() }
    }

    // Issue #814 评论 5935346839: 单层交互边界日志入口，不进连续移动热路径。
    function logInteraction(event, itemKind, itemId, fields) {
        if (!starmapBackendRef)
            return
        var fj = fields ? JSON.stringify(fields) : ""
        starmapBackendRef.record_interaction(event, scenePathKey, finalStarmapId, itemKind, itemId, fj)
    }

    // ── 本层连线 ──
    Canvas {
        id: edgeCanvas
        anchors.fill: parent
        z: 0

        Connections {
            target: content.selectionController
            function onScenePathKeyChanged() { edgeCanvas.requestPaint() }
            function onKindChanged() { edgeCanvas.requestPaint() }
            function onItemIdChanged() { edgeCanvas.requestPaint() }
        }

        onPaint: {
            var ctx = getContext("2d")
            ctx.clearRect(0, 0, width, height)
            ctx.lineWidth = 2
            var renders = graphController.edgeRenders
            for (var i = 0; i < renders.length; i++) {
                var r = renders[i]
                var edge = null
                for (var ei = 0; ei < graphController.edgesModel.length; ei++) {
                    if (graphController.edgesModel[ei].id === r.edgeId) {
                        edge = graphController.edgesModel[ei]
                        break
                    }
                }
                if (!edge)
                    continue
                var edgeSelected = selectionController
                        ? selectionController.matches(scenePathKey, "edge", edge.id)
                        : false
                var color = edgeSelected ? _accent : _border

                ctx.beginPath()
                ctx.moveTo(r.startX, r.startY)
                ctx.lineTo(r.endX, r.endY)
                ctx.strokeStyle = color
                ctx.stroke()

                ctx.beginPath()
                ctx.moveTo(r.arrowTipX, r.arrowTipY)
                ctx.lineTo(r.arrowLeftX, r.arrowLeftY)
                ctx.lineTo(r.arrowRightX, r.arrowRightY)
                ctx.closePath()
                ctx.fillStyle = color
                ctx.fill()

                if (edge.label) {
                    ctx.fillStyle = _surfaceContainer
                    var tw = ctx.measureText(edge.label).width
                    ctx.fillRect(r.labelX - tw / 2 - 4, r.labelY - 10, tw + 8, 20)
                    ctx.fillStyle = _textPrimary
                    ctx.font = "12px sans-serif"
                    ctx.textAlign = "center"
                    ctx.textBaseline = "middle"
                    ctx.fillText(edge.label, r.labelX, r.labelY)
                }
            }
        }
    }

    // ── 本层节点与子星图 ──
    Item {
        id: nodeLayer
        anchors.fill: parent
        z: 1

        Repeater {
            id: nodeRepeater
            model: graphController.nodesModel
            delegate: StarMapNode {
                required property var modelData
                required property int index
                dt: content.dt
                property var nodeData: modelData
                readonly property string nodeId: nodeData.id

                // 本层局部坐标，不再经过 worldToScreen：相机在祖先 Content 上。
                x: content.isMovingNode(nodeData.id) ? content.interactionController.moveX : nodeData.x
                y: content.isMovingNode(nodeData.id) ? content.interactionController.moveY : nodeData.y
                width: nodeData.width
                height: nodeData.height
                title: nodeData.title
                isSelected: content.selectionController
                        ? content.selectionController.matches(content.scenePathKey, "node", nodeData.id)
                        : false
                wobbleIndex: index

                onMouseInteracted: {
                    if (content.menuHost) content.menuHost.noteMouseInteracted()
                }

                // 鼠标按下只登记归属（pressPending），不决定 move 还是 connect。
                onItemPressed: function(qx, qy) {
                    content.onItemPressed("node", nodeData.id, content.nodePath(nodeData.id), qx, qy)
                }

                onTouchLongPressed: {
                    content.onItemTouchLongPressed("node", nodeData.id, content.nodePath(nodeData.id))
                }

                onMoveDelta: function(dx, dy) {
                    content.onItemDragDelta("node", nodeData.id, dx, dy)
                }

                onLeftReleased: content.releaseOwnerGesture()

                // Issue #822：节点自身就是编辑器，双击直接把光标放进节点框。
                onDoubleClicked: content.beginInlineEdit(nodeData.id)

                onTitleCommitted: function(newTitle) {
                    content.commitNodeTitle(nodeData.id, newTitle)
                }
            }
        }

        Repeater {
            id: embedRepeater
            model: graphController.embedsModel
            delegate: StarMapEmbed {
                required property var modelData
                required property int index
                dt: content.dt
                property var embedData: modelData

                x: content.isMovingEmbed(embedData.instanceId) ? content.interactionController.moveX : embedData.x
                y: content.isMovingEmbed(embedData.instanceId) ? content.interactionController.moveY : embedData.y
                width: embedData.width
                height: embedData.height
                instanceId: embedData.instanceId
                targetStarmapId: embedData.targetStarmapId
                label: embedData.label
                isSelected: content.selectionController
                        ? content.selectionController.matches(content.scenePathKey, "embed", embedData.instanceId)
                        : false
                wobbleIndex: index

                // Issue #822：子星图"看起来多大"只是视觉 LOD。
                // 只由父层 globalZoom / depth / 屏幕投影尺寸决定，不能反写全局相机，
                // 也不能产生独立可修改的 zoomLevel。
                ancestorScale: content.sceneScale
                scale: visualScale

                // 递归子星图内容只在投影矩形进入视口后才创建，按需懒加载。
                // 命中判定与视觉 LOD 无关，纯看投影矩形是否进入当前可见区域。
                childContentInViewport: {
                    var margin = 64
                    var o = content.localToScene(embedData.x, embedData.y)
                    var s = content.sceneScale * visualScale
                    var v = content.viewportRect
                    return o.x + width * s >= v.x - margin
                            && o.y + height * s >= v.y - margin
                            && o.x <= v.x + v.width + margin
                            && o.y <= v.y + v.height + margin
                }

                rootStarmapId: content.rootStarmapId
                parentPathSegments: content.pathSegments
                starmapBackendRef: content.starmapBackendRef
                parentPathKey: content.scenePathKey
                contentDepth: content.depth + 1
                globalZoom: content.globalZoom
                selectionController: content.selectionController
                interactionController: content.interactionController
                rootContent: content.rootContent
                menuHost: content.menuHost
                viewportRect: content.viewportRect

                // 子层 scene 换算参数：本 Embed 左上角在 scene 坐标里的位置 + 累积比例。
                sceneOriginX: content.localToScene(embedData.x, embedData.y).x
                sceneOriginY: content.localToScene(embedData.x, embedData.y).y
                sceneScale: content.sceneScale * visualScale

                onMouseInteracted: {
                    if (content.menuHost) content.menuHost.noteMouseInteracted()
                }

                onItemPressed: function(qx, qy) {
                    content.onItemPressed("embed", embedData.instanceId,
                                          content.embedPath(embedData.instanceId), qx, qy)
                }

                onTouchLongPressed: {
                    content.onItemTouchLongPressed("embed", embedData.instanceId,
                                                   content.embedPath(embedData.instanceId))
                }

                onMoveDelta: function(dx, dy) {
                    content.onItemDragDelta("embed", embedData.instanceId, dx, dy)
                }

                onLeftReleased: content.releaseOwnerGesture()
            }
        }
    }

    // ── 解析状态提示 ──
    AppText {
        dt: content.dt
        anchors.centerIn: parent
        visible: content.resolveError.length > 0
        text: content.resolveError
        color: content._error
        font.pointSize: content.dt.fontSmPt
        wrapMode: Text.Wrap
        horizontalAlignment: Text.AlignHCenter
    }

    AppText {
        dt: content.dt
        anchors.centerIn: parent
        visible: content.rootStarmapId !== "" && content.finalStarmapId === "" && content.resolveError === ""
        text: qsTr("加载中…")
        color: content._textMuted
        font.pointSize: content.dt.fontSmPt
    }
}
