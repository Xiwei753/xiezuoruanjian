// =============================================================================
// ProjectHomePage.qml — 作品首页
// =============================================================================
//
// 层级：Linux_qt UI 层（QML 页面）
// 职责：作品卡片列表、项目字数统计、新建/重命名/删除操作
// 约束：
//   - 纯展示层，业务逻辑通过 signal 传递给 main.qml
//   - 不直接操作文件系统或 Core 层
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Rectangle {
    id: root
    property var dt: null
    property var backendRef: null
    property var appState: ({})
    property var tree: []

    signal openProject(string projectId)
    signal createProject()
    signal renameProjectRequested(string projectId, string title)
    signal deleteProjectRequested(string projectId, string title)

    color: dt.bg

    function getProjects() {
        var projects = []
        if (!tree) return projects
        for (var i = 0; i < tree.length; i++) {
            if (tree[i].type === "project") projects.push(tree[i])
        }
        return projects
    }

    function getProjectWordCount(projectId) {
        var count = 0
        if (!tree) return count
        for (var i = 0; i < tree.length; i++) {
            var item = tree[i]
            if (item.type === "chapter" && item.projectId === projectId) count += (item.wordCount || 0)
        }
        return count
    }

    function getTodayInput(projectId) {
        if (!backendRef) return 0
        try {
            var today = new Date()
            var dateStr = today.getFullYear() + "-" + String(today.getMonth() + 1).padStart(2, "0") + "-" + String(today.getDate()).padStart(2, "0")
            var summary = backendRef.get_writing_stats_summary_object(dateStr, dateStr)
            if (summary && summary.per_project && summary.per_project[projectId]) return summary.per_project[projectId].human_typed_chars || 0
        } catch (e) {}
        return 0
    }

    CardCollectionPage {
        anchors.fill: parent
        dt: root.dt
        title: qsTr("作品")
        subtitle: projectModel.count > 0 ? qsTr("%1 部作品").arg(projectModel.count) : qsTr("开始你的创作之旅")
        actionText: qsTr("+ 新建作品")
        model: projectModel
        cardHeight: 184
        minCardWidth: 280
        emptyIcon: ""
        emptyTitle: qsTr("暂无作品")
        emptySubtitle: qsTr("点击「新建作品」开始创作")
        onActionClicked: root.createProject()

        delegate: ProjectCard {
            dt: root.dt
            width: GridView.view.gridRoot.cardWidth
            height: GridView.view.gridRoot.cardHeight
            projectId: model.projectId
            title: model.projectTitle
            wordCount: model.projectWordCount
            todayInput: model.projectTodayInput
            lastEdited: model.projectLastEdited
            syncStatus: model.projectSyncStatus
            accentColor: model.projectAccent
            onClicked: root.openProject(model.projectId)
            onRightClicked: {
                projectContextMenu.projectId = model.projectId
                projectContextMenu.projectTitle = model.projectTitle
                projectContextMenu.popup()
            }
        }
    }

    Menu {
        id: projectContextMenu
        property string projectId: ""
        property string projectTitle: ""
        MenuItem { text: qsTr("打开"); onTriggered: root.openProject(projectContextMenu.projectId) }
        MenuSeparator {}
        MenuItem { text: qsTr("重命名"); onTriggered: { renameProjectDialog.projectId = projectContextMenu.projectId; renameProjectDialog.currentTitle = projectContextMenu.projectTitle; renameProjectDialog.open() } }
        MenuItem { text: qsTr("删除"); onTriggered: root.deleteProjectRequested(projectContextMenu.projectId, projectContextMenu.projectTitle) }
    }

    Dialog {
        id: renameProjectDialog
        property string projectId: ""
        property string currentTitle: ""
        modal: true
        width: 360
        height: 208
        anchors.centerIn: Overlay.overlay
        background: Rectangle { color: dt.surface; border.color: dt.border; radius: dt.radiusXl; border.width: 1 }
        header: null
        ColumnLayout {
            anchors.fill: parent
            anchors.margins: dt.sp24
            spacing: dt.sp12

            AppText {
                dt: root.dt
                text: qsTr("重命名作品")
                color: dt.onSurface
                font.pixelSize: dt.subtitle
                font.family: dt.fontFamily
                font.weight: Font.DemiBold
            }
            AppTextField {
                id: renameField
                Layout.fillWidth: true
                dt: dt
                text: renameProjectDialog.currentTitle
                placeholderText: qsTr("作品名称")
                onAccepted: renameConfirmButton.clicked()
            }
            RowLayout {
                Layout.fillWidth: true
                Item { Layout.fillWidth: true }
                AppButton {
                    text: qsTr("取消")
                    dt: dt
                    variant: "text"
                    onClicked: renameProjectDialog.close()
                }
                AppButton {
                    id: renameConfirmButton
                    text: qsTr("确定")
                    dt: dt
                    variant: "primary"
                    onClicked: {
                        var t = renameField.text.trim();
                        if (t === "") return;
                        root.renameProjectRequested(renameProjectDialog.projectId, t);
                        renameProjectDialog.close();
                    }
                }
            }
        }
        onOpened: {
            renameField.text = renameProjectDialog.currentTitle;
            renameField.forceActiveFocus();
        }
    }

    ListModel { id: projectModel }

    function refreshProjects() {
        projectModel.clear()
        var projects = getProjects()
        var accentColors = dt.projectAccentColors
        for (var i = 0; i < projects.length; i++) {
            var p = projects[i]
            projectModel.append({
                projectId: p.id,
                projectTitle: p.title || qsTr("未命名作品"),
                projectWordCount: getProjectWordCount(p.id),
                projectTodayInput: getTodayInput(p.id),
                projectLastEdited: p.updatedAt || "",
                projectSyncStatus: "none",
                projectAccent: accentColors[i % accentColors.length]
            })
        }
    }

    Component.onCompleted: refreshProjects()
    onTreeChanged: refreshProjects()
}
