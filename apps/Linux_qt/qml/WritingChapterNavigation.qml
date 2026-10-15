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

    signal openChapter(string projectId, string volumeId, string chapterId, string chapterTitle)
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
                font: Kirigami.Theme.defaultFont
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
                                font: Kirigami.Theme.defaultFont
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
                            onClicked: function(mouse) {
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
}
