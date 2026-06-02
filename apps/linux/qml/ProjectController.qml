// =============================================================================
// ProjectController.qml — 项目树操作控制器
// =============================================================================

import QtQuick

QtObject {
    id: controller

    property var backendRef: null
    property var projectBackendRef: null
    property var appController: null

    function projectApi() {
        return projectBackendRef || backendRef;
    }

    function createProject(title) {
        var api = projectApi();
        if (!api || !appController) return false;
        try {
            return appController.handleMutationResult(api.create_project_json(title, appController.generateActionId()), qsTr("创建作品失败"));
        } catch (e) {
            appController.emitError(qsTr("后端调用失败: ") + e);
            return false;
        }
    }

    function renameProject(projectId, title) {
        var api = projectApi();
        if (!api || !appController || !projectId || !title) return false;
        try {
            return appController.handleMutationResult(api.rename_project_json(projectId, title), qsTr("重命名作品失败"));
        } catch (e) {
            appController.emitError(qsTr("后端调用失败: ") + e);
            return false;
        }
    }

    function renameVolume(projectId, volumeId, title) {
        var api = projectApi();
        if (!api || !appController || !projectId || !volumeId || !title) return false;
        try {
            return appController.handleMutationResult(api.rename_volume_json(projectId, volumeId, title), qsTr("重命名卷失败"));
        } catch (e) {
            appController.emitError(qsTr("后端调用失败: ") + e);
            return false;
        }
    }

    function renameChapter(projectId, volumeId, chapterId, title) {
        var api = projectApi();
        if (!api || !appController || !projectId || !volumeId || !chapterId || !title) return false;
        try {
            return appController.handleMutationResult(api.rename_chapter_json(projectId, volumeId, chapterId, title), qsTr("重命名章节失败"));
        } catch (e) {
            appController.emitError(qsTr("后端调用失败: ") + e);
            return false;
        }
    }

    function createVolume(projectId, title) {
        var api = projectApi();
        if (!api || !appController) return false;
        try {
            return appController.handleMutationResult(api.create_volume_json(projectId, title, appController.generateActionId()), qsTr("创建卷失败"));
        } catch (e) {
            appController.emitError(qsTr("后端调用失败: ") + e);
            return false;
        }
    }

    function createChapter(projectId, volumeId, title) {
        var api = projectApi();
        if (!api || !appController) return false;
        try {
            return appController.handleMutationResult(api.create_chapter_json(projectId, volumeId, title, appController.generateActionId()), qsTr("创建章节失败"));
        } catch (e) {
            appController.emitError(qsTr("后端调用失败: ") + e);
            return false;
        }
    }

    function deleteItem(type, contextData) {
        var api = projectApi();
        if (!api || !appController) return false;
        var actionId = appController.generateActionId();
        var raw = "";
        try {
            if (type === "delete_project") raw = api.delete_project_json(contextData.projectId, actionId);
            else if (type === "delete_volume") raw = api.delete_volume_json(contextData.projectId, contextData.volumeId, actionId);
            else if (type === "delete_chapter") raw = api.delete_chapter_json(contextData.projectId, contextData.volumeId, contextData.chapterId, actionId);
            else return false;
            return appController.handleMutationResult(raw, qsTr("删除失败"));
        } catch (e) {
            appController.emitError(qsTr("后端调用失败: ") + e);
            return false;
        }
    }
}
