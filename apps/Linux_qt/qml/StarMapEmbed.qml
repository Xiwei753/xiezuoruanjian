// =============================================================================
// StarMapEmbed.qml — 子星图 Embed 卡片组件（递归内容渲染）
// =============================================================================
//
// 层级：Linux_qt UI 层（QML UI 组件）
// 职责：单个子星图 Embed 的可视化渲染、选中态展示、递归内容懒加载
//
// Issue #832：Embed 只负责"怎么画、什么时候加载子内容"，不再负责
// "这个鼠标动作是什么意思"：
//   - chrome 上的左键/右键/触屏 TapHandler、DragHandler、PointHandler 全部删除，
//     业务手势统一由根层 StarMapInputRouter 解释；
//   - contentViewport 的 embed_child_content_routed 输入观察层删除；
//   - 命中几何（正圆 chrome / 圆内内容区）只有一份真相：GraphController 的
//     递归命中测试，不再维护第二套 PointerHandler acceptance；
//   - pressed/move 视觉统一绑定共享 StarMapInteractionController。
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
//   子内容布局被限制在圆的内接正方形扣掉交互壳的安全区里（fit 与 clamp 同源），
//   既不溢出圆外，也不会盖住父圆"点标题/边框选中"的入口。
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

    // Issue #832：身份与共享手势状态机，只用于 pressed/move 视觉查询（不解释输入）。
    property string scenePathKey: ""
    property var interactionController: null

    // Issue #814 评论 5935285879: 共享选中控制器，由归属层传入，传给子内容。
    property var selectionController: null

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

    // Issue #832：是否被当前手势按住/拖动（视觉动画暂停的唯一判据）。
    readonly property bool isPressed: interactionController
            ? interactionController.isPressedTarget(scenePathKey, "embed", instanceId)
            : false

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
        syncChildUsableSide()
    }
    onHeightChanged: {
        resolveChildContentDetail()
        recomputeChildContentInViewport()
        syncChildUsableSide()
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

    // Issue #814 评论 5935346839: Embed 只记录自己的渲染分层事件。
    // starmapBackendRef 为 null 时静默跳过。不记录连续移动/输入观察。
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
    // 递归加载用的路径段由归属层 Controller 的 embedPathSegment() 分流后传入：
    // 正式 Embed 是 enterEmbed{instanceId}，旧 portal 归一的是 enterPortal{nodeId}。
    // Embed 不再自己重新猜路径（那样旧 portal 会拿不存在的 instanceId 去解析）。
    property var pathSegment
    readonly property var childContentPathSegments: parentPathSegments.concat([pathSegment])
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
            // 子内容安全区来源：父圆的内接正方形扣掉交互壳（子内容再扣自己的留白）。
            // local fit 与移动/新建 clamp 共用这一份；onLoaded 时再同步一次，
            // 避免创建事务内的旧值。
            "contentUsableSide": contentUsableSideNow(),
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

    // 外壳的标题带高度 + 圆周边框交互壳宽度：子内容安全区要扣掉这一圈。
    readonly property int _chromeHeight: 24
    readonly property int _borderSlop: 6

    // 子内容可用边长（世界/本层局部单位）：圆的内接正方形扣掉父圆的交互壳
    // （标题 24 + 边框 6）。子内容再扣自己的留白得到安全区，
    // 由 StarMapSceneContent 的 local fit 与移动/新建 clamp 共用同一份。
    //
    // 用函数现算而不是 readonly 派生绑定：delegate 创建事务里 width/height 与
    // 派生绑定的求值顺序不保证，Component.onCompleted 里读派生绑定会拿到
    // 事务开始前的旧值（0），安全区就会以 0 传给子内容。
    function contentUsableSideNow() {
        var d = Math.min(width, height)
        return Math.max(0, d * (1 / Math.SQRT2) - _chromeHeight - _borderSlop)
    }

    // 子内容实例已经挂上时，把当前安全区同步过去（创建后 / 尺寸变化时）。
    function syncChildUsableSide() {
        if (childContentLoader.item)
            childContentLoader.item.contentUsableSide = contentUsableSideNow()
    }

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
        (isSelected || isPressed) ? 0 : _wobbleAnimX
    property real visualOffsetY:
        (isSelected || isPressed) ? 0 : _wobbleAnimY

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

        // ── 标题文字（纯展示）──
        // 点不点得到由递归命中测试决定（GraphController 的圆壳几何），
        // 文字只是画在圆顶部。
        AppText {
            id: titleLabel
            dt: root.dt
            anchors.top: parent.top
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.leftMargin: 8
            anchors.rightMargin: 8
            height: root._chromeHeight
            text: root.label
            color: root._textPrimary
            font.pointSize: root.dt.fontSmPt
            wrapMode: Text.NoWrap
            elide: Text.ElideRight
            horizontalAlignment: Text.AlignHCenter
            verticalAlignment: Text.AlignVCenter
        }

        // ── contentViewport ──
        // 铺满整个圆盒：圆内（chrome 之外）都属于"进入子图"的交互语义，
        // 不再用一块更小的矩形制造接不到事件的死区。
        // 子内容布局由安全区约束（见 contentUsableSideNow），不会溢出圆外。
        // clip 保留，子星图节点不会跑出这块内容区。
        Item {
            id: contentViewport
            anchors.fill: parent
            clip: true

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
                    if (childContentLoader.item) {
                        childContentLoader.item.renderDetail = root.childContentDetail
                        root.syncChildUsableSide()
                    }
                }
            }
        }
    }

    // wobble 降速，和 StarMapNode.qml 一致；选中/按下/拖动时动画暂停
    // Issue #832: 按下判据改绑共享 InteractionController（Embed 自己不再有 Handler）
    SequentialAnimation on _wobbleAnimX {
        loops: Animation.Infinite
        running: !isSelected && !isPressed
        NumberAnimation { to: 0.6; duration: 7000 + (wobbleIndex % 7) * 400; easing.type: Easing.InOutSine }
        NumberAnimation { to: -0.6; duration: 7000 + (wobbleIndex % 7) * 400; easing.type: Easing.InOutSine }
    }
    SequentialAnimation on _wobbleAnimY {
        loops: Animation.Infinite
        running: !isSelected && !isPressed
        NumberAnimation { to: 0.4; duration: 8500 + (wobbleIndex % 5) * 300; easing.type: Easing.InOutSine }
        NumberAnimation { to: -0.4; duration: 8500 + (wobbleIndex % 5) * 300; easing.type: Easing.InOutSine }
    }
}
