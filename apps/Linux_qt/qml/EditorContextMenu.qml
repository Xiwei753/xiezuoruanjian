// =============================================================================
// EditorContextMenu.qml - 编辑器右键/长按菜单
// =============================================================================
//
// 职责：Linux_qt UI 层（QML 组件）
// 边界：只负责菜单弹出和命令分发（不包含选区逻辑和业务判断）
// 约束：
//   - 菜单项文本固定，不依赖外部数据源
//   - 菜单项状态绑定 SujianEditorItem 属性
//   - 不包含业务逻辑，不直接调用 Core
//
// Issue #900：外观交给 Breeze QQC2 样式（去掉自绘背景 / AppText contentItem），
// 菜单项换成 Breeze 图标。editorItem / dt 属性签名保持不变。
// =============================================================================

import QtQuick
import QtQuick.Controls

Menu {
    id: editorContextMenu

    property var editorItem: null
    required property var dt

    MenuItem {
        text: qsTr("复制")
        icon.name: "edit-copy"
        enabled: editorContextMenu.editorItem && editorContextMenu.editorItem.has_selection
        onTriggered: {
            if (editorContextMenu.editorItem)
                editorContextMenu.editorItem.clipboard_copy()
        }
    }

    MenuItem {
        text: qsTr("粘贴")
        icon.name: "edit-paste"
        enabled: editorContextMenu.editorItem && editorContextMenu.editorItem.editor_enabled
        onTriggered: {
            if (editorContextMenu.editorItem)
                editorContextMenu.editorItem.clipboard_paste()
        }
    }

    MenuItem {
        text: qsTr("全选")
        icon.name: "edit-select-all"
        enabled: editorContextMenu.editorItem && editorContextMenu.editorItem.editor_enabled
        onTriggered: {
            if (editorContextMenu.editorItem)
                editorContextMenu.editorItem.select_all()
        }
    }

    MenuItem {
        text: qsTr("删除")
        icon.name: "edit-delete"
        enabled: editorContextMenu.editorItem && editorContextMenu.editorItem.has_selection
        onTriggered: {
            if (editorContextMenu.editorItem)
                editorContextMenu.editorItem.delete_selection()
        }
    }
}
