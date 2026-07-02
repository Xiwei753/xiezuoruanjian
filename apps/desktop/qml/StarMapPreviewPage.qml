// =============================================================================
// StarMapPreviewPage.qml — 星图预览页面
// =============================================================================
//
// 层级：Desktop UI 层（QML 页面）
// 职责：在右侧抽屉中展示星图的缩略预览
// 约束：
//   - 纯展示组件，数据通过 backendRef 获取
//   - 点击后跳转到完整的星图编辑页面
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Rectangle {
    id: root
    property var dt: null
    property var backendRef: null
    property var appState: ({})

    color: dt.bg

    ColumnLayout {
        anchors.centerIn: parent
        spacing: dt.sp24
        width: Math.min(parent.width - 80, 560)

        // Constellation icon
        Rectangle {
            Layout.alignment: Qt.AlignHCenter
            width: 80; height: 80
            radius: dt.radiusCard
            color: dt.accentSoft

            AppText {
                anchors.centerIn: parent
                text: "\u2B50"
                dt: root.dt
                font.pixelSize: 36
            }
        }

        AppText {
            dt: root.dt
            text: qsTr("星图")
            color: dt.textPrimary
            font.pixelSize: dt.fontTitle
            font.weight: Font.Bold
            Layout.alignment: Qt.AlignHCenter
        }

        AppText {
            dt: root.dt
            text: qsTr("构建你的创作宇宙，可视化人物关系与故事脉络")
            color: dt.textSecondary
            font.pixelSize: dt.fontMd
            horizontalAlignment: Text.AlignHCenter
            Layout.fillWidth: true
            wrapMode: Text.Wrap
        }

        Item { Layout.preferredHeight: dt.sp16 }

        // Feature cards
        Repeater {
            model: [
                { label: qsTr("作品宇宙"), desc: qsTr("整体世界观与核心设定"), icon: "" },
                { label: qsTr("人物关系"), desc: qsTr("角色之间的关联与冲突"), icon: "" },
                { label: qsTr("地点"), desc: qsTr("故事发生的场景与空间"), icon: "" },
                { label: qsTr("事件"), desc: qsTr("推动剧情的关键事件链"), icon: "" },
                { label: qsTr("伏笔"), desc: qsTr("埋设与回收的叙事线索"), icon: "" }
            ]

            Rectangle {
                Layout.fillWidth: true
                Layout.preferredHeight: 64
                radius: dt.radiusMd
                color: dt.card

                RowLayout {
                    anchors.fill: parent
                    anchors.margins: dt.sp16
                    spacing: dt.sp12

                    Rectangle {
                        width: 40; height: 40
                        radius: dt.radiusSm
                        color: dt.accentSoft

                        AppText {
                            dt: root.dt
                            anchors.centerIn: parent
                            text: modelData.icon
                            font.pixelSize: 18
                        }
                    }

                    Column {
                        Layout.fillWidth: true
                        spacing: 2
                        AppText {
                            dt: root.dt
                            text: modelData.label
                            color: dt.textPrimary
                            font.pixelSize: dt.fontMd
                            font.weight: Font.Medium
                        }
                        AppText {
                            dt: root.dt
                            text: modelData.desc
                            color: dt.textMuted
                            font.pixelSize: dt.fontXs
                        }
                    }

                    AppText {
                        dt: root.dt
                        text: "\u2192"
                        color: dt.textMuted
                        font.pixelSize: dt.fontLg
                    }
                }
            }
        }

        // Placeholder hint
        AppText {
            dt: root.dt
            text: qsTr("完整星图渲染将在后续版本实现")
            color: dt.textMuted
            font.pixelSize: dt.fontXs
            Layout.alignment: Qt.AlignHCenter
            Layout.topMargin: dt.sp8
        }
    }
}
