// =============================================================================
// WritingChapterNavigation.qml — 章节导航（作品名 / 卷章树 / 新卷 / 章纲 / 右键菜单）
// =============================================================================
//
// 层级：Linux_qt UI 层（QML UI 组件）
// 职责：渲染左树的全部章节导航内容，供 WritingWorkspace 在 Workbench 和
//   SinglePane 两种壳模式下复用，避免两套结构各画一份。
// 边界：纯展示 + 信号回调。展开态、当前章节、树数据都由调用方持有，
//   组件自己不存业务状态，也不查后端。
//
// Issue #833 复核：从 WritingWorkspace.qml 的 sidebarRect 抽出。
//   - Workbench：作为 RowLayout 子项，宽度只吃 Core 的 ChapterNavigation bounds。
//   - SinglePane：作为覆盖在正文上的抽屉/浮层打开，不参与 RowLayout 宽度分配。
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Rectangle {
    id: root

    required property var dt
    // flat tree 数组（与 WritingWorkspace.tree 同源）。
    property var tree: []
    // 当前作品 id，用于 WritingTreeController 过滤。
    property string workspaceProjectId: ""
    // 当前作品标题（左树顶部分组头）。
    property string workspaceProjectTitle: ""
    // 作品名分组头是否折叠（true = 只留分组头）。
    property bool projectGroupCollapsed: false
    // 章纲分组头是否展开。
    property bool outlineGroupExpanded: false
    // 当前选中章节 id（用于列表项高亮）。
    property string currentChapterId: ""
    // Issue #835：当前章节章纲文本。由 WritingWorkspace 从 editorController.chapterNote
    // 透传进来，组件不查后端、不存第二份。编辑后发 chapterNoteChanged 交回 backend。
    property string currentChapterNote: ""

    signal openChapter(string projectId, string volumeId, string chapterId, string chapterTitle)
    signal createVolumeRequested(string projectId)
    signal createChapterRequested(string projectId, string volumeId)
    signal renameItemRequested(var itemData)
    signal deleteItemRequested(var itemData)
    signal toggleProjectGroup()
    signal toggleOutlineGroup()
    // Issue #835：章纲文本编辑完成（失焦）时发出，由 WritingWorkspace 调
    // editorBackendRef.update_chapter_note 写回 Core。
    signal chapterNoteChanged(string note)

    color: dt.sidebar
    border.color: dt.border
    border.width: 1

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

    // Issue #835 评论 6019713847: 章纲保存失败时，Core 仍是 note 唯一事实来源，
    // 把编辑框恢复成 currentChapterNote（即 editorController.chapterNote 透传值），
    // 不让界面显示成"已保存"。
    function restoreChapterNoteFromSource() {
        outlineTextArea.text = root.currentChapterNote
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

        // ── Issue #829：章节树顶部分组头「作品名 ∨」──
        // 手稿的左树是「作品名 ∨ → 卷 ∨ → 章节列表 → 章纲 ∨」四段结构，
        // 作品名本身是一个可折叠分组头，不是一条普通列表项。
        // 折叠状态只是端侧 UI 状态，不进 Core、不进同步。
        WritingTreeGroupHeader {
            Layout.fillWidth: true
            dt: root.dt
            title: root.workspaceProjectTitle
            expanded: !root.projectGroupCollapsed
            // 作品名是当前写作上下文，折叠它等于把整棵树收起来，
            // 不提供「新建」——新建入口在下面的「+ 新卷」。
            showAddButton: false
            onToggleExpanded: root.toggleProjectGroup()
        }

        // Tree list
        // Issue #835：按层级渲染 —— 卷是可折叠分组头（WritingTreeGroupHeader），
        // 卷展开时紧接其章节行。取代旧 flat ListView 把 volume 当 36px 列表行的画法。
        // 「作品名 ∨」折叠时整棵子树收起，只留顶部分组头。
        ScrollView {
                Layout.fillWidth: true
                Layout.fillHeight: true
                visible: !root.projectGroupCollapsed
                clip: true
                // Issue #782 评论 5855709706: 桌面鼠标左键不能按住空白处拖页面。
                Component.onCompleted: {
                    if (contentItem) contentItem.acceptedButtons = Qt.NoButton
                }

            ListView {
                id: treeListView
                model: treeModel
                delegate: Item {
                    width: treeListView.width
                    height: model.rowType === "volume" ? 36 : 32

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
                        anchors.leftMargin: dt.sp32
                        anchors.rightMargin: dt.sp8
                        radius: dt.radiusPill
                        color: {
                            if (isSelected) return dt.primaryContainer
                            if (chapterHover.containsMouse) return dt.surfaceVariant
                            return "transparent"
                        }

                        property bool isSelected: model.itemId === root.currentChapterId

                        RowLayout {
                            anchors.fill: parent
                            anchors.leftMargin: dt.sp8
                            spacing: dt.sp6

                            Rectangle {
                                width: 6; height: 6
                                radius: 3
                                color: chapterDelegateBg.isSelected ? dt.selectedText : dt.textSecondary
                                Layout.alignment: Qt.AlignVCenter
                                opacity: 0.6
                            }

                            AppText {
                                dt: root.dt
                                text: model.itemTitle || ""
                                color: chapterDelegateBg.isSelected ? dt.onPrimaryContainer : dt.textPrimary
                                font.pointSize: dt.labelPt
                                font.family: dt.fontFamily
                                font.weight: chapterDelegateBg.isSelected ? Font.DemiBold : Font.Normal
                                Layout.fillWidth: true
                                elide: Text.ElideRight
                            }

                            // "⋯" menu button
                            Rectangle {
                                z: 10
                                width: 24; height: 24
                                radius: 12
                                color: chapterMenuBtnHover.containsMouse ? dt.surfaceVariant : "transparent"
                                Layout.alignment: Qt.AlignVCenter

                                AppText {
                                    dt: root.dt
                                    anchors.centerIn: parent
                                    text: "⋯"
                                    color: dt.textSecondary
                                    font.pointSize: dt.fontMdPt
                                }

                                MouseArea {
                                    id: chapterMenuBtnHover
                                    anchors.fill: parent
                                    hoverEnabled: true
                                    cursorShape: Qt.PointingHandCursor
                                    onClicked: {
                                        treeContextMenu.itemType = "chapter"
                                        treeContextMenu.itemId = model.itemId
                                        treeContextMenu.itemTitle = model.itemTitle
                                        treeContextMenu.itemProjectId = model.itemProjectId || ""
                                        treeContextMenu.itemVolumeId = model.itemVolumeId || ""
                                        treeContextMenu.popup(chapterMenuBtnHover, 0, chapterMenuBtnHover.height)
                                    }
                                }
                            }
                        }

                        MouseArea {
                            id: chapterHover
                            anchors.left: parent.left
                            anchors.top: parent.top
                            anchors.bottom: parent.bottom
                            anchors.right: parent.right
                            anchors.rightMargin: 32
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
        // Issue #835：作品名折叠时整棵树收起，「+ 新卷」也一并隐藏。
        Rectangle {
            Layout.fillWidth: true
            Layout.preferredHeight: 36
            Layout.leftMargin: dt.sp8
            Layout.rightMargin: dt.sp8
            visible: !root.projectGroupCollapsed
            radius: dt.radiusPill
            color: addVolumeHover.containsMouse ? dt.primaryContainer : "transparent"

            RowLayout {
                anchors.centerIn: parent
                spacing: dt.sp4
                AppText {
                    dt: root.dt
                    text: "+"
                    color: dt.primary
                    font.pointSize: dt.fontMdPt
                    font.weight: Font.Bold
                }
                AppText {
                    dt: root.dt
                    text: qsTr("新卷")
                    color: dt.primary
                    font.pointSize: dt.labelPt
                    font.family: dt.fontFamily
                }
            }

            MouseArea {
                id: addVolumeHover
                anchors.fill: parent
                hoverEnabled: true
                cursorShape: Qt.PointingHandCursor
                onClicked: root.createVolumeRequested(root.workspaceProjectId)
            }
        }

        // ── Issue #829/#835：章节树底部分组头「章纲 ∨」──
        // 手稿把它放在章节列表下面，作为左树的最后一个分组。
        // 展开后显示当前章节的章纲（chapter.note）可编辑多行文本；
        // 没选章节时显示「请选择章节」空态。章纲数据唯一来源是 Core
        // （currentChapterNote 由 WritingWorkspace 从 editorController.chapterNote 透传），
        // 编辑完成后发 chapterNoteChanged 交回 backend，不在 QML 存第二份。
        WritingTreeGroupHeader {
            Layout.fillWidth: true
            dt: root.dt
            title: qsTr("章纲")
            expanded: root.outlineGroupExpanded
            onToggleExpanded: root.toggleOutlineGroup()
        }

        // 章纲展开内容：空态 / 可编辑多行文本。
        Item {
            Layout.fillWidth: true
            Layout.preferredHeight: root.outlineGroupExpanded ? (root.currentChapterId ? 120 : 40) : 0
            visible: root.outlineGroupExpanded
            clip: true

            // 没选章节：空态提示
            AppText {
                visible: root.currentChapterId === ""
                anchors.centerIn: parent
                dt: root.dt
                text: qsTr("请选择章节")
                color: dt.textSecondary
                font.pointSize: dt.labelPt
                font.family: dt.fontFamily
            }

            // 已选章节：可编辑多行文本
            // text 不用绑定（用户输入会破坏绑定），改用 Connections 在非聚焦时
            // 同步 currentChapterNote，避免切章后显示旧 note 或打断用户输入。
            TextArea {
                id: outlineTextArea
                visible: root.currentChapterId !== ""
                anchors.fill: parent
                anchors.margins: root.dt.sp8
                text: ""
                wrapMode: TextArea.Wrap
                color: root.dt.textPrimary
                font.pointSize: root.dt.labelPt
                font.family: root.dt.fontFamily
                background: Rectangle { color: "transparent" }
                Component.onCompleted: outlineTextArea.text = root.currentChapterNote
                onEditingFinished: root.chapterNoteChanged(outlineTextArea.text)

                Connections {
                    target: root
                    function onCurrentChapterNoteChanged() {
                        if (!outlineTextArea.activeFocus) outlineTextArea.text = root.currentChapterNote
                    }
                    function onCurrentChapterIdChanged() {
                        if (!outlineTextArea.activeFocus) outlineTextArea.text = root.currentChapterNote
                    }
                }
            }
        }

        // Tree context menu
        Menu {
            id: treeContextMenu
            property string itemType: ""
            property string itemId: ""
            property string itemTitle: ""
            property string itemProjectId: ""
            property string itemVolumeId: ""
            background: Rectangle {
                color: dt.surface
                border.color: dt.border
                radius: dt.radiusMd
                border.width: 1
            }

            MenuItem {
                id: createVolumeMenuItem
                text: qsTr("新建卷")
                visible: treeContextMenu.itemType === "project"
                contentItem: AppText {
                    dt: root.dt
                    text: createVolumeMenuItem.text
                    color: dt.textPrimary
                    font.pointSize: dt.labelPt
                    font.family: dt.fontFamily
                    verticalAlignment: Text.AlignVCenter
                }
                background: Rectangle {
                    color: createVolumeMenuItem.highlighted ? dt.surfaceVariant : "transparent"
                }
                onTriggered: root.createVolumeRequested(treeContextMenu.itemProjectId || root.workspaceProjectId)
            }
            MenuItem {
                id: createChapterMenuItem
                text: qsTr("新建章节")
                visible: treeContextMenu.itemType === "volume"
                contentItem: AppText {
                    dt: root.dt
                    text: createChapterMenuItem.text
                    color: dt.textPrimary
                    font.pointSize: dt.labelPt
                    font.family: dt.fontFamily
                    verticalAlignment: Text.AlignVCenter
                }
                background: Rectangle {
                    color: createChapterMenuItem.highlighted ? dt.surfaceVariant : "transparent"
                }
                onTriggered: root.createChapterRequested(treeContextMenu.itemProjectId, treeContextMenu.itemId)
            }
            MenuSeparator {
                visible: treeContextMenu.itemType === "project" || treeContextMenu.itemType === "volume" || treeContextMenu.itemType === "chapter"
            }
            MenuItem {
                id: renameMenuItem
                text: qsTr("重命名")
                visible: treeContextMenu.itemType === "project" || treeContextMenu.itemType === "volume" || treeContextMenu.itemType === "chapter"
                contentItem: AppText {
                    dt: root.dt
                    text: renameMenuItem.text
                    color: dt.textPrimary
                    font.pointSize: dt.labelPt
                    font.family: dt.fontFamily
                    verticalAlignment: Text.AlignVCenter
                }
                background: Rectangle {
                    color: renameMenuItem.highlighted ? dt.surfaceVariant : "transparent"
                }
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
                visible: treeContextMenu.itemType === "project" || treeContextMenu.itemType === "volume" || treeContextMenu.itemType === "chapter"
                contentItem: AppText {
                    dt: root.dt
                    text: deleteMenuItem.text
                    color: dt.error
                    font.pointSize: dt.labelPt
                    font.family: dt.fontFamily
                    verticalAlignment: Text.AlignVCenter
                }
                background: Rectangle {
                    color: deleteMenuItem.highlighted ? dt.surfaceVariant : "transparent"
                }
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
