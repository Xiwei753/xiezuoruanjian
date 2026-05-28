// =============================================================================
// WritingTreeController.qml — 写作树控制器
// =============================================================================
//
// 层级：Linux UI 层（QML 逻辑控制器）
// 职责：将 flat tree 数组转换为层级列表，供 WorkspaceTree 渲染
// 约束：
//   - 纯数据转换，不包含 UI 渲染
//   - 不直接操作文件系统或 Core 层
//   - 输出 items 数组供 WorkspaceTree 绑定
// =============================================================================

import QtQuick 2.15

QtObject {
    id: controller

    property var tree: []
    property string projectId: ""
    property var items: []

    function rebuild() {
        var next = [];
        if (!tree || !projectId) {
            items = next;
            return;
        }

        for (var i = 0; i < tree.length; i++) {
            var project = tree[i];
            if (project.type !== "project" || project.id !== projectId) continue;
            next.push(normalize(project, project.id, ""));

            for (var j = 0; j < tree.length; j++) {
                var volume = tree[j];
                if (volume.type !== "volume" || volume.projectId !== project.id) continue;
                next.push(normalize(volume, volume.projectId, volume.id));

                for (var k = 0; k < tree.length; k++) {
                    var chapter = tree[k];
                    if (chapter.type === "chapter" && chapter.projectId === volume.projectId && chapter.volumeId === volume.id) {
                        next.push(normalize(chapter, chapter.projectId, chapter.volumeId));
                    }
                }
            }
            break;
        }
        items = next;
    }

    function normalize(item, projectIdValue, volumeIdValue) {
        return {
            id: item.id || "",
            type: item.type || "",
            title: item.title || "",
            projectId: projectIdValue || "",
            volumeId: volumeIdValue || ""
        };
    }

    onTreeChanged: rebuild()
    onProjectIdChanged: rebuild()
}
