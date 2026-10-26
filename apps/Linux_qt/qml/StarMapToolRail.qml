import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

Rectangle {
    id: root
    required property var dt
    property real railWidth: 56
    property string selectedTool: ""
    property bool drawerOpen: false
    property bool aiAvailable: false
    property bool aiEnabled: false

    signal toolRequested(string toolKey)
    signal drawerToggled()

    color: Kirigami.Theme.alternateBackgroundColor

    ColumnLayout {
        anchors.fill: parent
        anchors.topMargin: Kirigami.Units.smallSpacing
        anchors.bottomMargin: Kirigami.Units.smallSpacing
        spacing: Kirigami.Units.smallSpacing

        Repeater {
            model: [
                { key: "ai", label: qsTr("AI"), icon: "tools-wizard", enabled: root.aiAvailable && root.aiEnabled },
                { key: "tags", label: qsTr("标签"), icon: "tag", enabled: true }
            ]
            ItemDelegate {
                required property var modelData
                Layout.alignment: Qt.AlignHCenter
                Layout.preferredWidth: Math.round(root.railWidth - Kirigami.Units.smallSpacing)
                Layout.preferredHeight: Math.max(Kirigami.Units.gridUnit * 2.4, implicitHeight)
                text: modelData.label
                icon.name: modelData.icon
                icon.width: Kirigami.Units.iconSizes.smallMedium
                icon.height: Kirigami.Units.iconSizes.smallMedium
                display: AbstractButton.TextUnderIcon
                highlighted: root.selectedTool === modelData.key && root.drawerOpen
                enabled: true
                opacity: modelData.enabled ? 1.0 : 0.65
                ToolTip.visible: hovered
                ToolTip.text: modelData.key === "ai"
                    ? (root.aiAvailable ? (root.aiEnabled ? qsTr("AI 已启用") : qsTr("AI 未启用")) : qsTr("AI 能力不可用"))
                    : modelData.label
                onClicked: root.toolRequested(modelData.key)
            }
        }

        Item { Layout.fillHeight: true }

        ToolButton {
            Layout.alignment: Qt.AlignHCenter
            Layout.preferredWidth: Math.round(root.railWidth - Kirigami.Units.smallSpacing)
            icon.name: root.drawerOpen ? "go-next" : "go-previous"
            display: AbstractButton.IconOnly
            ToolTip.visible: hovered
            ToolTip.text: root.drawerOpen ? qsTr("收起工具面板") : qsTr("展开工具面板")
            onClicked: root.drawerToggled()
        }
    }
}
