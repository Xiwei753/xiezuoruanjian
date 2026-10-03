// =============================================================================
// StarMapEmbed.qml — 子星图 Embed 卡片组件（事件分层 + 递归内容渲染）
// =============================================================================
//
// 层级：Linux_qt UI 层（QML UI 组件）
// 职责：单个子星图 Embed 的可视化渲染、选中态展示、上抛点击类交互信号
//
// 事件分层
//   Embed → chrome(顶部标题带 + 圆周边框 hit slop) + contentViewport(子星图内容)
//   - 标题文字命中：选择/移动/右键/长按都作用于 Embed。
//   - 圆周边框命中：同上；边框可以有少量 hit slop。
//   - contentViewport（圆的内接正方形）：父 Embed 不挂 TapHandler/DragHandler/
//     MouseArea，事件直接给子星图内容。
//   - findEmbedChromeAt() 先做圆内判定：圆外即使还在外接矩形里也不算 Embed 命中。
//
// Issue #822：子星图是正圆，内容是"内容"，不是"子视口"
//   Embed 外壳 world 几何恒定：width === height（直径 200），radius = width / 2，
//   任何档位都是同一颗正圆，不再有 240×220 的矩形卡片。
//   contentViewport 内部懒加载 StarMapSceneContent（不是 StarMapScene）。
//   子内容没有自己的 pan/zoom，也没有子视口手势状态：
//   整棵星图只有一个全局 viewport/camera，只在根 StarMapCanvas。
//   子星图显示多少细节是 Deep Zoom 档位：所属 Scene 的 ownerEffectiveScale
//   （全局相机 × 祖先 local fit）+ 根视口短边算出覆盖率，档位只决定
//   contentViewport 里渲染完整交互子内容、轻量 preview 还是只留外壳；
//   档位绝不写回全局相机，也不改 Embed 的 world 几何 / authored position。
//   contentViewport 取圆的内接正方形并保持 clip:true，子星图节点不会溢出圆外。
//
// 约束：
//   - 纯 UI 组件，数据通过 property 传入
//   - 根对象是稳定 Item：x/y/width/height 与命中框恒定
//   - 使用 DesignTokens 统一样式
//   - Issue #805 评论 5914170620：不得静态引用递归子组件类型，
//     必须运行时 Qt.resolvedUrl + Loader.setSource
// =============================================================================

import QtQuick

