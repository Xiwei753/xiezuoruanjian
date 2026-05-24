import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15

Rectangle {
    id: root
    property var dt: null
    property var backendRef: null
    property var appState: ({})
    property var starmaps: []
    property string filterProjectId: ""

    signal openStarmap(string starmapId)
    signal openSettings()

    color: dt ? dt.bg : "#111318"

    function loadStarmaps() {
        if (!backendRef) return;
        try {
            var raw = backendRef.list_starmaps_json();
            starmaps = JSON.parse(raw) || [];
        } catch (e) {
            starmaps = [];
        }
        refreshGridModel();
    }

    function refreshGridModel() {
        var roots = getRootStarmaps();
        gridModel.clear();
        for (var i = 0; i < roots.length; i++) {
            gridModel.append({
                starmapObj: roots[i],
                childCount: roots[i].child_starmap_count || 0
            });
        }
    }

    function getRootStarmaps() {
        var roots = [];
        for (var i = 0; i < starmaps.length; i++) {
            if (!starmaps[i].parent_starmap_id) {
                if (filterProjectId === "" || starmaps[i].project_id === filterProjectId) {
                    roots.push(starmaps[i]);
                }
            }
        }
        return roots;
    }

    function getChildStarmaps(parentId) {
        var children = [];
        for (var i = 0; i < starmaps.length; i++) {
            if (starmaps[i].parent_starmap_id === parentId) {
                children.push(starmaps[i]);
            }
        }
        return children;
    }

    function getProjectTitle(projectId) {
        if (!projectId || !appState || !appState.tree) return "";
        for (var i = 0; i < appState.tree.length; i++) {
            if (appState.tree[i].id === projectId && appState.tree[i].type === "project") {
                return appState.tree[i].title;
            }
        }
        return "";
    }

    Component.onCompleted: loadStarmaps()
    onFilterProjectIdChanged: refreshGridModel()
    onStarmapsChanged: refreshGridModel()

    ColumnLayout {
        anchors.fill: parent
        anchors.leftMargin: root.width >= 980 ? (dt ? dt.pageMarginWide : 40) : (dt ? dt.pageMarginNarrow : 24)
        anchors.rightMargin: root.width >= 980 ? (dt ? dt.pageMarginWide : 40) : (dt ? dt.pageMarginNarrow : 24)
        anchors.topMargin: root.width >= 980 ? (dt ? dt.pageMarginWide : 40) : (dt ? dt.pageMarginNarrow : 24)
        anchors.bottomMargin: dt ? dt.sp24 : 24
        spacing: dt ? dt.sp16 : 16

        // Header
        RowLayout {
            Layout.fillWidth: true
            spacing: dt ? dt.sp16 : 16

            Column {
                Layout.fillWidth: true
                spacing: dt ? dt.sp6 : 6

                Text {
                    text: "星图"
                    color: dt ? dt.textPrimary : "#E2E4E9"
                    font.pixelSize: dt ? dt.fontTitle : 26
                    font.weight: Font.Bold
                }
                Text {
                    text: "构建你的创作宇宙，可视化人物关系与故事脉络"
                    color: dt ? dt.textSecondary : "#9CA0AB"
                    font.pixelSize: dt ? dt.fontMd : 14
                }
            }

            // Filter: show current project only
            Rectangle {
                width: filterRow.implicitWidth + (dt ? dt.sp16 : 16)
                height: 36
                radius: dt ? dt.radiusSm : 8
                color: filterProjectId !== "" ?
                       (dt ? dt.accentSoft : "rgba(123,140,222,0.12)") :
                       "transparent"
                border.color: dt ? dt.border : "#2A2E36"
                border.width: 1
                visible: false

                Row {
                    id: filterRow
                    anchors.centerIn: parent
                    spacing: dt ? dt.sp4 : 4
                    Text {
                        text: "\uD83D\uDCCB"
                        font.pixelSize: dt ? dt.fontSm : 12
                        anchors.verticalCenter: parent.verticalCenter
                    }
                    Text {
                        text: "当前作品"
                        color: filterProjectId !== "" ?
                               (dt ? dt.accentText : "#3D4D9E") :
                               (dt ? dt.textSecondary : "#5C6070")
                        font.pixelSize: dt ? dt.fontSm : 12
                        anchors.verticalCenter: parent.verticalCenter
                    }
                }

                MouseArea {
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    onClicked: filterProjectId = filterProjectId !== "" ? "" : (appState.selected ? appState.selected.projectId || "" : "")
                }
            }

            // Create starmap button
            Rectangle {
                width: createRow.implicitWidth + (dt ? dt.sp24 : 24)
                height: dt ? dt.actionButtonHeight : 40
                radius: dt ? dt.actionButtonRadius : 12
                color: createHover.containsMouse ? (dt ? dt.accentHover : "#8E9EE8") : (dt ? dt.accent : "#7B8CDE")

                Row {
                    id: createRow
                    anchors.centerIn: parent
                    spacing: dt ? dt.sp6 : 6
                    Text {
                        text: "+"
                        color: "#FFFFFF"
                        font.pixelSize: dt ? dt.fontLg : 16
                        font.weight: Font.Bold
                        anchors.verticalCenter: parent.verticalCenter
                    }
                    Text {
                        text: "新建星图"
                        color: "#FFFFFF"
                        font.pixelSize: dt ? dt.fontMd : 14
                        font.weight: Font.Medium
                        anchors.verticalCenter: parent.verticalCenter
                    }
                }

                MouseArea {
                    id: createHover
                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    onClicked: createStarmapDialog.open()
                }
            }
        }

        // Starmap grid
        ScrollView {
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            ScrollBar.horizontal.policy: ScrollBar.AlwaysOff

            GridView {
                id: grid
                width: parent ? parent.width : 0
                cellWidth: root.computeCellWidth()
                cellHeight: 260
                model: ListModel { id: gridModel }
                delegate: Item {
                    width: grid.cellWidth - (dt ? dt.gridGap : 16)
                    height: grid.cellHeight

                    Column {
                        anchors.fill: parent
                        anchors.margins: (dt ? dt.gridGap : 16) / 2
                        spacing: dt ? dt.sp8 : 8

                        StarMapCard {
                            dt: root.dt
                            starmapData: starmapObj
                            width: parent.width
                            height: parent.height - (childRepeater.count > 0 ? 60 : 0)
                            onClicked: function(starmapId) {
                                root.openStarmap(starmapId);
                            }
                            onMenuRequested: function(starmapId, title) {
                                starmapContextMenu.starmapId = starmapId;
                                starmapContextMenu.starmapTitle = title;
                                starmapContextMenu.popup();
                            }
                        }

                        // Child starmaps row
                        Flow {
                            width: parent.width
                            spacing: dt ? dt.sp4 : 4
                            visible: childCount > 0

                            Repeater {
                                id: childRepeater
                                model: {
                                    var children = [];
                                    for (var i = 0; i < root.starmaps.length; i++) {
                                        if (root.starmaps[i].parent_starmap_id === starmapObj.starmap_id) {
                                            children.push(root.starmaps[i]);
                                        }
                                    }
                                    return children;
                                }

                                Rectangle {
                                    width: Math.min(120, parent ? parent.width : 120)
                                    height: 28
                                    radius: dt ? dt.radiusSm : 8
                                    color: dt ? dt.cardHover : "#22262E"
                                    border.color: dt ? dt.border : "#2A2E36"
                                    border.width: 1

                                    Row {
                                        anchors.centerIn: parent
                                        anchors.leftMargin: dt ? dt.sp6 : 6
                                        anchors.rightMargin: dt ? dt.sp6 : 6
                                        spacing: dt ? dt.sp4 : 4

                                        Rectangle {
                                            width: 6; height: 6; radius: 3
                                            color: modelData.accent_color || (dt ? dt.accent : "#7B8CDE")
                                            anchors.verticalCenter: parent.verticalCenter
                                        }

                                        Text {
                                            text: modelData.title || ""
                                            color: dt ? dt.textSecondary : "#9CA0AB"
                                            font.pixelSize: dt ? dt.fontXs : 11
                                            anchors.verticalCenter: parent.verticalCenter
                                            elide: Text.ElideRight
                                            maximumLineCount: 1
                                        }
                                    }

                                    MouseArea {
                                        anchors.fill: parent
                                        cursorShape: Qt.PointingHandCursor
                                        onClicked: root.openStarmap(modelData.starmap_id || "")
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // Empty state
        Item {
            Layout.fillWidth: true
            Layout.fillHeight: true
            visible: gridModel.count === 0

            ColumnLayout {
                anchors.centerIn: parent
                spacing: dt ? dt.sp16 : 16

                Rectangle {
                    width: 80; height: 80
                    radius: dt ? dt.radiusCard : 18
                    color: dt ? dt.accentSoft : "rgba(123,140,222,0.12)"
                    Layout.alignment: Qt.AlignHCenter

                    Text {
                        anchors.centerIn: parent
                        text: "\u2B50"
                        font.pixelSize: 36
                    }
                }

                Text {
                    text: "还没有星图"
                    color: dt ? dt.textPrimary : "#E2E4E9"
                    font.pixelSize: dt ? dt.fontLg : 16
                    font.weight: Font.DemiBold
                    Layout.alignment: Qt.AlignHCenter
                }

                Text {
                    text: "创建你的第一个星图，构建角色关系与故事脉络"
                    color: dt ? dt.textMuted : "#606470"
                    font.pixelSize: dt ? dt.fontSm : 12
                    Layout.alignment: Qt.AlignHCenter
                }
            }
        }
    }

    // Refresh when visible
    onVisibleChanged: {
        if (visible) loadStarmaps();
    }

    function computeCellWidth() {
        var gap = dt ? dt.gridGap : 16;
        var w = root.width - ((root.width >= 980 ? (dt ? dt.pageMarginWide : 40) : (dt ? dt.pageMarginNarrow : 24)) * 2);
        var minCardWidth = 280;
        var cols = Math.max(1, Math.floor((w + gap) / (minCardWidth + gap)));
        return Math.floor((w - (cols - 1) * gap) / cols) + gap;
    }

    // Create starmap dialog
    Dialog {
        id: createStarmapDialog
        title: "新建星图"
        modal: true
        width: 360
        anchors.centerIn: Overlay.overlay
        background: Rectangle {
            color: dt ? dt.surface : "#1A1D23"
            border.color: dt ? dt.border : "#2A2E36"
            radius: dt ? dt.radiusMd : 12
            border.width: 1
        }

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: dt ? dt.sp24 : 24
            spacing: dt ? dt.sp16 : 16

            ColumnLayout {
                spacing: dt ? dt.sp4 : 4
                Layout.fillWidth: true

                Text {
                    text: "星图名称"
                    color: dt ? dt.textSecondary : "#9CA0AB"
                    font.pixelSize: dt ? dt.fontSm : 12
                }
                TextField {
                    id: starmapTitleField
                    Layout.fillWidth: true
                    placeholderText: "例如：人物关系图"
                    color: dt ? dt.textPrimary : "#E2E4E9"
                    font.pixelSize: dt ? dt.fontMd : 14
                    background: Rectangle {
                        color: dt ? dt.paper : "#191C21"
                        border.color: starmapTitleField.activeFocus ?
                                     (dt ? dt.accent : "#7B8CDE") :
                                     (dt ? dt.border : "#2A2E36")
                        border.width: 1
                        radius: dt ? dt.radiusSm : 8
                    }
                    selectByMouse: true
                }
            }

            ColumnLayout {
                spacing: dt ? dt.sp4 : 4
                Layout.fillWidth: true

                Text {
                    text: "描述（可选）"
                    color: dt ? dt.textSecondary : "#9CA0AB"
                    font.pixelSize: dt ? dt.fontSm : 12
                }
                TextField {
                    id: starmapDescField
                    Layout.fillWidth: true
                    placeholderText: "简要描述这个星图的内容"
                    color: dt ? dt.textPrimary : "#E2E4E9"
                    font.pixelSize: dt ? dt.fontMd : 14
                    background: Rectangle {
                        color: dt ? dt.paper : "#191C21"
                        border.color: starmapDescField.activeFocus ?
                                     (dt ? dt.accent : "#7B8CDE") :
                                     (dt ? dt.border : "#2A2E36")
                        border.width: 1
                        radius: dt ? dt.radiusSm : 8
                    }
                    selectByMouse: true
                }
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: dt ? dt.sp8 : 8
                Layout.alignment: Qt.AlignRight

                Rectangle {
                    width: cancelRow.implicitWidth + (dt ? dt.sp16 : 16)
                    height: 36
                    radius: dt ? dt.radiusSm : 8
                    color: cancelHover.containsMouse ? (dt ? dt.cardHover : "#22262E") : "transparent"
                    border.color: dt ? dt.border : "#2A2E36"
                    border.width: 1

                    Row {
                        id: cancelRow
                        anchors.centerIn: parent
                        Text {
                            text: "取消"
                            color: dt ? dt.textSecondary : "#9CA0AB"
                            font.pixelSize: dt ? dt.fontMd : 14
                        }
                    }
                    MouseArea {
                        id: cancelHover
                        anchors.fill: parent
                        hoverEnabled: true
                        cursorShape: Qt.PointingHandCursor
                        onClicked: createStarmapDialog.close()
                    }
                }

                Rectangle {
                    width: confirmRow.implicitWidth + (dt ? dt.sp16 : 16)
                    height: 36
                    radius: dt ? dt.radiusSm : 8
                    color: confirmHover.containsMouse ? (dt ? dt.accentHover : "#8E9EE8") : (dt ? dt.accent : "#7B8CDE")

                    Row {
                        id: confirmRow
                        anchors.centerIn: parent
                        spacing: dt ? dt.sp4 : 4
                        Text {
                            text: "\u2B50"
                            font.pixelSize: dt ? dt.fontSm : 12
                            anchors.verticalCenter: parent.verticalCenter
                        }
                        Text {
                            text: "创建"
                            color: "#FFFFFF"
                            font.pixelSize: dt ? dt.fontMd : 14
                            font.weight: Font.Medium
                            anchors.verticalCenter: parent.verticalCenter
                        }
                    }

                    MouseArea {
                        id: confirmHover
                        anchors.fill: parent
                        hoverEnabled: true
                        cursorShape: Qt.PointingHandCursor
                        onClicked: {
                            var title = starmapTitleField.text.trim();
                            if (title === "") return;
                            var desc = starmapDescField.text.trim();
                            var res = backendRef.create_starmap_json(title, desc, "");
                            try {
                                var obj = JSON.parse(res);
                                if (obj.success === false) return;
                            } catch (e) {}
                            starmapTitleField.text = "";
                            starmapDescField.text = "";
                            createStarmapDialog.close();
                            loadStarmaps();
                        }
                    }
                }
            }
        }

        onOpened: {
            starmapTitleField.text = "";
            starmapDescField.text = "";
            starmapTitleField.forceActiveFocus();
        }
    }

    // Context menu
    Menu {
        id: starmapContextMenu
        property string starmapId: ""
        property string starmapTitle: ""

        MenuItem {
            text: "打开"
            onTriggered: root.openStarmap(starmapContextMenu.starmapId)
        }
        MenuItem {
            text: "新建子星图"
            onTriggered: {
                createChildStarmapDialog.parentId = starmapContextMenu.starmapId;
                createChildStarmapDialog.open();
            }
        }
        MenuSeparator {}
        MenuItem {
            text: "重命名"
            onTriggered: {
                renameStarmapDialog.starmapId = starmapContextMenu.starmapId;
                renameStarmapDialog.currentTitle = starmapContextMenu.starmapTitle;
                renameStarmapDialog.open();
            }
        }
        MenuItem {
            text: "删除"
            onTriggered: {
                var res = backendRef.delete_starmap_json(starmapContextMenu.starmapId);
                loadStarmaps();
            }
        }
    }

    // Rename dialog
    Dialog {
        id: renameStarmapDialog
        property string starmapId: ""
        property string currentTitle: ""
        title: "重命名星图"
        modal: true
        width: 320
        anchors.centerIn: Overlay.overlay
        background: Rectangle {
            color: dt ? dt.surface : "#1A1D23"
            border.color: dt ? dt.border : "#2A2E36"
            radius: dt ? dt.radiusMd : 12
            border.width: 1
        }
        ColumnLayout {
            anchors.fill: parent
            anchors.margins: dt ? dt.sp20 : 20
            spacing: dt ? dt.sp12 : 12
            TextField {
                id: renameField
                Layout.fillWidth: true
                text: renameStarmapDialog.currentTitle
                color: dt ? dt.textPrimary : "#E2E4E9"
                font.pixelSize: dt ? dt.fontMd : 14
                background: Rectangle {
                    color: dt ? dt.paper : "#191C21"
                    border.color: dt ? dt.border : "#2A2E36"
                    border.width: 1
                    radius: dt ? dt.radiusSm : 8
                }
                selectByMouse: true
            }
            Button {
                text: "确定"
                Layout.alignment: Qt.AlignRight
                onClicked: {
                    var newTitle = renameField.text.trim();
                    if (newTitle === "") return;
                    backendRef.rename_starmap_json(renameStarmapDialog.starmapId, newTitle);
                    renameStarmapDialog.close();
                    loadStarmaps();
                }
            }
        }
        onOpened: {
            renameField.text = currentTitle;
            renameField.forceActiveFocus();
        }
    }

    // Create child starmap dialog
    Dialog {
        id: createChildStarmapDialog
        property string parentId: ""
        title: "新建子星图"
        modal: true
        width: 320
        anchors.centerIn: Overlay.overlay
        background: Rectangle {
            color: dt ? dt.surface : "#1A1D23"
            border.color: dt ? dt.border : "#2A2E36"
            radius: dt ? dt.radiusMd : 12
            border.width: 1
        }
        ColumnLayout {
            anchors.fill: parent
            anchors.margins: dt ? dt.sp20 : 20
            spacing: dt ? dt.sp12 : 12
            TextField {
                id: childTitleField
                Layout.fillWidth: true
                placeholderText: "子星图名称"
                color: dt ? dt.textPrimary : "#E2E4E9"
                font.pixelSize: dt ? dt.fontMd : 14
                background: Rectangle {
                    color: dt ? dt.paper : "#191C21"
                    border.color: dt ? dt.border : "#2A2E36"
                    border.width: 1
                    radius: dt ? dt.radiusSm : 8
                }
                selectByMouse: true
            }
            Button {
                text: "创建"
                Layout.alignment: Qt.AlignRight
                onClicked: {
                    var title = childTitleField.text.trim();
                    if (title === "") return;
                    backendRef.create_child_starmap_json(createChildStarmapDialog.parentId, title, "", "");
                    createChildStarmapDialog.close();
                    loadStarmaps();
                }
            }
        }
        onOpened: {
            childTitleField.text = "";
            childTitleField.forceActiveFocus();
        }
    }
}
