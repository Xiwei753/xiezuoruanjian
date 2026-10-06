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
                    width: _embedDiameter,
                    height: _embedDiameter,
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
            loadGraph();
            return true;
        } else {
            setError(backendErrorText(res, qsTr("创建连线失败")));
            return false;
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

    // 边端点解析用的本层 node layout（含旧 portal 归一条目）。
    // computeEdgeRenders 与候选边预览共用同一份几何，避免两处拼装漂移。
    function nodeLayoutEntries(moveOverride) {
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

    function computeEdgeRenders(moveOverride) {
        if (!ensureBackend()) return;
        if (!graphData) return;
        var res = normalizeBackendResult(starmapBackendRef.compute_edge_renders(
            JSON.stringify(graphData),
            JSON.stringify(nodeLayoutEntries(moveOverride)),
            JSON.stringify(embedLayoutEntries(moveOverride))), "");
        if (res.success && res.data) {
            edgeRenders = res.data;
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
        var res = normalizeBackendResult(starmapBackendRef.compute_prospective_edge_render(
            JSON.stringify(graphData),
            JSON.stringify(nodeLayoutEntries(null)),
            JSON.stringify(embedLayoutEntries(null)),
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
        if (!edgeRenders || edgeRenders.length === 0) return null;
        var res = normalizeBackendResult(starmapBackendRef.hit_test_edge_renders(JSON.stringify(edgeRenders), wx, wy, threshold), "");
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

    // ---------------------------------------------------------------------------
    // Issue #832 评论 6013799805 / #373：超链接菜单（add/update/delete/list）
    // 后端 API 已齐全（starmap_backend.rs 第 207-219/820-855 行），这里只做
    // normalizeBackendResult/ensureBackend 包装。sourcePath 由调用方传入完整
    // nodePath()/embedPath()，不退化成裸 nodeId。
    // ---------------------------------------------------------------------------
    function addHyperlink(sourcePath, targetUri, label) {
        if (!ensureBackend()) return null;
        var body = { source: sourcePath, target_uri: targetUri, label: label || null };
        var res = normalizeBackendResult(
            starmapBackendRef.add_starmap_hyperlink(starmapId, JSON.stringify(body)),
            qsTr("添加超链接失败")
        );
        if (res.success) {
            clearError();
            return res.data;
        }
        setError(backendErrorText(res, qsTr("添加超链接失败")));
        return null;
    }

    // patch 遵循 StarMapHyperlinkPatchInputDto：{label?, clear_label, target_uri?, source?}。
    // 清空 label 由调用方传 clear_label:true（不在此处隐式构造）。
    function updateHyperlink(hlId, patch) {
        if (!ensureBackend()) return false;
        var res = normalizeBackendResult(
            starmapBackendRef.update_starmap_hyperlink(starmapId, hlId, JSON.stringify(patch)),
            qsTr("更新超链接失败")
        );
        if (res.success) {
            clearError();
            return true;
        }
        setError(backendErrorText(res, qsTr("更新超链接失败")));
        return false;
    }

    function deleteHyperlink(hlId) {
        if (!ensureBackend()) return false;
        var res = normalizeBackendResult(
            starmapBackendRef.delete_starmap_hyperlink(starmapId, hlId),
            qsTr("删除超链接失败")
        );
        if (res.success) {
            clearError();
            return true;
        }
        setError(backendErrorText(res, qsTr("删除超链接失败")));
        return false;
    }

    // 返回 StarMapHyperlinkDto 数组（list_starmap_hyperlinks 返回
    // StarMapHyperlinkListWithDiagnosticsDto，取 items）。
    function listHyperlinks() {
        if (!ensureBackend()) return [];
        var res = normalizeBackendResult(
            starmapBackendRef.list_starmap_hyperlinks(starmapId),
            qsTr("列出超链接失败")
        );
        if (res.success && res.data && res.data.items) {
            clearError();
            return res.data.items;
        }
        return [];
    }
}
