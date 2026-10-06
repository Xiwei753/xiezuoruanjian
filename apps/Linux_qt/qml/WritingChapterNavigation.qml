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

    signal openChapter(string projectId, string volumeId, string chapterId, string chapterTitle)
    signal createVolumeRequested(string projectId)
    signal createChapterRequested(string projectId, string volumeId)
    signal renameItemRequested(var itemData)
    signal deleteItemRequested(var itemData)
    signal toggleProjectGroup()
    signal toggleOutlineGroup()

    color: dt.sidebar
    border.color: dt.border
    border.width: 1

    WritingTreeController {
        id: writingTree
        tree: root.tree
        projectId: root.workspaceProjectId
        onItemsChanged: root.populateTreeModel()
    }

    ListModel {
        id: treeModel
    }

    function populateTreeModel() {
        treeModel.clear();
        var items = writingTree.items || [];
        for (var i = 0; i < items.length; i++) {
            treeModel.append({
                "itemId": items[i].id || "",
                "itemType": items[i].type || "",
                "itemTitle": items[i].title || "",
                "itemProjectId": items[i].projectId || "",
                "itemVolumeId": items[i].volumeId || ""
            });
        }
    }

    Component.onCompleted: root.populateTreeModel()

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
        // Issue #829：「作品名 ∨」折叠时整棵子树收起，只留顶部分组头。
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
                    height: model.itemType === "volume" ? 36 : 32

                    Rectangle {
                        id: delegateBg
                        anchors.fill: parent
                        anchors.leftMargin: dt.sp8
                        anchors.rightMargin: dt.sp8
                        radius: dt.radiusPill
                        color: {
                            if (isSelected) return dt.primaryContainer;
                            if (delegateHover.containsMouse) return dt.surfaceVariant;
                            return "transparent";
                        }

                        property bool isSelected: model.itemId === root.currentChapterId

                        RowLayout {
                            anchors.fill: parent
                            anchors.leftMargin: model.itemType === "chapter" ? dt.sp32 : dt.sp12
                            spacing: dt.sp6

                            Rectangle {
                                width: 6; height: 6
                                radius: model.itemType === "volume" ? 0 : 3
                                color: delegateBg.isSelected ? dt.selectedText : dt.textSecondary
                                Layout.alignment: Qt.AlignVCenter
                                opacity: 0.6
                            }

                            AppText {
                                dt: root.dt
                                text: model.itemTitle || ""
                                color: {
                                    if (delegateBg.isSelected) return dt.onPrimaryContainer;
                                    return dt.textPrimary;
                                }
                                font.pointSize: dt.labelPt
                                font.family: dt.fontFamily
                                font.weight: delegateBg.isSelected ? Font.DemiBold : Font.Normal
                                Layout.fillWidth: true
                                elide: Text.ElideRight
                            }

                            // "⋯" menu button — visible for both volume and chapter
                            Rectangle {
                                z: 10
                                width: 24; height: 24
                                radius: 12
                                color: menuBtnHover.containsMouse ? dt.surfaceVariant : "transparent"
                                Layout.alignment: Qt.AlignVCenter

                                AppText {
                                    dt: root.dt
                                    anchors.centerIn: parent
                                    text: "⋯"
                                    color: dt.textSecondary
                                    font.pointSize: dt.fontMdPt
                                }

                                MouseArea {
                                    id: menuBtnHover
                                    anchors.fill: parent
                                    hoverEnabled: true
                                    cursorShape: Qt.PointingHandCursor
                                    onClicked: {
                                        treeContextMenu.itemType = model.itemType;
                                        treeContextMenu.itemId = model.itemId;
                                        treeContextMenu.itemTitle = model.itemTitle;
                                        treeContextMenu.itemProjectId = model.itemProjectId || "";
                                        treeContextMenu.itemVolumeId = model.itemVolumeId || "";
                                        treeContextMenu.popup(menuBtnHover, 0, menuBtnHover.height);
                                    }
                                }
                            }
                        }

                        MouseArea {
                            id: delegateHover
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
                                    if (model.itemType === "chapter") {
                                         root.openChapter(model.itemProjectId || root.workspaceProjectId, model.itemVolumeId, model.itemId, model.itemTitle);
                                    }
                                } else if (mouse.button === Qt.RightButton) {
                                    treeContextMenu.itemType = model.itemType;
                                    treeContextMenu.itemId = model.itemId;
                                    treeContextMenu.itemTitle = model.itemTitle;
                                    treeContextMenu.itemProjectId = model.itemProjectId || "";
                                    treeContextMenu.itemVolumeId = model.itemVolumeId || "";
                                    treeContextMenu.popup(delegateHover, mouse.x, mouse.y);
                                }
                            }
                        }

                        // 长按弹出菜单（触屏支持）
                        TapHandler {
                            onLongPressed: {
                                treeContextMenu.itemType = model.itemType;
                                treeContextMenu.itemId = model.itemId;
                                treeContextMenu.itemTitle = model.itemTitle;
                                treeContextMenu.itemProjectId = model.itemProjectId || "";
                                treeContextMenu.itemVolumeId = model.itemVolumeId || "";
                                treeContextMenu.popup(delegateBg, point.position.x, point.position.y);
                            }
                        }

                        // "+" button for volumes (create chapter)
                        Rectangle {
                            visible: model.itemType === "volume"
                            width: 20; height: 20
                            radius: 10
                            color: addChapterHover.containsMouse ? dt.primaryContainer : "transparent"
                            anchors {
                                right: parent.right
                                rightMargin: dt.sp8
                            }
                            anchors.verticalCenter: parent.verticalCenter

                            AppText {
                                dt: root.dt
                                anchors.centerIn: parent
                                text: "+"
                                color: dt.primary
                                font.pointSize: dt.fontSmPt
                                font.weight: Font.Bold
                            }

                            MouseArea {
                                id: addChapterHover
                                anchors.fill: parent
                                hoverEnabled: true
                                cursorShape: Qt.PointingHandCursor
                                onClicked: root.createChapterRequested(model.itemProjectId || "", model.itemId)
                            }
                        }
                    }
                }
            }
        }

        // "+" button for project (create volume)
        Rectangle {
            Layout.fillWidth: true
            Layout.preferredHeight: 36
            Layout.leftMargin: dt.sp8
            Layout.rightMargin: dt.sp8
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

        // ── Issue #829：章节树底部分组头「章纲 ∨」──
        // 手稿把它放在章节列表下面，作为左树的最后一个分组。
        // 这一轮只落分组头和展开/收起交互：章纲内容（Core 的
        // chapter.note）还没接上，展开区域留空，不摆假数据。
        WritingTreeGroupHeader {
            Layout.fillWidth: true
            dt: root.dt
            title: qsTr("章纲")
            expanded: root.outlineGroupExpanded
            onToggleExpanded: root.toggleOutlineGroup()
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
