// =============================================================================
// StarMapGraphController.qml — 星图图控制器
// =============================================================================
//
// 层级：Linux_qt UI 层（QML 逻辑控制器）
// 职责：加载/保存星图图数据+布局、节点/边增删改、选区管理
// 约束：
//   - 纯状态管理，不包含 UI 渲染
//   - 通过 starmapBackendRef 调用 AppBackend (Rust QObject)
//   - 图数据通过 AppBackend 暴露的对象/数组 DTO 与 Core 层交互
//
// 数据流：starmapBackendRef (DTO) → controller (graphData) → Canvas (nodesModel/edgesModel)
// =============================================================================

import QtQuick
import QtQml.Models
import "StarMapNodeMetrics.js" as StarMapNodeMetrics

QtObject {
    id: controller

    property string starmapId: ""
    property var starmapBackendRef: null
    // Issue #984：平台文本排版器（StarMapNodeMeasurer）。图数据加载时用它
    // 一次量完整张图，不带“未打开子图按 64 猜”的第二种几何。
    property var textMeasurer: null
    property string errorMessage: ""
    property var graphData: null
    // 稳定行身份模型：更新节点只 set 对应 row，不整体替换数组。
    // Repeater 因此不会销毁重建正在交互的 delegate。
    property ListModel nodesModel: ListModel { id: nodesListModel }
    property var edgesModel: []
    property var edgeRenders: []
    // Issue #1029：传给原生连线图层的稳定 JSON 与只含标签的轻量模型。
    // 数组变化时才 stringify 一次，不再每帧/每次命中都序列化全部边。
    property string edgeRendersJson: "[]"
    // Issue #1035：节点卡片与子图圆壳的完整 world 几何快照。只在图数据 /
    // 布局 / 拖动落盘确实变化时更新；相机缩放平移不再生成或序列化对象几何。
    property string objectGeometryJson: "[]"
    property string objectNodesJson: "[]"
    property string objectEmbedsJson: "[]"
    property var edgeLabels: []
    // Issue #1029 评论：缓存按「场景实例 + 活动星图 + 导航路径」隔离；同一
    // Canvas 过渡期两份 Scene、或同图从不同 Embed 入口打开时都不能共享命中
    // / 连线缓存，销毁时也只能释放自己这一个 key。
    readonly property string sceneInstanceId:
        starmapBackendRef ? starmapBackendRef.create_scene_instance_id() : ""
    readonly property string sceneCacheKey:
        sceneInstanceId + "::" + starmapId + "::" + pathKey
    property bool _sceneCacheDirty: true
    // 统一的世界锚点：节点 / 边 / Embed 的局部坐标都相对它，父级只保留
    // 视口中心相对的 double 平移与 zoom，避免巨大世界坐标进入场景图。
    property real worldAnchorX: 0
    property real worldAnchorY: 0
    // 记录上一个 key，切图时只释放旧画布缓存，不整表清空。
    property string _lastSceneCacheKey: ""
    // Issue #796 评论 5886483653: 子星图 Embed 显示模型，从 graphData.embeds 派生。
    property var embedsModel: []

    onSceneCacheKeyChanged: {
        if (_lastSceneCacheKey !== "" && starmapBackendRef)
            starmapBackendRef.release_scene_cache(_lastSceneCacheKey)
        _lastSceneCacheKey = sceneCacheKey
        _sceneCacheDirty = true
    }

    // 场景销毁时只释放自己的显示缓存，避免旧画布 / 过渡层残留几何。
    Component.onDestruction: {
        if (starmapBackendRef && sceneCacheKey !== "")
            starmapBackendRef.release_scene_cache(sceneCacheKey)
    }

    // Issue #1010：同机其他进程（如 MCP）修改星图后，通过 Core 事件流
    // 增量感知并重新读取图数据，避免继续用旧 graphData 覆盖外部修改。
    property double _workspaceEventOffset: 0
    property Timer workspaceChangeTimer: Timer {
        interval: 1500
        repeat: true
        running: controller.starmapId !== ""
        onTriggered: controller.pollWorkspaceChanges()
    }

    onStarmapIdChanged: {
        _workspaceEventOffset = 0
        if (starmapId !== "") pollWorkspaceChanges()
    }

    // Issue #814 评论 5935285879: 共享选中控制器与本 Scene 的 pathKey。
    // selectNode/selectEdge/selectEmbed/clearSelection 改成调用共享 selectionController，
    // 选择身份带当前 pathKey。nodesModel/edgesModel/embedsModel 只保存图数据，
    // 不再把 isSelected 当模型状态反复浅拷贝。
    property var selectionController: null
    property string pathKey: "root"

    // Issue #805 评论 5908703621 问题 3 / #822 评论 5972215936：Embed chrome
    // 命中区域几何常量。与 StarMapEmbed.qml 的 _chromeHeight / _borderSlop 保持一致。
    // 圆内只有顶部标题带 + 圆周边框算 chrome，圆内其余区域返回 null（childContent）。
    readonly property int _chromeHeight: 24
    readonly property int _borderSlop: 6

    // Issue #822 评论 5972215936: Embed 外壳是正圆，world 尺寸恒定 = 直径 200，
    // 与 docs/starmap_viewport.md 的 DEFAULT_EMBED_DIAMETER 同值。
    // 不再有 240×220 的矩形卡片尺寸：档位只改渲染细节，不改外壳几何。
    readonly property int _embedDiameter: 200

    signal graphChanged()
    // Issue #1035：完整对象几何（world 坐标 / 宽高）发生变化。相机变化不发此信号。
    signal objectGeometryChanged()
    // 工作区事件流出现外部改动时发出原始事件列表；活动场景据此刷新子星图
    // 预览快照，但不会因为相机缩放 / 平移重算任何预览。
    signal workspaceActivity(var events)
    // 只有真正落盘成功的节点/嵌入结构或标题变化才发出；位置、镜头、
    // 边和链接变化不触发导航树重载。
    signal structureChanged()
    signal selectionCleared()
    signal nodeSelected(var node)
    signal edgeSelected(var edge)
    signal embedSelected(var embed)

    function setError(msg) {
        errorMessage = msg || "";
        if (errorMessage) { /* error tracked via errorMessage property */ }
    }

    function clearError() { errorMessage = ""; }

    // Issue #1036：星图成功写操作是真实人工前台活动的起点；纯加载/相机
    // 平移缩放不调用这里，也不会产生创作时长。
    function noteHumanActivity() {
        if (starmapId !== "" && starmapBackendRef
                && starmapBackendRef.note_starmap_operation)
            starmapBackendRef.note_starmap_operation(starmapId)
    }

    function pollWorkspaceChanges() {
        if (!starmapBackendRef || starmapId === "") return
        var res = normalizeBackendResult(starmapBackendRef.workspace_changes_since_json(_workspaceEventOffset), "")
        if (!res || !res.success) return
        var page = res.data || {}
        _workspaceEventOffset = page.nextOffset || 0
        var events = page.events || []
        var structureEventSeen = false
        var needLoadGraph = false
        for (var i = 0; i < events.length; i++) {
            var event = events[i]
            // Issue #1028：新建 / 绑定 / 嵌入 / 解绑 / 删除等结构变更由 Core
            // 发出 starmapStructure；左栏需要按真实 starmapId 重载，而不是把
            // 重绘当成写入成功。
            if (event.kind === "starmapStructure") {
                structureEventSeen = true
                if (event.starmapId === starmapId)
                    needLoadGraph = true
                continue
            }
            if (event.kind === "starmap" && event.starmapId === starmapId)
                needLoadGraph = true
        }
        if (needLoadGraph)
            loadGraph()
        if (structureEventSeen)
            structureChanged()
        if (events.length > 0)
            workspaceActivity(events)
    }

    function revisionOfNode(nodeId) {
        var nodes = graphData && graphData.nodes ? graphData.nodes : []
        for (var i = 0; i < nodes.length; i++)
            if (nodes[i].id === nodeId) return nodes[i].updatedAt
        return undefined
    }

    function revisionOfEdge(edgeId) {
        var edges = graphData && graphData.edges ? graphData.edges : []
        for (var i = 0; i < edges.length; i++)
            if (edges[i].id === edgeId) return edges[i].updatedAt
        return undefined
    }

    function revisionOfEmbed(instanceId) {
        var embeds = graphData && graphData.embeds ? graphData.embeds : []
        for (var i = 0; i < embeds.length; i++)
            if (embeds[i].instanceId === instanceId) return embeds[i].updatedAt
        return undefined
    }

    function revisionOfLink(linkId) {
        var links = graphData && graphData.links ? graphData.links : []
        for (var i = 0; i < links.length; i++)
            if (links[i].linkId === linkId) return links[i].updatedAt
        return undefined
    }

    function withExpectedRevision(patch, revision) {
        var next = copyObject(patch || {})
        if (revision !== undefined && next.expectedRevision === undefined)
            next.expectedRevision = revision
        return next
    }

    // 用 Core 返回的完整对象替换 graphData 中的规范对象。
    // 只改显示模型会留下旧的 updatedAt，下一次 expectedRevision 会误报冲突。
    function _replaceGraphObject(collection, idKey, objectId, updated) {
        if (!graphData || !graphData[collection] || !updated) return false
        var source = graphData[collection]
        var nextItems = []
        var replaced = false
        for (var i = 0; i < source.length; i++) {
            var item = source[i]
            if (item && item[idKey] === objectId) {
                nextItems.push(copyObject(updated))
                replaced = true
            } else {
                nextItems.push(item)
            }
        }
        if (!replaced) return false
        var nextGraph = copyObject(graphData)
        nextGraph[collection] = nextItems
        graphData = nextGraph
        _sceneCacheDirty = true
        invalidatePreviewCache()
        return true
    }

    // 本地落到 Core 的图内容 / 位置变化也要让共享预览缓存失效；镜头、
    // 选择和拖拽临时帧不调用这里。
    function invalidatePreviewCache() {
        if (!starmapBackendRef || !starmapId
                || !starmapBackendRef.invalidate_starmap_preview)
            return
        starmapBackendRef.invalidate_starmap_preview(starmapId, 0, "")
    }

    function _reloadOnConflict(res) {
        if (res && res.errorCode === "CONFLICT") {
            loadGraph()
            return true
        }
        return false
    }

    // 统一后端错误文本：按 ResultEnvelope 标准字段（errorCode）拼接，
    // 不读不存在的 message 字段；rawError 只进诊断日志，不直接展示给用户。
    function backendErrorText(res, fallback) {
        if (!res) return fallback
        if (res.errorCode) return fallback + " (" + res.errorCode + ")"
        return fallback
    }

    function normalizeBackendResult(raw, fallbackMessage) {
        if (raw && raw.success !== undefined) return raw;
        setError(fallbackMessage);
        return { success: false };
    }

    function ensureBackend() {
        if (!starmapBackendRef) {
            setError(qsTr("星图后端未初始化"));
            return false;
        }
        return true;
    }

    // invalidatePreview=true 只用于确实已经持久化成功、需要让父级预览重算
    // 的写操作。普通打开 / 切换 / 事件触发的 loadGraph 是只读入口，不能
    // 让共享预览缓存失效。
    function loadGraph(invalidatePreview) {
        if (starmapId === "") return;
        if (!ensureBackend()) return;

        var res = normalizeBackendResult(starmapBackendRef.get_starmap_graph(starmapId), qsTr("加载星图数据失败"));
        if (res.success) {
            clearError();
            graphData = res.data.graph;
            _sceneCacheDirty = true;
            if (invalidatePreview === true)
                invalidatePreviewCache();
            // 结构通知与几何缓存提交分离：先更新模型、同步一次缓存并算一次边，
            // 最后才通知视图；视图不再触发第二遍全图计算。
            buildModels(false);
            computeEdgeRenders(null);
            graphChanged();
        } else {
            setError(qsTr("加载星图数据失败"));
        }
    }

    function updateWorldAnchor() {
        var minX = Infinity
        var minY = Infinity
        var maxX = -Infinity
        var maxY = -Infinity
        for (var i = 0; i < nodesModel.count; i++) {
            var n = nodesModel.get(i)
            var nw = n.width !== undefined ? n.width : 0
            var nh = n.height !== undefined ? n.height : 0
            minX = Math.min(minX, n.x)
            minY = Math.min(minY, n.y)
            maxX = Math.max(maxX, n.x + nw)
            maxY = Math.max(maxY, n.y + nh)
        }
        for (var j = 0; j < embedsModel.length; j++) {
            var e = embedsModel[j]
            var ew = e.width !== undefined ? e.width : 0
            var eh = e.height !== undefined ? e.height : 0
            minX = Math.min(minX, e.x)
            minY = Math.min(minY, e.y)
            maxX = Math.max(maxX, e.x + ew)
            maxY = Math.max(maxY, e.y + eh)
        }
        if (!isFinite(minX) || !isFinite(minY) || !isFinite(maxX) || !isFinite(maxY))
            return
        var ax = (minX + maxX) / 2
        var ay = (minY + maxY) / 2
        if (Math.abs(ax - worldAnchorX) > 0.01)
            worldAnchorX = ax
        if (Math.abs(ay - worldAnchorY) > 0.01)
            worldAnchorY = ay
    }

    // Issue #1035：对象几何是完整 world 事实，和相机变换彻底分离。
    // 这里只在模型真正变化时发布一次；原生图层相机变化只切换矩阵。
    function publishObjectGeometry() {
        var nodes = []
        for (var i = 0; i < nodesModel.count; i++) {
            var n = nodesModel.get(i)
            nodes.push({
                id: n.id,
                x: n.x,
                y: n.y,
                width: n.width,
                height: n.height
            })
        }
        var embeds = []
        for (var j = 0; j < embedsModel.length; j++) {
            var e = embedsModel[j]
            embeds.push({
                instanceId: e.instanceId,
                x: e.x,
                y: e.y,
                width: e.width,
                height: e.height
            })
        }
        var next = JSON.stringify({ nodes: nodes, embeds: embeds })
        if (next !== objectGeometryJson) {
            objectGeometryJson = next
            objectNodesJson = JSON.stringify(nodes)
            objectEmbedsJson = JSON.stringify(embeds)
            objectGeometryChanged()
        }
    }

    function buildModels(notify) {
        // graph.nodes[].position 是节点位置唯一真相，直接从 graph 派生平台显示模型。
        // Issue #984：普通节点是「名称 + 正文」文字卡片，宽高统一由图级
        // 布局快照提供；快照由平台文本排版器测量整张图（包括尚未打开的
        // 子图），绘制 / 命中 / 连线 / Embed 概览共用这一份。
        var desiredNodes = [];
        // Issue #801 评论 5895625744: newEmbeds 提前声明，旧 portal Node 也往这里归一。
        var newEmbeds = [];
        var graphNodes = graphData && graphData.nodes ? graphData.nodes : [];
        var layoutSizes = textMeasurer
                ? StarMapNodeMetrics.snapshotFor(starmapId, graphNodes, textMeasurer.measureNode)
                : StarMapNodeMetrics.snapshotSizes(starmapId);
        for (var i = 0; i < graphNodes.length; i++) {
            var gn = graphNodes[i];
            var pos = gn.position || { x: 0, y: 0 };
            // Issue #801 评论 5895625744: 旧 portal Node 在模型转换层归一到 Embed，
            // 不作为普通 Node 下发给活动画布 delegate。保留旧节点的位置、标题和目标 starmap。
            // Canvas 以后只有一种子星图语义：Embed。
            if (gn.portal && gn.portal.destinationStarmapId) {
                // Issue #801 评论 5896594591: 旧 portal 真实身份是 Node，操作必须走 Node API。
                // instanceId 加前缀仅作 UI 唯一 key（不与真实 Embed instanceId 冲突），
                // legacyPortalNodeId 保存真实 Node ID 供操作分流。
                newEmbeds.push({
                    instanceId: "legacy-portal:" + gn.id,
                    legacyPortalNodeId: gn.id,
                    targetStarmapId: gn.portal.destinationStarmapId,
                    label: gn.title || qsTr("未命名"),
                    x: pos.x,
                    y: pos.y,
                    width: _embedDiameter,
                    height: _embedDiameter,
                    hostPath: null
                });
            } else {
                var size = StarMapNodeMetrics.sizeFor(layoutSizes, gn);
                desiredNodes.push({
                    id: gn.id,
                    title: gn.title,
                    kind: gn.kind,
                    // Issue #984：完整 content（type/summary/body 等）必须带给
                    // delegate 与度量函数，不能只留标题。
                    content: gn.content || { type: "empty" },
                    x: pos.x,
                    y: pos.y,
                    width: size.width,
                    height: size.height,

                    tags: gn.tags
                });
            }
        }
        syncNodeRows(desiredNodes);

        var newEdges = [];
        var graphEdges = graphData && graphData.edges ? graphData.edges : [];
        for (var j = 0; j < graphEdges.length; j++) {
            var ge = graphEdges[j];
            // from/to 现在是 StarMapTargetPathDto 路径对象，保留完整路径供 Core 统一解析。
            newEdges.push({ id: ge.id, fromPath: ge.from, toPath: ge.to, kind: ge.kind, label: ge.label });
        }
        edgesModel = newEdges;

        // Issue #796 评论 5886483653: 从 graphData.embeds 构造 embedsModel。
        // 位置只读 embed.position，宽高继续属于 Linux 显示层（用默认值，和 node 一致）。
        // Issue #801 评论 5895625744: 不再用"子星图"当 label fallback，改用未命名占位，
        // 不再把对象类型当标题显示。newEmbeds 已在上方声明（旧 portal Node 也往里归一）。
        var graphEmbeds = graphData && graphData.embeds ? graphData.embeds : [];
        for (var k = 0; k < graphEmbeds.length; k++) {
            var gem = graphEmbeds[k];
            var epos = gem.position || { x: 0, y: 0 };
            newEmbeds.push({
                instanceId: gem.instanceId,
                targetStarmapId: gem.targetStarmapId || "",
                label: gem.label || qsTr("未命名"),
                x: epos.x,
                y: epos.y,
                width: _embedDiameter,
                height: _embedDiameter,
                hostPath: gem.hostPath || null
            });
        }
        embedsModel = newEmbeds;

        updateWorldAnchor();
        publishObjectGeometry();
        if (notify !== false)
            graphChanged();
    }

    function getNode(id) {
        for (var i = 0; i < nodesModel.count; i++) {
            var node = nodesModel.get(i);
            if (node.id === id) return node;
        }
        return null;
    }

    function _nodeRowIndex(id) {
        for (var i = 0; i < nodesModel.count; i++) {
            if (nodesModel.get(i).id === id) return i;
        }
        return -1;
    }

    function _sameNodeRow(rowNode, desired) {
        return rowNode.id === desired.id
                && rowNode.title === desired.title
                && rowNode.kind === desired.kind
                && rowNode.x === desired.x
                && rowNode.y === desired.y
                && rowNode.width === desired.width
                && rowNode.height === desired.height
                && JSON.stringify(rowNode.content) === JSON.stringify(desired.content)

                && JSON.stringify(rowNode.tags) === JSON.stringify(desired.tags);
    }

    // 稳定行身份同步：已有节点只更新 row 的字段，只有新增/删除才增删行。
    // 这样 Repeater 的 delegate 在编辑保存、拖动落盘、文本重测时不会被重建。
    function syncNodeRows(desiredNodes) {
        var seen = {};
        for (var i = 0; i < desiredNodes.length; i++) {
            var node = desiredNodes[i];
            seen[node.id] = true;
            var row = _nodeRowIndex(node.id);
            if (row >= 0) {
                if (!_sameNodeRow(nodesModel.get(row), node))
                    nodesModel.set(row, node);
            } else {
                nodesModel.append(node);
            }
        }
        for (var j = nodesModel.count - 1; j >= 0; j--) {
            var existingId = nodesModel.get(j).id;
            if (!seen[existingId])
                nodesModel.remove(j);
        }
    }

    function findNodeAt(wx, wy) {
        if (!ensureBackend()) return null;
        var res = normalizeBackendResult(starmapBackendRef.hit_test_scene_node(sceneCacheKey, wx, wy), "");
        if (res.success && res.data) {
            return getNode(res.data);
        }
        return null;
    }

    // Issue #793 评论 5884923277: copyObject 保留供其它需要浅拷贝的路径使用。
    function copyObject(src) {
        var dst = {}
        for (var key in src)
            dst[key] = src[key]
        return dst
    }

    // Issue #814 评论 5935285879: applySelection 数组重建已删除。
    // nodesModel/edgesModel/embedsModel 只保存图数据，不再把 isSelected 当模型
    // 状态反复浅拷贝。选中状态统一由共享 selectionController 维护，
    // Node/Embed/Edge 的 isSelected 从 selectionController.matches 派生。
    // 保留空 applySelection 仅为兼容可能的外部调用，不再重建数组。
    function applySelection(nodeId, edgeId, embedId) {
        if (selectionController) {
            if (nodeId !== "")
                selectionController.select(pathKey, "node", nodeId)
            else if (edgeId !== "")
                selectionController.select(pathKey, "edge", edgeId)
            else if (embedId !== "")
                selectionController.select(pathKey, "embed", embedId)
            else
                selectionController.clear()
        }
    }

    function clearSelection() {
        if (selectionController)
            selectionController.clear()
        selectionCleared()
    }

    function selectNode(nodeId) {
        if (selectionController)
            selectionController.select(pathKey, "node", nodeId)
        var node = getNode(nodeId)
        if (node) nodeSelected(node)
        return node
    }

    function selectEdge(edgeId) {
        if (selectionController)
            selectionController.select(pathKey, "edge", edgeId)
        var edge = null
        for (var i = 0; i < edgesModel.length; i++) {
            if (edgesModel[i].id === edgeId) { edge = edgesModel[i]; break }
        }
        if (edge) edgeSelected(edge)
        return edge
    }

    // Issue #796 评论 5886483653: Embed 选中。
    function selectEmbed(instanceId) {
        if (selectionController)
            selectionController.select(pathKey, "embed", instanceId)
        var embed = getEmbed(instanceId)
        if (embed) embedSelected(embed)
        return embed
    }

    function getEmbed(instanceId) {
        for (var i = 0; i < embedsModel.length; i++) {
            if (embedsModel[i].instanceId === instanceId) return embedsModel[i]
        }
        return null
    }

    // Issue #805 评论 5908703621 问题 4：Controller 不再自己生成完整路径，
    // 只返回当前 Embed 的一个 segment。Canvas 的 embedPath() 用 pathSegments.concat
    // 拼完整路径，这样第 N 层连线端点不会退化成第一层。
    function embedPathSegment(instanceId) {
        var embed = getEmbed(instanceId)
        if (embed && embed.legacyPortalNodeId) {
            return {
                type: "enterPortal",
                instanceId: null,
                nodeId: embed.legacyPortalNodeId
            }
        }
        return {
            type: "enterEmbed",
            instanceId: instanceId,
            nodeId: null
        }
    }

    // Issue #822 评论 5977278030：路径段 → UI instanceId 的映射不再由 QML 持有。
    // 深路径投影（含旧 portal 归一身份）只存在于 Rust edge_render 的
    // resolve_edge_endpoint_anchor；QML 拉线预览把路径整条交给平台 edge renderer，
    // 不再需要、也不应该再抄一份身份映射。

    // Issue #822 评论 5972215936：Embed 是正圆，命中先做圆内判定：
    // 圆外哪怕还在外接矩形里，也必须继续判为空白 / 下面的对象。
    // 圆内再分：顶部标题带、圆周边框 → chrome；其余圆内区域 → childContent。
    // 纯本地几何判断，不需要后端。
    function _embedCircle(em) {
        return {
            cx: em.x + em.width / 2,
            cy: em.y + em.height / 2,
            radius: Math.min(em.width, em.height) / 2
        }
    }

    function _insideEmbedCircle(em, wx, wy) {
        var c = _embedCircle(em)
        var dx = wx - c.cx
        var dy = wy - c.cy
        return dx * dx + dy * dy <= c.radius * c.radius
    }

    // 圆周边框：由内半径到圆边之间的一圈（不是外接矩形的四条边）。
    function _insideEmbedBorderRing(em, wx, wy) {
        var c = _embedCircle(em)
        var dx = wx - c.cx
        var dy = wy - c.cy
        var inner = c.radius - _borderSlop
        return dx * dx + dy * dy >= inner * inner
    }

    // findEmbedChromeAt 只判断 chrome 命中区域（顶部标题带 + 圆周边框），
    // 圆内其余区域返回 null。父 Canvas 不再把整个 Embed 矩形判成命中。
    function findEmbedChromeAt(wx, wy) {
        for (var i = 0; i < embedsModel.length; i++) {
            var em = embedsModel[i]
            if (!_insideEmbedCircle(em, wx, wy)) continue
            // 顶部标题带（圆内、从圆顶往下 _chromeHeight 高）
            if (wy <= em.y + _chromeHeight) return em
            // 圆周边框
            if (_insideEmbedBorderRing(em, wx, wy)) return em
        }
        return null
    }

    // Issue #814 评论 5945557717 问题 1: findEmbedContentAt 判断"圆内、
    // 但不在 chrome 的区域"——即子星图 contentViewport 的命中区域。
    // 父 Scene 的 pointer_press 据此把合法的子场景内部点击记成 childContent，
    // 不再冒充 empty（empty 应只表示坐标/命中错误）。带 instanceId。
    // 纯本地几何判断，不需要后端。
    function findEmbedContentAt(wx, wy) {
        for (var i = 0; i < embedsModel.length; i++) {
            var em = embedsModel[i]
            if (!_insideEmbedCircle(em, wx, wy)) continue
            // 排除 chrome 区域（顶部标题带 + 圆周边框），命中 chrome 不算 content
            if (wy <= em.y + _chromeHeight) continue
            if (_insideEmbedBorderRing(em, wx, wy)) continue
            return em
        }
        return null
    }

    // Issue #793 评论 5884923277: createNode 接收 title，不再写死"新节点"。
    function createNode(title, wx, wy) {
        if (!ensureBackend()) return;
        var res = normalizeBackendResult(starmapBackendRef.create_starmap_node(starmapId, title, "Note", wx, wy), qsTr("创建节点失败"));
        if (res.success) {
            clearError();
            noteHumanActivity();
            loadGraph(true);
            selectNode(res.data.id);
            structureChanged();
        } else {
            setError(backendErrorText(res, qsTr("创建节点失败")));
        }
    }

    function createEdge(fromId, toId) {
        if (!ensureBackend()) return;
        var res = normalizeBackendResult(starmapBackendRef.create_starmap_edge(starmapId, fromId, toId, "RelatedTo", ""), qsTr("创建连线失败"));
        if (res.success) {
            clearError();
            noteHumanActivity();
            loadGraph(true);
        } else {
            setError(backendErrorText(res, qsTr("创建连线失败")));
        }
    }

    // Issue #796 评论 5887280405: 用 fromPath/toPath 建边（path 版），
    // 支持 Node 和 Embed 作为端点。fromPath/toPath 是 StarMapTargetPathDto 的 JS 对象，
    // 由 StarMapCanvas.nodePath()/embedPath() 构造，这里 JSON.stringify 后传给后端。
    // Issue #814 评论 5945557717 问题 3: createEdgeWithPaths 返回 true/false，
    // Canvas 的 connect_end.success 必须使用这个真实后端结果，不再无条件写 true。
    function createEdgeWithPaths(fromPath, toPath) {
        if (!ensureBackend()) return false;
        var res = normalizeBackendResult(
            starmapBackendRef.create_starmap_edge_with_paths(
                starmapId,
                JSON.stringify(fromPath),
                JSON.stringify(toPath),
                "RelatedTo",
                ""
            ),
            qsTr("创建连线失败")
        );
        if (res.success) {
            clearError();
            noteHumanActivity();
            loadGraph(true);
            return true;
        } else {
            setError(backendErrorText(res, qsTr("创建连线失败")));
            return false;
        }
    }

    // Issue #984：patch 原样交给后端（content 也是 patch 的一部分）。
    // 成功后把 Core 返回的更新后 StarMapNodeDto 合并回 graphData，再按稳定
    // 行身份刷新模型；delegate 不会因为一次保存被销毁，场景级编辑器可以
    // 在拿到 success 后再退出。
    // 返回 { success, message }：编辑层只有拿到 success 才退出，失败保留草稿。
    function updateNode(nodeId, patch) {
        if (!ensureBackend())
            return { success: false, message: errorMessage }
        var res = normalizeBackendResult(starmapBackendRef.update_starmap_node(starmapId, nodeId, JSON.stringify(withExpectedRevision(patch, revisionOfNode(nodeId)))), qsTr("更新节点失败"));
        if (res.success) {
            clearError();
            var updated = res.data;
            if (!updated || updated.id !== nodeId) {
                // 返回结构不可用时退回全量加载，仍然不能只改 title。
            noteHumanActivity();
            loadGraph(true);
                if (graphData === null)
                    return { success: false, message: errorMessage || qsTr("更新节点失败") }
                structureChanged()
                return { success: true }
            }

            // graphData 是节点内容、边渲染与子图概览的事实输入，先替换为完整 DTO。
            if (!_replaceGraphObject("nodes", "id", nodeId, updated)) {
                loadGraph(true)
                if (graphData === null)
                    return { success: false, message: errorMessage || qsTr("更新节点失败") }
                structureChanged()
                return { success: true }
            }

            // 内容签名变化的节点会在 buildModels 里被重新排版测量；
            // 模型只 set 对应 row，Repeater delegate 保持稳定。
            buildModels();
            computeEdgeRenders(null);
            structureChanged()
            return { success: true }
        } else {
            setError(backendErrorText(res, qsTr("更新节点失败")));
            return { success: false, message: errorMessage }
        }
    }

    function deleteNode(nodeId) {
        if (!ensureBackend()) return;
        var res = normalizeBackendResult(starmapBackendRef.delete_starmap_node(starmapId, nodeId, revisionOfNode(nodeId) || 0), qsTr("删除节点失败"));
        if (res.success) {
            clearError();
            noteHumanActivity();
            loadGraph(true);
            clearSelection();
            structureChanged();
        } else {
            _reloadOnConflict(res);
            setError(backendErrorText(res, qsTr("删除节点失败")));
        }
    }

    function updateEdge(edgeId, patch) {
        if (!ensureBackend()) return;
        var res = normalizeBackendResult(starmapBackendRef.update_starmap_edge(starmapId, edgeId, JSON.stringify(withExpectedRevision(patch, revisionOfEdge(edgeId)))), qsTr("更新连线失败"));
        if (res.success) {
            clearError();
            var updated = res.data;
            if (!updated || updated.id !== edgeId
                    || !_replaceGraphObject("edges", "id", edgeId, updated)) {
            noteHumanActivity();
            loadGraph(true);
                return;
            }
            var nextEdges = [];
            for (var i = 0; i < edgesModel.length; i++) {
                var e = copyObject(edgesModel[i]);
                if (e.id === edgeId) {
                    e.label = updated.label;
                    e.kind = updated.kind;
                }
                nextEdges.push(e);
            }
            edgesModel = nextEdges;
            computeEdgeRenders(null);
            graphChanged();
        } else {
            setError(backendErrorText(res, qsTr("更新连线失败")));
        }
    }

    function deleteEdge(edgeId) {
        if (!ensureBackend()) return;
        var res = normalizeBackendResult(starmapBackendRef.delete_starmap_edge(starmapId, edgeId, revisionOfEdge(edgeId) || 0), qsTr("删除连线失败"));
        if (res.success) {
            clearError();
            noteHumanActivity();
            loadGraph(true);
            clearSelection();
        } else {
            _reloadOnConflict(res);
            setError(backendErrorText(res, qsTr("删除连线失败")));
        }
    }

    // Issue #796 评论 5886483653: Embed 增删改（后端 create_starmap_embed /
    // update_starmap_embed / delete_starmap_embed 由 Canvas 在 createSubStarmapAt
    // 里直接调，这里提供 update/delete 供右键菜单复用）。
    function updateEmbed(instanceId, patch) {
        if (!ensureBackend()) return;
        // Issue #801 评论 5896594591: 旧 portal 真实身份是 Node，操作分流到 Node API。
        var embed = getEmbed(instanceId);
        if (embed && embed.legacyPortalNodeId) {
            var nodePatch = {};
            if (patch.label !== undefined) nodePatch.title = patch.label;
            if (patch.position !== undefined) nodePatch.position = patch.position;
            var nodeRes = normalizeBackendResult(starmapBackendRef.update_starmap_node(starmapId, embed.legacyPortalNodeId, JSON.stringify(withExpectedRevision(nodePatch, revisionOfNode(embed.legacyPortalNodeId)))), qsTr("更新节点失败"));
            if (nodeRes.success) {
                clearError();
                var updatedNode = nodeRes.data;
                if (!updatedNode || updatedNode.id !== embed.legacyPortalNodeId
                        || !_replaceGraphObject("nodes", "id", embed.legacyPortalNodeId, updatedNode)) {
                    // Core 已保存成功；即使返回 DTO 不完整、无法原地替换，
                    // 重新载入图后也必须通知左栏刷新标题。
            noteHumanActivity();
            loadGraph(true);
                    structureChanged();
                    return;
                }
                var nextEmbeds = [];
                for (var i = 0; i < embedsModel.length; i++) {
                    var em = copyObject(embedsModel[i]);
                    if (em.instanceId === instanceId) {
                        if (updatedNode.title !== undefined) em.label = updatedNode.title;
                        if (updatedNode.position !== undefined) {
                            em.x = updatedNode.position.x;
                            em.y = updatedNode.position.y;
                        }
                    }
                    nextEmbeds.push(em);
                }
                embedsModel = nextEmbeds;
                publishObjectGeometry();
                computeEdgeRenders(null);
                graphChanged();
                structureChanged();
            } else {
                setError(backendErrorText(nodeRes, qsTr("更新节点失败")));
            }
            return;
        }
        // 正式 Embed 继续走 Embed API（原逻辑不变）
        var res = normalizeBackendResult(starmapBackendRef.update_starmap_embed(starmapId, instanceId, JSON.stringify(withExpectedRevision(patch, revisionOfEmbed(instanceId)))), qsTr("更新子星图入口失败"));
        if (res.success) {
            clearError();
            var updated = res.data;
            if (!updated || updated.instanceId !== instanceId
                    || !_replaceGraphObject("embeds", "instanceId", instanceId, updated)) {
                // 正式 Embed 同样已在 Core 保存成功，重载后补发结构变更。
            noteHumanActivity();
            loadGraph(true);
                structureChanged();
                return;
            }
            var nextEmbeds = [];
            for (var i = 0; i < embedsModel.length; i++) {
                var em = copyObject(embedsModel[i]);
                if (em.instanceId === instanceId) {
                    if (updated.label !== undefined) em.label = updated.label;
                    if (updated.position !== undefined) {
                        em.x = updated.position.x;
                        em.y = updated.position.y;
                    }
                }
                nextEmbeds.push(em);
            }
            embedsModel = nextEmbeds;
            publishObjectGeometry();
            computeEdgeRenders(null);
            graphChanged();
            structureChanged();
        } else {
            setError(backendErrorText(res, qsTr("更新子星图入口失败")));
        }
    }

    function deleteEmbed(instanceId) {
        if (!ensureBackend()) return;
        // Issue #801 评论 5896594591: 旧 portal 走 Node API。
        var embed = getEmbed(instanceId);
        if (embed && embed.legacyPortalNodeId) {
            var nodeRes = normalizeBackendResult(starmapBackendRef.delete_starmap_node(
                starmapId, embed.legacyPortalNodeId, revisionOfNode(embed.legacyPortalNodeId) || 0),
                qsTr("删除节点失败"));
            if (nodeRes.success) {
                clearError();
            noteHumanActivity();
            loadGraph(true);
                clearSelection();
                structureChanged();
            } else {
                _reloadOnConflict(nodeRes);
                setError(backendErrorText(nodeRes, qsTr("删除节点失败")));
            }
            return;
        }
        // 正式 Embed 继续走 Embed API（原逻辑不变）
        var res = normalizeBackendResult(starmapBackendRef.delete_starmap_embed(
            starmapId, instanceId, revisionOfEmbed(instanceId) || 0),
            qsTr("删除子星图入口失败"));
        if (res.success) {
            clearError();
            noteHumanActivity();
            loadGraph(true);
            clearSelection();
            structureChanged();
        } else {
            _reloadOnConflict(res);
            setError(backendErrorText(res, qsTr("删除子星图入口失败")));
        }
    }

    // Issue #798: 从 StarMapCanvas 移入，Controller 成为图操作唯一入口。
    // Issue #805 评论 5907045450 第 5 部分：改走 Core 原子接口
    // create_starmap_child_embed，不再用 create_starmap → create_starmap_embed →
    // 失败时 delete_starmap 的非原子拼接。Core 侧通过 starmap_child_embed
    // journal 保证 crash-safe，QML 不再需要回滚。
    function createSubStarmapAt(title, wx, wy) {
        if (!ensureBackend()) return;
        // 原子创建子星图并嵌入当前图，Core 一次性完成 meta + embed + history。
        var res = normalizeBackendResult(
            starmapBackendRef.create_starmap_child_embed(starmapId, title, wx, wy),
            qsTr("创建子星图失败")
        );
        if (!res.success) {
            setError(backendErrorText(res, qsTr("创建子星图失败")));
            return;
        }
        var instanceId = res.data && res.data.embed && res.data.embed.instanceId
            ? res.data.embed.instanceId : "";
        // 成功，reload graph 并选中新 Embed
        clearError();
            noteHumanActivity();
            loadGraph(true);
        if (instanceId) {
            selectEmbed(instanceId);
        }
        structureChanged();
    }

    // Issue #798: 拖动结束后提交节点新位置。先持久化单节点 position，
    // 后端成功后再浅拷贝新数组一次赋值更新 canonical model。
    // 不再调用全量 saveLayout()，避免拖一个节点把所有节点逐个重写。
    // 失败时 canonical model 不动，结束 transient move 后 delegate 因 binding
    // 自动回旧位置（与 commitEmbedMove 顺序一致）。
    function commitNodeMove(nodeId, nx, ny, trajectory) {
        if (!ensureBackend()) return false;
        var movePatch = { position: { x: nx, y: ny } }
        // Issue #1036：轨迹只是回放显示辅助，最终坐标仍来自持久化 position。
        if (trajectory && trajectory.length > 0)
            movePatch.trajectory = trajectory
        var res = normalizeBackendResult(
            starmapBackendRef.update_starmap_node(starmapId, nodeId, JSON.stringify(withExpectedRevision(movePatch, revisionOfNode(nodeId)))),
            qsTr("更新节点位置失败")
        );
        if (res.success) {
            clearError();
            var updated = res.data;
            if (!updated || updated.id !== nodeId
                    || !_replaceGraphObject("nodes", "id", nodeId, updated)
                    || !updated.position) {
            noteHumanActivity();
            loadGraph(true);
                return true;
            }
            var row = _nodeRowIndex(nodeId);
            if (row >= 0) {
                nodesModel.setProperty(row, "x", updated.position.x);
                nodesModel.setProperty(row, "y", updated.position.y);
            }
            publishObjectGeometry();
            computeEdgeRenders(null);
            graphChanged();
            return true;
        } else {
            _reloadOnConflict(res);
            setError(backendErrorText(res, qsTr("更新节点位置失败")));
            // Issue #798 评论 5892406254: 提交失败时 canonical model 未动，
            // 但 edgeRenders 已被 transient move 更新成临时坐标。
            // 标记缓存过期，下一次计算从 canonical 布局重建。
            _sceneCacheDirty = true;
            computeEdgeRenders(null);
            graphChanged();
            return false;
        }
    }

    // Issue #798: 拖动结束后提交 Embed 新位置。先持久化到后端，
    // 再浅拷贝新数组一次赋值更新本地模型。
    function commitEmbedMove(instanceId, nx, ny, trajectory) {
        if (!ensureBackend()) return false;
        var embedMovePatch = { position: { x: nx, y: ny } }
        if (trajectory && trajectory.length > 0)
            embedMovePatch.trajectory = trajectory
        // Issue #801 评论 5896594591: 旧 portal 移动走 Node API。
        var embed = getEmbed(instanceId);
        if (embed && embed.legacyPortalNodeId) {
            var nodeRes = normalizeBackendResult(
                starmapBackendRef.update_starmap_node(starmapId, embed.legacyPortalNodeId, JSON.stringify(withExpectedRevision(embedMovePatch, revisionOfNode(embed.legacyPortalNodeId)))),
                qsTr("更新节点位置失败")
            );
            if (nodeRes.success) {
                clearError();
                var updatedNode = nodeRes.data;
                if (!updatedNode || updatedNode.id !== embed.legacyPortalNodeId
                        || !_replaceGraphObject("nodes", "id", embed.legacyPortalNodeId, updatedNode)
                        || !updatedNode.position) {
            noteHumanActivity();
            loadGraph(true);
                    return true;
                }
                var nextEmbeds = [];
                for (var i = 0; i < embedsModel.length; i++) {
                    var em = copyObject(embedsModel[i]);
                    if (em.instanceId === instanceId) {
                        em.x = updatedNode.position.x;
                        em.y = updatedNode.position.y;
                    }
                    nextEmbeds.push(em);
                }
                embedsModel = nextEmbeds;
                publishObjectGeometry();
                computeEdgeRenders(null);
                graphChanged();
                return true;
            } else {
                _reloadOnConflict(nodeRes);
                setError(backendErrorText(nodeRes, qsTr("更新节点位置失败")));
                _sceneCacheDirty = true;
                computeEdgeRenders(null);
                graphChanged();
                return false;
            }
        }
        // 正式 Embed 继续走 Embed API（原逻辑不变）
        var res = normalizeBackendResult(
            starmapBackendRef.update_starmap_embed(starmapId, instanceId, JSON.stringify(withExpectedRevision(embedMovePatch, revisionOfEmbed(instanceId)))),
            qsTr("更新子星图入口失败")
        );
        if (res.success) {
            clearError();
            var updated = res.data;
            if (!updated || updated.instanceId !== instanceId
                    || !_replaceGraphObject("embeds", "instanceId", instanceId, updated)
                    || !updated.position) {
            noteHumanActivity();
            loadGraph(true);
                return true;
            }
            var nextEmbeds = [];
            for (var i = 0; i < embedsModel.length; i++) {
                var em = copyObject(embedsModel[i]);
                if (em.instanceId === instanceId) {
                    em.x = updated.position.x;
                    em.y = updated.position.y;
                }
                nextEmbeds.push(em);
            }
            embedsModel = nextEmbeds;
            publishObjectGeometry();
            computeEdgeRenders(null);
            graphChanged();
            return true;
        } else {
            _reloadOnConflict(res);
            setError(backendErrorText(res, qsTr("更新子星图入口失败")));
            // Issue #798 评论 5892406254: 提交失败时 canonical model 未动，
            // 但 edgeRenders 已被 transient move 更新成临时坐标。
            // 标记缓存过期，下一次计算从 canonical 布局重建。
            _sceneCacheDirty = true;
            computeEdgeRenders(null);
            graphChanged();
            return false;
        }
    }

    // 边端点解析用的本层 node layout（含旧 portal 归一条目）。
    // computeEdgeRenders 与候选边预览共用同一份几何，避免两处拼装漂移。
    function nodeLayoutEntries(moveOverride) {
        var nodePos = [];
        for (var j = 0; j < nodesModel.count; j++) {
            var n = nodesModel.get(j);
            var nx = n.x, ny = n.y;
            if (moveOverride && moveOverride.kind === "node" && moveOverride.id === n.id) {
                nx = moveOverride.x; ny = moveOverride.y;
            }
            nodePos.push({ id: n.id, x: nx, y: ny, width: n.width, height: n.height });
        }
        // Issue #801 评论 5895709352: 旧 portal Node 已归一到 embedsModel，不再出现在
        // nodesModel，但边端点解析仍按 node id 查 layout（本地 Node 目标走
        // node_layout_center，EnterPortal 段也要求 portal 节点在 layout 里）。
        // 不补这份几何，连到旧 portal 节点的边会整条消失（LocalNodeMissing/PortalMissing）。
        // 位置读归一条目，拖动中的位置用 embed move override 对齐。
        var canonicalNodes = graphData && graphData.nodes ? graphData.nodes : [];
        for (var p = 0; p < canonicalNodes.length; p++) {
            var pn = canonicalNodes[p];
            if (!pn.portal || !pn.portal.destinationStarmapId) continue;
            var portalEntry = getEmbed("legacy-portal:" + pn.id);
            if (!portalEntry) continue;
            var px = portalEntry.x, py = portalEntry.y;
            if (moveOverride && moveOverride.kind === "embed" && moveOverride.id === portalEntry.instanceId) {
                px = moveOverride.x; py = moveOverride.y;
            }
            nodePos.push({ id: pn.id, x: px, y: py, width: portalEntry.width, height: portalEntry.height });
        }
        return nodePos;
    }

    function embedLayoutEntries(moveOverride) {
        var embedPos = [];
        for (var k = 0; k < embedsModel.length; k++) {
            var em = embedsModel[k];
            var ex = em.x, ey = em.y;
            if (moveOverride && moveOverride.kind === "embed" && moveOverride.id === em.instanceId) {
                ex = moveOverride.x; ey = moveOverride.y;
            }
            embedPos.push({ instanceId: em.instanceId, x: ex, y: ey, width: em.width, height: em.height });
        }
        return embedPos;
    }

    function syncSceneCache() {
        if (!ensureBackend() || !graphData) return false;
        // 正常选择 / pointerMode 变化不重传整图；只有图、布局或位置真的
        // 变化（_replaceGraphObject / loadGraph）才同步一次显示缓存。
        if (!_sceneCacheDirty)
            return true;
        var res = normalizeBackendResult(starmapBackendRef.sync_starmap_scene(
            sceneCacheKey,
            JSON.stringify(graphData),
            JSON.stringify(nodeLayoutEntries(null)),
            JSON.stringify(embedLayoutEntries(null))), "");
        if (res.success === true)
            _sceneCacheDirty = false;
        return res.success === true;
    }

    function computeEdgeRenders(moveOverride) {
        if (!ensureBackend()) return;
        if (!graphData) return;
        // 非拖拽帧（加载 / 落盘 / 选择变化）先把结构快照同步进显示缓存；
        // 拖拽帧只传被拖对象的临时位置，其余几何直接复用缓存。
        var hasOverride = moveOverride
                && (moveOverride.kind === "node" || moveOverride.kind === "embed")
                && moveOverride.id !== undefined;
        if (!hasOverride) {
            updateWorldAnchor();
            if (!syncSceneCache())
                return;
        }
        var res = normalizeBackendResult(starmapBackendRef.compute_scene_edge_renders(
            sceneCacheKey,
            hasOverride ? moveOverride.kind : "",
            hasOverride ? moveOverride.id : "",
            hasOverride ? moveOverride.x : 0,
            hasOverride ? moveOverride.y : 0,
            hasOverride), "");
        if (res.success && res.data) {
            var nextJson = JSON.stringify(res.data);
            // 相机 / 选择变化但几何未变时保持同一字符串，原生边层不重新解析。
            if (nextJson !== edgeRendersJson) {
                edgeRenders = res.data;
                edgeRendersJson = nextJson;
                var labels = [];
                for (var i = 0; i < res.data.length; i++) {
                    var render = res.data[i];
                    if (render.label && render.label.length > 0)
                        labels.push(render);
                }
                edgeLabels = labels;
            }
        }
    }

    // Issue #822 评论 5977278030：候选边预览（拉线用）。
    // 把 prospective LCA 规划出的 from/to 路径交给平台 edge renderer，
    // 在现有边表上临时追加候选边后只取它自己的 render：
    // Node 矩形 / Embed 圆周、旧 portal 归一、深路径投影、已有反向边时的
    // 双向偏移全部与正式的松手结果同源，QML 不需要再抄第二份几何。
    // 返回 null 表示端点当前无法在宿主图定位（退回自由预览）。
    function computeProspectiveEdgeRender(fromPath, toPath) {
        if (!graphData) return null;
        if (!ensureBackend()) return null;
        var res = normalizeBackendResult(starmapBackendRef.compute_scene_prospective_edge_render(
            sceneCacheKey,
            JSON.stringify(fromPath),
            JSON.stringify(toPath)), "");
        if (res.success && res.data) return res.data;
        return null;
    }

    // `threshold` 由归属层按屏幕像素折算（屏幕像素 ÷ effectiveScale），
    // 不再吃固定的 world 阈值：相机允许 1e-4~1e5 后，world 阈值在屏幕上的
    // 手感会差几个数量级。
    function hitTestEdge(wx, wy, threshold) {
        if (!ensureBackend()) return null;
        if (!edgeRenders || edgeRenders.length === 0) computeEdgeRenders(null);
        var res = normalizeBackendResult(starmapBackendRef.hit_test_scene_edge(sceneCacheKey, wx, wy, threshold), "");
        if (res.success && res.data) {
            for (var i = 0; i < edgesModel.length; i++) {
                if (edgesModel[i].id === res.data) return edgesModel[i];
            }
        }
        return null;
    }

    // 屏幕空间命中：与原生连线图层共用 f64 投影、固定像素箭头与容差。
    // 世界几何仍只来自 EdgeRender，不在 QML 里复制另一套箭头。
    function hitTestEdgeScreen(sx, sy, zoom, panX, panY, thresholdPx) {
        if (!ensureBackend()) return null;
        if (!edgeRenders || edgeRenders.length === 0) computeEdgeRenders(null);
        var res = normalizeBackendResult(starmapBackendRef.hit_test_scene_edge_screen(
            sceneCacheKey, sx, sy, zoom, panX, panY, thresholdPx), "");
        if (res.success && res.data) {
            for (var i = 0; i < edgesModel.length; i++) {
                if (edgesModel[i].id === res.data) return edgesModel[i];
            }
        }
        return null;
    }

    // 从 StarMapTargetPathDto 路径对象中提取本图节点 ID。
    // 仅当 target.type === "node" 且 segments 为空（即直接指向本图节点）时返回 nodeId，否则 null。
    function localNodeIdFromPath(path) {
        if (!path || !path.target) return null;
        if (path.target.type !== "node") return null;
        if (path.segments && path.segments.length > 0) return null;
        return path.target.nodeId || null;
    }

    function invalidateEdgeRenders() {
        edgeRenders = [];
        edgeRendersJson = "[]";
        edgeLabels = [];
    }

    // ---------------------------------------------------------------------------
    // Issue #834：内部链接（StarMapLink）菜单（add/update/delete/list）。
    // Link 是星图内部跳转（source/target 都是 StarMapTargetPathDto）。
    // 后端 bridge 已注册 add/update/delete/list_starmap_link
    // （starmap_backend/links.rs），这里只做 ensureBackend/normalizeBackendResult 包装。
    // ---------------------------------------------------------------------------
    function addLink(sourcePath, targetPath, label) {
        if (!ensureBackend()) return null;
        // bridge 层 StarMapLinkCreateInput（camelCase）：{source, target, label?}
        var body = { source: sourcePath, target: targetPath, label: label || null };
        var res = normalizeBackendResult(
            starmapBackendRef.add_starmap_link(starmapId, JSON.stringify(body)),
            qsTr("添加内部链接失败")
        );
        if (res.success) {
            clearError();
            return res.data;
        }
        setError(backendErrorText(res, qsTr("添加内部链接失败")));
        return null;
    }

    // patch 遵循 StarMapLinkPatchInputDto（camelCase）：{source?, target?, label?, clearLabel}。
    // 清空 label 由调用方传 clearLabel:true（不在此处隐式构造）。
    function updateLink(linkId, patch) {
        if (!ensureBackend()) return false;
        var res = normalizeBackendResult(
            starmapBackendRef.update_starmap_link(starmapId, linkId, JSON.stringify(withExpectedRevision(patch, revisionOfLink(linkId)))),
            qsTr("更新内部链接失败")
        );
        if (res.success) {
            clearError();
            var updated = res.data;
            if (!updated || updated.linkId !== linkId
                    || !_replaceGraphObject("links", "linkId", linkId, updated)) {
            noteHumanActivity();
            loadGraph(true);
                return true;
            }
            graphChanged();
            return true;
        }
        _reloadOnConflict(res);
        setError(backendErrorText(res, qsTr("更新内部链接失败")));
        return false;
    }

    function deleteLink(linkId) {
        if (!ensureBackend()) return false;
        var res = normalizeBackendResult(
            starmapBackendRef.delete_starmap_link(starmapId, linkId, revisionOfLink(linkId) || 0),
            qsTr("删除内部链接失败")
        );
        if (res.success) {
            clearError();
            noteHumanActivity();
            loadGraph(true);
            return true;
        }
        _reloadOnConflict(res);
        setError(backendErrorText(res, qsTr("删除内部链接失败")));
        return false;
    }

    // 返回 StarMapLinkDto 数组（list_starmap_links 返回
    // StarMapLinkListWithDiagnosticsDto，取 items）。
    function listLinks() {
        if (!ensureBackend()) return [];
        var res = normalizeBackendResult(
            starmapBackendRef.list_starmap_links(starmapId),
            qsTr("列出内部链接失败")
        );
        if (res.success && res.data && res.data.items) {
            clearError();
            return res.data.items;
        }
        return [];
    }

    // Issue #834：与 createEdgeWithPaths 命名对称的 Link 版本，供
    // SceneContent.commitLinkWithPaths 调用。label 暂留空，后续可在菜单补编辑。
    function createLinkWithPaths(fromPath, toPath) {
        var link = addLink(fromPath, toPath, "");
        if (link !== null) {
            noteHumanActivity();
            loadGraph(true);
            return true;
        }
        return false;
    }
}
