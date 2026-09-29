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
// 数据流：starmapBackendRef (DTO) → controller (graphData/layoutData) → Canvas (nodesModel/edgesModel)
// =============================================================================

import QtQuick

QtObject {
    id: controller

    property string starmapId: ""
    property var starmapBackendRef: null
    property string errorMessage: ""
    property var graphData: null
    property var layoutData: null
    property var nodesModel: []
    property var edgesModel: []
    property var edgeRenders: []
    // Issue #796 评论 5886483653: 子星图 Embed 显示模型，从 graphData.embeds 派生。
    property var embedsModel: []

    signal graphChanged()
    signal selectionCleared()
    signal nodeSelected(var node)
    signal edgeSelected(var edge)
    signal embedSelected(var embed)

    onGraphChanged: invalidateEdgeRenders()

    function setError(msg) {
        errorMessage = msg || "";
        if (errorMessage) { /* error tracked via errorMessage property */ }
    }

    function clearError() { errorMessage = ""; }

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

    function loadGraph() {
        if (starmapId === "") return;
        if (!ensureBackend()) return;

        var res = normalizeBackendResult(starmapBackendRef.get_starmap_graph(starmapId), qsTr("加载星图数据失败"));
        if (res.success) {
            clearError();
            graphData = res.data.graph;
            layoutData = res.data.layout;
            buildModels();
        } else {
            setError(qsTr("加载星图数据失败"));
        }
    }

    function buildModels() {
        // graph.nodes[].position 是节点位置唯一真相，直接从 graph 派生平台显示模型。
        // 宽高/圆角是纯显示参数，用默认值（与 starmap_bridge.rs DEFAULT_NODE_* 一致）。
        var newNodes = [];
        var graphNodes = graphData && graphData.nodes ? graphData.nodes : [];
        for (var i = 0; i < graphNodes.length; i++) {
            var gn = graphNodes[i];
            var pos = gn.position || { x: 0, y: 0 };
            newNodes.push({
                id: gn.id,
                title: gn.title,
                kind: gn.kind,
                x: pos.x,
                y: pos.y,
                width: 150,
                height: 60,
                isSelected: false,
                payload: gn.payload,
                tags: gn.tags,
                portal: gn.portal
            });
        }
        nodesModel = newNodes;

        var newEdges = [];
        var graphEdges = graphData && graphData.edges ? graphData.edges : [];
        for (var j = 0; j < graphEdges.length; j++) {
            var ge = graphEdges[j];
            // from/to 现在是 StarMapTargetPathDto 路径对象，保留完整路径供 Core 统一解析。
            newEdges.push({ id: ge.id, fromPath: ge.from, toPath: ge.to, kind: ge.kind, label: ge.label, isSelected: false });
        }
        edgesModel = newEdges;

        // Issue #796 评论 5886483653: 从 graphData.embeds 构造 embedsModel。
        // 位置只读 embed.position，宽高继续属于 Linux 显示层（用默认值，和 node 一致）。
        var newEmbeds = [];
        var graphEmbeds = graphData && graphData.embeds ? graphData.embeds : [];
        for (var k = 0; k < graphEmbeds.length; k++) {
            var gem = graphEmbeds[k];
            var epos = gem.position || { x: 0, y: 0 };
            newEmbeds.push({
                instanceId: gem.instanceId,
                targetStarmapId: gem.targetStarmapId || "",
                label: gem.label || qsTr("子星图"),
                x: epos.x,
                y: epos.y,
                width: 150,
                height: 60,
                isSelected: false,
                hostPath: gem.hostPath || null
            });
        }
        embedsModel = newEmbeds;

        if (nodesModel.length > 0 && (!layoutData || !layoutData.nodes || layoutData.nodes.length === 0)) autoLayout();
        graphChanged();
    }

    function autoLayout() {
        if (!ensureBackend()) return;
        var nodeIds = [];
        for (var i = 0; i < nodesModel.length; i++) nodeIds.push(nodesModel[i].id);
        var existingJson = layoutData ? JSON.stringify(layoutData) : "{}";
        var res = normalizeBackendResult(starmapBackendRef.calculate_grid_layout(JSON.stringify(nodeIds), existingJson), qsTr("自动布局失败"));
        if (res.success && res.data && res.data.nodes) {
            var layoutNodes = res.data.nodes;
            for (var j = 0; j < nodesModel.length; j++) {
                for (var k = 0; k < layoutNodes.length; k++) {
                    if (nodesModel[j].id === layoutNodes[k].nodeId) {
                        nodesModel[j].x = layoutNodes[k].x;
                        nodesModel[j].y = layoutNodes[k].y;
                        break;
                    }
                }
            }
            nodesModelChanged();
            saveLayout();
        } else {
            // 后端失败直接报错，不再用 QML 临时坐标兜底成"成功"
            setError(qsTr("自动布局失败"));
        }
    }

    // 从 graph.nodes[].position 查节点位置，保持 layout.nodes[].nodeId/x/y 契约兼容。
    // 不再作为坐标真相源（buildModels 直接从 graph.position 取），仅作辅助查询。
    function getLayoutNode(id) {
        if (!graphData || !graphData.nodes) return null;
        for (var i = 0; i < graphData.nodes.length; i++) {
            var gn = graphData.nodes[i];
            if (gn.id === id) {
                var pos = gn.position || { x: 0, y: 0 };
                return { nodeId: gn.id, x: pos.x, y: pos.y, width: 150, height: 60 };
            }
        }
        return null;
    }

    function getNode(id) {
        for (var i = 0; i < nodesModel.length; i++) {
            if (nodesModel[i].id === id) return nodesModel[i];
        }
        return null;
    }

    function findNodeAt(wx, wy) {
        if (!ensureBackend()) return null;
        var layoutNodes = [];
        for (var i = 0; i < nodesModel.length; i++) {
            var n = nodesModel[i];
            layoutNodes.push({ nodeId: n.id, x: n.x, y: n.y, width: n.width, height: n.height, radius: 30, collapsed: false, zIndex: 0, scale: 1.0, depth: 0.0, focusWeight: 0.0, orbitGroup: null });
        }
        var res = normalizeBackendResult(starmapBackendRef.hit_test_nodes(JSON.stringify(layoutNodes), wx, wy), "");
        if (res.success && res.data) {
            return getNode(res.data);
        }
        return null;
    }

    // Issue #793 评论 5884923277: 选中状态用浅拷贝重新构造数组，
    // 不再原地改普通 JS 对象，保证 delegate 绑定的 nodeData.isSelected 有独立 notify。
    // Issue #796 评论 5886483653: applySelection 同时管理 node / embed / edge。
    function copyObject(src) {
        var dst = {}
        for (var key in src)
            dst[key] = src[key]
        return dst
    }

    function applySelection(nodeId, edgeId, embedId) {
        var nextNodes = []
        for (var i = 0; i < nodesModel.length; i++) {
            var n = copyObject(nodesModel[i])
            n.isSelected = nodeId !== "" && n.id === nodeId
            nextNodes.push(n)
        }

        var nextEdges = []
        for (var j = 0; j < edgesModel.length; j++) {
            var e = copyObject(edgesModel[j])
            e.isSelected = edgeId !== "" && e.id === edgeId
            nextEdges.push(e)
        }

        var nextEmbeds = []
        for (var m = 0; m < embedsModel.length; m++) {
            var em = copyObject(embedsModel[m])
            em.isSelected = embedId !== "" && em.instanceId === embedId
            nextEmbeds.push(em)
        }

        nodesModel = nextNodes
        edgesModel = nextEdges
        embedsModel = nextEmbeds
        graphChanged()
    }

    function clearSelection() {
        applySelection("", "", "")
        selectionCleared()
    }

    function selectNode(nodeId) {
        applySelection(nodeId, "", "")
        var node = getNode(nodeId)
        if (node) nodeSelected(node)
        return node
    }

    function selectEdge(edgeId) {
        applySelection("", edgeId, "")
        var edge = null
        for (var i = 0; i < edgesModel.length; i++) {
            if (edgesModel[i].id === edgeId) { edge = edgesModel[i]; break }
        }
        if (edge) edgeSelected(edge)
        return edge
    }

    // Issue #796 评论 5886483653: Embed 选中。
    function selectEmbed(instanceId) {
        applySelection("", "", instanceId)
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

    // Issue #796 评论 5886483653: 按世界坐标命中 Embed。
    // 复用 hit_test_nodes 后端能力（embed 几何与 node 同构）。
    function findEmbedAt(wx, wy) {
        if (!ensureBackend()) return null;
        var layoutEmbeds = [];
        for (var i = 0; i < embedsModel.length; i++) {
            var em = embedsModel[i];
            layoutEmbeds.push({ nodeId: em.instanceId, x: em.x, y: em.y, width: em.width, height: em.height, radius: 30, collapsed: false, zIndex: 0, scale: 1.0, depth: 0.0, focusWeight: 0.0, orbitGroup: null });
        }
        if (layoutEmbeds.length === 0) return null;
        var res = normalizeBackendResult(starmapBackendRef.hit_test_nodes(JSON.stringify(layoutEmbeds), wx, wy), "");
        if (res.success && res.data) {
            return getEmbed(res.data);
        }
        return null;
    }

    // Issue #793 评论 5884923277: createNode 接收 title，不再写死"新节点"。
    function createNode(title, wx, wy) {
        if (!ensureBackend()) return;
        var res = normalizeBackendResult(starmapBackendRef.create_starmap_node(starmapId, title, "Note", wx, wy), qsTr("创建节点失败"));
        if (res.success) {
            clearError();
            loadGraph();
            selectNode(res.data.id);
        } else {
            setError(backendErrorText(res, qsTr("创建节点失败")));
        }
    }

    function createEdge(fromId, toId) {
        if (!ensureBackend()) return;
        var res = normalizeBackendResult(starmapBackendRef.create_starmap_edge(starmapId, fromId, toId, "RelatedTo", ""), qsTr("创建连线失败"));
        if (res.success) {
            clearError();
            loadGraph();
        } else {
            setError(backendErrorText(res, qsTr("创建连线失败")));
        }
    }

    function saveLayout() {
        if (!ensureBackend()) return;
        var layoutNodes = [];
        for (var i = 0; i < nodesModel.length; i++) {
            var n = nodesModel[i];
            layoutNodes.push({ nodeId: n.id, x: n.x, y: n.y, width: n.width, height: n.height, radius: 30, collapsed: false, zIndex: 0 });
        }
        var res = normalizeBackendResult(starmapBackendRef.save_starmap_layout(starmapId, JSON.stringify({ kind: "Freeform", nodes: layoutNodes })), qsTr("保存布局失败"));
        if (res.success) clearError();
        else setError(qsTr("保存布局失败"));
    }

    function updateNode(nodeId, patch) {
        if (!ensureBackend()) return;
        var res = normalizeBackendResult(starmapBackendRef.update_starmap_node(starmapId, nodeId, JSON.stringify(patch)), qsTr("更新节点失败"));
        if (res.success) {
            clearError();
            for (var i = 0; i < nodesModel.length; i++) {
                if (nodesModel[i].id === nodeId) {
                    if (patch.title !== undefined) nodesModel[i].title = patch.title;
                    if (patch.kind !== undefined) nodesModel[i].kind = patch.kind;
                    nodesModelChanged();
                    graphChanged();
                    break;
                }
            }
        } else {
            setError(backendErrorText(res, qsTr("更新节点失败")));
        }
    }

    function deleteNode(nodeId) {
        if (!ensureBackend()) return;
        var res = normalizeBackendResult(starmapBackendRef.delete_starmap_node(starmapId, nodeId), qsTr("删除节点失败"));
        if (res.success) {
            clearError();
            loadGraph();
            clearSelection();
        } else {
            setError(backendErrorText(res, qsTr("删除节点失败")));
        }
    }

    function updateEdge(edgeId, patch) {
        if (!ensureBackend()) return;
        var res = normalizeBackendResult(starmapBackendRef.update_starmap_edge(starmapId, edgeId, JSON.stringify(patch)), qsTr("更新连线失败"));
        if (res.success) {
            clearError();
            for (var i = 0; i < edgesModel.length; i++) {
                if (edgesModel[i].id === edgeId) {
                    if (patch.label !== undefined) edgesModel[i].label = patch.label;
                    if (patch.kind !== undefined) edgesModel[i].kind = patch.kind;
                    edgesModelChanged();
                    graphChanged();
                    break;
                }
            }
        } else {
            setError(backendErrorText(res, qsTr("更新连线失败")));
        }
    }

    function deleteEdge(edgeId) {
        if (!ensureBackend()) return;
        var res = normalizeBackendResult(starmapBackendRef.delete_starmap_edge(starmapId, edgeId), qsTr("删除连线失败"));
        if (res.success) {
            clearError();
            loadGraph();
            clearSelection();
        } else {
            setError(backendErrorText(res, qsTr("删除连线失败")));
        }
    }

    // Issue #796 评论 5886483653: Embed 增删改（后端 create_starmap_embed /
    // update_starmap_embed / delete_starmap_embed 由 Canvas 在 createSubStarmapAt
    // 里直接调，这里提供 update/delete 供右键菜单复用）。
    function updateEmbed(instanceId, patch) {
        if (!ensureBackend()) return;
        var res = normalizeBackendResult(starmapBackendRef.update_starmap_embed(starmapId, instanceId, JSON.stringify(patch)), qsTr("更新子星图入口失败"));
        if (res.success) {
            clearError();
            for (var i = 0; i < embedsModel.length; i++) {
                if (embedsModel[i].instanceId === instanceId) {
                    if (patch.label !== undefined) embedsModel[i].label = patch.label;
                    if (patch.position !== undefined) {
                        embedsModel[i].x = patch.position.x;
                        embedsModel[i].y = patch.position.y;
                    }
                    embedsModelChanged();
                    graphChanged();
                    break;
                }
            }
        } else {
            setError(backendErrorText(res, qsTr("更新子星图入口失败")));
        }
    }

    function deleteEmbed(instanceId) {
        if (!ensureBackend()) return;
        var res = normalizeBackendResult(starmapBackendRef.delete_starmap_embed(starmapId, instanceId), qsTr("删除子星图入口失败"));
        if (res.success) {
            clearError();
            loadGraph();
            clearSelection();
        } else {
            setError(backendErrorText(res, qsTr("删除子星图入口失败")));
        }
    }

    function computeEdgeRenders() {
        if (!ensureBackend()) return;
        var nodePos = [];
        for (var j = 0; j < nodesModel.length; j++) {
            var n = nodesModel[j];
            nodePos.push({ id: n.id, x: n.x, y: n.y, width: n.width, height: n.height });
        }
        var res = normalizeBackendResult(starmapBackendRef.compute_edge_renders(starmapId, JSON.stringify(nodePos)), "");
        if (res.success && res.data) {
            edgeRenders = res.data;
        }
    }

    function hitTestEdge(wx, wy) {
        if (!ensureBackend()) return null;
        if (!edgeRenders || edgeRenders.length === 0) computeEdgeRenders();
        if (!edgeRenders || edgeRenders.length === 0) return null;
        var res = normalizeBackendResult(starmapBackendRef.hit_test_edge_renders(JSON.stringify(edgeRenders), wx, wy), "");
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

    // Issue #790 评论 5875963057: 添加超链接（Core 正式 hyperlink API）
    function addHyperlink(nodeId, url, label) {
        if (!ensureBackend()) return;
        var source = {
            starmapId: starmapId,
            segments: [],
            target: { type: "node", nodeId: nodeId }
        };
        var hl = {
            // hyperlinkId 由 Core 生成
            hyperlinkId: "",
            source: source,
            targetUri: url,
            label: label || null,
            createdAt: 0,
            updatedAt: 0
        };
        var res = normalizeBackendResult(starmapBackendRef.add_starmap_hyperlink(starmapId, JSON.stringify(hl)), qsTr("添加超链接失败"));
        if (res.success) {
            clearError();
        } else {
            setError(backendErrorText(res, qsTr("添加超链接失败")));
        }
    }

    function invalidateEdgeRenders() { edgeRenders = []; }
}
