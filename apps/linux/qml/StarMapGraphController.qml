// =============================================================================
// StarMapGraphController.qml — 星图图控制器
// =============================================================================
//
// 层级：Linux UI 层（QML 逻辑控制器）
// 职责：加载/保存星图图数据+布局、节点/边增删改、选区管理
// 约束：
//   - 纯状态管理，不包含 UI 渲染
//   - 通过 backendRef 调用 AppBackend (Rust QObject)
//   - 图数据通过 AppBackend 暴露的对象/数组 DTO 与 Core 层交互
//
// 数据流：backendRef (DTO) → controller (graphData/layoutData) → Canvas (nodesModel/edgesModel)
// =============================================================================

import QtQuick 2.15

QtObject {
    id: controller

    property string starmapId: ""
    property var backendRef: null
    property string errorMessage: ""
    property var graphData: null
    property var layoutData: null
    property var nodesModel: []
    property var edgesModel: []

    signal graphChanged()
    signal selectionCleared()
    signal nodeSelected(var node)
    signal edgeSelected(var edge)

    function setError(msg) {
        errorMessage = msg || "";
        if (errorMessage) console.warn("[StarMapGraphController]", errorMessage);
    }

    function clearError() { errorMessage = ""; }

    function normalizeBackendResult(raw, fallbackMessage) {
        if (raw && raw.success !== undefined) return raw;
        setError(fallbackMessage);
        return { success: false, message: fallbackMessage };
    }

    function ensureBackend() {
        if (!backendRef) {
            setError("星图后端未初始化");
            return false;
        }
        return true;
    }

    function loadGraph() {
        if (starmapId === "") return;
        if (!ensureBackend()) return;

        var res = normalizeBackendResult(backendRef.get_starmap_graph(starmapId), "加载星图数据失败");
        if (res.success) {
            clearError();
            graphData = res.data.graph;
            layoutData = res.data.layout;
            buildModels();
        } else {
            setError(res.userMessage || res.message || "加载星图数据失败");
        }
    }

    function buildModels() {
        var newNodes = [];
        var graphNodes = graphData && graphData.nodes ? graphData.nodes : [];
        for (var i = 0; i < graphNodes.length; i++) {
            var gn = graphNodes[i];
            var ln = getLayoutNode(gn.id);
            newNodes.push({
                id: gn.id,
                title: gn.title,
                kind: gn.kind,
                x: ln ? ln.x : 0,
                y: ln ? ln.y : 0,
                width: ln ? ln.width : 150,
                height: ln ? ln.height : 60,
                isSelected: false,
                payload: gn.payload,
                tags: gn.tags
            });
        }
        nodesModel = newNodes;

        var newEdges = [];
        var graphEdges = graphData && graphData.edges ? graphData.edges : [];
        for (var j = 0; j < graphEdges.length; j++) {
            var ge = graphEdges[j];
            newEdges.push({ id: ge.id, from: ge.from, to: ge.to, kind: ge.kind, label: ge.label, isSelected: false });
        }
        edgesModel = newEdges;

        if (nodesModel.length > 0 && (!layoutData || !layoutData.nodes || layoutData.nodes.length === 0)) autoLayout();
        graphChanged();
    }

    function autoLayout() {
        var curX = 100;
        var curY = 100;
        for (var i = 0; i < nodesModel.length; i++) {
            nodesModel[i].x = curX;
            nodesModel[i].y = curY;
            curX += 200;
            if (curX > 800) {
                curX = 100;
                curY += 100;
            }
        }
        nodesModelChanged();
        saveLayout();
    }

    function getLayoutNode(id) {
        if (!layoutData || !layoutData.nodes) return null;
        for (var i = 0; i < layoutData.nodes.length; i++) {
            if (layoutData.nodes[i].nodeId === id) return layoutData.nodes[i];
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
        for (var i = 0; i < nodesModel.length; i++) {
            var n = nodesModel[i];
            if (wx >= n.x && wx <= n.x + n.width && wy >= n.y && wy <= n.y + n.height) return n;
        }
        return null;
    }

    function clearSelection() {
        for (var i = 0; i < nodesModel.length; i++) nodesModel[i].isSelected = false;
        for (var j = 0; j < edgesModel.length; j++) edgesModel[j].isSelected = false;
        nodesModelChanged();
        edgesModelChanged();
        graphChanged();
        selectionCleared();
    }

    function selectNode(nodeId) {
        clearSelection();
        for (var i = 0; i < nodesModel.length; i++) {
            if (nodesModel[i].id === nodeId) {
                nodesModel[i].isSelected = true;
                nodesModelChanged();
                graphChanged();
                nodeSelected(nodesModel[i]);
                return nodesModel[i];
            }
        }
        return null;
    }

    function selectEdge(edgeId) {
        clearSelection();
        for (var i = 0; i < edgesModel.length; i++) {
            if (edgesModel[i].id === edgeId) {
                edgesModel[i].isSelected = true;
                edgesModelChanged();
                graphChanged();
                edgeSelected(edgesModel[i]);
                return edgesModel[i];
            }
        }
        return null;
    }

    function createNode(wx, wy) {
        if (!ensureBackend()) return;
        var res = normalizeBackendResult(backendRef.create_starmap_node(starmapId, "新节点", "Note", wx, wy), "创建节点失败");
        if (res.success) {
            clearError();
            loadGraph();
            selectNode(res.data.id);
        } else {
            setError(res.userMessage || res.message || "创建节点失败");
        }
    }

    function createEdge(fromId, toId) {
        if (!ensureBackend()) return;
        var res = normalizeBackendResult(backendRef.create_starmap_edge(starmapId, fromId, toId, "RelatedTo", ""), "创建连线失败");
        if (res.success) {
            clearError();
            loadGraph();
        } else {
            setError(res.userMessage || res.message || "创建连线失败");
        }
    }

    function saveLayout() {
        if (!ensureBackend()) return;
        var layoutNodes = [];
        for (var i = 0; i < nodesModel.length; i++) {
            var n = nodesModel[i];
            layoutNodes.push({ nodeId: n.id, x: n.x, y: n.y, width: n.width, height: n.height, radius: 30, collapsed: false, zIndex: 0 });
        }
        var res = normalizeBackendResult(backendRef.save_starmap_layout(starmapId, JSON.stringify({ kind: "Freeform", nodes: layoutNodes })), "保存布局失败");
        if (res.success) clearError();
        else setError(res.userMessage || res.message || "保存布局失败");
    }

    function updateNode(nodeId, patch) {
        if (!ensureBackend()) return;
        var res = normalizeBackendResult(backendRef.update_starmap_node(starmapId, nodeId, JSON.stringify(patch)), "更新节点失败");
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
            setError(res.userMessage || res.message || "更新节点失败");
        }
    }

    function deleteNode(nodeId) {
        if (!ensureBackend()) return;
        var res = normalizeBackendResult(backendRef.delete_starmap_node(starmapId, nodeId), "删除节点失败");
        if (res.success) {
            clearError();
            loadGraph();
            clearSelection();
        } else {
            setError(res.userMessage || res.message || "删除节点失败");
        }
    }

    function updateEdge(edgeId, patch) {
        if (!ensureBackend()) return;
        var res = normalizeBackendResult(backendRef.update_starmap_edge(starmapId, edgeId, JSON.stringify(patch)), "更新连线失败");
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
            setError(res.userMessage || res.message || "更新连线失败");
        }
    }

    function deleteEdge(edgeId) {
        if (!ensureBackend()) return;
        var res = normalizeBackendResult(backendRef.delete_starmap_edge(starmapId, edgeId), "删除连线失败");
        if (res.success) {
            clearError();
            loadGraph();
            clearSelection();
        } else {
            setError(res.userMessage || res.message || "删除连线失败");
        }
    }
}
