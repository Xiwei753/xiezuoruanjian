// =============================================================================
// WritingChapterNavigation.qml — 章节导航（作品标题 / 卷章树 / 新卷 / 右键菜单）
// =============================================================================
//
// 层级：Linux_qt UI 层（QML UI 组件）
// 职责：渲染左树的全部章节导航内容，供 WritingWorkspace 在 Workbench 和
//   SinglePane 两种壳模式下复用，避免两套结构各画一份。
// 边界：纯展示 + 信号回调。展开态、当前章节、树数据都由调用方持有，
//   组件自己不存业务状态，也不查后端。
//
// Issue #836：左树语义纠正。作品标题是静态行（不可折叠），卷仍走
//   WritingTreeGroupHeader 的展开/收起。章纲（chapter.note）不再在
//   左树里显示——它是 Core 数据字段，不是左树分组。
//
// Issue #833 复核：从 WritingWorkspace.qml 的 sidebarRect 抽出。
//   - Workbench：作为 RowLayout 子项，宽度只吃 Core 的 ChapterNavigation bounds。
//   - SinglePane：作为覆盖在正文上的抽屉/浮层打开，不参与 RowLayout 宽度分配。
//
// Issue #900：自绘 AppText / 背景 / 右键菜单外观迁移到 Breeze + Kirigami
//   主题色与间距；chapter 行 ⋯ 按钮换成 Breeze overflow-menu 图标。
//   WritingTreeController、信号、函数与全部注入属性保持不变。
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

