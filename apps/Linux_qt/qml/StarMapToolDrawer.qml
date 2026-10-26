import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

Rectangle {
    id: root
    required property var dt
    property string selectedTool: "tags"
    property bool isOpen: false
    property bool aiAvailable: false
    property bool aiEnabled: false
    property var annotationBackendRef: null
    property var searchBackendRef: null
    property string projectId: ""
    property var targetRef: null
    property string targetLabel: ""

    signal closeRequested()
    signal targetActivated(var target)
    signal linkPickerRequested(var sourceRef, var link)

    function refresh() {
        tagsPanel.refresh()
    }

    visible: isOpen
    clip: true
    color: Kirigami.Theme.alternateBackgroundColor

    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        RowLayout {
            Layout.fillWidth: true
            Layout.preferredHeight: Kirigami.Units.gridUnit * 2.4
            anchors.leftMargin: Kirigami.Units.smallSpacing
            anchors.rightMargin: Kirigami.Units.smallSpacing
            Label {
                Layout.fillWidth: true
                text: root.selectedTool === "ai" ? qsTr("AI") : qsTr("标签与引用")
                font.bold: true
                elide: Text.ElideRight
            }
            ToolButton {
                icon.name: "window-close"
                display: AbstractButton.IconOnly
                onClicked: root.closeRequested()
            }
        }
        Rectangle { Layout.fillWidth: true; height: 1; color: Kirigami.Theme.disabledTextColor; opacity: 0.35 }

        TagsPanel {
            id: tagsPanel
            visible: root.selectedTool === "tags"
            Layout.fillWidth: true
            Layout.fillHeight: true
            dt: root.dt
            annotationBackendRef: root.annotationBackendRef
            searchBackendRef: root.searchBackendRef
            projectId: root.projectId
            targetRef: root.targetRef
            targetLabel: root.targetLabel
            onTargetActivated: function(target) { root.targetActivated(target) }
            onLinkPickerRequested: function(sourceRef, link) { root.linkPickerRequested(sourceRef, link) }
        }

        ColumnLayout {
            visible: root.selectedTool === "ai"
            Layout.fillWidth: true
            Layout.fillHeight: true
            Layout.margins: Kirigami.Units.largeSpacing
            spacing: Kirigami.Units.smallSpacing
            Label { text: qsTr("AI 能力状态"); font.bold: true }
            Label {
                Layout.fillWidth: true
                text: !root.aiAvailable ? qsTr("当前环境未提供 AI 能力")
                    : root.aiEnabled ? qsTr("AI 能力已在设置中启用")
                    : qsTr("AI 能力尚未启用")
                wrapMode: Text.Wrap
                color: Kirigami.Theme.disabledTextColor
            }
        }
    }
}
