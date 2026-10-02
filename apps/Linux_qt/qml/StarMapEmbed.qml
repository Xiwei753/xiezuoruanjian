// =============================================================================
// StarMapEmbed.qml — 子星图 Embed 卡片组件（事件分层 + 递归渲染）
// =============================================================================
//
// 层级：Linux_qt UI 层（QML UI 组件）
// 职责：单个子星图 Embed 的可视化渲染、选中态展示、上抛点击类交互信号
//
// Issue #805 评论 5907045450 第 3 部分：事件分层
//   改成两层: Embed → chrome(title hit area + 4条 border hit area) +
//   contentViewport(child StarMapScene)。
//   - 标题文字命中：选择/移动/右键/长按都作用于 Embed。
//   - 四条边框命中：同上；边框可以有少量 hit slop。
//   - contentViewport：父 Embed 不挂 TapHandler/DragHandler/MouseArea，
//     事件直接给 child Scene。
//   - 删除 doubleClicked(targetStarmapId) 信号和右下角"▸ 进入"语义。
//   - 不用 findEmbedChromeAt() 把整个矩形都判成 Embed 命中。
//
// Issue #805 评论 5907045450 第 2 部分：递归渲染
//   contentViewport 内部用 Loader 创建下一层 StarMapScene
//   （childPath = parent.pathSegments + EnterEmbed(embed.instanceId)）。
//   子 Scene 自己有 viewport 和手势状态。
//
// 约束：
//   - 纯 UI 组件，数据通过 property 传入
//   - 根对象是稳定 Item：x/y/width/height 与命中框恒定
//   - 使用 DesignTokens 统一样式
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Item {
    id: root

    required property var dt

    readonly property color _primary: dt.primary
    readonly property color _onPrimary: dt.onPrimary
    readonly property color _accent: dt.accent
    readonly property color _accentSoft: dt.accentSoft
    readonly property color _border: dt.border
    readonly property color _surfaceContainer: dt.surfaceContainer
    readonly property color _shadowLight: dt.shadowLight
    readonly property color _textPrimary: dt.textPrimary
    readonly property color _textMuted: dt.textMuted
    readonly property int _radiusXs: dt.radiusXs
    readonly property int _radiusSm: dt.radiusSm

    // Embed 身份与数据
    property string instanceId: ""
    property string targetStarmapId: ""
    property string label: ""
    property bool isSelected: false

    // 由 Canvas 控制：是否正处于拖动中（拖动时停止 idle wobble）
    property bool isBeingDragged: false

    // Issue #814 评论 5935285879: 共享选中控制器，由 Canvas 传入，传给 child Scene。
    property var selectionController: null

    // wobble 改纯视觉偏移，不影响命中框
    property int wobbleIndex: 0
    property real _wobbleAnimX: 0
    property real _wobbleAnimY: 0

    // Issue #805 评论 5907045450 第 2 部分：递归渲染上下文。
    // rootStarmapId / parentPathSegments / starmapBackendRef 由 Canvas 传入，
    // Embed 的 contentViewport 用这些构造子 Scene 的 pathSegments。
    property string rootStarmapId: ""
    property var parentPathSegments: []
    property var starmapBackendRef: null
    // Issue #805 评论 5908703621 问题 1：父路径 key，由 Canvas 传入，
    // 用于构造 child Scene 的 pathKey（父路径 + "/embed_<instanceId>"）。
    property string parentPathKey: ""

    // 递归 Scene 不再无条件一次性展开整棵引用树。
    // Canvas 只把“当前视口内的 Embed”置为 true；第一次进入视口后锁存为已激活，
    // 这样滚动离开后不会销毁 child Scene，也不会丢掉该 Scene 自己的 pan/zoom/手势状态。
    // 层级仍不设固定上限，下一层继续按它自己的视口决定何时实例化。
    property bool childSceneInViewport: false
    property bool childSceneActivated: false
    onChildSceneInViewportChanged: {
        if (childSceneInViewport)
            childSceneActivated = true
    }
    Component.onCompleted: {
        if (childSceneInViewport)
            childSceneActivated = true
        _syncChildScene()
    }

    // Issue #814 评论 5935346839: Embed 自己的事件分层边界日志入口。
    // starmapBackendRef 为 null 时静默跳过。不记录连续移动。
    function logEmbedInteraction(event, fields) {
        if (!starmapBackendRef) return
        var fj = fields ? JSON.stringify(fields) : ""
        starmapBackendRef.record_interaction(event, parentPathKey, targetStarmapId, "embed", instanceId, fj)
    }

    // Issue #814 评论 5935346839: embed_child_scene_activated 边界日志。
    onChildSceneActivatedChanged: {
        if (childSceneActivated) {
            logEmbedInteraction("embed_child_scene_activated", {
                "parentPathKey": parentPathKey,
                "childScenePathKey": childScenePathKey,
                "instanceId": instanceId,
                "targetStarmapId": targetStarmapId
            })
        }
    }

    // ---------------------------------------------------------------------------
    // 递归 child Scene 的输入（原来写在静态 Component 里，现在改成运行时构造参数）。
    // dt / starmapBackendRef / rootStarmapId 在一次运行内由 Canvas 恒定传入；
    // pathSegments / pathKey 只依赖 parentPathSegments（每个 Canvas 恒定）
    // 与 instanceId，所以 childScenePathKey 就是子 Scene 的身份标识。
    // ---------------------------------------------------------------------------
    readonly property bool childSceneWanted:
        childSceneActivated && targetStarmapId.length > 0 && rootStarmapId.length > 0
    readonly property var childScenePathSegments: parentPathSegments.concat([
        { type: "enterEmbed", instanceId: instanceId, nodeId: null }
    ])
    readonly property string childScenePathKey: parentPathKey + "/embed_" + instanceId

    // setSource 是命令式的，需要一个内部状态避免重复请求同一份 child Scene。
    property bool _childSceneRequested: false

    // Issue #805 评论 5914170620：Embed 不得静态引用 StarMapScene 类型。
    // StarMapScene → StarMapCanvas → StarMapEmbed 已是一条静态编译边，
    // Embed 再静态引用 StarMapScene 就构成编译期环 Scene → Canvas → Embed → Scene，
    // Qt type loader 会主线程与 QQmlThread 互等死锁，整个 QML 树
    // （含 main.qml 的根 ApplicationWindow）永远停在 Loading，进程只剩空事件循环。
    // 递归边因此必须落到运行时按 URL 解析。
    function _syncChildScene() {
        if (!childSceneWanted) {
            _childSceneRequested = false
            childSceneLoader.active = false
            return
        }
        if (_childSceneRequested)
            return
        _childSceneRequested = true
        childSceneLoader.active = true
        // setSource 带 initialProperties：一次性满足 StarMapScene 的 required
        // property dt，避免「先建空对象再补值」触发
        // Required property 'dt' was not initialized。
        childSceneLoader.setSource(Qt.resolvedUrl("StarMapScene.qml"), {
            "dt": dt,
            "starmapBackendRef": starmapBackendRef,
            "rootStarmapId": rootStarmapId,
            "pathSegments": childScenePathSegments,
            "pathKey": childScenePathKey,
            // Issue #814 评论 5935285879: 共享选中控制器逐层下传，子 Scene 沿用同一个。
            "selectionController": selectionController
        })
    }
    onChildSceneWantedChanged: _syncChildScene()
    // Repeater 复用 delegate 时 instanceId 可能变；身份变了子 Scene 必须重建。
    // 正在异步加载中也一样重发，避免用旧 pathKey 建出子 Scene。
    onChildScenePathKeyChanged: {
        if (!_childSceneRequested)
            return
        _childSceneRequested = false
        childSceneLoader.active = false
        _syncChildScene()
    }

    // Issue #805 评论 5907045450 第 3 部分：chrome 命中区域高度 + 边框 hit slop。
    readonly property int _chromeHeight: 24
    readonly property int _borderSlop: 6

    // Issue #814 评论 5935285879: Embed 独立显示尺寸常量，不再复用 node 尺寸 150×60。
    // 尺寸仍放 Linux_Qt 显示层，不进 Core。内容区（标题 24px、底边 6px 后）要有
    // 足够高度容纳真正能拖动、缩放、放节点的子画布。
    // _embedDefaultWidth/Height 是默认尺寸，实际 width/height 由 delegate 传入
    // （GraphController buildModels 用同样常量初始化）。
    readonly property int _embedDefaultWidth: 240
    readonly property int _embedDefaultHeight: 220

    property real visualOffsetX:
        (isSelected || isBeingDragged || chromeMouseTap.pressed || chromeTouchTap.pressed) ? 0 : _wobbleAnimX
    property real visualOffsetY:
        (isSelected || isBeingDragged || chromeMouseTap.pressed || chromeTouchTap.pressed) ? 0 : _wobbleAnimY

    // ---------------------------------------------------------------------------
    // 对外信号：Embed 只上抛事件，由 Canvas 决定后续行为
    // Issue #805 评论 5907045450 第 3 部分：删除 doubleClicked 信号。
    // ---------------------------------------------------------------------------
    signal clicked(string instanceId)
    signal rightClicked(string instanceId)
    signal moveDelta(real dx, real dy)
    signal mouseLongPressed(string instanceId)
    signal touchLongPressed(string instanceId)
    signal contextMenuRequested(string instanceId, real sceneX, real sceneY)
    signal leftReleased()
    signal mouseInteracted()
    // Issue #805 评论 5908703621 问题 5：child Scene 的节点编辑请求向上冒泡。
    // 已带 owner 上下文（ownerStarmapId/ownerPathKey），Canvas 接收后转发给 Scene。
    // Issue #805 评论 5912394108：ownerScene(var) 携带真正拥有该节点的子 Scene 引用，
    // 原样冒泡保持指向不变，Workspace 据此直接回写到对应子 Scene 的 Controller。
    signal editNodeRequested(var ownerScene, string ownerStarmapId, string ownerPathKey, var node)

    // ---------------------------------------------------------------------------
    // 内部视觉卡片：只有它承载 transform 偏移，根 Item 几何保持稳定
    // ---------------------------------------------------------------------------
    Rectangle {
        id: visualEmbed
        anchors.fill: parent

        radius: root._radiusSm
        color: root.isSelected ? root._surfaceContainer : root._accentSoft
        border.color: root.isSelected ? root._accent : root._border
        border.width: root.isSelected ? 2 : 1

        // 纯视觉偏移，不影响根 Item 的 x/y 命中测试
        transform: Translate {
            x: root.visualOffsetX
            y: root.visualOffsetY
        }

        // Shadow effect approximation
        Rectangle {
            anchors.fill: parent
            anchors.margins: -1
            z: -1
            color: "transparent"
            border.color: root._shadowLight
            radius: visualEmbed.radius + 1
            visible: !root.isSelected
        }

        // ── Issue #805 评论 5907045450 第 3 部分：chrome 区域 ──
        // 标题条（顶部 _chromeHeight 高度），挂 TapHandler/DragHandler。
        // 命中标题条：选择/移动/右键/长按都作用于 Embed。
        // Issue #805 评论 5908703621 问题 2：Handler 直接放进 titleBar 内部，
        // parent Item 就是 titleBar，命中范围限定在标题条。
        // 不靠 target: titleBar 做事件隔离。
        //
        // Issue #812: 本文件所有桌面语义 Handler（标题条与四条边框上的
        // TapHandler/DragHandler）的 acceptedDevices 一律 Mouse | TouchPad，
        // 不能只写 Mouse。acceptedDevices 是硬过滤，设备类型不匹配时 Handler
        // 根本不参与该事件；Wayland 的桌面 pointer 路径不能可靠还原成 Mouse，
        // 只写 Mouse 会让实体鼠标在这些区域完全点不动。
        // 触屏语义（TouchScreen 长按/滑动）保持 TouchScreen 独立，不混进来。
        // 新增 chrome 区域 Handler 时必须照此约定，否则会出现
        // "标题能点、边框还是死" 的半套状态。
        Rectangle {
            id: titleBar
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.top: parent.top
            height: root._chromeHeight
            color: "transparent"

            AppText {
                dt: root.dt
                anchors.fill: parent
                anchors.leftMargin: 8
                anchors.rightMargin: 8
                text: root.label
                color: root._textPrimary
                font.pointSize: root.dt.fontSmPt
                wrapMode: Text.NoWrap
                elide: Text.ElideRight
                horizontalAlignment: Text.AlignHCenter
                verticalAlignment: Text.AlignVCenter
            }

            TapHandler {
                id: chromeMouseTap
                acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
                acceptedButtons: Qt.LeftButton
                exclusiveSignals: TapHandler.SingleTap | TapHandler.DoubleTap
                onPressedChanged: {
                    if (pressed) {
                        root.mouseInteracted()
                        // Issue #814 评论 5935346839: embed_chrome_press 边界日志（title, mouse）。
                        root.logEmbedInteraction("embed_chrome_press", {
                            "parentPathKey": root.parentPathKey,
                            "childScenePathKey": root.childScenePathKey,
                            "instanceId": root.instanceId,
                            "targetStarmapId": root.targetStarmapId,
                            "chromeRegion": "title",
                            "device": "mouse"
                        })
                    }
                }
                onSingleTapped: root.clicked(root.instanceId)
                onLongPressed: root.mouseLongPressed(root.instanceId)
            }

            TapHandler {
                id: chromeTouchTap
                acceptedDevices: PointerDevice.TouchScreen
                acceptedButtons: Qt.LeftButton
                exclusiveSignals: TapHandler.SingleTap | TapHandler.DoubleTap
                onPressedChanged: {
                    if (pressed) {
                        // Issue #814 评论 5935346839: embed_chrome_press 边界日志（title, touch）。
                        root.logEmbedInteraction("embed_chrome_press", {
                            "parentPathKey": root.parentPathKey,
                            "childScenePathKey": root.childScenePathKey,
                            "instanceId": root.instanceId,
                            "targetStarmapId": root.targetStarmapId,
                            "chromeRegion": "title",
                            "device": "touch"
                        })
                    }
                }
                onSingleTapped: root.clicked(root.instanceId)
                onLongPressed: root.touchLongPressed(root.instanceId)
            }

            TapHandler {
                acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
                acceptedButtons: Qt.RightButton
                onPressedChanged: {
                    if (pressed) {
                        root.mouseInteracted()
                        // Issue #814 评论 5935346839: embed_chrome_press 边界日志（title, right）。
                        root.logEmbedInteraction("embed_chrome_press", {
                            "parentPathKey": root.parentPathKey,
                            "childScenePathKey": root.childScenePathKey,
                            "instanceId": root.instanceId,
                            "targetStarmapId": root.targetStarmapId,
                            "chromeRegion": "title",
                            "device": "mouse",
                            "button": "right"
                        })
                    }
                }
                onSingleTapped: function(eventPoint) {
                    root.rightClicked(root.instanceId)
                    root.contextMenuRequested(root.instanceId, eventPoint.scenePosition.x, eventPoint.scenePosition.y)
                }
            }

            DragHandler {
                id: titleDragHandler
                acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
                target: null
                acceptedButtons: Qt.LeftButton
                grabPermissions: PointerHandler.CanTakeOverFromHandlersOfDifferentType
                property real lastTx: 0
                property real lastTy: 0
                onActiveChanged: {
                    if (active) { lastTx = 0; lastTy = 0 }
                }
                onActiveTranslationChanged: {
                    var dx = activeTranslation.x - lastTx
                    var dy = activeTranslation.y - lastTy
                    lastTx = activeTranslation.x
                    lastTy = activeTranslation.y
                    // Issue #814 评论 5947740838: 只上抛 raw scene delta，
                    // scene→world 映射统一由 Canvas 的 sceneDeltaToWorld 完成，
                    // 不再在本层做 zoom 换算（会漏掉祖先 Embed scale）。
                    root.moveDelta(dx, dy)
                }
            }

            PointHandler {
                id: titlePointTracker
                acceptedButtons: Qt.LeftButton
                onActiveChanged: {
                    if (!active) root.leftReleased()
                }
            }
        }

        // ── Issue #805 评论 5907045450 第 3 部分：四条边框命中区域 ──
        // 边框有少量 hit slop，命中时选择/移动作用于 Embed。
        // Issue #805 评论 5908703621 问题 2：每条 border 内部放一套与 titleBar
        // 对称的 handler（单击选择 + 长按拉线 + 右键菜单 + 拖动移动 + press→release）。
        // Handler 声明在 border 内部，命中范围就是那条 border。
        Rectangle {
            id: borderTop
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.top: parent.top
            height: root._borderSlop
            color: "transparent"

            TapHandler {
                acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
                acceptedButtons: Qt.LeftButton
                onPressedChanged: {
                    if (pressed) {
                        root.mouseInteracted()
                        root.logEmbedInteraction("embed_chrome_press", {
                            "parentPathKey": root.parentPathKey,
                            "childScenePathKey": root.childScenePathKey,
                            "instanceId": root.instanceId,
                            "targetStarmapId": root.targetStarmapId,
                            "chromeRegion": "borderTop",
                            "device": "mouse"
                        })
                    }
                }
                onSingleTapped: root.clicked(root.instanceId)
                onLongPressed: root.mouseLongPressed(root.instanceId)
            }
            TapHandler {
                acceptedDevices: PointerDevice.TouchScreen
                acceptedButtons: Qt.LeftButton
                onPressedChanged: {
                    if (pressed) {
                        root.logEmbedInteraction("embed_chrome_press", {
                            "parentPathKey": root.parentPathKey,
                            "childScenePathKey": root.childScenePathKey,
                            "instanceId": root.instanceId,
                            "targetStarmapId": root.targetStarmapId,
                            "chromeRegion": "borderTop",
                            "device": "touch"
                        })
                    }
                }
                onSingleTapped: root.clicked(root.instanceId)
                onLongPressed: root.touchLongPressed(root.instanceId)
            }
            TapHandler {
                acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
                acceptedButtons: Qt.RightButton
                onPressedChanged: {
                    if (pressed) {
                        root.logEmbedInteraction("embed_chrome_press", {
                            "parentPathKey": root.parentPathKey,
                            "childScenePathKey": root.childScenePathKey,
                            "instanceId": root.instanceId,
                            "targetStarmapId": root.targetStarmapId,
                            "chromeRegion": "borderTop",
                            "device": "mouse",
                            "button": "right"
                        })
                    }
                }
                onSingleTapped: function(eventPoint) {
                    root.rightClicked(root.instanceId)
                    root.contextMenuRequested(root.instanceId, eventPoint.scenePosition.x, eventPoint.scenePosition.y)
                }
            }
            DragHandler {
                acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
                target: null
                acceptedButtons: Qt.LeftButton
                grabPermissions: PointerHandler.CanTakeOverFromHandlersOfDifferentType
                property real lastTx: 0
                property real lastTy: 0
                onActiveChanged: { if (active) { lastTx = 0; lastTy = 0 } }
                onActiveTranslationChanged: {
                    var dx = activeTranslation.x - lastTx
                    var dy = activeTranslation.y - lastTy
                    lastTx = activeTranslation.x
                    lastTy = activeTranslation.y
                    // Issue #814 评论 5947740838: 只上抛 raw scene delta，
                    // scene→world 映射统一由 Canvas 的 sceneDeltaToWorld 完成，
                    // 不再在本层做 zoom 换算（会漏掉祖先 Embed scale）。
                    root.moveDelta(dx, dy)
                }
            }
            PointHandler {
                acceptedButtons: Qt.LeftButton
                onActiveChanged: { if (!active) root.leftReleased() }
            }
        }
        Rectangle {
            id: borderBottom
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.bottom: parent.bottom
            height: root._borderSlop
            color: "transparent"

            TapHandler {
                acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
                acceptedButtons: Qt.LeftButton
                onPressedChanged: {
                    if (pressed) {
                        root.mouseInteracted()
                        root.logEmbedInteraction("embed_chrome_press", {
                            "parentPathKey": root.parentPathKey,
                            "childScenePathKey": root.childScenePathKey,
                            "instanceId": root.instanceId,
                            "targetStarmapId": root.targetStarmapId,
                            "chromeRegion": "borderBottom",
                            "device": "mouse"
                        })
                    }
                }
                onSingleTapped: root.clicked(root.instanceId)
                onLongPressed: root.mouseLongPressed(root.instanceId)
            }
            TapHandler {
                acceptedDevices: PointerDevice.TouchScreen
                acceptedButtons: Qt.LeftButton
                onPressedChanged: {
                    if (pressed) {
                        root.logEmbedInteraction("embed_chrome_press", {
                            "parentPathKey": root.parentPathKey,
                            "childScenePathKey": root.childScenePathKey,
                            "instanceId": root.instanceId,
                            "targetStarmapId": root.targetStarmapId,
                            "chromeRegion": "borderBottom",
                            "device": "touch"
                        })
                    }
                }
                onSingleTapped: root.clicked(root.instanceId)
                onLongPressed: root.touchLongPressed(root.instanceId)
            }
            TapHandler {
                acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
                acceptedButtons: Qt.RightButton
                onPressedChanged: {
                    if (pressed) {
                        root.logEmbedInteraction("embed_chrome_press", {
                            "parentPathKey": root.parentPathKey,
                            "childScenePathKey": root.childScenePathKey,
                            "instanceId": root.instanceId,
                            "targetStarmapId": root.targetStarmapId,
                            "chromeRegion": "borderBottom",
                            "device": "mouse",
                            "button": "right"
                        })
                    }
                }
                onSingleTapped: function(eventPoint) {
                    root.rightClicked(root.instanceId)
                    root.contextMenuRequested(root.instanceId, eventPoint.scenePosition.x, eventPoint.scenePosition.y)
                }
            }
            DragHandler {
                acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
                target: null
                acceptedButtons: Qt.LeftButton
                grabPermissions: PointerHandler.CanTakeOverFromHandlersOfDifferentType
                property real lastTx: 0
                property real lastTy: 0
                onActiveChanged: { if (active) { lastTx = 0; lastTy = 0 } }
                onActiveTranslationChanged: {
                    var dx = activeTranslation.x - lastTx
                    var dy = activeTranslation.y - lastTy
                    lastTx = activeTranslation.x
                    lastTy = activeTranslation.y
                    // Issue #814 评论 5947740838: 只上抛 raw scene delta，
                    // scene→world 映射统一由 Canvas 的 sceneDeltaToWorld 完成，
                    // 不再在本层做 zoom 换算（会漏掉祖先 Embed scale）。
                    root.moveDelta(dx, dy)
                }
            }
            PointHandler {
                acceptedButtons: Qt.LeftButton
                onActiveChanged: { if (!active) root.leftReleased() }
            }
        }
        Rectangle {
            id: borderLeft
            anchors.left: parent.left
            anchors.top: titleBar.bottom
            anchors.bottom: parent.bottom
            width: root._borderSlop
            color: "transparent"

            TapHandler {
                acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
                acceptedButtons: Qt.LeftButton
                onPressedChanged: {
                    if (pressed) {
                        root.mouseInteracted()
                        root.logEmbedInteraction("embed_chrome_press", {
                            "parentPathKey": root.parentPathKey,
                            "childScenePathKey": root.childScenePathKey,
                            "instanceId": root.instanceId,
                            "targetStarmapId": root.targetStarmapId,
                            "chromeRegion": "borderLeft",
                            "device": "mouse"
                        })
                    }
                }
                onSingleTapped: root.clicked(root.instanceId)
                onLongPressed: root.mouseLongPressed(root.instanceId)
            }
            TapHandler {
                acceptedDevices: PointerDevice.TouchScreen
                acceptedButtons: Qt.LeftButton
                onPressedChanged: {
                    if (pressed) {
                        root.logEmbedInteraction("embed_chrome_press", {
                            "parentPathKey": root.parentPathKey,
                            "childScenePathKey": root.childScenePathKey,
                            "instanceId": root.instanceId,
                            "targetStarmapId": root.targetStarmapId,
                            "chromeRegion": "borderLeft",
                            "device": "touch"
                        })
                    }
                }
                onSingleTapped: root.clicked(root.instanceId)
                onLongPressed: root.touchLongPressed(root.instanceId)
            }
            TapHandler {
                acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
                acceptedButtons: Qt.RightButton
                onPressedChanged: {
                    if (pressed) {
                        root.logEmbedInteraction("embed_chrome_press", {
                            "parentPathKey": root.parentPathKey,
                            "childScenePathKey": root.childScenePathKey,
                            "instanceId": root.instanceId,
                            "targetStarmapId": root.targetStarmapId,
                            "chromeRegion": "borderLeft",
                            "device": "mouse",
                            "button": "right"
                        })
                    }
                }
                onSingleTapped: function(eventPoint) {
                    root.rightClicked(root.instanceId)
                    root.contextMenuRequested(root.instanceId, eventPoint.scenePosition.x, eventPoint.scenePosition.y)
                }
            }
            DragHandler {
                acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
                target: null
                acceptedButtons: Qt.LeftButton
                grabPermissions: PointerHandler.CanTakeOverFromHandlersOfDifferentType
                property real lastTx: 0
                property real lastTy: 0
                onActiveChanged: { if (active) { lastTx = 0; lastTy = 0 } }
                onActiveTranslationChanged: {
                    var dx = activeTranslation.x - lastTx
                    var dy = activeTranslation.y - lastTy
                    lastTx = activeTranslation.x
                    lastTy = activeTranslation.y
                    // Issue #814 评论 5947740838: 只上抛 raw scene delta，
                    // scene→world 映射统一由 Canvas 的 sceneDeltaToWorld 完成，
                    // 不再在本层做 zoom 换算（会漏掉祖先 Embed scale）。
                    root.moveDelta(dx, dy)
                }
            }
            PointHandler {
                acceptedButtons: Qt.LeftButton
                onActiveChanged: { if (!active) root.leftReleased() }
            }
        }
        Rectangle {
            id: borderRight
            anchors.right: parent.right
            anchors.top: titleBar.bottom
            anchors.bottom: parent.bottom
            width: root._borderSlop
            color: "transparent"

            TapHandler {
                acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
                acceptedButtons: Qt.LeftButton
                onPressedChanged: {
                    if (pressed) {
                        root.mouseInteracted()
                        root.logEmbedInteraction("embed_chrome_press", {
                            "parentPathKey": root.parentPathKey,
                            "childScenePathKey": root.childScenePathKey,
                            "instanceId": root.instanceId,
                            "targetStarmapId": root.targetStarmapId,
                            "chromeRegion": "borderRight",
                            "device": "mouse"
                        })
                    }
                }
                onSingleTapped: root.clicked(root.instanceId)
                onLongPressed: root.mouseLongPressed(root.instanceId)
            }
            TapHandler {
                acceptedDevices: PointerDevice.TouchScreen
                acceptedButtons: Qt.LeftButton
                onPressedChanged: {
                    if (pressed) {
                        root.logEmbedInteraction("embed_chrome_press", {
                            "parentPathKey": root.parentPathKey,
                            "childScenePathKey": root.childScenePathKey,
                            "instanceId": root.instanceId,
                            "targetStarmapId": root.targetStarmapId,
                            "chromeRegion": "borderRight",
                            "device": "touch"
                        })
                    }
                }
                onSingleTapped: root.clicked(root.instanceId)
                onLongPressed: root.touchLongPressed(root.instanceId)
            }
            TapHandler {
                acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
                acceptedButtons: Qt.RightButton
                onPressedChanged: {
                    if (pressed) {
                        root.logEmbedInteraction("embed_chrome_press", {
                            "parentPathKey": root.parentPathKey,
                            "childScenePathKey": root.childScenePathKey,
                            "instanceId": root.instanceId,
                            "targetStarmapId": root.targetStarmapId,
                            "chromeRegion": "borderRight",
                            "device": "mouse",
                            "button": "right"
                        })
                    }
                }
                onSingleTapped: function(eventPoint) {
                    root.rightClicked(root.instanceId)
                    root.contextMenuRequested(root.instanceId, eventPoint.scenePosition.x, eventPoint.scenePosition.y)
                }
            }
            DragHandler {
                acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
                target: null
                acceptedButtons: Qt.LeftButton
                grabPermissions: PointerHandler.CanTakeOverFromHandlersOfDifferentType
                property real lastTx: 0
                property real lastTy: 0
                onActiveChanged: { if (active) { lastTx = 0; lastTy = 0 } }
                onActiveTranslationChanged: {
                    var dx = activeTranslation.x - lastTx
                    var dy = activeTranslation.y - lastTy
                    lastTx = activeTranslation.x
                    lastTy = activeTranslation.y
                    // Issue #814 评论 5947740838: 只上抛 raw scene delta，
                    // scene→world 映射统一由 Canvas 的 sceneDeltaToWorld 完成，
                    // 不再在本层做 zoom 换算（会漏掉祖先 Embed scale）。
                    root.moveDelta(dx, dy)
                }
            }
            PointHandler {
                acceptedButtons: Qt.LeftButton
                onActiveChanged: { if (!active) root.leftReleased() }
            }
        }

        // ── Issue #805 评论 5907045450 第 3 部分：contentViewport ──
        // 中间区域，父 Embed 不挂 TapHandler/DragHandler/MouseArea，
        // 事件直接给 child Scene。
        //
        // Issue #814 评论 5935346839：contentViewport 上唯一允许的 handler 是
        // passive grab 的 PointHandler，它只观察 press 事件并记录
        // embed_child_content_routed 边界日志，不拦截事件传递给 child Scene。
        // 这不违反 Issue #805 "事件直接给 child Scene" 的设计约束——
        // passive grab 不会取得 exclusive grab，事件流不受影响。
        Item {
            id: contentViewport
            anchors.left: borderLeft.right
            anchors.right: borderRight.left
            anchors.top: titleBar.bottom
            anchors.bottom: borderBottom.top
            clip: true

            // Issue #814 评论 5935346839: embed_child_content_routed 边界日志。
            // PointHandler 用 passive grab 观察 press，不抢事件，不影响 child Scene。
            // 下一次点子星图内部，就能看出事件到底给了父 Embed chrome，
            // 还是确实进入 child Scene。
            PointHandler {
                id: contentViewportPressObserver
                acceptedButtons: Qt.LeftButton | Qt.RightButton
                onActiveChanged: {
                    if (active) {
                        root.logEmbedInteraction("embed_child_content_routed", {
                            "parentPathKey": root.parentPathKey,
                            "childScenePathKey": root.childScenePathKey,
                            "instanceId": root.instanceId,
                            "targetStarmapId": root.targetStarmapId
                        })
                    }
                }
            }

            // Issue #805 评论 5907045450 第 2 部分：递归渲染子 StarMapScene。
            // childPath = parent.pathSegments + EnterEmbed(embed.instanceId)。
            // 子 Scene 自己有 viewport 和手势状态。
            Loader {
                id: childSceneLoader
                anchors.fill: parent
                // 只在该 Embed 真正进入父 Scene 视口后才创建递归 child Scene。
                // asynchronous 避免一帧里同步构造多层 QML 对象树把 GUI 线程堵死。
                // active / source 都由 root._syncChildScene() 命令式驱动：
                // source 必须是运行时 URL，静态 sourceComponent 会把
                // StarMapScene 变成 Embed 的编译期类型依赖，从而形成环。
                active: false
                asynchronous: true
            }

            // Issue #805 评论 5908703621 问题 5：child Scene 的 editNodeRequested
            // 继续向父 Scene / Workspace 冒泡。已带正确的 owner 上下文，原样转发。
            // Issue #805 评论 5912394108：ownerScene 一并原样转发，保持指向真正
            // 拥有该节点的子 Scene，不被 Embed / 父 Scene 替换。
            Connections {
                target: childSceneLoader.item
                function onEditNodeRequested(ownerScene, ownerStarmapId, ownerPathKey, node) {
                    root.editNodeRequested(ownerScene, ownerStarmapId, ownerPathKey, node)
                }
            }
        }
    }

    // wobble 降速，和 StarMapNode.qml 一致；选中/按下/拖动时动画暂停
    SequentialAnimation on _wobbleAnimX {
        loops: Animation.Infinite
        running: !isSelected && !isBeingDragged && !chromeMouseTap.pressed && !chromeTouchTap.pressed
        NumberAnimation { to: 0.6; duration: 7000 + (wobbleIndex % 7) * 400; easing.type: Easing.InOutSine }
        NumberAnimation { to: -0.6; duration: 7000 + (wobbleIndex % 7) * 400; easing.type: Easing.InOutSine }
    }
    SequentialAnimation on _wobbleAnimY {
        loops: Animation.Infinite
        running: !isSelected && !isBeingDragged && !chromeMouseTap.pressed && !chromeTouchTap.pressed
        NumberAnimation { to: 0.4; duration: 8500 + (wobbleIndex % 5) * 300; easing.type: Easing.InOutSine }
        NumberAnimation { to: -0.4; duration: 8500 + (wobbleIndex % 5) * 300; easing.type: Easing.InOutSine }
    }

    // ---------------------------------------------------------------------------
    // Issue #805 评论 5908703621 问题 2：所有 PointerHandler 已移进 titleBar /
    // border 内部（parent Item 决定命中范围）。根 Item 和 contentViewport 祖先链
    // 上不再有任何 TapHandler / DragHandler / MouseArea / exclusive-grab PointHandler，
    // 内部事件直接给 child Scene，不会被父 Embed 截走。
    // Issue #814 评论 5935346839：contentViewport 上新增 passive-grab PointHandler
    // （contentViewportPressObserver），只观察 press 记录 embed_child_content_routed
    // 边界日志，不取得 exclusive grab，不影响事件传递给 child Scene。
    // ---------------------------------------------------------------------------
}
