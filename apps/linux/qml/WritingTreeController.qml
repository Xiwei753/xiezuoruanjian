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
