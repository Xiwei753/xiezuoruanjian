// =============================================================================
// StarMapPreviewPage.qml — 星图预览页面
// =============================================================================
//
// 层级：Linux_qt UI 层（QML 页面）
// 职责：在右侧抽屉中展示星图的缩略预览
// 约束：
//   - 纯展示组件，数据通过 starmapBackendRef 获取
//   - 点击后跳转到完整的星图编辑页面
// Issue #900：视觉迁移到 KDE 官方组件体系（Kirigami.Theme + Breeze 图标），
// 不再使用 AppText 等自制 M3 皮肤组件。
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

Rectangle {
    id: root
    required property var dt
    property var starmapBackendRef: null
    property var appState: ({})

    color: Kirigami.Theme.backgroundColor

    ColumnLayout {
        anchors.centerIn: parent
        spacing: Kirigami.Units.largeSpacing
        width: Math.min(parent.width - 80, 560)

        // Constellation icon
        Kirigami.ShadowedRectangle {
            Layout.alignment: Qt.AlignHCenter
            Layout.preferredWidth: 80
            Layout.preferredHeight: 80
            radius: width / 2
            color: Kirigami.Theme.highlightColor

            Kirigami.Icon {
                anchors.centerIn: parent
                width: 44
                height: 44
                source: "starred"
                color: Kirigami.Theme.highlightedTextColor
            }
        }

        Kirigami.Heading {
            level: 2
            text: qsTr("星图")
            color: Kirigami.Theme.textColor
            Layout.alignment: Qt.AlignHCenter
        }

        Label {
            text: qsTr("构建你的创作宇宙，可视化人物关系与故事脉络")
            color: Kirigami.Theme.disabledTextColor
            horizontalAlignment: Text.AlignHCenter
            Layout.fillWidth: true
            wrapMode: Text.Wrap
        }

        // Feature cards
        Repeater {
            model: [
                { label: qsTr("作品宇宙"), desc: qsTr("整体世界观与核心设定"), icon: "globe" },
                { label: qsTr("人物关系"), desc: qsTr("角色之间的关联与冲突"), icon: "system-users" },
                { label: qsTr("地点"), desc: qsTr("故事发生的场景与空间"), icon: "mark-location" },
                { label: qsTr("事件"), desc: qsTr("推动剧情的关键事件链"), icon: "view-calendar" },
                { label: qsTr("伏笔"), desc: qsTr("埋设与回收的叙事线索"), icon: "flag" }
            ]

            Kirigami.ShadowedRectangle {
                Layout.fillWidth: true
                Layout.preferredHeight: 64
                radius: Kirigami.Units.cornerRadius
                color: Kirigami.Theme.alternateBackgroundColor

                RowLayout {
                    anchors.fill: parent
                    anchors.margins: Kirigami.Units.largeSpacing
                    spacing: Kirigami.Units.largeSpacing

                    Kirigami.ShadowedRectangle {
                        Layout.preferredWidth: 40
                        Layout.preferredHeight: 40
                        radius: Kirigami.Units.cornerRadius
                        color: Kirigami.Theme.highlightColor

                        Kirigami.Icon {
                            anchors.centerIn: parent
                            width: 22
                            height: 22
                            source: modelData.icon
                            color: Kirigami.Theme.highlightedTextColor
                        }
                    }

                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 2
                        Label {
                            text: modelData.label
                            color: Kirigami.Theme.textColor
                            font.weight: Font.Medium
                            Layout.fillWidth: true
                        }
                        Label {
                            text: modelData.desc
                            color: Kirigami.Theme.disabledTextColor
                            Layout.fillWidth: true
                        }
                    }

                    Kirigami.Icon {
                        Layout.preferredWidth: 16
                        Layout.preferredHeight: 16
                        source: "arrow-right"
                        color: Kirigami.Theme.disabledTextColor
                    }
                }
            }
        }

        // Placeholder hint
        Label {
            text: qsTr("完整星图渲染将在后续版本实现")
            color: Kirigami.Theme.disabledTextColor
            Layout.alignment: Qt.AlignHCenter
            Layout.topMargin: Kirigami.Units.smallSpacing
        }
    }
}
