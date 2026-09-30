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

QtObject {
    id: controller

    property string starmapId: ""
    property var starmapBackendRef: null
    property string errorMessage: ""
    property var graphData: null
    property var nodesModel: []
    property var edgesModel: []
    property var edgeRenders: []
    // Issue #796 评论 5886483653: 子星图 Embed 显示模型，从 graphData.embeds 派生。
    property var embedsModel: []

    // Issue #805 评论 5908703621 问题 3：Embed chrome 命中区域几何常量。
    // 与 StarMapEmbed.qml 的 _chromeHeight / _borderSlop 保持一致。
    // findEmbedChromeAt 只判断标题条 + 四条 border，内部矩形返回 null。
    readonly property int _chromeHeight: 24
    readonly property int _borderSlop: 6

    signal graphChanged()
    signal selectionCleared()
    signal nodeSelected(var node)
    signal edgeSelected(var edge)
    signal embedSelected(var embed)

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
            buildModels();
            computeEdgeRenders(null);
        } else {
            setError(qsTr("加载星图数据失败"));
        }
    }

    function buildModels() {
        // graph.nodes[].position 是节点位置唯一真相，直接从 graph 派生平台显示模型。
        // 宽高/圆角是纯显示参数，用默认值（与 starmap_bridge.rs DEFAULT_NODE_* 一致）。
        var newNodes = [];
        // Issue #801 评论 5895625744: newEmbeds 提前声明，旧 portal Node 也往这里归一。
        var newEmbeds = [];
        var graphNodes = graphData && graphData.nodes ? graphData.nodes : [];
        for (var i = 0; i < graphNodes.length; i++) {
            var gn = graphNodes[i];
            var pos = gn.position || { x: 0, y: 0 };
            // Issue #801 评论 5895625744: 旧 portal Node 在模型转换层归一到 Embed，
            // 不作为普通 Node 下发给 StarMapNode。保留旧节点的位置、标题和目标 starmap。
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
                    width: 150,
                    height: 60,
                    isSelected: false,
                    hostPath: null
                });
            } else {
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
                    tags: gn.tags
                });
            }
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
                width: 150,
                height: 60,
                isSelected: false,
                hostPath: gem.hostPath || null
            });
        }
        embedsModel = newEmbeds;

        graphChanged();
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

    // Issue #805 评论 5908703621 问题 3：findEmbedChromeAt 只判断 chrome 命中区域
    // （标题条矩形 + 四条 border 矩形），内部矩形返回 null。
    // 父 Canvas 不再把整个 Embed 矩形判成命中，内部事件不会被父场景截走。
    // 纯本地几何判断，不需要后端。
    function _rectContains(rx, ry, rw, rh, px, py) {
        return px >= rx && px <= rx + rw && py >= ry && py <= ry + rh
    }

    function findEmbedChromeAt(wx, wy) {
        for (var i = 0; i < embedsModel.length; i++) {
            var em = embedsModel[i]
            // 标题条矩形（顶部 _chromeHeight 高度）
            if (_rectContains(em.x, em.y, em.width, _chromeHeight, wx, wy)) return em
            // borderTop
            if (_rectContains(em.x, em.y, em.width, _borderSlop, wx, wy)) return em
            // borderBottom
            if (_rectContains(em.x, em.y + em.height - _borderSlop, em.width, _borderSlop, wx, wy)) return em
            // borderLeft（标题条下方到 borderBottom 上方）
            if (_rectContains(em.x, em.y + _chromeHeight, _borderSlop, em.height - _chromeHeight - _borderSlop, wx, wy)) return em
            // borderRight
            if (_rectContains(em.x + em.width - _borderSlop, em.y + _chromeHeight, _borderSlop, em.height - _chromeHeight - _borderSlop, wx, wy)) return em
        }
        return null
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

    // Issue #796 评论 5887280405: 用 fromPath/toPath 建边（path 版），
    // 支持 Node 和 Embed 作为端点。fromPath/toPath 是 StarMapTargetPathDto 的 JS 对象，
    // 由 StarMapCanvas.nodePath()/embedPath() 构造，这里 JSON.stringify 后传给后端。
    function createEdgeWithPaths(fromPath, toPath) {
        if (!ensureBackend()) return;
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
            loadGraph();
        } else {
            setError(backendErrorText(res, qsTr("创建连线失败")));
        }
    }

    function updateNode(nodeId, patch) {
        if (!ensureBackend()) return;
        var res = normalizeBackendResult(starmapBackendRef.update_starmap_node(starmapId, nodeId, JSON.stringify(patch)), qsTr("更新节点失败"));
        if (res.success) {
            clearError();
            var nextNodes = [];
            for (var i = 0; i < nodesModel.length; i++) {
                var n = copyObject(nodesModel[i]);
                if (n.id === nodeId) {
                    if (patch.title !== undefined) n.title = patch.title;
                    if (patch.kind !== undefined) n.kind = patch.kind;
                }
                nextNodes.push(n);
            }
            nodesModel = nextNodes;
            graphChanged();
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
            var nextEdges = [];
            for (var i = 0; i < edgesModel.length; i++) {
                var e = copyObject(edgesModel[i]);
                if (e.id === edgeId) {
                    if (patch.label !== undefined) e.label = patch.label;
                    if (patch.kind !== undefined) e.kind = patch.kind;
                }
                nextEdges.push(e);
            }
            edgesModel = nextEdges;
            graphChanged();
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
        // Issue #801 评论 5896594591: 旧 portal 真实身份是 Node，操作分流到 Node API。
        var embed = getEmbed(instanceId);
        if (embed && embed.legacyPortalNodeId) {
            var nodePatch = {};
            if (patch.label !== undefined) nodePatch.title = patch.label;
            if (patch.position !== undefined) nodePatch.position = patch.position;
            var nodeRes = normalizeBackendResult(starmapBackendRef.update_starmap_node(starmapId, embed.legacyPortalNodeId, JSON.stringify(nodePatch)), qsTr("更新节点失败"));
            if (nodeRes.success) {
                clearError();
                var nextEmbeds = [];
                for (var i = 0; i < embedsModel.length; i++) {
                    var em = copyObject(embedsModel[i]);
                    if (em.instanceId === instanceId) {
                        if (patch.label !== undefined) em.label = patch.label;
                        if (patch.position !== undefined) {
                            em.x = patch.position.x;
                            em.y = patch.position.y;
                        }
                    }
                    nextEmbeds.push(em);
                }
                embedsModel = nextEmbeds;
                graphChanged();
            } else {
                setError(backendErrorText(nodeRes, qsTr("更新节点失败")));
            }
            return;
        }
        // 正式 Embed 继续走 Embed API（原逻辑不变）
        var res = normalizeBackendResult(starmapBackendRef.update_starmap_embed(starmapId, instanceId, JSON.stringify(patch)), qsTr("更新子星图入口失败"));
        if (res.success) {
            clearError();
            var nextEmbeds = [];
            for (var i = 0; i < embedsModel.length; i++) {
                var em = copyObject(embedsModel[i]);
                if (em.instanceId === instanceId) {
                    if (patch.label !== undefined) em.label = patch.label;
                    if (patch.position !== undefined) {
                        em.x = patch.position.x;
                        em.y = patch.position.y;
                    }
                }
                nextEmbeds.push(em);
            }
            embedsModel = nextEmbeds;
            graphChanged();
        } else {
            setError(backendErrorText(res, qsTr("更新子星图入口失败")));
        }
    }

    function deleteEmbed(instanceId) {
        if (!ensureBackend()) return;
        // Issue #801 评论 5896594591: 旧 portal 走 Node API。
        var embed = getEmbed(instanceId);
        if (embed && embed.legacyPortalNodeId) {
            var nodeRes = normalizeBackendResult(starmapBackendRef.delete_starmap_node(starmapId, embed.legacyPortalNodeId), qsTr("删除节点失败"));
            if (nodeRes.success) {
                clearError();
                loadGraph();
                clearSelection();
            } else {
                setError(backendErrorText(nodeRes, qsTr("删除节点失败")));
            }
            return;
        }
        // 正式 Embed 继续走 Embed API（原逻辑不变）
        var res = normalizeBackendResult(starmapBackendRef.delete_starmap_embed(starmapId, instanceId), qsTr("删除子星图入口失败"));
        if (res.success) {
            clearError();
            loadGraph();
            clearSelection();
        } else {
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
        loadGraph();
        if (instanceId) {
            selectEmbed(instanceId);
        }
    }

    // Issue #798: 拖动结束后提交节点新位置。先持久化单节点 position，
    // 后端成功后再浅拷贝新数组一次赋值更新 canonical model。
    // 不再调用全量 saveLayout()，避免拖一个节点把所有节点逐个重写。
    // 失败时 canonical model 不动，结束 transient move 后 delegate 因 binding
    // 自动回旧位置（与 commitEmbedMove 顺序一致）。
    function commitNodeMove(nodeId, nx, ny) {
        if (!ensureBackend()) return false;
        var res = normalizeBackendResult(
            starmapBackendRef.update_starmap_node(starmapId, nodeId, JSON.stringify({ position: { x: nx, y: ny } })),
            qsTr("更新节点位置失败")
        );
        if (res.success) {
            clearError();
            var nextNodes = [];
            for (var i = 0; i < nodesModel.length; i++) {
                var n = copyObject(nodesModel[i]);
                if (n.id === nodeId) { n.x = nx; n.y = ny; }
                nextNodes.push(n);
            }
            nodesModel = nextNodes;
            computeEdgeRenders(null);
            graphChanged();
            return true;
        } else {
            setError(backendErrorText(res, qsTr("更新节点位置失败")));
            // Issue #798 评论 5892406254: 提交失败时 canonical model 未动，
            // 但 edgeRenders 已被 transient move 更新成临时坐标。
            // 恢复 edge cache 到 canonical，与 delegate 回旧位置保持一致。
            computeEdgeRenders(null);
            graphChanged();
            return false;
        }
    }

    // Issue #798: 拖动结束后提交 Embed 新位置。先持久化到后端，
    // 再浅拷贝新数组一次赋值更新本地模型。
    function commitEmbedMove(instanceId, nx, ny) {
        if (!ensureBackend()) return false;
        // Issue #801 评论 5896594591: 旧 portal 移动走 Node API。
        var embed = getEmbed(instanceId);
        if (embed && embed.legacyPortalNodeId) {
            var nodeRes = normalizeBackendResult(
                starmapBackendRef.update_starmap_node(starmapId, embed.legacyPortalNodeId, JSON.stringify({ position: { x: nx, y: ny } })),
                qsTr("更新节点位置失败")
            );
            if (nodeRes.success) {
                clearError();
                var nextEmbeds = [];
                for (var i = 0; i < embedsModel.length; i++) {
                    var em = copyObject(embedsModel[i]);
                    if (em.instanceId === instanceId) { em.x = nx; em.y = ny; }
                    nextEmbeds.push(em);
                }
                embedsModel = nextEmbeds;
                computeEdgeRenders(null);
                graphChanged();
                return true;
            } else {
                setError(backendErrorText(nodeRes, qsTr("更新节点位置失败")));
                computeEdgeRenders(null);
                graphChanged();
                return false;
            }
        }
        // 正式 Embed 继续走 Embed API（原逻辑不变）
        var res = normalizeBackendResult(
            starmapBackendRef.update_starmap_embed(starmapId, instanceId, JSON.stringify({ position: { x: nx, y: ny } })),
            qsTr("更新子星图入口失败")
        );
        if (res.success) {
            clearError();
            var nextEmbeds = [];
            for (var i = 0; i < embedsModel.length; i++) {
                var em = copyObject(embedsModel[i]);
                if (em.instanceId === instanceId) { em.x = nx; em.y = ny; }
                nextEmbeds.push(em);
            }
            embedsModel = nextEmbeds;
            computeEdgeRenders(null);
            graphChanged();
            return true;
        } else {
            setError(backendErrorText(res, qsTr("更新子星图入口失败")));
            // Issue #798 评论 5892406254: 提交失败时 canonical model 未动，
            // 但 edgeRenders 已被 transient move 更新成临时坐标。
            // 恢复 edge cache 到 canonical，与 delegate 回旧位置保持一致。
            computeEdgeRenders(null);
            graphChanged();
            return false;
        }
    }

    function computeEdgeRenders(moveOverride) {
        if (!ensureBackend()) return;
        var nodePos = [];
        for (var j = 0; j < nodesModel.length; j++) {
            var n = nodesModel[j];
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
        var embedPos = [];
        for (var k = 0; k < embedsModel.length; k++) {
            var em = embedsModel[k];
            var ex = em.x, ey = em.y;
            if (moveOverride && moveOverride.kind === "embed" && moveOverride.id === em.instanceId) {
                ex = moveOverride.x; ey = moveOverride.y;
            }
            embedPos.push({ instanceId: em.instanceId, x: ex, y: ey, width: em.width, height: em.height });
        }
        if (!graphData) return;
        var res = normalizeBackendResult(starmapBackendRef.compute_edge_renders(JSON.stringify(graphData), JSON.stringify(nodePos), JSON.stringify(embedPos)), "");
        if (res.success && res.data) {
            edgeRenders = res.data;
        }
    }

    function hitTestEdge(wx, wy) {
        if (!ensureBackend()) return null;
        if (!edgeRenders || edgeRenders.length === 0) computeEdgeRenders(null);
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

    function invalidateEdgeRenders() { edgeRenders = []; }
}
