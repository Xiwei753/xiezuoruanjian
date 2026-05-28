// =============================================================================
// StarMapPreviewPage.qml — 星图预览页面
// =============================================================================
//
// 层级：Linux UI 层（QML 页面）
// 职责：在右侧抽屉中展示星图的缩略预览
// 约束：
//   - 纯展示组件，数据通过 backendRef 获取
//   - 点击后跳转到完整的星图编辑页面
// =============================================================================

import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15

Rectangle {
    id: root
    property var dt: null
    property var backendRef: null
    property var appState: ({})

    color: dt ? dt.bg : "#111318"

    ColumnLayout {
        anchors.centerIn: parent
        spacing: dt ? dt.sp24 : 24
        width: Math.min(parent.width - 80, 560)

        // Constellation icon
        Rectangle {
            Layout.alignment: Qt.AlignHCenter
            width: 80; height: 80
            radius: dt ? dt.radiusCard : 18
            color: dt ? dt.accentSoft : "rgba(123,140,222,0.12)"

            Text {
                anchors.centerIn: parent
                text: "\u2B50"
                font.pixelSize: 36
            }
        }

        Text {
            text: "星图"
            color: dt ? dt.textPrimary : "#E2E4E9"
            font.pixelSize: dt ? dt.fontTitle : 26
            font.weight: Font.Bold
            Layout.alignment: Qt.AlignHCenter
        }

        Text {
            text: "构建你的创作宇宙，可视化人物关系与故事脉络"
            color: dt ? dt.textSecondary : "#9CA0AB"
            font.pixelSize: dt ? dt.fontMd : 14
            horizontalAlignment: Text.AlignHCenter
            Layout.fillWidth: true
            wrapMode: Text.Wrap
        }

        Item { Layout.preferredHeight: dt ? dt.sp16 : 16 }

        // Feature cards
        Repeater {
            model: ListModel {
                ListElement { label: "作品宇宙"; desc: "整体世界观与核心设定"; icon: "\uD83C\uDF0F" }
                ListElement { label: "人物关系"; desc: "角色之间的关联与冲突"; icon: "\uD83D\uDC65" }
                ListElement { label: "地点"; desc: "故事发生的场景与空间"; icon: "\uD83D\uDDFA" }
                ListElement { label: "事件"; desc: "推动剧情的关键事件链"; icon: "\u26A1" }
                ListElement { label: "伏笔"; desc: "埋设与回收的叙事线索"; icon: "\uD83D\uDD0D" }
            }

            Rectangle {
                Layout.fillWidth: true
                Layout.preferredHeight: 64
                radius: dt ? dt.radiusMd : 12
                color: dt ? dt.card : "#1E2128"

                RowLayout {
                    anchors.fill: parent
                    anchors.margins: dt ? dt.sp16 : 16
                    spacing: dt ? dt.sp12 : 12

                    Rectangle {
                        width: 40; height: 40
                        radius: dt ? dt.radiusSm : 8
                        color: dt ? dt.accentSoft : "rgba(123,140,222,0.12)"

                        Text {
                            anchors.centerIn: parent
                            text: model.icon
                            font.pixelSize: 18
                        }
                    }

                    Column {
                        Layout.fillWidth: true
                        spacing: 2
                        Text {
                            text: model.label
                            color: dt ? dt.textPrimary : "#E2E4E9"
                            font.pixelSize: dt ? dt.fontMd : 14
                            font.weight: Font.Medium
                        }
                        Text {
                            text: model.desc
                            color: dt ? dt.textMuted : "#606470"
                            font.pixelSize: dt ? dt.fontXs : 11
                        }
                    }

                    Text {
                        text: "\u2192"
                        color: dt ? dt.textMuted : "#606470"
                        font.pixelSize: dt ? dt.fontLg : 16
                    }
                }
            }
        }

        // Placeholder hint
        Text {
            text: "完整星图渲染将在后续版本实现"
            color: dt ? dt.textMuted : "#606470"
            font.pixelSize: dt ? dt.fontXs : 11
            Layout.alignment: Qt.AlignHCenter
            Layout.topMargin: dt ? dt.sp8 : 8
        }
    }
}
