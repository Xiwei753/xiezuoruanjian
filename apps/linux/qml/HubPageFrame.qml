import QtQuick 2.15
import QtQuick.Layouts 1.15

Item {
    id: root
    property var dt: null
    readonly property int pageMargin: width >= 980 ? (dt ? dt.pageMarginWide : 40) : (dt ? dt.pageMarginNarrow : 24)
    property int maxContentWidth: dt ? dt.maxContentWidth : 1240
    readonly property int contentWidth: Math.min(Math.max(1, width - pageMargin * 2), maxContentWidth)
    default property alias contentData: contentColumn.data
    property alias headerData: headerContainer.data

    ColumnLayout {
        width: root.contentWidth
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.top: parent.top
        anchors.bottom: parent.bottom
        anchors.topMargin: root.pageMargin
        anchors.bottomMargin: dt ? dt.sp24 : 24
        spacing: dt ? dt.sp16 : 16

        Item {
            id: headerContainer
            Layout.fillWidth: true
            Layout.preferredHeight: dt ? dt.pageHeaderHeight : 72
        }

        ColumnLayout {
            id: contentColumn
            Layout.fillWidth: true
            Layout.fillHeight: true
            spacing: dt ? dt.sp16 : 16
        }
    }
}