Rectangle {
    id: root

    required property var dt
    // flat tree 数组（与 WritingWorkspace.tree 同源）。
    property var tree: []
    // 当前作品 id，用于 WritingTreeController 过滤。
    property string workspaceProjectId: ""
    // 当前作品标题（左树顶部静态标题行）。
    property string workspaceProjectTitle: ""
    // 当前选中章节 id（用于列表项高亮）。
    property string currentChapterId: ""

    // Issue #1051：拖动排序需要调用 ProjectBackend 的 reorder/move 接口，
    // 由 WritingWorkspace 注入（组件自身不查后端，只把拖动结果交给调用方）。
    property var projectBackendRef: null

    // ── Issue #1051：左树拖动排序状态机 ──
    // 按住先记录候选源，位移达到 Qt 通用拖动阈值后才进入拖动；
    // 拖动期间真实 delegate 留在原位，只有 dragPreview 浮层跟随指针。
    property bool dragActive: false
    // 拖动结束后吞掉随之而来的 onClicked，避免误开章节/误折叠卷。
    property bool dragSuppressClick: false
    property string dragSourceId: ""
    property string dragSourceKind: ""
    property string dragSourceVolumeId: ""
    property string dragSourceTitle: ""
    property real dragPressX: 0
    property real dragPressY: 0
    property real dragPointerX: 0
    property real dragPointerY: 0

    // 落点：dropTargetIndex 是插入线所在的 model 行下标（0..count），
    // dropLineY 是插入线在 ListView 内容坐标里的 Y。
    property int dropTargetIndex: -1
    property string dropTargetId: ""
    property string dropTargetKind: ""
    property string dropTargetVolumeId: ""
    property bool dropInsertAfter: false
    property bool dropTargetValid: false
    property real dropLineY: -1

    // Qt 默认 startDragDistance 为 10 逻辑像素；QML 的 Qt.styleHints 属性
    // 未在 qmllint 类型信息中暴露，这里用同值常量，保持与 Qt 拖动阈值一致。
    readonly property int dragThreshold: 10

    signal openChapter(string projectId, string volumeId, string chapterId, string chapterTitle)
    // Issue #1051 复审：跨卷移动会移动正文文件，组件不直接调 Core；由工作区
    // 先保存正在编辑的正文，再提交移动并迁移编辑器身份。
    signal chapterMoveRequested(string projectId, string sourceVolumeId, string targetVolumeId,
                                string chapterId, int targetIndex)
    signal createVolumeRequested(string projectId)
    signal createChapterRequested(string projectId, string volumeId)
    signal renameItemRequested(var itemData)
    signal deleteItemRequested(var itemData)

    color: Kirigami.Theme.alternateBackgroundColor

    // Issue #912：章/卷行高按桌面默认字体实际高度 + KDE 间距派生，替代
    // gridUnit*1.8 / gridUnit*2 这类在部分 DPI 下偏小的硬值；行内文字、
    // 操作入口随之回到可读的桌面尺寸。
    TextMetrics {
        id: lineMetrics
        font: Kirigami.Theme.defaultFont
        text: "章节标题"
    }
    readonly property real volumeRowHeight: Math.max(Kirigami.Units.gridUnit * 2, lineMetrics.height + Kirigami.Units.smallSpacing * 2)
    readonly property real chapterRowHeight: Math.max(Kirigami.Units.gridUnit * 1.8, lineMetrics.height + Kirigami.Units.smallSpacing * 2)

    WritingTreeController {
        id: writingTree
        tree: root.tree
        projectId: root.workspaceProjectId
        onItemsChanged: root.buildTreeModel()
    }

    ListModel {
        id: treeModel
    }

    // Issue #835：卷展开态是纯端侧 UI 状态，不进 Core、不进同步。
    // 用 { volumeId: bool } 记录，切换时整体赋值触发更新并重建 model。
    property var volumeExpandedMap: ({})

    function isVolumeExpanded(volumeId) {
        return !!root.volumeExpandedMap[volumeId]
    }

    // Issue #835 评论 6019235318: property var 修改 JS 对象内部成员不触发
    // changed 信号，必须赋一个全新对象引用，onVolumeExpandedMapChanged 才会
    // 可靠触发并重建 treeModel。见 https://doc.qt.io/qt-6.12/qml-var.html
    function toggleVolumeExpanded(volumeId) {
        var next = {}
        for (var key in root.volumeExpandedMap) {
            next[key] = root.volumeExpandedMap[key]
        }
        next[volumeId] = !root.isVolumeExpanded(volumeId)
        root.volumeExpandedMap = next
    }

    function volumesArray() {
        var items = writingTree.items || []
        var vols = []
        for (var i = 0; i < items.length; i++) {
            if (items[i].type === "volume") vols.push(items[i])
        }
        return vols
    }

    function chaptersOfVolume(volumeId) {
        var items = writingTree.items || []
        var chs = []
        for (var i = 0; i < items.length; i++) {
            if (items[i].type === "chapter" && items[i].volumeId === volumeId) chs.push(items[i])
        }
        return chs
    }

    // ── Issue #1051：拖动排序 ────────────────────────────────────────────────

    function modelRowHeight(index) {
        if (index < 0 || index >= treeModel.count) return 0
        return treeModel.get(index).rowType === "volume" ? root.volumeRowHeight
                                                         : root.chapterRowHeight
    }

    function beginDragCandidate(kind, itemId, volumeId, title, mouseX, mouseY) {
        root.resetDragState()
        root.dragSourceKind = kind || ""
        root.dragSourceId = itemId || ""
        root.dragSourceVolumeId = volumeId || ""
        root.dragSourceTitle = title || ""
        root.dragPressX = mouseX
        root.dragPressY = mouseY
        root.dragActive = false
    }

    function resetDragState() {
        root.dragActive = false
        root.dragSourceId = ""
        root.dragSourceKind = ""
        root.dragSourceVolumeId = ""
        root.dragSourceTitle = ""
        root.clearDropTarget()
    }

    function clearDropTarget() {
        root.dropTargetIndex = -1
        root.dropTargetId = ""
        root.dropTargetKind = ""
        root.dropTargetVolumeId = ""
        root.dropInsertAfter = false
        root.dropTargetValid = false
        root.dropLineY = -1
    }

    function setDropTarget(index, targetId, targetKind, targetVolumeId, insertAfter, lineY) {
        root.dropTargetIndex = index
        root.dropTargetId = targetId || ""
        root.dropTargetKind = targetKind || ""
        root.dropTargetVolumeId = targetVolumeId || ""
        root.dropInsertAfter = insertAfter
        root.dropLineY = lineY
        var isSelf = (targetKind === root.dragSourceKind
                      && targetId === root.dragSourceId
                      && targetVolumeId === root.dragSourceVolumeId)
        root.dropTargetValid = root.projectBackendRef !== null
                               && root.dragSourceId !== ""
                               && root.dropTargetId !== ""
                               && !isSelf
    }

    // 超过阈值才进入拖动；contentYPos 是 ListView 内容坐标。
    function updateDrag(mouseX, mouseY, contentYPos) {
        if (root.dragSourceId === "") return
        if (!root.dragActive) {
            var dx = mouseX - root.dragPressX
            var dy = mouseY - root.dragPressY
            if (Math.sqrt(dx * dx + dy * dy) < root.dragThreshold) return
            root.dragActive = true
        }
        if (root.dragSourceKind === "volume") root.updateVolumeDropTarget(contentYPos)
        else if (root.dragSourceKind === "chapter") root.updateChapterDropTarget(contentYPos)
    }

    // 卷拖动：以「卷块」为落点（卷头 + 其展开的章节）。落点在卷头上半 → 插到
    // 该卷之前；其余位置 → 插到下一个卷块之前；超出末尾 → 插到最后一个卷块后。
    function updateVolumeDropTarget(contentYPos) {
        var y = 0
        var lastVolumeRow = -1
        for (var i = 0; i < treeModel.count; i++) {
            var row = treeModel.get(i)
            var h = root.modelRowHeight(i)
            if (row.rowType === "volume") {
                if (contentYPos < y + h / 2) {
                    root.setDropTarget(i, row.itemId, "volume", row.itemId, false, y)
                    return
                }
                lastVolumeRow = i
            }
            y += h
        }
        if (lastVolumeRow >= 0) {
            var last = treeModel.get(lastVolumeRow)
            root.setDropTarget(treeModel.count, last.itemId, "volume", last.itemId, true, y)
        } else {
            root.clearDropTarget()
        }
    }

    // 章节拖动：章节行按上下半区决定插到该行前/后；卷头行插到该卷最前；
    // 超出末尾追加到最后一个卷。
    function updateChapterDropTarget(contentYPos) {
        var y = 0
        var lastVolumeRow = -1
        for (var i = 0; i < treeModel.count; i++) {
            var row = treeModel.get(i)
            var h = root.modelRowHeight(i)
            if (row.rowType === "volume") {
                lastVolumeRow = i
                if (contentYPos < y + h) {
                    root.setDropTarget(i + 1, row.itemId, "volume", row.itemId, false, y + h)
                    return
                }
            } else if (contentYPos < y + h) {
                var insertAfter = contentYPos > y + h / 2
                root.setDropTarget(insertAfter ? i + 1 : i, row.itemId, "chapter",
                                   row.itemVolumeId, insertAfter,
                                   insertAfter ? y + h : y)
                return
            }
            y += h
        }
        if (lastVolumeRow >= 0) {
            var last = treeModel.get(lastVolumeRow)
            root.setDropTarget(treeModel.count, last.itemId, "volume", last.itemId, true, y)
        } else {
            root.clearDropTarget()
        }
    }

    function endDrag() {
        if (!root.dragActive) {
            // 未达到阈值：按普通单击处理，不落盘。
            root.resetDragState()
            return
        }
        var sourceId = root.dragSourceId
        var sourceKind = root.dragSourceKind
        var sourceVolumeId = root.dragSourceVolumeId
        var targetId = root.dropTargetId
        var targetKind = root.dropTargetKind
        var targetVolumeId = root.dropTargetVolumeId
        var insertAfter = root.dropInsertAfter
        var valid = root.dropTargetValid
        root.resetDragState()
        // 拖动结束后的 onClicked 不应再触发打开章节/折叠卷。
        root.dragSuppressClick = true
        Qt.callLater(function() { root.dragSuppressClick = false })
        if (valid) {
            // 延后到当前鼠标事件处理结束再提交：Core 成功后左树会在本轮内
            // 重建 model，如果在 delegate 的 released 处理器里同步 clear model，
            // 可能销毁正在派发事件的 delegate。Qt.callLater 让提交发生在
            // 事件循环下一拍，delegate 已不再参与事件派发。
            Qt.callLater(function() {
                root.commitReorder(sourceId, sourceKind, sourceVolumeId,
                                   targetId, targetKind, targetVolumeId, insertAfter)
            })
        }
    }

    // Issue #1051：把一次 drop 换算成 Core 的完整顺序序列并调用后端。
    // - 卷排序：volumesArray() 完整 ID 顺序，移出 source 后按目标前/后插入；
    // - 同卷章排序：chaptersOfVolume() 完整章 ID 顺序重排；
    // - 跨卷移动章节：一次原子 Core move（不能用两个卷的 reorder 伪造）。
    // 空位置/相同 ID/位置未变化都不调用 Core。
    function commitReorder(sourceId, sourceKind, sourceVolumeId,
                           targetId, targetKind, targetVolumeId, insertAfter) {
        if (!root.projectBackendRef || !root.workspaceProjectId) return
        if (!sourceId || !targetId) return
        if (sourceKind === targetKind && sourceId === targetId
                && sourceVolumeId === targetVolumeId) return

        if (sourceKind === "volume" && targetKind === "volume") {
            var vols = root.volumesArray()
            var ids = []
            for (var i = 0; i < vols.length; i++) ids.push(vols[i].id)
            var from = ids.indexOf(sourceId)
            if (from < 0) return
            ids.splice(from, 1)
            var targetAt = ids.indexOf(targetId)
            if (targetAt < 0) return
            if (insertAfter) targetAt += 1
            // 目标插入位置与原位置相同，顺序没变，不写盘。
            if (targetAt === from) return
            ids.splice(targetAt, 0, sourceId)
            root.projectBackendRef.reorder_volumes(root.workspaceProjectId, ids.join(","))
            return
        }

        if (sourceKind !== "chapter") return

        if (sourceVolumeId === targetVolumeId) {
            var chapters = root.chaptersOfVolume(sourceVolumeId)
            var chapterIds = []
            for (var j = 0; j < chapters.length; j++) chapterIds.push(chapters[j].id)
            var chapterFrom = chapterIds.indexOf(sourceId)
            if (chapterFrom < 0) return
            chapterIds.splice(chapterFrom, 1)
            var chapterAt = 0
            if (targetKind === "chapter") {
                chapterAt = chapterIds.indexOf(targetId)
                if (chapterAt < 0) return
                if (insertAfter) chapterAt += 1
            } else if (insertAfter) {
                // 落到本卷末尾（卷头下方的追加位置）。
                chapterAt = chapterIds.length
            }
            if (chapterAt === chapterFrom) return
            chapterIds.splice(chapterAt, 0, sourceId)
            root.projectBackendRef.reorder_chapters(root.workspaceProjectId,
                                                    sourceVolumeId,
                                                    chapterIds.join(","))
            return
        }

        // 跨卷移动：目标卷内插入位置。
        var targetChapters = root.chaptersOfVolume(targetVolumeId)
        var insertAt = insertAfter ? targetChapters.length : 0
        if (targetKind === "chapter") {
            insertAt = targetChapters.length
            for (var k = 0; k < targetChapters.length; k++) {
                if (targetChapters[k].id === targetId) {
                    insertAt = k + (insertAfter ? 1 : 0)
                    break
                }
            }
        }
        // 跨卷移动 = 移动正文文件，必须先把正在编辑的章节落盘；
        // 交给 WritingWorkspace 统一“保存 -> move -> 迁移身份”。
        root.chapterMoveRequested(root.workspaceProjectId, sourceVolumeId, targetVolumeId,
                                  sourceId, insertAt)
    }

    // Issue #835：按层级重建 model —— 每卷一行 header，卷展开时紧接其章节行。
    // 取代旧 flat ListView（把 volume 当 36px 普通列表行）。
    // WritingTreeController 已按 project → volume → chapter 顺序输出 items，
    // 这里按 type 分组挂载，不重排 Core 的输出顺序。
    function buildTreeModel() {
        treeModel.clear()
        var vols = root.volumesArray()
        for (var i = 0; i < vols.length; i++) {
            var vol = vols[i]
            var volExpanded = root.isVolumeExpanded(vol.id)
            treeModel.append({
                "rowType": "volume",
                "itemId": vol.id || "",
                "itemTitle": vol.title || "",
                "itemProjectId": vol.projectId || "",
                "itemVolumeId": vol.id || "",
                "expanded": volExpanded
            })
            if (volExpanded) {
                var chs = root.chaptersOfVolume(vol.id)
                for (var j = 0; j < chs.length; j++) {
                    var ch = chs[j]
                    treeModel.append({
                        "rowType": "chapter",
                        "itemId": ch.id || "",
                        "itemTitle": ch.title || "",
                        "itemProjectId": ch.projectId || "",
                        "itemVolumeId": ch.volumeId || "",
                        "expanded": false
                    })
                }
            }
        }
    }

    onVolumeExpandedMapChanged: root.buildTreeModel()

    Component.onCompleted: root.buildTreeModel()

    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        // ── Issue #836：左树顶部静态标题行 ──
        // 作品标题是当前写作上下文的标识，不是可折叠分组头。
        // 整行只显示 workspaceProjectTitle，没有折叠箭头、没有点击折叠。
        Rectangle {
            Layout.fillWidth: true
            Layout.preferredHeight: root.volumeRowHeight
            color: "transparent"

            Label {
                anchors.left: parent.left
                anchors.leftMargin: Kirigami.Units.smallSpacing
                anchors.verticalCenter: parent.verticalCenter
                text: root.workspaceProjectTitle
                color: Kirigami.Theme.textColor
                font.family: Kirigami.Theme.defaultFont.family
                font.pointSize: Kirigami.Theme.defaultFont.pointSize
                font.bold: true
                elide: Text.ElideRight
            }
        }

        // Tree list
        // Issue #835：按层级渲染 —— 卷是可折叠分组头（WritingTreeGroupHeader），
        // 卷展开时紧接其章节行。取代旧 flat ListView 把 volume 当 36px 列表行的画法。
        ScrollView {
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            // Issue #782 评论 5855709706: 桌面鼠标左键不能按住空白处拖页面。
            Component.onCompleted: {
                if (contentItem) contentItem.acceptedButtons = Qt.NoButton
            }

            ListView {
                id: treeListView
                model: treeModel
                // Issue #842: 桌面鼠标左键不能按住空白处拖页面，
                // 保留滚轮/触摸板/滚动条/触屏滚动。
                acceptedButtons: Qt.NoButton
                delegate: Item {
                    width: treeListView.width
                    height: model.rowType === "volume" ? root.volumeRowHeight
                                                    : root.chapterRowHeight

                    // ── 卷分组头 ──
                    WritingTreeGroupHeader {
                        visible: model.rowType === "volume"
                        anchors.fill: parent
                        dt: root.dt
                        title: model.itemTitle || ""
                        expanded: model.expanded
                        // 卷头右侧带「+ 新章节」
                        showAddButton: true
                        onToggleExpanded: root.toggleVolumeExpanded(model.itemId)
                        onAddRequested: root.createChapterRequested(model.itemProjectId || root.workspaceProjectId, model.itemId)
                    }

                    // 卷头右键菜单（重命名 / 删除）。WritingTreeGroupHeader 内部
                    // MouseArea 只吃左键，右键穿透到这里。
                    MouseArea {
                        visible: model.rowType === "volume"
                        anchors.fill: parent
                        acceptedButtons: Qt.RightButton
                        z: 10
                        cursorShape: Qt.PointingHandCursor
                        onClicked: function(mouse) {
                            if (mouse.button === Qt.RightButton) {
                                treeContextMenu.itemType = "volume"
                                treeContextMenu.itemId = model.itemId
                                treeContextMenu.itemTitle = model.itemTitle
                                treeContextMenu.itemProjectId = model.itemProjectId || ""
                                treeContextMenu.itemVolumeId = model.itemId || ""
                                treeContextMenu.popup(this, mouse.x, mouse.y)
                            }
                        }
                    }

                    // Issue #1051：卷头左键按住可拖动排序。未进入拖动时点按仍
                    // 切换展开（WritingTreeGroupHeader 内部 MouseArea 被这层
                    // 左键 MouseArea 覆盖，所以折叠回调在这里补上）。
                    MouseArea {
                        visible: model.rowType === "volume"
                        anchors.fill: parent
                        // 让出右侧「+ 新章节」按钮的可点区域，保持原行为。
                        anchors.rightMargin: Math.round(Kirigami.Units.gridUnit * 2)
                        acceptedButtons: Qt.LeftButton
                        hoverEnabled: true
                        z: 5
                        cursorShape: Qt.PointingHandCursor
                        onPressed: function(mouse) {
                            if (mouse.button !== Qt.LeftButton) return
                            var p = mapToItem(root, mouse.x, mouse.y)
                            root.dragPointerX = p.x
                            root.dragPointerY = p.y
                            root.beginDragCandidate("volume", model.itemId, model.itemId,
                                                    model.itemTitle || "",
                                                    mouse.x, mouse.y)
                        }
                        onPositionChanged: function(mouse) {
                            if (!pressed) return
                            var p = mapToItem(root, mouse.x, mouse.y)
                            root.dragPointerX = p.x
                            root.dragPointerY = p.y
                            var contentPoint = mapToItem(treeListView.contentItem,
                                                         mouse.x, mouse.y)
                            root.updateDrag(mouse.x, mouse.y, contentPoint.y)
                        }
                        onReleased: root.endDrag()
                        onCanceled: root.resetDragState()
                        onClicked: function(mouse) {
                            if (mouse.button !== Qt.LeftButton) return
                            if (root.dragSuppressClick) return
                            root.toggleVolumeExpanded(model.itemId)
                        }
                    }

                    // ── 章节行 ──
                    Rectangle {
                        id: chapterDelegateBg
                        visible: model.rowType === "chapter"
                        anchors.fill: parent
                        anchors.leftMargin: Kirigami.Units.gridUnit * 2
                        anchors.rightMargin: Kirigami.Units.smallSpacing
                        radius: Kirigami.Units.cornerRadius
                        color: {
                            if (isSelected) return Kirigami.Theme.highlightColor
                            if (chapterHover.containsMouse) return Kirigami.Theme.backgroundColor
                            return "transparent"
                        }

                        property bool isSelected: model.itemId === root.currentChapterId

                        RowLayout {
                            anchors.fill: parent
                            anchors.leftMargin: Kirigami.Units.smallSpacing
                            spacing: Kirigami.Units.smallSpacing

                            Rectangle {
                                Layout.preferredWidth: 6
                                Layout.preferredHeight: 6
                                radius: 3
                                color: chapterDelegateBg.isSelected ? Kirigami.Theme.highlightedTextColor
                                                                    : Kirigami.Theme.disabledTextColor
                                Layout.alignment: Qt.AlignVCenter
                                opacity: 0.6
                            }

                            Label {
                                text: model.itemTitle || ""
                                color: chapterDelegateBg.isSelected ? Kirigami.Theme.highlightedTextColor
                                                                    : Kirigami.Theme.textColor
                                font.family: Kirigami.Theme.defaultFont.family
                                font.pointSize: Kirigami.Theme.defaultFont.pointSize
                                font.bold: chapterDelegateBg.isSelected
                                Layout.fillWidth: true
                                elide: Text.ElideRight
                            }

                            // 章节操作菜单入口（Breeze overflow-menu 图标）
                            ToolButton {
                                z: 10
                                icon.name: "overflow-menu"
                                icon.width: Kirigami.Units.iconSizes.smallMedium
                                icon.height: Kirigami.Units.iconSizes.smallMedium
                                display: AbstractButton.IconOnly
                                flat: true
                                // Issue #912：给「⋯」入口完整的可点区域，不再用偏小的 gridUnit*1.4。
                                implicitWidth: Math.max(Kirigami.Units.gridUnit * 1.8, Kirigami.Units.iconSizes.smallMedium + Kirigami.Units.smallSpacing * 2)
                                implicitHeight: Math.max(Kirigami.Units.gridUnit * 1.8, Kirigami.Units.iconSizes.smallMedium + Kirigami.Units.smallSpacing * 2)
                                Layout.alignment: Qt.AlignVCenter
                                onClicked: {
                                    treeContextMenu.itemType = "chapter"
                                    treeContextMenu.itemId = model.itemId
                                    treeContextMenu.itemTitle = model.itemTitle
                                    treeContextMenu.itemProjectId = model.itemProjectId || ""
                                    treeContextMenu.itemVolumeId = model.itemVolumeId || ""
                                    treeContextMenu.popup(this, 0, height)
                                }
                            }
                        }

                        MouseArea {
                            id: chapterHover
                            anchors.left: parent.left
                            anchors.top: parent.top
                            anchors.bottom: parent.bottom
                            anchors.right: parent.right
                            anchors.rightMargin: Math.round(Kirigami.Units.gridUnit * 2)
                            hoverEnabled: true
                            acceptedButtons: Qt.LeftButton | Qt.RightButton
                            cursorShape: Qt.PointingHandCursor
                            // Issue #1051：左键按住可拖动章节排序；onClicked 只处理
                            // 未进入拖动（未超过阈值）的单击。
                            onPressed: function(mouse) {
                                if (mouse.button !== Qt.LeftButton) return
                                var p = mapToItem(root, mouse.x, mouse.y)
                                root.dragPointerX = p.x
                                root.dragPointerY = p.y
                                root.beginDragCandidate("chapter", model.itemId,
                                                        model.itemVolumeId,
                                                        model.itemTitle || "",
                                                        mouse.x, mouse.y)
                            }
                            onPositionChanged: function(mouse) {
                                if (!pressed) return
                                var p = mapToItem(root, mouse.x, mouse.y)
                                root.dragPointerX = p.x
                                root.dragPointerY = p.y
                                var contentPoint = mapToItem(treeListView.contentItem,
                                                             mouse.x, mouse.y)
                                root.updateDrag(mouse.x, mouse.y, contentPoint.y)
                            }
                            onReleased: root.endDrag()
                            onCanceled: root.resetDragState()
                            onClicked: function(mouse) {
                                if (root.dragSuppressClick) return
                                if (mouse.button === Qt.LeftButton) {
                                    root.openChapter(model.itemProjectId || root.workspaceProjectId, model.itemVolumeId, model.itemId, model.itemTitle)
                                } else if (mouse.button === Qt.RightButton) {
                                    treeContextMenu.itemType = "chapter"
                                    treeContextMenu.itemId = model.itemId
                                    treeContextMenu.itemTitle = model.itemTitle
                                    treeContextMenu.itemProjectId = model.itemProjectId || ""
                                    treeContextMenu.itemVolumeId = model.itemVolumeId || ""
                                    treeContextMenu.popup(chapterHover, mouse.x, mouse.y)
                                }
                            }
                        }

                        // 长按弹出菜单（触屏支持）
                        TapHandler {
                            onLongPressed: {
                                treeContextMenu.itemType = "chapter"
                                treeContextMenu.itemId = model.itemId
                                treeContextMenu.itemTitle = model.itemTitle
                                treeContextMenu.itemProjectId = model.itemProjectId || ""
                                treeContextMenu.itemVolumeId = model.itemVolumeId || ""
                                treeContextMenu.popup(chapterDelegateBg, point.position.x, point.position.y)
                            }
                        }
                    }
                }

                // Issue #1051：拖动目标插入线。显式挂到 ListView 的 contentItem 上，
                // Y 直接用内容坐标，滚动时跟着内容走（Qt 不会自动 parent 非 delegate 子项）。
                Rectangle {
                    id: dropIndicator
                    parent: treeListView.contentItem
                    visible: root.dragActive && root.dropLineY >= 0
                    x: Kirigami.Units.smallSpacing
                    width: Math.max(0, treeListView.width - Kirigami.Units.smallSpacing * 2)
                    height: 2
                    y: root.dropLineY - 1
                    radius: 1
                    color: root.dropTargetValid ? Kirigami.Theme.highlightColor
                                                : Kirigami.Theme.disabledTextColor
                    opacity: root.dropTargetValid ? 1.0 : 0.6
                    z: 100
                }
            }
        }

        // "+" button for project (create volume)
        ToolButton {
            Layout.fillWidth: true
            Layout.preferredHeight: Math.max(root.volumeRowHeight, implicitHeight)
            Layout.leftMargin: Kirigami.Units.smallSpacing
            Layout.rightMargin: Kirigami.Units.smallSpacing
            flat: true
            text: qsTr("新卷")
            icon.name: "list-add"
            icon.width: Kirigami.Units.iconSizes.smallMedium
            icon.height: Kirigami.Units.iconSizes.smallMedium
            display: AbstractButton.TextBesideIcon
            onClicked: root.createVolumeRequested(root.workspaceProjectId)
        }

        // Tree context menu
        Menu {
            id: treeContextMenu
            property string itemType: ""
            property string itemId: ""
            property string itemTitle: ""
            property string itemProjectId: ""
            property string itemVolumeId: ""

            MenuItem {
                id: createVolumeMenuItem
                text: qsTr("新建卷")
                icon.name: "list-add"
                visible: treeContextMenu.itemType === "project"
                onTriggered: root.createVolumeRequested(treeContextMenu.itemProjectId || root.workspaceProjectId)
            }
            MenuItem {
                id: createChapterMenuItem
                text: qsTr("新建章节")
                icon.name: "list-add"
                visible: treeContextMenu.itemType === "volume"
                onTriggered: root.createChapterRequested(treeContextMenu.itemProjectId, treeContextMenu.itemId)
            }
            MenuSeparator {
                visible: treeContextMenu.itemType === "project" || treeContextMenu.itemType === "volume" || treeContextMenu.itemType === "chapter"
            }
            MenuItem {
                id: renameMenuItem
                text: qsTr("重命名")
                icon.name: "edit-rename"
                visible: treeContextMenu.itemType === "project" || treeContextMenu.itemType === "volume" || treeContextMenu.itemType === "chapter"
                onTriggered: root.renameItemRequested({
                    type: treeContextMenu.itemType,
                    id: treeContextMenu.itemId,
                    projectId: treeContextMenu.itemProjectId,
                    volumeId: treeContextMenu.itemVolumeId,
                    title: treeContextMenu.itemTitle
                })
            }
            MenuItem {
                id: deleteMenuItem
                text: qsTr("删除")
                icon.name: "edit-delete"
                visible: treeContextMenu.itemType === "project" || treeContextMenu.itemType === "volume" || treeContextMenu.itemType === "chapter"
                onTriggered: root.deleteItemRequested({
                    type: treeContextMenu.itemType,
                    id: treeContextMenu.itemId,
                    projectId: treeContextMenu.itemProjectId,
                    volumeId: treeContextMenu.itemVolumeId,
                    title: treeContextMenu.itemTitle
                })
            }
        }
    }

    // Issue #1051：浮动拖拽预览。拖动期间真实 delegate 留在原位，
    // 只有这个浮层跟随指针，标明正在拖哪一项。
    Rectangle {
        id: dragPreview
        visible: root.dragActive
        z: 1000
        width: Math.max(0, Math.min(root.width - Kirigami.Units.smallSpacing,
                                    Math.max(Kirigami.Units.gridUnit * 8,
                                             dragPreviewLabel.implicitWidth + Kirigami.Units.gridUnit)))
        height: root.volumeRowHeight
        radius: Kirigami.Units.cornerRadius
        color: Kirigami.Theme.backgroundColor
        border.width: 1
        border.color: Kirigami.Theme.highlightColor
        opacity: 0.94
        x: Math.max(0, Math.min(root.width - width,
                                root.dragPointerX + Kirigami.Units.smallSpacing))
        y: Math.max(0, Math.min(root.height - height,
                                root.dragPointerY + Kirigami.Units.smallSpacing))

        Label {
            id: dragPreviewLabel
            anchors.fill: parent
            anchors.leftMargin: Kirigami.Units.smallSpacing
            anchors.rightMargin: Kirigami.Units.smallSpacing
            verticalAlignment: Text.AlignVCenter
            text: root.dragSourceTitle
            color: Kirigami.Theme.textColor
            font.family: Kirigami.Theme.defaultFont.family
            font.pointSize: Kirigami.Theme.defaultFont.pointSize
            font.bold: true
            elide: Text.ElideRight
        }
    }
}