Item {
    id: root

    required property var dt

    readonly property color _accent: dt.accent
    readonly property color _accentSoft: dt.accentSoft
    readonly property color _border: dt.border
    readonly property color _surfaceContainer: dt.surfaceContainer
    readonly property color _shadowLight: dt.shadowLight
    readonly property color _textPrimary: dt.textPrimary

    // Embed 身份与数据
    property string instanceId: ""
    property string targetStarmapId: ""
    property string label: ""
    property bool isSelected: false

    // 由归属层控制：是否正处于拖动中（拖动时停止 idle wobble）
    property bool isBeingDragged: false

    // Issue #814 评论 5935285879: 共享选中控制器，由归属层传入，传给子内容。
    property var selectionController: null

    // Issue #822: 全局共享的手势状态机，由根 Canvas 唯一创建，逐层原样传下来。
    // 子星图内容不创建自己的状态机，也不持有自己的相机。
    property var interactionController: null

    // wobble 改纯视觉偏移，不影响命中框
    property int wobbleIndex: 0
    property real _wobbleAnimX: 0
    property real _wobbleAnimY: 0

    // Issue #822：递归渲染上下文，由归属层 StarMapSceneContent 传入。
    property string rootStarmapId: ""
    property var parentPathSegments: []
    property var starmapBackendRef: null
    // 父层 pathKey，用于构造子层完整 pathKey 和子内容的事件日志归属。
    property string parentPathKey: ""
    property int contentDepth: 0
    // 根内容引用，子层用它做跨层递归命中测试（连线松手落点可能在别的层）。
    property var rootContent: null
    // 菜单宿主（根 Canvas），逐层原样传下去，子层直接回调不必冒泡信号。
    property var menuHost: null
    // 本 Embed 所属的 SceneContent。子内容的累计比例沿这条链现读，
    // 不复制标量：相机 / 祖先 local fit 变化后不会拿到过期值。
    property var ownerSceneContent: null
    // 本 Embed 所属 Scene 的累计有效比例（globalZoom × 祖先 local fit）。
    // 只用来算投影尺寸与覆盖率，绝不写回全局相机。
    property real ownerEffectiveScale: 1.0
    // 根视口短边（屏幕像素）：coverage 的分母。
    property real viewportShortSide: 0
    // 当前可见区域（scene 坐标矩形）：沿 ownerSceneContent 链读同一份，不复制。
    readonly property var viewportRect: ownerSceneContent
            ? ownerSceneContent.viewportRect
            : ({ x: 0, y: 0, width: 0, height: 0 })

    // Issue #822：子星图内容不再无条件一次性展开整棵引用树。
    // 只有投影矩形接近当前可见区域才创建；第一次进入后锁存，离开视口不销毁。
    // 懒加载裁剪只决定"什么时候开始创建"，不是命中/创建/连线的坐标真相。
    property bool childContentInViewport: false
    property bool childContentActivated: false
    onChildContentInViewportChanged: {
        if (childContentInViewport)
            childContentActivated = true
    }

    function recomputeChildContentInViewport() {
        var margin = 64
        if (!rootContent || !ownerSceneContent) {
            childContentInViewport = true
            return
        }
        // 用 Qt 真实 Item 映射把本 Embed 投到 scene 坐标，再和根视口矩形求交。
        var tl = mapToItem(rootContent, 0, 0)
        var br = mapToItem(rootContent, width, height)
        var v = viewportRect
        childContentInViewport = br.x >= v.x - margin && br.y >= v.y - margin
                && tl.x <= v.x + v.width + margin
                && tl.y <= v.y + v.height + margin
    }
    onXChanged: recomputeChildContentInViewport()
    onYChanged: recomputeChildContentInViewport()
    onViewportRectChanged: recomputeChildContentInViewport()
    onWidthChanged: {
        resolveChildContentDetail()
        recomputeChildContentInViewport()
    }
    onHeightChanged: {
        resolveChildContentDetail()
        recomputeChildContentInViewport()
    }

    Connections {
        // 祖先链 local fit 变化会平移本 Embed 在 scene 坐标里的投影。
        target: root.ownerSceneContent
        function onTransformChanged() { root.recomputeChildContentInViewport() }
    }

    Component.onCompleted: {
        resolveChildContentDetail()
        recomputeChildContentInViewport()
        if (childContentInViewport)
            childContentActivated = true
        _syncChildContent()
    }

    // Issue #814 评论 5935346839: Embed 自己的事件分层边界日志入口。
    // starmapBackendRef 为 null 时静默跳过。不记录连续移动。
    function logEmbedInteraction(event, fields) {
        if (!starmapBackendRef) return
        var fj = fields ? JSON.stringify(fields) : ""
        starmapBackendRef.record_interaction(event, parentPathKey, targetStarmapId, "embed", instanceId, fj)
    }

    // Issue #814 评论 5935346839: embed_child_content_activated 边界日志。
    onChildContentActivatedChanged: {
        if (childContentActivated) {
            logEmbedInteraction("embed_child_content_activated", {
                "parentPathKey": parentPathKey,
                "childContentPathKey": childContentPathKey,
                "instanceId": instanceId,
                "targetStarmapId": targetStarmapId
            })
        }
    }

    // ---------------------------------------------------------------------------
    // 递归子内容的输入（运行时构造参数）。
    // pathSegments / scenePathKey 在创建时一次性给全，子层不会先冒充根层再修正。
    // ---------------------------------------------------------------------------
    readonly property bool childContentWanted:
        childContentActivated && childContentDetail !== "shell"
        && targetStarmapId.length > 0 && rootStarmapId.length > 0
    readonly property var childContentPathSegments: parentPathSegments.concat([
        { type: "enterEmbed", instanceId: instanceId, nodeId: null }
    ])
    readonly property string childContentPathKey: parentPathKey + "/embed_" + instanceId

    // setSource 是命令式的，需要一个内部状态避免重复请求同一份子内容。
    property bool _childContentRequested: false

    // Issue #805 评论 5914170620：Embed 不得静态引用递归子组件类型。
    // StarMapSceneContent → StarMapNode/StarMapEmbed 已是一条静态编译边，
    // Embed 再静态引用就构成编译期环 Content → Embed → Content，
    // Qt type loader 会主线程与 QQmlThread 互等死锁，整个 QML 树永远停在 Loading。
    // 递归边因此必须落到运行时按 URL 解析。
    function _syncChildContent() {
        if (!childContentWanted) {
            _childContentRequested = false
            childContentLoader.active = false
            return
        }
        if (_childContentRequested)
            return
        _childContentRequested = true
        childContentLoader.active = true
        // setSource 带 initialProperties：一次性满足 StarMapSceneContent 的全部
        // required property，避免「先建空对象再补值」触发 required property 报错。
        childContentLoader.setSource(Qt.resolvedUrl("StarMapSceneContent.qml"), {
            "dt": dt,
            "rootStarmapId": rootStarmapId,
            "pathSegments": childContentPathSegments,
            // Issue #822: 完整 pathKey 在创建时一次给全，不存在默认 "root" 的中间态。
            "scenePathKey": childContentPathKey,
            "starmapBackendRef": starmapBackendRef,
            "selectionController": selectionController,
            "interactionController": interactionController,
            "depth": contentDepth,
            "rootContent": rootContent,
            "menuHost": menuHost,
            "ownerSceneContent": ownerSceneContent,
            // 子内容按当前档位渲染：interactive 完整交互；preview 只画静态投影。
            "renderDetail": childContentDetail
        })
    }
    onChildContentWantedChanged: _syncChildContent()
    // Repeater 复用 delegate 时 instanceId 可能变；身份变了子内容必须重建。
    onChildContentPathKeyChanged: {
        if (!_childContentRequested)
            return
        _childContentRequested = false
        childContentLoader.active = false
        _syncChildContent()
    }

    // 子内容实例（可能为 null：未进入视口或还在异步加载）。
    function childContent() {
        return childContentLoader.item
    }

    // chrome 命中区域高度 + 边框 hit slop。
    readonly property int _chromeHeight: 24
    readonly property int _borderSlop: 6

    // Issue #822 评论 5972215936: Embed 外壳是正圆，world 尺寸恒定（模型给
    // 直径 200，width === height），不再有 240×220 的矩形卡片。
    // 内容区取圆的内接正方形（再扣掉边框 slop），子内容不会溢出圆外。
    readonly property real _contentSide: Math.max(0, (width - 2 * _borderSlop) / Math.SQRT2)

    // ---------------------------------------------------------------------------
    // Issue #822：Deep Zoom 显示档位（对齐 docs/starmap_viewport.md）。
    // Embed 外壳 world 尺寸恒定：屏幕上多大 = 短边 × ownerEffectiveScale
    // （所属 Scene 的全局相机 × 祖先 local fit），绝不再拿 scale 改整颗 Embed，
    // 也绝不把本 Embed 子内容的 local fit 乘进外壳尺寸。
    // 档位只决定子内容渲染多少细节：
    //   interactive — coverage 越过 0.70（退出 0.60）时挂完整交互子内容
    //   preview     — 投影 ≥ 48px（退出 40px）时只画轻量静态投影
    //   shell       — 更小就只留外壳
    // ---------------------------------------------------------------------------
    readonly property real _interactiveEnterCoverage: 0.70
    readonly property real _interactiveExitCoverage: 0.60
    readonly property real _previewEnterPx: 48
    readonly property real _previewExitPx: 40

    // 子内容当前档位，带滞回，避免在阈值附近抖动。
    property string childContentDetail: "shell"

    // 投影尺寸 / 覆盖率都在函数里从原始输入现算：
    //   projectedSize = Embed world 短边 × ownerEffectiveScale
    //   coverage      = projectedSize / 根视口短边
    // 不能先声明成派生绑定再在变更处理器里读：QML 的属性变更信号先于依赖它的绑定
    // 被标脏，处理器里读派生绑定会拿到上一帧的旧值，档位会停在旧尺寸上。
    function resolveChildContentDetail() {
        var scale = ownerEffectiveScale > 0 ? ownerEffectiveScale : 1.0
        var projectedSize = Math.min(width, height) * scale
        var coverage = viewportShortSide > 0 ? projectedSize / viewportShortSide : 0
        var previous = childContentDetail
        var next = "shell"
        if (coverage >= _interactiveEnterCoverage
                || (previous === "interactive" && coverage >= _interactiveExitCoverage)) {
            next = "interactive"
        } else if (projectedSize >= _previewEnterPx
                || (previous === "preview" && projectedSize >= _previewExitPx)) {
            next = "preview"
        }
        if (next !== childContentDetail)
            childContentDetail = next
    }
    onOwnerEffectiveScaleChanged: resolveChildContentDetail()
    onViewportShortSideChanged: resolveChildContentDetail()
    onChildContentDetailChanged: {
        // 已经加载出来的子内容就地换档，不重建 Item。
        if (childContentLoader.item)
            childContentLoader.item.renderDetail = childContentDetail
        _syncChildContent()
    }

    property real visualOffsetX:
        (isSelected || isBeingDragged || chromeMouseTap.pressed || chromeTouchTap.pressed) ? 0 : _wobbleAnimX
    property real visualOffsetY:
        (isSelected || isBeingDragged || chromeMouseTap.pressed || chromeTouchTap.pressed) ? 0 : _wobbleAnimY

    // ---------------------------------------------------------------------------
    // 对外信号：Embed 只上抛事件，行为由共享状态机 / 归属层决定
    // ---------------------------------------------------------------------------
    signal clicked(string instanceId)
    // 桌面鼠标按下（Qt scene 坐标）：归属层据此登记 pressPending
    signal itemPressed(real sceneX, real sceneY)
    signal touchLongPressed(string instanceId)
    // 原始 Qt scene 坐标位移增量
    signal moveDelta(real dx, real dy)
    signal leftReleased()
    signal mouseInteracted()

    // ---------------------------------------------------------------------------
    // 内部视觉外壳：正圆。只有它承载 transform 偏移，根 Item 几何保持稳定。
    // 档位、缩放都不改这个圆的 world 几何（直径恒定）。
    // ---------------------------------------------------------------------------
    Rectangle {
        id: visualEmbed
        anchors.fill: parent

        radius: width / 2
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

        // ── chrome 区域 ──
        // 标题条（顶部 _chromeHeight 高度），挂 TapHandler/DragHandler。
        // 命中标题条：选择/移动/右键都作用于 Embed。
        // Handler 直接放进 titleBar 内部，parent Item 就是 titleBar，命中范围限定在标题条。
        //
        // Issue #812: 本文件所有桌面语义 Handler 的 acceptedDevices 一律
        // Mouse | TouchPad，不能只写 Mouse。acceptedDevices 是硬过滤。
        // 触屏语义（TouchScreen 长按）保持独立，不混进来。
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
                onPressedChanged: {
                    if (pressed) {
                        root.mouseInteracted()
                        root.logEmbedInteraction("embed_chrome_press", {
                            "parentPathKey": root.parentPathKey,
                            "childContentPathKey": root.childContentPathKey,
                            "instanceId": root.instanceId,
                            "targetStarmapId": root.targetStarmapId,
                            "chromeRegion": "title",
                            "device": "mouse"
                        })
                        root.itemPressed(chromeMouseTap.point.scenePressPosition.x,
                                         chromeMouseTap.point.scenePressPosition.y)
                    }
                }
                onSingleTapped: root.clicked(root.instanceId)
            }

            TapHandler {
                id: chromeTouchTap
                acceptedDevices: PointerDevice.TouchScreen
                acceptedButtons: Qt.LeftButton
                onPressedChanged: {
                    if (pressed) {
                        root.logEmbedInteraction("embed_chrome_press", {
                            "parentPathKey": root.parentPathKey,
                            "childContentPathKey": root.childContentPathKey,
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
                        root.logEmbedInteraction("embed_chrome_press", {
                            "parentPathKey": root.parentPathKey,
                            "childContentPathKey": root.childContentPathKey,
                            "instanceId": root.instanceId,
                            "targetStarmapId": root.targetStarmapId,
                            "chromeRegion": "title",
                            "device": "mouse",
                            "button": "right"
                        })
                    }
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
                    // 只上抛原始 activeTranslation 增量（Qt scene 坐标）；
                    // Qt scene → 本层 local 的换算只在归属层做一次。
                    var dx = activeTranslation.x - lastTx
                    var dy = activeTranslation.y - lastTy
                    lastTx = activeTranslation.x
                    lastTy = activeTranslation.y
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

        // ── 四条边框命中区域 ──
        // 边框有少量 hit slop，命中时选择/移动作用于 Embed。
        // 每条 border 内部放一套与 titleBar 对称的 handler。
        Rectangle {
            id: borderTop
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.top: parent.top
            height: root._borderSlop
            color: "transparent"

            TapHandler {
                id: borderTopMouseTap
                acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
                acceptedButtons: Qt.LeftButton
                onPressedChanged: {
                    if (pressed) {
                        root.mouseInteracted()
                        root.logEmbedInteraction("embed_chrome_press", {
                            "parentPathKey": root.parentPathKey,
                            "childContentPathKey": root.childContentPathKey,
                            "instanceId": root.instanceId,
                            "targetStarmapId": root.targetStarmapId,
                            "chromeRegion": "borderTop",
                            "device": "mouse"
                        })
                        root.itemPressed(borderTopMouseTap.point.scenePressPosition.x,
                                         borderTopMouseTap.point.scenePressPosition.y)
                    }
                }
                onSingleTapped: root.clicked(root.instanceId)
            }
            TapHandler {
                acceptedDevices: PointerDevice.TouchScreen
                acceptedButtons: Qt.LeftButton
                onSingleTapped: root.clicked(root.instanceId)
                onLongPressed: root.touchLongPressed(root.instanceId)
            }
            TapHandler {
                acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
                acceptedButtons: Qt.RightButton
                onPressedChanged: {
                    if (pressed) {
                        root.mouseInteracted()
                        root.logEmbedInteraction("embed_chrome_press", {
                            "parentPathKey": root.parentPathKey,
                            "childContentPathKey": root.childContentPathKey,
                            "instanceId": root.instanceId,
                            "targetStarmapId": root.targetStarmapId,
                            "chromeRegion": "borderTop",
                            "device": "mouse",
                            "button": "right"
                        })
                    }
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
                    // 只上抛原始 activeTranslation 增量（Qt scene 坐标）；
                    // Qt scene → 本层 local 的换算只在归属层做一次。
                    var dx = activeTranslation.x - lastTx
                    var dy = activeTranslation.y - lastTy
                    lastTx = activeTranslation.x
                    lastTy = activeTranslation.y
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
                id: borderBottomMouseTap
                acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
                acceptedButtons: Qt.LeftButton
                onPressedChanged: {
                    if (pressed) {
                        root.mouseInteracted()
                        root.logEmbedInteraction("embed_chrome_press", {
                            "parentPathKey": root.parentPathKey,
                            "childContentPathKey": root.childContentPathKey,
                            "instanceId": root.instanceId,
                            "targetStarmapId": root.targetStarmapId,
                            "chromeRegion": "borderBottom",
                            "device": "mouse"
                        })
                        root.itemPressed(borderBottomMouseTap.point.scenePressPosition.x,
                                         borderBottomMouseTap.point.scenePressPosition.y)
                    }
                }
                onSingleTapped: root.clicked(root.instanceId)
            }
            TapHandler {
                acceptedDevices: PointerDevice.TouchScreen
                acceptedButtons: Qt.LeftButton
                onSingleTapped: root.clicked(root.instanceId)
                onLongPressed: root.touchLongPressed(root.instanceId)
            }
            TapHandler {
                acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
                acceptedButtons: Qt.RightButton
                onPressedChanged: {
                    if (pressed) {
                        root.mouseInteracted()
                        root.logEmbedInteraction("embed_chrome_press", {
                            "parentPathKey": root.parentPathKey,
                            "childContentPathKey": root.childContentPathKey,
                            "instanceId": root.instanceId,
                            "targetStarmapId": root.targetStarmapId,
                            "chromeRegion": "borderBottom",
                            "device": "mouse",
                            "button": "right"
                        })
                    }
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
                    // 只上抛原始 activeTranslation 增量（Qt scene 坐标）；
                    // Qt scene → 本层 local 的换算只在归属层做一次。
                    var dx = activeTranslation.x - lastTx
                    var dy = activeTranslation.y - lastTy
                    lastTx = activeTranslation.x
                    lastTy = activeTranslation.y
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
                id: borderLeftMouseTap
                acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
                acceptedButtons: Qt.LeftButton
                onPressedChanged: {
                    if (pressed) {
                        root.mouseInteracted()
                        root.logEmbedInteraction("embed_chrome_press", {
                            "parentPathKey": root.parentPathKey,
                            "childContentPathKey": root.childContentPathKey,
                            "instanceId": root.instanceId,
                            "targetStarmapId": root.targetStarmapId,
                            "chromeRegion": "borderLeft",
                            "device": "mouse"
                        })
                        root.itemPressed(borderLeftMouseTap.point.scenePressPosition.x,
                                         borderLeftMouseTap.point.scenePressPosition.y)
                    }
                }
                onSingleTapped: root.clicked(root.instanceId)
            }
            TapHandler {
                acceptedDevices: PointerDevice.TouchScreen
                acceptedButtons: Qt.LeftButton
                onSingleTapped: root.clicked(root.instanceId)
                onLongPressed: root.touchLongPressed(root.instanceId)
            }
            TapHandler {
                acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
                acceptedButtons: Qt.RightButton
                onPressedChanged: {
                    if (pressed) {
                        root.mouseInteracted()
                        root.logEmbedInteraction("embed_chrome_press", {
                            "parentPathKey": root.parentPathKey,
                            "childContentPathKey": root.childContentPathKey,
                            "instanceId": root.instanceId,
                            "targetStarmapId": root.targetStarmapId,
                            "chromeRegion": "borderLeft",
                            "device": "mouse",
                            "button": "right"
                        })
                    }
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
                    // 只上抛原始 activeTranslation 增量（Qt scene 坐标）；
                    // Qt scene → 本层 local 的换算只在归属层做一次。
                    var dx = activeTranslation.x - lastTx
                    var dy = activeTranslation.y - lastTy
                    lastTx = activeTranslation.x
                    lastTy = activeTranslation.y
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
                id: borderRightMouseTap
                acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
                acceptedButtons: Qt.LeftButton
                onPressedChanged: {
                    if (pressed) {
                        root.mouseInteracted()
                        root.logEmbedInteraction("embed_chrome_press", {
                            "parentPathKey": root.parentPathKey,
                            "childContentPathKey": root.childContentPathKey,
                            "instanceId": root.instanceId,
                            "targetStarmapId": root.targetStarmapId,
                            "chromeRegion": "borderRight",
                            "device": "mouse"
                        })
                        root.itemPressed(borderRightMouseTap.point.scenePressPosition.x,
                                         borderRightMouseTap.point.scenePressPosition.y)
                    }
                }
                onSingleTapped: root.clicked(root.instanceId)
            }
            TapHandler {
                acceptedDevices: PointerDevice.TouchScreen
                acceptedButtons: Qt.LeftButton
                onSingleTapped: root.clicked(root.instanceId)
                onLongPressed: root.touchLongPressed(root.instanceId)
            }
            TapHandler {
                acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
                acceptedButtons: Qt.RightButton
                onPressedChanged: {
                    if (pressed) {
                        root.mouseInteracted()
                        root.logEmbedInteraction("embed_chrome_press", {
                            "parentPathKey": root.parentPathKey,
                            "childContentPathKey": root.childContentPathKey,
                            "instanceId": root.instanceId,
                            "targetStarmapId": root.targetStarmapId,
                            "chromeRegion": "borderRight",
                            "device": "mouse",
                            "button": "right"
                        })
                    }
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
                    // 只上抛原始 activeTranslation 增量（Qt scene 坐标）；
                    // Qt scene → 本层 local 的换算只在归属层做一次。
                    var dx = activeTranslation.x - lastTx
                    var dy = activeTranslation.y - lastTy
                    lastTx = activeTranslation.x
                    lastTy = activeTranslation.y
                    root.moveDelta(dx, dy)
                }
            }
            PointHandler {
                acceptedButtons: Qt.LeftButton
                onActiveChanged: { if (!active) root.leftReleased() }
            }
        }

        // ── contentViewport ──
        // 圆的内接正方形：子内容完全落在圆形外壳内，不会从圆边溢出。
        // 父 Embed 不挂 TapHandler/DragHandler/MouseArea，事件直接给子星图内容；
        // clip 保留，子星图节点不会跑出这块内容区。
        //
        // 唯一允许的 handler 是 passive grab 的 PointHandler，它只观察 press 并记录
        // embed_child_content_routed 边界日志，不拦截事件。
        Item {
            id: contentViewport
            width: root._contentSide
            height: root._contentSide
            anchors.centerIn: parent
            clip: true

            // embed_child_content_routed 边界日志。
            // PointHandler 用 passive grab 观察 press，不抢事件。
            PointHandler {
                id: contentViewportPressObserver
                acceptedButtons: Qt.LeftButton | Qt.RightButton
                onActiveChanged: {
                    if (active) {
                        root.logEmbedInteraction("embed_child_content_routed", {
                            "parentPathKey": root.parentPathKey,
                            "childContentPathKey": root.childContentPathKey,
                            "instanceId": root.instanceId,
                            "targetStarmapId": root.targetStarmapId
                        })
                    }
                }
            }

            // Issue #822：懒加载子星图内容（不是子视口）。
            // 子内容没有自己的 pan/zoom，也没有子视口手势状态。
            // active / source 都由 root._syncChildContent() 命令式驱动：
            // source 必须是运行时 URL，静态 sourceComponent 会把
            // StarMapSceneContent 变成 Embed 的编译期类型依赖，从而形成环。
            Loader {
                id: childContentLoader
                anchors.fill: parent
                // 只在该 Embed 投影矩形进入视口后才创建递归子内容。
                // asynchronous 避免一帧里同步构造多层 QML 对象树把 GUI 线程堵死。
                active: false
                asynchronous: true
                // setSource 的 initialProperties 是创建时的快照；异步加载完成时
                // 档位可能已经变了，这里把子内容拉回当前档位。
                onLoaded: {
                    if (childContentLoader.item)
                        childContentLoader.item.renderDetail = root.childContentDetail
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
    // 所有 PointerHandler 都放在 titleBar / border 内部（parent Item 决定命中范围）。
    // 根 Item 和 contentViewport 祖先链上没有任何独占 grab 的
    // TapHandler / DragHandler / MouseArea，子星图内容的内部事件不会被父 Embed 截走。
    // ---------------------------------------------------------------------------
}
